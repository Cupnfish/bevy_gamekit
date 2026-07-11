//! `bevy-agent` — `bevy_agent_harness` 的命令行客户端。
//!
//! 通过 BRP 驱动一个正在运行的 Bevy 游戏：语义目标输入、状态读取与断言、截图、
//! 以及 JS 脚本（队列 `run` / 实时 `test`，经内建 `boa_engine` 执行）。

use std::{
    any::type_name,
    cell::RefCell,
    error::Error,
    path::{Path, PathBuf},
    str::FromStr,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use bevy::{
    input::keyboard::KeyCode,
    math::Vec2,
    remote::{
        BrpRequest,
        builtin_methods::{
            BRP_GET_RESOURCE_METHOD, BRP_WRITE_MESSAGE_METHOD, BrpGetResourcesParams,
            BrpWriteMessageParams,
        },
    },
};
use bevy_agent_harness::{
    AgentAction, AgentInteractionRequest, AgentInteractionStatus, AgentMouseButton,
    AgentScrollUnit, AgentTargetPoint, AgentTargetRegistry, AgentTextInputMode, CaptureManifest,
    CaptureSequenceRequest, CaptureSequenceStatus,
};
use boa_engine::{
    Context, JsError, JsNativeError, JsResult, JsString, JsValue, NativeFunction, Source,
};
use clap::{Parser, Subcommand};
use image::{ImageBuffer, Rgb, RgbImage, imageops};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

#[path = "support/script_runner.rs"]
mod script_runner;

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Parser)]
#[command(
    name = "bevy-agent",
    about = "Drive a Bevy game and assert its state over BRP (bevy_agent_harness CLI).",
    version
)]
struct Cli {
    /// BRP base URL of the running game.
    #[arg(long, global = true, default_value = "http://127.0.0.1:15702/")]
    url: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show capture + interaction status.
    Status,
    /// Read any reflected resource's JSON value.
    Get { resource: String },
    /// Poll until a resource field satisfies a comparison (assertion primitive).
    WaitFor {
        resource: String,
        field: String,
        expected: String,
        #[arg(long, default_value = "eq")]
        op: CompareOp,
        #[arg(long, default_value_t = 20)]
        timeout: u64,
    },
    /// List semantic targets, optionally filtered by id/label.
    Targets { filter: Option<String> },
    /// Capture a single frame.
    Capture { label: Option<String> },
    /// Capture a sequence of frames.
    Sequence {
        label: String,
        frames: u32,
        interval_ms: Option<f64>,
        delay_frames: Option<u32>,
    },
    /// Click a target.
    Click {
        target: String,
        #[arg(value_parser = parse_mouse_button, default_value = "left")]
        button: AgentMouseButton,
    },
    /// Click at a window position (logical pixels).
    ClickAt {
        x: f32,
        y: f32,
        #[arg(value_parser = parse_mouse_button, default_value = "left")]
        button: AgentMouseButton,
    },
    /// Hover over a target.
    Hover { target: String, frames: Option<u32> },
    /// Move the cursor to a target.
    Move { target: String, frames: Option<u32> },
    /// Move the cursor to a window position.
    MoveAt { x: f32, y: f32, frames: Option<u32> },
    /// Double-click a target.
    DoubleClick {
        target: String,
        #[arg(value_parser = parse_mouse_button, default_value = "left")]
        button: AgentMouseButton,
        gap_frames: Option<u32>,
    },
    /// Press (hold) a mouse button on a target.
    Press {
        target: String,
        #[arg(value_parser = parse_mouse_button, default_value = "left")]
        button: AgentMouseButton,
    },
    /// Release a mouse button.
    Release {
        #[arg(value_parser = parse_mouse_button, default_value = "left")]
        button: AgentMouseButton,
    },
    /// Drag from one target to another.
    Drag {
        from: String,
        to: String,
        frames: Option<u32>,
        #[arg(value_parser = parse_mouse_button, default_value = "left")]
        button: AgentMouseButton,
    },
    /// Drag a target by a delta.
    DragBy {
        target: String,
        dx: f32,
        dy: f32,
        frames: Option<u32>,
        #[arg(value_parser = parse_mouse_button, default_value = "left")]
        button: AgentMouseButton,
    },
    /// Scroll on a target.
    Scroll {
        target: String,
        dx: f32,
        dy: f32,
        #[arg(value_parser = parse_scroll_unit, default_value = "line")]
        unit: AgentScrollUnit,
    },
    /// Scroll at a window position.
    ScrollAt {
        x: f32,
        y: f32,
        dx: f32,
        dy: f32,
        #[arg(value_parser = parse_scroll_unit, default_value = "line")]
        unit: AgentScrollUnit,
    },
    /// Tap a key (Bevy KeyCode name, e.g. `KeyR`, `Space`).
    Key {
        #[arg(value_parser = parse_key_code)]
        key: KeyCode,
    },
    /// Press (hold) a key.
    KeyDown {
        #[arg(value_parser = parse_key_code)]
        key: KeyCode,
    },
    /// Release a key.
    KeyUp {
        #[arg(value_parser = parse_key_code)]
        key: KeyCode,
    },
    /// Type text at the cursor.
    Text {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        text: Vec<String>,
    },
    /// Type text onto a target.
    TextTo {
        target: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        text: Vec<String>,
    },
    /// Wait N frames.
    Wait { frames: u32 },
    /// Run a JS test script with a live agent (drive + assert) via embedded boa.
    Test {
        script: PathBuf,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Submit a queued JS script of input actions.
    #[command(alias = "script")]
    Run {
        script: PathBuf,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Print the normalized request a script would submit (no game needed).
    Plan {
        script: PathBuf,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Build a contact sheet from a capture directory.
    Sheet {
        directory: PathBuf,
        columns: Option<u32>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let url = &cli.url;
    match cli.command {
        Command::Status => {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "capture": get_capture_status(url)?,
                    "interaction": get_interaction_status(url)?,
                }))?
            );
        }
        Command::Get { resource } => {
            let value = get_resource_value(url, &resource)?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        Command::WaitFor {
            resource,
            field,
            expected,
            op,
            timeout,
        } => {
            let value = wait_for_resource(url, &resource, &field, op, &expected, timeout)?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        Command::Targets { filter } => {
            let mut registry = get_targets(url)?;
            if let Some(filter) = filter {
                registry.targets.retain(|target| {
                    target.id.contains(&filter)
                        || target
                            .label
                            .as_ref()
                            .is_some_and(|label| label.contains(&filter))
                });
            }
            println!("{}", serde_json::to_string_pretty(&registry)?);
        }
        Command::Capture { label } => {
            let label = label.as_deref().unwrap_or("agent");
            submit_capture_and_wait(url, CaptureSequenceRequest::single(label))?;
        }
        Command::Sequence {
            label,
            frames,
            interval_ms,
            delay_frames,
        } => {
            let interval_ms = interval_ms.unwrap_or(1000.0 / 60.0);
            let mut request = CaptureSequenceRequest::sequence(label, frames);
            request.frame_interval_seconds = interval_ms / 1000.0;
            request.delay_frames = delay_frames.unwrap_or(0);
            submit_capture_and_wait(url, request)?;
        }
        Command::Click { target, button } => {
            submit_action(
                url,
                AgentAction::Click {
                    point: AgentTargetPoint::center(target),
                    button,
                },
            )?;
        }
        Command::ClickAt { x, y, button } => {
            submit_action(
                url,
                AgentAction::ClickAt {
                    position: Vec2::new(x, y),
                    button,
                },
            )?;
        }
        Command::Hover { target, frames } => {
            submit_action(
                url,
                AgentAction::Hover {
                    point: AgentTargetPoint::center(target),
                    frames: frames.unwrap_or(12),
                },
            )?;
        }
        Command::Move { target, frames } => {
            submit_action(
                url,
                AgentAction::MoveTo {
                    point: AgentTargetPoint::center(target),
                    frames: frames.unwrap_or(1),
                },
            )?;
        }
        Command::MoveAt { x, y, frames } => {
            submit_action(
                url,
                AgentAction::MoveToPoint {
                    position: Vec2::new(x, y),
                    frames: frames.unwrap_or(1),
                },
            )?;
        }
        Command::DoubleClick {
            target,
            button,
            gap_frames,
        } => {
            submit_action(
                url,
                AgentAction::DoubleClick {
                    point: AgentTargetPoint::center(target),
                    button,
                    gap_frames: gap_frames.unwrap_or(2),
                },
            )?;
        }
        Command::Press { target, button } => {
            submit_action(
                url,
                AgentAction::Press {
                    point: AgentTargetPoint::center(target),
                    button,
                },
            )?;
        }
        Command::Release { button } => {
            submit_action(
                url,
                AgentAction::Release {
                    point: None,
                    button,
                },
            )?;
        }
        Command::Drag {
            from,
            to,
            frames,
            button,
        } => {
            submit_action(
                url,
                AgentAction::DragTo {
                    from: AgentTargetPoint::center(from),
                    to: AgentTargetPoint::center(to),
                    button,
                    frames: frames.unwrap_or(12),
                },
            )?;
        }
        Command::DragBy {
            target,
            dx,
            dy,
            frames,
            button,
        } => {
            submit_action(
                url,
                AgentAction::DragBy {
                    from: AgentTargetPoint::center(target),
                    delta: Vec2::new(dx, dy),
                    button,
                    frames: frames.unwrap_or(12),
                },
            )?;
        }
        Command::Scroll {
            target,
            dx,
            dy,
            unit,
        } => {
            submit_action(
                url,
                AgentAction::Scroll {
                    point: AgentTargetPoint::center(target),
                    delta: Vec2::new(dx, dy),
                    unit,
                },
            )?;
        }
        Command::ScrollAt { x, y, dx, dy, unit } => {
            submit_action(
                url,
                AgentAction::ScrollAt {
                    position: Vec2::new(x, y),
                    delta: Vec2::new(dx, dy),
                    unit,
                },
            )?;
        }
        Command::Key { key } => submit_action(url, AgentAction::KeyTap { key })?,
        Command::KeyDown { key } => submit_action(url, AgentAction::KeyPress { key })?,
        Command::KeyUp { key } => submit_action(url, AgentAction::KeyRelease { key })?,
        Command::Text { text } => {
            submit_action(
                url,
                AgentAction::TextInput {
                    point: None,
                    text: text.join(" "),
                    mode: AgentTextInputMode::Keyboard,
                },
            )?;
        }
        Command::TextTo { target, text } => {
            submit_action(
                url,
                AgentAction::TextInput {
                    point: Some(AgentTargetPoint::center(target)),
                    text: text.join(" "),
                    mode: AgentTextInputMode::Keyboard,
                },
            )?;
        }
        Command::Wait { frames } => submit_action(url, AgentAction::WaitFrames { frames })?,
        Command::Test { script, args } => run_live_test(url, &script, &args)?,
        Command::Run { script, args } => {
            let mut request =
                script_runner::load_request(&script, &args).map_err(|e| e.to_string())?;
            if request.request_id.is_empty() {
                request.request_id = interaction_request_id();
            }
            submit_interaction_and_wait(url, request)?;
        }
        Command::Plan { script, args } => {
            let request = script_runner::load_request(&script, &args).map_err(|e| e.to_string())?;
            println!("{}", serde_json::to_string_pretty(&request)?);
        }
        Command::Sheet { directory, columns } => {
            let output = build_contact_sheet(&directory, columns.unwrap_or(6))?;
            println!("{}", output.display());
        }
    }
    Ok(())
}

// ---- BRP 客户端 ----

fn submit_capture_and_wait(url: &str, request: CaptureSequenceRequest) -> Result<()> {
    let before = get_capture_status(url)?.last_completed_session;
    brp_request(
        url,
        BRP_WRITE_MESSAGE_METHOD,
        &BrpWriteMessageParams {
            message: type_name::<CaptureSequenceRequest>().to_string(),
            value: Some(serde_json::to_value(request)?),
        },
    )?;

    let started = Instant::now();
    loop {
        let status = get_capture_status(url)?;
        if status.last_completed_session != before
            && let Some(manifest) = status.last_manifest
        {
            println!("{manifest}");
            return Ok(());
        }
        if let Some(error) = status.last_error {
            return Err(error.into());
        }
        if started.elapsed() > Duration::from_secs(60) {
            return Err("timed out waiting for capture".into());
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn get_capture_status(url: &str) -> Result<CaptureSequenceStatus> {
    get_resource(url)
}

fn get_interaction_status(url: &str) -> Result<AgentInteractionStatus> {
    get_resource(url)
}

fn get_targets(url: &str) -> Result<AgentTargetRegistry> {
    get_resource(url)
}

fn get_resource<T: DeserializeOwned>(url: &str) -> Result<T> {
    let response = brp_request(
        url,
        BRP_GET_RESOURCE_METHOD,
        &BrpGetResourcesParams {
            resource: type_name::<T>().to_string(),
        },
    )?;
    Ok(serde_json::from_value(response["result"]["value"].clone())?)
}

/// 按完整类型路径读取任意反射资源的 JSON 值。供 `get` 命令与 live `__get` 复用。
fn get_resource_value(url: &str, resource: &str) -> Result<Value> {
    let response = brp_request(
        url,
        BRP_GET_RESOURCE_METHOD,
        &BrpGetResourcesParams {
            resource: resource.to_string(),
        },
    )?;
    Ok(response["result"]["value"].clone())
}

/// 点号字段路径 → JSON 指针（`a.b` → `/a/b`）。
fn field_pointer(field: &str) -> String {
    field
        .split('.')
        .filter(|part| !part.is_empty())
        .fold(String::new(), |mut acc, part| {
            acc.push('/');
            acc.push_str(part);
            acc
        })
}

/// 轮询资源直到字段满足比较关系；匹配返回完整资源值，超时返回 Err。
fn wait_for_resource(
    url: &str,
    resource: &str,
    field: &str,
    op: CompareOp,
    expected: &str,
    timeout_secs: u64,
) -> Result<Value> {
    let pointer = field_pointer(field);
    let started = Instant::now();
    let mut last_error = String::new();
    let mut last_value = Value::Null;
    loop {
        match get_resource_value(url, resource) {
            Ok(full) => {
                last_error.clear();
                last_value = full.pointer(&pointer).cloned().unwrap_or(Value::Null);
                let actual = match &last_value {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                if value_matches(op, &actual, expected) {
                    return Ok(full);
                }
            }
            Err(error) => last_error = error.to_string(),
        }
        if started.elapsed() > Duration::from_secs(timeout_secs) {
            return Err(format!(
                "timed out waiting for `{resource}.{field}` ({op} `{expected}`); \
                 last value: {last_value}{}",
                if last_error.is_empty() {
                    String::new()
                } else {
                    format!("; last error: {last_error}")
                }
            )
            .into());
        }
        thread::sleep(Duration::from_millis(150));
    }
}

fn submit_action(url: &str, action: AgentAction) -> Result<()> {
    submit_interaction_and_wait(
        url,
        AgentInteractionRequest::single(interaction_request_id(), action),
    )
}

fn submit_interaction_and_wait(url: &str, request: AgentInteractionRequest) -> Result<()> {
    let request_id = request.request_id.clone();
    brp_request(
        url,
        BRP_WRITE_MESSAGE_METHOD,
        &BrpWriteMessageParams {
            message: type_name::<AgentInteractionRequest>().to_string(),
            value: Some(serde_json::to_value(request)?),
        },
    )?;

    let started = Instant::now();
    loop {
        let status = get_interaction_status(url)?;
        if status.last_completed_request.as_deref() == Some(request_id.as_str()) {
            println!("{}", serde_json::to_string_pretty(&status)?);
            return Ok(());
        }
        if status.last_failed_request.as_deref() == Some(request_id.as_str()) {
            return Err(status
                .last_error
                .unwrap_or_else(|| "interaction failed".to_string())
                .into());
        }
        if started.elapsed() > Duration::from_secs(30) {
            return Err(format!("timed out waiting for interaction `{request_id}`").into());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

/// `wait-for` 的比较运算符。经 clap `--op` 直接解析为枚举（实现 `FromStr`），
/// 非法值在解析期被 clap 拒绝，`value_matches` 因此可写为穷尽匹配。
#[derive(Clone, Copy, Debug)]
enum CompareOp {
    Eq,
    Ne,
    Ge,
    Le,
    Gt,
    Lt,
    Contains,
}

impl std::str::FromStr for CompareOp {
    type Err = Box<dyn Error + Send + Sync>;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "eq" => Ok(Self::Eq),
            "ne" => Ok(Self::Ne),
            "ge" => Ok(Self::Ge),
            "le" => Ok(Self::Le),
            "gt" => Ok(Self::Gt),
            "lt" => Ok(Self::Lt),
            "contains" => Ok(Self::Contains),
            _ => {
                Err(format!("unknown op `{s}`; expected one of eq|ne|ge|le|gt|lt|contains").into())
            }
        }
    }
}

impl std::fmt::Display for CompareOp {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(match self {
            Self::Eq => "eq",
            Self::Ne => "ne",
            Self::Ge => "ge",
            Self::Le => "le",
            Self::Gt => "gt",
            Self::Lt => "lt",
            Self::Contains => "contains",
        })
    }
}

/// `wait-for` 的比较关系。`Eq`/`Ne`/`Contains` 按字符串；`Ge`/`Le`/`Gt`/`Lt` 按浮点数。
fn value_matches(op: CompareOp, actual: &str, expected: &str) -> bool {
    use CompareOp::*;
    match op {
        Eq => actual == expected,
        Ne => actual != expected,
        Contains => actual.contains(expected),
        Ge | Le | Gt | Lt => match (actual.parse::<f64>(), expected.parse::<f64>()) {
            (Ok(a), Ok(e)) => match op {
                Ge => a >= e,
                Le => a <= e,
                Gt => a > e,
                Lt => a < e,
                Eq | Ne | Contains => false,
            },
            _ => false,
        },
    }
}

fn parse_mouse_button(value: &str) -> Result<AgentMouseButton> {
    // 大小写不敏感；`other:N` 表示其它按键（只剥一次前缀，避免 `other:other:5` 被误解析）。
    let lower = value.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("other:") {
        return Ok(AgentMouseButton::Other(rest.parse()?));
    }
    match lower.as_str() {
        "left" => Ok(AgentMouseButton::Left),
        "right" => Ok(AgentMouseButton::Right),
        "middle" => Ok(AgentMouseButton::Middle),
        "back" => Ok(AgentMouseButton::Back),
        "forward" => Ok(AgentMouseButton::Forward),
        _ => Err(format!(
            "unknown mouse button `{value}`; expected left|right|middle|back|forward|other:N"
        )
        .into()),
    }
}

fn parse_scroll_unit(value: &str) -> Result<AgentScrollUnit> {
    match value.to_ascii_lowercase().as_str() {
        "line" | "lines" => Ok(AgentScrollUnit::Line),
        "pixel" | "pixels" => Ok(AgentScrollUnit::Pixel),
        _ => Err(format!("unknown scroll unit `{value}`; expected line|pixel").into()),
    }
}

fn parse_key_code(value: &str) -> Result<KeyCode> {
    serde_json::from_value(Value::String(value.to_string()))
        .map_err(|error| format!("invalid Bevy KeyCode `{value}`: {error}").into())
}

fn interaction_request_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("cli-{nanos}")
}

fn build_contact_sheet(directory: &Path, columns: u32) -> Result<PathBuf> {
    let manifest: CaptureManifest =
        serde_json::from_slice(&std::fs::read(directory.join("manifest.json"))?)?;
    if manifest.frames.is_empty() {
        return Err("manifest contains no frames".into());
    }

    const TILE_WIDTH: u32 = 320;
    const TILE_HEIGHT: u32 = 180;
    const GAP: u32 = 8;
    let columns = columns.max(1).min(manifest.frames.len() as u32);
    let rows = (manifest.frames.len() as u32).div_ceil(columns);
    let width = columns * TILE_WIDTH + (columns + 1) * GAP;
    let height = rows * TILE_HEIGHT + (rows + 1) * GAP;
    let mut sheet: RgbImage = ImageBuffer::from_pixel(width, height, Rgb([28, 31, 36]));

    for (slot, frame) in manifest.frames.iter().enumerate() {
        let image = image::open(directory.join(&frame.path))?;
        let thumbnail = image.thumbnail(TILE_WIDTH, TILE_HEIGHT).to_rgb8();
        let column = slot as u32 % columns;
        let row = slot as u32 / columns;
        let x = GAP + column * TILE_WIDTH + (TILE_WIDTH - thumbnail.width()) / 2;
        let y = GAP + row * TILE_HEIGHT + (TILE_HEIGHT - thumbnail.height()) / 2;
        imageops::replace(&mut sheet, &thumbnail, i64::from(x), i64::from(y));
    }

    let output = directory.join("contact_sheet.png");
    sheet.save(&output)?;
    Ok(output)
}

// ---- live 测试模式（boa） ----

/// `bevy-agent test <script.js>`：用内建 boa 运行 JS，注入实时 `agent`。预置 JS 见 `support/test_agent.js`。
const TEST_AGENT_PREAMBLE: &str = include_str!("support/test_agent.js");

thread_local! {
    /// live 宿主函数读取的 BRP URL（单线程同步执行，故用 thread_local 传递）。
    static LIVE_URL: RefCell<String> = const { RefCell::new(String::new()) };
}

fn live_url() -> String {
    LIVE_URL.with(|cell| cell.borrow().clone())
}

/// 构造一个 TypeError `JsError`（boa 1.0 中 `.into()` 目标歧义，故显式类型化）。
fn js_err(message: impl std::fmt::Display) -> JsError {
    JsNativeError::typ()
        .with_message(message.to_string())
        .into()
}

/// 取第 `idx` 个字符串参数，缺省/非字符串则抛 TypeError。
fn js_string_arg(args: &[JsValue], idx: usize, name: &str) -> JsResult<String> {
    args.get(idx)
        .and_then(|value| value.as_string())
        .map(|string| string.to_std_string_escaped())
        .ok_or_else(|| js_err(format!("missing {name}")))
}

fn js_act(_this: &JsValue, args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let url = live_url();
    let json = js_string_arg(args, 0, "action JSON")?;
    let action: AgentAction = serde_json::from_str(&json)
        .map_err(|error| js_err(format!("invalid action JSON: {error}")))?;
    submit_action(&url, action).map_err(|error| js_err(format!("action failed: {error}")))?;
    Ok(JsValue::undefined())
}

fn js_get(_this: &JsValue, args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let url = live_url();
    let resource = js_string_arg(args, 0, "resource")?;
    let value = get_resource_value(&url, &resource)
        .map_err(|error| js_err(format!("get failed: {error}")))?;
    Ok(JsValue::new(JsString::from(
        serde_json::to_string(&value).unwrap_or_else(|_| "null".to_string()),
    )))
}

fn js_wait_for(_this: &JsValue, args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let url = live_url();
    let resource = js_string_arg(args, 0, "resource")?;
    let field = js_string_arg(args, 1, "field")?;
    let expected = js_string_arg(args, 2, "expected")?;
    let op = args
        .get(3)
        .and_then(|value| value.as_string())
        .map(|string| string.to_std_string_escaped())
        .unwrap_or_else(|| "eq".to_string());
    let op = CompareOp::from_str(&op).map_err(js_err)?;
    let timeout = args
        .get(4)
        .and_then(|value| value.as_number())
        .map(|number| number as u64)
        .unwrap_or(20);
    let value = wait_for_resource(&url, &resource, &field, op, &expected, timeout)
        .map_err(|error| js_err(format!("wait_for failed: {error}")))?;
    Ok(JsValue::new(JsString::from(
        serde_json::to_string(&value).unwrap_or_else(|_| "null".to_string()),
    )))
}

fn js_targets(_this: &JsValue, args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let url = live_url();
    let filter = args
        .first()
        .and_then(|value| value.as_string())
        .map(|string| string.to_std_string_escaped())
        .unwrap_or_default();
    let mut registry =
        get_targets(&url).map_err(|error| js_err(format!("targets failed: {error}")))?;
    if !filter.is_empty() {
        registry.targets.retain(|target| {
            target.id.contains(&filter)
                || target
                    .label
                    .as_ref()
                    .is_some_and(|label| label.contains(&filter))
        });
    }
    Ok(JsValue::new(JsString::from(
        serde_json::to_string(&registry.targets).unwrap_or_else(|_| "[]".to_string()),
    )))
}

fn js_print(_this: &JsValue, args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let text = args
        .iter()
        .map(|value| match value.as_string() {
            Some(string) => string.to_std_string_escaped(),
            None => format!("{value:?}"),
        })
        .collect::<Vec<_>>()
        .join(" ");
    println!("{text}");
    Ok(JsValue::undefined())
}

fn run_live_test(url: &str, script: &Path, args: &[String]) -> Result<()> {
    LIVE_URL.with(|cell| *cell.borrow_mut() = url.to_string());
    let source = std::fs::read_to_string(script)?;
    let args_json = serde_json::to_string(args)?;
    let program = format!("{TEST_AGENT_PREAMBLE}\nvar agentArgs = {args_json};\n{source}\n");

    let mut context = Context::default();
    // register_global_callable 返回 boa 的 JsError（内部用 Rc，非 Send/Sync），这里转成 String 错误。
    context
        .register_global_callable(
            JsString::from("__act"),
            1,
            NativeFunction::from_fn_ptr(js_act),
        )
        .map_err(|e| e.to_string())?;
    context
        .register_global_callable(
            JsString::from("__get"),
            1,
            NativeFunction::from_fn_ptr(js_get),
        )
        .map_err(|e| e.to_string())?;
    context
        .register_global_callable(
            JsString::from("__wait_for"),
            5,
            NativeFunction::from_fn_ptr(js_wait_for),
        )
        .map_err(|e| e.to_string())?;
    context
        .register_global_callable(
            JsString::from("__targets"),
            1,
            NativeFunction::from_fn_ptr(js_targets),
        )
        .map_err(|e| e.to_string())?;
    context
        .register_global_callable(
            JsString::from("print"),
            0,
            NativeFunction::from_fn_ptr(js_print),
        )
        .map_err(|e| e.to_string())?;

    context
        .eval(Source::from_bytes(&program))
        .map_err(|error| format!("test script error: {error}"))?;
    Ok(())
}

fn brp_request(url: &str, method: &str, params: &impl serde::Serialize) -> Result<Value> {
    let request = BrpRequest {
        method: method.to_string(),
        id: Some(json!(1)),
        params: Some(serde_json::to_value(params)?),
    };
    let mut response = ureq::post(url).send_json(request)?;
    let response: Value = response.body_mut().read_json()?;
    if let Some(error) = response.get("error") {
        return Err(format!("BRP error: {error}").into());
    }
    Ok(response)
}
