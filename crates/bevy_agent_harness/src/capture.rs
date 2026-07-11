//! 帧序列截图捕获。
//!
//! 捕获期间会**接管 `Time<Virtual>`**：在 `drive_capture_sequence` 里把虚拟时钟暂停，
//! 每捕获一帧后由 `advance_capture_time`（`First` 阶段）按 `frame_interval_seconds`
//! 精确推进一帧，从而得到与帧率解耦的确定性截图序列。捕获结束后恢复原来的暂停状态。
//!
//! 因此：依赖本捕获功能时，游戏应使用默认的 `Time<Virtual>`（`DefaultPlugins` 已包含），
//! 且建议固定时间步长以保证逐帧可复现。

use std::{
    collections::VecDeque,
    fs,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use bevy::{
    app::AppExit,
    prelude::*,
    reflect::{ReflectDeserialize, ReflectSerialize},
    render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk},
    time::Virtual,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Message, Reflect, Serialize)]
#[reflect(Debug, Message, Serialize, Deserialize)]
pub struct CaptureSequenceRequest {
    pub label: String,
    pub frame_count: u32,
    #[serde(default = "default_frame_interval")]
    pub frame_interval_seconds: f64,
    #[serde(default)]
    pub delay_frames: u32,
    #[serde(default)]
    pub exit_after: bool,
}

impl CaptureSequenceRequest {
    pub fn single(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            frame_count: 1,
            frame_interval_seconds: default_frame_interval(),
            delay_frames: 0,
            exit_after: false,
        }
    }

    pub fn sequence(label: impl Into<String>, frame_count: u32) -> Self {
        Self {
            label: label.into(),
            frame_count: frame_count.max(1),
            frame_interval_seconds: default_frame_interval(),
            delay_frames: 0,
            exit_after: false,
        }
    }
}

const fn default_frame_interval() -> f64 {
    1.0 / 60.0
}

