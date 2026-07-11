//! 游戏核心：11 级 emoji 数据、Suika 合并机制、得分、失败判定、重启。

use std::collections::{HashMap, HashSet};

use avian2d::prelude::*;
use bevy::{
    asset::{AssetId, RenderAssetUsages},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use bevy_agent_harness::{AgentTarget, AgentTargetCapability, AgentTargetRole};

use crate::audio::{self, GameSound};
use crate::juice::{HitFlash, Spring};
use crate::pointer::{CanvasViewport, PrimaryPointer};
use crate::{
    EFFECTS_LAYER, GAME1_LAYER, GAME2_LAYER, GAME3_LAYER, SCREEN_H, SCREEN_W, UI2_LAYER, scenes,
};

/// 11 个 emoji 等级：`(文件名, 半径, 合并得分)`，对应原作 value_to_emoji_data。
pub const EMOJI_DATA: [(&str, f32, u32); 11] = [
    ("slight_smile.png", 9.0, 1),
    ("blush.png", 11.5, 3),
    ("thinking.png", 16.5, 6),
    ("devil.png", 18.5, 10),
    ("angry.png", 23.0, 15),
    ("relieved.png", 29.5, 21),
    ("yum.png", 35.0, 28),
    ("joy.png", 41.5, 36),
    ("sob.png", 47.5, 45),
    ("smirk.png", 59.0, 56),
    ("sunglasses.png", 70.0, 66),
];

pub const MAX_VALUE: usize = 11;

/// 原作 value_to_emoji_data[].stars：合成新 emoji 出生后爆出的星星数量。
const EMOJI_STARS: [usize; 11] = [2, 2, 3, 3, 4, 4, 5, 6, 8, 12, 24];

/// 顶部 spawn 位置 y（世界坐标，y-up）。原作 spawner_emoji 浮在 arena 顶部 y1=40（屏幕）→ 世界 +140。
/// 手持球（Held）浮在此处跟随 spawner；点击下落。
pub const SPAWN_Y: f32 = 140.0;

/// 失败线 y（世界坐标）：emoji 中心超过此线（被顶上去）且静止 → 游戏结束。
/// 与 SPAWN_Y 同为 arena 顶部（原作 lose_line 在 y1）。Held 球非 Emoji 实体，不参与判定。
const LOSE_LINE_Y: f32 = 140.0;

#[derive(Component, Clone, Copy, Debug)]
pub struct Emoji {
    pub value: usize,
}

/// 出生保护期（秒）：刚 spawn 的 emoji 在此期间不参与失败判定（对应原作 dropping 标志，
/// 避免刚下落的球被墙挤出失败线时立刻误判 GameOver）。
#[derive(Component)]
pub struct SpawnGrace(pub f32);

/// 合并迸出的星星粒子。
#[derive(Component)]
struct Particle {
    velocity: Vec2,
    angular: f32,
    life: f32,
    max_life: f32,
}

/// 原作 from_merge 后 `timer_after(0.01)` 触发的星星爆发。
#[derive(Component)]
struct MergeParticleBurst {
    pos: Vec2,
    value: usize,
    timer: Timer,
}

/// 合并飞溅：旧 emoji 从原位飞向碰撞点并缩小（对应原作 emoji_merge_effect）。
#[derive(Component)]
struct MergeEffect {
    from: Vec2,
    target: Vec2,
    timer: f32,
    duration: f32,
}

#[derive(Resource, Default, Deref, DerefMut)]
pub struct Score(pub u32);

/// 预览的下一只 emoji 等级（drop 时释放它，再随机新的）。
#[derive(Resource, Deref, DerefMut)]
pub struct NextValue(pub usize);

/// emoji 等级对应的贴图文件名。
pub fn emoji_filename(value: usize) -> &'static str {
    EMOJI_DATA[value.saturating_sub(1).min(EMOJI_DATA.len() - 1)].0
}

/// emoji 等级对应的半径（物理 + 渲染）。
pub fn emoji_radius(value: usize) -> f32 {
    EMOJI_DATA[value.saturating_sub(1).min(EMOJI_DATA.len() - 1)].1
}

fn emoji_stars(value: usize) -> usize {
    EMOJI_STARS[value.saturating_sub(1).min(EMOJI_STARS.len() - 1)]
}

/// emoji 等级对应的质量倍数（原作 value_to_emoji_data[].mass_multiplier）：
/// 小球重（×8）、大球轻（sunglasses ×0.25），让大球不会压塌一切。
const EMOJI_MASS_MULTIPLIER: [f32; 11] = [8.0, 6.0, 4.0, 2.0, 1.0, 1.0, 1.0, 1.0, 0.5, 0.5, 0.25];

/// emoji 等级对应的质量倍数。
pub fn emoji_mass_multiplier(value: usize) -> f32 {
    EMOJI_MASS_MULTIPLIER[value.saturating_sub(1).min(EMOJI_MASS_MULTIPLIER.len() - 1)]
}

/// 最高分（持久化到 bevy_emoji_save.txt）。
#[derive(Resource, Deref, DerefMut)]
pub struct Best(pub u32);

/// 存盘内容：最高分 + sfx/music 音量等级。文件格式 `best sfx music`（空格分隔）。
/// 兼容旧版单数字格式（只存 best → sfx/music 默认 4）。
#[derive(Clone, Copy)]
pub struct SaveData {
    pub best: u32,
    pub sfx: u8,
    pub music: u8,
}

#[cfg(not(target_arch = "wasm32"))]
const SAVE_PATH: &str = "bevy_emoji_save.txt";

/// 读 bevy_emoji_save.txt。旧版单数字文件视为只存 best。
pub fn load_save() -> SaveData {
    #[cfg(not(target_arch = "wasm32"))]
    let serialized = std::fs::read_to_string(SAVE_PATH).unwrap_or_default();
    #[cfg(target_arch = "wasm32")]
    let serialized = crate::web::load_save();

    let parts = serialized
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let best = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
    let sfx = parts
        .get(1)
        .and_then(|s| s.parse().ok())
        .map(|v: u8| v.min(4))
        .unwrap_or(4);
    let music = parts
        .get(2)
        .and_then(|s| s.parse().ok())
        .map(|v: u8| v.min(4))
        .unwrap_or(4);
    SaveData { best, sfx, music }
}

pub fn save_save(data: SaveData) {
    let serialized = format!("{} {} {}", data.best, data.sfx, data.music);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = std::fs::write(SAVE_PATH, serialized);
    #[cfg(target_arch = "wasm32")]
    crate::web::save_save(&serialized);
}

/// 分数超过 best 时更新并存盘（连同当前音量等级一并写入）。
fn update_best(score: Res<Score>, mut best: ResMut<Best>, levels: Res<audio::SoundLevels>) {
    if score.is_changed() && **score > **best {
        **best = **score;
        save_save(SaveData {
            best: **best,
            sfx: levels.sfx,
            music: levels.music,
        });
    }
}

/// 屏幕震动量（合并时累积，逐帧衰减）。
#[derive(Resource, Default)]
pub struct Shake(pub f32);

/// 原作开机直接进入 arena（标题屏代码被注释掉），无 Title 状态。
#[derive(Resource, Default, Clone, Copy, PartialEq, Debug)]
pub enum GameState {
    #[default]
    Playing,
    GameOver,
}

/// GameOver 触发信息：(触发 emoji 位置, 触发时刻 elapsed_secs)，用于失败灰化波纹。
#[derive(Resource, Default)]
pub struct GameOverInfo(pub Option<(Vec2, f32)>);

#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArenaSolid {
    Bottom,
    Left,
    Right,
}

#[derive(Component)]
pub struct RoundEndSolidForeground;

#[derive(Component, Clone)]
pub struct RoundEndPiece {
    pub kind: RoundEndPieceKind,
    home: Transform,
    home_gravity_scale: f32,
    fall_home: Option<Transform>,
    fall_color: Option<Color>,
    fall_image: Option<Handle<Image>>,
    physics_released: bool,
}

#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum RoundEndJointGroup {
    SolidLeftJoint,
    SolidRightJoint,
    BoardStabilizer,
    ScoreLeftChain,
    ScoreRightChain,
    BestChain,
    NextLeftChain,
    NextRightChain,
    EvojiLeftChain,
    EvojiRightChain,
    CurvingChain,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RoundEndPieceKind {
    Emoji,
    Solid,
    Board,
    Chain,
    Plant,
    BoardPlant,
    BoardNextEmoji,
    Evoji,
    Spawner,
}

impl RoundEndPiece {
    pub fn new(kind: RoundEndPieceKind, home: Transform) -> Self {
        Self {
            kind,
            home,
            home_gravity_scale: 1.0,
            fall_home: None,
            fall_color: None,
            fall_image: None,
            physics_released: false,
        }
    }

    pub fn with_home_gravity_scale(mut self, gravity_scale: f32) -> Self {
        self.home_gravity_scale = gravity_scale;
        self
    }
}

