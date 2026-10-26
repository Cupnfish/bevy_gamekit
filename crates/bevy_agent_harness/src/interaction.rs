use std::collections::{HashMap, VecDeque};

#[cfg(feature = "ci")]
use bevy::dev_tools::ci_testing::CiTestingCustomEvent;
use bevy::{
    camera::RenderTarget,
    input::{
        ButtonState,
        keyboard::{Key, KeyCode, KeyboardInput, NativeKey, NativeKeyCode},
        mouse::{MouseButton, MouseButtonInput, MouseScrollUnit, MouseWheel},
        touch::TouchPhase,
    },
    prelude::*,
    reflect::{ReflectDeserialize, ReflectSerialize},
    ui::{ComputedNode, UiGlobalTransform},
    window::{CursorLeft, CursorMoved, Ime, PrimaryWindow, WindowEvent, WindowRef},
};
use serde::{Deserialize, Serialize};

#[cfg(feature = "ci")]
use crate::CI_INTERACTION_PREFIX;
use crate::HarnessConfig;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentCanvasScaleMode {
    /// Preserve aspect ratio and use the largest whole-number scale that fits.
    IntegerFit,
    /// Preserve aspect ratio and fit continuously inside the window.
    Fit,
    /// Scale each axis independently to fill the window.
    Stretch,
}

#[derive(Clone, Debug)]
pub struct AgentCanvasConfig {
    pub logical_size: Vec2,
    pub world_center: Vec2,
    pub y_up: bool,
    pub scale_mode: AgentCanvasScaleMode,
}

impl AgentCanvasConfig {
    pub fn centered(logical_size: Vec2) -> Self {
        Self {
            logical_size,
            world_center: Vec2::ZERO,
            y_up: true,
            scale_mode: AgentCanvasScaleMode::IntegerFit,
        }
    }

    fn scale(&self, window_size: Vec2) -> Vec2 {
        let canvas = self.logical_size.max(Vec2::splat(f32::EPSILON));
        match self.scale_mode {
            AgentCanvasScaleMode::IntegerFit => {
                let fit = (window_size.x / canvas.x).min(window_size.y / canvas.y);
                let scale = if fit >= 1.0 {
                    fit.floor()
                } else {
                    fit.max(f32::EPSILON)
                };
                Vec2::splat(scale)
            }
            AgentCanvasScaleMode::Fit => {
                let scale = (window_size.x / canvas.x)
                    .min(window_size.y / canvas.y)
                    .max(f32::EPSILON);
                Vec2::splat(scale)
            }
            AgentCanvasScaleMode::Stretch => window_size / canvas,
        }
    }

    fn world_to_window(&self, world: Vec2, window_size: Vec2) -> Vec2 {
        let mut relative = world - self.world_center;
        if self.y_up {
            relative.y = -relative.y;
        }
        window_size * 0.5 + relative * self.scale(window_size)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Reflect, Serialize)]
#[reflect(Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentTargetSpace {
    #[default]
    Auto,
    Ui,
    World,
    Window,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Reflect, Serialize)]
#[reflect(Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentTargetRole {
    #[default]
    Generic,
    Button,
    Control,
    Region,
    GameObject,
    DragSurface,
    TextInput,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Reflect, Serialize)]
#[reflect(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentTargetCapability {
    Hover,
    Click,
    DoubleClick,
    Press,
    Drag,
    Scroll,
    TextInput,
}

#[derive(Clone, Debug, Component, Deserialize, Reflect, Serialize)]
#[reflect(Component, Debug, Serialize, Deserialize)]
pub struct AgentTarget {
    pub id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub role: AgentTargetRole,
    #[serde(default)]
    pub capabilities: Vec<AgentTargetCapability>,
    #[serde(default)]
    pub space: AgentTargetSpace,
    /// Local-space size for world targets, or logical-pixel size for window targets.
    #[serde(default)]
    pub size: Option<Vec2>,
    /// Local-space offset for world targets, or logical-pixel center for window targets.
    #[serde(default)]
    pub offset: Vec2,
}

impl AgentTarget {
    pub fn world(id: impl Into<String>, size: Vec2) -> Self {
        Self {
            id: id.into(),
            label: None,
            role: AgentTargetRole::Generic,
            capabilities: Vec::new(),
            space: AgentTargetSpace::World,
            size: Some(size),
            offset: Vec2::ZERO,
        }
    }

    pub fn ui(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: None,
            role: AgentTargetRole::Generic,
            capabilities: Vec::new(),
            space: AgentTargetSpace::Ui,
            size: None,
            offset: Vec2::ZERO,
        }
    }

    pub fn window(id: impl Into<String>, center: Vec2, size: Vec2) -> Self {
        Self {
            id: id.into(),
            label: None,
            role: AgentTargetRole::Generic,
            capabilities: Vec::new(),
            space: AgentTargetSpace::Window,
            size: Some(size),
            offset: center,
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn with_role(mut self, role: AgentTargetRole) -> Self {
        self.role = role;
        self
    }

    pub fn with_capabilities(
        mut self,
        capabilities: impl IntoIterator<Item = AgentTargetCapability>,
    ) -> Self {
        self.capabilities = capabilities.into_iter().collect();
        self
    }

    pub fn with_offset(mut self, offset: Vec2) -> Self {
        self.offset = offset;
        self
    }
}

/// Temporarily removes a target from semantic action resolution without despawning it.
#[derive(Clone, Copy, Debug, Default, Component, Reflect)]
#[reflect(Component, Debug, Default)]
pub struct AgentTargetDisabled;

#[derive(Clone, Debug, Deserialize, PartialEq, Reflect, Serialize)]
#[reflect(Debug, PartialEq, Serialize, Deserialize)]
pub struct AgentTargetInfo {
    pub id: String,
    pub label: Option<String>,
    pub role: AgentTargetRole,
    pub capabilities: Vec<AgentTargetCapability>,
    pub entity: u64,
    pub center: Vec2,
    pub size: Vec2,
    pub visible: bool,
    pub enabled: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Reflect, Resource, Serialize)]
