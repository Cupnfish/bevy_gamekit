//! 演示游戏：逐模块验收 TapTap 适配成果的载体。
//!
//! 阶段 0：Bevy 空场景在 Android 真机/桌面跑通。
//! 阶段 1：统一初始化 + 登录验收（点按屏幕发起登录，左上角显示事件日志）。
//!
//! 验收配置：把 clientId/clientToken 填进本 crate 的 `taptap_config.json`。

use bevy::input::touch::TouchPhase;
use bevy::prelude::*;
use taptap_core::{scopes, TapTapConfig};

/// 编译期内嵌的验收配置：build.rs 优先取 gitignore 的 taptap.local.json（真实凭据），
/// 否则用仓库模板（空值）。真实凭据不要提交进仓库。
const CONFIG_JSON: &str = include_str!(concat!(env!("OUT_DIR"), "/taptap_config.json"));

pub fn run() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "TapTap Demo".into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::srgb(0.08, 0.09, 0.12)))
        .init_resource::<LogLines>()
        .init_resource::<CloudSaveTest>()
        .add_plugins(bevy_taptap::TapTapPlugin::new(load_config()))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                spin,
                pulse_color,
                handle_tap,
                consume_events,
                consume_login,
                consume_cloudsave,
                render_log,
            ),
        )
        .run();
}

fn load_config() -> TapTapConfig {
    match serde_json::from_str(CONFIG_JSON) {
        Ok(config) => config,
        Err(e) => {
            error!("taptap_config.json 解析失败: {e}");
            TapTapConfig::new("", "")
        }
    }
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    asset_server: Res<AssetServer>,
) {
    commands.spawn(Camera2d);
    commands.spawn((
        Mesh2d(meshes.add(Rectangle::new(160.0, 160.0))),
        MeshMaterial2d(materials.add(Color::srgb(0.20, 0.55, 1.0))),
        Spin,
    ));
    // 中文日志需 CJK 字形（默认字体只覆盖拉丁字符），内置思源黑体
    let font = asset_server.load("fonts/NotoSansSC-Regular.ttf");
    commands.spawn((
        Text::new("TapTap 验收"),
        TextFont {
            font: FontSource::Handle(font),
            font_size: FontSize::Px(26.0),
            ..default()
        },
        TextColor(Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(16.0),
            left: Val::Px(16.0),
            ..default()
        },
    ));
}

#[derive(Component)]
struct Spin;

fn spin(time: Res<Time>, mut query: Query<&mut Transform, With<Spin>>) {
    for mut transform in &mut query {
        transform.rotate_z(time.delta_secs() * 1.2);
        transform.scale = Vec3::splat(1.0 + 0.15 * (time.elapsed_secs() * 2.0).sin());
    }
}

/// 让方块颜色缓慢呼吸，证明渲染循环与帧时间正常。
fn pulse_color(time: Res<Time>, mut materials: ResMut<Assets<ColorMaterial>>) {
    let t = 0.5 + 0.5 * (time.elapsed_secs() * 1.5).sin();
    for (_, material) in materials.iter_mut() {
        material.color = Color::srgb(0.15 + 0.25 * t, 0.40 + 0.15 * t, 1.0 - 0.2 * t);
    }
}

/// 点按/点击屏幕发起登录（简单防抖：事件处理中不重复发起）。
fn handle_tap(
    mouse: Res<ButtonInput<MouseButton>>,
    mut touches: MessageReader<TouchInput>,
    mut lines: ResMut<LogLines>,
    mut logged: Local<bool>,
) {
    let tapped = mouse.just_pressed(MouseButton::Left)
        || touches
            .read()
            .any(|t| t.phase == TouchPhase::Started);
    if !tapped {
        return;
    }
    if *logged {
        lines.push("已发起过登录，重装应用可重试".into());
        return;
    }
    *logged = true;
    lines.push("发起登录...".into());
    let _ = taptap_android::db_log_event(
        "demo_login_click",
        &serde_json::json!({ "trigger": "tap" }),
    );
    if let Err(e) = taptap_android::login(&[scopes::PUBLIC_PROFILE]) {
        lines.push(format!("登录派发失败: {e}"));
        *logged = false;
    }
}

fn consume_events(
    mut events: MessageReader<bevy_taptap::InitEvent>,
    mut lines: ResMut<LogLines>,
) {
    for event in events.read() {
        let r = &event.0;
        if r.success {
            lines.push("初始化成功 ✓".into());
            let _ = taptap_android::db_log_event(
                "taptap_rs_init_success",
                &serde_json::json!({ "source": "bevy_demo" }),
            );
        } else {
            lines.push(format!(
                "初始化失败: code={:?} msg={:?}",
                r.error_code, r.error_message
            ));
        }
    }
}

/// 登录成功后自动执行的云存档验收脚本状态机。
/// 0=未开始 1=create 2=list 3=read 4=update 5=delete 6=完成
#[derive(Resource, Default)]
struct CloudSaveTest {
    step: u8,
    uuid: Option<String>,
    file_id: Option<String>,
}

