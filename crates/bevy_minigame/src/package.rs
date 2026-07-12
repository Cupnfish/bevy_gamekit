use std::{
    fs::{self, File},
    io::{BufReader, Read},
    path::{Component, Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};
use bevy_minigame_wasm_split::package::{SplitOptions, normalize_bindgen_glue, split};
use serde::{Deserialize, Serialize};

const ADAPTER_JS: &str = include_str!("../runtime/adapter.js");
const GAME_JS: &str = include_str!("../runtime/game.js.template");
const SUBPACKAGE_ENTRY_JS: &str =
    "// Runtime modules are required by the main package after this subpackage loads.\n";
const DEFAULT_PRESERVED_PROJECT_FILES: [&str; 3] = [
    "project.config.json",
    "project.private.config.json",
    "icon.png",
];
const GENERATED_PROJECT_CONFIG_KEYS: [&str; 5] = [
    "appid",
    "projectname",
    "description",
    "compileType",
    "douyinProjectType",
];
const GENERATED_ROOT_FILES: [&str; 7] = [
    "__wasm_split.js",
    "brotli_decoder.js",
    "brotli_decoder_bg.wasm",
    "game_wasm.js",
    "game_wasm_bg.wasm",
    "game_wasm_bg.wasm.br",
    "wasm-split-report.json",
];

/// Platform-specific configuration consumed by the generic packaging engine.
pub trait MiniGamePlatform {
    /// Stable lowercase platform identifier used by the JavaScript adapter.
    fn id(&self) -> &'static str;

    /// Human-readable platform name used in diagnostics.
    fn display_name(&self) -> &'static str;

    /// JavaScript API global exposed by the host, such as `tt` or `wx`.
    fn api_global(&self) -> &'static str;

    /// Enables framebuffer and simulator-mirroring fixes specific to Douyin.
    fn douyin_workarounds(&self) -> bool {
        false
    }

    /// Uses a host WebAssembly implementation that instantiates package paths
    /// directly instead of accepting Wasm bytes.
    fn webassembly_path_only(&self) -> bool {
        false
    }

    /// Installs the clock fallback required by host performance wrappers that
    /// hide their `ProfileGlobal` object inside a closure.
    fn profile_clock_retry(&self) -> bool {
        false
    }

    /// Caps the physical size of the onscreen render target while preserving
    /// the Mini Game's logical/CSS dimensions and input coordinates.
    fn max_render_resolution(&self) -> Option<(u32, u32)> {
        None
    }

    /// Places the generated Wasm runtime under this Mini Game subpackage root.
    fn runtime_subpackage(&self) -> Option<&'static str> {
        None
    }

    /// Places copied game assets under this Mini Game subpackage root. This is
    /// separate from `runtime_subpackage` because some hosts keep the runtime
    /// and assets in different package locations.
    fn asset_subpackage(&self) -> Option<&'static str> {
        None
    }

    /// Maximum number of bytes allowed in the platform's main package.
    fn max_main_package_bytes(&self) -> Option<u64> {
        None
    }

    /// Generates the project file consumed by the official developer tool.
    fn project_config(&self, options: &PackageOptions) -> serde_json::Value;

    /// Generates the Mini Game runtime configuration.
    fn game_config(&self, options: &PackageOptions) -> serde_json::Value {
        serde_json::json!({
            "deviceOrientation": options.orientation.as_str(),
            "showStatusBar": false
        })
    }

    /// Developer-tool-owned files retained when rebuilding an open project.
    fn preserved_project_files(&self) -> &'static [&'static str] {
        &DEFAULT_PRESERVED_PROJECT_FILES
    }
}

#[derive(Clone, Debug)]
pub struct PackageOptions {
    pub wasm: PathBuf,
    pub assets: Option<PathBuf>,
    pub output: PathBuf,
    pub app_id: String,
    pub project_name: String,
    pub orientation: Orientation,
    pub wasm_bindgen: PathBuf,
    pub exclude_extensions: Vec<String>,
    pub wasm_split_decoder: Option<PathBuf>,
    pub wasm_opt: Option<PathBuf>,
    pub compression_quality: u32,
    pub max_total_bytes: Option<u64>,
}

#[derive(Clone, Copy, Debug)]
pub enum Orientation {
    Landscape,
    Portrait,
}

impl Orientation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Landscape => "landscape",
            Self::Portrait => "portrait",
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageReport {
    pub output: PathBuf,
    pub wasm_bytes: u64,
    pub compressed_wasm_bytes: u64,
    pub asset_bytes: u64,
    pub runtime_bytes: u64,
    pub total_bytes: u64,
    #[serde(default)]
    pub main_package_bytes: u64,
    #[serde(default)]
    pub subpackage_bytes: u64,
    pub files: Vec<PackageFile>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageFile {
    pub path: PathBuf,
    pub bytes: u64,
}

pub fn package(platform: &dyn MiniGamePlatform, options: &PackageOptions) -> Result<PackageReport> {
    validate_options(options)?;
    let staging = sibling_path(&options.output, "staging");
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;

    let result = package_to(platform, options, &staging);
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
        return result;
    }
    let report = result?;
    replace_output(&staging, &options.output)?;
    Ok(report)
}

