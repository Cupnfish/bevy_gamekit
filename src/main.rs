//! emoji-merge 复刻 — 像素相机 + avian 物理 + Juice + Suika 合并 + 失败/重启 + 音频。
//!
//! 坐标系：世界居中原点、y 向上。原作用 640×360 屏幕坐标（左上原点、y 向下），
//! 用 `screen_x` / `screen_y` 换算。所有布局数值取自原作 `main.lua`。
//!
//! 手持球浮在 spawner 顶部跟随鼠标（Held，非物理），点击下落成真实物理 Emoji，
//! 再立即补一只新 Held。同级相撞合并升阶并加分，堆过失败线 → GameOver（R 重启）。
//! 美术/音频素材全部复用原版 emoji-merge/assets。

mod audio;
mod background;
mod boards;
mod game;
mod juice;
mod outline;
mod pointer;
mod scenes;
mod web;

#[cfg(all(target_arch = "wasm32", feature = "minigame", feature = "web_audio"))]
compile_error!("Mini Game builds require `--no-default-features --features douyin|wechat`");
#[cfg(all(target_arch = "wasm32", feature = "douyin", feature = "wechat"))]
compile_error!("The `douyin` and `wechat` features are mutually exclusive");

use avian2d::{math::*, prelude::*};
#[cfg(feature = "web_audio")]
use bevy::audio::{AudioPlayer, PlaybackSettings, Volume};
#[cfg(not(target_arch = "wasm32"))]
use bevy::window::{MonitorSelection, WindowMode};
use bevy::{
    app::AppExit,
    asset::{AssetMetaCheck, RenderAssetUsages},
    camera::{ClearColorConfig, RenderTarget, visibility::RenderLayers},
    ecs::system::ParamSet,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    window::{CursorOptions, WindowResolution},
};
use bevy_agent_harness::{
    AgentCanvasConfig, AgentHarnessConfig, AgentHarnessPlugin, AgentTarget, AgentTargetCapability,
    AgentTargetRole,
};
#[cfg(all(target_arch = "wasm32", feature = "minigame"))]
use bevy_minigame::MiniGamePlugin;

#[cfg(feature = "web_audio")]
use audio::Bgm;
#[cfg(all(target_arch = "wasm32", feature = "minigame"))]
use audio::WasmBgm;
use audio::{GameSound, SoundLevels, play_sfx};
use game::{
    ArenaSolid, Best, EMOJI_DATA, Emoji, GamePlugin, GameState, NextValue, RoundEndJointGroup,
    RoundEndPiece, RoundEndPieceKind, RoundEndSolidForeground, SPAWN_Y, SaveData, Shake,
    SpawnGrace, choose_spawn_value, emoji_filename, emoji_radius, save_save, spawn_emoji,
};
use juice::{HitFlash, JuicePlugin, Spring, update_springs};
use outline::{OutlinePlugin, OutlinePostProcess};
use pointer::{CanvasViewport, PointerKind, PointerPlugin, PointerSystems, PrimaryPointer};

pub const RES_WIDTH: u32 = 640;
pub const RES_HEIGHT: u32 = 360;
pub const SCREEN_W: f32 = 640.0;
pub const SCREEN_H: f32 = 360.0;

// 竞技场（世界坐标）。原作 x1=194,x2=446,y1=40,y2=340。
pub const ARENA_WIDTH: f32 = 252.0;
pub const ARENA_LEFT: f32 = -126.0;
pub const ARENA_RIGHT: f32 = 126.0;
pub const ARENA_TOP: f32 = 140.0;
pub const ARENA_BOTTOM: f32 = -160.0;

// 板的水平位置：原作 score_x=94.5（左）、next_x=546.5（右）。
pub const SCORE_X: f32 = -225.5;
pub const NEXT_X: f32 = 226.5;

pub const BG_LAYER_ID: usize = 0;
pub const GAME1_LAYER_ID: usize = 1;
pub const GAME2_LAYER_ID: usize = 2;
pub const GAME3_LAYER_ID: usize = 3;
pub const EFFECTS_LAYER_ID: usize = 4;
pub const UI1_LAYER_ID: usize = 5;
pub const UI2_LAYER_ID: usize = 6;
pub const HIGH_RES_LAYER_ID: usize = 7;

/// 背景层（渐变/云/星）：单独低分辨率 canvas 渲染、不描边、不投影。
pub const BG_LAYER: RenderLayers = RenderLayers::layer(BG_LAYER_ID);
/// 原作 game1：后景装饰，参与 outline/shadow。
pub const GAME1_LAYER: RenderLayers = RenderLayers::layer(GAME1_LAYER_ID);
/// 原作 game2：主物理对象/木牌，参与 outline/shadow。
pub const GAME2_LAYER: RenderLayers = RenderLayers::layer(GAME2_LAYER_ID);
/// 原作 game3：前景装饰/手/按钮，参与 outline/shadow。
pub const GAME3_LAYER: RenderLayers = RenderLayers::layer(GAME3_LAYER_ID);
/// 原作 effects：合并飞溅等特效，参与 outline/shadow。
pub const EFFECTS_LAYER: RenderLayers = RenderLayers::layer(EFFECTS_LAYER_ID);
/// 原作 ui1：不描边 UI 层，当前预留。
pub const UI1_LAYER: RenderLayers = RenderLayers::layer(UI1_LAYER_ID);
/// 原作 ui2：光标/过渡等最前 UI，参与 outline。
pub const UI2_LAYER: RenderLayers = RenderLayers::layer(UI2_LAYER_ID);
pub const HIGH_RES_LAYERS: RenderLayers = RenderLayers::layer(HIGH_RES_LAYER_ID);
/// 旧名保留给尚未拆分的调用点；语义上等于原作 game2。
pub const PIXEL_PERFECT_LAYERS: RenderLayers = RenderLayers::layer(GAME2_LAYER_ID);

/// 原作屏幕 x（左上原点）→ 世界 x。
pub fn screen_x(x: f32) -> f32 {
    x - SCREEN_W / 2.0
}
/// 原作屏幕 y（左上原点、y 向下）→ 世界 y（向上）。
pub fn screen_y(y: f32) -> f32 {
    SCREEN_H / 2.0 - y
}

fn main() {
    #[cfg(all(target_arch = "wasm32", feature = "minigame"))]
    wasm_bindgen_futures::spawn_local(run_split_game());

    #[cfg(not(all(target_arch = "wasm32", feature = "minigame")))]
    run_game();
}

#[cfg(all(target_arch = "wasm32", feature = "minigame"))]
#[wasm_split::wasm_split(bevy_game)]
async fn run_split_game() {
    run_game();
}

