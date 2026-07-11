//! 背景：天空色带 + 漂移云朵 + 漂移星星。
//!
//! 对应原作 `update(dt)` 里的 bg 绘制 + `main.clouds` / `main.stars`。
//! 原作：上 150px 是 colors.blue[10]，中间 60px 渐变到底色，底部 150px 是 colors.fg[0]；
//! 云朵（cloud.png）按 10×3 网格水平漂移，星星（star_gray.png）按 10×8 网格斜向上漂移。
//!
//! 渲染在 BG_LAYER（由 BgCamera 画到低分辨率 canvas_bg，pixel-perfect），不描边、不投影。

use bevy::{
    asset::RenderAssetUsages,
    image::Image,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use crate::game::GameState;
use crate::{BG_LAYER, DistanceToTop, SCREEN_H, SCREEN_W};

/// colors.fg[0] = #E7E8E9（近白）。
const FG0: [f32; 4] = [0.905882, 0.909804, 0.913725, 1.0];
/// colors.fg[10]：twitter_emoji fg 经过 color_ramp(+0.25)，原作背景星星用它。
const FG10: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
/// colors.blue[10]：twitter_emoji blue(#53AFEF) 经 HSL lighten(+0.25)。
const BLUE10: [f32; 4] = [0.782937, 0.899044, 0.979808, 1.0];

#[derive(Resource)]
struct BgPalettes {
    playing: Handle<Image>,
    game_over: Handle<Image>,
}

#[derive(Component)]
struct BgCloud {
    x_offset: f32,
}

#[derive(Component)]
struct BgStar;

#[derive(Component)]
struct BgGradient;

pub struct BackgroundPlugin;

impl Plugin for BackgroundPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_background)
            .add_systems(Update, (drift_clouds, drift_stars, update_fail_background));
    }
}

fn setup_background(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    asset_server: Res<AssetServer>,
) {
    let playing = images.add(background_image(BLUE10));
    let game_over = images.add(background_image(FG0));
    commands.insert_resource(BgPalettes {
        playing: playing.clone(),
        game_over,
    });

    commands.spawn((
        Sprite {
            image: playing,
            custom_size: Some(Vec2::new(SCREEN_W, SCREEN_H)),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, -2.0),
        BgGradient,
        BG_LAYER,
    ));

    // 星星：原作 8×10 网格，偶数行错半格，初始在屏幕坐标系内/外循环。
    let cell_w = SCREEN_W / 8.0;
    let cell_h = SCREEN_H / 6.0;
    for j in 0..8 {
        let x_offset = if j % 2 == 1 { cell_w / 2.0 } else { 0.0 };
        for i in 0..10 {
            let sx = i as f32 * cell_w + x_offset;
            let sy = j as f32 * cell_h;
            commands.spawn((
                Sprite {
                    image: asset_server.load("star_gray.png"),
                    custom_size: Some(Vec2::splat(32.0)),
                    color: Color::srgba(FG10[0], FG10[1], FG10[2], FG10[3]),
                    ..default()
                },
                Transform::from_xyz(screen_x_to_world(sx), screen_y_to_world(sy), -1.8)
                    .with_rotation(Quat::from_rotation_z(
                        rand::random::<f32>() * std::f32::consts::TAU,
                    )),
                BgStar,
                BG_LAYER,
            ));
        }
    }

    // 云朵：原作 3×10 网格，偶数行错半格，固定 32px，向右漂。
    for j in 0..3 {
        let x_offset = if j % 2 == 1 { cell_w / 2.0 } else { 0.0 };
        for i in 0..10 {
            let sx = i as f32 * cell_w + x_offset;
            let sy = j as f32 * cell_h + 14.0;
            commands.spawn((
                Sprite {
                    image: asset_server.load("cloud.png"),
                    custom_size: Some(Vec2::splat(32.0)),
                    flip_x: (i + j) % 2 == 0,
                    ..default()
                },
                Transform::from_xyz(screen_x_to_world(sx), screen_y_to_world(sy), -1.7),
                BgCloud { x_offset },
                BG_LAYER,
            ));
        }
    }
}