#[derive(Resource, Default)]
struct RoundEndPhysics {
    joints_released: bool,
    object_count: usize,
    gray_delays: HashMap<Entity, f32>,
    joint_release_schedule: Vec<RoundEndJointRelease>,
    solid_joints_spawned: bool,
}

#[derive(Resource, Default)]
struct RoundEndGrayscaleImages(HashMap<AssetId<Image>, Handle<Image>>);

#[derive(Clone, Copy)]
struct RoundEndJointRelease {
    at: f32,
    group: RoundEndJointGroup,
    mode: RoundEndJointReleaseMode,
    done: bool,
}

#[derive(Clone, Copy)]
enum RoundEndJointReleaseMode {
    OneRandom,
    All,
}

#[derive(Resource, Default)]
struct RoundEndUi {
    phase: RoundEndPhase,
    retry_button: Option<Entity>,
    transition: Option<Entity>,
}

#[derive(Default)]
enum RoundEndPhase {
    #[default]
    Idle,
    Waiting {
        timer: f32,
    },
    Active,
    Transition {
        timer: f32,
        sound_done: bool,
        reset_done: bool,
    },
}

#[derive(Component)]
struct RoundEndEntity;

#[derive(Component)]
struct RoundEndGeneratedJoint;

#[derive(Component)]
struct ScoreChainChar;

#[derive(Component)]
struct RetryChainChar {
    index: usize,
    flash_delay: Option<f32>,
    flash: f32,
}

#[derive(Component, Default)]
struct EmojiCharacterVisual {
    source: Option<Handle<Image>>,
    white: bool,
}

#[derive(Component)]
struct RetryButton {
    hot: bool,
    pressed: bool,
    hot_timer: f32,
    hot_frame: i8,
    hot_dir: i8,
}

#[derive(Component)]
struct PendingRoundEndImpulse {
    linear: Vec2,
    angular: f32,
}

#[derive(Component)]
struct DelayedRoundEndDamping {
    remaining: f32,
    linear: f32,
    angular: Option<f32>,
}

// collider_init creates a solid fixture and an identical sensor fixture. With
// Love2D's default 64 px/m meter, both contribute to the body's mass.
const SOURCE_BODY_DENSITY: f32 = 2.0 / (64.0 * 64.0);
const SOURCE_LIGHT_CHAIN_DENSITY: f32 = SOURCE_BODY_DENSITY * 0.05;
const SOURCE_ANGULAR_IMPULSE_SCALE: f32 = 64.0 * 64.0;
const SOURCE_MAX_ANGULAR_SPEED: f32 = 30.0 * std::f32::consts::PI;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum EmojiCharacterTone {
    BlueOriginal,
    White,
}

impl EmojiCharacterTone {
    fn multiplier(self) -> Vec3 {
        match self {
            Self::BlueOriginal => Vec3::new(0.49166, 1.13333, 1.625),
            Self::White => Vec3::splat(3.0),
        }
    }
}

#[derive(Resource, Default)]
struct EmojiCharacterImages(HashMap<(AssetId<Image>, EmojiCharacterTone), Handle<Image>>);

#[derive(Component)]
struct RetryHotCorner {
    x_sign: f32,
    y_sign: f32,
    horizontal: bool,
}

#[derive(Component)]
struct TransitionCircle;

fn char_filename(c: char) -> Option<&'static str> {
    Some(match c {
        '0' => "0.png",
        '1' => "1.png",
        '2' => "2.png",
        '3' => "3.png",
        '4' => "4.png",
        '5' => "5.png",
        '6' => "6.png",
        '7' => "7.png",
        '8' => "8.png",
        '9' => "9.png",
        'a' => "a.png",
        'b' => "b.png",
        'c' => "c.png",
        'd' => "d.png",
        'e' => "e.png",
        'f' => "f.png",
        'g' => "g.png",
        'h' => "h.png",
        'i' => "i.png",
        'j' => "j.png",
        'k' => "k.png",
        'l' => "l.png",
        'm' => "m.png",
        'n' => "n.png",
        'o' => "o.png",
        'p' => "p.png",
        'q' => "q.png",
        'r' => "r.png",
        's' => "s.png",
        't' => "t.png",
        'u' => "u.png",
        'v' => "v.png",
        'w' => "w.png",
        'x' => "x.png",
        'y' => "y.png",
        'z' => "z.png",
        _ => return None,
    })
}

fn circle_image(images: &mut Assets<Image>) -> Handle<Image> {
    let size = 128u32;
    let radius = size as f32 * 0.5 - 1.0;
    let center = Vec2::splat(size as f32 * 0.5 - 0.5);
    let mut data = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let p = Vec2::new(x as f32, y as f32);
            if p.distance(center) <= radius {
                let i = ((y * size + x) * 4) as usize;
                data[i] = 255;
                data[i + 1] = 255;
                data[i + 2] = 255;
                data[i + 3] = 255;
            }
        }
    }
    images.add(Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    ))
}

fn unit_noise(seed: f32) -> f32 {
    (seed.sin() * 43_758.547).fract().abs()
}

fn score_for(value: usize) -> u32 {
    EMOJI_DATA
        .get(value.wrapping_sub(1))
        .map(|(_, _, s)| *s)
        .unwrap_or(0)
}

/// 加权随机生成 1..=5（原作 `random_weighted_pick(30,25,20,15,10)`）。
pub fn choose_spawn_value() -> usize {
    let weights = [30u32, 25, 20, 15, 10];
    let total: u32 = weights.iter().sum();
    let mut r = rand::random::<u32>() % total;
    for (i, w) in weights.iter().enumerate() {
        if r < *w {
            return i + 1;
        }
        r -= *w;
    }
    weights.len()
}

/// 在 `pos` 生成一只 `value` 级 emoji（带物理 + juice），返回其 entity。
pub fn spawn_emoji(
    commands: &mut Commands,
    asset_server: &AssetServer,
    value: usize,
    pos: Vec2,
) -> Entity {
    let idx = value.saturating_sub(1).min(EMOJI_DATA.len() - 1);
    let (name, radius, _) = EMOJI_DATA[idx];

    let transform = Transform::from_translation(pos.extend(1.0));
    let emoji = commands
        .spawn((
            Sprite {
                image: asset_server.load(name),
                custom_size: Some(Vec2::splat(radius * 2.0)),
                ..default()
            },
            transform,
            RigidBody::Dynamic,
            Collider::circle(radius),
            // 质量倍数通过碰撞体密度实现：mass = density × 面积 = mm × πr²（= 原作 mm × collider_get_mass）。
            ColliderDensity(emoji_mass_multiplier(value)),
            CollisionEventsEnabled,
            Restitution::new(0.2),
            SleepingDisabled,
            Emoji { value },
            Spring {
                current: -0.35,
                ..default()
            },
            HitFlash::new(0.35),
            RoundEndPiece::new(RoundEndPieceKind::Emoji, transform),
            SpawnGrace(1.0),
            GAME2_LAYER,
        ))
        .id();

    emoji
}

/// 合成新 emoji 出生后在 `pos` 迸出一圈星星粒子。
fn spawn_merge_particles(commands: &mut Commands, value: usize, pos: Vec2) {
    let radius = emoji_radius(value);
    let particle_scale = 1.0 + ((radius - 9.0) / (70.0 - 9.0)).clamp(0.0, 1.0) * 2.0;

    for _ in 0..emoji_stars(value) {
        let angle = rand::random::<f32>() * std::f32::consts::TAU;
        let d = 0.8 + rand::random::<f32>() * 0.2;
        let burst_pos = pos + Vec2::new(angle.cos(), angle.sin()) * radius * d;
        let speed = particle_scale * (50.0 + rand::random::<f32>() * 50.0);
        let life = 0.4 + rand::random::<f32>() * 0.2;
        let transform = Transform::from_translation(burst_pos.extend(2.0))
            .with_rotation(Quat::from_rotation_z(angle));

        commands
            .spawn_scene(scenes::sprite_scene(
                "star.png",
                Vec2::splat(14.0 * particle_scale),
                transform,
                EFFECTS_LAYER,
            ))
            .insert(Particle {
                velocity: Vec2::new(angle.cos(), angle.sin()) * speed,
                angular: rand::random::<f32>() * std::f32::consts::TAU * 2.0
                    - std::f32::consts::TAU,
                life,
                max_life: life,
            });
    }
}

fn update_merge_particle_bursts(
    mut commands: Commands,
    time: Res<Time>,
    state: Res<GameState>,
    mut bursts: Query<(Entity, &mut MergeParticleBurst)>,
) {
    if *state != GameState::Playing {
        return;
    }

    for (entity, mut burst) in &mut bursts {
        burst.timer.tick(time.delta());
        if burst.timer.just_finished() {
            spawn_merge_particles(&mut commands, burst.value, burst.pos);
            commands.entity(entity).despawn();
        }
    }
}

