//! 音频系统：复刻原版 emoji-merge 的 `sfx`/`music` 双 sound_tag + sounds 表。
//!
//! 原版（anchor/sound.lua、sound_tag.lua、main.lua:129-145,203-235）：
//! - `sfx`/`music` 两个**独立** sound_tag，各有 `level_to_volume` 等级（0..=4）。
//! - `sounds.<name> = sound(file, {tag = ...})`，`sound_play(base, pitch)` 的
//!   effective volume = `base × tag.level_volume`；pitch 多为 `random_float(0.95,1.05)`。
//! - BGM：`music_player_play_song(closed_shop, 0.375)` → `0.375 × music.level` 循环；
//!   `arena:end_round()` 时 `music_player_stop()`，重新 `arena:enter()` 再播。
//!
//! bevy_audio 的 `GlobalVolume` 是单总线，无法做独立双通道（曾因此让 sfx/music 旋钮
//! 互相串扰）。这里改为：SFX 每次播放时**预乘** `base × LEVEL_TO_VOLUME[sfx_level]`
//! （对应 `instance:setVolume(volume*tag.volume)`），BGM 用 `AudioSink` 每帧实时调
//! `0.375 × LEVEL_TO_VOLUME[music_level]`。`GlobalVolume` 保持默认 1.0（identity）。

#[cfg(feature = "web_audio")]
use bevy::audio::{AudioPlayer, AudioSink, AudioSinkPlayback, PlaybackSettings, Volume};
use bevy::prelude::*;

#[cfg(feature = "web_audio")]
use crate::game::GameState;

/// 原版 `level_to_volume = {0, 0.0625, 0.125, 0.25, 0.5}`（线性，等级 0..=4）。
#[cfg(feature = "web_audio")]
pub const LEVEL_TO_VOLUME: [f32; 5] = [0.0, 0.0625, 0.125, 0.25, 0.5];

/// 原版 BGM `music_player_play_song(closed_shop, 0.375)` 的 base 音量。
#[cfg(feature = "web_audio")]
const BGM_VOLUME: f32 = 0.375;

/// sfx/music 音量等级（原作 sfx_sound_level/music_sound_level，0..=4，持久化）。
#[derive(Resource, Clone, Copy)]
pub struct SoundLevels {
    pub sfx: u8,
    pub music: u8,
}

impl SoundLevels {
    /// sfx tag 的线性音量（原作 `sfx.volume = level_to_volume[level+1]`）。
    #[cfg(feature = "web_audio")]
    pub fn sfx_volume(self) -> f32 {
        LEVEL_TO_VOLUME[self.sfx.min(4) as usize]
    }

    /// music tag 的线性音量。
    #[cfg(feature = "web_audio")]
    pub fn music_volume(self) -> f32 {
        LEVEL_TO_VOLUME[self.music.min(4) as usize]
    }
}

/// 原版 `sounds` 表里的每条声音（文件名 / base 音量 / 是否带 pitch 抖动）。
#[derive(Clone, Copy)]
pub enum GameSound {
    Drop,               // パパッ        base 0.6  pitched  (main.lua:703)
    Merge1,             // スイッチを押す base 0.4  pitched  (756)
    Merge2,             // ぷよん        base 0.4  pitched  (757)
    FinalMerge,         // 可愛い動作1   base 0.5  pitched  (768)
    ItsOver,            // ショック1     base 0.75 固定1.0  (905)
    ButtonPress,        // カーソル移動2 base 1.0  pitched  (210/219; 230/235=0.5)
    ButtonHover,        // hover         base 1.0  pitched  (610/1487)
    EndRoundRetry,      // se_19         base 0.75 pitched  (638)
    EndRoundRetryPress, // se_17         base 1.0  固定1.0  (631)
    EndRoundScore,      // se_13         base 0.75 固定1.0  (904)
    EndRoundFall,       // se_11         base 1.0  pitched  (836)
    EndRound,           // se_14         base 1.0  pitched  (777)
    DeathHit,           // se_22         base 0.5  pitched  (808)
}

#[cfg(feature = "web_audio")]
struct Def {
    file: &'static str,
    base: f32,
    pitched: bool,
}

