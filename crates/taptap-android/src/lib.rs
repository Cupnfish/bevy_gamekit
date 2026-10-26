//! TapTap SDK 的 Android 平台实现。
//!
//! 架构（JNI 面只传 String/数值，复杂对象在两侧以 JSON 表达）：
//!
//! ```text
//! Rust API（本 crate）
//!   └─ JNI 静态调用 cn.taptap.demo.TapTapShim（Kotlin，随宿主 Gradle 工程编译）
//!        └─ 官方 TapSDK AAR（com.taptap.sdk:tap-*）
//!             └─ 回调（主线程）→ TapTapShim 的 external fun
//!                  └─ JNI 自动绑定回本 crate 的 Java_cn_taptap_demo_TapTapShim_* 符号
//!                       └─ 推入事件通道，poll() 由上层轮询
//! ```
//!
//! 依赖前提：Android 侧由 `android-activity`（Bevy 的 native-activity 特性）注入
//! `ndk-context`；shim 类与 TapSDK 依赖在 `android/` Gradle 工程里配置。

#[cfg(target_os = "android")]
use taptap_core::{CloudSaveResult, TapAccount, TapTapConfig, TapTapEvent};
#[cfg(not(target_os = "android"))]
use taptap_core::{TapAccount, TapTapConfig, TapTapEvent};

/// JNI 桥调用的 Kotlin shim 类（对应 android/ 工程中的 TapTapShim.kt）。
pub const SHIM_CLASS: &str = "cn/taptap/demo/TapTapShim";

#[derive(Debug)]
pub enum TapTapError {
    /// 在非 Android 环境调用了 Android 实现。
    NotAndroid,
    /// ndk-context 尚未就绪（AndroidApp 未初始化）。
    NoContext,
    /// JNI 调用失败。
    Jni(String),
}

impl std::fmt::Display for TapTapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TapTapError::NotAndroid => write!(f, "TapTap Android 实现只能在 Android 上调用"),
            TapTapError::NoContext => write!(f, "ndk-context 未初始化"),
            TapTapError::Jni(e) => write!(f, "JNI 错误: {e}"),
        }
    }
}

impl std::error::Error for TapTapError {}

#[cfg(target_os = "android")]
mod platform {
    use super::*;
    use jni::{
        objects::{JClass, JObject, JString, JValue},
        sys::jboolean,
        JNIEnv, JavaVM,
    };
    use std::sync::mpsc::{channel, Receiver, Sender};
    use std::sync::{Mutex, OnceLock};
    use taptap_core::{InitResult, LoginResult};

    static VM: OnceLock<JavaVM> = OnceLock::new();
    static EVENTS: OnceLock<Mutex<(Sender<TapTapEvent>, Receiver<TapTapEvent>)>> = OnceLock::new();
    /// shim 类的 GlobalRef 缓存。JNI attach 的线程默认持有系统 ClassLoader，
    /// `find_class` 找不到应用 dex 里的类，必须经 Activity 的 ClassLoader 加载一次后缓存。
    static SHIM: OnceLock<jni::objects::GlobalRef> = OnceLock::new();

