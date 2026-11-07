# Browser frame hash

[`scripts/ci/frame-hash-web.mjs`](../../scripts/ci/frame-hash-web.mjs) answers one
question: does this bundle draw what that bundle draws? It is the browser's equivalence
proof for a change that should move how fast a frame arrives and nothing else. `retail-compare`
scores the native window, and `check-pages.mjs` has no pixel check, so nothing else on the
browser side can tell a batched draw from a broken one. It hashes the play page, so it
reads the play bundle, or a site's single module when the base predates that bundle.

```bash
scripts/ci/frame-hash-web.mjs --site site --out new.txt                     # hash one site
scripts/ci/frame-hash-web.mjs --compare base.txt new.txt                    # line for line
scripts/ci/frame-hash-web.mjs --site new/site --base-site old/site --out dir   # both, then compare
scripts/ci/frame-hash-web.mjs --gl egl --no-msaa --out new.txt              # the fast form
```

It exits 1 when two runs differ, naming the first frame and column per scene and mode, and
when a run proves nothing. Hashes are never committed: they differ between drivers, so the
two sides of a comparison come from one machine in one session. The hash run is never the
timing run either, since every frame reads back; that is
[`frame-bench-web.mjs`](frame-bench.md)'s job.

## What a run does

Each of the four default scenes runs in each of two modes on a fresh page, so no scene inherits another's
state:

| Scene | Drives |
|---|---|
| `town01` | A dense town, walked down, up and left by the pad script |
| `world_map` | `map01`, the same script |
| `battle` | `map01`, a forced fight, then the confirm button on a rhythm |
| `cast` | `map01`, a forced fight with a big summon seeded in flight |
| `bio_castle` | `jouina`, the same walk: every env morph live. By name only (`--scene`), for a change to them |
| `menu` | `town01`, the pause menu opened and each row entered and left. By name only |
| `dialog` | `town01`, each placement talked to in turn until one answers, then confirm through its pages. By name only |
| `shop` | `bylon`, the scene's first shop armed, a pick, a close. By name only |
| `results` | `map01`, a fight played to its result windows and the fade out of them. By name only |
| `levelup` | `map01`, eleven fights back to back with the first level-ups on the way, hashed from the ninth fight's end. By name only |

