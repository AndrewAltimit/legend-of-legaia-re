# Community progression capture — one-page guide

Thanks for helping map *Legend of Legaia*'s story progression! You play the
game normally; a small script watches memory and records **which story flags,
items, party members, levels, Seru-magic grants, and battle triggers change, in
which scene, at which map tile, over which music track, and which enemy
formation each fight was** — plus your dialogue choices. One long playthrough
answers a pile of reverse-engineering questions at once.

This uses the **fast** capture (`autorun_state_poll.lua`) — full speed, no
debugger, pleasant to play. It records *what* changed and *where*, not the
exact code that changed it (that's a separate, slower probe the maintainer
runs on just the few cases that need it).

## What you need

- The **USA** disc image: *Legend of Legaia (USA)* — the script fingerprints
  the game and **refuses to run on any other region/revision** (JP/EU/PAL),
  because the memory addresses are USA-specific. This is on purpose: better a
  hard stop than a file full of garbage.
- A PSX BIOS (`SCPH1001.BIN` recommended).
- The locally-built PCSX-Redux the maintainer sends you (the run script points
  at it), or your own PCSX-Redux build.
- The starting save the maintainer sends you (load it so everyone starts from
  the same point) — or start a fresh New Game if asked.

## Run it

```bash
timeout --kill-after=15s 14400s \
  bash scripts/pcsx-redux/run_probe.sh --fast \
    --lua scripts/pcsx-redux/autorun_state_poll.lua \
    --sstate /path/to/the/save/you/were/sent.sstate
```

- `--fast` is required — it's what keeps the emulator at full speed.
- The `timeout` wraps a 4-hour session (`14400s`); raise it if you play longer.
  The script never quits on its own.
- If you'd rather load a memory-card save by hand inside the emulator, add
  `LEGAIA_NO_SSTATE=1` before `bash` and skip `--sstate`.
- **Your existing PCSX-Redux settings are left untouched.** Under `--fast` the
  script runs the emulator against a throwaway profile (a clean, fast preset)
  instead of your saved config, so a debugger-on / odd-GPU / frame-limit setting
  can't slow the capture down — you don't have to change anything in your own
  PCSX-Redux. Your memory cards still work normally (saving in-game writes to
  your usual cards). If you'd rather use your own saved layout/settings, add
  `--no-isolate-config`. On a box where the software renderer is slow, add
  `LEGAIA_PCSX_HARDWARE_GPU=1` before `bash` to use the OpenGL renderer.

Then **just play.** Deeper is strictly better — every new area, boss, and
quest step adds coverage. You'll see a heartbeat line in the log every few
seconds (`alive tick=… scene=…`) confirming it's recording.

### What the extra columns add (all on by default)

The capture now also records, per playthrough:

- **Player tile** on every tile-crossing (in the field) — so a flag flip is
  pinned to *where* on the map it fired, which door/trigger did it.
- **Music track id** whenever the BGM changes.
- **Button presses + your dialogue menu choices** — so a branch decision is
  attributable to the answer you picked.
- **XP, equipment changes, and the fishing/casino/Point-Card counters** — so
  battle rewards, gear swaps, and minigame payouts date themselves.
- **In-battle statuses + HP** — every poison/petrify/etc. infliction and every
  hit's damage, per actor. (One rare status bit is an open research question;
  if your run ever triggers it the script flags it and saves a snapshot —
  that file alone would close the hunt.)
- **Your battle command inputs** — the raw arts-input queue for each party
  member, including the exact byte sequence every combo/Super Art commits to.
  If you perform any of the Super Arts during your run, those rows alone
  validate research data that otherwise needs dedicated capture sessions.
- **Auto-snapshots**: the script quietly saves a full state the first time you
  enter any new area, at each boss fight, at a few key story flags (several of
  which are *open writer hunts* — a snapshot of one firing organically closes
  a research thread by itself), and the first time each character enters arts
  command input. These land as `snap_*.sstate` files next to the CSV — a free
  harvest of mid-story brackets. **If you're short on disk or upload
  bandwidth**, turn them off with `LEGAIA_AUTOSNAP=0` before `bash` (the CSV
  is unaffected).

You can trim any single stream if you want a leaner file: `LEGAIA_TRACE_POS=0`
(tiles), `LEGAIA_TRACE_BGM=0` (music), `LEGAIA_TRACE_INPUT=0` (buttons),
`LEGAIA_TRACE_BATTLE=0` (statuses/HP). None of this is required — the defaults
capture the most.

## What to send back

Everything under the run's output directory (printed at startup, under
`captures/state_poll/<timestamp>/`):

- `state_poll.csv` — the capture. This is the prize.
- `autosave_a.sstate` / `autosave_b.sstate` — crash-resume snapshots (send the
  newest if the emulator crashed and you want the maintainer to continue).
- `snap_*.sstate` — the auto-snapshots (optional but valuable; skip them if
  they're too large to upload). Named for the event that triggered them.

The CSV is small (hundreds of KB even for hours of play) and contains **no
copyrighted game data** — only flag numbers, item ids, character levels,
spell-grant ids, scene names, tile coordinates, music ids, and tick counts.
It's safe to share.

## If it crashes

PCSX-Redux can occasionally segfault on a scene transition (a known emulator
bug). The script autosaves every ~30s, so you lose almost nothing:

```bash
# resume from the newest autosave
… run_probe.sh --fast --lua …/autorun_state_poll.lua \
    --sstate captures/state_poll/<timestamp>/autosave_a.sstate
```

You can also just play in chunks — stop whenever, send what you have, and pick
up later. Partial captures are still useful.

## For the maintainer — locking the version guard before handoff

The guard ships **unlocked** (warn-only) so you can test on your own box. Lock
it so volunteers on the wrong disc get a hard refusal:

1. On your known-good USA disc, run any poll session with `LEGAIA_FP_RECORD=1`:
   ```bash
   LEGAIA_FP_RECORD=1 bash scripts/pcsx-redux/run_probe.sh --fast \
     --lua scripts/pcsx-redux/autorun_state_poll.lua --sstate <your save>
   ```
   It logs `[state_poll] fingerprint = <hex>` and refuses to arm.
2. Paste that hex into `M.USA_FINGERPRINT` in
   `scripts/pcsx-redux/lib/probe/version.lua` (or have volunteers export
   `LEGAIA_FP_EXPECTED=<hex>`).
3. Re-run once without `LEGAIA_FP_RECORD` to confirm `version guard: OK`, then
   hand off.

After that, a JP/EU/PAL or wrong-revision disc trips `FATAL version guard:
MISMATCH` and never records a byte.

## The two-tier model (why this exists)

- **Tier 1 — this probe (community, `--fast`, full speed):** wide net over all
  progression state by per-frame diff. No writer provenance.
- **Tier 2 — `autorun_flag_firehose.lua` (maintainer, interpreter, ~10 fps):**
  exec-breakpoints capture the exact writer `ra` for the specific flags Tier 1
  flags as interesting. Run in short targeted bursts, not a full playthrough.

Tier 1 tells you *what changed where*; Tier 2 tells you *who did it*, only for
the few that matter.