    fn shim_class(env: &mut JNIEnv) -> Result<&'static jni::objects::GlobalRef, TapTapError> {
        if let Some(g) = SHIM.get() {
            return Ok(g);
        }
        let activity = activity()?;
        let loader = env
            .call_method(&activity, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
            .map_err(|e| jerr(env, e))?
            .l()
            .map_err(|e| jerr(env, e))?;
        let name = env
            .new_string(SHIM_CLASS)
            .map_err(|e| jerr(env, e))?;
        let class = env
            .call_method(
                &loader,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::Object(&name)],
            )
            .map_err(|e| jerr(env, e))?
            .l()
            .map_err(|e| jerr(env, e))?;
        let global = env.new_global_ref(&class).map_err(|e| jerr(env, e))?;
        // libdemo.so 由框架 NativeActivity（boot classloader）加载，应用类的
        // native 方法无法通过 ClassLoader 关联解析到它，必须显式 RegisterNatives。
        let methods = [
            jni::NativeMethod {
                name: "nativeOnInitResult".into(),
                sig: "(ZLjava/lang/String;)V".into(),
                fn_ptr: Java_cn_taptap_demo_TapTapShim_nativeOnInitResult as *mut _,
            },
            jni::NativeMethod {
                name: "nativeOnLoginResult".into(),
                sig: "(ILjava/lang/String;)V".into(),
                fn_ptr: Java_cn_taptap_demo_TapTapShim_nativeOnLoginResult as *mut _,
            },
            jni::NativeMethod {
                name: "nativeOnLogout".into(),
                sig: "()V".into(),
                fn_ptr: Java_cn_taptap_demo_TapTapShim_nativeOnLogout as *mut _,
            },
            jni::NativeMethod {
                name: "nativeOnCloudSaveResult".into(),
                sig: "(IILjava/lang/String;)V".into(),
                fn_ptr: Java_cn_taptap_demo_TapTapShim_nativeOnCloudSaveResult as *mut _,
            },
        ];
        env.register_native_methods(&global, &methods)
            .map_err(|e| jerr(env, e))?;
        let _ = SHIM.set(global);
        SHIM.get()
            .ok_or_else(|| TapTapError::Jni("shim 类缓存失败".into()))
    }

    fn events() -> &'static Mutex<(Sender<TapTapEvent>, Receiver<TapTapEvent>)> {
        EVENTS.get_or_init(|| Mutex::new(channel()))
    }

    pub(crate) fn emit(event: TapTapEvent) {
        if let Ok(pair) = events().lock() {
            let _ = pair.0.send(event);
        }
    }

    /// 取出并清空积累的平台回调事件，供上层（如 bevy_taptap）每帧轮询。
    pub fn poll() -> Vec<TapTapEvent> {
        let mut out = Vec::new();
        if let Ok(pair) = events().lock() {
            while let Ok(e) = pair.1.try_recv() {
                out.push(e);
            }
        }
        out
    }

    fn vm() -> Result<&'static JavaVM, TapTapError> {
        if let Some(vm) = VM.get() {
            return Ok(vm);
        }
        let raw = ndk_context::android_context().vm();
        if raw.is_null() {
            return Err(TapTapError::NoContext);
        }
        let vm = unsafe { JavaVM::from_raw(raw.cast()) }
            .map_err(|e| TapTapError::Jni(e.to_string()))?;
        let _ = VM.set(vm);
        VM.get().ok_or(TapTapError::NoContext)
    }

    fn activity() -> Result<JObject<'static>, TapTapError> {
        let raw = ndk_context::android_context().context();
        if raw.is_null() {
            return Err(TapTapError::NoContext);
        }
        Ok(unsafe { JObject::from_raw(raw.cast()) })
    }

    fn with_env<T>(
        f: impl FnOnce(&mut JNIEnv) -> Result<T, TapTapError>,
    ) -> Result<T, TapTapError> {
        let vm = vm()?;
        let mut env = vm
            .attach_current_thread()
            .map_err(|e| TapTapError::Jni(e.to_string()))?;
        f(&mut env)
    }

    fn err<E: std::fmt::Display>(e: E) -> TapTapError {
        TapTapError::Jni(e.to_string())
    }

    /// JNI 调用失败时，把挂起的 Java 异常读出来拼进错误（否则异常被吞，无法定位）。
    /// `exception_describe` 会把完整 Java 堆栈打到 stderr（Android 上进 logcat）。
    fn jerr(env: &mut JNIEnv, e: jni::errors::Error) -> TapTapError {
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_describe();
            let desc = env
                .exception_occurred()
                .ok()
                .filter(|t| !t.is_null())
                .and_then(|t| {
                    let msg = env
                        .call_method(&t, "toString", "()Ljava/lang/String;", &[])
                        .ok()
                        .and_then(|v| v.l().ok())
                        .map(|s| {
                            let jstr = JString::from(s);
                            env.get_string(&jstr)
                                .map(|js| js.to_string_lossy().to_string())
                                .unwrap_or_default()
                        })
                        .unwrap_or_default();
                    env.exception_clear().ok();
                    if msg.is_empty() { None } else { Some(msg) }
                })
                .unwrap_or_default();
            return TapTapError::Jni(format!("{e}; Java 异常: {desc}"));
        }
        TapTapError::Jni(e.to_string())
    }

    /// 触发统一初始化（异步）。结果经 [`poll`] 的 [`TapTapEvent::Init`] 返回。
    pub fn init(config: &TapTapConfig) -> Result<(), TapTapError> {
        let json = serde_json::to_string(config).map_err(|e| TapTapError::Jni(e.to_string()))?;
        with_env(|env| {
            let shim = shim_class(env)?;
            // android-activity 注入的 context 即当前 Activity（NativeActivity）
            let activity = activity()?;
            let jconfig = env.new_string(json).map_err(|e| jerr(env, e))?;
            env.call_static_method(
                shim,
                "init",
                "(Landroid/content/Context;Ljava/lang/String;)V",
                &[JValue::Object(&activity), JValue::Object(&jconfig)],
            )
            .map_err(|e| jerr(env, e))?;
            Ok(())
        })
    }

    /// 触发登录（异步，会拉起 TapTap 授权页）。结果经 [`TapTapEvent::Login`] 返回。
    pub fn login(scopes: &[&str]) -> Result<(), TapTapError> {
        let json = serde_json::to_string(scopes).map_err(|e| TapTapError::Jni(e.to_string()))?;
        with_env(|env| {
            let shim = shim_class(env)?;
            // ndk-context 给的是 Application；shim 内部用生命周期跟踪到的
            // resumed Activity 真正发起登录。
            let context = activity()?;
            let jscopes = env.new_string(json).map_err(|e| jerr(env, e))?;
            env.call_static_method(
                shim,
                "login",
                "(Landroid/content/Context;Ljava/lang/String;)V",
                &[JValue::Object(&context), JValue::Object(&jscopes)],
            )
            .map_err(|e| jerr(env, e))?;
            Ok(())
        })
    }

    /// 登出（清本地缓存）。完成经 [`TapTapEvent::Logout`] 返回。
    pub fn logout() -> Result<(), TapTapError> {
        with_env(|env| {
            let shim = shim_class(env)?;
            env.call_static_method(shim, "logout", "()V", &[])
                .map_err(|e| jerr(env, e))?;
            Ok(())
        })
    }

    /// 同步读取本地缓存的登录账号（可能为 None，不代表 token 有效）。
    pub fn current_account() -> Result<Option<TapAccount>, TapTapError> {
        with_env(|env| {
            let shim = shim_class(env)?;
            let ret = env
                .call_static_method(shim, "currentAccount", "()Ljava/lang/String;", &[])
                .map_err(|e| jerr(env, e))?;
            let obj = ret.l().map_err(err)?;
            if obj.is_null() {
                return Ok(None);
            }
            let jstr = JString::from(obj);
            let s: String = env.get_string(&jstr).map_err(err)?.into();
            if s.is_empty() {
                return Ok(None);
            }
            serde_json::from_str(&s)
                .map(Some)
                .map_err(|e| TapTapError::Jni(format!("解析账号 JSON 失败: {e}")))
        })
    }

    // ---- JNI 回调入口（由 TapTapShim.kt 的 external fun 自动绑定） ----

    #[no_mangle]
    pub extern "system" fn Java_cn_taptap_demo_TapTapShim_nativeOnInitResult<'local>(
        mut env: JNIEnv<'local>,
        _class: JClass<'local>,
        success: jboolean,
        info: JString<'local>,
    ) {
        let info: String = env
            .get_string(&info)
            .map(|s| s.into())
            .unwrap_or_default();
        let result = if success != 0 {
            InitResult { success: true, error_code: None, error_message: None }
        } else {
            let (code, msg) = info
                .split_once(':')
                .map_or((None, info.as_str()), |(c, m)| (c.parse::<i32>().ok(), m));
            InitResult {
                success: false,
                error_code: code,
                error_message: Some(msg.to_string()),
            }
        };
        emit(TapTapEvent::Init(result));
    }

    #[no_mangle]
    pub extern "system" fn Java_cn_taptap_demo_TapTapShim_nativeOnLoginResult<'local>(
        mut env: JNIEnv<'local>,
        _class: JClass<'local>,
        status: i32,
        payload: JString<'local>,
    ) {
        let payload: String = env
            .get_string(&payload)
            .map(|s| s.into())
            .unwrap_or_default();
        let account: Option<TapAccount> = if payload.is_empty() {
            None
        } else {
            serde_json::from_str(&payload).ok()
        };
        let error = if status == 1 {
            Some(account.as_ref().map_or_else(
                || payload.clone(),
                |_| payload.clone(),
            ))
        } else {
            None
        };
        emit(TapTapEvent::Login(LoginResult { status: status as u32, account, error }));
    }

    #[no_mangle]
    pub extern "system" fn Java_cn_taptap_demo_TapTapShim_nativeOnLogout(
        _env: JNIEnv,
        _class: JClass,
    ) {
        emit(TapTapEvent::Logout);
    }

    /// 云存档回调统一入口。kind：1=单档元信息 2=列表 3=内容 4=错误。
    #[no_mangle]
    pub extern "system" fn Java_cn_taptap_demo_TapTapShim_nativeOnCloudSaveResult<'local>(
        mut env: JNIEnv<'local>,
        _class: JClass<'local>,
        callback_id: i32,
        kind: i32,
        payload: JString<'local>,
    ) {
        let _ = callback_id;
        let payload: String = env
            .get_string(&payload)
            .map(|s| s.into())
            .unwrap_or_default();
        let parsed: serde_json::Value = serde_json::from_str(&payload).unwrap_or_default();

        let result = match kind {
            1 => CloudSaveResult {
                op: String::new(),
                success: true,
                error: None,
                archive: Some(parsed),
                archives: None,
                content: None,
            },
            2 => CloudSaveResult {
                op: "list".into(),
                success: true,
                error: None,
                archive: None,
                archives: Some(
                    parsed
                        .as_array()
                        .cloned()
                        .unwrap_or_default(),
                ),
                content: None,
            },
            3 => CloudSaveResult {
                op: "read".into(),
                success: true,
                error: None,
                archive: None,
                archives: None,
                content: parsed
                    .get("content")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
            },
            4 => CloudSaveResult {
                op: String::new(),
                success: false,
                error: Some(
                    parsed
                        .get("message")
                        .and_then(|v| v.as_str())
                        .map(|m| {
                            let code = parsed.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
                            format!("{code}: {m}")
                        })
                        .unwrap_or_else(|| payload.clone()),
                ),
                archive: None,
                archives: None,
                content: None,
            },
            _ => return,
        };
        emit(TapTapEvent::CloudSave(result));
    }

    // ---- TapDB 埋点（fire-and-forget，无回调） ----

    /// 上报账号 ID（`None` 即 clearUser）。properties 为空对象即可。
    pub fn db_set_user(
        user_id: Option<&str>,
        properties: &serde_json::Value,
    ) -> Result<(), TapTapError> {
        let jid = user_id.unwrap_or("");
        let props = serde_json::to_string(properties)
            .map_err(|e| TapTapError::Jni(e.to_string()))?;
        with_env(|env| {
            let shim = shim_class(env)?;
            let jid = env.new_string(jid).map_err(|e| jerr(env, e))?;
            let jprops = env.new_string(props).map_err(|e| jerr(env, e))?;
            env.call_static_method(
                shim,
                "dbSetUserId",
                "(Ljava/lang/String;Ljava/lang/String;)V",
                &[JValue::Object(&jid), JValue::Object(&jprops)],
            )
            .map_err(|e| jerr(env, e))?;
            Ok(())
        })
    }

    /// 自定义事件。
    pub fn db_log_event(
        name: &str,
        properties: &serde_json::Value,
    ) -> Result<(), TapTapError> {
        let props = serde_json::to_string(properties)
            .map_err(|e| TapTapError::Jni(e.to_string()))?;
        with_env(|env| {
            let shim = shim_class(env)?;
            let jname = env.new_string(name).map_err(|e| jerr(env, e))?;
            let jprops = env.new_string(props).map_err(|e| jerr(env, e))?;
            env.call_static_method(
                shim,
                "dbLogEvent",
                "(Ljava/lang/String;Ljava/lang/String;)V",
                &[JValue::Object(&jname), JValue::Object(&jprops)],
            )
            .map_err(|e| jerr(env, e))?;
            Ok(())
        })
    }

    /// 追加静态通用属性（随后续所有事件上报）。
    pub fn db_add_common(properties: &serde_json::Value) -> Result<(), TapTapError> {
        let props = serde_json::to_string(properties)
            .map_err(|e| TapTapError::Jni(e.to_string()))?;
        with_env(|env| {
            let shim = shim_class(env)?;
            let jprops = env.new_string(props).map_err(|e| jerr(env, e))?;
            env.call_static_method(
                shim,
                "dbAddCommon",
                "(Ljava/lang/String;)V",
                &[JValue::Object(&jprops)],
            )
            .map_err(|e| jerr(env, e))?;
            Ok(())
        })
    }

    // ---- 云存档（异步，结果经 TapTapEvent::CloudSave 返回） ----

    fn next_callback_id() -> i32 {
        use std::sync::atomic::{AtomicI32, Ordering};
        static NEXT: AtomicI32 = AtomicI32::new(1);
        NEXT.fetch_add(1, Ordering::Relaxed)
    }

    /// 创建云存档。`content` 为存档内容（UTF-8 文本）。
    pub fn cloud_save_create(
        name: &str,
        summary: &str,
        extra: &str,
        playtime: i32,
        content: &str,
    ) -> Result<(), TapTapError> {
        let cb = next_callback_id();
        with_env(|env| {
            let shim = shim_class(env)?;
            let jname = env.new_string(name).map_err(|e| jerr(env, e))?;
            let jsummary = env.new_string(summary).map_err(|e| jerr(env, e))?;
            let jextra = env.new_string(extra).map_err(|e| jerr(env, e))?;
            let jcontent = env.new_string(content).map_err(|e| jerr(env, e))?;
            env.call_static_method(
                shim,
                "cloudSaveCreate",
                "(IILjava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V",
                &[
                    JValue::Int(cb),
                    JValue::Object(&jname),
                    JValue::Object(&jsummary),
                    JValue::Object(&jextra),
                    JValue::Int(playtime),
                    JValue::Object(&jcontent),
                ],
            )
            .map_err(|e| jerr(env, e))?;
            Ok(())
        })
    }

    /// 覆盖更新指定云存档。
    pub fn cloud_save_update(
        uuid: &str,
        name: &str,
        summary: &str,
        extra: &str,
        playtime: i32,
        content: &str,
    ) -> Result<(), TapTapError> {
        let cb = next_callback_id();
        with_env(|env| {
            let shim = shim_class(env)?;
            let juuid = env.new_string(uuid).map_err(|e| jerr(env, e))?;
            let jname = env.new_string(name).map_err(|e| jerr(env, e))?;
            let jsummary = env.new_string(summary).map_err(|e| jerr(env, e))?;
            let jextra = env.new_string(extra).map_err(|e| jerr(env, e))?;
            let jcontent = env.new_string(content).map_err(|e| jerr(env, e))?;
            env.call_static_method(
                shim,
                "cloudSaveUpdate",
                "(ILjava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V",
                &[
                    JValue::Int(cb),
                    JValue::Object(&juuid),
                    JValue::Object(&jname),
                    JValue::Object(&jsummary),
                    JValue::Object(&jextra),
                    JValue::Int(playtime),
                    JValue::Object(&jcontent),
                ],
            )
            .map_err(|e| jerr(env, e))?;
            Ok(())
        })
    }

    /// 拉取当前玩家的存档列表。
    pub fn cloud_save_list() -> Result<(), TapTapError> {
        let cb = next_callback_id();
        with_env(|env| {
            let shim = shim_class(env)?;
            env.call_static_method(shim, "cloudSaveList", "(I)V", &[JValue::Int(cb)])
                .map_err(|e| jerr(env, e))?;
            Ok(())
        })
    }

    /// 读取指定存档内容。
    pub fn cloud_save_read(uuid: &str, file_id: &str) -> Result<(), TapTapError> {
        let cb = next_callback_id();
        with_env(|env| {
            let shim = shim_class(env)?;
            let juuid = env.new_string(uuid).map_err(|e| jerr(env, e))?;
            let jfile = env.new_string(file_id).map_err(|e| jerr(env, e))?;
            env.call_static_method(
                shim,
                "cloudSaveRead",
                "(ILjava/lang/String;Ljava/lang/String;)V",
                &[JValue::Int(cb), JValue::Object(&juuid), JValue::Object(&jfile)],
            )
            .map_err(|e| jerr(env, e))?;
            Ok(())
        })
    }

    /// 删除指定云存档。
    pub fn cloud_save_delete(uuid: &str) -> Result<(), TapTapError> {
        let cb = next_callback_id();
        with_env(|env| {
            let shim = shim_class(env)?;
            let juuid = env.new_string(uuid).map_err(|e| jerr(env, e))?;
            env.call_static_method(
                shim,
                "cloudSaveDelete",
                "(ILjava/lang/String;)V",
                &[JValue::Int(cb), JValue::Object(&juuid)],
            )
            .map_err(|e| jerr(env, e))?;
            Ok(())
        })
    }
}

