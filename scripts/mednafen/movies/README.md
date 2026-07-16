# Mednafen movie files (`.mcm`)

This is where optional `.mcm` movie files live - bit-exact recordings of
controller input from frame 0. Replaying a movie against the same disc
produces deterministic emulator state at every frame.

Reach for one when a save-state diff isn't precise enough about *when*
something happened: a movie turns "somewhere between these two states"
into a frame-bounded window (see below). The directory ships empty; you
record your own.

## Is it safe to commit?

`.mcm` files are NEVER committed if they encode Sony-owned bytes (movies
that include disc-content snapshots fall under the same Sony-IP rule as
`extracted/`).

Most input-only `.mcm` files are tiny (<10 KB) and contain only the
frame-by-frame button presses, so they ARE safe to commit. When in doubt,
run `file movie.mcm` and check for embedded image data.

## Recording a movie

1. Boot mednafen on the disc and play to the point where you want
   recording to start.
2. Press `Shift+F5` to start recording (mednafen writes to the
   default `mcm/` directory).
3. Play through the input sequence you want to capture (open menu,
   trigger battle, etc.).
4. Press `Shift+F5` again to stop recording.
5. The .mcm file appears in `~/.mednafen/mcm/`.

## Replaying

```bash
scripts/mednafen/run-mednafen.sh disc.bin --movie ~/.mednafen/mcm/movie.mcm
```

To replay deterministically against a known starting state, combine
`--state` and `--movie` (the helper turns `--state mcN` into mednafen's
`-loadstate N`, so the movie replays from that slot instead of frame 0):

```bash
scripts/mednafen/run-mednafen.sh disc.bin --state mc1 --movie /path/to/movie.mcm
```

Add `--save-as mcN` to copy the state out to a named slot after you quit.

## Why this matters for the watchpoint workflow

Save-state diffs (`mednafen-state diff`) tell you WHAT changed between two
points. They don't tell you WHEN - only "after the user moved on with the
game."

A replayable movie gives you a deterministic timeline instead: take a
state at frame 100, replay to frame 110, take another state, diff. The
window of code that ran is bounded to exactly 10 frames.

For coarser bisection you don't need a movie - just take states at
progressive points by hand and `mednafen-state bisect` against them.