/// 推进粒子：移动、旋转、缩小、淡出，到期销毁。
fn update_particles(
    time: Res<Time>,
    mut commands: Commands,
    mut query: Query<(Entity, &mut Particle, &mut Transform, &mut Sprite)>,
) {
    let dt = time.delta_secs();
    for (entity, mut p, mut transform, mut sprite) in &mut query {
        p.life -= dt;
        if p.life <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        transform.translation.x += p.velocity.x * dt;
        transform.translation.y += p.velocity.y * dt;
        transform.rotate_z(p.angular * dt);
        let f = (p.life / p.max_life).clamp(0.0, 1.0);
        transform.scale = Vec3::splat(f.max(0.1));
        sprite.color = sprite.color.with_alpha(f);
    }
}

/// 合并飞溅：从 `from` 飞向 `target` 并缩小。
fn spawn_merge_effect(
    commands: &mut Commands,
    asset_server: &AssetServer,
    value: usize,
    from: Vec2,
    target: Vec2,
) {
    let (name, radius, _) = EMOJI_DATA[value.saturating_sub(1).min(EMOJI_DATA.len() - 1)];
    commands.spawn((
        Sprite {
            image: asset_server.load(name),
            custom_size: Some(Vec2::splat(radius * 2.0)),
            ..default()
        },
        Transform::from_translation(from.extend(1.0)),
        MergeEffect {
            from,
            target,
            timer: 0.0,
            duration: 0.15,
        },
        GAME2_LAYER,
    ));
}

/// 推进合并飞溅：smoothstep 飞向 target + 缩小，到期销毁。
fn update_merge_effects(
    time: Res<Time>,
    mut commands: Commands,
    mut query: Query<(Entity, &mut MergeEffect, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (entity, mut fx, mut transform) in &mut query {
        fx.timer += dt;
        let p = (fx.timer / fx.duration).clamp(0.0, 1.0);
        let pe = p * p * (3.0 - 2.0 * p); // smoothstep
        transform.translation = fx.from.lerp(fx.target, pe).extend(transform.translation.z);
        transform.scale = Vec3::splat((1.0 - p).max(0.0));
        if p >= 1.0 {
            commands.entity(entity).despawn();
        }
    }
}

pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        let save = load_save();
        app.init_resource::<Score>()
            .init_resource::<GameState>()
            .insert_resource(NextValue(choose_spawn_value()))
            .insert_resource(Best(save.best))
            .insert_resource(audio::SoundLevels {
                sfx: save.sfx,
                music: save.music,
            })
            .init_resource::<Shake>()
            .init_resource::<GameOverInfo>()
            .init_resource::<RoundEndUi>()
            .init_resource::<RoundEndPhysics>()
            .init_resource::<RoundEndGrayscaleImages>()
            .init_resource::<EmojiCharacterImages>()
            .add_systems(
                Update,
                (
                    handle_merges,
                    spawn_pending_merges,
                    check_game_over,
                    force_game_over_for_screenshot,
                    restart_on_key,
                    round_end_state_system,
                    round_end_release_physics,
                    round_end_piece_motion,
                    sync_round_end_solid_foreground,
                    apply_pending_round_end_impulses,
                    update_delayed_round_end_damping,
                    update_round_end_motion,
                    retry_button_input,
                    transition_round_end,
                    reset_round_end_pieces_on_playing,
                    update_merge_particle_bursts,
                    update_particles,
                    update_merge_effects,
                    update_best,
                    tick_spawn_grace,
                ),
            )
            .add_systems(
                Update,
                update_emoji_character_visuals.after(update_round_end_motion),
            );
    }
}

fn world_from_screen(pos: Vec2) -> Vec2 {
    Vec2::new(pos.x - SCREEN_W * 0.5, SCREEN_H * 0.5 - pos.y)
}

fn ease_cubic_in_out(p: f32) -> f32 {
    let p = p.clamp(0.0, 1.0);
    if p < 0.5 {
        4.0 * p * p * p
    } else {
        1.0 - (-2.0 * p + 2.0).powi(3) * 0.5
    }
}

fn round_end_object_delay(physics: &RoundEndPhysics) -> f32 {
    0.02 * physics.object_count.max(1) as f32
}

fn round_end_fall_at(physics: &RoundEndPhysics) -> f32 {
    round_end_object_delay(physics) + 0.5
}

fn round_end_score_at(physics: &RoundEndPhysics) -> f32 {
    round_end_object_delay(physics) + 3.0
}

fn spawn_round_end_solid_joints(
    commands: &mut Commands,
    solids: &Query<(Entity, &ArenaSolid, &Transform), With<RigidBody>>,
) -> bool {
    let mut bottom = None;
    let mut left = None;
    let mut right = None;
    for (entity, solid, transform) in solids {
        match solid {
            ArenaSolid::Bottom => bottom = Some((entity, *transform)),
            ArenaSolid::Left => left = Some((entity, *transform)),
            ArenaSolid::Right => right = Some((entity, *transform)),
        }
    }

    let (Some((bottom, bottom_tf)), Some((left, left_tf)), Some((right, right_tf))) =
        (bottom, left, right)
    else {
        return false;
    };

    for entity in [bottom, left, right] {
        commands.entity(entity).insert(RigidBody::Dynamic);
    }

    let mut spawn_joint = |body1: Entity,
                           body2: Entity,
                           body1_tf: Transform,
                           body2_tf: Transform,
                           group: RoundEndJointGroup| {
        let world_anchor = Vec2::new(body1_tf.translation.x, body2_tf.translation.y);
        commands.spawn((
            FixedJoint::new(body1, body2)
                .with_local_anchor1(world_anchor - body1_tf.translation.truncate())
                .with_local_anchor2(world_anchor - body2_tf.translation.truncate())
                .with_point_compliance(0.000001),
            JointCollisionDisabled,
            RoundEndGeneratedJoint,
            group,
        ));
    };
    spawn_joint(
        left,
        bottom,
        left_tf,
        bottom_tf,
        RoundEndJointGroup::SolidLeftJoint,
    );
    spawn_joint(
        right,
        bottom,
        right_tf,
        bottom_tf,
        RoundEndJointGroup::SolidRightJoint,
    );

    true
}

fn random_between(min: f32, max: f32) -> f32 {
    min + rand::random::<f32>() * (max - min)
}

fn random_lerp(min: f32, max: f32, t: f32) -> f32 {
    min + (max - min) * t.clamp(0.0, 1.0)
}

fn grayscale_image_for(
    image: &Handle<Image>,
    images: &mut Assets<Image>,
    grayscale_images: &mut RoundEndGrayscaleImages,
) -> Option<Handle<Image>> {
    let id = image.id();
    if let Some(cached) = grayscale_images.0.get(&id) {
        return Some(cached.clone());
    }

    let mut gray = images.get(image)?.clone();
    let data = gray.data.as_mut()?;
    match gray.texture_descriptor.format {
        TextureFormat::Rgba8Unorm | TextureFormat::Rgba8UnormSrgb => {
            for pixel in data.chunks_exact_mut(4) {
                let luma =
                    (0.299 * pixel[0] as f32 + 0.587 * pixel[1] as f32 + 0.114 * pixel[2] as f32)
                        .round() as u8;
                pixel[0] = luma;
                pixel[1] = luma;
                pixel[2] = luma;
            }
        }
        TextureFormat::Bgra8Unorm | TextureFormat::Bgra8UnormSrgb => {
            for pixel in data.chunks_exact_mut(4) {
                let luma =
                    (0.299 * pixel[2] as f32 + 0.587 * pixel[1] as f32 + 0.114 * pixel[0] as f32)
                        .round() as u8;
                pixel[0] = luma;
                pixel[1] = luma;
                pixel[2] = luma;
            }
        }
        _ => return None,
    }

    let handle = images.add(gray);
    grayscale_images.0.insert(id, handle.clone());
    Some(handle)
}