#[cfg(target_os = "android")]
pub use platform::*;

#[cfg(not(target_os = "android"))]
mod platform {
    use super::*;

    pub fn poll() -> Vec<TapTapEvent> {
        Vec::new()
    }

    pub fn init(_config: &TapTapConfig) -> Result<(), TapTapError> {
        Err(TapTapError::NotAndroid)
    }

    pub fn login(_scopes: &[&str]) -> Result<(), TapTapError> {
        Err(TapTapError::NotAndroid)
    }

    pub fn logout() -> Result<(), TapTapError> {
        Err(TapTapError::NotAndroid)
    }

    pub fn current_account() -> Result<Option<TapAccount>, TapTapError> {
        Err(TapTapError::NotAndroid)
    }

    pub fn db_set_user(
        _user_id: Option<&str>,
        _properties: &serde_json::Value,
    ) -> Result<(), TapTapError> {
        Err(TapTapError::NotAndroid)
    }

    pub fn db_log_event(
        _name: &str,
        _properties: &serde_json::Value,
    ) -> Result<(), TapTapError> {
        Err(TapTapError::NotAndroid)
    }

    pub fn db_add_common(_properties: &serde_json::Value) -> Result<(), TapTapError> {
        Err(TapTapError::NotAndroid)
    }

    pub fn cloud_save_create(
        _name: &str,
        _summary: &str,
        _extra: &str,
        _playtime: i32,
        _content: &str,
    ) -> Result<(), TapTapError> {
        Err(TapTapError::NotAndroid)
    }

    pub fn cloud_save_update(
        _uuid: &str,
        _name: &str,
        _summary: &str,
        _extra: &str,
        _playtime: i32,
        _content: &str,
    ) -> Result<(), TapTapError> {
        Err(TapTapError::NotAndroid)
    }

    pub fn cloud_save_list() -> Result<(), TapTapError> {
        Err(TapTapError::NotAndroid)
    }

    pub fn cloud_save_read(_uuid: &str, _file_id: &str) -> Result<(), TapTapError> {
        Err(TapTapError::NotAndroid)
    }

    pub fn cloud_save_delete(_uuid: &str) -> Result<(), TapTapError> {
        Err(TapTapError::NotAndroid)
    }
}

#[cfg(not(target_os = "android"))]
pub use platform::*;
