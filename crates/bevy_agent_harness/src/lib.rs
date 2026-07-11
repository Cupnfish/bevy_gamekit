mod capture;
mod interaction;

pub use capture::{
    CaptureFrameManifest, CaptureManifest, CaptureSequenceRequest, CaptureSequenceStatus,
};
pub use interaction::{
    AgentAction, AgentCanvasConfig, AgentCanvasScaleMode, AgentInteractionRequest,
    AgentInteractionStatus, AgentMouseButton, AgentScrollUnit, AgentTarget, AgentTargetCapability,
    AgentTargetDisabled, AgentTargetInfo, AgentTargetPoint, AgentTargetRegistry, AgentTargetRole,
    AgentTargetSpace, AgentTextInputMode,
};

use std::{path::PathBuf, time::Duration};

#[cfg(feature = "ci")]
use bevy::dev_tools::ci_testing::CiTestingCustomEvent;
#[cfg(feature = "remote")]
use bevy::remote::{RemotePlugin, http::RemoteHttpPlugin};
use bevy::{prelude::*, time::TimeSystems};
use capture::CaptureRuntime;

/// Prefix accepted by `CiTestingEvent::Custom` for capture requests.
pub const CI_CAPTURE_PREFIX: &str = "agent.capture ";
/// Prefix accepted by `CiTestingEvent::Custom` for interaction requests.
pub const CI_INTERACTION_PREFIX: &str = "agent.interact ";

#[derive(Clone, Debug)]
pub struct AgentHarnessConfig {
    pub output_dir: PathBuf,
    pub capture_hotkey: Option<KeyCode>,
    pub enable_remote: bool,
    pub remote_port: u16,
    pub default_frame_interval: Duration,
    pub canvas: Option<AgentCanvasConfig>,
    pub max_queued_interactions: usize,
}

impl Default for AgentHarnessConfig {
    fn default() -> Self {
        Self {
            output_dir: PathBuf::from("agent-captures"),
            capture_hotkey: Some(KeyCode::F12),
            enable_remote: true,
            remote_port: 15_702,
            default_frame_interval: Duration::from_secs_f64(1.0 / 60.0),
            canvas: None,
            max_queued_interactions: 64,
        }
    }
}

pub struct AgentHarnessPlugin {
    config: AgentHarnessConfig,
}

impl AgentHarnessPlugin {
    pub fn new(config: AgentHarnessConfig) -> Self {
        Self { config }
    }
}

impl Default for AgentHarnessPlugin {
    fn default() -> Self {
        Self::new(AgentHarnessConfig::default())
    }
}

#[derive(Resource, Clone)]
struct HarnessConfig(AgentHarnessConfig);

impl Plugin for AgentHarnessPlugin {
    fn build(&self, app: &mut App) {
        if self.config.enable_remote {
            #[cfg(feature = "remote")]
            {
                if !app.is_plugin_added::<RemotePlugin>() {
                    app.add_plugins(RemotePlugin::default());
                }
                if !app.is_plugin_added::<RemoteHttpPlugin>() {
                    app.add_plugins(RemoteHttpPlugin::default().with_port(self.config.remote_port));
                }
            }
        }

        app.register_type::<CaptureSequenceRequest>()
            .register_type::<CaptureSequenceStatus>()
            .register_type::<AgentTarget>()
            .register_type::<AgentTargetDisabled>()
            .register_type::<AgentTargetRegistry>()
            .register_type::<AgentInteractionRequest>()
            .register_type::<AgentInteractionStatus>()
            .add_message::<CaptureSequenceRequest>()
            .add_message::<AgentInteractionRequest>()
            .insert_resource(HarnessConfig(self.config.clone()))
            .insert_resource(CaptureRuntime::new(self.config.output_dir.clone()))
            .insert_resource(interaction::InteractionRuntime::default())
            .init_resource::<CaptureSequenceStatus>()
            .init_resource::<AgentTargetRegistry>()
            .init_resource::<AgentInteractionStatus>()
            .add_systems(
                Startup,
                (
                    capture_from_environment,
                    interaction::interaction_from_environment,
                ),
            )
            .add_systems(First, capture::advance_capture_time.after(TimeSystems))
            .add_systems(
                PreUpdate,
                (
                    interaction::receive_interaction_requests,
                    interaction::drive_interactions,
                )
                    .chain()
                    .before(bevy::input::InputSystems),
            )
            .add_systems(Update, capture_hotkey)
            .add_systems(
                PostUpdate,
                interaction::sync_target_registry
                    .after(bevy::transform::TransformSystems::Propagate),
            )
            .add_systems(
                Last,
                (
                    capture::receive_capture_requests,
                    capture::drive_capture_sequence,
                )
                    .chain(),
            );

        #[cfg(feature = "ci")]
        app.add_systems(
            Update,
            (
                capture_from_ci_event,
                interaction::interaction_from_ci_event,
            ),
        );
    }
}

fn capture_hotkey(
    config: Res<HarnessConfig>,
    keys: Res<ButtonInput<KeyCode>>,
    mut requests: MessageWriter<CaptureSequenceRequest>,
) {
    let Some(key) = config.0.capture_hotkey else {
        return;
    };
    if keys.just_pressed(key) {
        requests.write(CaptureSequenceRequest::single("hotkey"));
    }
}

fn capture_from_environment(mut requests: MessageWriter<CaptureSequenceRequest>) {
    let Some(value) = std::env::var_os("BEVY_AGENT_CAPTURE") else {
        return;
    };
    match serde_json::from_str::<CaptureSequenceRequest>(&value.to_string_lossy()) {
        Ok(request) => {
            requests.write(request);
        }
        Err(error) => error!("invalid BEVY_AGENT_CAPTURE request: {error}"),
    }
}

#[cfg(feature = "ci")]
fn capture_from_ci_event(
    mut events: MessageReader<CiTestingCustomEvent>,
    mut requests: MessageWriter<CaptureSequenceRequest>,
) {
    for event in events.read() {
        let Some(json) = event.0.strip_prefix(CI_CAPTURE_PREFIX) else {
            continue;
        };
        match serde_json::from_str::<CaptureSequenceRequest>(json) {
            Ok(request) => {
                requests.write(request);
            }
            Err(error) => error!("invalid CI capture request: {error}"),
        }
    }
}
