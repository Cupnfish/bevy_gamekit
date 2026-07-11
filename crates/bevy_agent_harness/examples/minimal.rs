//! 最小示例：装 `AgentHarnessPlugin`，放一个可点的世界空间按钮（`AgentTarget`），
//! 左键点击累加一个反射资源 `Clicks`，演示「目标发现 + 注入点击 + BRP 读取状态」。
//!
//! 运行：`cargo run --example minimal --package bevy_agent_harness`
//! 另开终端对跑中的示例发号施令：
//! ```text
//! bevy-agent targets
//! bevy-agent click button.demo
//! bevy-agent get minimal::Clicks
//! bevy-agent wait-for minimal::Clicks 0 ge --op ge        # 等到点击数 >= N
//! ```

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::WindowResolution;
use bevy_agent_harness::{
    AgentCanvasConfig, AgentHarnessConfig, AgentHarnessPlugin, AgentTarget, AgentTargetCapability,
    AgentTargetRole,
};

/// 反射资源：记录按钮被点了几次（经 BRP `world.get_resources` 可读，类型路径 `minimal::Clicks`）。
#[derive(Resource, Reflect, Default)]
#[reflect(Resource)]
struct Clicks(u32);

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "bevy_agent_harness — minimal".into(),
                resolution: WindowResolution::new(640, 360),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(AgentHarnessPlugin::new(AgentHarnessConfig {
            canvas: Some(AgentCanvasConfig::centered(Vec2::new(640.0, 360.0))),
            ..default()
        }))
        .register_type::<Clicks>()
        .init_resource::<Clicks>()
        .add_systems(Startup, setup)
        .add_systems(Update, count_clicks)
        .run();
}

fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.spawn(Camera2d);
    // 1×1 纯白贴图，按 custom_size 拉成彩色按钮。
    let white = images.add(Image::new(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        vec![255u8, 255, 255, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    ));
    commands.spawn((
        Sprite {
            image: white,
            color: Color::srgb(0.3, 0.7, 1.0),
            custom_size: Some(Vec2::new(160.0, 80.0)),
            ..default()
        },
        Transform::default(),
        AgentTarget::world("button.demo", Vec2::new(160.0, 80.0))
            .with_role(AgentTargetRole::Button)
            .with_capabilities([AgentTargetCapability::Click]),
    ));
    info!(
        "agent harness minimal ready — in another terminal:\n  \
         bevy-agent targets\n  \
         bevy-agent click button.demo\n  \
         bevy-agent get minimal::Clicks"
    );
}

/// harness 注入的点击会更新 `ButtonInput<MouseButton>`，这里统计左键按下。
fn count_clicks(mouse: Res<ButtonInput<MouseButton>>, mut clicks: ResMut<Clicks>) {
    if mouse.just_pressed(MouseButton::Left) {
        clicks.0 += 1;
        info!("clicks = {}", clicks.0);
    }
}