fn package_to(
    platform: &dyn MiniGamePlatform,
    options: &PackageOptions,
    staging: &Path,
) -> Result<PackageReport> {
    let mut staging_options = options.clone();
    staging_options.output = staging.to_path_buf();

    run_wasm_bindgen(&staging_options)?;
    export_bindgen_module(&staging.join("game_wasm.js"))?;
    if let Some(decoder_wasm) = &options.wasm_split_decoder {
        let report = split(&SplitOptions {
            original_wasm: options.wasm.clone(),
            bindgen_wasm: staging.join("game_wasm_bg.wasm"),
            bindgen_js: staging.join("game_wasm.js"),
            output: staging.to_path_buf(),
            decoder_wasm: decoder_wasm.clone(),
            wasm_bindgen: options.wasm_bindgen.clone(),
            wasm_opt: options.wasm_opt.clone(),
            compression_quality: options.compression_quality,
            max_main_bytes: Some(30 * 1024 * 1024),
        })?;
        println!(
            "Wasm split: main {:.2} MiB -> {:.2} MiB, compressed modules {:.2} MiB",
            report.main_bytes as f64 / (1024.0 * 1024.0),
            report.compressed_main_bytes as f64 / (1024.0 * 1024.0),
            report.compressed_module_bytes as f64 / (1024.0 * 1024.0),
        );
    }
    arrange_runtime_files(platform, staging)?;
    fs::write(staging.join("adapter.js"), render_adapter(platform)?)?;
    fs::write(staging.join("game.js"), render_game(platform)?)?;
    write_json(&staging.join("game.json"), &platform.game_config(options))?;
    write_json(
        &staging.join("project.config.json"),
        &platform.project_config(options),
    )?;

    if let Some(assets) = &options.assets {
        copy_tree(assets, &staging.join("assets"), &options.exclude_extensions)?;
        arrange_asset_files(platform, staging)?;
    }
    preserve_project_shell(platform, &options.output, staging)?;

    let mut report = build_report(platform, &staging_options)?;
    report.output = options.output.clone();
    if let Some(limit) = platform.max_main_package_bytes()
        && report.main_package_bytes > limit
    {
        bail!(
            "{} main package is {:.2} MiB, above the platform {:.2} MiB limit",
            platform.display_name(),
            report.main_package_bytes as f64 / (1024.0 * 1024.0),
            limit as f64 / (1024.0 * 1024.0),
        );
    }
    if let Some(limit) = options.max_total_bytes
        && report.total_bytes > limit
    {
        bail!(
            "{} package is {:.2} MiB, above the configured {:.2} MiB upload limit",
            platform.display_name(),
            report.total_bytes as f64 / (1024.0 * 1024.0),
            limit as f64 / (1024.0 * 1024.0),
        );
    }
    let report_json = serde_json::to_vec_pretty(&report)?;
    fs::write(staging.join("package-report.json"), report_json)?;
    Ok(report)
}

fn preserve_project_shell(
    platform: &dyn MiniGamePlatform,
    existing: &Path,
    staging: &Path,
) -> Result<()> {
    if !existing.is_dir() {
        return Ok(());
    }
    for file_name in platform.preserved_project_files() {
        let source = existing.join(file_name);
        if source.is_file() {
            if *file_name == "project.config.json" {
                merge_project_config(&source, &staging.join(file_name))?;
                continue;
            }
            fs::copy(&source, staging.join(file_name)).with_context(|| {
                format!(
                    "failed to preserve official project file {}",
                    source.display()
                )
            })?;
        }
    }
    Ok(())
}

fn merge_project_config(existing: &Path, generated: &Path) -> Result<()> {
    let Ok(existing_bytes) = fs::read(existing) else {
        return Ok(());
    };
    let Ok(mut existing_config) = serde_json::from_slice::<serde_json::Value>(&existing_bytes)
    else {
        // A malformed developer-tool file must not replace the valid generated
        // configuration or silently restore a stale AppId.
        return Ok(());
    };
    let generated_bytes = fs::read(generated).with_context(|| {
        format!(
            "failed to read generated project configuration {}",
            generated.display()
        )
    })?;
    let generated_config = serde_json::from_slice::<serde_json::Value>(&generated_bytes)
        .with_context(|| {
            format!(
                "failed to parse generated project configuration {}",
                generated.display()
            )
        })?;

    if let Some(object) = existing_config.as_object_mut() {
        // These fields select the account and platform. Clear every known
        // generated variant first so a project previously opened by another
        // Mini Game tool cannot leak (for example) a WeChat `compileType` into
        // a Douyin package.
        for key in GENERATED_PROJECT_CONFIG_KEYS {
            object.remove(key);
        }
    }
    merge_json(&mut existing_config, generated_config);
    write_json(generated, &existing_config)
}