#[reflect(Debug, Default, Resource, Serialize, Deserialize)]
pub struct AgentTargetRegistry {
    pub revision: u64,
    pub targets: Vec<AgentTargetInfo>,
    pub errors: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Reflect, Serialize)]
#[reflect(Debug, PartialEq, Serialize, Deserialize)]
pub struct AgentTargetPoint {
    pub target: String,
    /// A normalized offset inside the target AABB: `(-1, -1)` is top-left and `(1, 1)` bottom-right.
    #[serde(default)]
    pub anchor: Vec2,
    /// Additional offset in window logical pixels.
    #[serde(default)]
    pub offset: Vec2,
}

impl AgentTargetPoint {
    pub fn center(target: impl Into<String>) -> Self {
        Self {
            target: target.into(),
            anchor: Vec2::ZERO,
            offset: Vec2::ZERO,
        }
    }

    pub fn at_anchor(mut self, anchor: Vec2) -> Self {
        self.anchor = anchor;
        self
    }

    pub fn with_offset(mut self, offset: Vec2) -> Self {
        self.offset = offset;
        self
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Reflect, Serialize)]
#[reflect(Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum AgentMouseButton {
    #[default]
    Left,
    Right,
    Middle,
    Back,
    Forward,
    Other(u16),
}

impl From<AgentMouseButton> for MouseButton {
    fn from(value: AgentMouseButton) -> Self {
        match value {
            AgentMouseButton::Left => Self::Left,
            AgentMouseButton::Right => Self::Right,
            AgentMouseButton::Middle => Self::Middle,
            AgentMouseButton::Back => Self::Back,
            AgentMouseButton::Forward => Self::Forward,
            AgentMouseButton::Other(value) => Self::Other(value),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Reflect, Serialize)]
#[reflect(Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentScrollUnit {
    #[default]
    Line,
    Pixel,
}

impl From<AgentScrollUnit> for MouseScrollUnit {
    fn from(value: AgentScrollUnit) -> Self {
        match value {
            AgentScrollUnit::Line => Self::Line,
            AgentScrollUnit::Pixel => Self::Pixel,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Reflect, Serialize)]
#[reflect(Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentTextInputMode {
    #[default]
    Keyboard,
    Ime,
}

#[derive(Clone, Debug, Deserialize, Reflect, Serialize)]
#[reflect(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentAction {
    Hover {
        point: AgentTargetPoint,
        #[serde(default = "default_hover_frames")]
        frames: u32,
    },
    MoveTo {
        point: AgentTargetPoint,
        #[serde(default = "default_move_frames")]
        frames: u32,
    },
    MoveToPoint {
        position: Vec2,
        #[serde(default = "default_move_frames")]
        frames: u32,
    },
    Click {
        point: AgentTargetPoint,
        #[serde(default)]
        button: AgentMouseButton,
    },
    ClickAt {
        position: Vec2,
        #[serde(default)]
        button: AgentMouseButton,
    },
    DoubleClick {
        point: AgentTargetPoint,
        #[serde(default)]
        button: AgentMouseButton,
        #[serde(default = "default_double_click_gap")]
        gap_frames: u32,
    },
    Press {
        point: AgentTargetPoint,
        #[serde(default)]
        button: AgentMouseButton,
    },
    PressAt {
        position: Vec2,
        #[serde(default)]
        button: AgentMouseButton,
    },
    Release {
        #[serde(default)]
        point: Option<AgentTargetPoint>,
        #[serde(default)]
        button: AgentMouseButton,
    },
    DragTo {
        from: AgentTargetPoint,
        to: AgentTargetPoint,
        #[serde(default)]
        button: AgentMouseButton,
        #[serde(default = "default_drag_frames")]
        frames: u32,
    },
    DragBy {
        from: AgentTargetPoint,
        delta: Vec2,
        #[serde(default)]
        button: AgentMouseButton,
        #[serde(default = "default_drag_frames")]
        frames: u32,
    },
    Scroll {
        point: AgentTargetPoint,
        delta: Vec2,
        #[serde(default)]
        unit: AgentScrollUnit,
    },
    ScrollAt {
        position: Vec2,
        delta: Vec2,
        #[serde(default)]
        unit: AgentScrollUnit,
    },
    KeyTap {
        key: KeyCode,
    },
    KeyPress {
        key: KeyCode,
    },
    KeyRelease {
        key: KeyCode,
    },
    TextInput {
        #[serde(default)]
        point: Option<AgentTargetPoint>,
        text: String,
        #[serde(default)]
        mode: AgentTextInputMode,
    },
    WaitFrames {
        frames: u32,
    },
    CursorLeave,
}

impl AgentAction {
    pub fn click(target: impl Into<String>) -> Self {
        Self::Click {
            point: AgentTargetPoint::center(target),
            button: AgentMouseButton::Left,
        }
    }

