package cn.taptap.demo

import android.app.Activity
import android.content.Context
import android.os.Handler
import android.os.Looper
import android.util.Log
import com.taptap.sdk.core.TapTapRegion
import com.taptap.sdk.core.TapTapSdk
import com.taptap.sdk.core.TapTapSdkOptions
import com.taptap.sdk.login.TapTapAccount
import com.taptap.sdk.login.TapTapLogin
import org.json.JSONArray
import org.json.JSONObject

/**
 * TapTap SDK 的 JNI 薄桥：把官方 AAR 的对象/回调压平成 JSON 字符串再回传 Rust。
 *
 * 约定（与 crates/taptap-android 一一对应）：
 *  - Rust → Kotlin：静态方法，参数为 Context/Activity + JSON 字符串
 *  - Kotlin → Rust：external fun（JNI 自动绑定 Java_cn_taptap_demo_TapTapShim_*）
 *  - SDK 要求主线程的调用统一 post 到主线程 Handler
 */
object TapTapShim {
    private const val TAG = "TapTapShim"
    private val mainHandler = Handler(Looper.getMainLooper())

    /** 当前 resumed 的 Activity。Rust 侧 ndk-context 给的是 Application，
     *  而登录等 UI 流程需要真正的 Activity，故经生命周期回调跟踪。 */
    @Volatile
    private var currentActivity: Activity? = null

    /** 应用上下文（init 时缓存，供云存档等文件操作使用）。 */
    @Volatile
    private var appContext: Context? = null

    // ---- 初始化 ----

    @JvmStatic
    fun init(context: Context, configJson: String) {
        appContext = context.applicationContext
        (context as? android.app.Application)?.registerActivityLifecycleCallbacks(
            object : android.app.Application.ActivityLifecycleCallbacks {
                override fun onActivityCreated(activity: Activity, savedInstanceState: android.os.Bundle?) {}
                override fun onActivityStarted(activity: Activity) {}
                override fun onActivityResumed(activity: Activity) {
                    currentActivity = activity
                }

                override fun onActivityPaused(activity: Activity) {
                    if (currentActivity === activity) currentActivity = null
                }

                override fun onActivityStopped(activity: Activity) {}
                override fun onActivitySaveInstanceState(activity: Activity, outState: android.os.Bundle) {}
                override fun onActivityDestroyed(activity: Activity) {}
            },
        )
        mainHandler.post {
            try {
                val cfg = JSONObject(configJson)
                val options = TapTapSdkOptions(
                    cfg.getString("client_id"),
                    cfg.getString("client_token"),
                    if (cfg.optString("region", "CN") == "GLOBAL") TapTapRegion.GLOBAL else TapTapRegion.CN,
                )
                options.enableLog = cfg.optBoolean("enable_log", true)
                TapTapSdk.addInitCallback(object : com.taptap.sdk.initializer.api.TapInitCallback {
                    override fun onInitSuccess() {
                        Log.i(TAG, "init success")
                        nativeOnInitResult(true, "")
                    }

                    override fun onInitFail(errorCode: Int, errorMsg: String) {
                        Log.e(TAG, "init fail: $errorCode $errorMsg")
                        nativeOnInitResult(false, "$errorCode:$errorMsg")
                    }
                })
                // TapDB（tap-db 模块）：随统一 init 传入 EventOptions 启用
                val eventOptions = com.taptap.sdk.core.TapTapEventOptions.Builder()
                    .apply {
                        val channel = cfg.optString("db_channel", "")
                        if (channel.isNotEmpty()) channel(channel)
                    }
                    .build()
                TapTapSdk.init(context.applicationContext, options, eventOptions)
            } catch (e: Exception) {
                Log.e(TAG, "init shim error", e)
                nativeOnInitResult(false, "-1:shim_init:${e.message}")
            }
        }
    }

    // ---- 登录 ----

    /**
     * scopesJson: 字符串数组 JSON，如 ["public_profile"]。
     * Rust 传入的 context 实为 Application（ndk-context 约定），真正发起登录时
     * 使用生命周期跟踪到的 resumed Activity；拿不到则回传错误。
     * 回传 status：0=成功 1=失败 2=取消
     */
    @JvmStatic
    fun login(context: Context, scopesJson: String) {
        mainHandler.post {
            val activity = currentActivity
            if (activity == null) {
                Log.e(TAG, "login: no resumed activity")
                nativeOnLoginResult(1, JSONObject().put("error", "no resumed activity").toString())
                return@post
            }
            try {
                val scopes = JSONArray(scopesJson).let { arr ->
                    (0 until arr.length()).map { arr.getString(it) }
                }.toTypedArray()
                TapTapLogin.loginWithScopes(
                    activity,
                    scopes,
                    object : com.taptap.sdk.kit.internal.callback.TapTapCallback<TapTapAccount> {
                        override fun onSuccess(account: TapTapAccount) {
                            Log.i(TAG, "login success")
                            nativeOnLoginResult(0, accountToJson(account))
                        }

                        override fun onFail(exception: com.taptap.sdk.kit.internal.exception.TapTapException) {
                            Log.e(TAG, "login fail: ${exception.message}")
                            nativeOnLoginResult(1, JSONObject().put("error", exception.message ?: "unknown").toString())
                        }

                        override fun onCancel() {
                            Log.i(TAG, "login cancel")
                            nativeOnLoginResult(2, "")
                        }
                    },
                )
            } catch (e: Exception) {
                Log.e(TAG, "login shim error", e)
                nativeOnLoginResult(1, JSONObject().put("error", "shim_login:${e.message}").toString())
            }
        }
    }

