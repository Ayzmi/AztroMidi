# Builds AztroMidi into ./dist
#   .\build.ps1                -> standalone exe, no runtime installation needed
#   .\build.ps1 -FrameworkDependent -> small exe, needs the .NET 10 Desktop Runtime
param([switch]$SelfContained, [switch]$FrameworkDependent, [string]$OutputDirectory = "dist")

$ErrorActionPreference = "Stop"
$env:DOTNET_CLI_TELEMETRY_OPTOUT = "1"
$env:DOTNET_NOLOGO = "1"

$project = Join-Path $PSScriptRoot "src\AztroMidi\AztroMidi.csproj"
$out = Join-Path $PSScriptRoot $OutputDirectory

$publishArgs = @(
    "publish", $project,
    "-c", "Release",
    "-r", "win-x64",
    "-o", $out,
    "-p:PublishSingleFile=true",
    "-p:IncludeNativeLibrariesForSelfExtract=true"
)
if ($SelfContained -and $FrameworkDependent) {
    throw "Choose either -SelfContained or -FrameworkDependent."
}
if (!$FrameworkDependent) {
    $publishArgs += @("--self-contained", "true", "-p:EnableCompressionInSingleFile=true")
} else {
    $publishArgs += @("--self-contained", "false")
}

dotnet @publishArgs
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

New-Item -ItemType Directory -Force (Join-Path $out "songs") | Out-Null
Write-Host ""
Write-Host "Done -> $out\AztroMidi.exe" -ForegroundColor Cyan