fn run_game() {
    let mut app = App::new();
    let default_plugins = DefaultPlugins
        .set(ImagePlugin::default_nearest())
        .set(AssetPlugin {
            file_path: if cfg!(all(target_arch = "wasm32", feature = "wechat")) {
                "bevy/assets".into()
            } else if cfg!(target_arch = "wasm32") {
                "assets".into()
            } else {
                "emoji-merge/assets".into()
            },
            meta_check: AssetMetaCheck::Never,
            ..default()
        })
        .set(WindowPlugin {
            primary_window: Some(primary_window()),
            ..default()
        });
    app.add_plugins(default_plugins)
        .add_plugins(PhysicsPlugins::default().with_length_unit(20.0));
    #[cfg(all(target_arch = "wasm32", feature = "minigame"))]
    app.add_plugins(MiniGamePlugin::default());
    app.add_plugins(AgentHarnessPlugin::new(AgentHarnessConfig {
        canvas: Some(AgentCanvasConfig::centered(Vec2::new(SCREEN_W, SCREEN_H))),
        enable_remote: cfg!(not(target_arch = "wasm32")),
        ..default()
    }))
    .add_plugins(PointerPlugin)
    .add_plugins(background::BackgroundPlugin)
    .add_plugins(JuicePlugin)
    .add_plugins(OutlinePlugin)
    .add_plugins(GamePlugin)
    .add_plugins(boards::BoardsPlugin)
    // 原作重力 (0, 360)（px/s²，y 向下）→ 世界 y 向上取负。
    .insert_resource(Gravity(Vector::NEG_Y * 360.0))
    .init_resource::<DropState>()
    .init_resource::<DistanceToTop>()
    .init_resource::<BgmStartup>()
    .register_type::<audio::AudioProbe>()
    .register_type::<audio::BgmCtl>()
    .init_resource::<audio::AudioProbe>()
    .add_systems(
        Startup,
        (
            setup_camera,
            hide_cursor,
            setup_arena,
            setup_lose_line,
            setup_spawner,
            setup_evoji,
            setup_plants,
            setup_bgm,
            setup_buttons,
            mark_web_ready,
        )
            .chain(),
    )
    .add_systems(PreUpdate, fit_canvas.before(PointerSystems::Update))
    .add_systems(
        Update,
        (
            update_distance_to_top,
            (update_spawner_and_held, drop_on_click).chain(),
            update_drop_state,
            reset_held_on_restart,
            update_spawner_hand,
            update_cursor,
            update_lose_line,
            shake_camera,
            sway_plants,
            update_plant_reaction,
            update_evoji_interaction,
            update_evoji_interaction_stars,
            update_button_hover,
            handle_buttons,
            start_bgm_after_input,
            audio::update_bgm,
        ),
    )
    .add_systems(PostUpdate, apply_emoji_spring_scale.after(update_springs))
    .run();
}

fn primary_window() -> Window {
    let window = Window {
        resolution: WindowResolution::new(RES_WIDTH * 2, RES_HEIGHT * 2),
        title: "bevy_emoji".into(),
        ..default()
    };
    #[cfg(target_arch = "wasm32")]
    {
        let mut window = window;
        window.canvas = Some("#bevy-canvas".into());
        window.fit_canvas_to_parent = true;
        window.prevent_default_event_handling = true;
        window
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        window
    }
}

#[derive(Component)]
struct OuterCamera;

#[derive(Component)]
struct Spawner {
    open_for: f32,
}

/// 两种手势纹理在启动阶段一起进入 AssetServer，切换时只复用稳定句柄。
/// 释放动画不再等待新的资源请求，也不会每帧反复写 `Sprite::image`。
#[derive(Resource)]
struct SpawnerImages {
    closed: Handle<Image>,
    open: Handle<Image>,
}

/// 手持球：浮在 spawner 顶部跟随鼠标，非物理（无 RigidBody）。
#[derive(Component)]
struct Held {
    value: usize,
}

/// 下落冷却（对应原作 spawner_emoji.dropping）：当前球离开 spawner 区或 1.4s 兜底前，
/// 不允许再次下落（防止连点）。期间手持球隐藏。
#[derive(Resource, Default)]
enum DropState {
    #[default]
    Idle,
    Dropping {
        ball: Entity,
        safety: Timer,
    },
}

#[derive(Component)]
struct Cursor;

/// UI 按钮（原作 emoji_button）：sfx/music 音量、screen 全屏、close 退出。
#[derive(Component, Clone, Copy)]
enum UiButton {
    Sfx,
    Music,
    Screen,
    Close,
}

impl UiButton {
    fn agent_id(self) -> &'static str {
        match self {
            Self::Sfx => "button.sfx",
            Self::Music => "button.music",
            Self::Screen => "button.fullscreen",
            Self::Close => "button.close",
        }
    }

    fn agent_label(self) -> &'static str {
        match self {
            Self::Sfx => "Sound effects volume",
            Self::Music => "Music volume",
            Self::Screen => "Toggle fullscreen",
            Self::Close => "Close game",
        }
    }
}

/// sfx/music 音量等级由 `audio::SoundLevels` 持有（从存盘加载，见 `audio.rs`）。

#[derive(Resource, Deref, DerefMut)]
pub struct DistanceToTop(pub f32);

impl Default for DistanceToTop {
    fn default() -> Self {
        Self(294.0)
    }
}

#[derive(Component)]
struct LoseDash;

#[derive(Component)]
struct Plant {
    base_rot: f32,
    /// 球经过时的反应角（视觉摇晃，对应原作 apply_moving_force 的近似）。
    react: f32,
}

/// 进化展示 emoji。
#[derive(Component)]
struct Evoji {
    hit_size: Vec2,
}

#[derive(Component)]
struct EvojiInteractionStar {
    velocity: Vec2,
    angular: f32,
    life: f32,
    max_life: f32,
}

#[derive(Resource, Default)]
struct BgmStartup {
    #[cfg(any(
        feature = "web_audio",
        all(target_arch = "wasm32", feature = "minigame")
    ))]
    started: bool,
}

const BLUE_CHAIN_PART_SIZE: f32 = 18.0;

