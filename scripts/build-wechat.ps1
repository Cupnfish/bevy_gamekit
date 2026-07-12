param(
    [string]$AppId = "touristappid",
    [string]$Output = "dist-wechat/emoji",
    [int]$BinaryenVersion = 130,
    [ValidateRange(1, 11)]
    [int]$CompressionQuality = 11
)

$ErrorActionPreference = "Stop"
$TargetDirectory = "target/wechat"
$DecoderTargetDirectory = "target/wechat-decoder"
$WasmOpt = & "$PSScriptRoot/ensure-binaryen.ps1" -Version $BinaryenVersion
$PreviousRustFlags = $env:RUSTFLAGS
$env:RUSTFLAGS = (($PreviousRustFlags, "-C target-feature=-reference-types") |
    Where-Object { -not [string]::IsNullOrWhiteSpace($_) }) -join " "

try {

cargo build `
    -Z build-std=std,panic_abort `
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
    -Z build-std=std,panic_abort `
    --release `
    --target wasm32-unknown-unknown `
    --bin bevy_emoji `
    --no-default-features `
    --features wechat `
    --target-dir $TargetDirectory `
    -- `
    -C link-arg=--emit-relocs `
    -C strip=none
if ($LASTEXITCODE -ne 0) {
    throw "Failed to build Bevy Wasm (cargo exit code $LASTEXITCODE)"
}
} finally {
    $env:RUSTFLAGS = $PreviousRustFlags
}

cargo run `
    -p bevy_minigame_wechat `
    --no-default-features `
    --features cli `
    --bin bevy-minigame-wechat `
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
    --max-total-mib 20
if ($LASTEXITCODE -ne 0) {
    throw "Failed to package WeChat Mini Game (cargo exit code $LASTEXITCODE)"
}
