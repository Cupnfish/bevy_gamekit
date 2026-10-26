//! 物理牌 UI：score/best/next 木牌 + 藤蔓链。
//!
//! 对应原作 board / emoji_chain（main.lua）。原作红/绿/蓝木牌用 vine_chain 藤蔓链吊在
//! 屏幕外顶部的 solid_top 上（best 牌吊在 score 牌下方），是原作「一切皆物理」UI 特色。
//!
//! 牌做成 Dynamic 钟摆（RevoluteJoint 到屏外 Static 锚点），藤蔓链按原作
//! emoji_chain 生成真实 chain_part 刚体，再用 RevoluteJoint 串起来。
//!
//! 布局数值取自原作：score(red,96px)@(score_x,120)、best(green,80px)@(score_x,253)、
//! next(blue,112px)@(next_x,108)。文字用 volkswagen-serial-bold，标签 fg[0] 白、数字 calendar_gray。

use avian2d::{math::*, prelude::*};
use bevy::{prelude::*, sprite::Anchor, text::FontSource};
use bevy_agent_harness::{AgentTarget, AgentTargetCapability, AgentTargetRole};

use crate::game::{
    Best, GameState, NextValue, RoundEndJointGroup, RoundEndPiece, RoundEndPieceKind, Score,
    emoji_filename, emoji_radius,
};
use crate::juice::{HitFlash, Spring};
use crate::pointer::{CanvasViewport, PrimaryPointer};
use crate::{
    EFFECTS_LAYER, GAME1_LAYER, GAME2_LAYER, GAME3_LAYER, NEXT_X, PIXEL_PERFECT_LAYERS, SCORE_X,
    scenes, screen_y,
};

#[derive(Component, Clone, Copy, PartialEq)]
enum BoardKind {
    Score,
    Best,
    Next,
}

impl BoardKind {
    const fn collider_size(self) -> f32 {
        match self {
            Self::Score => 88.0,
            Self::Best => 70.0,
            Self::Next => 96.0,
        }
    }

    const fn collider_half(self) -> f32 {
        self.collider_size() * 0.5
    }

    fn agent_id(self) -> &'static str {
        match self {
            Self::Score => "board.score",
            Self::Best => "board.best",
            Self::Next => "board.next",
        }
    }

    fn agent_label(self) -> &'static str {
        match self {
            Self::Score => "Score board",
            Self::Best => "Best score board",
            Self::Next => "Next emoji board",
        }
    }
}

/// 木牌标记（查 transform 用）。
#[derive(Component)]
struct Board {
    render: f32,
    hit_size: f32,
}

/// 右侧 next 木牌标记，供 evoji 蓝链锚到动态摆动的木牌底部。
#[derive(Component)]
pub struct NextBoard;

/// score/best 牌上的标签文字子实体（SCORE/BEST/NEXT）。
#[derive(Component)]
struct BoardLabel;

/// score/best 牌上的数值文字子实体（独立标记，避免用文本长度区分）。
#[derive(Component)]
struct BoardValue(BoardKind);

/// next 牌上的 emoji 子实体。
#[derive(Component)]
struct BoardNextEmoji;

#[derive(Component)]
struct BoardPlant {
    phase: f32,
    sway: f32,
}

#[derive(Component)]
struct BoardInteractionStar {
    velocity: Vec2,
    angular: f32,
    life: f32,
    max_life: f32,
}

/// 木牌锚点 y（原作 solid_top 在屏幕 y=-120 → 世界 +300，屏外上方）。
const TOP_ANCHOR_Y: f32 = 300.0;
const VINE_CHAIN_PART_SIZE: f32 = 18.0;
pub const NEXT_BOARD_COLLIDER_HALF: f32 = 48.0;
pub const NEXT_BOARD_CHAIN_INSET: f32 = 8.0;

pub struct BoardsPlugin;

impl Plugin for BoardsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, setup_boards).add_systems(
            Update,
            (
                update_board_labels,
                update_board_next,
                update_board_plants,
                update_board_interaction,
                update_board_interaction_stars,
            ),
        );
    }
}

