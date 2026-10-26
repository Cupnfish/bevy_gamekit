//! Small BSN scene helpers for static sprite entities.
//!
//! This keeps the first BSN pass conservative: scene notation owns the
//! declarative render components, while runtime-only gameplay components are
//! still inserted by the caller.

use bevy::{camera::visibility::RenderLayers, prelude::*, sprite::Anchor};

pub fn sprite_scene(
    image: &'static str,
    size: Vec2,
    transform: Transform,
    layers: RenderLayers,
) -> impl Scene {
    bsn! {
        Sprite {
            image: { image },
            custom_size: { Some(size) },
        }
        transform
        layers
    }
}

pub fn anchored_sprite_scene(
    image: &'static str,
    size: Vec2,
    transform: Transform,
    anchor: Anchor,
    layers: RenderLayers,
) -> impl Scene {
    bsn! {
        Sprite {
            image: { image },
            custom_size: { Some(size) },
        }
        anchor
        transform
        layers
    }
}

pub fn colored_sprite_scene(
    color: Color,
    size: Vec2,
    transform: Transform,
    layers: RenderLayers,
) -> impl Scene {
    bsn! {
        Sprite {
            color: { color },
            custom_size: { Some(size) },
        }
        transform
        layers
    }
}