fn merge_json(destination: &mut serde_json::Value, source: serde_json::Value) {
    match (destination, source) {
        (serde_json::Value::Object(destination), serde_json::Value::Object(source)) => {
            for (key, value) in source {
                match destination.get_mut(&key) {
                    Some(existing) => merge_json(existing, value),
                    None => {
                        destination.insert(key, value);
                    }
                }
            }
        }
        (destination, source) => *destination = source,
    }
}

fn replace_output(staging: &Path, output: &Path) -> Result<()> {
    if !output.exists() {
        fs::rename(staging, output)?;
        return Ok(());
    }

    let backup = sibling_path(output, "previous");
    if backup.exists() {
        fs::remove_dir_all(&backup)?;
    }
    if let Err(rename_error) = fs::rename(output, &backup) {
        if output.is_dir() {
            install_staged_in_place(staging, output).with_context(|| {
                format!(
                    "failed to update {} in place after the Developer Tool prevented an atomic directory swap ({rename_error})",
                    output.display()
                )
            })?;
            fs::remove_dir_all(staging).with_context(|| {
                format!("failed to remove staging directory {}", staging.display())
            })?;
            return Ok(());
        }
        return Err(rename_error).with_context(|| {
            format!(
                "failed to move existing output {} to {}",
                output.display(),
                backup.display()
            )
        });
    }
    if let Err(error) = fs::rename(staging, output) {
        let _ = fs::rename(&backup, output);
        return Err(error).context("failed to install staged Mini Game project");
    }
    fs::remove_dir_all(backup)?;
    Ok(())
}

fn install_staged_in_place(staging: &Path, output: &Path) -> Result<()> {
    remove_stale_generated_files(staging, output)?;
    sync_tree(staging, output)
}

fn remove_stale_generated_files(staging: &Path, output: &Path) -> Result<()> {
    for file_name in GENERATED_ROOT_FILES {
        let staged = staging.join(file_name);
        let destination = output.join(file_name);
        if !staged.exists() && destination.is_file() {
            fs::remove_file(&destination).with_context(|| {
                format!(
                    "failed to remove stale generated file {}",
                    destination.display()
                )
            })?;
        }
    }
    remove_orphaned_files(&staging.join("wasm-split"), &output.join("wasm-split"))?;

    let report_path = output.join("package-report.json");
    let Ok(report_json) = fs::read(&report_path) else {
        return Ok(());
    };
    let Ok(report) = serde_json::from_slice::<PackageReport>(&report_json) else {
        return Ok(());
    };

    for file in report.files {
        if !is_safe_relative_path(&file.path) || staging.join(&file.path).exists() {
            continue;
        }
        let destination = output.join(&file.path);
        if destination.is_file() {
            fs::remove_file(&destination).with_context(|| {
                format!(
                    "failed to remove stale generated file {}",
                    destination.display()
                )
            })?;
        }
    }
    Ok(())
}

fn remove_orphaned_files(source: &Path, destination: &Path) -> Result<()> {
    if !destination.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(destination)?.collect::<std::io::Result<Vec<_>>>()? {
        let destination_path = entry.path();
        let source_path = source.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            remove_orphaned_files(&source_path, &destination_path)?;
            if destination_path.read_dir()?.next().is_none() {
                fs::remove_dir(&destination_path)?;
            }
        } else if !source_path.is_file() {
            fs::remove_file(&destination_path).with_context(|| {
                format!(
                    "failed to remove stale generated chunk {}",
                    destination_path.display()
                )
            })?;
        }
    }
    Ok(())
}

fn is_safe_relative_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn sync_tree(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    let mut entries = fs::read_dir(source)?.collect::<std::io::Result<Vec<_>>>()?;
    // Keep the entry point last so the Developer Tool sees a complete package on hot reload.
    entries.sort_by_key(|entry| entry.file_name() == "game.js");

    for entry in entries {
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            if destination_path.is_file() {
                fs::remove_file(&destination_path)?;
            }
            sync_tree(&source_path, &destination_path)?;
        } else {
            if destination_path.is_dir() {
                fs::remove_dir_all(&destination_path)?;
            } else if destination_path.is_file()
                && files_have_same_contents(&source_path, &destination_path)?
            {
                continue;
            }
            fs::copy(&source_path, &destination_path).with_context(|| {
                format!(
                    "failed to copy staged file {} to {}",
                    source_path.display(),
                    destination_path.display()
                )
            })?;
        }
    }
    Ok(())
}

fn files_have_same_contents(left: &Path, right: &Path) -> Result<bool> {
    if fs::metadata(left)?.len() != fs::metadata(right)?.len() {
        return Ok(false);
    }

    let mut left = BufReader::new(File::open(left)?);
    let mut right = BufReader::new(File::open(right)?);
    let mut left_buffer = [0_u8; 64 * 1024];
    let mut right_buffer = [0_u8; 64 * 1024];
    loop {
        let left_read = left.read(&mut left_buffer)?;
        let right_read = right.read(&mut right_buffer)?;
        if left_read != right_read || left_buffer[..left_read] != right_buffer[..right_read] {
            return Ok(false);
        }
        if left_read == 0 {
            return Ok(true);
        }
    }
}