    pub fn hover(target: impl Into<String>, frames: u32) -> Self {
        Self::Hover {
            point: AgentTargetPoint::center(target),
            frames,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Hover { .. } => "hover",
            Self::MoveTo { .. } => "move_to",
            Self::MoveToPoint { .. } => "move_to_point",
            Self::Click { .. } => "click",
            Self::ClickAt { .. } => "click_at",
            Self::DoubleClick { .. } => "double_click",
            Self::Press { .. } => "press",
            Self::PressAt { .. } => "press_at",
            Self::Release { .. } => "release",
            Self::DragTo { .. } => "drag_to",
            Self::DragBy { .. } => "drag_by",
            Self::Scroll { .. } => "scroll",
            Self::ScrollAt { .. } => "scroll_at",
            Self::KeyTap { .. } => "key_tap",
            Self::KeyPress { .. } => "key_press",
            Self::KeyRelease { .. } => "key_release",
            Self::TextInput { .. } => "text_input",
            Self::WaitFrames { .. } => "wait_frames",
            Self::CursorLeave => "cursor_leave",
        }
    }
}

const fn default_hover_frames() -> u32 {
    12
}

const fn default_move_frames() -> u32 {
    1
}

const fn default_drag_frames() -> u32 {
    12
}

const fn default_double_click_gap() -> u32 {
    2
}

#[derive(Clone, Debug, Deserialize, Message, Reflect, Serialize)]
#[reflect(Debug, Message, Serialize, Deserialize)]
pub struct AgentInteractionRequest {
    #[serde(default)]
    pub request_id: String,
    pub actions: Vec<AgentAction>,
}

impl AgentInteractionRequest {
    pub fn new(request_id: impl Into<String>, actions: Vec<AgentAction>) -> Self {
        Self {
            request_id: request_id.into(),
            actions,
        }
    }

    pub fn single(request_id: impl Into<String>, action: AgentAction) -> Self {
        Self::new(request_id, vec![action])
    }
}

#[derive(Clone, Debug, Default, Deserialize, Reflect, Resource, Serialize)]
#[reflect(Debug, Default, Resource, Serialize, Deserialize)]
pub struct AgentInteractionStatus {
    pub busy: bool,
    pub queued_requests: usize,
    pub active_request: Option<String>,
    pub current_action: Option<String>,
    pub completed_requests: u64,
    pub last_completed_request: Option<String>,
    pub last_failed_request: Option<String>,
    pub last_error: Option<String>,
    pub cursor_position: Option<Vec2>,
}

struct PendingRequest {
    id: String,
    actions: VecDeque<AgentAction>,
}

struct ActiveRequest {
    id: String,
    actions: VecDeque<AgentAction>,
    steps: VecDeque<InteractionStep>,
}

enum InteractionStep {
    Move {
        from: Vec2,
        to: Vec2,
        index: u32,
        total: u32,
    },
    Wait {
        remaining: u32,
    },
    MouseButton {
        button: AgentMouseButton,
        state: ButtonState,
    },
    Wheel {
        delta: Vec2,
        unit: AgentScrollUnit,
    },
    Key {
        key: KeyCode,
        state: ButtonState,
    },
    TextKeyboard {
        text: String,
        state: ButtonState,
    },
    TextIme {
        text: String,
    },
    CursorLeave,
}

#[derive(Resource, Default)]
pub(crate) struct InteractionRuntime {
    queue: VecDeque<PendingRequest>,
    active: Option<ActiveRequest>,
    cursor_position: Option<Vec2>,
    generated_request_id: u64,
}

pub(crate) fn interaction_from_environment(mut requests: MessageWriter<AgentInteractionRequest>) {
    let Some(value) = std::env::var_os("BEVY_AGENT_INTERACTION") else {
        return;
    };
    match serde_json::from_str::<AgentInteractionRequest>(&value.to_string_lossy()) {
        Ok(request) => {
            requests.write(request);
        }
        Err(error) => error!("invalid BEVY_AGENT_INTERACTION request: {error}"),
    }
}

#[cfg(feature = "ci")]
pub(crate) fn interaction_from_ci_event(
    mut events: MessageReader<CiTestingCustomEvent>,
    mut requests: MessageWriter<AgentInteractionRequest>,
) {
    for event in events.read() {
        let Some(json) = event.0.strip_prefix(CI_INTERACTION_PREFIX) else {
            continue;
        };
        match serde_json::from_str::<AgentInteractionRequest>(json) {
            Ok(request) => {
                requests.write(request);
            }
            Err(error) => error!("invalid CI interaction request: {error}"),
        }
    }
}