    @JvmStatic
    fun logout() {
        mainHandler.post {
            try {
                TapTapLogin.logout()
            } catch (e: Exception) {
                Log.e(TAG, "logout error", e)
            }
            nativeOnLogout()
        }
    }

    /** 同步读取本地缓存的登录账号；未登录返回 null。 */
    @JvmStatic
    fun currentAccount(): String? {
        return try {
            accountToJson(TapTapLogin.getCurrentTapAccount())
        } catch (e: Exception) {
            Log.e(TAG, "currentAccount error", e)
            null
        }
    }

    // ---- TapDB 埋点（fire-and-forget，无回调） ----

    private fun props(json: String): org.json.JSONObject =
        if (json.isEmpty()) org.json.JSONObject() else org.json.JSONObject(json)

    /** userId 为空串时等同 clearUser。 */
    @JvmStatic
    fun dbSetUserId(userId: String, propertiesJson: String) {
        mainHandler.post {
            try {
                if (userId.isEmpty()) {
                    com.taptap.sdk.core.TapTapEvent.clearUser()
                } else {
                    com.taptap.sdk.core.TapTapEvent.setUserId(userId, props(propertiesJson))
                }
            } catch (e: Exception) {
                Log.e(TAG, "dbSetUserId error", e)
            }
        }
    }

    @JvmStatic
    fun dbClearUser() {
        mainHandler.post {
            try {
                com.taptap.sdk.core.TapTapEvent.clearUser()
            } catch (e: Exception) {
                Log.e(TAG, "dbClearUser error", e)
            }
        }
    }

    @JvmStatic
    fun dbLogEvent(name: String, propertiesJson: String) {
        mainHandler.post {
            try {
                com.taptap.sdk.core.TapTapEvent.logEvent(name, props(propertiesJson))
            } catch (e: Exception) {
                Log.e(TAG, "dbLogEvent error", e)
            }
        }
    }

    @JvmStatic
    fun dbAddCommon(propertiesJson: String) {
        mainHandler.post {
            try {
                com.taptap.sdk.core.TapTapEvent.addCommon(props(propertiesJson))
            } catch (e: Exception) {
                Log.e(TAG, "dbAddCommon error", e)
            }
        }
    }

    // ---- 云存档 ----

    /**
     * 回传统一走 nativeOnCloudSaveResult(callbackId, kind, payloadJson)：
     * kind 1=单档元信息 2=列表 3=内容 4=错误
     */
    private fun csCallback(callbackId: Int) =
        object : com.taptap.sdk.cloudsave.internal.TapCloudSaveRequestCallback {
            override fun onArchiveCreated(archive: com.taptap.sdk.cloudsave.ArchiveData) =
                nativeOnCloudSaveResult(callbackId, 1, archiveToJson(archive).toString())

            override fun onArchiveUpdated(archive: com.taptap.sdk.cloudsave.ArchiveData) =
                nativeOnCloudSaveResult(callbackId, 1, archiveToJson(archive).toString())

            override fun onArchiveDeleted(archive: com.taptap.sdk.cloudsave.ArchiveData) =
                nativeOnCloudSaveResult(callbackId, 1, archiveToJson(archive).toString())

            override fun onArchiveListResult(archives: List<com.taptap.sdk.cloudsave.ArchiveData>) {
                val arr = org.json.JSONArray()
                archives.forEach { arr.put(archiveToJson(it)) }
                nativeOnCloudSaveResult(callbackId, 2, arr.toString())
            }

            override fun onArchiveDataResult(bytes: ByteArray) {
                val content = String(bytes, Charsets.UTF_8)
                nativeOnCloudSaveResult(
                    callbackId, 3,
                    org.json.JSONObject().put("content", content).toString(),
                )
            }

            override fun onArchiveCoverResult(bytes: ByteArray) =
                nativeOnCloudSaveResult(callbackId, 3, "{}")

            override fun onRequestError(errorCode: Int, errorMessage: String) =
                nativeOnCloudSaveResult(
                    callbackId, 4,
                    org.json.JSONObject()
                        .put("code", errorCode)
                        .put("message", errorMessage).toString(),
                )
        }