fn sibling_path(output: &Path, suffix: &str) -> PathBuf {
    let file_name = output
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("bevy-minigame");
    output.with_file_name(format!(".{file_name}.{suffix}-{}", std::process::id()))
}

fn validate_options(options: &PackageOptions) -> Result<()> {
    if !options.wasm.is_file() {
        bail!("Wasm input does not exist: {}", options.wasm.display());
    }
    if let Some(assets) = &options.assets
        && !assets.is_dir()
    {
        bail!("Asset directory does not exist: {}", assets.display());
    }
    if options.wasm_bindgen.as_os_str().is_empty() {
        bail!("wasm-bindgen executable path is empty");
    }
    if options
        .output
        .components()
        .any(|component| component == Component::ParentDir)
    {
        bail!("Output directory must not contain `..`");
    }
    let current_dir = fs::canonicalize(std::env::current_dir()?)?;
    let output = if options.output.exists() {
        fs::canonicalize(&options.output)?
    } else if options.output.is_absolute() {
        options.output.clone()
    } else {
        current_dir.join(&options.output)
    };
    let wasm = fs::canonicalize(&options.wasm)?;
    if output == current_dir || output.parent().is_none() || wasm.starts_with(&output) {
        bail!(
            "Refusing to replace unsafe output directory: {}",
            options.output.display()
        );
    }
    if let Some(assets) = &options.assets
        && fs::canonicalize(assets)?.starts_with(&output)
    {
        bail!("Asset input must not be inside the output directory");
    }
    Ok(())
}

fn run_wasm_bindgen(options: &PackageOptions) -> Result<()> {
    let status = Command::new(&options.wasm_bindgen)
        .args([
            "--target",
            "no-modules",
            "--out-name",
            "game_wasm",
            "--no-typescript",
            "--out-dir",
        ])
        .arg(&options.output)
        .arg(&options.wasm)
        .status()
        .with_context(|| {
            format!(
                "failed to launch wasm-bindgen at {}",
                options.wasm_bindgen.display()
            )
        })?;
    if !status.success() {
        bail!(
            "wasm-bindgen failed with status {status}; its version must match Cargo.lock and the Wasm input must support `--target no-modules` (no inline/imported JavaScript modules)"
        );
    }
    Ok(())
}

fn export_bindgen_module(path: &Path) -> Result<()> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("failed to read generated glue {}", path.display()))?;
    // Some Mini Game runtimes expose `document.currentScript` as undefined.
    // wasm-bindgen's strict null check would then dereference `.src`.
    let source = normalize_bindgen_glue(&source);
    let mut source = format!(
        "const __bevyMiniGameHost = typeof GameGlobal !== 'undefined' ? GameGlobal : globalThis;\nconst BevyMiniGame = __bevyMiniGameHost.BevyMiniGame;\n{source}"
    );
    source.push_str(
        "\nif (typeof module !== 'undefined' && module.exports) module.exports = wasm_bindgen;\n",
    );
    fs::write(path, source)?;
    Ok(())
}

fn runtime_subpackage_root(platform: &dyn MiniGamePlatform) -> Result<Option<&'static str>> {
    let Some(root) = platform.runtime_subpackage() else {
        return Ok(None);
    };
    let mut components = Path::new(root).components();
    if root.is_empty()
        || !matches!(components.next(), Some(Component::Normal(_)))
        || components.next().is_some()
    {
        bail!("runtime subpackage root must be one safe path component: {root}");
    }
    Ok(Some(root))
}

fn asset_subpackage_root(platform: &dyn MiniGamePlatform) -> Result<Option<&'static str>> {
    let Some(root) = platform.asset_subpackage() else {
        return Ok(None);
    };
    let mut components = Path::new(root).components();
    if root.is_empty()
        || !matches!(components.next(), Some(Component::Normal(_)))
        || components.next().is_some()
    {
        bail!("asset subpackage root must be one safe path component: {root}");
    }
    Ok(Some(root))
}

fn arrange_asset_files(platform: &dyn MiniGamePlatform, staging: &Path) -> Result<()> {
    let Some(root) = asset_subpackage_root(platform)? else {
        return Ok(());
    };
    let source = staging.join("assets");
    if !source.is_dir() {
        return Ok(());
    }
    let destination_root = staging.join(root);
    fs::create_dir_all(&destination_root)?;
    fs::rename(&source, destination_root.join("assets")).with_context(|| {
        format!(
            "failed to move asset directory {} into subpackage {root}",
            source.display()
        )
    })?;
    Ok(())
}

