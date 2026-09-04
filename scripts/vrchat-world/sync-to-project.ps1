# Sync the VRChat kit sources into a Unity project.
#
#   .\scripts\vrchat-world\sync-to-project.ps1 -Project "D:\Unity\Projects\Legaia Town01"
#
# The kit under scripts/vrchat-world/world-project/Assets/LegaiaWorld is the
# single source of truth; the copy inside the Unity project is build output.
# This script copies the Udon/, Editor/ and Shaders/ trees over, and REFUSES
# to run when any project-side file is newer than its kit counterpart - that
# means someone edited inside Unity, and clobbering it would silently lose
# the change. Port the edit back into the repo (or pass -Force to discard).
#
# It never deletes project-side files: Unity's .meta files (which carry the
# GUIDs the scene wiring points at) live only in the project, and a removed
# kit script's leftover copy is a by-hand cleanup.

param(
    [Parameter(Mandatory = $true)]
    [string]$Project,
    [switch]$Force
)

$ErrorActionPreference = 'Stop'

$kitRoot = Join-Path $PSScriptRoot 'world-project/Assets/LegaiaWorld'
$destRoot = Join-Path $Project 'Assets/LegaiaWorld'
if (-not (Test-Path $kitRoot)) { throw "kit root not found: $kitRoot" }
if (-not (Test-Path (Join-Path $Project 'Assets'))) {
    throw "not a Unity project (no Assets/): $Project"
}

$dirs = @('Udon', 'Editor', 'Shaders')

# Guard pass: a project-side file newer than the kit's means an in-Unity edit.
$newerInProject = @()
foreach ($dir in $dirs) {
    $src = Join-Path $kitRoot $dir
    if (-not (Test-Path $src)) { continue }
    foreach ($file in Get-ChildItem $src -Recurse -File) {
        $rel = $file.FullName.Substring($src.Length).TrimStart('\', '/')
        $destFile = Join-Path (Join-Path $destRoot $dir) $rel
        if ((Test-Path $destFile) -and
            ((Get-Item $destFile).LastWriteTimeUtc -gt $file.LastWriteTimeUtc.AddSeconds(2))) {
            $newerInProject += $destFile
        }
    }
}
if ($newerInProject.Count -gt 0 -and -not $Force) {
    Write-Host 'REFUSING to sync - these project files are newer than the kit:' -ForegroundColor Red
    $newerInProject | ForEach-Object { Write-Host "  $_" }
    Write-Host 'Port the edits back into scripts/vrchat-world/world-project/, or re-run with -Force to discard them.'
    exit 1
}

$copied = 0
foreach ($dir in $dirs) {
    $src = Join-Path $kitRoot $dir
    if (-not (Test-Path $src)) { continue }
    foreach ($file in Get-ChildItem $src -Recurse -File) {
        $rel = $file.FullName.Substring($src.Length).TrimStart('\', '/')
        $destFile = Join-Path (Join-Path $destRoot $dir) $rel
        $destDir = Split-Path $destFile -Parent
        if (-not (Test-Path $destDir)) {
            New-Item -ItemType Directory -Force $destDir | Out-Null
        }
        Copy-Item $file.FullName $destFile -Force
        $copied++
    }
}
Write-Host "synced $copied kit files -> $destRoot"