/// 生成一张悬挂木牌：Dynamic 钟摆（屏外 Static 锚点 + 刚性 RevoluteJoint，低密度轻质量保证不坠落）。
fn spawn_board(
    commands: &mut Commands,
    asset_server: &AssetServer,
    font: &Handle<Font>,
    kind: BoardKind,
    image: &'static str,
    cx: f32,
    cy: f32,
    render: f32,
    next_value: Option<usize>,
) -> Entity {
    let collider_size = kind.collider_size();

    let board_transform = Transform::from_xyz(cx, cy, 1.0);
    let board = commands
        .spawn((
            Sprite {
                image: asset_server.load(image),
                custom_size: Some(Vec2::splat(render)),
                ..default()
            },
            board_transform,
            RigidBody::Dynamic,
            Collider::rectangle(collider_size, collider_size),
            // 低密度 → 轻质量，刚性关节稳稳挂住（早期密度 1 的重牌会把软关节拽坠）。
            ColliderDensity(0.05),
            SleepingDisabled,
            Board {
                render,
                hit_size: collider_size,
            },
            AgentTarget::world(kind.agent_id(), Vec2::splat(collider_size))
                .with_label(kind.agent_label())
                .with_role(AgentTargetRole::DragSurface)
                .with_capabilities([
                    AgentTargetCapability::Hover,
                    AgentTargetCapability::Click,
                    AgentTargetCapability::Press,
                    AgentTargetCapability::Drag,
                ]),
            RoundEndPiece::new(RoundEndPieceKind::Board, board_transform),
            PIXEL_PERFECT_LAYERS,
        ))
        .id();

    // 原作由左右藤链承担木牌重量。不要再加隐藏的中心铰链，否则藤链不受拉力会松垂。
    let kick = (rand::random::<f32>() - 0.5) * 60.0;
    commands
        .entity(board)
        .insert(LinearVelocity(RVector::X * kick));
    if matches!(kind, BoardKind::Next) {
        commands.entity(board).insert(NextBoard);
    }

    // 标签 + 数值/预览（子实体，随牌 transform 倾斜）。
    let (label_off, label_text) = match kind {
        BoardKind::Score => (24.0, "SCORE"),
        BoardKind::Best => (20.0, "BEST"),
        BoardKind::Next => (28.0, "NEXT"),
    };
    commands.entity(board).with_child((
        Text2d::new(label_text),
        TextFont {
            font: FontSource::Handle(font.clone()),
            font_size: FontSize::Px(26.0),
            ..default()
        },
        TextColor(Color::srgb(0.9059, 0.9098, 0.9137)),
        Transform::from_xyz(0.0, label_off, 0.1),
        BoardLabel,
        GAME2_LAYER,
    ));

    match kind {
        BoardKind::Score | BoardKind::Best => {
            let num_off = if matches!(kind, BoardKind::Score) {
                -12.0
            } else {
                -10.0
            };
            let size = if matches!(kind, BoardKind::Score) {
                46.0
            } else {
                36.0
            };
            commands.entity(board).with_child((
                Text2d::new("0"),
                TextFont {
                    font: FontSource::Handle(font.clone()),
                    font_size: FontSize::Px(size),
                    ..default()
                },
                TextColor(Color::srgb(0.4, 0.4588, 0.4980)), // calendar_gray[0]=#66757F
                Transform::from_xyz(0.0, num_off, 0.1),
                BoardValue(kind),
                GAME2_LAYER,
            ));
        }
        BoardKind::Next => {
            let value = next_value.unwrap_or(1);
            let size = emoji_radius(value) * 2.0;
            commands.entity(board).with_child((
                Sprite {
                    image: asset_server.load(emoji_filename(value)),
                    custom_size: Some(Vec2::splat(size)),
                    ..default()
                },
                Transform::from_xyz(0.0, -15.0, 0.1),
                Spring::default(),
                BoardNextEmoji,
                RoundEndPiece::new(
                    RoundEndPieceKind::BoardNextEmoji,
                    Transform::from_xyz(0.0, -15.0, 0.1),
                ),
                GAME3_LAYER,
            ));
        }
    }

    board
}