fn arrange_runtime_files(platform: &dyn MiniGamePlatform, staging: &Path) -> Result<()> {
    let Some(root) = runtime_subpackage_root(platform)? else {
        return Ok(());
    };
    let destination = staging.join(root);
    fs::create_dir_all(&destination)?;
    for file_name in GENERATED_ROOT_FILES {
        let source = staging.join(file_name);
        if source.is_file() {
            fs::rename(&source, destination.join(file_name)).with_context(|| {
                format!(
                    "failed to move runtime file {} into subpackage {root}",
                    source.display()
                )
            })?;
        }
    }
    let split_modules = staging.join("wasm-split");
    if split_modules.is_dir() {
        fs::rename(&split_modules, destination.join("wasm-split")).with_context(|| {
            format!(
                "failed to move runtime directory {} into subpackage {root}",
                split_modules.display()
            )
        })?;
    }
    fs::write(destination.join("game.js"), SUBPACKAGE_ENTRY_JS)?;
    Ok(())
}

fn render_game(platform: &dyn MiniGamePlatform) -> Result<String> {
    let root = runtime_subpackage_root(platform)?;
    let prefix = root.map_or_else(String::new, |root| format!("{root}/"));
    let source = GAME_JS
        .replace(
            "__BEVY_MINIGAME_RUNTIME_ROOT__",
            &serde_json::to_string(&prefix)?,
        )
        .replace(
            "__BEVY_MINIGAME_SUBPACKAGE__",
            &serde_json::to_string(&root)?,
        )
        .replace(
            "__BEVY_MINIGAME_WASM_MODULE__",
            &serde_json::to_string(&format!("./{prefix}game_wasm.js"))?,
        )
        .replace(
            "__BEVY_MINIGAME_SPLIT_MODULE__",
            &serde_json::to_string(&format!("./{prefix}__wasm_split.js"))?,
        )
        .replace(
            "__BEVY_MINIGAME_DECODER_MODULE__",
            &serde_json::to_string(&format!("./{prefix}brotli_decoder.js"))?,
        );
    if source.contains("__BEVY_MINIGAME_") {
        bail!("game runtime template contains an unresolved placeholder");
    }
    Ok(source)
}

fn render_adapter(platform: &dyn MiniGamePlatform) -> Result<String> {
    let (max_canvas_width, max_canvas_height) =
        platform.max_render_resolution().unwrap_or_default();
    let asset_root = asset_subpackage_root(platform)?
        .map(|root| format!("{root}/assets"))
        .unwrap_or_else(|| "assets".to_string());
    let config = serde_json::to_string(&serde_json::json!({
        "platform": platform.id(),
        "displayName": platform.display_name(),
        "apiGlobal": platform.api_global(),
        "douyinWorkarounds": platform.douyin_workarounds(),
        "webAssemblyPathOnly": platform.webassembly_path_only(),
        "profileClockRetry": platform.profile_clock_retry(),
        "maxCanvasWidth": max_canvas_width,
        "maxCanvasHeight": max_canvas_height,
        "assetRoot": asset_root,
    }))?;
    Ok(format!(
        "(function installBevyMiniGamePlatform(config, global, host) {{\n  global.__bevyMiniGamePlatform = config;\n  if (host !== global) host.__bevyMiniGamePlatform = config;\n}})({config}, typeof globalThis !== 'undefined' ? globalThis : (typeof GameGlobal !== 'undefined' ? GameGlobal : this), typeof GameGlobal !== 'undefined' ? GameGlobal : (typeof globalThis !== 'undefined' ? globalThis : this));\n{ADAPTER_JS}"
    ))
}

fn copy_tree(source: &Path, destination: &Path, excluded_extensions: &[String]) -> Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&source_path, &destination_path, excluded_extensions)?;
        } else if is_excluded(&source_path, excluded_extensions) {
            continue;
        } else {
            fs::copy(&source_path, &destination_path).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    source_path.display(),
                    destination_path.display()
                )
            })?;
        }
    }
    Ok(())
}

fn is_excluded(path: &Path, excluded_extensions: &[String]) -> bool {
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return false;
    };
    excluded_extensions
        .iter()
        .any(|excluded| extension.eq_ignore_ascii_case(excluded.trim_start_matches('.')))
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

