param(
    [ValidatePattern('^tt[0-9a-zA-Z]+$')]
    [string]$AppId = "tt975be1e608633d4202",
    [string]$Output = "dist-douyin/emoji",
    [int]$BinaryenVersion = 130,
    [ValidateRange(1, 11)]
    [int]$CompressionQuality = 11
)

$ErrorActionPreference = "Stop"
$TargetDirectory = "target/douyin"
$DecoderTargetDirectory = "target/douyin-decoder"
$WasmOpt = & "$PSScriptRoot/ensure-binaryen.ps1" -Version $BinaryenVersion

cargo build `
    --release `
    --target wasm32-unknown-unknown `
    -p bevy_minigame_wasm_split `
    --no-default-features `
    --features decoder `
    --target-dir $DecoderTargetDirectory
if ($LASTEXITCODE -ne 0) {
    throw "Failed to build the Brotli decoder (cargo exit code $LASTEXITCODE)"
}

cargo rustc `
    --release `
    --target wasm32-unknown-unknown `
    --bin bevy_emoji `
    --no-default-features `
    --features douyin `
    --target-dir $TargetDirectory `
    -- `
    -C link-arg=--emit-relocs `
    -C strip=none
if ($LASTEXITCODE -ne 0) {
    throw "Failed to build Bevy Wasm (cargo exit code $LASTEXITCODE)"
}

cargo run `
    -p bevy_minigame_douyin `
    --no-default-features `
    --features cli `
    --bin bevy-minigame-douyin `
    -- package `
    --wasm $TargetDirectory/wasm32-unknown-unknown/release/bevy_emoji.wasm `
    --assets emoji-merge/assets `
    --output $Output `
    --project-name bevy-emoji `
    --app-id $AppId `
    --orientation landscape `
    --wasm-split-decoder $DecoderTargetDirectory/wasm32-unknown-unknown/release/bevy_minigame_wasm_split.wasm `
    --wasm-opt $WasmOpt `
    --compression-quality $CompressionQuality `
    --max-total-mib 30
if ($LASTEXITCODE -ne 0) {
    throw "Failed to package Douyin Mini Game (cargo exit code $LASTEXITCODE)"
}
