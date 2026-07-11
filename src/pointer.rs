use bevy::{
    input::{
        InputSystems,
        mouse::MouseButtonInput,
        touch::{Touch, Touches},
    },
    prelude::*,
    window::{CursorMoved, PrimaryWindow},
};

use crate::{SCREEN_H, SCREEN_W, web};

const SYNTHETIC_MOUSE_SUPPRESSION_FRAMES: u8 = 12;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PointerKind {
    #[default]
    Unknown,
    Mouse,
    Touch,
}

#[derive(Resource, Debug)]
pub struct PrimaryPointer {
    kind: PointerKind,
    touch_capable: bool,
    active_touch_id: Option<u64>,
    position: Option<Vec2>,
    delta: Vec2,
    pressed: bool,
    just_pressed: bool,
    just_released: bool,
    suppress_mouse_frames: u8,
}

impl Default for PrimaryPointer {
    fn default() -> Self {
        Self {
            kind: PointerKind::Unknown,
            touch_capable: web::touch_capable(),
            active_touch_id: None,
            position: None,
            delta: Vec2::ZERO,
            pressed: false,
            just_pressed: false,
            just_released: false,
            suppress_mouse_frames: 0,
        }
    }
}

impl PrimaryPointer {
    pub fn kind(&self) -> PointerKind {
        self.kind
    }

    pub fn is_touch(&self) -> bool {
        self.kind == PointerKind::Touch
            || (self.kind == PointerKind::Unknown && self.touch_capable())
    }

    pub fn touch_capable(&self) -> bool {
        self.touch_capable
    }

    pub fn position(&self) -> Option<Vec2> {
        self.position
    }

    pub fn interaction_position(&self) -> Option<Vec2> {
        match self.kind {
            PointerKind::Touch if !self.pressed && !self.just_pressed => None,
            PointerKind::Unknown => None,
            _ => self.position,
        }
    }

    pub fn delta(&self) -> Vec2 {
        self.delta
    }

    pub fn pressed(&self) -> bool {
        self.pressed
    }

    pub fn just_pressed(&self) -> bool {
        self.just_pressed
    }
}

#[derive(Resource, Clone, Copy, Debug)]
pub struct CanvasViewport {
    logical_size: Vec2,
    window_size: Vec2,
    top_left: Vec2,
    rendered_size: Vec2,
    scale: f32,
}

impl Default for CanvasViewport {
    fn default() -> Self {
        Self {
            logical_size: Vec2::new(SCREEN_W, SCREEN_H),
            window_size: Vec2::new(SCREEN_W, SCREEN_H),
            top_left: Vec2::ZERO,
            rendered_size: Vec2::new(SCREEN_W, SCREEN_H),
            scale: 1.0,
        }
    }
}

impl CanvasViewport {
    pub fn update(&mut self, window_size: Vec2, continuous_scale: bool) {
        self.window_size = window_size.max(Vec2::splat(f32::EPSILON));
        let fit = (self.window_size.x / self.logical_size.x)
            .min(self.window_size.y / self.logical_size.y);
        self.scale = if fit >= 1.0 && !continuous_scale {
            fit.floor()
        } else {
            fit.max(f32::EPSILON)
        };
        self.rendered_size = self.logical_size * self.scale;
        self.top_left = (self.window_size - self.rendered_size) * 0.5;
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }

    pub fn contains_window_point(&self, position: Vec2) -> bool {
        position.cmpge(self.top_left).all()
            && position.cmple(self.top_left + self.rendered_size).all()
    }

    pub fn window_to_world(&self, position: Vec2) -> Vec2 {
        let canvas = (position - self.top_left) / self.scale;
        Vec2::new(
            canvas.x - self.logical_size.x * 0.5,
            self.logical_size.y * 0.5 - canvas.y,
        )
    }

    pub fn touch_hit_half_extent(&self) -> f32 {
        22.0 / self.scale.max(f32::EPSILON)
    }
}

#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone)]
pub enum PointerSystems {
    Update,
}

pub struct PointerPlugin;

impl Plugin for PointerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PrimaryPointer>()
            .init_resource::<CanvasViewport>()
            .configure_sets(PreUpdate, PointerSystems::Update.after(InputSystems))
            .add_systems(
                PreUpdate,
                update_primary_pointer.in_set(PointerSystems::Update),
            );
    }
}