fn build_report(
    platform: &dyn MiniGamePlatform,
    options: &PackageOptions,
) -> Result<PackageReport> {
    let mut files = Vec::new();
    collect_files(&options.output, &options.output, &mut files)?;
    files.sort_by(|left, right| left.path.cmp(&right.path));

    let wasm_bytes = files
        .iter()
        .filter(|file| {
            file.path
                .extension()
                .is_some_and(|extension| extension == "wasm")
        })
        .map(|file| file.bytes)
        .sum();
    let compressed_wasm_bytes = files
        .iter()
        .filter(|file| {
            file.path
                .extension()
                .is_some_and(|extension| extension == "br")
        })
        .map(|file| file.bytes)
        .sum();
    let asset_root =
        asset_subpackage_root(platform)?.map(|root| PathBuf::from(root).join("assets"));
    let asset_bytes = files
        .iter()
        .filter(|file| {
            file.path.starts_with("assets")
                || asset_root
                    .as_ref()
                    .is_some_and(|root| file.path.starts_with(root))
        })
        .map(|file| file.bytes)
        .sum();
    let total_bytes: u64 = files.iter().map(|file| file.bytes).sum();
    let subpackage_bytes = if let Some(root) = runtime_subpackage_root(platform)? {
        files
            .iter()
            .filter(|file| file.path.starts_with(root))
            .map(|file| file.bytes)
            .sum()
    } else {
        0
    };
    let main_package_bytes = total_bytes.saturating_sub(subpackage_bytes);
    let runtime_bytes =
        total_bytes.saturating_sub(wasm_bytes + compressed_wasm_bytes + asset_bytes);
    let mut warnings = Vec::new();
    if wasm_bytes + compressed_wasm_bytes > 20 * 1024 * 1024 {
        warnings.push(format!(
            "Wasm payload is {:.1} MiB before platform packaging; verify current package limits in {} Developer Tool",
            (wasm_bytes + compressed_wasm_bytes) as f64 / (1024.0 * 1024.0),
            platform.display_name()
        ));
    }
    if asset_bytes > 20 * 1024 * 1024 {
        warnings.push(format!(
            "Assets are {:.1} MiB; consider remote assets or platform subpackages",
            asset_bytes as f64 / (1024.0 * 1024.0)
        ));
    }

    Ok(PackageReport {
        output: options.output.clone(),
        wasm_bytes,
        compressed_wasm_bytes,
        asset_bytes,
        runtime_bytes,
        total_bytes,
        main_package_bytes,
        subpackage_bytes,
        files,
        warnings,
    })
}

