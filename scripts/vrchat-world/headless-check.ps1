# Run one of the kit's headless Unity checks against a PROJECT COPY.
#
#   .\scripts\vrchat-world\headless-check.ps1 -Project C:\lgw-qa `
#       -Method LegaiaWorld.LegaiaBatchChecks.CommonPrefabs
#   .\scripts\vrchat-world\headless-check.ps1 -Project C:\lgw-qa `
#       -Method LegaiaWorld.LegaiaCardGameChecks.Soak -PlayMode `
#       -Extra @('-legaiaCardSeconds', '200', '-legaiaCardScale', '2')
#
# Why a copy: the user's editor holds Temp/UnityLockfile on the real
# project, and a batch run on it would fail to open. Make the copy with
# robocopy from PowerShell (the Bash tool mangles /E /XD into paths):
#
#   robocopy "D:\Unity\Projects\Legaia Town01" C:\lgw-qa /E `
#       /XD Temp Logs obj .git Bee /XF ilpp.pid      # exit 1 = success
#
# `Bee` and `ilpp.pid` MUST be excluded: the IL post-processor pipe name
# is a hash of the project path, and a copied Bee DAG points the copy at
# the source project's pipe - every kit script then logs "no MonoScript
# asset found", TryAttachUdon returns null and the builder NREs. A short
# path (C:\lgw-x) matters too: the session scratchpad exceeds MAX_PATH
# for UdonSharp's Roslyn.
#
# What this script does: -Sync copies the kit sources in (sync-to-project
# -Force) and touches every synced .cs so a half-imported copy reimports
# them; then launches the editor detached (Start-Process -PassThru +
# WaitForExit - `-Wait` also waits on the orphaned Unity.Licensing.Client
# and stalls after Unity has exited), with the log INSIDE the copy
# (-logFile at a drive root fails with exit 127 and no log), kills the
# orphaned licensing client, greps the log for the [UdonSharp] compile
# errors that make every later failure a red herring, and prints the
# tail. Edit-mode checks get -quit; a play-mode check (-PlayMode) must
# NOT get -quit - it exits itself (0 pass / 1 assertion / 3 play mode
# never started / 4 watchdog).
#
# Never name a parameter $args here: Start-Process would then launch
# Unity with NO arguments, exit 0 in ~15 s, write no log, and every stage
# would read as green.

param(
    [Parameter(Mandatory = $true)] [string]$Project,
    [Parameter(Mandatory = $true)] [string]$Method,
    [string[]]$Extra = @(),
    [switch]$PlayMode,
    [switch]$Sync,
    [string]$Unity = 'D:\Unity\Editor\2022.3.22f1\Editor\Unity.exe',
    [int]$TimeoutMinutes = 40
)

$ErrorActionPreference = 'Stop'
if (-not (Test-Path (Join-Path $Project 'Assets'))) { throw "not a Unity project: $Project" }
if (-not (Test-Path $Unity)) { throw "no Unity editor at $Unity" }

if ($Sync) {
    # The sync script exits 1 via `exit` when it refuses; -Force never refuses,
    # and a thrown error propagates through $ErrorActionPreference = 'Stop'.
    & (Join-Path $PSScriptRoot 'sync-to-project.ps1') -Project $Project -Force
    $kit = Join-Path $PSScriptRoot 'world-project\Assets\LegaiaWorld'
    foreach ($f in Get-ChildItem (Join-Path $Project 'Assets\LegaiaWorld') -Recurse -Filter *.cs) {
        Add-Content -Path $f.FullName -Value '' -Encoding UTF8
    }
}

$logDir = Join-Path $Project 'Logs'
New-Item -ItemType Directory -Force $logDir | Out-Null
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$short = ($Method -split '\.')[-1]
$log = Join-Path $logDir "$short-$stamp.log"

$argList = @('-batchmode', '-nographics', '-projectPath', "`"$Project`"",
             '-executeMethod', $Method, '-logFile', "`"$log`"")
if (-not $PlayMode) { $argList += '-quit' }
$argList += $Extra

Write-Host "[headless] $Method -> $log"
$p = Start-Process -FilePath $Unity -ArgumentList $argList -PassThru -WindowStyle Hidden
if (-not $p.WaitForExit($TimeoutMinutes * 60 * 1000)) {
    Write-Host "[headless] TIMEOUT after $TimeoutMinutes min - killing Unity" -ForegroundColor Red
    Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
}
$code = $p.ExitCode
# Each headless editor leaves an orphaned licensing client behind.
Get-Process Unity.Licensing.Client -ErrorAction SilentlyContinue |
    Where-Object { $_.StartTime -gt $p.StartTime.AddSeconds(-5) } |
    Stop-Process -Force -ErrorAction SilentlyContinue

if (-not (Test-Path $log)) {
    Write-Host "[headless] NO LOG written (exit $code) - check the arguments" -ForegroundColor Red
    exit 2
}
$lines = Get-Content $log
$usharp = $lines | Select-String -Pattern '\[UdonSharp\].*error|error CS\d+' | Select-Object -First 15
if ($usharp) {
    Write-Host '[headless] compile errors (fix these first - everything after is a symptom):' -ForegroundColor Red
    $usharp | ForEach-Object { Write-Host "  $_" }
}
$lines | Select-String -Pattern '\[Legaia\]' | Select-Object -Last 40 | ForEach-Object { Write-Host $_ }
Write-Host "[headless] exit $code"
exit $code
