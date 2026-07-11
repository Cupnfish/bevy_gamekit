param(
    [int]$Version = 130,
    [string]$ToolDirectory = "target/tools"
)

$ErrorActionPreference = "Stop"
$PackageName = "binaryen-version_$Version-x86_64-windows"
$InstallDirectory = Join-Path $ToolDirectory "binaryen-version_$Version"
$WasmOpt = Join-Path $InstallDirectory "bin/wasm-opt.exe"

if (Test-Path -LiteralPath $WasmOpt -PathType Leaf) {
    Write-Output $WasmOpt
    exit 0
}

New-Item -ItemType Directory -Force $ToolDirectory | Out-Null
$BaseUrl = "https://github.com/WebAssembly/binaryen/releases/download/version_$Version"
$ArchiveName = "$PackageName.tar.gz"
$Archive = Join-Path $ToolDirectory $ArchiveName
$Checksum = "$Archive.sha256"

Write-Host "Downloading Binaryen version_$Version to $ToolDirectory"
Invoke-WebRequest "$BaseUrl/$ArchiveName" -OutFile $Archive
Invoke-WebRequest "$BaseUrl/$ArchiveName.sha256" -OutFile $Checksum

$Expected = (Get-Content -LiteralPath $Checksum -Raw).Trim().Split()[0].ToLowerInvariant()
$Actual = (Get-FileHash -LiteralPath $Archive -Algorithm SHA256).Hash.ToLowerInvariant()
if ($Actual -ne $Expected) {
    throw "Binaryen checksum mismatch: expected $Expected, got $Actual"
}

tar -xzf $Archive -C $ToolDirectory
if ($LASTEXITCODE -ne 0 -or !(Test-Path -LiteralPath $WasmOpt -PathType Leaf)) {
    throw "Binaryen extraction did not produce $WasmOpt"
}

Write-Output $WasmOpt