fn collect_files(root: &Path, directory: &Path, files: &mut Vec<PackageFile>) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect_files(root, &path, files)?;
        } else if entry.file_name() == "package-report.json" {
            continue;
        } else {
            files.push(PackageFile {
                path: path.strip_prefix(root)?.to_path_buf(),
                bytes: entry.metadata()?.len(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestPlatform;

    impl MiniGamePlatform for TestPlatform {
        fn id(&self) -> &'static str {
            "test"
        }

        fn display_name(&self) -> &'static str {
            "Test"
        }

        fn api_global(&self) -> &'static str {
            "testApi"
        }

        fn project_config(&self, options: &PackageOptions) -> serde_json::Value {
            serde_json::json!({
                "compileType": "game",
                "appid": options.app_id,
                "projectname": options.project_name,
            })
        }
    }

    struct SubpackagePlatform;

    impl MiniGamePlatform for SubpackagePlatform {
        fn id(&self) -> &'static str {
            "subpackage"
        }

        fn display_name(&self) -> &'static str {
            "Subpackage"
        }

        fn api_global(&self) -> &'static str {
            "testApi"
        }

        fn runtime_subpackage(&self) -> Option<&'static str> {
            Some("bevy")
        }

        fn asset_subpackage(&self) -> Option<&'static str> {
            Some("bevy")
        }

        fn project_config(&self, _options: &PackageOptions) -> serde_json::Value {
            serde_json::json!({})
        }
    }

    #[test]
    fn copy_tree_preserves_nested_assets() {
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        fs::create_dir(source.path().join("nested")).unwrap();
        fs::write(source.path().join("nested/test.bin"), [1, 2, 3]).unwrap();

        copy_tree(source.path(), &destination.path().join("assets"), &[]).unwrap();

        assert_eq!(
            fs::read(destination.path().join("assets/nested/test.bin")).unwrap(),
            [1, 2, 3]
        );
    }

    #[test]
    fn assets_can_be_moved_into_the_runtime_subpackage() {
        let staging = tempfile::tempdir().unwrap();
        fs::create_dir_all(staging.path().join("assets/shaders")).unwrap();
        fs::write(staging.path().join("assets/shaders/test.wgsl"), [1, 2, 3]).unwrap();

        arrange_asset_files(&SubpackagePlatform, staging.path()).unwrap();

        assert!(!staging.path().join("assets").exists());
        assert_eq!(
            fs::read(staging.path().join("bevy/assets/shaders/test.wgsl")).unwrap(),
            [1, 2, 3]
        );
    }

    #[test]
    fn report_separates_wasm_assets_and_runtime() {
        let output = tempfile::tempdir().unwrap();
        fs::create_dir(output.path().join("assets")).unwrap();
        fs::write(output.path().join("game_wasm_bg.wasm"), [0; 5]).unwrap();
        fs::write(output.path().join("assets/a.png"), [0; 7]).unwrap();
        fs::write(output.path().join("game.js"), [0; 3]).unwrap();
        let options = PackageOptions {
            wasm: output.path().join("input.wasm"),
            assets: None,
            output: output.path().to_path_buf(),
            app_id: "test".into(),
            project_name: "test".into(),
            orientation: Orientation::Landscape,
            wasm_bindgen: "wasm-bindgen".into(),
            exclude_extensions: Vec::new(),
            wasm_split_decoder: None,
            wasm_opt: None,
            compression_quality: 11,
            max_total_bytes: None,
        };

        let report = build_report(&TestPlatform, &options).unwrap();
        assert_eq!(report.wasm_bytes, 5);
        assert_eq!(report.asset_bytes, 7);
        assert_eq!(report.runtime_bytes, 3);
        assert_eq!(report.main_package_bytes, 15);
        assert_eq!(report.subpackage_bytes, 0);
    }

    #[test]
    fn copy_tree_can_exclude_extensions() {
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        fs::write(source.path().join("keep.png"), [1]).unwrap();
        fs::write(source.path().join("skip.OGG"), [2]).unwrap();

        copy_tree(
            source.path(),
            &destination.path().join("assets"),
            &["ogg".into()],
        )
        .unwrap();

        assert!(destination.path().join("assets/keep.png").is_file());
        assert!(!destination.path().join("assets/skip.OGG").exists());
    }

    #[test]
    fn replace_output_swaps_staged_directory() {
        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("dist");
        let staging = root.path().join("staging");
        fs::create_dir(&output).unwrap();
        fs::create_dir(&staging).unwrap();
        fs::write(output.join("old"), []).unwrap();
        fs::write(staging.join("new"), []).unwrap();

        replace_output(&staging, &output).unwrap();

        assert!(!output.join("old").exists());
        assert!(output.join("new").is_file());
        assert!(!staging.exists());
    }

    #[test]
    fn in_place_install_updates_owned_files_and_keeps_unknown_project_files() {
        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("dist");
        let staging = root.path().join("staging");
        fs::create_dir(&output).unwrap();
        fs::create_dir(&staging).unwrap();
        fs::write(output.join("old.js"), "old").unwrap();
        fs::write(output.join("tool-owned.json"), "keep").unwrap();
        fs::write(staging.join("new.js"), "new").unwrap();
        write_json(
            &output.join("package-report.json"),
            &PackageReport {
                output: output.clone(),
                wasm_bytes: 0,
                compressed_wasm_bytes: 0,
                asset_bytes: 0,
                runtime_bytes: 3,
                total_bytes: 3,
                main_package_bytes: 3,
                subpackage_bytes: 0,
                files: vec![PackageFile {
                    path: "old.js".into(),
                    bytes: 3,
                }],
                warnings: Vec::new(),
            },
        )
        .unwrap();

        install_staged_in_place(&staging, &output).unwrap();

        assert!(!output.join("old.js").exists());
        assert_eq!(fs::read_to_string(output.join("new.js")).unwrap(), "new");
        assert!(output.join("tool-owned.json").is_file());
    }

    #[test]
    fn in_place_install_removes_legacy_wasm_and_orphaned_chunks() {
        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("dist");
        let staging = root.path().join("staging");
        fs::create_dir_all(output.join("wasm-split")).unwrap();
        fs::create_dir_all(staging.join("wasm-split")).unwrap();
        fs::write(output.join("game_wasm_bg.wasm"), [1]).unwrap();
        fs::write(output.join("wasm-split/old.wasm.br"), [2]).unwrap();
        fs::write(staging.join("game_wasm_bg.wasm.br"), [3]).unwrap();
        fs::write(staging.join("wasm-split/current.wasm.br"), [4]).unwrap();

        install_staged_in_place(&staging, &output).unwrap();

        assert!(!output.join("game_wasm_bg.wasm").exists());
        assert!(!output.join("wasm-split/old.wasm.br").exists());
        assert_eq!(fs::read(output.join("game_wasm_bg.wasm.br")).unwrap(), [3]);
        assert_eq!(
            fs::read(output.join("wasm-split/current.wasm.br")).unwrap(),
            [4]
        );
    }

    #[test]
    fn preserve_project_shell_keeps_tool_settings_but_generated_identity_wins() {
        let existing = tempfile::tempdir().unwrap();
        let staging = tempfile::tempdir().unwrap();
        write_json(
            &existing.path().join("project.config.json"),
            &serde_json::json!({
                "appid": "wx-stale",
                "projectname": "old-name",
                "compileType": "game",
                "setting": {
                    "urlCheck": true,
                    "toolOnlySetting": true
                },
                "packOptions": { "ignore": ["local-only"] }
            }),
        )
        .unwrap();
        fs::write(existing.path().join("icon.png"), [1, 2, 3]).unwrap();
        write_json(
            &staging.path().join("project.config.json"),
            &serde_json::json!({
                "appid": "tt-current",
                "projectname": "new-name",
                "douyinProjectType": "native",
                "setting": {
                    "urlCheck": false,
                    "nativeCompile": true
                }
            }),
        )
        .unwrap();

        preserve_project_shell(&TestPlatform, existing.path(), staging.path()).unwrap();

        let config: serde_json::Value =
            serde_json::from_slice(&fs::read(staging.path().join("project.config.json")).unwrap())
                .unwrap();
        assert_eq!(config["appid"], "tt-current");
        assert_eq!(config["projectname"], "new-name");
        assert_eq!(config["douyinProjectType"], "native");
        assert!(config.get("compileType").is_none());
        assert_eq!(config["setting"]["urlCheck"], false);
        assert_eq!(config["setting"]["nativeCompile"], true);
        assert_eq!(config["setting"]["toolOnlySetting"], true);
        assert_eq!(config["packOptions"]["ignore"][0], "local-only");
        assert_eq!(
            fs::read(staging.path().join("icon.png")).unwrap(),
            [1, 2, 3]
        );
    }

    #[test]
    fn malformed_preserved_project_config_cannot_replace_generated_config() {
        let existing = tempfile::tempdir().unwrap();
        let staging = tempfile::tempdir().unwrap();
        fs::write(existing.path().join("project.config.json"), "not-json").unwrap();
        write_json(
            &staging.path().join("project.config.json"),
            &serde_json::json!({ "appid": "tt-current" }),
        )
        .unwrap();

        preserve_project_shell(&TestPlatform, existing.path(), staging.path()).unwrap();

        let config: serde_json::Value =
            serde_json::from_slice(&fs::read(staging.path().join("project.config.json")).unwrap())
                .unwrap();
        assert_eq!(config["appid"], "tt-current");
    }

    #[test]
    fn platform_supplies_project_and_game_configuration() {
        let options = PackageOptions {
            wasm: "game.wasm".into(),
            assets: None,
            output: "dist".into(),
            app_id: "testAppId".into(),
            project_name: "emoji".into(),
            orientation: Orientation::Landscape,
            wasm_bindgen: "wasm-bindgen".into(),
            exclude_extensions: Vec::new(),
            wasm_split_decoder: None,
            wasm_opt: None,
            compression_quality: 11,
            max_total_bytes: None,
        };

        let config = TestPlatform.project_config(&options);
        assert_eq!(config["compileType"], "game");
        assert_eq!(
            TestPlatform.game_config(&options)["deviceOrientation"],
            "landscape"
        );
    }

    #[test]
    fn generated_glue_gets_a_game_global_bridge_alias() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("game_wasm.js");
        fs::write(
            &path,
            "if (document.currentScript !== null) { document.currentScript.src; }\nlet wasm_bindgen = function () {};",
        )
        .unwrap();

        export_bindgen_module(&path).unwrap();

        let source = fs::read_to_string(path).unwrap();
        assert!(source.starts_with("const __bevyMiniGameHost"));
        assert!(source.contains("const BevyMiniGame = __bevyMiniGameHost.BevyMiniGame;"));
        assert!(source.contains("document.currentScript != null"));
        assert!(!source.contains("document.currentScript !== null"));
        assert!(source.contains("module.exports = wasm_bindgen"));
    }

    #[test]
    fn rendered_adapter_contains_platform_configuration() {
        let source = render_adapter(&TestPlatform).unwrap();

        assert!(source.contains("\"apiGlobal\":\"testApi\""));
        assert!(source.contains("\"platform\":\"test\""));
        assert!(source.contains("\"webAssemblyPathOnly\":false"));
        assert!(source.contains("\"profileClockRetry\":false"));
        assert!(source.contains("\"maxCanvasWidth\":0"));
        assert!(source.contains("\"maxCanvasHeight\":0"));
        assert!(source.contains("\"assetRoot\":\"assets\""));
        assert!(source.contains("installBevyMiniGameAdapter"));
    }

    #[test]
    fn runtime_files_move_into_configured_subpackage() {
        let staging = tempfile::tempdir().unwrap();
        fs::write(staging.path().join("game_wasm.js"), [1]).unwrap();
        fs::write(staging.path().join("game_wasm_bg.wasm.br"), [2]).unwrap();
        fs::write(staging.path().join("brotli_decoder.js"), [3]).unwrap();
        fs::create_dir(staging.path().join("wasm-split")).unwrap();
        fs::write(staging.path().join("wasm-split/chunk.wasm.br"), [4]).unwrap();

        arrange_runtime_files(&SubpackagePlatform, staging.path()).unwrap();

        assert!(staging.path().join("bevy/game_wasm.js").is_file());
        assert!(staging.path().join("bevy/game_wasm_bg.wasm.br").is_file());
        assert!(staging.path().join("bevy/brotli_decoder.js").is_file());
        assert_eq!(
            fs::read_to_string(staging.path().join("bevy/game.js")).unwrap(),
            SUBPACKAGE_ENTRY_JS
        );
        assert!(
            staging
                .path()
                .join("bevy/wasm-split/chunk.wasm.br")
                .is_file()
        );
        assert!(!staging.path().join("game_wasm.js").exists());
    }

    #[test]
    fn rendered_game_loads_subpackage_before_runtime() {
        let source = render_game(&SubpackagePlatform).unwrap();

        assert!(source.contains("loadSubpackage(\"bevy\")"));
        assert!(source.contains("require(\"./bevy/game_wasm.js\")"));
        assert!(source.contains("const runtimeRoot = \"bevy/\""));
        assert!(!source.contains("__BEVY_MINIGAME_"));
    }
}
