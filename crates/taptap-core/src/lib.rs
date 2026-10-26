//! TapTap SDK Rust 适配——平台无关类型层。
//!
//! 本 crate 不接触任何平台 API，只承载：
//! - 统一初始化配置（clientId / clientToken / region）
//! - 各模块共享的请求与结果类型
//! - TapSDK 错误码常量（与官方文档一一对应，见各模块文档）
//!
//! 平台实现：
//! - Android：`taptap-android`（JNI 桥）
//! - Windows：`taptap-pc`（官方 PC C++ SDK 的 FFI 封装）
//! - Bevy 集成：`bevy_taptap`

use serde::{Deserialize, Serialize};

/// TapTap 服务区域，对应官方 `TapTapRegion.CN | GLOBAL`。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Region {
    Cn,
    Global,
}

/// 统一初始化配置，对应官方 `TapTapSdkOptions`。
///
/// `client_id` / `client_token` 从 TapTap 开发者中心「游戏服务」获取；
/// 包名与签名必须与开发者中心配置一致，否则初始化报 1001（CONFIG_ERROR）。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TapTapConfig {
    pub client_id: String,
    pub client_token: String,
    pub region: Region,
    /// 对应 `TapTapSdkOptions.setEnableLog`，开发期建议 true。
    #[serde(default)]
    pub enable_log: bool,
    /// 游戏版本号，可选，对应 `setGameVersion`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_version: Option<String>,
}

impl TapTapConfig {
    pub fn new(client_id: impl Into<String>, client_token: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            client_token: client_token.into(),
            region: Region::Cn,
            enable_log: true,
            game_version: None,
        }
    }
}

/// 初始化结果错误码，对应官方 `TapInitErrorCode`。
pub mod init_error {
    pub const PARAM_ERROR: i32 = 1000;
    /// 包名/签名/clientId 与开发者中心不一致，不可重试。
    pub const CONFIG_ERROR: i32 = 1001;
    pub const INTERNAL_ERROR: i32 = 1003;
}

/// 统一初始化结果事件载荷（平台实现回调后填充）。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InitResult {
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

/// TapTap 账号（登录成功后可得），字段对应官方 `TapTapAccount`。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TapAccount {
    /// Base64 编码，<=50 字符，同游戏内唯一
    pub open_id: String,
    /// 同一开发者的所有游戏共享
    pub union_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
}

/// 登录结果。`status`: 0=成功 1=失败 2=用户取消。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoginResult {
    pub status: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<TapAccount>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 云存档操作结果。`op` 标识是哪步操作（create/list/read/update/delete）。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CloudSaveResult {
    pub op: String,
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// 单个存档元信息（create/update/delete 成功时）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive: Option<serde_json::Value>,
    /// 存档列表（list 成功时）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archives: Option<Vec<serde_json::Value>>,
    /// 存档内容（read 成功时）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

/// 平台回调统一事件。平台实现把 SDK 回调转成此枚举，上层（如 bevy_taptap）轮询消费。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TapTapEvent {
    Init(InitResult),
    Login(LoginResult),
    Logout,
    CloudSave(CloudSaveResult),
}

/// 登录 scope 常量，对应官方 `Scopes`。
pub mod scopes {
    /// openId/unionId/昵称/头像
    pub const PUBLIC_PROFILE: &str = "public_profile";
    /// 仅 openId/unionId，支持无感登录
    pub const BASIC_INFO: &str = "basic_info";
    pub const USER_FRIENDS: &str = "user_friends";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_roundtrip() {
        let config = TapTapConfig::new("id", "token");
        let json = serde_json::to_string(&config).unwrap();
        let back: TapTapConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.client_id, "id");
        assert_eq!(back.region, Region::Cn);
    }
}
