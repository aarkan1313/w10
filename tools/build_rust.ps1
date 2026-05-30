# Build the wg10_terrain Rust GDExtension WITHOUT force-killing the owner's Godot editor.
#
# Why this exists: the Godot editor loads wg10_terrain.dll, so Windows locks the file and a raw
# `cargo build` fails with "failed to remove file ...wg10_terrain.dll". The `.gdextension` has
# `reloadable = true`, which means the editor RELEASES the DLL lock when its window loses focus.
# So in most cases you do NOT need to close the editor — just click any other window (alt-tab) so
# Godot drops the lock, and the build succeeds. This script makes that the workflow:
#   1. unset CARGO_TARGET_DIR (machine override sends output to the wrong dir otherwise)
#   2. try the build
#   3. if it fails ONLY because the DLL is locked, print the alt-tab/close hint and retry once.
#
# Usage (from repo root or anywhere):  pwsh tools/build_rust.ps1   [-Release]
param([switch]$Release)

$ErrorActionPreference = "Stop"
$crate = Join-Path $PSScriptRoot "..\wg-10\rust"
$env:CARGO_TARGET_DIR = $null   # honor the crate's local target dir, not the machine override

function Invoke-Build {
    Push-Location $crate
    try {
        if ($Release) { cargo build --release } else { cargo build }
        return $LASTEXITCODE
    } finally { Pop-Location }
}

$code = Invoke-Build
if ($code -ne 0) {
    $dll = Join-Path $crate ("target\" + ($(if ($Release) {"release"} else {"debug"})) + "\wg10_terrain.dll")
    $locked = $false
    try { [System.IO.File]::Open($dll, 'Open', 'ReadWrite', 'None').Close() }
    catch { $locked = $true }
    if ($locked) {
        Write-Host ""
        Write-Host "=== DLL is LOCKED by the Godot editor ===" -ForegroundColor Yellow
        Write-Host "The .gdextension is reloadable: just CLICK ANOTHER WINDOW (alt-tab off the Godot" -ForegroundColor Yellow
        Write-Host "editor) so it releases the DLL, then this will retry. (Closing the editor also works.)" -ForegroundColor Yellow
        Write-Host "Waiting 4s for the editor to lose focus, then retrying once..." -ForegroundColor Yellow
        Start-Sleep -Seconds 4
        $code = Invoke-Build
    }
}
exit $code
