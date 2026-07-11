use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tempfile::TempDir;
use wasm_split_cli::{OutputModules, SplitModule, Splitter};

const LOADER_TEMPLATE: &str = include_str!("../runtime/loader.js.template");

#[derive(Clone, Debug)]
pub struct SplitOptions {
    pub original_wasm: PathBuf,
    pub bindgen_wasm: PathBuf,
    pub bindgen_js: PathBuf,
    pub output: PathBuf,
    pub decoder_wasm: PathBuf,
    pub wasm_bindgen: PathBuf,
    pub wasm_opt: Option<PathBuf>,
    pub compression_quality: u32,
    pub max_main_bytes: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SplitReport {
    pub original_bytes: u64,
    pub bindgen_bytes: u64,
    pub main_bytes: u64,
    pub compressed_main_bytes: u64,
    pub decoder_bytes: u64,
    pub compressed_module_bytes: u64,
    pub modules: Vec<ModuleReport>,
    pub loader_imports: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleReport {
    pub kind: ModuleKind,
    pub name: String,
    pub path: PathBuf,
    pub uncompressed_bytes: u64,
    pub compressed_bytes: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ModuleKind {
    Chunk,
    SplitPoint,
}

pub fn split(options: &SplitOptions) -> Result<SplitReport> {
    validate_options(options)?;

    let original = fs::read(&options.original_wasm).with_context(|| {
        format!(
            "failed to read original Wasm {}",
            options.original_wasm.display()
        )
    })?;
    let bindgened = fs::read(&options.bindgen_wasm).with_context(|| {
        format!(
            "failed to read wasm-bindgen output {}",
            options.bindgen_wasm.display()
        )
    })?;

    let emitted = Splitter::new(&original, &bindgened)
        .context("failed to analyze Wasm relocations; compile with `-C link-arg=--emit-relocs`")?
        .emit()
        .context("failed to emit split Wasm modules")?;
    if emitted.modules.is_empty() {
        bail!(
            "no Wasm split points were found; wrap a Bevy startup function with `#[wasm_split::wasm_split(module_name)]`"
        );
    }

    fs::create_dir_all(&options.output)?;
    let split_directory = options.output.join("wasm-split");
    if split_directory.exists() {
        fs::remove_dir_all(&split_directory)?;
    }
    fs::create_dir_all(&split_directory)?;

    let work = tempfile::tempdir().context("failed to create Wasm split work directory")?;
    let OutputModules {
        main,
        modules,
        chunks,
    } = emitted;

    let main_bytes = optimize_module(&main.bytes, "main", options, &work)?;
    let compressed_main = compress(&main_bytes, options.compression_quality)?;
    if let Some(limit) = options.max_main_bytes
        && compressed_main.len() as u64 > limit
    {
        bail!(
            "compressed split main Wasm is {:.2} MiB, above the configured {:.2} MiB limit",
            mib(compressed_main.len() as u64),
            mib(limit)
        );
    }
    fs::write(
        options.output.join("game_wasm_bg.wasm.br"),
        &compressed_main,
    )?;
    fs::remove_file(&options.bindgen_wasm)?;

    let chunk_names = chunks
        .iter()
        .enumerate()
        .map(|(index, module)| {
            format!("chunk_{index}_{}", safe_file_component(&module.module_name))
        })
        .collect::<Vec<_>>();

    let mut reports = Vec::new();
    for (index, module) in chunks.iter().enumerate() {
        reports.push(write_compressed_module(
            module,
            ModuleKind::Chunk,
            &chunk_names[index],
            options,
            &work,
            &split_directory,
        )?);
    }

    let mut loader_imports = Vec::new();
    let mut module_files = Vec::new();
    for (index, module) in modules.iter().enumerate() {
        let component =
            safe_file_component(module.component_name.as_deref().unwrap_or("split_point"));
        let file_name = format!("module_{index}_{component}");
        reports.push(write_compressed_module(
            module,
            ModuleKind::SplitPoint,
            &file_name,
            options,
            &work,
            &split_directory,
        )?);

        let loader_name = split_loader_name(module)?;
        loader_imports.push(loader_name);
        module_files.push(file_name);
    }

    write_loader(
        &options.output.join("__wasm_split.js"),
        &chunks,
        &chunk_names,
        &modules,
        &module_files,
        &loader_imports,
    )?;
    validate_bindgen_glue(&options.bindgen_js)?;
    let decoder_bytes = build_decoder(options, &work)?;

    let report = SplitReport {
        original_bytes: original.len() as u64,
        bindgen_bytes: bindgened.len() as u64,
        main_bytes: main_bytes.len() as u64,
        compressed_main_bytes: compressed_main.len() as u64,
        decoder_bytes,
        compressed_module_bytes: reports.iter().map(|module| module.compressed_bytes).sum(),
        modules: reports,
        loader_imports,
    };
    fs::write(
        options.output.join("wasm-split-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}

fn validate_options(options: &SplitOptions) -> Result<()> {
    for (label, path) in [
        ("original Wasm", &options.original_wasm),
        ("wasm-bindgen Wasm", &options.bindgen_wasm),
        ("wasm-bindgen JavaScript", &options.bindgen_js),
        ("decoder Wasm", &options.decoder_wasm),
    ] {
        if !path.is_file() {
            bail!("{label} does not exist: {}", path.display());
        }
    }
    if options.wasm_bindgen.as_os_str().is_empty() {
        bail!("wasm-bindgen executable path is empty");
    }
    if !(1..=11).contains(&options.compression_quality) {
        bail!("Brotli compression quality must be between 1 and 11");
    }
    Ok(())
}

fn write_compressed_module(
    module: &SplitModule,
    kind: ModuleKind,
    name: &str,
    options: &SplitOptions,
    work: &TempDir,
    split_directory: &Path,
) -> Result<ModuleReport> {
    let optimized = optimize_module(&module.bytes, name, options, work)?;
    let compressed = compress(&optimized, options.compression_quality)?;
    let relative_path = PathBuf::from("wasm-split").join(format!("{name}.wasm.br"));
    fs::write(split_directory.join(format!("{name}.wasm.br")), &compressed)?;

    Ok(ModuleReport {
        kind,
        name: name.to_owned(),
        path: relative_path,
        uncompressed_bytes: optimized.len() as u64,
        compressed_bytes: compressed.len() as u64,
    })
}

fn optimize_module(
    bytes: &[u8],
    name: &str,
    options: &SplitOptions,
    work: &TempDir,
) -> Result<Vec<u8>> {
    let Some(wasm_opt) = &options.wasm_opt else {
        return Ok(bytes.to_vec());
    };

    let input = work.path().join(format!("{name}.input.wasm"));
    let output = work.path().join(format!("{name}.opt.wasm"));
    fs::write(&input, bytes)?;
    let status = Command::new(wasm_opt)
        .args([
            "-Oz",
            "--enable-bulk-memory",
            "--enable-nontrapping-float-to-int",
            "--enable-sign-ext",
            "--enable-reference-types",
            "--enable-multivalue",
        ])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .status()
        .with_context(|| format!("failed to launch wasm-opt at {}", wasm_opt.display()))?;
    if !status.success() {
        bail!("wasm-opt failed for {name} with status {status}");
    }
    fs::read(&output).with_context(|| {
        format!(
            "wasm-opt did not produce expected output {}",
            output.display()
        )
    })
}

fn compress(bytes: &[u8], quality: u32) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    {
        let mut writer = brotli::CompressorWriter::new(&mut output, 64 * 1024, quality, 22);
        writer.write_all(bytes)?;
    }
    Ok(output)
}

fn split_loader_name(module: &SplitModule) -> Result<String> {
    let hash = module
        .hash_id
        .as_deref()
        .context("split module is missing its hash id")?;
    let component = module
        .component_name
        .as_deref()
        .context("split module is missing its component name")?;
    Ok(format!(
        "__wasm_split_load_{}_{}_{}",
        module.module_name, hash, component
    ))
}

fn write_loader(
    path: &Path,
    chunks: &[SplitModule],
    chunk_names: &[String],
    modules: &[SplitModule],
    module_files: &[String],
    loader_imports: &[String],
) -> Result<()> {
    let mut generated = String::new();
    for (index, name) in chunk_names.iter().enumerate() {
        generated.push_str(&format!(
            "const __wasmSplitChunk{index} = makeLoad(\"wasm-split/{name}.wasm.br\", []);\n"
        ));
    }
    for (index, module) in modules.iter().enumerate() {
        let dependencies = module
            .relies_on_chunks
            .iter()
            .map(|chunk| format!("__wasmSplitChunk{chunk}"))
            .collect::<Vec<_>>()
            .join(", ");
        generated.push_str(&format!(
            "const {} = makeLoad(\"wasm-split/{}.wasm.br\", [{}]);\n",
            loader_imports[index], module_files[index], dependencies
        ));
    }
    generated.push_str("\nmodule.exports = {\n  configure,\n");
    for loader in loader_imports {
        generated.push_str(&format!("  {loader},\n"));
    }
    generated.push_str("};\n");

    let source = LOADER_TEMPLATE.replace("// __LOADERS__", &generated);
    fs::write(path, source)?;

    if chunks.len() != chunk_names.len() {
        bail!("internal error: chunk metadata length mismatch");
    }
    Ok(())
}

fn validate_bindgen_glue(path: &Path) -> Result<()> {
    let source = fs::read_to_string(path)?;
    if !source.contains("require(\"./__wasm_split.js\")") {
        bail!(
            "wasm-bindgen glue {} does not import the split runtime; use a wasm-bindgen version compatible with raw Wasm module imports",
            path.display()
        );
    }
    Ok(())
}

fn build_decoder(options: &SplitOptions, work: &TempDir) -> Result<u64> {
    let decoder_output = work.path().join("decoder-bindgen");
    fs::create_dir_all(&decoder_output)?;
    let status = Command::new(&options.wasm_bindgen)
        .args([
            "--target",
            "no-modules",
            "--out-name",
            "brotli_decoder",
            "--no-typescript",
            "--out-dir",
        ])
        .arg(&decoder_output)
        .arg(&options.decoder_wasm)
        .status()
        .with_context(|| {
            format!(
                "failed to launch wasm-bindgen at {}",
                options.wasm_bindgen.display()
            )
        })?;
    if !status.success() {
        bail!("wasm-bindgen failed for the Brotli decoder with status {status}");
    }

    let decoder_wasm = decoder_output.join("brotli_decoder_bg.wasm");
    let decoder_bytes = fs::read(&decoder_wasm)?;
    let optimized = optimize_module(&decoder_bytes, "brotli_decoder", options, work)?;
    fs::write(options.output.join("brotli_decoder_bg.wasm"), &optimized)?;

    let decoder_js = decoder_output.join("brotli_decoder.js");
    let mut source = fs::read_to_string(decoder_js)?;
    source.push_str(
        "\nif (typeof module !== 'undefined' && module.exports) module.exports = wasm_bindgen;\n",
    );
    fs::write(options.output.join("brotli_decoder.js"), source)?;
    Ok(optimized.len() as u64)
}

fn safe_file_component(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn mib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    #[test]
    fn brotli_round_trip() {
        let input = b"bevy wasm split payload".repeat(4096);
        let compressed = compress(&input, 11).unwrap();
        let mut decoded = Vec::new();
        brotli_decompressor::Decompressor::new(compressed.as_slice(), 64 * 1024)
            .read_to_end(&mut decoded)
            .unwrap();
        assert_eq!(decoded, input);
        assert!(compressed.len() < input.len());
    }

    #[test]
    fn sanitizes_module_file_names() {
        assert_eq!(safe_file_component("game::run/now"), "game__run_now");
    }
}