const SAVE_NAME: &str = "slot1";
const SAVE_V1: &str = r#"{"level":1,"gold":100,"note":"tapdemo v1"}"#;
const SAVE_V2: &str = r#"{"level":2,"gold":250,"note":"tapdemo v2"}"#;

fn consume_login(
    mut events: MessageReader<bevy_taptap::LoginEvent>,
    mut lines: ResMut<LogLines>,
    mut cs: ResMut<CloudSaveTest>,
) {
    for event in events.read() {
        let r = &event.0;
        match r.status {
            0 => {
                let who = r
                    .account
                    .as_ref()
                    .map(|a| format!("{} ({})", a.name.as_deref().unwrap_or("?"), a.open_id))
                    .unwrap_or_else(|| "?".into());
                lines.push(format!("登录成功 ✓ {who}"));
                if let Some(account) = &r.account {
                    let _ = taptap_android::db_set_user(
                        Some(&account.open_id),
                        &serde_json::json!({ "name": account.name.clone().unwrap_or_default() }),
                    );
                }
                // 进入云存档验收：create → list → read → update → delete
                cs.step = 1;
                match taptap_android::cloud_save_create(SAVE_NAME, "验收存档", "extra-1", 60, SAVE_V1) {
                    Ok(()) => lines.push("云存档: create 已发起".into()),
                    Err(e) => {
                        lines.push(format!("云存档 create 派发失败: {e}"));
                        cs.step = 0;
                    }
                }
            }
            1 => lines.push(format!("登录失败: {:?}", r.error)),
            2 => lines.push("登录被取消".into()),
            _ => {}
        }
    }
}

fn consume_cloudsave(
    mut events: MessageReader<bevy_taptap::CloudSaveEvent>,
    mut lines: ResMut<LogLines>,
    mut cs: ResMut<CloudSaveTest>,
) {
    for event in events.read() {
        let r = &event.0;
        if !r.success {
            lines.push(format!("云存档失败: {:?}", r.error));
            cs.step = 0;
            continue;
        }
        match cs.step {
            1 => {
                cs.uuid = r
                    .archive
                    .as_ref()
                    .and_then(|a| a.get("uuid"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                cs.file_id = r
                    .archive
                    .as_ref()
                    .and_then(|a| a.get("file_id"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                lines.push("云存档: create ✓".into());
                cs.step = 2;
                let _ = taptap_android::cloud_save_list();
            }
            2 => {
                let count = r.archives.as_ref().map(|a| a.len()).unwrap_or(0);
                lines.push(format!("云存档: list ✓ {count} 个"));
                // 重新拉一次 uuid（create 回调里的 uuid 与列表应一致）
                if let Some(item) = r
                    .archives
                    .as_ref()
                    .and_then(|a| a.iter().find(|x| x.get("name").and_then(|n| n.as_str()) == Some(SAVE_NAME)))
                {
                    cs.uuid = item.get("uuid").and_then(|v| v.as_str()).map(str::to_string);
                    cs.file_id = item.get("file_id").and_then(|v| v.as_str()).map(str::to_string);
                }
                if let (Some(uuid), Some(fid)) = (cs.uuid.clone(), cs.file_id.clone()) {
                    cs.step = 3;
                    let _ = taptap_android::cloud_save_read(&uuid, &fid);
                } else {
                    lines.push("云存档: 列表里没找到 slot1，终止".into());
                    cs.step = 0;
                }
            }
            3 => {
                let ok = r.content.as_deref() == Some(SAVE_V1);
                lines.push(format!("云存档: read ✓ 内容一致={ok}"));
                if let Some(uuid) = cs.uuid.clone() {
                    cs.step = 4;
                    let _ = taptap_android::cloud_save_update(&uuid, SAVE_NAME, "验收存档v2", "extra-2", 120, SAVE_V2);
                }
            }
            4 => {
                lines.push("云存档: update ✓".into());
                if let Some(uuid) = cs.uuid.clone() {
                    cs.step = 5;
                    let _ = taptap_android::cloud_save_delete(&uuid);
                }
            }
            5 => {
                lines.push("云存档: delete ✓ —— 阶段3验收完成".into());
                cs.step = 6;
            }
            _ => {}
        }
    }
}

#[derive(Resource, Default)]
struct LogLines(Vec<String>);

impl LogLines {
    fn push(&mut self, line: String) {
        info!("{line}");
        self.0.push(line);
        if self.0.len() > 8 {
            self.0.remove(0);
        }
    }
}

/// 把日志渲染到左上角。
fn render_log(lines: Res<LogLines>, mut query: Query<&mut Text>) {
    let Ok(mut text) = query.single_mut() else {
        return;
    };
    let joined = if lines.0.is_empty() {
        "TapTap 验收：点按屏幕发起登录".to_string()
    } else {
        lines.0.join("\n")
    };
    if text.to_string() != joined {
        **text = joined;
    }
}

/// Android 入口：NativeActivity 胶水层调用本符号（`bevy_main` 在 0.20 中位于 prelude）。
#[cfg(target_os = "android")]
#[bevy_main]
fn main() {
    run();
}