fn emoji_character_image_for(
    image: &Handle<Image>,
    tone: EmojiCharacterTone,
    images: &mut Assets<Image>,
    character_images: &mut EmojiCharacterImages,
) -> Option<Handle<Image>> {
    let key = (image.id(), tone);
    if let Some(cached) = character_images.0.get(&key) {
        return Some(cached.clone());
    }

    let mut tinted = images.get(image)?.clone();
    let data = tinted.data.as_mut()?;
    let multiplier = tone.multiplier();
    let base = 161.0 / 255.0;
    let tint_channel = |value: u8, channel_multiplier: f32, v: f32| -> u8 {
        let source = value as f32 / 255.0;
        let scaled_multiplier = channel_multiplier * (1.0 - v) + v;
        (source * scaled_multiplier * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8
    };

    match tinted.texture_descriptor.format {
        TextureFormat::Rgba8Unorm | TextureFormat::Rgba8UnormSrgb => {
            for pixel in data.chunks_exact_mut(4) {
                let v = (pixel[0] as f32 / 255.0 - base) / (1.0 - base);
                pixel[0] = tint_channel(pixel[0], multiplier.x, v);
                pixel[1] = tint_channel(pixel[1], multiplier.y, v);
                pixel[2] = tint_channel(pixel[2], multiplier.z, v);
            }
        }
        TextureFormat::Bgra8Unorm | TextureFormat::Bgra8UnormSrgb => {
            for pixel in data.chunks_exact_mut(4) {
                let v = (pixel[2] as f32 / 255.0 - base) / (1.0 - base);
                pixel[2] = tint_channel(pixel[2], multiplier.x, v);
                pixel[1] = tint_channel(pixel[1], multiplier.y, v);
                pixel[0] = tint_channel(pixel[0], multiplier.z, v);
            }
        }
        _ => return None,
    }

    let handle = images.add(tinted);
    character_images.0.insert(key, handle.clone());
    Some(handle)
}

fn schedule_chain_pair_release(
    schedule: &mut Vec<RoundEndJointRelease>,
    fall_at: f32,
    left: RoundEndJointGroup,
    right: RoundEndJointGroup,
) {
    let first_at = fall_at + random_between(0.0, 0.8);
    let second_at = first_at + random_between(0.4, 0.8);
    let (first, second) = if rand::random::<f32>() < 0.5 {
        (left, right)
    } else {
        (right, left)
    };
    schedule.push(RoundEndJointRelease {
        at: first_at,
        group: first,
        mode: RoundEndJointReleaseMode::OneRandom,
        done: false,
    });
    schedule.push(RoundEndJointRelease {
        at: second_at,
        group: second,
        mode: RoundEndJointReleaseMode::OneRandom,
        done: false,
    });
}

fn schedule_solid_joint_release(schedule: &mut Vec<RoundEndJointRelease>, fall_at: f32) {
    let (first, second) = if rand::random::<f32>() < 0.5 {
        (
            RoundEndJointGroup::SolidLeftJoint,
            RoundEndJointGroup::SolidRightJoint,
        )
    } else {
        (
            RoundEndJointGroup::SolidRightJoint,
            RoundEndJointGroup::SolidLeftJoint,
        )
    };
    schedule.push(RoundEndJointRelease {
        at: fall_at,
        group: first,
        mode: RoundEndJointReleaseMode::OneRandom,
        done: false,
    });
    schedule.push(RoundEndJointRelease {
        at: fall_at + random_between(0.4, 0.8),
        group: second,
        mode: RoundEndJointReleaseMode::OneRandom,
        done: false,
    });
}

fn build_round_end_joint_release_schedule(fall_at: f32) -> Vec<RoundEndJointRelease> {
    let mut schedule = vec![RoundEndJointRelease {
        at: fall_at,
        group: RoundEndJointGroup::BoardStabilizer,
        mode: RoundEndJointReleaseMode::All,
        done: false,
    }];

    schedule_solid_joint_release(&mut schedule, fall_at);
    schedule.push(RoundEndJointRelease {
        at: fall_at + random_between(0.6, 0.8),
        group: RoundEndJointGroup::BestChain,
        mode: RoundEndJointReleaseMode::OneRandom,
        done: false,
    });
    schedule_chain_pair_release(
        &mut schedule,
        fall_at,
        RoundEndJointGroup::ScoreLeftChain,
        RoundEndJointGroup::ScoreRightChain,
    );
    schedule_chain_pair_release(
        &mut schedule,
        fall_at,
        RoundEndJointGroup::EvojiLeftChain,
        RoundEndJointGroup::EvojiRightChain,
    );
    schedule_chain_pair_release(
        &mut schedule,
        fall_at,
        RoundEndJointGroup::NextLeftChain,
        RoundEndJointGroup::NextRightChain,
    );

    schedule.sort_by(|a, b| a.at.total_cmp(&b.at));
    schedule
}

fn spawn_revolute_joint(
    commands: &mut Commands,
    body1: Entity,
    body2: Entity,
    anchor1: Vec2,
    anchor2: Vec2,
    lock_angle: bool,
    collide_connected: bool,
) {
    let mut joint = RevoluteJoint::new(body1, body2)
        .with_local_anchor1(anchor1)
        .with_local_anchor2(anchor2)
        .with_point_compliance(0.000001);
    if lock_angle {
        joint = joint.with_angle_limits(0.0, 0.0);
    }
    let mut joint_entity = commands.spawn((joint, RoundEndEntity));
    if !collide_connected {
        joint_entity.insert(JointCollisionDisabled);
    }
}

fn spawn_score_char(
    commands: &mut Commands,
    image: &'static str,
    size: f32,
    start: Vec2,
) -> Entity {
    // Love2D uses a positive angle for clockwise screen-space rotation.
    let angular_impulse = -random_between(8.0, 12.0)
        * random_between(std::f32::consts::FRAC_PI_2, std::f32::consts::PI)
        * SOURCE_ANGULAR_IMPULSE_SCALE;

    commands
        .spawn_scene(scenes::sprite_scene(
            image,
            Vec2::splat(size),
            Transform::from_translation(start.extend(1.2)),
            GAME2_LAYER,
        ))
        .insert((
            RoundEndEntity,
            ScoreChainChar,
            EmojiCharacterVisual::default(),
            RigidBody::Dynamic,
            Collider::rectangle(size, size),
            GravityScale(0.0),
            ColliderDensity(SOURCE_BODY_DENSITY),
            LinearDamping(0.0),
            AngularDamping(0.0),
            MaxAngularSpeed(SOURCE_MAX_ANGULAR_SPEED),
            SleepingDisabled,
            PendingRoundEndImpulse {
                linear: Vec2::X * 48.0,
                angular: angular_impulse,
            },
            DelayedRoundEndDamping {
                remaining: 4.0,
                linear: 0.5,
                angular: None,
            },
        ))
        .id()
}

fn spawn_score_rope(
    commands: &mut Commands,
    from: Entity,
    to: Entity,
    from_pos: Vec2,
    to_pos: Vec2,
    char_size: f32,
) {
    let part_size = 9.0;
    let part_half = part_size * 0.5;
    let from_anchor = Vec2::X * (char_size * 0.5);
    let to_anchor = Vec2::NEG_X * (char_size * 0.5);
    let start = from_pos + from_anchor;
    let end = to_pos + to_anchor;
    let delta = end - start;
    let len = delta.length();

    if len <= f32::EPSILON {
        return;
    }

    let dir = delta / len;
    let angle = dir.y.atan2(dir.x);
    let count = (len / part_size).ceil() as usize;
    let inverse_rotation = Mat2::from_angle(-angle);
    let mut parts = Vec::with_capacity(count);

    for i in 0..count {
        let d = part_half + i as f32 * part_size;
        let pos = start + dir * d;
        let part = commands
            .spawn_scene(scenes::sprite_scene(
                "blue_chain.png",
                Vec2::splat(part_size),
                Transform::from_translation(pos.extend(1.1))
                    .with_rotation(Quat::from_rotation_z(angle)),
                GAME1_LAYER,
            ))
            .insert((
                RoundEndEntity,
                RigidBody::Dynamic,
                Collider::rectangle(part_size, part_size * 0.5),
                GravityScale(0.0),
                ColliderDensity(SOURCE_BODY_DENSITY),
                LinearDamping(0.2),
                AngularDamping(0.0),
                MaxAngularSpeed(SOURCE_MAX_ANGULAR_SPEED),
                SleepingDisabled,
            ))
            .id();
        parts.push((part, pos));
    }

    let (first, first_pos) = parts[0];
    spawn_revolute_joint(
        commands,
        from,
        first,
        from_anchor,
        inverse_rotation * (start - first_pos),
        false,
        false,
    );

    for pair in parts.windows(2) {
        let (previous, previous_pos) = pair[0];
        let (next, next_pos) = pair[1];
        let anchor = (previous_pos + next_pos) * 0.5;
        spawn_revolute_joint(
            commands,
            previous,
            next,
            inverse_rotation * (anchor - previous_pos),
            inverse_rotation * (anchor - next_pos),
            false,
            false,
        );
    }

    let (last, last_pos) = parts[parts.len() - 1];
    spawn_revolute_joint(
        commands,
        last,
        to,
        inverse_rotation * (end - last_pos),
        to_anchor,
        false,
        true,
    );
}

fn spawn_score_text(commands: &mut Commands, score: u32) {
    let text = format!("score {score}");
    let char_size = 32.0;
    let spacing = 16.0;
    let screen_y = SCREEN_H * 0.5 + 48.0;
    let mut screen_x = -46.0 * text.chars().count() as f32;
    let mut previous: Option<(Entity, Vec2)> = None;

    for ch in text.chars() {
        if ch == ' ' {
            screen_x += char_size * 1.1875;
            continue;
        }
        let Some(image) = char_filename(ch) else {
            continue;
        };
        let start = world_from_screen(Vec2::new(screen_x, screen_y));
        let character = spawn_score_char(commands, image, char_size, start);

        if let Some((prev_entity, prev_start)) = previous {
            spawn_score_rope(
                commands,
                prev_entity,
                character,
                prev_start,
                start,
                char_size,
            );
        }

        previous = Some((character, start));
        screen_x += char_size + spacing;
    }
}

fn spawn_retry_char(
    commands: &mut Commands,
    image: &'static str,
    size: f32,
    start: Vec2,
    index: usize,
) -> Entity {
    commands
        .spawn_scene(scenes::sprite_scene(
            image,
            Vec2::splat(size),
            Transform::from_translation(start.extend(1.2)),
            GAME1_LAYER,
        ))
        .insert((
            RoundEndEntity,
            RetryChainChar {
                index,
                flash_delay: None,
                flash: 0.0,
            },
            EmojiCharacterVisual::default(),
            RigidBody::Dynamic,
            Collider::rectangle(size, size),
            GravityScale(0.0),
            ColliderDensity(SOURCE_LIGHT_CHAIN_DENSITY),
            LinearDamping(0.2),
            AngularDamping(0.0),
            MaxAngularSpeed(SOURCE_MAX_ANGULAR_SPEED),
            SleepingDisabled,
        ))
        .id()
}

fn spawn_retry_button(commands: &mut Commands) -> Entity {
    let start_screen = Vec2::new(
        SCREEN_W + 64.0 + random_between(-2.0, 2.0),
        SCREEN_H * 0.5 - 48.0 + random_between(-8.0, 8.0),
    );
    let start = world_from_screen(start_screen);
    let angular_sign = if rand::random::<bool>() { 1.0 } else { -1.0 };
    // Love2D 的大角冲量会被 Box2D 内部旋转限制截断；按惯量直接放大到 Avian 后会
    // 长时间卡在约 94rad/s 的上限。这里直接给一个受控角速度，让入场只翻转半圈左右。
    let angular_velocity = angular_sign * random_between(1.5, 2.5) * std::f32::consts::PI;
    let button = commands
        .spawn_scene(scenes::sprite_scene(
            "retry.png",
            Vec2::splat(64.0),
            Transform::from_translation(start.extend(3.0)),
            GAME2_LAYER,
        ))
        .insert((
            RoundEndEntity,
            RetryButton {
                hot: false,
                pressed: false,
                hot_timer: 0.0,
                hot_frame: 1,
                hot_dir: 1,
            },
            Spring::default(),
            RigidBody::Dynamic,
            Collider::rectangle(64.0, 64.0),
            GravityScale(0.0),
            ColliderDensity(SOURCE_BODY_DENSITY),
            LinearDamping(0.0),
            AngularVelocity(angular_velocity),
            AngularDamping(2.0),
            MaxAngularSpeed(3.0 * std::f32::consts::PI),
            SleepingDisabled,
            PendingRoundEndImpulse {
                linear: Vec2::NEG_X * 128.0,
                angular: 0.0,
            },
            DelayedRoundEndDamping {
                remaining: 4.0,
                linear: 0.5,
                angular: None,
            },
            AgentTarget::world("game.retry", Vec2::splat(64.0))
                .with_label("Retry game")
                .with_role(AgentTargetRole::Button)
                .with_capabilities([
                    AgentTargetCapability::Hover,
                    AgentTargetCapability::Click,
                    AgentTargetCapability::Press,
                    AgentTargetCapability::DoubleClick,
                ]),
        ))
        .id();

    for x_sign in [-1.0_f32, 1.0] {
        for y_sign in [-1.0_f32, 1.0] {
            for horizontal in [true, false] {
                let size = if horizontal {
                    Vec2::new(6.0, 2.0)
                } else {
                    Vec2::new(2.0, 6.0)
                };
                commands
                    .spawn_scene(scenes::colored_sprite_scene(
                        Color::srgb(0.9059, 0.9098, 0.9137),
                        size,
                        Transform::default(),
                        GAME3_LAYER,
                    ))
                    .insert((
                        Visibility::Hidden,
                        RetryHotCorner {
                            x_sign,
                            y_sign,
                            horizontal,
                        },
                        RoundEndEntity,
                        ChildOf(button),
                    ));
            }
        }
    }

    let text = "retry";
    let char_size = 16.0;
    let mut previous = button;
    let mut previous_anchor = Vec2::X * 32.0;

    for (i, ch) in text.chars().enumerate() {
        let Some(image) = char_filename(ch) else {
            continue;
        };
        let x = char_size * 0.5 + i as f32 * char_size;
        let start = world_from_screen(start_screen + Vec2::new(32.0 + x, 0.0));
        let character = spawn_retry_char(commands, image, char_size, start, i);
        spawn_revolute_joint(
            commands,
            previous,
            character,
            previous_anchor,
            Vec2::NEG_X * (char_size * 0.5),
            true,
            false,
        );
        previous = character;
        previous_anchor = Vec2::X * (char_size * 0.5);
    }
    button
}

fn round_end_state_system(
    mut commands: Commands,
    time: Res<Time>,
    asset_server: Res<AssetServer>,
    state: Res<GameState>,
    info: Res<GameOverInfo>,
    score: Res<Score>,
    mut ui: ResMut<RoundEndUi>,
    mut physics: ResMut<RoundEndPhysics>,
    levels: Res<audio::SoundLevels>,
    pieces: Query<(Entity, &GlobalTransform), (With<RoundEndPiece>, Without<RoundEndEntity>)>,
) {
    match (&mut ui.phase, *state) {
        (RoundEndPhase::Idle, GameState::GameOver) => {
            let Some((trigger, _)) = info.0 else {
                return;
            };
            audio::play_sfx(
                &mut commands,
                &asset_server,
                &levels,
                GameSound::EndRound,
                None,
            );
            let mut ordered = pieces
                .iter()
                .map(|(entity, transform)| {
                    (entity, transform.translation().truncate().distance(trigger))
                })
                .collect::<Vec<_>>();
            ordered.sort_by(|a, b| a.1.total_cmp(&b.1));
            physics.object_count = ordered.len();
            physics.gray_delays.clear();
            for (index, (entity, _)) in ordered.into_iter().enumerate() {
                physics.gray_delays.insert(entity, index as f32 * 0.02);
            }
            physics.joints_released = false;
            physics.solid_joints_spawned = false;
            physics.joint_release_schedule =
                build_round_end_joint_release_schedule(round_end_fall_at(&physics));
            ui.phase = RoundEndPhase::Waiting { timer: 0.0 };
        }
        (RoundEndPhase::Waiting { timer }, GameState::GameOver) => {
            *timer += time.delta_secs();
            if *timer >= round_end_score_at(&physics) {
                audio::play_sfx(
                    &mut commands,
                    &asset_server,
                    &levels,
                    GameSound::EndRoundScore,
                    None,
                );
                audio::play_sfx(
                    &mut commands,
                    &asset_server,
                    &levels,
                    GameSound::ItsOver,
                    None,
                );
                spawn_score_text(&mut commands, **score);
                ui.retry_button = Some(spawn_retry_button(&mut commands));
                ui.phase = RoundEndPhase::Active;
            }
        }
        (RoundEndPhase::Transition { .. }, GameState::Playing) => {
            physics.gray_delays.clear();
        }
        (_, GameState::Playing) => {
            ui.phase = RoundEndPhase::Idle;
            ui.retry_button = None;
            physics.gray_delays.clear();
        }
        _ => {}
    }
}

fn round_end_release_physics(
    mut commands: Commands,
    time: Res<Time>,
    state: Res<GameState>,
    info: Res<GameOverInfo>,
    mut physics: ResMut<RoundEndPhysics>,
    asset_server: Res<AssetServer>,
    levels: Res<audio::SoundLevels>,
    solids: Query<(Entity, &ArenaSolid, &Transform), With<RigidBody>>,
    old_joints: Query<
        (Entity, &RoundEndJointGroup),
        (
            Or<(With<RevoluteJoint>, With<FixedJoint>)>,
            Without<RoundEndEntity>,
            Without<JointDisabled>,
        ),
    >,
    disabled_joints: Query<
        Entity,
        (
            Or<(With<RevoluteJoint>, With<FixedJoint>)>,
            With<RoundEndJointGroup>,
            With<JointDisabled>,
            Without<RoundEndEntity>,
            Without<RoundEndGeneratedJoint>,
        ),
    >,
    generated_joints: Query<Entity, With<RoundEndGeneratedJoint>>,
) {
    if *state == GameState::Playing {
        physics.joints_released = false;
        physics.solid_joints_spawned = false;
        physics.gray_delays.clear();
        physics.joint_release_schedule.clear();
        for entity in &generated_joints {
            // Avian treats removing JointDisabled as re-enabling a joint, including
            // when that removal happens implicitly during despawn. Remove the joint
            // itself first so its cleanup cannot be re-added to the island graph.
            commands.entity(entity).remove::<FixedJoint>().despawn();
        }
        for entity in &disabled_joints {
            commands.entity(entity).remove::<JointDisabled>();
        }
        return;
    }

    let Some((_trigger, started_at)) = info.0 else {
        return;
    };
    let elapsed = time.elapsed_secs() - started_at;
    if !physics.solid_joints_spawned && elapsed >= round_end_fall_at(&physics) {
        physics.solid_joints_spawned = spawn_round_end_solid_joints(&mut commands, &solids);
        // 跌落启动音效（原作 timer_after(0.02*#objects+0.5) 时的 end_round_fall）。
        audio::play_sfx(
            &mut commands,
            &asset_server,
            &levels,
            GameSound::EndRoundFall,
            None,
        );
    }

    let due: Vec<(usize, RoundEndJointGroup, RoundEndJointReleaseMode)> = physics
        .joint_release_schedule
        .iter()
        .enumerate()
        .filter_map(|(idx, release)| {
            (!release.done && elapsed >= release.at).then_some((idx, release.group, release.mode))
        })
        .collect();

    for (idx, group, mode) in due {
        let candidates: Vec<Entity> = old_joints
            .iter()
            .filter_map(|(entity, joint_group)| (*joint_group == group).then_some(entity))
            .collect();
        let completed = !candidates.is_empty();
        match mode {
            RoundEndJointReleaseMode::All => {
                for entity in candidates {
                    commands.entity(entity).insert(JointDisabled);
                }
            }
            RoundEndJointReleaseMode::OneRandom => {
                if !candidates.is_empty() {
                    let index = (rand::random::<f32>() * candidates.len() as f32) as usize
                        % candidates.len();
                    commands.entity(candidates[index]).insert(JointDisabled);
                }
            }
        }
        if completed {
            if let Some(release) = physics.joint_release_schedule.get_mut(idx) {
                release.done = true;
            }
        }
    }

    physics.joints_released = !physics.joint_release_schedule.is_empty()
        && physics
            .joint_release_schedule
            .iter()
            .all(|release| release.done);
}

fn round_end_piece_motion(
    mut commands: Commands,
    time: Res<Time>,
    state: Res<GameState>,
    info: Res<GameOverInfo>,
    physics: Res<RoundEndPhysics>,
    mut images: ResMut<Assets<Image>>,
    mut grayscale_images: ResMut<RoundEndGrayscaleImages>,
    asset_server: Res<AssetServer>,
    levels: Res<audio::SoundLevels>,
    mut grayed: Local<HashSet<Entity>>,
    mut pieces: Query<(
        Entity,
        &mut RoundEndPiece,
        &mut Transform,
        Option<&mut Sprite>,
        Option<&RigidBody>,
        Option<&ArenaSolid>,
    )>,
) {
    if *state != GameState::GameOver {
        grayed.clear();
        return;
    }
    let Some((trigger, started_at)) = info.0 else {
        return;
    };

    let elapsed = time.elapsed_secs() - started_at;
    let gray = Color::srgb(0.6314, 0.6314, 0.6314);
    for (entity, mut piece, mut transform, sprite, body, solid) in &mut pieces {
        if piece.fall_home.is_none() {
            piece.fall_home = Some(*transform);
        }
        let home = piece.fall_home.unwrap_or(piece.home);
        let gray_delay = physics
            .gray_delays
            .get(&entity)
            .copied()
            .unwrap_or_else(|| home.translation.truncate().distance(trigger) / 420.0);
        let is_gray = elapsed >= gray_delay;
        // 逐物体首次变灰时播 death_hit（原作 end_round 里每物体置 dying 时的 death_hit）。
        if is_gray && grayed.insert(entity) {
            audio::play_sfx(
                &mut commands,
                &asset_server,
                &levels,
                GameSound::DeathHit,
                None,
            );
        }

        if let Some(mut sprite) = sprite {
            if piece.fall_color.is_none() {
                piece.fall_color = Some(sprite.color);
            }
            if piece.fall_image.is_none() {
                piece.fall_image = Some(sprite.image.clone());
            }
            let base = piece.fall_color.unwrap_or(Color::WHITE);
            if piece.kind == RoundEndPieceKind::Solid {
                sprite.color = if is_gray { gray } else { base };
            } else if is_gray {
                if let Some(source) = piece.fall_image.as_ref() {
                    if let Some(gray_image) =
                        grayscale_image_for(source, &mut images, &mut grayscale_images)
                    {
                        sprite.image = gray_image;
                    }
                }
                sprite.color = Color::WHITE;
            } else {
                if let Some(source) = piece.fall_image.as_ref() {
                    sprite.image = source.clone();
                }
                sprite.color = base;
            }
        }

        let fall_t = elapsed - round_end_fall_at(&physics);
        if fall_t <= 0.0 {
            if body.is_some() && !piece.physics_released {
                commands.entity(entity).insert((
                    GravityScale(0.0),
                    LinearVelocity(Vec2::ZERO),
                    AngularVelocity(0.0),
                ));
            }
            continue;
        }

        let passive_physics_piece = matches!(
            piece.kind,
            RoundEndPieceKind::Board | RoundEndPieceKind::Chain | RoundEndPieceKind::Evoji
        );
        if matches!(
            piece.kind,
            RoundEndPieceKind::BoardPlant | RoundEndPieceKind::BoardNextEmoji
        ) {
            continue;
        }
        if body.is_some() && passive_physics_piece {
            if !piece.physics_released {
                commands.entity(entity).insert((
                    RigidBody::Dynamic,
                    GravityScale(piece.home_gravity_scale),
                    LinearVelocity(Vec2::ZERO),
                    AngularVelocity(0.0),
                ));
                piece.physics_released = true;
            }
            continue;
        }

        let seed = home.translation.x * 12.9898 + home.translation.y * 78.233;
        let n = unit_noise(seed);
        let n2 = unit_noise(seed + 17.17);
        let n3 = unit_noise(seed + 43.43);
        let n4 = unit_noise(seed + 91.91);
        let (vx, vy, gravity, angular) = match piece.kind {
            RoundEndPieceKind::Emoji => (
                random_lerp(-80.0, 80.0, n),
                random_lerp(0.0, 160.0, n2),
                360.0 * random_lerp(0.8, 1.2, n3),
                random_lerp(
                    -16.0 * std::f32::consts::PI,
                    16.0 * std::f32::consts::PI,
                    n4,
                ),
            ),
            RoundEndPieceKind::Solid => match solid {
                Some(ArenaSolid::Left) => (
                    -100.0,
                    0.0,
                    360.0 * random_lerp(0.3, 0.5, n),
                    -random_lerp(0.8, 1.8, n2),
                ),
                Some(ArenaSolid::Right) => (
                    100.0,
                    0.0,
                    360.0 * random_lerp(0.3, 0.5, n),
                    random_lerp(0.8, 1.8, n2),
                ),
                Some(ArenaSolid::Bottom) | None => {
                    (0.0, 0.0, 360.0 * random_lerp(0.3, 0.5, n), 0.0)
                }
            },
            RoundEndPieceKind::Plant => (
                random_lerp(-5.0, 5.0, n),
                random_lerp(0.0, 5.0, n2),
                360.0 * random_lerp(0.1, 0.6, n3),
                random_lerp(
                    -12.0 * std::f32::consts::PI,
                    12.0 * std::f32::consts::PI,
                    n4,
                ),
            ),
            RoundEndPieceKind::Spawner => {
                let vx = random_lerp(-40.0, 40.0, n);
                let spin = if vx < 0.0 { -1.0 } else { 1.0 }
                    * random_lerp(8.0 * std::f32::consts::PI, 24.0 * std::f32::consts::PI, n3);
                (
                    vx,
                    random_lerp(20.0, 60.0, n2),
                    360.0 * random_lerp(1.0, 1.2, n4),
                    spin,
                )
            }
            RoundEndPieceKind::Board | RoundEndPieceKind::Chain | RoundEndPieceKind::Evoji => {
                (0.0, 0.0, 360.0, 0.0)
            }
            RoundEndPieceKind::BoardPlant | RoundEndPieceKind::BoardNextEmoji => {
                (0.0, 0.0, 0.0, 0.0)
            }
        };

        if body.is_some() {
            if !piece.physics_released {
                commands.entity(entity).insert((
                    RigidBody::Dynamic,
                    GravityScale((gravity / 360.0_f32).clamp(0.1, 1.2)),
                    LinearVelocity(Vec2::new(vx, vy)),
                    AngularVelocity(angular),
                ));
                piece.physics_released = true;
            }
            continue;
        }

        let t = fall_t.min(5.0);
        transform.translation =
            home.translation + Vec3::new(vx * t, vy * t - 0.5 * gravity * t * t, 0.0);
        transform.rotation = home.rotation * Quat::from_rotation_z(angular * t);
    }
}

fn sync_round_end_solid_foreground(
    parents: Query<
        &Sprite,
        (
            With<RoundEndPiece>,
            With<ArenaSolid>,
            Without<RoundEndSolidForeground>,
        ),
    >,
    mut foregrounds: Query<(&ChildOf, &mut Sprite), With<RoundEndSolidForeground>>,
) {
    for (ChildOf(parent), mut sprite) in &mut foregrounds {
        if let Ok(parent_sprite) = parents.get(*parent) {
            sprite.color = parent_sprite.color;
        }
    }
}

fn reset_round_end_pieces_on_playing(
    mut commands: Commands,
    state: Res<GameState>,
    mut previous: Local<GameState>,
    mut pieces: Query<(
        Entity,
        &mut RoundEndPiece,
        &mut Transform,
        Option<&mut Sprite>,
        Option<&mut GravityScale>,
        Option<&mut LinearVelocity>,
        Option<&mut AngularVelocity>,
        Option<&RigidBody>,
    )>,
) {
    if *previous == GameState::GameOver && *state == GameState::Playing {
        for (
            entity,
            mut piece,
            mut transform,
            sprite,
            gravity_scale,
            velocity,
            angular_velocity,
            body,
        ) in &mut pieces
        {
            if piece.kind != RoundEndPieceKind::Emoji || body.is_none() {
                *transform = piece.home;
            }
            if piece.kind == RoundEndPieceKind::Solid && body.is_some() {
                commands.entity(entity).insert(RigidBody::Static);
            }
            if piece.kind == RoundEndPieceKind::Evoji {
                commands.entity(entity).insert(RigidBody::Dynamic);
            }
            if let Some(mut gravity_scale) = gravity_scale {
                gravity_scale.0 = piece.home_gravity_scale;
            }
            if let Some(mut sprite) = sprite {
                if let Some(image) = piece.fall_image.take() {
                    sprite.image = image;
                }
                sprite.color = piece.fall_color.unwrap_or(Color::WHITE);
            }
            if let Some(mut velocity) = velocity {
                velocity.0 = Vec2::ZERO;
            }
            if let Some(mut angular_velocity) = angular_velocity {
                angular_velocity.0 = 0.0;
            }
            piece.fall_home = None;
            piece.fall_color = None;
            piece.fall_image = None;
            piece.physics_released = false;
        }
    }
    *previous = *state;
}

fn apply_pending_round_end_impulses(
    mut commands: Commands,
    mut pending: Query<(Entity, Forces, &PendingRoundEndImpulse)>,
) {
    for (entity, mut forces, impulse) in &mut pending {
        forces.apply_linear_impulse(impulse.linear);
        forces.apply_angular_impulse(impulse.angular);
        commands.entity(entity).remove::<PendingRoundEndImpulse>();
    }
}

fn update_delayed_round_end_damping(
    mut commands: Commands,
    time: Res<Time>,
    mut delayed: Query<(
        Entity,
        &mut DelayedRoundEndDamping,
        &mut LinearDamping,
        &mut AngularDamping,
    )>,
) {
    for (entity, mut delay, mut linear, mut angular) in &mut delayed {
        delay.remaining -= time.delta_secs();
        if delay.remaining > 0.0 {
            continue;
        }
        linear.0 = delay.linear;
        if let Some(value) = delay.angular {
            angular.0 = value;
        }
        commands.entity(entity).remove::<DelayedRoundEndDamping>();
    }
}

fn update_round_end_motion(
    time: Res<Time>,
    mut retry_chars: Query<(
        &mut RetryChainChar,
        &mut Transform,
        &mut EmojiCharacterVisual,
    )>,
) {
    let dt = time.delta_secs();
    for (mut ch, mut tf, mut visual) in &mut retry_chars {
        let should_flash = if let Some(delay) = ch.flash_delay.as_mut() {
            *delay -= dt;
            *delay <= 0.0
        } else {
            false
        };
        if should_flash {
            ch.flash_delay = None;
            ch.flash = 0.15;
        }
        ch.flash = (ch.flash - dt).max(0.0);
        let flash = (ch.flash / 0.15).clamp(0.0, 1.0);
        visual.white = flash > 0.0;
        tf.scale = Vec3::splat(1.0 + 0.18 * flash);
    }
}

fn update_emoji_character_visuals(
    mut images: ResMut<Assets<Image>>,
    mut character_images: ResMut<EmojiCharacterImages>,
    mut characters: Query<(&mut EmojiCharacterVisual, &mut Sprite)>,
) {
    for (mut visual, mut sprite) in &mut characters {
        let source = visual
            .source
            .get_or_insert_with(|| sprite.image.clone())
            .clone();
        let tone = if visual.white {
            EmojiCharacterTone::White
        } else {
            EmojiCharacterTone::BlueOriginal
        };
        if let Some(image) =
            emoji_character_image_for(&source, tone, &mut images, &mut character_images)
        {
            sprite.image = image;
            sprite.color = Color::WHITE;
        }
    }
}

fn retry_button_input(
    mut commands: Commands,
    time: Res<Time>,
    mut images: ResMut<Assets<Image>>,
    asset_server: Res<AssetServer>,
    pointer: Res<PrimaryPointer>,
    canvas: Res<CanvasViewport>,
    mut ui: ResMut<RoundEndUi>,
    levels: Res<audio::SoundLevels>,
    mut retry_buttons: Query<(Entity, &mut RetryButton, &Transform, &mut Spring)>,
    mut retry_chars: Query<&mut RetryChainChar>,
    mut corners: Query<
        (&RetryHotCorner, &ChildOf, &mut Transform, &mut Visibility),
        Without<RetryButton>,
    >,
) {
    if !matches!(ui.phase, RoundEndPhase::Active) {
        for (_, _, _, mut visibility) in &mut corners {
            *visibility = Visibility::Hidden;
        }
        return;
    }
    let auto_replay = std::env::var_os("BEVY_EMOJI_AUTO_REPLAY").is_some();
    let mp = pointer
        .interaction_position()
        .filter(|position| canvas.contains_window_point(*position))
        .map(|position| canvas.window_to_world(position));
    let hit_half = if pointer.is_touch() {
        32.0_f32.max(canvas.touch_hit_half_extent())
    } else {
        32.0
    };
    for (entity, mut button, tf, mut spring) in &mut retry_buttons {
        let hovered = mp.is_some_and(|mp| {
            (mp.x - tf.translation.x).abs() <= hit_half
                && (mp.y - tf.translation.y).abs() <= hit_half
        });
        if hovered && !button.hot {
            audio::play_sfx(
                &mut commands,
                &asset_server,
                &levels,
                GameSound::ButtonHover,
                None,
            );
            button.hot_timer = 0.0;
            button.hot_frame = 1;
            button.hot_dir = 1;
            spring.current += 0.125;
        }
        button.hot = hovered;
        if hovered && !button.pressed {
            button.hot_timer += time.delta_secs();
            if button.hot_timer > 0.08 {
                button.hot_timer = 0.0;
                button.hot_frame += button.hot_dir;
                if button.hot_frame > 4 || button.hot_frame < 1 {
                    button.hot_dir = -button.hot_dir;
                    button.hot_frame += 2 * button.hot_dir;
                }
            }
        } else {
            button.hot_timer = 0.0;
            button.hot_frame = 1;
            button.hot_dir = 1;
        }

        let hot_offset = if hovered && !button.pressed {
            (button.hot_frame - 1).max(0) as f32 * 2.0
        } else {
            0.0
        };
        let corner_visible = if hovered && !button.pressed {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        let edge = 1.3 * 64.0 * 0.5 - hot_offset;
        for (corner, ChildOf(parent), mut corner_tf, mut visibility) in &mut corners {
            if *parent != entity {
                continue;
            }
            *visibility = corner_visible;
            let x_inside = if corner.horizontal { 3.0 } else { 1.0 };
            let y_inside = if corner.horizontal { 1.0 } else { 3.0 };
            corner_tf.translation.x = corner.x_sign * edge - corner.x_sign * x_inside;
            corner_tf.translation.y = corner.y_sign * edge - corner.y_sign * y_inside;
            corner_tf.translation.z = 0.2;
            corner_tf.rotation = Quat::IDENTITY;
        }

        let replay_requested = hovered && pointer.just_pressed() || auto_replay;
        if replay_requested && !button.pressed {
            button.pressed = true;
            spring.current += 0.5;
            commands.entity(entity).insert(HitFlash::new(0.15));
            for mut character in &mut retry_chars {
                character.flash_delay = Some(0.066 + character.index as f32 * 0.066);
            }
            for (_, ChildOf(parent), _, mut visibility) in &mut corners {
                if *parent == entity {
                    *visibility = Visibility::Hidden;
                }
            }
            audio::play_sfx(
                &mut commands,
                &asset_server,
                &levels,
                GameSound::EndRoundRetryPress,
                None,
            );
            let circle = commands
                .spawn((
                    Sprite {
                        image: circle_image(&mut images),
                        color: Color::srgb(0.2392, 0.5255, 0.7804),
                        custom_size: Some(Vec2::splat(1.0)),
                        ..default()
                    },
                    Transform::from_xyz(0.0, 0.0, 10.0),
                    TransitionCircle,
                    UI2_LAYER,
                ))
                .id();
            ui.transition = Some(circle);
            ui.phase = RoundEndPhase::Transition {
                timer: 0.0,
                sound_done: false,
                reset_done: false,
            };
        }
    }
}

fn transition_round_end(
    mut commands: Commands,
    time: Res<Time>,
    asset_server: Res<AssetServer>,
    mut ui: ResMut<RoundEndUi>,
    mut state: ResMut<GameState>,
    mut info: ResMut<GameOverInfo>,
    mut score: ResMut<Score>,
    levels: Res<audio::SoundLevels>,
    emojis: Query<Entity, With<Emoji>>,
    pending: Query<Entity, With<PendingMerge>>,
    bursts: Query<Entity, With<MergeParticleBurst>>,
    particles: Query<Entity, With<Particle>>,
    effects: Query<Entity, With<MergeEffect>>,
    round_entities: Query<Entity, With<RoundEndEntity>>,
    mut transition: Query<(Entity, &mut Sprite), With<TransitionCircle>>,
) {
    let RoundEndPhase::Transition {
        timer,
        sound_done,
        reset_done,
    } = &mut ui.phase
    else {
        return;
    };

    *timer += time.delta_secs();
    if !*sound_done && *timer >= 0.066 * 7.0 {
        audio::play_sfx(
            &mut commands,
            &asset_server,
            &levels,
            GameSound::EndRoundRetry,
            None,
        );
        *sound_done = true;
    }

    if let Ok((_, mut sprite)) = transition.single_mut() {
        let cover_start = 0.066 * 7.0;
        let cover_end = cover_start + 0.8;
        let reset_at = cover_end + 0.4;
        let done_at = reset_at + 0.8;
        let radius = SCREEN_W * 0.75;
        let diameter = radius * 2.0;
        let size = if *timer < cover_start {
            1.0
        } else if *timer < cover_end {
            diameter * ease_cubic_in_out((*timer - cover_start) / 0.8)
        } else if *timer < reset_at {
            diameter
        } else {
            diameter * (1.0 - ease_cubic_in_out((*timer - reset_at) / 0.8))
        };
        sprite.custom_size = Some(Vec2::splat(size.max(1.0)));

        if !*reset_done && *timer >= reset_at {
            for e in &emojis {
                commands.entity(e).despawn();
            }
            for e in &pending {
                commands.entity(e).despawn();
            }
            for e in &bursts {
                commands.entity(e).despawn();
            }
            for e in &particles {
                commands.entity(e).despawn();
            }
            for e in &effects {
                commands.entity(e).despawn();
            }
            for e in &round_entities {
                commands.entity(e).despawn();
            }
            **score = 0;
            *info = GameOverInfo::default();
            *state = GameState::Playing;
            *reset_done = true;
        }

        if *timer >= done_at {
            if let Ok((entity, _)) = transition.single_mut() {
                commands.entity(entity).despawn();
            }
            ui.transition = None;
            ui.retry_button = None;
            ui.phase = RoundEndPhase::Idle;
        }
    }
}

/// 检测两个同级 emoji 相撞 → 合并（Suika 核心）。
fn handle_merges(
    mut commands: Commands,
    mut starts: MessageReader<CollisionStart>,
    asset_server: Res<AssetServer>,
    emojis: Query<(&Emoji, &Transform, &LinearVelocity)>,
    mut score: ResMut<Score>,
    state: Res<GameState>,
    mut shake: ResMut<Shake>,
    levels: Res<audio::SoundLevels>,
    // 防止同一只 emoji 在一帧内被多个碰撞事件重复合并。
    mut handled: Local<HashSet<Entity>>,
) {
    if *state != GameState::Playing {
        return;
    }
    handled.clear();
    for event in starts.read() {
        let (e1, e2) = (event.collider1, event.collider2);
        if handled.contains(&e1) || handled.contains(&e2) {
            continue;
        }
        let Ok([(emoji1, t1, v1), (emoji2, t2, v2)]) = emojis.get_many([e1, e2]) else {
            continue; // 碰撞双方不都是 emoji（如撞墙）。
        };
        if emoji1.value != emoji2.value {
            continue;
        }

        let value = emoji1.value;
        let mid = ((t1.translation + t2.translation) * 0.5).truncate();
        let from1 = t1.translation.truncate();
        let from2 = t2.translation.truncate();
        **score += score_for(value);

        commands.entity(e1).despawn();
        commands.entity(e2).despawn();
        handled.insert(e1);
        handled.insert(e2);

        // 合并飞溅（旧 emoji 飞向中点缩小）。
        spawn_merge_effect(&mut commands, &asset_server, value, from1, mid);
        spawn_merge_effect(&mut commands, &asset_server, value, from2, mid);

        if value < MAX_VALUE {
            // 普通合并：merge_1 + merge_2 双音效（各带独立 ±5% pitch）；延迟 0.15s 生成 value+1，继承 (v1+v2)/6 冲量。
            audio::play_sfx(
                &mut commands,
                &asset_server,
                &levels,
                GameSound::Merge1,
                None,
            );
            audio::play_sfx(
                &mut commands,
                &asset_server,
                &levels,
                GameSound::Merge2,
                None,
            );
            commands.spawn((
                Transform::default(),
                PendingMerge {
                    pos: mid,
                    value: value + 1,
                    impulse: (v1.0 + v2.0) / 6.0,
                    timer: Timer::from_seconds(0.15, TimerMode::Once),
                },
            ));
        } else {
            // 满级（11+11）：final_merge 音效，不生成新球（原作 a.value==11 && b.value==11）。
            audio::play_sfx(
                &mut commands,
                &asset_server,
                &levels,
                GameSound::FinalMerge,
                None,
            );
        }

        // 屏幕震动；星星粒子由新合成 emoji 出生后 0.01s 触发，对齐原作 from_merge 分支。
        shake.0 += 0.3;
    }
}

/// 待生成的合并球：原作在 merge_emojis 里延迟 0.15s 才生成 value+1（让飞溅特效先播）。
#[derive(Component)]
struct PendingMerge {
    pos: Vec2,
    value: usize,
    impulse: Vec2,
    timer: Timer,
}

/// 计时到期后生成合并球，并赋予继承的冲量（原作 collider_apply_impulse((avx+bvx)/6,..)）。
fn spawn_pending_merges(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    time: Res<Time>,
    state: Res<GameState>,
    mut pending: Query<(Entity, &mut PendingMerge)>,
) {
    if *state != GameState::Playing {
        return;
    }
    for (entity, mut pm) in &mut pending {
        pm.timer.tick(time.delta());
        if pm.timer.just_finished() {
            let ball = spawn_emoji(&mut commands, &asset_server, pm.value, pm.pos);
            commands.entity(ball).insert(LinearVelocity(pm.impulse));
            commands.spawn(MergeParticleBurst {
                pos: pm.pos,
                value: pm.value,
                timer: Timer::from_seconds(0.01, TimerMode::Once),
            });
            commands.entity(entity).despawn();
        }
    }
}

/// 失败判定：emoji 超过失败线且静止（速度阈值排除下落经过）；出生保护期内不判。
fn check_game_over(
    time: Res<Time>,
    emojis: Query<(&Transform, &LinearVelocity, Option<&SpawnGrace>), With<Emoji>>,
    mut state: ResMut<GameState>,
    mut info: ResMut<GameOverInfo>,
) {
    if *state != GameState::Playing {
        return;
    }
    for (transform, vel, grace) in &emojis {
        if grace.map_or(false, |g| g.0 > 0.0) {
            continue;
        }
        if transform.translation.y > LOSE_LINE_Y && vel.0.length() < 10.0 {
            *state = GameState::GameOver;
            *info = GameOverInfo(Some((
                transform.translation.truncate(),
                time.elapsed_secs(),
            )));
            return;
        }
    }
}

fn force_game_over_for_screenshot(
    time: Res<Time>,
    mut done: Local<bool>,
    mut score: ResMut<Score>,
    mut state: ResMut<GameState>,
    mut info: ResMut<GameOverInfo>,
) {
    if *done || std::env::var_os("BEVY_EMOJI_FORCE_GAME_OVER").is_none() {
        return;
    }
    if time.elapsed_secs() < 0.4 {
        return;
    }
    **score = std::env::var("BEVY_EMOJI_FORCE_SCORE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1223);
    *state = GameState::GameOver;
    *info = GameOverInfo(Some((Vec2::ZERO, time.elapsed_secs())));
    *done = true;
}

/// 递减出生保护期。
fn tick_spawn_grace(time: Res<Time>, mut q: Query<&mut SpawnGrace>) {
    let dt = time.delta_secs();
    for mut g in &mut q {
        g.0 = (g.0 - dt).max(0.0);
    }
}

/// 按 R 重启（仅在 GameOver）：清空 emoji、重置分数与状态。
/// 手持球/下落冷却由 main.rs 的 reset_held_on_restart 在状态切回 Playing 时重置（不再 spawn 活球）。
fn restart_on_key(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    emojis: Query<Entity, With<Emoji>>,
    bursts: Query<Entity, With<MergeParticleBurst>>,
    round_entities: Query<Entity, With<RoundEndEntity>>,
    mut score: ResMut<Score>,
    mut state: ResMut<GameState>,
    mut info: ResMut<GameOverInfo>,
) {
    if *state == GameState::GameOver && keys.just_pressed(KeyCode::KeyR) {
        for e in &emojis {
            commands.entity(e).despawn();
        }
        for e in &bursts {
            commands.entity(e).despawn();
        }
        for e in &round_entities {
            commands.entity(e).despawn();
        }
        **score = 0;
        *state = GameState::Playing;
        *info = GameOverInfo::default();
    }
}