fn background_image(top_color: [f32; 4]) -> Image {
    let h = 360usize;
    let mut data = vec![0u8; h * 4];
    for (y, row) in data.chunks_exact_mut(4).enumerate() {
        let sy = y as f32;
        let color = if sy < 150.0 {
            top_color
        } else if sy < 210.0 {
            let t = ((sy - 150.0) / 60.0).clamp(0.0, 1.0);
            [
                top_color[0] * (1.0 - t) + FG0[0] * t,
                top_color[1] * (1.0 - t) + FG0[1] * t,
                top_color[2] * (1.0 - t) + FG0[2] * t,
                top_color[3] * (1.0 - t) + FG0[3] * t,
            ]
        } else {
            FG0
        };
        row[0] = (color[0] * 255.0).round().clamp(0.0, 255.0) as u8;
        row[1] = (color[1] * 255.0).round().clamp(0.0, 255.0) as u8;
        row[2] = (color[2] * 255.0).round().clamp(0.0, 255.0) as u8;
        row[3] = (color[3] * 255.0).round().clamp(0.0, 255.0) as u8;
    }
    Image::new(
        Extent3d {
            width: 1,
            height: h as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

fn screen_x_to_world(sx: f32) -> f32 {
    sx - SCREEN_W / 2.0
}

/// 原作屏幕坐标（左上原点、y 向下）→ 世界坐标（居中原点、y 向上）。
fn screen_y_to_world(sy: f32) -> f32 {
    SCREEN_H / 2.0 - sy
}

fn drift_clouds(time: Res<Time>, mut clouds: Query<(&BgCloud, &mut Transform)>) {
    let dt = time.delta_secs();
    for (cloud, mut tf) in &mut clouds {
        let mut sx = tf.translation.x + SCREEN_W / 2.0;
        sx += 10.0 * dt;
        if sx >= SCREEN_W + SCREEN_W / 8.0 + cloud.x_offset {
            sx = -SCREEN_W / 8.0 + cloud.x_offset;
        }
        tf.translation.x = screen_x_to_world(sx);
    }
}

fn drift_stars(
    time: Res<Time>,
    distance: Res<DistanceToTop>,
    mut stars: Query<(&BgStar, &mut Transform, &mut Sprite)>,
) {
    let dt = time.delta_secs();
    let angle = std::f32::consts::PI + std::f32::consts::FRAC_PI_6;
    let pressure = (1.0 - (**distance / 294.0).clamp(0.0, 1.0)).clamp(0.0, 1.0);
    let v = 4.0 + pressure * 12.0;
    let vr = (-0.05 - pressure * 0.15) * std::f32::consts::PI;
    for (_star, mut tf, mut sprite) in &mut stars {
        let mut sx = tf.translation.x + SCREEN_W / 2.0;
        let mut sy = SCREEN_H / 2.0 - tf.translation.y;
        sx += v * angle.cos() * dt;
        sy += v * angle.sin() * dt;
        tf.rotate_z(vr * dt);
        if sx <= -80.0 {
            sx = SCREEN_W + 80.0;
        }
        if sy <= -60.0 {
            sy = SCREEN_H + 60.0;
        }
        let alpha = if sy < SCREEN_H - 120.0 {
            ((sy - (SCREEN_H - 120.0) + 60.0) / 60.0).clamp(0.0, 1.0)
        } else {
            1.0
        };
        sprite.color = sprite.color.with_alpha(alpha);
        tf.translation.x = screen_x_to_world(sx);
        tf.translation.y = screen_y_to_world(sy);
    }
}

fn update_fail_background(
    state: Res<GameState>,
    palettes: Res<BgPalettes>,
    asset_server: Res<AssetServer>,
    mut gradient: Single<&mut Sprite, With<BgGradient>>,
    mut clouds: Query<&mut Sprite, (With<BgCloud>, Without<BgGradient>)>,
) {
    let failed = *state == GameState::GameOver;
    if !state.is_changed() {
        return;
    }
    gradient.image = if failed {
        palettes.game_over.clone()
    } else {
        palettes.playing.clone()
    };
    let image = if failed {
        asset_server.load("cloud_gray.png")
    } else {
        asset_server.load("cloud.png")
    };
    for mut sprite in &mut clouds {
        sprite.image = image.clone();
    }
}
