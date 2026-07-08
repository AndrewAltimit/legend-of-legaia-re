# Community progression capture — one-page guide

Thanks for helping map *Legend of Legaia*'s story progression! You play the
game normally; a small script watches memory and records **which story flags,
items, party members, and battle triggers change, and in which scene**. One
long playthrough answers a pile of reverse-engineering questions at once.

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

Then **just play.** Deeper is strictly better — every new area, boss, and
quest step adds coverage. You'll see a heartbeat line in the log every few
seconds (`alive tick=… scene=…`) confirming it's recording.

## What to send back

Everything under the run's output directory (printed at startup, under
`captures/state_poll/<timestamp>/`):

- `state_poll.csv` — the capture. This is the prize.
- `autosave_a.sstate` / `autosave_b.sstate` — crash-resume snapshots (send the
  newest if the emulator crashed and you want the maintainer to continue).

The CSV is small (hundreds of KB even for hours of play) and contains **no
copyrighted game data** — only flag numbers, item ids, scene names, and tick
counts. It's safe to share.

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