#[derive(Clone, Debug, Default, Deserialize, Reflect, Resource, Serialize)]
#[reflect(Debug, Resource, Serialize, Deserialize)]
pub struct CaptureSequenceStatus {
    pub active: bool,
    pub session_id: Option<String>,
    pub label: Option<String>,
    pub requested_frames: u32,
    pub captured_frames: u32,
    pub last_completed_session: Option<String>,
    pub last_manifest: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CaptureFrameManifest {
    pub index: u32,
    pub app_frame: u64,
    pub elapsed_seconds: f64,
    pub path: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CaptureManifest {
    pub schema_version: u32,
    pub complete: bool,
    pub session_id: String,
    pub label: String,
    pub requested_frames: u32,
    pub frame_interval_seconds: f64,
    pub delay_frames: u32,
    pub frames: Vec<CaptureFrameManifest>,
}

#[derive(Component)]
struct PendingSequenceFrame {
    session_id: String,
    index: u32,
    app_frame: u64,
    elapsed_seconds: f64,
    path: PathBuf,
    relative_path: String,
}

struct ActiveSequence {
    request: CaptureSequenceRequest,
    session_id: String,
    directory: PathBuf,
    delay_remaining: u32,
    next_index: u32,
    screenshot_pending: bool,
    advance_time: bool,
    virtual_time_was_paused: bool,
    frames: Vec<CaptureFrameManifest>,
}

#[derive(Resource)]
pub(crate) struct CaptureRuntime {
    output_dir: PathBuf,
    queue: VecDeque<CaptureSequenceRequest>,
    active: Option<ActiveSequence>,
    app_frame: u64,
    session_counter: u64,
}

impl CaptureRuntime {
    pub(crate) fn new(output_dir: PathBuf) -> Self {
        Self {
            output_dir,
            queue: VecDeque::new(),
            active: None,
            app_frame: 0,
            session_counter: 0,
        }
    }
}

pub(crate) fn receive_capture_requests(
    mut requests: MessageReader<CaptureSequenceRequest>,
    mut runtime: ResMut<CaptureRuntime>,
) {
    runtime
        .queue
        .extend(requests.read().cloned().map(normalize_request));
}

pub(crate) fn advance_capture_time(
    mut runtime: ResMut<CaptureRuntime>,
    mut virtual_time: ResMut<Time<Virtual>>,
    mut generic_time: ResMut<Time>,
) {
    let Some(active) = runtime.active.as_mut() else {
        return;
    };
    if !active.advance_time {
        return;
    }

    virtual_time.advance_by(Duration::from_secs_f64(
        active.request.frame_interval_seconds,
    ));
    *generic_time = virtual_time.as_generic();
    active.advance_time = false;
}

pub(crate) fn drive_capture_sequence(
    mut commands: Commands,
    time: Res<Time>,
    mut virtual_time: ResMut<Time<Virtual>>,
    mut runtime: ResMut<CaptureRuntime>,
    mut status: ResMut<CaptureSequenceStatus>,
) {
    runtime.app_frame = runtime.app_frame.wrapping_add(1);

    if runtime.active.is_none()
        && let Some(request) = runtime.queue.pop_front()
    {
        let session_id = session_id(&request.label, runtime.session_counter);
        runtime.session_counter = runtime.session_counter.wrapping_add(1);
        let directory = runtime.output_dir.join(&session_id);
        if let Err(error) = fs::create_dir_all(&directory) {
            status.last_error = Some(format!("failed to create {}: {error}", directory.display()));
            return;
        }
        status.active = true;
        status.session_id = Some(session_id.clone());
        status.label = Some(request.label.clone());
        status.requested_frames = request.frame_count;
        status.captured_frames = 0;
        status.last_error = None;
        runtime.active = Some(ActiveSequence {
            delay_remaining: request.delay_frames,
            request,
            session_id,
            directory,
            next_index: 0,
            screenshot_pending: false,
            advance_time: false,
            virtual_time_was_paused: virtual_time.is_paused(),
            frames: Vec::new(),
        });
    }

    let app_frame = runtime.app_frame;
    let Some(active) = runtime.active.as_mut() else {
        return;
    };

    if active.delay_remaining > 0 {
        active.delay_remaining -= 1;
        return;
    }

    if !virtual_time.is_paused() {
        virtual_time.pause();
    }
    if active.screenshot_pending || active.advance_time {
        return;
    }

    let index = active.next_index;
    let relative_path = format!("frame_{index:04}.png");
    let frame = PendingSequenceFrame {
        session_id: active.session_id.clone(),
        index,
        app_frame,
        elapsed_seconds: time.elapsed_secs_f64(),
        path: active.directory.join(&relative_path),
        relative_path,
    };
    commands
        .spawn((Screenshot::primary_window(), frame))
        .observe(save_sequence_frame);
    active.screenshot_pending = true;
}

fn save_sequence_frame(
    captured: On<ScreenshotCaptured>,
    frame: Query<&PendingSequenceFrame>,
    mut runtime: ResMut<CaptureRuntime>,
    mut status: ResMut<CaptureSequenceStatus>,
    mut virtual_time: ResMut<Time<Virtual>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Ok(frame) = frame.get(captured.entity) else {
        status.last_error = Some("screenshot completed without sequence metadata".into());
        return;
    };
    let Some(active) = runtime.active.as_mut() else {
        return;
    };
    if frame.session_id != active.session_id {
        return;
    }

    let width = captured.image.width();
    let height = captured.image.height();
    save_to_disk(&frame.path)(captured);
    active.frames.push(CaptureFrameManifest {
        index: frame.index,
        app_frame: frame.app_frame,
        elapsed_seconds: frame.elapsed_seconds,
        path: frame.relative_path.clone(),
        width,
        height,
    });
    active.next_index += 1;
    active.screenshot_pending = false;
    status.captured_frames = active.next_index;

    if active.next_index < active.request.frame_count {
        active.advance_time = true;
        let _ = write_manifest(active, false);
        return;
    }

    let manifest_path = write_manifest(active, true);
    let completed_session = active.session_id.clone();
    let exit_after = active.request.exit_after;
    let restore_pause = active.virtual_time_was_paused;
    if !restore_pause {
        virtual_time.unpause();
    }
    status.active = false;
    status.last_completed_session = Some(completed_session);
    status.last_manifest = manifest_path
        .as_ref()
        .ok()
        .map(|path| path.display().to_string());
    if let Err(error) = manifest_path {
        status.last_error = Some(error);
    }
    runtime.active = None;

    if exit_after {
        exit.write(AppExit::Success);
    }
}

fn write_manifest(active: &ActiveSequence, complete: bool) -> Result<PathBuf, String> {
    let manifest = CaptureManifest {
        schema_version: 1,
        complete,
        session_id: active.session_id.clone(),
        label: active.request.label.clone(),
        requested_frames: active.request.frame_count,
        frame_interval_seconds: active.request.frame_interval_seconds,
        delay_frames: active.request.delay_frames,
        frames: active.frames.clone(),
    };
    let path = active.directory.join("manifest.json");
    let json = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| format!("failed to serialize capture manifest: {error}"))?;
    fs::write(&path, json)
        .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
    Ok(path)
}

fn normalize_request(mut request: CaptureSequenceRequest) -> CaptureSequenceRequest {
    request.frame_count = request.frame_count.max(1);
    if !request.frame_interval_seconds.is_finite() || request.frame_interval_seconds <= 0.0 {
        request.frame_interval_seconds = default_frame_interval();
    }
    request
}

fn session_id(label: &str, counter: u64) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("{}-{millis}-{counter:03}", sanitize_label(label))
}

fn sanitize_label(label: &str) -> String {
    let label = label
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    let label = label.trim_matches('-');
    if label.is_empty() {
        "capture".into()
    } else {
        label.chars().take(48).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn invalid_requests_are_normalized() {
        let request = normalize_request(CaptureSequenceRequest {
            label: "test".into(),
            frame_count: 0,
            frame_interval_seconds: f64::NAN,
            delay_frames: 0,
            exit_after: false,
        });
        assert_eq!(request.frame_count, 1);
        assert_eq!(request.frame_interval_seconds, 1.0 / 60.0);
    }

    #[test]
    fn labels_are_safe_for_session_directories() {
        assert_eq!(sanitize_label("Retry animation #1"), "retry-animation--1");
        assert_eq!(sanitize_label("---"), "capture");
    }

    #[test]
    fn manifest_path_is_inside_the_session_directory() {
        let path = Path::new("captures").join("session").join("manifest.json");
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("manifest.json")
        );
    }
}
