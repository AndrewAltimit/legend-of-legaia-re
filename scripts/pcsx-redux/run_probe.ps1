<#
.SYNOPSIS
  Windows-native PCSX-Redux probe runner -- the counterpart of run_probe.sh.

.DESCRIPTION
  Launches PCSX-Redux with the Legaia disc + BIOS and -dofile's an autorun Lua
  probe (default: the battle->MIDI relay). Sets the LEGAIA_* env vars the autorun
  reads, including LEGAIA_MIDI_WINPORT so the winmm sink (midi_sink.lua) pushes
  Control-Change to your Windows MIDI port.

  Interpreter mode is the default (the recompiler diverges on interpreter-authored
  save states, and Lua breakpoints only fire under -interpreter -debugger).

.EXAMPLE
  # Drive the battle relay into LegaiaDiorama (B); VRChat listens on (A).
  ./scripts/pcsx-redux/run_probe.ps1 -Sstate D:\Unreal\Repos\legend-of-legaia-re\saves\slot_00.bin

.EXAMPLE
  # Dry run (null sink, no MIDI) -- just writes the CC text log for inspection.
  ./scripts/pcsx-redux/run_probe.ps1 -Sstate <state> -NoMidi

.NOTES
  Loopback pairs CROSS OVER: relay -> "LegaiaDiorama (B)", VRChat -> --midi="LegaiaDiorama (A)".
#>
[CmdletBinding()]
param(
    [string]$Lua      = "scripts/pcsx-redux/autorun_battle_midi_stream.lua",
    [string]$Sstate   = $env:LEGAIA_SSTATE,
    [string]$Scenario = "",
    [string]$MidiPort = "LegaiaDiorama (B)",
    [string]$Iso      = $(if ($env:LEGAIA_ISO) { $env:LEGAIA_ISO } else { "$env:USERPROFILE\Documents\ROMS\Legend of Legaia (USA)\Legend of Legaia (USA).bin" }),
    [string]$Bios     = $(if ($env:LEGAIA_BIOS) { $env:LEGAIA_BIOS } else { "$env:USERPROFILE\Documents\DuckStation\bios\SCPH1001.BIN" }),
    [string]$Pcsx     = $(if ($env:PCSX_REDUX) { $env:PCSX_REDUX } else { "$env:USERPROFILE\Tools\pcsx-redux\pcsx-redux.exe" }),
    [int]$Frames      = 1800,
    [int]$Sweep       = 120,
    [string]$OutDir   = "",
    [switch]$Fast,
    [switch]$NoMidi
)
$ErrorActionPreference = "Stop"

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path

# ---- scenario -> save-state resolution (optional; mirrors run_probe.sh) ----
# Uses python + scripts/scenarios.toml when -Scenario is given. Otherwise -Sstate
# (or $env:LEGAIA_SSTATE) is used directly.
if ($Scenario) {
    $py = (Get-Command python, python3, py -ErrorAction SilentlyContinue | Select-Object -First 1).Source
    if (-not $py) { throw "-Scenario needs python (for tomllib); pass -Sstate instead." }
    $manifest = Join-Path $RepoRoot "scripts\scenarios.toml"
    if (-not (Test-Path $manifest)) { throw "scenarios.toml not found: $manifest" }
    $pyResolve = @'
import os, sys, glob, tomllib
manifest_path, label, repo_root = sys.argv[1], sys.argv[2], sys.argv[3]
with open(manifest_path, "rb") as f:
    data = tomllib.load(f)
for s in data.get("scenarios", []):
    if s.get("label") == label:
        fp = s.get("backup_fingerprint")
        if fp:
            hits = sorted(glob.glob(os.path.join(repo_root, "saves", "library", "pcsx-redux", fp + "*")))
            if hits:
                print(hits[0]); sys.exit(0)
        v = s.get("pcsx_redux_sstate")
        if v:
            print(os.path.expanduser(os.path.expandvars(v)))
        sys.exit(0)
sys.exit(2)
'@
    # python '-' reads the program from stdin; argv[1..3] = manifest, label, repo.
    $resolved = $pyResolve | & $py - $manifest $Scenario $RepoRoot
    if ($LASTEXITCODE -ne 0 -or -not $resolved) { throw "scenario '$Scenario' not resolved from scenarios.toml" }
    $Sstate = $resolved.Trim()
}

# ---- preflight ----
if (-not $Sstate) { throw "No save state. Pass -Sstate <path>, -Scenario <name>, or set `$env:LEGAIA_SSTATE." }
$required = @{ "pcsx-redux" = $Pcsx; "iso" = $Iso; "bios" = $Bios; "sstate" = $Sstate; "lua" = (Join-Path $RepoRoot $Lua) }
foreach ($kv in $required.GetEnumerator()) {
    if (-not (Test-Path -LiteralPath $kv.Value)) { throw ("required {0} not found: {1}" -f $kv.Key, $kv.Value) }
}

# ---- output + log locations (per-run subtree, like the bash runner) ----
$stem = [IO.Path]::GetFileNameWithoutExtension($Lua) -replace '^autorun_', ''
if (-not $OutDir) { $OutDir = Join-Path $RepoRoot "captures\$stem\win-run" }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$log = Join-Path $OutDir "pcsx.log"

# Most repo autoruns build a default save-state path as
# `os.getenv("HOME") .. "/Tools/pcsx-redux/..."`. Lua evaluates that default
# expression EAGERLY even when LEGAIA_SSTATE is set, and HOME is unset on Windows
# -> "attempt to concatenate a nil value" aborts the autorun. Provide HOME so the
# (discarded) default is harmless. Does not affect which state actually loads.
if (-not $env:HOME) { $env:HOME = $env:USERPROFILE }

# ---- env the autorun reads ----
$env:LEGAIA_SSTATE        = $Sstate
$env:LEGAIA_STREAM_FRAMES = "$Frames"
$env:LEGAIA_STREAM_SWEEP  = "$Sweep"
$env:LEGAIA_OUT_DIR       = $OutDir
# Empty string reads as "unset" to the sink's from_env (null sink / dry run).
$env:LEGAIA_MIDI_WINPORT  = $(if ($NoMidi) { "" } else { $MidiPort })

# ---- emulator flags (interpreter+debugger unless -Fast) ----
$emuArgs = @('-bios', $Bios, '-iso', $Iso, '-run', '-stdout', '-dofile', $Lua)
if (-not $Fast) { $emuArgs = @('-interpreter', '-debugger') + $emuArgs }

Write-Host "=== run_probe.ps1 ==="
Write-Host "  pcsx-redux : $Pcsx"
Write-Host "  bios       : $Bios"
Write-Host "  iso        : $Iso"
Write-Host "  sstate     : $Sstate"
Write-Host "  lua        : $Lua"
Write-Host "  midi       : $(if ($NoMidi) { '(null sink -- -NoMidi)' } else { $MidiPort })"
Write-Host "  frames     : $Frames  sweep: $Sweep"
Write-Host "  mode       : $(if ($Fast) { 'fast (recompiler -- no Lua BPs)' } else { 'interpreter+debugger' })"
Write-Host "  out_dir    : $OutDir"
Write-Host "  log        : $log"
Write-Host "====================="

# Run from repo root so the autorun's relative package.path entries resolve.
Push-Location $RepoRoot
try {
    & $Pcsx @emuArgs *>&1 | Tee-Object -FilePath $log
    $code = $LASTEXITCODE
} finally {
    Pop-Location
}
Write-Host ""
Write-Host "pcsx-redux exited with status $code"
exit $code