The last five are the overlay's own screens, where the GL column says little. Each fails its run
if its screen never came up (`menu` up, `shop` up, a dialog's draws open, a battle over) or if it
shows too few distinct overlays, and the run prints both figures.

| Mode | Is |
|---|---|
| `retail` | The page's own Retail-faithful and PSX rasterisation boxes: a 320x240 canvas |
| `enhanced` | A fresh browser profile's defaults, every enhancement as a player gets it |

The scene is entered and the page's animation loop stopped in one task, so no frame runs on
the wall clock. Each frame is then the page's own single step: `PlayView.step()`, then
`_frame()`, which runs exactly one sim tick and resyncs the sim clock
(`play_resync_sim_clock`). The pad is keyboard events through the page's own listeners, keyed
by frame number.

One line per frame:

```text
town01 retail 0042 gl=<hash> ov=<hash> st=<hash> lit=1.000 col=704
```

`gl` is the default framebuffer read back, `ov` the 2D overlay canvas the menus, dialogue and
HUD text draw on, `st` the engine's `state_json`. A difference in `st` is the simulation; one
in `gl` alone is the renderer. `lit` and `col` are the share of non-black pixels and the count
of colours, off a sample of the frame, for the check below. DOM text on the page, a movie's
own canvas are not in it. The Muscle Dome's hub screens are: they draw in the GL frame, through
the screen-primitive pass.

## What makes agreeing mean something

A run fails if a scene is vacuous: under a tenth of its frames distinct, over half blank, a pad
script that did not move the player, or a battle that never came up. These were all run:

| Check | How | Result |
|---|---|---|
| One bundle, two runs | Run twice, compare | Agree |
| The play bundle against the tool bundle, which links the same play host | One tree, a site with each on the play page | Agree |
| Two builds of one tree | `build-wasm.sh --fast` twice, the second into a fresh target directory | The same bytes, and agree |
| The shipped build against the iteration build | `build-wasm.sh` against `--fast`: different bytes | Agree |
| A pixel moved, by the driver | `--poke scene/mode:frame` changes one pixel before the read | Exactly that frame differs |
| A pixel moved, in the page | A scratch site whose frame clears one pixel | Every frame of every scene and mode differs, in the `gl` column only |

Repeat the first and the `--poke` after changing the driver.

## What was not repeatable, and what was

**A hardware GPU's MSAA resolve.** On the real driver, the same frame on the same bundle comes
back now and then with a few pixels off by a little, scattered over the picture, with state and
overlay unchanged; once in a while a first frame differs across half its pixels. A frame or two
in ninety does it, a different frame each run, in both modes, with every enhancement off too.
Ask for a context without MSAA and it stops; SwiftShader, which keeps the page's context as it
stands, MSAA included, never does it. So the default is SwiftShader, and `--gl egl --no-msaa` is
the fast form, on a context that differs from a player's by that one
attribute. Plain `--gl egl` warns. Both forms are repeatable: no tolerance is applied to either.

PSX mode draws into a single-sample framebuffer of its own on either form, so its frames do not
depend on the context's MSAA ([the native target](../subsystems/renderer-port.md#the-native-target)):
a retail run with `--no-msaa` and one without agree.

**A 2D canvas's raster.** Read back every frame, the browser moves the overlay from GPU to
software raster at a point that depends on how much was drawn between reads, and the two differ
by a few pixels on a scaled quad. A page that repaints less then reads as drawing something else,
with the frames forced onto one raster agreeing. So the driver asks every 2D context for
`willReadFrequently`: the hash reads the software raster, on both sides.

**Nothing else moved a frame.** These change when things happen against the ticks, and hashes
stayed line for line:

| Moved | How | Reads as |
|---|---|---|
| The wall clock | `--real-clock` | The stepped path reads no clock that reaches a frame |
| The audio pump and disc reads | `--jitter seed` sleeps between steps | The sim is not paced by audio |
| The prewarm, and on an older bundle the disc fill | `--idle ms`, `--no-fill-wait` | Neither changes what is drawn |
| A movie's read and its end | scene `movie` (by name) | The frames after a movie agree; the frames under its overlay hash the world's state only |

Three readers of `performance.now` on the page do not fail this only because the path does not
reach them. A live frame's ticks come from the wall clock (`play_drain_sim_steps`), which the
step replaces. The ocean's CLUT animation in `webgl-tmd-assembled-scene.js` runs on the wall
clock, on the world-overview page: the play page never calls `setOceanAssets`. A movie is paced
by the mixer's XA cursor, the page's audio position or the wall clock (`MovieClock`), so a movie
is outside this oracle; its proof is its PCM and `play_fmv` tests.

The clock stays virtual during a run regardless, advancing 1/60 s a step, so a later reader
of it steps with the frames.

## When two runs differ

`--compare` names the column first, which splits simulation from renderer. Then:

- `--shots dir --shot-every n` writes PNGs. It waits between frames, which can hide a difference
  that needs no wait.
- `--raw dir` writes every frame's raw RGBA after the run, with no wait between frames.
- `--uncheck play-ssao,play-dyn-light` clears the page's own option boxes first; `--no-msaa` asks
  for the context without MSAA.

All of these are Sony-derived. They go in a scratch directory and never in the repo.

## See also

- [`frame-bench.md`](frame-bench.md) - the timing side, and the cost ratchet that reuses this
  driver's stepped path for its counts.
- [`renderer-port.md`](../subsystems/renderer-port.md) - the native side's oracle is
  [`retail-compare.md`](retail-compare.md).
- [`browser-host.md`](../subsystems/browser-host.md) - what the play page calls per frame.