fn spawn_chain_joint(
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

/// 原作 emoji_chain('vine_chain', ...)：逐节 chain_part 刚体 + revolute joints。
fn spawn_vine_chain_between(
    commands: &mut Commands,
    from_entity: Option<Entity>,
    from_anchor: Vec2,
    from_world_anchor: Vec2,
    to_entity: Entity,
    to_anchor: Vec2,
    to_world_anchor: Vec2,
    group: RoundEndJointGroup,
) {
    let start_anchor = match from_entity {
        Some(entity) => (entity, from_anchor),
        None => {
            let anchor = commands
                .spawn((
                    Transform::from_xyz(from_world_anchor.x, from_world_anchor.y, 0.0),
                    RigidBody::Static,
                    PIXEL_PERFECT_LAYERS,
                ))
                .id();
            (anchor, Vec2::ZERO)
        }
    };

    let delta = to_world_anchor - from_world_anchor;
    let len = delta.length().max(1.0);
    let dir = delta / len;
    let angle = dir.y.atan2(dir.x);
    let part_half = VINE_CHAIN_PART_SIZE * 0.5;
    let count = (len / VINE_CHAIN_PART_SIZE).ceil().max(1.0) as usize;
    let mut previous_entity = start_anchor.0;
    let mut previous_anchor = start_anchor.1;

    for i in 0..count {
        let distance = part_half + i as f32 * VINE_CHAIN_PART_SIZE;
        let position = from_world_anchor + dir * distance;
        let transform = Transform::from_translation(position.extend(0.8))
            .with_rotation(Quat::from_rotation_z(angle));
        let part = commands
            .spawn_scene(scenes::sprite_scene(
                "vine_chain.png",
                Vec2::splat(VINE_CHAIN_PART_SIZE),
                transform,
                GAME1_LAYER,
            ))
            .insert((
                RigidBody::Dynamic,
                Collider::rectangle(VINE_CHAIN_PART_SIZE, VINE_CHAIN_PART_SIZE * 0.5),
                ColliderDensity(0.04),
                LinearDamping(0.2),
                AngularDamping(0.2),
                SleepingDisabled,
                RoundEndPiece::new(RoundEndPieceKind::Chain, transform),
            ))
            .id();

        spawn_chain_joint(
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

    let last_center_distance = part_half + (count - 1) as f32 * VINE_CHAIN_PART_SIZE;
    let last_anchor = Vec2::X * (len - last_center_distance);
    spawn_chain_joint(
        commands,
        previous_entity,
        to_entity,
        last_anchor,
        to_anchor,
        group,
        true,
    );
}

fn spawn_board_plant(
    commands: &mut Commands,
    board: Entity,
    image: &'static str,
    x: f32,
    y_extra: f32,
    board_collider_height: f32,
    size: f32,
    phase: f32,
) {
    // main.lua positions board plants from board.h, which is overwritten by
    // collider_init (88/70/96), not from the 96/80/112 render size. The
    // bottom anchor then sits a few pixels inside the painted board edge.
    let root_y = board_collider_height / 2.0 + y_extra - size / 2.0;
    let transform = Transform::from_xyz(x, root_y, 0.2);
    commands
        .spawn_scene(scenes::anchored_sprite_scene(
            image,
            Vec2::splat(size),
            transform,
            Anchor::BOTTOM_CENTER,
            GAME3_LAYER,
        ))
        .insert((
            ChildOf(board),
            BoardPlant { phase, sway: 0.0 },
            RoundEndPiece::new(RoundEndPieceKind::BoardPlant, transform),
        ));
}

fn spawn_board_plants(commands: &mut Commands, score: Entity, best: Entity, next: Entity) {
    // Stable version of original spawn_plants board_plant clusters. The source randomizes
    // image choice/presence; keeping deterministic clusters makes pixel iteration repeatable.
    for (x, y_extra, size, image, phase) in [
        (-21.0, 11.0, 20.0, "sheaf.png", 0.2),
        (-9.0, 8.0, 15.0, "seedling.png", 1.1),
        (-33.0, 6.0, 11.0, "tulip.png", 2.0),
        (21.0, 11.0, 20.0, "blossom.png", 2.7),
        (33.0, 6.0, 11.0, "seedling.png", 3.5),
        (9.0, 6.0, 11.0, "four_leaf_clover.png", 4.2),
    ] {
        spawn_board_plant(commands, score, image, x, y_extra, 88.0, size, phase);
    }

    for (x, y_extra, size, image, phase) in [
        (0.0, 12.0, 20.0, "blossom.png", 0.5),
        (12.0, 10.0, 15.0, "seedling.png", 1.4),
        (-12.0, 10.0, 15.0, "sheaf.png", 2.3),
        (24.0, 8.0, 11.0, "tulip.png", 3.1),
        (-24.0, 8.0, 11.0, "four_leaf_clover.png", 4.0),
    ] {
        spawn_board_plant(commands, best, image, x, y_extra, 70.0, size, phase);
    }

    for (x, y_extra, size, image, phase) in [
        (0.0, 17.0, 26.0, "sheaf.png", 0.8),
        (16.0, 14.0, 20.0, "blossom.png", 1.7),
        (-16.0, 14.0, 20.0, "seedling.png", 2.6),
        (28.0, 12.0, 15.0, "tulip.png", 3.4),
        (-28.0, 12.0, 15.0, "four_leaf_clover.png", 4.3),
        (40.0, 10.0, 11.0, "seedling.png", 5.1),
        (-40.0, 10.0, 11.0, "tulip.png", 5.8),
    ] {
        spawn_board_plant(commands, next, image, x, y_extra, 96.0, size, phase);
    }
}

fn setup_boards(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    next_value: Res<NextValue>,
) {
    let font = asset_server.load("volkswagen-serial-bold.ttf");

    // Score 红牌（左上）：render 96 @ (score_x,120)。
    let score = spawn_board(
        &mut commands,
        &asset_server,
        &font,
        BoardKind::Score,
        "red_board.png",
        SCORE_X,
        screen_y(120.0),
        96.0,
        None,
    );
    // score 两根藤蔓链：屏外顶部 (score_x±21, 300) → score 牌顶 (±21, +48)。
    let score_center = Vec2::new(SCORE_X, screen_y(120.0));
    let score_half = BoardKind::Score.collider_half();
    for s in [-21.0_f32, 21.0] {
        let group = if s < 0.0 {
            RoundEndJointGroup::ScoreLeftChain
        } else {
            RoundEndJointGroup::ScoreRightChain
        };
        spawn_vine_chain_between(
            &mut commands,
            None,
            Vec2::ZERO,
            Vec2::new(SCORE_X + s, TOP_ANCHOR_Y),
            score,
            Vec2::new(s, score_half),
            score_center + Vec2::new(s, score_half),
            group,
        );
    }

    // Best 绿牌（左下）：render 80 @ (score_x,253)，吊在 score 牌底。
    let best = spawn_board(
        &mut commands,
        &asset_server,
        &font,
        BoardKind::Best,
        "green_board.png",
        SCORE_X,
        screen_y(253.0),
        80.0,
        None,
    );
    // best 一根链：score 牌底 (0,-48) → best 牌顶 (0,+40)。
    let best_center = Vec2::new(SCORE_X, screen_y(253.0));
    let best_half = BoardKind::Best.collider_half();
    spawn_vine_chain_between(
        &mut commands,
        Some(score),
        Vec2::new(0.0, -score_half),
        score_center + Vec2::new(0.0, -score_half),
        best,
        Vec2::new(0.0, best_half),
        best_center + Vec2::new(0.0, best_half),
        RoundEndJointGroup::BestChain,
    );

    // Next 蓝牌（右上）：render 112 @ (next_x,108)。
    let next = spawn_board(
        &mut commands,
        &asset_server,
        &font,
        BoardKind::Next,
        "blue_board.png",
        NEXT_X,
        screen_y(108.0),
        112.0,
        Some(**next_value),
    );
    let next_center = Vec2::new(NEXT_X, screen_y(108.0));
    let next_half = BoardKind::Next.collider_half();
    for s in [-21.0_f32, 21.0] {
        let group = if s < 0.0 {
            RoundEndJointGroup::NextLeftChain
        } else {
            RoundEndJointGroup::NextRightChain
        };
        spawn_vine_chain_between(
            &mut commands,
            None,
            Vec2::ZERO,
            Vec2::new(NEXT_X + s, TOP_ANCHOR_Y),
            next,
            Vec2::new(s, next_half),
            next_center + Vec2::new(s, next_half),
            group,
        );
    }

    spawn_board_plants(&mut commands, score, best, next);
}

fn spawn_board_interaction_star(commands: &mut Commands, pos: Vec2, mouse_delta: Vec2) {
    let angle = rand::random::<f32>() * std::f32::consts::TAU;
    let speed = 55.0 + rand::random::<f32>() * 65.0;
    let life = 0.4 + rand::random::<f32>() * 0.2;
    let angular = -std::f32::consts::TAU + rand::random::<f32>() * std::f32::consts::TAU * 2.0;
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
            BoardInteractionStar {
                velocity: Vec2::new(angle.cos(), angle.sin()) * speed,
                angular,
                life,
                max_life: life,
            },
            HitFlash::new(0.375),
        ));
}

fn update_board_interaction(
    mut commands: Commands,
    time: Res<Time>,
    state: Res<GameState>,
    pointer: Res<PrimaryPointer>,
    canvas: Res<CanvasViewport>,
    mut boards: Query<(Entity, &Board, &Transform, &mut LinearVelocity)>,
) {
    if *state != GameState::Playing {
        return;
    }

    let Some(window_position) = pointer.interaction_position() else {
        return;
    };
    if !canvas.contains_window_point(window_position) {
        return;
    }
    let mouse_pos = canvas.window_to_world(window_position);
    let screen_delta = pointer.delta() / canvas.scale();
    let mouse_delta = Vec2::new(screen_delta.x, -screen_delta.y);

    for (entity, board, transform, mut velocity) in &mut boards {
        let local = transform.rotation.inverse()
            * (mouse_pos - transform.translation.truncate()).extend(0.0);
        let half_hit = board.hit_size * 0.5;
        let hovered = local.x.abs() <= half_hit && local.y.abs() <= half_hit;
        if !hovered {
            continue;
        }

        let multiplier = if pointer.pressed() { 3.0 } else { 1.0 };
        let impulse = Vec2::new(mouse_delta.x * board.render, mouse_delta.y * board.render)
            * multiplier
            * time.delta_secs()
            * 0.45;
        velocity.0 = (velocity.0 + impulse).clamp_length_max(220.0);

        if pointer.just_pressed() {
            commands.entity(entity).insert(HitFlash::new(0.25));
            let count = 2 + (rand::random::<f32>() > 0.5) as usize;
            for _ in 0..count {
                spawn_board_interaction_star(&mut commands, mouse_pos, mouse_delta);
            }
        }
    }
}

fn update_board_interaction_stars(
    mut commands: Commands,
    time: Res<Time>,
    mut stars: Query<(
        Entity,
        &mut BoardInteractionStar,
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

fn update_board_plants(
    time: Res<Time>,
    state: Res<GameState>,
    mut plants: Query<(&mut Transform, &mut BoardPlant)>,
) {
    if *state != GameState::Playing {
        return;
    }
    let t = time.elapsed_secs();
    for (mut transform, mut plant) in &mut plants {
        let target = 0.1 * (1.4 * t + plant.phase).sin();
        plant.sway = plant.sway * 0.85 + target * 0.15;
        transform.rotation = Quat::from_rotation_z(plant.sway);
    }
}

/// score/best 牌数值随分数刷新（按 BoardValue 标记定位数值行）。
fn update_board_labels(
    score: Res<Score>,
    best: Res<Best>,
    mut values: Query<(&BoardValue, &mut Text2d)>,
) {
    for (value, mut text) in &mut values {
        let expected = match value.0 {
            BoardKind::Score => (**score).to_string(),
            BoardKind::Best => (**best).to_string(),
            BoardKind::Next => continue,
        };
        if text.0 != expected {
            text.0 = expected;
        }
    }
}

/// next 牌的 emoji 随 NextValue 刷新，并弹一下（原作 next_board:hitfx_use('emoji', 0.5)）。
fn update_board_next(
    next: Res<NextValue>,
    asset_server: Res<AssetServer>,
    mut emoji: Query<(&mut Sprite, &mut Spring), With<BoardNextEmoji>>,
) {
    if !next.is_changed() {
        return;
    }
    for (mut sprite, mut spring) in &mut emoji {
        sprite.image = asset_server.load(emoji_filename(**next));
        sprite.custom_size = Some(Vec2::splat(emoji_radius(**next) * 2.0));
        spring.pull(0.3);
    }
}
