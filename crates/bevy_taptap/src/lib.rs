//! TapTap SDK 的 Bevy 集成。
//!
//! 用法：
//!
//! ```ignore
//! App::new()
//!     .add_plugins(TapTapPlugin::new(TapTapConfig::new(client_id, client_token)))
//!     .add_systems(Update, |mut ev: MessageReader<InitEvent>| { /* ... */ });
//! ```
//!
//! 请求是普通函数调用（`taptap_android::login(..)` 等，桌面端为安全的 no-op），
//! 结果统一以 Message 回达，每帧在 `PreUpdate` 从平台回调通道搬运。

use bevy::prelude::*;
use taptap_core::{CloudSaveResult, InitResult, LoginResult, TapTapConfig, TapTapEvent};

pub use taptap_android::TapTapError;

/// 统一初始化完成。
#[derive(Message)]
pub struct InitEvent(pub InitResult);

/// 登录流程结束（成功/失败/取消）。
#[derive(Message)]
pub struct LoginEvent(pub LoginResult);

/// 登出完成。
#[derive(Message)]
pub struct LogoutEvent;

/// 云存档操作结果（create/list/read/update/delete 由状态机区分）。
#[derive(Message)]
pub struct CloudSaveEvent(pub CloudSaveResult);

pub struct TapTapPlugin {
    config: TapTapConfig,
}

impl TapTapPlugin {
    pub fn new(config: TapTapConfig) -> Self {
        Self { config }
    }
}

impl Plugin for TapTapPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<InitEvent>()
            .add_message::<LoginEvent>()
            .add_message::<LogoutEvent>()
            .add_message::<CloudSaveEvent>();

        match taptap_android::init(&self.config) {
            Ok(()) => info!("TapTap init 已派发，等待回调"),
            Err(e) => warn!("TapTap init 未派发: {e}"),
        }

        app.add_systems(PreUpdate, poll_platform_events);
    }
}

/// 把平台回调事件搬运为 Bevy Message。
fn poll_platform_events(
    mut init: MessageWriter<InitEvent>,
    mut login: MessageWriter<LoginEvent>,
    mut logout: MessageWriter<LogoutEvent>,
    mut cloudsave: MessageWriter<CloudSaveEvent>,
) {
    for event in taptap_android::poll() {
        match event {
            TapTapEvent::Init(r) => {
                init.write(InitEvent(r));
            }
            TapTapEvent::Login(r) => {
                login.write(LoginEvent(r));
            }
            TapTapEvent::Logout => {
                logout.write(LogoutEvent);
            }
            TapTapEvent::CloudSave(r) => {
                cloudsave.write(CloudSaveEvent(r));
            }
        }
    }
}