/// 多画布合成，贴近原作 anchor 的 layer 顺序：
///
/// bg -> shadow(game1/game2/game3/effects) -> game1 -> game2 -> game3 -> effects -> ui1 -> ui2
///
/// 每个参与描边的层独立渲染到透明低分辨率 canvas，再跑 outline。拆层后，墙/牌/emoji/
/// 前景装饰/UI 不会再互相吞描边，也能逐层校准阴影和前后遮挡。
fn setup_camera(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    fn new_canvas(images: &mut Assets<Image>) -> Handle<Image> {
        images.add(Image::new_target_texture(
            RES_WIDTH,
            RES_HEIGHT,
            TextureFormat::Rgba8UnormSrgb,
            None,
        ))
    }

    fn spawn_layer_camera(
        commands: &mut Commands,
        canvas: Handle<Image>,
        layer: RenderLayers,
        order: isize,
        clear_color: ClearColorConfig,
        outline: bool,
    ) {
        let mut entity = commands.spawn((
            Camera2d,
            Camera {
                order,
                clear_color,
                ..default()
            },
            RenderTarget::Image(canvas.into()),
            Msaa::Off,
            layer,
        ));
        if outline {
            entity.insert(OutlinePostProcess);
        }
    }

    fn spawn_canvas_sprite(
        commands: &mut Commands,
        canvas: Handle<Image>,
        color: Color,
        translation: Vec3,
    ) {
        commands.spawn((
            Sprite {
                image: canvas,
                color,
                ..default()
            },
            Transform::from_translation(translation),
            HIGH_RES_LAYERS,
        ));
    }

    let canvas_bg = new_canvas(&mut images);
    let canvas_game1 = new_canvas(&mut images);
    let canvas_game2 = new_canvas(&mut images);
    let canvas_game3 = new_canvas(&mut images);
    let canvas_effects = new_canvas(&mut images);
    let canvas_ui1 = new_canvas(&mut images);
    let canvas_ui2 = new_canvas(&mut images);

    // 背景相机 → canvas_bg（不透明，pixel-perfect）。
    spawn_layer_camera(
        &mut commands,
        canvas_bg.clone(),
        BG_LAYER,
        -10,
        ClearColorConfig::Custom(Color::srgb(0.9059, 0.9098, 0.9137)),
        false,
    );
    let transparent = ClearColorConfig::Custom(Color::NONE);
    spawn_layer_camera(
        &mut commands,
        canvas_game1.clone(),
        GAME1_LAYER,
        -9,
        transparent.clone(),
        true,
    );
    spawn_layer_camera(
        &mut commands,
        canvas_game2.clone(),
        GAME2_LAYER,
        -8,
        transparent.clone(),
        true,
    );
    spawn_layer_camera(
        &mut commands,
        canvas_game3.clone(),
        GAME3_LAYER,
        -7,
        transparent.clone(),
        true,
    );
    spawn_layer_camera(
        &mut commands,
        canvas_effects.clone(),
        EFFECTS_LAYER,
        -6,
        transparent.clone(),
        true,
    );
    spawn_layer_camera(
        &mut commands,
        canvas_ui1.clone(),
        UI1_LAYER,
        -5,
        transparent.clone(),
        false,
    );
    spawn_layer_camera(
        &mut commands,
        canvas_ui2.clone(),
        UI2_LAYER,
        -4,
        transparent,
        true,
    );

    // 合成精灵（HIGH_RES，OuterCamera 显示）。
    spawn_canvas_sprite(
        &mut commands,
        canvas_bg,
        Color::WHITE,
        Vec3::new(0.0, 0.0, 0.0),
    );
    // 阴影：原作 shadow 层偏移 4*sx，shader = vec4(0.1,0.1,0.1,a*0.2)。
    let shadow = Color::srgba(0.1, 0.1, 0.1, 0.2);
    for (i, canvas) in [
        canvas_game1.clone(),
        canvas_game2.clone(),
        canvas_game3.clone(),
        canvas_effects.clone(),
    ]
    .into_iter()
    .enumerate()
    {
        spawn_canvas_sprite(
            &mut commands,
            canvas,
            shadow,
            Vec3::new(8.0, -8.0, 1.0 + i as f32 * 0.01),
        );
    }
    for (i, canvas) in [
        canvas_game1,
        canvas_game2,
        canvas_game3,
        canvas_effects,
        canvas_ui1,
        canvas_ui2,
    ]
    .into_iter()
    .enumerate()
    {
        spawn_canvas_sprite(
            &mut commands,
            canvas,
            Color::WHITE,
            Vec3::new(0.0, 0.0, 2.0 + i as f32),
        );
    }

    // 外层相机：合成精灵整数倍放大到窗口。
    commands.spawn((Camera2d, Msaa::Off, OuterCamera, HIGH_RES_LAYERS));
}

/// 隐藏系统鼠标光标（原作 input_set_mouse_visible(false)），改用 index.png 手形光标。
fn hide_cursor(mut q: Query<&mut CursorOptions, With<bevy::window::Window>>) {
    for mut opts in &mut q {
        opts.visible = false;
    }
}

/// 竞技场墙体（地面 + 左右墙），原作 solid 圆角矩形，颜色 colors.green[0]=#7AB357。
fn setup_arena(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let green = Color::srgb(0.4784, 0.7020, 0.3412);
    let floor_img = rounded_rect(&mut images, 252, 10, 4);
    let wall_img = rounded_rect(&mut images, 10, 304, 4);

    let wall = |img: Handle<Image>, w: f32, h: f32| Sprite {
        image: img,
        color: green,
        custom_size: Some(Vec2::new(w, h)),
        ..default()
    };
    let spawn_solid = |commands: &mut Commands,
                       img: Handle<Image>,
                       pos: Vec2,
                       w: f32,
                       h: f32,
                       marker: ArenaSolid| {
        let transform = Transform::from_xyz(pos.x, pos.y, 0.0);
        let solid = commands
            .spawn((
                wall(img.clone(), w, h),
                transform,
                RigidBody::Static,
                Friction::new(1.0),
                Collider::rectangle(w, h),
                marker,
                RoundEndPiece::new(RoundEndPieceKind::Solid, transform),
                GAME2_LAYER,
            ))
            .id();
        // 原作 solid:update() 会在 game2 和 game3 各画一次；game3 副本提供前景遮挡。
        commands.entity(solid).with_child((
            wall(img, w, h),
            Transform::default(),
            RoundEndSolidForeground,
            GAME3_LAYER,
        ));
    };

    // 地面：原作 (320,340,252,10) → 世界 (0,-160)。
    spawn_solid(
        &mut commands,
        floor_img,
        Vec2::new(0.0, ARENA_BOTTOM),
        ARENA_WIDTH,
        10.0,
        ArenaSolid::Bottom,
    );
    // 左墙：原作 (194,193,10,304) → 世界 (-126,-13)。
    spawn_solid(
        &mut commands,
        wall_img.clone(),
        Vec2::new(ARENA_LEFT, -13.0),
        10.0,
        304.0,
        ArenaSolid::Left,
    );
    // 右墙：原作 (446,193,10,304) → 世界 (126,-13)。
    spawn_solid(
        &mut commands,
        wall_img,
        Vec2::new(ARENA_RIGHT, -13.0),
        10.0,
        304.0,
        ArenaSolid::Right,
    );
}

/// 生成圆角矩形（radius r）白色贴图，配合 Sprite color 着色。对应原作 rectangle(...,4,4)。
fn rounded_rect(images: &mut Assets<Image>, w: u32, h: u32, r: u32) -> Handle<Image> {
    let r = (r.min(w / 2).min(h / 2)) as i64;
    let mut data = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let cx = (x as i64).clamp(r, (w - 1) as i64 - r);
            let cy = (y as i64).clamp(r, (h - 1) as i64 - r);
            let dx = x as i64 - cx;
            let dy = y as i64 - cy;
            if dx * dx + dy * dy <= r * r {
                let i = ((y * w + x) * 4) as usize;
                data[i] = 255;
                data[i + 1] = 255;
                data[i + 2] = 255;
                data[i + 3] = 255;
            }
        }
    }
    let img = Image::new(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    images.add(img)
}

/// 失败线：arena 顶部的红色虚线（原作 dashed_line），危险时淡入。
fn setup_lose_line(mut commands: Commands) {
    let red = Color::srgb(0.8745, 0.1451, 0.2510);
    let y = ARENA_TOP;
    let x_start = ARENA_LEFT + 8.0;
    let x_end = ARENA_RIGHT - 8.0;
    let (dash, gap) = (16.0_f32, 8.0);
    let mut x = x_start;
    while x + dash <= x_end {
        commands.spawn((
            Sprite {
                color: red.with_alpha(0.0),
                custom_size: Some(Vec2::new(dash, 2.0)),
                ..default()
            },
            Transform::from_xyz(x + dash / 2.0, y, 0.5),
            LoseDash,
            UI1_LAYER,
        ));
        x += dash + gap;
    }
}