fn update_primary_pointer(
    window: Single<(Entity, &Window), With<PrimaryWindow>>,
    touches: Res<Touches>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut cursor_events: MessageReader<CursorMoved>,
    mut mouse_button_events: MessageReader<MouseButtonInput>,
    mut pointer: ResMut<PrimaryPointer>,
) {
    let (window_entity, window) = *window;
    let input_scale = input_position_scale(window);
    let previous_position = pointer.position;
    pointer.delta = Vec2::ZERO;
    pointer.just_pressed = false;
    pointer.just_released = false;

    let mut moved_position = None;
    for event in cursor_events.read() {
        if event.window == window_entity {
            moved_position = Some(event.position / input_scale);
        }
    }
    let mouse_button_changed = mouse_button_events
        .read()
        .any(|event| event.window == window_entity);

    let mut handled_touch = update_active_touch(&touches, &mut pointer, input_scale);
    if !handled_touch
        && pointer.active_touch_id.is_none()
        && let Some(touch) = touches.iter_just_pressed().min_by_key(|touch| touch.id())
    {
        begin_touch(touch, &touches, &mut pointer, input_scale);
        handled_touch = true;
    }
    if !handled_touch
        && pointer.active_touch_id.is_none()
        && let Some(touch) = touches.iter().min_by_key(|touch| touch.id())
    {
        pointer.kind = PointerKind::Touch;
        pointer.touch_capable = true;
        pointer.active_touch_id = Some(touch.id());
        pointer.position = Some(touch.position() / input_scale);
        pointer.pressed = true;
        handled_touch = true;
    }

    if handled_touch {
        pointer.suppress_mouse_frames = SYNTHETIC_MOUSE_SUPPRESSION_FRAMES;
    } else {
        pointer.suppress_mouse_frames = pointer.suppress_mouse_frames.saturating_sub(1);
        let mouse_activity = moved_position.is_some() || mouse_button_changed;
        if pointer.suppress_mouse_frames == 0
            && (mouse_activity || pointer.kind == PointerKind::Mouse)
        {
            if mouse_activity {
                pointer.kind = PointerKind::Mouse;
            }
            if pointer.kind == PointerKind::Mouse {
                pointer.position = moved_position.or_else(|| {
                    window
                        .cursor_position()
                        .map(|position| position / input_scale)
                });
                pointer.pressed = mouse.pressed(MouseButton::Left);
                pointer.just_pressed = mouse.just_pressed(MouseButton::Left);
                pointer.just_released = mouse.just_released(MouseButton::Left);
            }
        }
    }

    if let (Some(previous), Some(current)) = (previous_position, pointer.position) {
        pointer.delta = current - previous;
    }
}

fn update_active_touch(touches: &Touches, pointer: &mut PrimaryPointer, input_scale: f32) -> bool {
    let Some(id) = pointer.active_touch_id else {
        return false;
    };
    pointer.kind = PointerKind::Touch;
    pointer.touch_capable = true;

    if let Some(touch) = touches.get_pressed(id) {
        pointer.position = Some(touch.position() / input_scale);
        pointer.pressed = true;
        pointer.just_pressed = touches.just_pressed(id);
        return true;
    }
    if let Some(touch) = touches.get_released(id) {
        pointer.position = Some(touch.position() / input_scale);
        pointer.pressed = false;
        pointer.just_released = true;
        pointer.active_touch_id = None;
        return true;
    }
    if touches.just_canceled(id) {
        pointer.pressed = false;
        pointer.just_released = true;
        pointer.active_touch_id = None;
        return true;
    }

    pointer.pressed = false;
    pointer.active_touch_id = None;
    false
}

fn begin_touch(touch: &Touch, touches: &Touches, pointer: &mut PrimaryPointer, input_scale: f32) {
    let id = touch.id();
    pointer.kind = PointerKind::Touch;
    pointer.touch_capable = true;
    pointer.active_touch_id = Some(id);
    pointer.position = Some(touch.position() / input_scale);
    pointer.just_pressed = true;
    pointer.pressed = touches.get_pressed(id).is_some();
    if let Some(released) = touches.get_released(id) {
        pointer.position = Some(released.position() / input_scale);
        pointer.just_released = true;
        pointer.pressed = false;
        pointer.active_touch_id = None;
    }
}

fn input_position_scale(window: &Window) -> f32 {
    #[cfg(target_arch = "wasm32")]
    {
        let css_width = web::canvas_client_width();
        let physical_width = window.physical_width() as f32;
        if css_width > 0.0 && physical_width > 0.0 {
            return (web::device_pixel_ratio() * css_width / physical_width).max(f32::EPSILON);
        }
        web::device_pixel_ratio().max(f32::EPSILON)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = window;
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_upscale_stays_integer_sharp() {
        let mut viewport = CanvasViewport::default();
        viewport.update(Vec2::new(844.0, 390.0), false);

        assert_eq!(viewport.scale(), 1.0);
    }

    #[test]
    fn touch_upscale_uses_the_available_short_edge() {
        let mut viewport = CanvasViewport::default();
        viewport.update(Vec2::new(844.0, 390.0), true);

        assert!((viewport.scale() - 390.0 / SCREEN_H).abs() < 0.0001);
        assert!((viewport.rendered_size.y - 390.0).abs() < 0.0001);
    }
}