    private fun archiveToJson(a: com.taptap.sdk.cloudsave.ArchiveData?): org.json.JSONObject {
        if (a == null) return org.json.JSONObject()
        return org.json.JSONObject().apply {
            put("uuid", a.uuid)
            put("file_id", a.fileId)
            put("name", a.name)
            put("summary", a.summary)
            put("extra", a.extra)
            put("playtime", a.playtime)
            put("save_size", a.saveSize)
            put("created_time", a.createdTime)
            put("modified_time", a.modifiedTime)
        }
    }

    private fun buildMeta(
        name: String,
        summary: String,
        extra: String,
        playtime: Int,
    ): com.taptap.sdk.cloudsave.ArchiveMetadata =
        com.taptap.sdk.cloudsave.ArchiveMetadata.Builder()
            .setName(name)
            .setSummary(summary)
            .setExtra(extra)
            .setPlaytime(playtime)
            .build()

    /** 把存档内容写到应用私有目录，返回可传给 SDK 的绝对路径。 */
    private fun writeUploadFile(content: String): String {
        val dir = appContext!!.filesDir
        val file = java.io.File(dir, "tapdemo_upload.bin")
        file.writeText(content, Charsets.UTF_8)
        return file.absolutePath
    }

    @JvmStatic
    fun cloudSaveCreate(
        callbackId: Int,
        name: String,
        summary: String,
        extra: String,
        playtime: Int,
        content: String,
    ) {
        mainHandler.post {
            try {
                val path = writeUploadFile(content)
                com.taptap.sdk.cloudsave.TapTapCloudSave.createArchive(
                    buildMeta(name, summary, extra, playtime),
                    path,
                    null,
                    csCallback(callbackId),
                )
            } catch (e: Exception) {
                Log.e(TAG, "cloudSaveCreate error", e)
                nativeOnCloudSaveResult(
                    callbackId, 4,
                    org.json.JSONObject().put("code", -1).put("message", "shim:${e.message}").toString(),
                )
            }
        }
    }

    @JvmStatic
    fun cloudSaveUpdate(
        callbackId: Int,
        uuid: String,
        name: String,
        summary: String,
        extra: String,
        playtime: Int,
        content: String,
    ) {
        mainHandler.post {
            try {
                val path = writeUploadFile(content)
                com.taptap.sdk.cloudsave.TapTapCloudSave.updateArchive(
                    uuid,
                    buildMeta(name, summary, extra, playtime),
                    path,
                    null,
                    csCallback(callbackId),
                )
            } catch (e: Exception) {
                Log.e(TAG, "cloudSaveUpdate error", e)
                nativeOnCloudSaveResult(
                    callbackId, 4,
                    org.json.JSONObject().put("code", -1).put("message", "shim:${e.message}").toString(),
                )
            }
        }
    }

    @JvmStatic
    fun cloudSaveList(callbackId: Int) {
        mainHandler.post {
            try {
                com.taptap.sdk.cloudsave.TapTapCloudSave.getArchiveList(csCallback(callbackId))
            } catch (e: Exception) {
                Log.e(TAG, "cloudSaveList error", e)
                nativeOnCloudSaveResult(
                    callbackId, 4,
                    org.json.JSONObject().put("code", -1).put("message", "shim:${e.message}").toString(),
                )
            }
        }
    }

    @JvmStatic
    fun cloudSaveRead(callbackId: Int, uuid: String, fileId: String) {
        mainHandler.post {
            try {
                com.taptap.sdk.cloudsave.TapTapCloudSave.getArchiveData(uuid, fileId, csCallback(callbackId))
            } catch (e: Exception) {
                Log.e(TAG, "cloudSaveRead error", e)
                nativeOnCloudSaveResult(
                    callbackId, 4,
                    org.json.JSONObject().put("code", -1).put("message", "shim:${e.message}").toString(),
                )
            }
        }
    }

    @JvmStatic
    fun cloudSaveDelete(callbackId: Int, uuid: String) {
        mainHandler.post {
            try {
                com.taptap.sdk.cloudsave.TapTapCloudSave.deleteArchive(uuid, csCallback(callbackId))
            } catch (e: Exception) {
                Log.e(TAG, "cloudSaveDelete error", e)
                nativeOnCloudSaveResult(
                    callbackId, 4,
                    org.json.JSONObject().put("code", -1).put("message", "shim:${e.message}").toString(),
                )
            }
        }
    }

    // ---- 序列化 ----

    private fun accountToJson(account: TapTapAccount?): String {
        if (account == null) return ""
        return JSONObject().apply {
            put("open_id", account.openId ?: "")
            put("union_id", account.unionId ?: "")
            account.name?.let { put("name", it) }
            account.avatar?.let { put("avatar", it) }
        }.toString()
    }

    // ---- Rust 回调 ----

    @JvmStatic
    private external fun nativeOnInitResult(success: Boolean, info: String)

    @JvmStatic
    private external fun nativeOnLoginResult(status: Int, payload: String)

    @JvmStatic
    private external fun nativeOnLogout()

    /** kind：1=单档元信息 2=列表 3=内容 4=错误 */
    @JvmStatic
    private external fun nativeOnCloudSaveResult(callbackId: Int, kind: Int, payloadJson: String)
}
