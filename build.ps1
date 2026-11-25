# Builds the standalone Windows x64 executable into dist/latest.
param(
    [switch]$SelfContained,
    [switch]$FrameworkDependent,
    [string]$OutputDirectory = "dist\latest",
    [switch]$Offline
)

$ErrorActionPreference = "Stop"
if ($SelfContained -and $FrameworkDependent) {
    throw "Choose at most one packaging flag."
}
if ($FrameworkDependent) {
    Write-Verbose "The Rust build is standalone on all packaging modes."
}

$out = if ([IO.Path]::IsPathRooted($OutputDirectory)) {
    [IO.Path]::GetFullPath($OutputDirectory)
} else {
    [IO.Path]::GetFullPath((Join-Path $PSScriptRoot $OutputDirectory))
}
$target = Join-Path $PSScriptRoot "target"
$toolchain = Join-Path $PSScriptRoot "artifacts\rust-toolchain"
$cargo = Join-Path $toolchain "cargo\bin\cargo.exe"
$priorCargo = $env:CARGO_HOME
$priorRustup = $env:RUSTUP_HOME
$priorPath = $env:Path

try {
    if (Test-Path -LiteralPath $cargo) {
        $env:CARGO_HOME = Join-Path $toolchain "cargo"
        $env:RUSTUP_HOME = Join-Path $toolchain "rustup"
        $env:Path = (Split-Path -Parent $cargo) + ";" + $env:Path
    } else {
        $cargo = "cargo"
    }
    $buildArgs = @("build", "--locked", "--release", "--target", "x86_64-pc-windows-msvc",
        "--manifest-path", (Join-Path $PSScriptRoot "Cargo.toml"), "--target-dir", $target)
    if ($Offline) {
        $buildArgs += "--offline"
    }
    Push-Location -LiteralPath $PSScriptRoot
    try {
        & $cargo @buildArgs
        if ($LASTEXITCODE -ne 0) { throw "Cargo build failed with exit code $LASTEXITCODE." }
    } finally {
        Pop-Location
    }
} finally {
    $env:CARGO_HOME = $priorCargo
    $env:RUSTUP_HOME = $priorRustup
    $env:Path = $priorPath
}

New-Item -ItemType Directory -Force -Path (Join-Path $out "songs") | Out-Null
$exe = Join-Path $out "AztroMidi.exe"
$staged = Join-Path $out (".aztromidi-" + [Guid]::NewGuid().ToString("N") + ".tmp")
try {
    Copy-Item -LiteralPath (Join-Path $target "x86_64-pc-windows-msvc\release\aztromidi.exe") -Destination $staged
    if ([IO.File]::Exists($exe)) {
        [IO.File]::Replace($staged, $exe, [NullString]::Value)
    } else {
        [IO.File]::Move($staged, $exe)
    }
} finally {
    if ([IO.File]::Exists($staged)) { Remove-Item -LiteralPath $staged }
}
Write-Host "Done -> $exe" -ForegroundColor Cyan
