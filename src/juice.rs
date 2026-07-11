//! Juice 系统：弹簧（Spring）+ 闪白（HitFlash）。
//!
//! 这是 emoji-merge「手感」的基础设施，对应原作 anchor 的 spring / flash / hitfx。
//!
//! - [`Spring`]：临界阻尼弹簧积分 `-k(x-target) - d*v`，驱动 scale/位置的弹性反馈。
//! - [`HitFlash`]：受击/合并/出生时的闪白反馈（Sprite.color 渐隐）。
//!
//! 两者都是 ECS 组件 + 系统，挂在需要 juice 的实体上即可。

use bevy::prelude::*;

/// 一维临界阻尼弹簧。
///
/// 典型用法：emoji 的缩放挂一个 Spring，出生时把 `current` 设小、target=0，
/// 系统每帧把它弹回 0，配合 `apply` 系统把 `current` 写进 Transform.scale，
/// 就得到「弹性出现」效果；碰撞时调 [`Spring::pull`] 给一个冲击，得到「弹一下」。
#[derive(Component, Clone, Debug)]
pub struct Spring {
    pub current: f32,
    pub target: f32,
    pub velocity: f32,
    /// 刚度 k：越大越紧、回弹越快。
    pub stiffness: f32,
    /// 阻尼 d：越大越不震荡；≈ 2*sqrt(k) 时临界阻尼（不 overshoot）。
    pub damping: f32,
}

impl Default for Spring {
    fn default() -> Self {
        Self {
            current: 0.0,
            target: 0.0,
            velocity: 0.0,
            // 对齐原作 anchor/spring.lua 默认 k=100/d=10。
            stiffness: 100.0,
            damping: 10.0,
        }
    }
}

impl Spring {
    /// 平滑过渡到新目标（不产生冲击）。
    pub fn animate(&mut self, target: f32) {
        self.target = target;
    }

    /// 施加一次冲击（叠加速度），target 不变。用于「弹一下」的受击反馈。
    pub fn pull(&mut self, impulse: f32) {
        self.velocity += impulse;
    }
}

/// 半隐式 Euler 积分推进所有弹簧。
pub fn update_springs(time: Res<Time>, mut query: Query<&mut Spring>) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    for mut s in &mut query {
        let force = s.stiffness * (s.target - s.current) - s.damping * s.velocity;
        s.velocity += force * dt;
        s.current += s.velocity * dt;
    }
}

/// 闪白反馈组件。挂上后会在 `duration` 秒内从亮色渐隐回原纹理色，然后自动移除。
#[derive(Component)]
pub struct HitFlash {
    elapsed: f32,
    duration: f32,
}

impl HitFlash {
    pub fn new(duration: f32) -> Self {
        Self {
            elapsed: 0.0,
            duration: duration.max(0.0001),
        }
    }
}

/// 闪白用的亮色调（暖白，偏黄），近似原作 combine shader 的闪白观感。
const FLASH_TINT: Color = Color::srgb(1.0, 0.95, 0.75);

/// 推进所有 HitFlash，把 Sprite.color 在亮色→白色之间渐隐，到点移除组件。
pub fn update_hit_flash(
    time: Res<Time>,
    mut commands: Commands,
    mut query: Query<(Entity, &mut HitFlash, &mut Sprite)>,
) {
    for (entity, mut flash, mut sprite) in &mut query {
        flash.elapsed += time.delta_secs();
        let t = (flash.elapsed / flash.duration).clamp(0.0, 1.0);
        // t=0 时最亮（FLASH_TINT），t=1 时回到纯白（= 原纹理色）。
        sprite.color = FLASH_TINT.mix(&Color::WHITE, t);
        if t >= 1.0 {
            sprite.color = Color::WHITE;
            commands.entity(entity).remove::<HitFlash>();
        }
    }
}

pub struct JuicePlugin;

impl Plugin for JuicePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (update_springs, update_hit_flash));
    }
}