/// spawner（云朵手）+ 手持球 + 自定义光标。
fn setup_spawner(mut commands: Commands, asset_server: Res<AssetServer>) {
    let spawner_images = SpawnerImages {
        closed: asset_server.load("closed_hand.png"),
        open: asset_server.load("open_hand.png"),
    };
    // 云朵手：原作 closed_hand，渲染 42px，浮在 arena 顶部上方（y=160）。
    let hand_transform = Transform::from_xyz(-12.0, 160.0, 2.0);
    commands
        .spawn_scene(scenes::sprite_scene(
            "closed_hand.png",
            Vec2::splat(42.0),
            hand_transform,
            GAME3_LAYER,
        ))
        .insert((
            Spawner { open_for: 0.0 },
            Spring::default(),
            RoundEndPiece::new(RoundEndPieceKind::Spawner, hand_transform),
            AgentTarget::world("game.spawner", Vec2::splat(42.0))
                .with_label("Drop spawner")
                .with_role(AgentTargetRole::Control)
                .with_capabilities([
                    AgentTargetCapability::Hover,
                    AgentTargetCapability::Click,
                    AgentTargetCapability::Press,
                ]),
        ));

    commands.insert_resource(spawner_images);

    // 手持球：浮在 arena 顶部（y=SPAWN_Y），随鼠标 x 移动。
    let held_value = choose_spawn_value();
    let r = emoji_radius(held_value);
    let held_transform = Transform::from_xyz(0.0, SPAWN_Y, 1.5);
    commands
        .spawn_scene(scenes::sprite_scene(
            emoji_filename(held_value),
            Vec2::splat(r * 2.0),
            held_transform,
            GAME2_LAYER,
        ))
        .insert((
            Visibility::Inherited,
            Held { value: held_value },
            Spring {
                current: -0.25,
                ..default()
            },
            RoundEndPiece::new(RoundEndPieceKind::Emoji, held_transform),
            AgentTarget::world("game.held", Vec2::splat(r * 2.0))
                .with_label("Held emoji")
                .with_role(AgentTargetRole::GameObject)
                .with_capabilities([AgentTargetCapability::Hover, AgentTargetCapability::Click]),
        ));

    commands.spawn((
        Transform::from_xyz(0.0, (ARENA_TOP + ARENA_BOTTOM) * 0.5, 0.0),
        AgentTarget::world(
            "game.drop",
            Vec2::new(ARENA_WIDTH - 10.0, ARENA_TOP - ARENA_BOTTOM),
        )
        .with_label("Emoji drop area")
        .with_role(AgentTargetRole::Region)
        .with_capabilities([
            AgentTargetCapability::Hover,
            AgentTargetCapability::Click,
            AgentTargetCapability::Press,
        ]),
    ));

    // 手形光标（index.png）。
    commands
        .spawn_scene(scenes::sprite_scene(
            "index.png",
            Vec2::splat(18.0),
            Transform::from_xyz(0.0, 0.0, 5.0),
            UI2_LAYER,
        ))
        .insert(Cursor);
}

/// 进化展示：原作以 (next_x,249) 为圆心、半径 64 的圆弧排布 11 个 emoji（rs=12），
/// 外加 curving_arrow 和三组 blue_chain 链节。链节按原作 chain_part 画在 game1。
fn setup_evoji(
    mut commands: Commands,
    next_board: Query<(Entity, &Transform), With<boards::NextBoard>>,
) {
    let Ok((next_board, next_board_transform)) = next_board.single() else {
        return;
    };

    let cx = NEXT_X;
    let cy = screen_y(249.0); // -69
    let radius = 64.0;
    let n = EMOJI_DATA.len() as f32;
    let mut angle = -std::f32::consts::FRAC_PI_4 + (3.0 * std::f32::consts::FRAC_PI_2) / 22.0;
    let dangle = (3.0 * std::f32::consts::FRAC_PI_2) / n;
    let arrow_pos = Vec2::new(cx, cy);
    let arrow_transform = Transform::from_xyz(cx, cy, 1.05)
        .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2));
    let arrow = commands
        .spawn_scene(scenes::sprite_scene(
            "curving_arrow.png",
            Vec2::splat(48.0),
            arrow_transform,
            GAME2_LAYER,
        ))
        .insert((
            Evoji {
                hit_size: Vec2::splat(48.0),
            },
            RigidBody::Dynamic,
            Collider::rectangle(48.0 * 0.95, 48.0 * 0.95),
            Restitution::new(1.0),
            ColliderDensity(0.1),
            GravityScale(-1.0),
            LinearDamping(0.25),
            AngularDamping(0.25),
            RoundEndPiece::new(RoundEndPieceKind::Evoji, arrow_transform)
                .with_home_gravity_scale(-1.0),
            AgentTarget::world("evolution.arrow", Vec2::splat(48.0))
                .with_label("Evolution arrow")
                .with_role(AgentTargetRole::DragSurface)
                .with_capabilities([
                    AgentTargetCapability::Hover,
                    AgentTargetCapability::Click,
                    AgentTargetCapability::Press,
                    AgentTargetCapability::Drag,
                ]),
        ))
        .id();

    let mut evoji_entities = Vec::with_capacity(EMOJI_DATA.len());
    let mut evoji_positions = Vec::with_capacity(EMOJI_DATA.len());
    for (index, (name, _, _)) in EMOJI_DATA.iter().enumerate() {
        // 原作屏幕坐标 y = 249 + 64*sin → 世界 y = 180 - (...) = -69 - 64*sin。
        let x = cx + radius * angle.cos();
        let y = cy - radius * angle.sin();
        let pos = Vec2::new(x, y);
        let transform = Transform::from_xyz(x, y, 1.0);
        let entity = commands
            .spawn_scene(scenes::sprite_scene(
                *name,
                Vec2::splat(24.0),
                transform,
                GAME2_LAYER,
            ))
            .insert((
                Evoji {
                    hit_size: Vec2::splat(24.0),
                },
                RigidBody::Dynamic,
                Collider::circle(12.0),
                Restitution::new(1.0),
                ColliderDensity(0.1),
                LinearDamping(0.1),
                AngularDamping(0.1),
                RoundEndPiece::new(RoundEndPieceKind::Evoji, transform),
                AgentTarget::world(format!("evolution.emoji.{}", index + 1), Vec2::splat(24.0))
                    .with_label(format!("Evolution emoji {}", index + 1))
                    .with_role(AgentTargetRole::DragSurface)
                    .with_capabilities([
                        AgentTargetCapability::Hover,
                        AgentTargetCapability::Click,
                        AgentTargetCapability::Press,
                        AgentTargetCapability::Drag,
                    ]),
            ))
            .id();
        evoji_entities.push(entity);
        evoji_positions.push(pos);
        angle += dangle;
    }

    for (entities, positions) in evoji_entities.windows(2).zip(evoji_positions.windows(2)) {
        let a = entities[0];
        let b = entities[1];
        let midpoint = (positions[0] + positions[1]) * 0.5;
        commands.spawn((
            FixedJoint::new(a, b)
                .with_local_anchor1(midpoint - positions[0])
                .with_local_anchor2(midpoint - positions[1]),
            JointCollisionDisabled,
        ));
    }

    // 原版使用 next board 的 96px collider 边缘，并在左右各内缩 8px。
    // 这会让蓝链首节藏进 112px 木牌贴图内，避免连接处露出缝隙。
    let board_chain_x = boards::NEXT_BOARD_COLLIDER_HALF - boards::NEXT_BOARD_CHAIN_INSET;
    let board_left = Vec2::new(-board_chain_x, -boards::NEXT_BOARD_COLLIDER_HALF);
    let board_right = Vec2::new(board_chain_x, -boards::NEXT_BOARD_COLLIDER_HALF);
    let board_anchor_world = |local: Vec2| {
        next_board_transform.translation.truncate()
            + (next_board_transform.rotation * local.extend(0.0)).truncate()
    };
    let left_from = board_anchor_world(board_left);
    let right_from = board_anchor_world(board_right);

    let last = evoji_entities[10];
    let last_pos = evoji_positions[10];
    let last_edge = last_pos - (last_pos - left_from).normalize_or_zero() * 12.0;
    spawn_evoji_blue_chain(
        &mut commands,
        next_board,
        board_left,
        left_from,
        last,
        last_edge - last_pos,
        last_edge,
        RoundEndJointGroup::EvojiLeftChain,
    );

    let first = evoji_entities[0];
    let first_pos = evoji_positions[0];
    let first_edge = first_pos - (first_pos - right_from).normalize_or_zero() * 12.0;
    spawn_evoji_blue_chain(
        &mut commands,
        next_board,
        board_right,
        right_from,
        first,
        first_edge - first_pos,
        first_edge,
        RoundEndJointGroup::EvojiRightChain,
    );

    let middle = evoji_entities[5];
    let middle_pos = evoji_positions[5];
    let arrow_chain_offset = Vec2::new(0.0, -24.0);
    let arrow_chain_anchor =
        (arrow_transform.rotation.inverse() * arrow_chain_offset.extend(0.0)).truncate();
    spawn_evoji_blue_chain(
        &mut commands,
        arrow,
        arrow_chain_anchor,
        arrow_pos + arrow_chain_offset,
        middle,
        Vec2::new(0.0, 12.0),
        middle_pos + Vec2::new(0.0, 12.0),
        RoundEndJointGroup::CurvingChain,
    );
}