pub(crate) fn receive_interaction_requests(
    config: Res<HarnessConfig>,
    mut requests: MessageReader<AgentInteractionRequest>,
    mut runtime: ResMut<InteractionRuntime>,
    mut status: ResMut<AgentInteractionStatus>,
) {
    for request in requests.read() {
        let mut id = request.request_id.trim().to_string();
        if id.is_empty() {
            runtime.generated_request_id = runtime.generated_request_id.wrapping_add(1);
            id = format!("interaction-{}", runtime.generated_request_id);
        }

        let queued = runtime.queue.len() + usize::from(runtime.active.is_some());
        if queued >= config.0.max_queued_interactions {
            status.last_failed_request = Some(id);
            status.last_error = Some(format!(
                "interaction queue is full (limit {})",
                config.0.max_queued_interactions
            ));
            continue;
        }

        runtime.queue.push_back(PendingRequest {
            id,
            actions: request.actions.clone().into(),
        });
    }
    update_status(&runtime, &mut status);
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_interactions(
    mut windows: Query<(Entity, &mut Window), With<PrimaryWindow>>,
    registry: Res<AgentTargetRegistry>,
    mut runtime: ResMut<InteractionRuntime>,
    mut status: ResMut<AgentInteractionStatus>,
    mut cursor_events: MessageWriter<CursorMoved>,
    mut cursor_left_events: MessageWriter<CursorLeft>,
    mut mouse_button_events: MessageWriter<MouseButtonInput>,
    mut mouse_wheel_events: MessageWriter<MouseWheel>,
    mut keyboard_events: MessageWriter<KeyboardInput>,
    mut ime_events: MessageWriter<Ime>,
    mut window_events: MessageWriter<WindowEvent>,
) {
    let Ok((window_entity, mut window)) = windows.single_mut() else {
        return;
    };
    if runtime.cursor_position.is_none() {
        runtime.cursor_position = window.cursor_position();
    }

    for _ in 0..128 {
        if runtime.active.is_none() {
            let Some(request) = runtime.queue.pop_front() else {
                break;
            };
            status.active_request = Some(request.id.clone());
            status.current_action = None;
            status.last_error = None;
            runtime.active = Some(ActiveRequest {
                id: request.id,
                actions: request.actions,
                steps: VecDeque::new(),
            });
        }

        let needs_steps = runtime
            .active
            .as_ref()
            .is_some_and(|active| active.steps.is_empty());
        if needs_steps {
            let action = runtime
                .active
                .as_mut()
                .and_then(|active| active.actions.pop_front());
            let Some(action) = action else {
                complete_active_request(&mut runtime, &mut status);
                continue;
            };
            status.current_action = Some(action.name().to_string());
            match expand_action(action, &registry, runtime.cursor_position) {
                Ok(steps) => {
                    if let Some(active) = runtime.active.as_mut() {
                        active.steps = steps;
                    }
                }
                Err(error) => {
                    fail_active_request(&mut runtime, &mut status, error);
                    continue;
                }
            }
            if runtime
                .active
                .as_ref()
                .is_some_and(|active| active.steps.is_empty())
            {
                continue;
            }
        }

        let step = runtime
            .active
            .as_mut()
            .and_then(|active| active.steps.pop_front());
        let Some(step) = step else {
            continue;
        };

        let remaining = execute_step(
            step,
            window_entity,
            &mut window,
            &mut runtime.cursor_position,
            &mut cursor_events,
            &mut cursor_left_events,
            &mut mouse_button_events,
            &mut mouse_wheel_events,
            &mut keyboard_events,
            &mut ime_events,
            &mut window_events,
        );
        if let Some(step) = remaining
            && let Some(active) = runtime.active.as_mut()
        {
            active.steps.push_front(step);
        }
        break;
    }

    update_status(&runtime, &mut status);
}

fn complete_active_request(runtime: &mut InteractionRuntime, status: &mut AgentInteractionStatus) {
    let Some(active) = runtime.active.take() else {
        return;
    };
    status.completed_requests = status.completed_requests.wrapping_add(1);
    status.last_completed_request = Some(active.id);
    status.active_request = None;
    status.current_action = None;
}

fn fail_active_request(
    runtime: &mut InteractionRuntime,
    status: &mut AgentInteractionStatus,
    error: String,
) {
    let Some(active) = runtime.active.take() else {
        return;
    };
    status.last_failed_request = Some(active.id);
    status.last_error = Some(error);
    status.active_request = None;
    status.current_action = None;
}

fn update_status(runtime: &InteractionRuntime, status: &mut AgentInteractionStatus) {
    status.busy = runtime.active.is_some() || !runtime.queue.is_empty();
    status.queued_requests = runtime.queue.len();
    status.cursor_position = runtime.cursor_position;
}

fn expand_action(
    action: AgentAction,
    registry: &AgentTargetRegistry,
    cursor: Option<Vec2>,
) -> Result<VecDeque<InteractionStep>, String> {
    let mut steps = VecDeque::new();
    match action {
        AgentAction::Hover { point, frames } => {
            let position = resolve_target_point(registry, &point)?;
            push_move(&mut steps, cursor, position, 1);
            push_wait(&mut steps, frames);
        }
        AgentAction::MoveTo { point, frames } => {
            let position = resolve_target_point(registry, &point)?;
            push_move(&mut steps, cursor, position, frames);
        }
        AgentAction::MoveToPoint { position, frames } => {
            push_move(&mut steps, cursor, position, frames);
        }
        AgentAction::Click { point, button } => {
            let position = resolve_target_point(registry, &point)?;
            push_click(&mut steps, cursor, position, button);
        }
        AgentAction::ClickAt { position, button } => {
            push_click(&mut steps, cursor, position, button);
        }
        AgentAction::DoubleClick {
            point,
            button,
            gap_frames,
        } => {
            let position = resolve_target_point(registry, &point)?;
            push_move(&mut steps, cursor, position, 1);
            push_button(&mut steps, button, ButtonState::Pressed);
            push_button(&mut steps, button, ButtonState::Released);
            push_wait(&mut steps, gap_frames);
            push_button(&mut steps, button, ButtonState::Pressed);
            push_button(&mut steps, button, ButtonState::Released);
        }
        AgentAction::Press { point, button } => {
            let position = resolve_target_point(registry, &point)?;
            push_move(&mut steps, cursor, position, 1);
            push_button(&mut steps, button, ButtonState::Pressed);
        }
        AgentAction::PressAt { position, button } => {
            push_move(&mut steps, cursor, position, 1);
            push_button(&mut steps, button, ButtonState::Pressed);
        }
        AgentAction::Release { point, button } => {
            if let Some(point) = point {
                let position = resolve_target_point(registry, &point)?;
                push_move(&mut steps, cursor, position, 1);
            }
            push_button(&mut steps, button, ButtonState::Released);
        }
        AgentAction::DragTo {
            from,
            to,
            button,
            frames,
        } => {
            let from = resolve_target_point(registry, &from)?;
            let to = resolve_target_point(registry, &to)?;
            push_drag(&mut steps, cursor, from, to, button, frames);
        }
        AgentAction::DragBy {
            from,
            delta,
            button,
            frames,
        } => {
            let from = resolve_target_point(registry, &from)?;
            push_drag(&mut steps, cursor, from, from + delta, button, frames);
        }
        AgentAction::Scroll { point, delta, unit } => {
            let position = resolve_target_point(registry, &point)?;
            push_move(&mut steps, cursor, position, 1);
            steps.push_back(InteractionStep::Wheel { delta, unit });
        }
        AgentAction::ScrollAt {
            position,
            delta,
            unit,
        } => {
            push_move(&mut steps, cursor, position, 1);
            steps.push_back(InteractionStep::Wheel { delta, unit });
        }
        AgentAction::KeyTap { key } => {
            steps.push_back(InteractionStep::Key {
                key,
                state: ButtonState::Pressed,
            });
            steps.push_back(InteractionStep::Key {
                key,
                state: ButtonState::Released,
            });
        }
        AgentAction::KeyPress { key } => {
            steps.push_back(InteractionStep::Key {
                key,
                state: ButtonState::Pressed,
            });
        }
        AgentAction::KeyRelease { key } => {
            steps.push_back(InteractionStep::Key {
                key,
                state: ButtonState::Released,
            });
        }
        AgentAction::TextInput { point, text, mode } => {
            if let Some(point) = point {
                let position = resolve_target_point(registry, &point)?;
                push_click(&mut steps, cursor, position, AgentMouseButton::Left);
            }
            match mode {
                AgentTextInputMode::Keyboard => {
                    steps.push_back(InteractionStep::TextKeyboard {
                        text: text.clone(),
                        state: ButtonState::Pressed,
                    });
                    steps.push_back(InteractionStep::TextKeyboard {
                        text,
                        state: ButtonState::Released,
                    });
                }
                AgentTextInputMode::Ime => {
                    steps.push_back(InteractionStep::TextIme { text });
                }
            }
        }
        AgentAction::WaitFrames { frames } => push_wait(&mut steps, frames),
        AgentAction::CursorLeave => steps.push_back(InteractionStep::CursorLeave),
    }
    Ok(steps)
}

fn resolve_target_point(
    registry: &AgentTargetRegistry,
    point: &AgentTargetPoint,
) -> Result<Vec2, String> {
    let matches = registry
        .targets
        .iter()
        .filter(|target| target.id == point.target)
        .collect::<Vec<_>>();
    let target = match matches.as_slice() {
        [] => {
            let suggestions = registry
                .targets
                .iter()
                .filter(|target| {
                    target.id.contains(&point.target) || point.target.contains(&target.id)
                })
                .map(|target| target.id.as_str())
                .take(5)
                .collect::<Vec<_>>();
            if suggestions.is_empty() {
                return Err(format!("unknown agent target `{}`", point.target));
            }
            return Err(format!(
                "unknown agent target `{}`; similar targets: {}",
                point.target,
                suggestions.join(", ")
            ));
        }
        [target] => *target,
        _ => return Err(format!("agent target `{}` is ambiguous", point.target)),
    };
    if !target.visible {
        return Err(format!("agent target `{}` is not visible", point.target));
    }
    if !target.enabled {
        return Err(format!("agent target `{}` is disabled", point.target));
    }

    let anchor = point.anchor.clamp(Vec2::splat(-1.0), Vec2::splat(1.0));
    Ok(target.center + anchor * target.size * 0.5 + point.offset)
}

fn push_move(steps: &mut VecDeque<InteractionStep>, cursor: Option<Vec2>, to: Vec2, frames: u32) {
    steps.push_back(InteractionStep::Move {
        from: cursor.unwrap_or(to),
        to,
        index: 0,
        total: frames.max(1),
    });
}

fn push_wait(steps: &mut VecDeque<InteractionStep>, frames: u32) {
    if frames > 0 {
        steps.push_back(InteractionStep::Wait { remaining: frames });
    }
}

fn push_button(
    steps: &mut VecDeque<InteractionStep>,
    button: AgentMouseButton,
    state: ButtonState,
) {
    steps.push_back(InteractionStep::MouseButton { button, state });
}

fn push_click(
    steps: &mut VecDeque<InteractionStep>,
    cursor: Option<Vec2>,
    position: Vec2,
    button: AgentMouseButton,
) {
    push_move(steps, cursor, position, 1);
    push_button(steps, button, ButtonState::Pressed);
    push_button(steps, button, ButtonState::Released);
}

fn push_drag(
    steps: &mut VecDeque<InteractionStep>,
    cursor: Option<Vec2>,
    from: Vec2,
    to: Vec2,
    button: AgentMouseButton,
    frames: u32,
) {
    push_move(steps, cursor, from, 1);
    push_button(steps, button, ButtonState::Pressed);
    steps.push_back(InteractionStep::Move {
        from,
        to,
        index: 0,
        total: frames.max(1),
    });
    push_button(steps, button, ButtonState::Released);
}

/// 把 [`KeyCode`] 近似映射为逻辑 [`Key`]（及可打印字符），让用 `logical_key` / `text`
/// 判键的游戏也能被 `key` 动作驱动。覆盖字母 / 数字 / 空格 / 回车 / 方向键等常见键；
/// 其余按 `Unidentified` 处理（物理 `key_code` 始终准确，`Input<KeyCode>` 不受影响）。
fn logical_key_for(key: KeyCode) -> (Key, Option<char>) {
    use KeyCode::*;
    match key {
        // Bevy 0.20 text widgets read logical modifier state, not KeyCode.
        ControlLeft | ControlRight => (Key::Control, None),
        ShiftLeft | ShiftRight => (Key::Shift, None),
        AltLeft | AltRight => (Key::Alt, None),
        SuperLeft | SuperRight => (Key::Super, None),
        Delete => (Key::Delete, None),
        Home => (Key::Home, None),
        End => (Key::End, None),
        PageUp => (Key::PageUp, None),
        PageDown => (Key::PageDown, None),
        Insert => (Key::Insert, None),
        Space => (Key::Space, Some(' ')),
        Enter => (Key::Enter, None),
        Tab => (Key::Tab, Some('\t')),
        Backspace => (Key::Backspace, None),
        Escape => (Key::Escape, None),
        ArrowUp => (Key::ArrowUp, None),
        ArrowDown => (Key::ArrowDown, None),
        ArrowLeft => (Key::ArrowLeft, None),
        ArrowRight => (Key::ArrowRight, None),
        KeyA => (Key::Character("a".into()), Some('a')),
        KeyB => (Key::Character("b".into()), Some('b')),
        KeyC => (Key::Character("c".into()), Some('c')),
        KeyD => (Key::Character("d".into()), Some('d')),
        KeyE => (Key::Character("e".into()), Some('e')),
        KeyF => (Key::Character("f".into()), Some('f')),
        KeyG => (Key::Character("g".into()), Some('g')),
        KeyH => (Key::Character("h".into()), Some('h')),
        KeyI => (Key::Character("i".into()), Some('i')),
        KeyJ => (Key::Character("j".into()), Some('j')),
        KeyK => (Key::Character("k".into()), Some('k')),
        KeyL => (Key::Character("l".into()), Some('l')),
        KeyM => (Key::Character("m".into()), Some('m')),
        KeyN => (Key::Character("n".into()), Some('n')),
        KeyO => (Key::Character("o".into()), Some('o')),
        KeyP => (Key::Character("p".into()), Some('p')),
        KeyQ => (Key::Character("q".into()), Some('q')),
        KeyR => (Key::Character("r".into()), Some('r')),
        KeyS => (Key::Character("s".into()), Some('s')),
        KeyT => (Key::Character("t".into()), Some('t')),
        KeyU => (Key::Character("u".into()), Some('u')),
        KeyV => (Key::Character("v".into()), Some('v')),
        KeyW => (Key::Character("w".into()), Some('w')),
        KeyX => (Key::Character("x".into()), Some('x')),
        KeyY => (Key::Character("y".into()), Some('y')),
        KeyZ => (Key::Character("z".into()), Some('z')),
        Digit0 => (Key::Character("0".into()), Some('0')),
        Digit1 => (Key::Character("1".into()), Some('1')),
        Digit2 => (Key::Character("2".into()), Some('2')),
        Digit3 => (Key::Character("3".into()), Some('3')),
        Digit4 => (Key::Character("4".into()), Some('4')),
        Digit5 => (Key::Character("5".into()), Some('5')),
        Digit6 => (Key::Character("6".into()), Some('6')),
        Digit7 => (Key::Character("7".into()), Some('7')),
        Digit8 => (Key::Character("8".into()), Some('8')),
        Digit9 => (Key::Character("9".into()), Some('9')),
        _ => (Key::Unidentified(NativeKey::Unidentified), None),
    }
}

#[allow(clippy::too_many_arguments)]
fn execute_step(
    step: InteractionStep,
    window_entity: Entity,
    window: &mut Window,
    cursor_position: &mut Option<Vec2>,
    cursor_events: &mut MessageWriter<CursorMoved>,
    cursor_left_events: &mut MessageWriter<CursorLeft>,
    mouse_button_events: &mut MessageWriter<MouseButtonInput>,
    mouse_wheel_events: &mut MessageWriter<MouseWheel>,
    keyboard_events: &mut MessageWriter<KeyboardInput>,
    ime_events: &mut MessageWriter<Ime>,
    window_events: &mut MessageWriter<WindowEvent>,
) -> Option<InteractionStep> {
    match step {
        InteractionStep::Move {
            from,
            to,
            index,
            total,
        } => {
            let next = index + 1;
            let position = from.lerp(to, next as f32 / total as f32);
            let event = CursorMoved {
                window: window_entity,
                position,
                delta: cursor_position.map(|previous| position - previous),
            };
            window.set_cursor_position(Some(position));
            *cursor_position = Some(position);
            cursor_events.write(event.clone());
            window_events.write(WindowEvent::CursorMoved(event));
            (next < total).then_some(InteractionStep::Move {
                from,
                to,
                index: next,
                total,
            })
        }
        InteractionStep::Wait { remaining } => (remaining > 1).then_some(InteractionStep::Wait {
            remaining: remaining - 1,
        }),
        InteractionStep::MouseButton { button, state } => {
            let event = MouseButtonInput {
                button: button.into(),
                state,
                window: window_entity,
            };
            mouse_button_events.write(event);
            window_events.write(WindowEvent::MouseButtonInput(event));
            None
        }
        InteractionStep::Wheel { delta, unit } => {
            let event = MouseWheel {
                unit: unit.into(),
                x: delta.x,
                y: delta.y,
                window: window_entity,
                phase: TouchPhase::Moved,
            };
            mouse_wheel_events.write(event);
            window_events.write(WindowEvent::MouseWheel(event));
            None
        }
        InteractionStep::Key { key, state } => {
            let (logical_key, text_char) = logical_key_for(key);
            let event = KeyboardInput {
                key_code: key,
                logical_key,
                text: if state == ButtonState::Pressed {
                    text_char.map(|character| character.to_string().into())
                } else {
                    None
                },
                state,
                repeat: false,
                window: window_entity,
            };
            keyboard_events.write(event.clone());
            window_events.write(WindowEvent::KeyboardInput(event));
            None
        }
        InteractionStep::TextKeyboard { text, state } => {
            let event = KeyboardInput {
                key_code: KeyCode::Unidentified(NativeKeyCode::Unidentified),
                logical_key: Key::Character(text.clone().into()),
                state,
                text: (state == ButtonState::Pressed).then(|| text.into()),
                repeat: false,
                window: window_entity,
            };
            keyboard_events.write(event.clone());
            window_events.write(WindowEvent::KeyboardInput(event));
            None
        }
        InteractionStep::TextIme { text } => {
            let event = Ime::Commit {
                window: window_entity,
                value: text,
            };
            ime_events.write(event.clone());
            window_events.write(WindowEvent::Ime(event));
            None
        }
        InteractionStep::CursorLeave => {
            let event = CursorLeft {
                window: window_entity,
            };
            window.set_cursor_position(None);
            *cursor_position = None;
            cursor_left_events.write(event.clone());
            window_events.write(WindowEvent::CursorLeft(event));
            None
        }
    }
}

#[allow(clippy::type_complexity)]
pub(crate) fn sync_target_registry(
    config: Res<HarnessConfig>,
    windows: Query<(Entity, &Window), With<PrimaryWindow>>,
    cameras: Query<(&Camera, &RenderTarget, &GlobalTransform)>,
    targets: Query<(
        Entity,
        &AgentTarget,
        Option<&AgentTargetDisabled>,
        Option<&GlobalTransform>,
        Option<&ComputedNode>,
        Option<&UiGlobalTransform>,
        Option<&InheritedVisibility>,
    )>,
    mut registry: ResMut<AgentTargetRegistry>,
) {
    let Ok((window_entity, window)) = windows.single() else {
        return;
    };
    let window_size = Vec2::new(window.width(), window.height());
    let window_camera = cameras
        .iter()
        .filter(|(camera, target, _)| {
            camera.is_active
                && match target {
                    RenderTarget::Window(WindowRef::Primary) => true,
                    RenderTarget::Window(WindowRef::Entity(entity)) => *entity == window_entity,
                    _ => false,
                }
        })
        .max_by_key(|(camera, _, _)| camera.order)
        .map(|(camera, _, transform)| (camera, transform));

    let mut next_targets = Vec::new();
    let mut errors = Vec::new();
    for (entity, target, disabled, global, computed, ui_global, inherited_visibility) in &targets {
        if target.id.trim().is_empty() {
            errors.push(format!("entity {entity} has an empty AgentTarget id"));
            continue;
        }

        let bounds = match target.space {
            AgentTargetSpace::Window => target
                .size
                .map(|size| (target.offset, size.abs()))
                .ok_or_else(|| "window target has no size".to_string()),
            AgentTargetSpace::Ui => ui_bounds(target, computed, ui_global),
            AgentTargetSpace::World => world_bounds(
                target,
                global,
                config.0.canvas.as_ref(),
                window_size,
                window_camera,
            ),
            AgentTargetSpace::Auto => {
                if computed.is_some() && ui_global.is_some() {
                    ui_bounds(target, computed, ui_global)
                } else {
                    world_bounds(
                        target,
                        global,
                        config.0.canvas.as_ref(),
                        window_size,
                        window_camera,
                    )
                }
            }
        };
        let (center, size) = match bounds {
            Ok(bounds) => bounds,
            Err(error) => {
                errors.push(format!("target `{}`: {error}", target.id));
                continue;
            }
        };

        next_targets.push(AgentTargetInfo {
            id: target.id.clone(),
            label: target.label.clone(),
            role: target.role,
            capabilities: target.capabilities.clone(),
            entity: entity.to_bits(),
            center,
            size,
            visible: inherited_visibility.is_none_or(|visibility| visibility.get()),
            enabled: disabled.is_none(),
        });
    }

    next_targets.sort_by(|left, right| left.id.cmp(&right.id).then(left.entity.cmp(&right.entity)));
    let mut counts = HashMap::<&str, usize>::new();
    for target in &next_targets {
        *counts.entry(&target.id).or_default() += 1;
    }
    let mut duplicate_ids = counts
        .into_iter()
        .filter_map(|(id, count)| (count > 1).then_some((id, count)))
        .collect::<Vec<_>>();
    duplicate_ids.sort_unstable_by_key(|(id, _)| *id);
    errors.extend(
        duplicate_ids
            .into_iter()
            .map(|(id, count)| format!("duplicate target id `{id}` appears {count} times")),
    );
    errors.sort();

    if registry.targets != next_targets || registry.errors != errors {
        registry.revision = registry.revision.wrapping_add(1);
        registry.targets = next_targets;
        registry.errors = errors;
    }
}

fn ui_bounds(
    target: &AgentTarget,
    computed: Option<&ComputedNode>,
    ui_global: Option<&UiGlobalTransform>,
) -> Result<(Vec2, Vec2), String> {
    let computed = computed.ok_or_else(|| "missing ComputedNode".to_string())?;
    let ui_global = ui_global.ok_or_else(|| "missing UiGlobalTransform".to_string())?;
    let inverse_scale = computed.inverse_scale_factor();
    let physical_size = target
        .size
        .map(|size| size / inverse_scale)
        .unwrap_or_else(|| computed.size());
    let physical_offset = target.offset / inverse_scale;
    let half = physical_size * 0.5;
    let points = [
        physical_offset + Vec2::new(-half.x, -half.y),
        physical_offset + Vec2::new(half.x, -half.y),
        physical_offset + Vec2::new(half.x, half.y),
        physical_offset + Vec2::new(-half.x, half.y),
    ]
    .map(|point| ui_global.transform_point2(point) * inverse_scale);
    Ok(aabb(points))
}

fn world_bounds(
    target: &AgentTarget,
    global: Option<&GlobalTransform>,
    canvas: Option<&AgentCanvasConfig>,
    window_size: Vec2,
    camera: Option<(&Camera, &GlobalTransform)>,
) -> Result<(Vec2, Vec2), String> {
    let global = global.ok_or_else(|| "missing GlobalTransform".to_string())?;
    let size = target
        .size
        .ok_or_else(|| "world target has no size".to_string())?;
    let half = size * 0.5;
    let world_points = [
        target.offset + Vec2::new(-half.x, -half.y),
        target.offset + Vec2::new(half.x, -half.y),
        target.offset + Vec2::new(half.x, half.y),
        target.offset + Vec2::new(-half.x, half.y),
    ]
    .map(|point| global.transform_point(point.extend(0.0)));

    let points = if let Some(canvas) = canvas {
        world_points.map(|point| canvas.world_to_window(point.truncate(), window_size))
    } else {
        let (camera, camera_transform) =
            camera.ok_or_else(|| "no active window camera and no canvas mapping".to_string())?;
        let mut projected = [Vec2::ZERO; 4];
        for (output, world) in projected.iter_mut().zip(world_points) {
            *output = camera
                .world_to_viewport(camera_transform, world)
                .map_err(|error| format!("camera projection failed: {error}"))?;
        }
        projected
    };
    Ok(aabb(points))
}

fn aabb(points: [Vec2; 4]) -> (Vec2, Vec2) {
    let mut min = points[0];
    let mut max = points[0];
    for point in points.into_iter().skip(1) {
        min = min.min(point);
        max = max.max(point);
    }
    ((min + max) * 0.5, max - min)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifier_and_navigation_injection_preserves_logical_keys() {
        for (physical, logical) in [
            (KeyCode::ControlLeft, Key::Control),
            (KeyCode::ControlRight, Key::Control),
            (KeyCode::ShiftLeft, Key::Shift),
            (KeyCode::ShiftRight, Key::Shift),
            (KeyCode::AltLeft, Key::Alt),
            (KeyCode::AltRight, Key::Alt),
            (KeyCode::SuperLeft, Key::Super),
            (KeyCode::SuperRight, Key::Super),
            (KeyCode::Delete, Key::Delete),
            (KeyCode::Home, Key::Home),
            (KeyCode::End, Key::End),
            (KeyCode::PageUp, Key::PageUp),
            (KeyCode::PageDown, Key::PageDown),
            (KeyCode::Insert, Key::Insert),
        ] {
            assert_eq!(logical_key_for(physical), (logical, None));
        }
        assert_eq!(
            logical_key_for(KeyCode::KeyA),
            (Key::Character("a".into()), Some('a'))
        );
    }

    #[test]
    fn integer_canvas_mapping_preserves_letterbox_center() {
        let canvas = AgentCanvasConfig::centered(Vec2::new(640.0, 360.0));
        let window = Vec2::new(1280.0, 800.0);

        assert_eq!(
            canvas.world_to_window(Vec2::ZERO, window),
            Vec2::new(640.0, 400.0)
        );
        assert_eq!(
            canvas.world_to_window(Vec2::new(-100.0, 50.0), window),
            Vec2::new(440.0, 300.0)
        );
    }

    #[test]
    fn target_anchor_resolves_inside_registry_bounds() {
        let registry = AgentTargetRegistry {
            targets: vec![AgentTargetInfo {
                id: "button.retry".into(),
                label: None,
                role: AgentTargetRole::Button,
                capabilities: vec![AgentTargetCapability::Click],
                entity: 1,
                center: Vec2::new(100.0, 80.0),
                size: Vec2::new(40.0, 20.0),
                visible: true,
                enabled: true,
            }],
            ..default()
        };

        let point = AgentTargetPoint::center("button.retry").at_anchor(Vec2::new(1.0, -1.0));
        assert_eq!(
            resolve_target_point(&registry, &point).unwrap(),
            Vec2::new(120.0, 70.0)
        );
    }
}
