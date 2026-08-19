# Publish the Windows app and pack it into a zip for a GitHub release.
#
#   scripts/release-windows.ps1
#
# Version comes from the VERSION environment variable, falling back to
# the workspace version in Cargo.toml. Builds for the host architecture:
# the core build inside the project file always targets the host. The
# app is unsigned, so SmartScreen warns once.
#
# Outputs:
#   target/windows/Paloma-<version>-windows-<arch>.zip
$ErrorActionPreference = "Stop"

Set-Location (Join-Path $PSScriptRoot "..")

$version = $env:VERSION
if (-not $version) {
    $match = Select-String -Path Cargo.toml -Pattern '^version = "(.+)"$' | Select-Object -First 1
    $version = $match.Matches[0].Groups[1].Value
}

$arch = if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") { "arm64" } else { "x64" }
$platform = if ($arch -eq "arm64") { "ARM64" } else { "x64" }
$archLabel = if ($arch -eq "arm64") { "arm64" } else { "amd64" }

$out = "target/windows"
$staging = "$out/Paloma"
$zip = "$out/Paloma-$version-windows-$archLabel.zip"

if (Test-Path $out) {
    Remove-Item $out -Recurse -Force
}
New-Item -ItemType Directory -Force $out | Out-Null

dotnet publish gui/windows/Paloma/Paloma.csproj `
    --configuration Release `
    --runtime "win-$arch" `
    --output $staging `
    -p:Platform=$platform `
    -p:Version=$version
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

Compress-Archive -Path "$staging/*" -DestinationPath $zip -Force

Write-Output "version: $version"
Write-Output "zip:     $zip"