fn spawn_blue_chain_joint(
    commands: &mut Commands,
    body1: Entity,
    body2: Entity,
    anchor1: Vec2,
    anchor2: Vec2,
    group: RoundEndJointGroup,
    collide_connected: bool,
) {
    let mut joint = commands.spawn((
        RevoluteJoint::new(body1, body2)
            .with_local_anchor1(anchor1)
            .with_local_anchor2(anchor2),
        group,
    ));
    if !collide_connected {
        joint.insert(JointCollisionDisabled);
    }
}

fn spawn_evoji_blue_chain(
    commands: &mut Commands,
    from_entity: Entity,
    from_anchor: Vec2,
    from_world_anchor: Vec2,
    to_entity: Entity,
    to_anchor: Vec2,
    to_world_anchor: Vec2,
    group: RoundEndJointGroup,
) {
    let delta = to_world_anchor - from_world_anchor;
    let len = delta.length().max(1.0);
    let dir = delta / len;
    let angle = delta.y.atan2(delta.x);
    let part_half = BLUE_CHAIN_PART_SIZE * 0.5;
    let count = (len / BLUE_CHAIN_PART_SIZE).ceil().max(1.0) as usize;
    let mut previous_entity = from_entity;
    let mut previous_anchor = from_anchor;

    for i in 0..count {
        let distance = part_half + i as f32 * BLUE_CHAIN_PART_SIZE;
        let pos = from_world_anchor + dir * distance;
        let transform =
            Transform::from_xyz(pos.x, pos.y, 0.9).with_rotation(Quat::from_rotation_z(angle));
        let part = commands
            .spawn_scene(scenes::sprite_scene(
                "blue_chain.png",
                Vec2::splat(BLUE_CHAIN_PART_SIZE),
                transform,
                GAME1_LAYER,
            ))
            .insert((
                RigidBody::Dynamic,
                Collider::rectangle(BLUE_CHAIN_PART_SIZE, BLUE_CHAIN_PART_SIZE * 0.5),
                ColliderDensity(0.04),
                LinearDamping(0.2),
                AngularDamping(0.2),
                SleepingDisabled,
                RoundEndPiece::new(RoundEndPieceKind::Chain, transform),
            ))
            .id();

        spawn_blue_chain_joint(
            commands,
            previous_entity,
            part,
            previous_anchor,
            Vec2::NEG_X * part_half,
            group,
            false,
        );
        previous_entity = part;
        previous_anchor = Vec2::X * part_half;
    }

    let last_center_distance = part_half + (count - 1) as f32 * BLUE_CHAIN_PART_SIZE;
    let last_anchor = Vec2::X * (len - last_center_distance);
    spawn_blue_chain_joint(
        commands,
        previous_entity,
        to_entity,
        last_anchor,
        to_anchor,
        group,
        true,
    );
}

/// arena 底部与墙上的花草装饰（原作 spawn_plants：底部向上、左右墙向中生长）。
fn setup_plants(mut commands: Commands, asset_server: Res<AssetServer>) {
    let mut idx = 0usize;
    let mut spawn_plant =
        |image: &'static str, sx: f32, sy: f32, rot: f32, size: Vec2, layer: RenderLayers| {
            let transform = Transform::from_xyz(screen_x(sx), screen_y(sy), 0.5)
                .with_rotation(Quat::from_rotation_z(rot));
            commands.spawn((
                Sprite {
                    image: asset_server.load(image),
                    custom_size: Some(size),
                    ..default()
                },
                transform,
                Plant {
                    base_rot: rot,
                    react: 0.0,
                },
                RoundEndPiece::new(RoundEndPieceKind::Plant, transform),
                layer,
            ));
            idx += 1;
        };

    // 原作每条边只随机挑 2-3 个位置生成花草簇；这里先用稳定簇，方便视觉对照。
    for sx in [244.0, 320.0, 396.0] {
        spawn_plant(
            "seedling.png",
            sx - 5.0,
            325.0,
            0.0,
            Vec2::splat(11.0),
            GAME1_LAYER,
        );
        spawn_plant(
            "sheaf.png",
            sx + 5.0,
            325.0,
            0.0,
            Vec2::splat(15.0),
            GAME3_LAYER,
        );
    }

    for sy in [88.0, 184.0, 286.0] {
        spawn_plant(
            "seedling.png",
            209.0,
            sy - 5.0,
            -std::f32::consts::FRAC_PI_2,
            Vec2::splat(11.0),
            GAME1_LAYER,
        );
        spawn_plant(
            "tulip.png",
            209.0,
            sy + 6.0,
            -std::f32::consts::FRAC_PI_2,
            Vec2::splat(15.0),
            GAME3_LAYER,
        );
    }

    for sy in [78.0, 164.0, 258.0] {
        spawn_plant(
            "sheaf.png",
            431.0,
            sy - 6.0,
            std::f32::consts::FRAC_PI_2,
            Vec2::splat(15.0),
            GAME1_LAYER,
        );
        spawn_plant(
            "four_leaf_clover.png",
            431.0,
            sy + 6.0,
            std::f32::consts::FRAC_PI_2,
            Vec2::splat(17.0),
            GAME3_LAYER,
        );
    }
}

/// BGM（原作 music_player_play_song(closed_shop, 0.375)）。音量/生命周期由 `audio::update_bgm` 控制。
/// `web_audio` also covers browser WASM through Bevy's built-in cpal/WebAudio
/// backend; the host bridge below is only for no-default-feature Mini Games.
#[cfg(feature = "web_audio")]
fn spawn_bgm(commands: &mut Commands, asset_server: &AssetServer, levels: &SoundLevels) -> bool {
    commands.spawn((
        AudioPlayer::new(asset_server.load("Recettear OST - Closed Shop.ogg")),
        PlaybackSettings::LOOP.with_volume(Volume::Linear(audio::bgm_volume(levels))),
        Bgm,
    ));
    true
}

#[cfg(all(target_arch = "wasm32", feature = "minigame"))]
fn spawn_bgm(commands: &mut Commands, _asset_server: &AssetServer, levels: &SoundLevels) -> bool {
    let player = web::audio_create("Recettear OST - Closed Shop.ogg", true);
    if player == 0 {
        return false;
    }
    web::audio_set_volume(player, audio::bgm_volume(levels));
    web::audio_set_playback_rate(player, 1.0);
    // Keep this call on the first input frame. Browsers require playback to be
    // initiated by a user gesture, while Mini Game hosts accept the same path.
    web::audio_play(player);
    commands.spawn((WasmBgm(player),));
    true
}