impl GameSound {
    #[cfg(feature = "web_audio")]
    const fn def(self) -> Def {
        match self {
            GameSound::Drop => Def {
                file: "パパッ.ogg",
                base: 0.6,
                pitched: true,
            },
            GameSound::Merge1 => Def {
                file: "スイッチを押す.ogg",
                base: 0.4,
                pitched: true,
            },
            GameSound::Merge2 => Def {
                file: "ぷよん.ogg",
                base: 0.4,
                pitched: true,
            },
            GameSound::FinalMerge => Def {
                file: "可愛い動作1.ogg",
                base: 0.5,
                pitched: true,
            },
            GameSound::ItsOver => Def {
                file: "ショック1.ogg",
                base: 0.75,
                pitched: false,
            },
            GameSound::ButtonPress => Def {
                file: "カーソル移動2.ogg",
                base: 1.0,
                pitched: true,
            },
            GameSound::ButtonHover => Def {
                file: "hover.ogg",
                base: 1.0,
                pitched: true,
            },
            GameSound::EndRoundRetry => Def {
                file: "se_19.ogg",
                base: 0.75,
                pitched: true,
            },
            GameSound::EndRoundRetryPress => Def {
                file: "se_17.ogg",
                base: 1.0,
                pitched: false,
            },
            GameSound::EndRoundScore => Def {
                file: "se_13.ogg",
                base: 0.75,
                pitched: false,
            },
            GameSound::EndRoundFall => Def {
                file: "se_11.ogg",
                base: 1.0,
                pitched: true,
            },
            GameSound::EndRound => Def {
                file: "se_14.ogg",
                base: 1.0,
                pitched: true,
            },
            GameSound::DeathHit => Def {
                file: "se_22.ogg",
                base: 0.5,
                pitched: true,
            },
        }
    }
}

/// 播放一个 SFX（原作 `sound:sound_play(base, pitch)`，effective = `base × tag.volume`）。
///
/// `base_override` 覆盖该音效的默认 base（如 screen/close 按钮 main.lua:230,235 base=0.5）。
pub fn play_sfx(
    commands: &mut Commands,
    asset_server: &AssetServer,
    levels: &SoundLevels,
    sound: GameSound,
    base_override: Option<f32>,
) {
    #[cfg(not(feature = "web_audio"))]
    {
        let _ = (commands, asset_server, levels, sound, base_override);
        return;
    }
    #[cfg(feature = "web_audio")]
    {
        let def = sound.def();
        let base = base_override.unwrap_or(def.base);
        let vol = base * levels.sfx_volume();
        let pitch = if def.pitched {
            0.95 + rand::random::<f32>() * 0.1
        } else {
            1.0
        };
        commands.spawn((
            AudioPlayer::new(asset_server.load(def.file)),
            PlaybackSettings::DESPAWN
                .with_volume(Volume::Linear(vol))
                .with_speed(pitch),
        ));
    }
}

/// 标记循环 BGM 实体（音量由 `update_bgm` 用 `AudioSink` 实时调）。
#[derive(Component)]
#[cfg(feature = "web_audio")]
pub struct Bgm;

/// `update_bgm` 对 BGM sink 执行的动作（供 `AudioProbe` 经 BRP 反射读取做断言）。
#[derive(Reflect, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum BgmCtl {
    /// 尚未发生任何状态切换。
    #[default]
    None,
    /// `sink.play()`（恢复，对应进入 Playing）。
    Playing,
    /// `sink.pause()`（暂停，对应进入 GameOver）。
    Paused,
}

/// BGM 音频状态探针（Reflect + Resource，经 BRP `world.get_resources` 可读）。
/// 用于自动化测试：断言游戏结束/重开时音乐是否正确暂停/恢复。
#[derive(Resource, Reflect, Default, Clone, Copy, Debug)]
#[reflect(Resource)]
pub struct AudioProbe {
    /// 最近一次对 BGM sink 的动作。
    pub last_action: BgmCtl,
    /// BGM 实体是否已挂上 `AudioSink`（音频后端就绪）。
    pub sink_present: bool,
    /// 最近一次写入的音量（线性，= `0.375 × music 等级`）。
    pub volume: f32,
    /// 累计响应过的状态切换次数（Playing↔GameOver）。
    pub transitions: u32,
}

/// BGM 维护：每帧把音量设为 `0.375 × music.level`（原作 sound_update 每帧重应用
/// tag.volume）；Playing→GameOver 暂停、GameOver→Playing 恢复（原作
/// music_player_stop / arena:enter 再播，pause/resume 单实体更稳且听觉等价）。
/// 同时把可观测状态写入 `AudioProbe` 供 BRP 断言。
#[cfg(feature = "web_audio")]
pub fn update_bgm(
    mut bgm_sinks: Query<&mut AudioSink, With<Bgm>>,
    state: Res<GameState>,
    levels: Res<SoundLevels>,
    mut probe: ResMut<AudioProbe>,
    mut prev: Local<Option<GameState>>,
) {
    let vol = BGM_VOLUME * levels.music_volume();
    probe.volume = vol;
    let Ok(mut sink) = bgm_sinks.single_mut() else {
        probe.sink_present = false;
        return;
    };
    probe.sink_present = true;
    sink.set_volume(Volume::Linear(vol));

    let curr = *state;
    if *prev != Some(curr) {
        match curr {
            GameState::GameOver => {
                sink.pause();
                probe.last_action = BgmCtl::Paused;
            }
            GameState::Playing => {
                sink.play();
                probe.last_action = BgmCtl::Playing;
            }
        }
        probe.transitions += 1;
        *prev = Some(curr);
    }
}

#[cfg(not(feature = "web_audio"))]
pub fn update_bgm() {}