#[cfg(feature = "web_audio")]
fn setup_bgm(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    levels: Res<SoundLevels>,
    mut startup: ResMut<BgmStartup>,
) {
    startup.started = spawn_bgm(&mut commands, &asset_server, &levels);
}

#[cfg(not(feature = "web_audio"))]
fn setup_bgm() {}

#[cfg(all(target_arch = "wasm32", feature = "minigame"))]
fn start_bgm_after_input(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    levels: Res<SoundLevels>,
    pointer: Res<PrimaryPointer>,
    mut startup: ResMut<BgmStartup>,
) {
    if startup.started || !pointer.just_pressed() {
        return;
    }
    startup.started = spawn_bgm(&mut commands, &asset_server, &levels);
}

#[cfg(not(all(target_arch = "wasm32", feature = "minigame")))]
fn start_bgm_after_input() {}

fn mark_web_ready() {
    web::mark_ready();
}

/// UI 按钮：sfx/music/screen/close（原作 emoji_button，w=18，左下角 + 右上角 close）。
fn setup_buttons(mut commands: Commands) {
    let mut mk = |icon: &'static str, sx: f32, sy: f32, kind: UiButton| {
        commands
            .spawn_scene(scenes::sprite_scene(
                icon,
                Vec2::splat(18.0),
                Transform::from_xyz(screen_x(sx), screen_y(sy), 5.0),
                GAME3_LAYER,
            ))
            .insert((
                kind,
                Spring::default(),
                AgentTarget::world(kind.agent_id(), Vec2::splat(18.0))
                    .with_label(kind.agent_label())
                    .with_role(AgentTargetRole::Button)
                    .with_capabilities([
                        AgentTargetCapability::Hover,
                        AgentTargetCapability::Click,
                        AgentTargetCapability::Press,
                        AgentTargetCapability::DoubleClick,
                    ]),
            ));
    };
    mk("sound_4.png", 20.0, 340.0, UiButton::Sfx);
    mk("sound_4.png", 48.0, 340.0, UiButton::Music);
    mk("screen.png", 78.0, 340.0, UiButton::Screen);
    mk("close.png", 620.0, 20.0, UiButton::Close);
}

fn update_button_hover(
    pointer: Res<PrimaryPointer>,
    canvas: Res<CanvasViewport>,
    mut buttons: Query<(&Transform, &mut Spring), With<UiButton>>,
) {
    let mp = pointer_interaction_world(&pointer, &canvas);
    let half = if pointer.is_touch() {
        canvas.touch_hit_half_extent()
    } else {
        9.0
    };
    for (tf, mut spring) in &mut buttons {
        let hovered = mp.is_some_and(|mp| {
            (mp.x - tf.translation.x).abs() <= half && (mp.y - tf.translation.y).abs() <= half
        });
        spring.animate(if hovered { 0.12 } else { 0.0 });
    }
}

/// 点击 UI 按钮：sfx/music 各自循环音量等级（不再共用 GlobalVolume）、screen 切全屏、close 退出。
fn handle_buttons(
    mut commands: Commands,
    pointer: Res<PrimaryPointer>,
    canvas: Res<CanvasViewport>,
    asset_server: Res<AssetServer>,
    mut windows: Query<&mut Window>,
    mut buttons: Query<(&UiButton, &mut Sprite, &Transform, &mut Spring)>,
    best: Res<Best>,
    mut levels: ResMut<SoundLevels>,
    mut exit: MessageWriter<AppExit>,
) {
    if !pointer.just_pressed() {
        return;
    }
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    let Some(mp) = pointer_world(&pointer, &canvas) else {
        return;
    };
    let half = if pointer.is_touch() {
        canvas.touch_hit_half_extent()
    } else {
        9.0
    };
    for (kind, mut sprite, tf, mut spring) in &mut buttons {
        if (mp.x - tf.translation.x).abs() > half || (mp.y - tf.translation.y).abs() > half {
            continue;
        }
        match kind {
            UiButton::Sfx => {
                levels.sfx = if levels.sfx == 0 { 4 } else { levels.sfx - 1 };
                sprite.image = asset_server.load(format!("sound_{}.png", levels.sfx));
                save_save(SaveData {
                    best: **best,
                    sfx: levels.sfx,
                    music: levels.music,
                });
            }
            UiButton::Music => {
                levels.music = if levels.music == 0 {
                    4
                } else {
                    levels.music - 1
                };
                sprite.image = asset_server.load(format!("sound_{}.png", levels.music));
                save_save(SaveData {
                    best: **best,
                    sfx: levels.sfx,
                    music: levels.music,
                });
            }
            UiButton::Screen => {
                toggle_fullscreen(&mut window);
            }
            UiButton::Close => {
                close_or_exit_fullscreen(&mut exit);
            }
        }
        spring.pull(-0.35);
        // 按钮按下音效（原作 sounds.button_press）。sfx/music 按钮 base=1.0（main.lua:210,219），
        // screen/close 按钮 base=0.5（main.lua:230,235），均带 ±5% pitch 抖动。
        let base = matches!(kind, UiButton::Screen | UiButton::Close).then_some(0.5);
        play_sfx(
            &mut commands,
            &asset_server,
            &levels,
            GameSound::ButtonPress,
            base,
        );
        break;
    }
}

fn toggle_fullscreen(window: &mut Window) {
    #[cfg(target_arch = "wasm32")]
    web::toggle_fullscreen();
    #[cfg(not(target_arch = "wasm32"))]
    {
        window.mode = if window.mode == WindowMode::Windowed {
            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
        } else {
            WindowMode::Windowed
        };
    }
    #[cfg(target_arch = "wasm32")]
    let _ = window;
}

fn close_or_exit_fullscreen(exit: &mut MessageWriter<AppExit>) {
    #[cfg(target_arch = "wasm32")]
    web::exit_fullscreen();
    #[cfg(not(target_arch = "wasm32"))]
    {
        exit.write(AppExit::Success);
    }
    #[cfg(target_arch = "wasm32")]
    let _ = exit;
}

fn pointer_world(pointer: &PrimaryPointer, canvas: &CanvasViewport) -> Option<Vec2> {
    pointer
        .position()
        .filter(|position| canvas.contains_window_point(*position))
        .map(|position| canvas.window_to_world(position))
}

fn pointer_interaction_world(pointer: &PrimaryPointer, canvas: &CanvasViewport) -> Option<Vec2> {
    pointer
        .interaction_position()
        .filter(|position| canvas.contains_window_point(*position))
        .map(|position| canvas.window_to_world(position))
}

fn update_distance_to_top(
    emojis: Query<(&Transform, Option<&SpawnGrace>), With<Emoji>>,
    mut distance: ResMut<DistanceToTop>,
) {
    let highest = emojis
        .iter()
        .filter(|(_, grace)| grace.map_or(true, |g| g.0 <= 0.0))
        .map(|(t, _)| t.translation.y)
        .fold(f32::NEG_INFINITY, f32::max);
    **distance = if highest.is_finite() {
        (ARENA_TOP - highest).max(0.0)
    } else {
        294.0
    };
}

fn spawner_offset(value: usize) -> Vec2 {
    let y = match value {
        1 => 18.0,
        2 => 20.0,
        3 => 25.0,
        4 => 27.0,
        5 => 32.0,
        _ => 20.0,
    };
    Vec2::new(0.0, y)
}

/// spawner 跟随鼠标 x（夹到 arena 内，留出当前球的半径），手持球随之。
/// 用 ParamSet 避免两个 &mut Transform 的 B0001 冲突。
fn update_spawner_and_held(
    time: Res<Time>,
    state: Res<GameState>,
    distance: Res<DistanceToTop>,
    pointer: Res<PrimaryPointer>,
    canvas: Res<CanvasViewport>,
    held: Single<&Held>,
    mut transforms: ParamSet<(
        Single<&mut Transform, With<Spawner>>,
        Single<&mut Transform, With<Held>>,
    )>,
) {
    if *state != GameState::Playing {
        return;
    }
    let r = emoji_radius(held.value);
    let pointer_x = pointer_world(&pointer, &canvas).map_or(0.0, |position| position.x);
    let hand_x = (pointer_x - 12.0).clamp(ARENA_LEFT + r - 4.0, ARENA_RIGHT - r - 20.0);
    let danger_lift = if **distance <= 100.0 {
        let rs_oy = if held.value <= 3 { r } else { 1.5 * r };
        (1.0 - (**distance / 100.0).clamp(0.0, 1.0)) * (32.0 + rs_oy)
    } else {
        0.0
    };
    let target_hand_y = 160.0 + danger_lift;
    let offset = spawner_offset(held.value);
    let hand_y = {
        let mut hand = transforms.p0();
        hand.translation.x = hand_x;
        let k = 1.0 - (-5.0 * time.delta_secs()).exp();
        hand.translation.y += (target_hand_y - hand.translation.y) * k;
        hand.translation.y
    };
    let mut held_tf = transforms.p1();
    held_tf.translation.x = hand_x + 12.0 + offset.x;
    held_tf.translation.y = hand_y - offset.y;
}

/// 左键 / 空格：释放手持球（生成真实物理 Emoji），手持球顺位 = 原 next 预览，再随机新 next。
/// 下落冷却（DropState）期间禁止再次下落（对应原作 spawner_emoji.dropping）。
fn drop_on_click(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    pointer: Res<PrimaryPointer>,
    canvas: Res<CanvasViewport>,
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<GameState>,
    mut next: ResMut<NextValue>,
    mut drop_state: ResMut<DropState>,
    levels: Res<SoundLevels>,
    mut spawner: Single<(&mut Spawner, &mut Spring)>,
    mut held: Single<(&mut Held, &mut Sprite, &mut Visibility, &Transform)>,
) {
    if *state != GameState::Playing || !matches!(*drop_state, DropState::Idle) {
        return;
    }
    let pointer_place = pointer.just_pressed()
        && pointer_world(&pointer, &canvas).is_some_and(|position| {
            position.x >= ARENA_LEFT
                && position.x <= ARENA_RIGHT
                && position.y >= ARENA_BOTTOM
                && position.y <= ARENA_TOP
        });
    let place = pointer_place || keys.just_pressed(KeyCode::Space);
    if !place {
        return;
    }
    let (spawner_state, hand_spring) = &mut *spawner;
    let (held_mut, held_sprite, held_vis, held_tf) = &mut *held;
    let value = held_mut.value;
    let ball = spawn_emoji(
        &mut commands,
        &asset_server,
        value,
        held_tf.translation.truncate(),
    );
    // drop 音效（原作 drop = パパッ，base 0.6 + ±5% pitch）。
    play_sfx(&mut commands, &asset_server, &levels, GameSound::Drop, None);

    // 手持球顺位 = 原 next 预览；再随机新 next；下落期间隐藏手持球（原作 spawner 落球时无球）。
    held_mut.value = **next;
    let r = emoji_radius(held_mut.value);
    held_sprite.image = asset_server.load(emoji_filename(held_mut.value));
    held_sprite.custom_size = Some(Vec2::splat(r * 2.0));
    **held_vis = Visibility::Hidden;
    **next = choose_spawn_value();

    *drop_state = DropState::Dropping {
        ball,
        safety: Timer::from_seconds(1.4, TimerMode::Once),
    };
    spawner_state.open_for = 0.5;
    hand_spring.pull(-0.3);
}

/// 原作 drop 时云朵手张开 0.5s 后合上。
fn update_spawner_hand(
    time: Res<Time>,
    state: Res<GameState>,
    images: Res<SpawnerImages>,
    mut spawner: Single<(&mut Spawner, &mut Sprite)>,
) {
    if *state != GameState::Playing {
        return;
    }
    let (spawner, sprite) = &mut *spawner;
    let should_open = spawner.open_for > 0.0;
    if should_open {
        spawner.open_for = (spawner.open_for - time.delta_secs()).max(0.0);
    }
    let expected = if should_open {
        &images.open
    } else {
        &images.closed
    };
    if sprite.image != *expected {
        sprite.image = expected.clone();
    }
}

/// 下落冷却：当前球**落地**（首次碰撞）或 1.4s 兜底后，恢复手持球、允许再次下落。
/// 对应原作 spawner_emoji.dropping 在 collision_enter 或 1.4s safety 后清零。用「落地」而非
/// 「离开 spawner 区」是因为后者只要 ~0.4s，连点仍像连续放置。
fn update_drop_state(
    time: Res<Time>,
    mut drop_state: ResMut<DropState>,
    mut starts: MessageReader<CollisionStart>,
    mut held_vis: Single<&mut Visibility, With<Held>>,
) {
    let cleared = {
        let DropState::Dropping { ball, safety } = &mut *drop_state else {
            return;
        };
        safety.tick(time.delta());
        safety.just_finished()
            || starts
                .read()
                .any(|c| c.collider1 == *ball || c.collider2 == *ball)
    };
    if cleared {
        **held_vis = Visibility::Inherited;
        *drop_state = DropState::Idle;
    }
}

/// 重启（GameOver→Playing）时重置手持球与下落冷却：随机新手持球、显示、解除冷却。
/// 取代旧版「restart 直接 spawn 一只活物理球」——那会导致开局就有一个正在下落的球。
fn reset_held_on_restart(
    state: Res<GameState>,
    asset_server: Res<AssetServer>,
    mut held: Single<(&mut Held, &mut Sprite, &mut Visibility)>,
    mut drop_state: ResMut<DropState>,
    mut prev: Local<GameState>,
) {
    if *prev != GameState::Playing && *state == GameState::Playing {
        let (held_val, held_sprite, held_vis) = &mut *held;
        held_val.value = choose_spawn_value();
        let r = emoji_radius(held_val.value);
        held_sprite.image = asset_server.load(emoji_filename(held_val.value));
        held_sprite.custom_size = Some(Vec2::splat(r * 2.0));
        **held_vis = Visibility::Inherited;
        *drop_state = DropState::Idle;
    }
    *prev = *state;
}

/// 自定义手形光标跟随鼠标（原作 index.png 绘制在 mouse+6,+6）。
fn update_cursor(
    pointer: Res<PrimaryPointer>,
    canvas: Res<CanvasViewport>,
    mut cursor: Single<(&mut Transform, &mut Visibility), With<Cursor>>,
) {
    let (transform, visibility) = &mut *cursor;
    if pointer.kind() != PointerKind::Mouse {
        **visibility = Visibility::Hidden;
        return;
    }
    let Some(position) = pointer_world(&pointer, &canvas) else {
        **visibility = Visibility::Hidden;
        return;
    };
    **visibility = Visibility::Inherited;
    transform.translation.x = position.x + 6.0;
    transform.translation.y = position.y - 6.0;
}

/// 失败线：**已稳定**的最高 emoji 接近 arena 顶部时淡入红色虚线。
/// 排除刚 spawn（SpawnGrace 内）和仍在下落（高速）的球——它们在顶部是正常的，不算危险
/// （对应原作 distance_to_top 不计 spawner_emoji）。
fn update_lose_line(distance: Res<DistanceToTop>, mut dashes: Query<&mut Sprite, With<LoseDash>>) {
    let target = 1.0 - (**distance / 64.0).clamp(0.0, 1.0);
    for mut sprite in &mut dashes {
        sprite.color = sprite.color.with_alpha(target);
    }
}

/// 花草随时间轻微摆动（原作 constant_wind_r = 0.2*sin(1.4t + 0.01x)）+ 球经过的反应角。
fn sway_plants(
    time: Res<Time>,
    state: Res<GameState>,
    mut plants: Query<(&Plant, &mut Transform)>,
) {
    if *state != GameState::Playing {
        return;
    }
    let t = time.elapsed_secs();
    for (plant, mut transform) in &mut plants {
        let wind = 0.2 * (1.4 * t + 0.01 * transform.translation.x).sin();
        transform.rotation = Quat::from_rotation_z(plant.base_rot + wind + plant.react);
    }
}

/// 花草对附近移动的球产生反应摇晃（近似原作 apply_moving_force：球速 × 距离衰减）。
fn update_plant_reaction(
    state: Res<GameState>,
    emojis: Query<(&Transform, &LinearVelocity), With<Emoji>>,
    mut plants: Query<(&Transform, &mut Plant)>,
) {
    if *state != GameState::Playing {
        return;
    }
    let balls: Vec<(Vec2, Vec2)> = emojis
        .iter()
        .map(|(t, v)| (t.translation.truncate(), v.0))
        .collect();
    for (tf, mut plant) in &mut plants {
        let mut react = 0.0;
        let p = tf.translation.truncate();
        for (pos, vel) in &balls {
            let d = p.distance(*pos);
            if d < 60.0 {
                react += vel.x * 0.0025 * (1.0 - d / 60.0);
            }
        }
        // 累加 + 衰减回 0。
        plant.react = plant.react * 0.88 + react;
    }
}

fn spawn_evoji_interaction_star(commands: &mut Commands, pos: Vec2, mouse_delta: Vec2) {
    let angle = rand::random::<f32>() * std::f32::consts::TAU;
    let speed = 75.0 + rand::random::<f32>() * 75.0;
    let life = 0.4 + rand::random::<f32>() * 0.2;
    let rotation_v = -std::f32::consts::TAU + rand::random::<f32>() * std::f32::consts::TAU * 2.0;
    let jitter = if mouse_delta.length_squared() > 0.01 {
        mouse_delta.normalize_or_zero() * 4.0
    } else {
        Vec2::ZERO
    };
    let transform = Transform::from_translation((pos + jitter).extend(2.2))
        .with_rotation(Quat::from_rotation_z(angle));

    commands
        .spawn_scene(scenes::sprite_scene(
            "star.png",
            Vec2::splat(14.0),
            transform,
            EFFECTS_LAYER,
        ))
        .insert((
            EvojiInteractionStar {
                velocity: Vec2::new(angle.cos(), angle.sin()) * speed,
                angular: rotation_v,
                life,
                max_life: life,
            },
            HitFlash::new(0.375),
        ));
}

fn update_evoji_interaction(
    mut commands: Commands,
    time: Res<Time>,
    state: Res<GameState>,
    pointer: Res<PrimaryPointer>,
    canvas: Res<CanvasViewport>,
    mut evoji: Query<(Entity, &Evoji, &Transform, &mut LinearVelocity), Without<Held>>,
) {
    if *state != GameState::Playing {
        return;
    }

    let Some(mouse_pos) = pointer_interaction_world(&pointer, &canvas) else {
        return;
    };
    let screen_delta = pointer.delta() / canvas.scale();
    let mouse_delta = Vec2::new(screen_delta.x, -screen_delta.y);

    for (entity, evoji, transform, mut velocity) in &mut evoji {
        let local = mouse_pos - transform.translation.truncate();
        let hovered =
            local.x.abs() <= evoji.hit_size.x * 0.5 && local.y.abs() <= evoji.hit_size.y * 0.5;
        if !hovered {
            continue;
        }

        let multiplier = if pointer.pressed() { 2.0 } else { 1.0 };
        let impulse = Vec2::new(
            mouse_delta.x * evoji.hit_size.x,
            mouse_delta.y * evoji.hit_size.y,
        ) * multiplier
            * time.delta_secs()
            * 0.85;
        velocity.0 = (velocity.0 + impulse).clamp_length_max(260.0);

        if pointer.just_pressed() {
            commands.entity(entity).insert(HitFlash::new(0.25));
            let count = 2 + (rand::random::<f32>() > 0.5) as usize;
            for _ in 0..count {
                spawn_evoji_interaction_star(&mut commands, mouse_pos, mouse_delta);
            }
        }
    }
}

fn update_evoji_interaction_stars(
    mut commands: Commands,
    time: Res<Time>,
    mut stars: Query<(
        Entity,
        &mut EvojiInteractionStar,
        &mut Transform,
        &mut Sprite,
    )>,
) {
    let dt = time.delta_secs();
    for (entity, mut star, mut transform, mut sprite) in &mut stars {
        star.life -= dt;
        if star.life <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }

        let t = 1.0 - (star.life / star.max_life).clamp(0.0, 1.0);
        let slow = 1.0 - t;
        transform.translation += (star.velocity * slow * dt).extend(0.0);
        transform.rotation *= Quat::from_rotation_z(star.angular * dt);
        transform.scale = Vec3::splat(slow);
        sprite.color = sprite.color.with_alpha(slow.clamp(0.0, 1.0));
    }
}

/// 把 Spring 的 current 作为额外缩放应用到 Transform.scale（视觉 pop，不影响物理 collider）。
fn apply_emoji_spring_scale(mut query: Query<(&Spring, &mut Transform)>) {
    for (spring, mut transform) in &mut query {
        transform.scale = Vec3::splat(1.0 + spring.current);
    }
}

/// 放大时保持整数像素，窗口小于 640x360 时连续缩小，避免移动端竖屏裁切。
fn fit_canvas(
    window: Single<&Window>,
    pointer: Res<PrimaryPointer>,
    mut canvas: ResMut<CanvasViewport>,
    mut projection: Single<&mut Projection, With<OuterCamera>>,
) {
    let Projection::Orthographic(projection) = &mut **projection else {
        return;
    };
    canvas.update(
        Vec2::new(window.width(), window.height()),
        pointer.touch_capable(),
    );
    projection.scale = 1.0 / canvas.scale();
}

/// 屏幕震动：OuterCamera 随 Shake 量随机偏移，逐帧衰减。
fn shake_camera(mut shake: ResMut<Shake>, mut camera: Single<&mut Transform, With<OuterCamera>>) {
    if shake.0 > 0.01 {
        let amp = shake.0 * 8.0;
        camera.translation.x = (rand::random::<f32>() - 0.5) * amp;
        camera.translation.y = (rand::random::<f32>() - 0.5) * amp;
        shake.0 *= 0.85;
    } else {
        shake.0 = 0.0;
        camera.translation.x = 0.0;
        camera.translation.y = 0.0;
    }
}
