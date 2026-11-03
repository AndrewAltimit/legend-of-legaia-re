# Frame benchmark

Two drivers measure what the engine costs a player, on a fixed set of scenes and
on the way into them:
[`scripts/ci/frame-bench.py`](../../scripts/ci/frame-bench.py) on the native
window and [`scripts/ci/frame-bench-web.mjs`](../../scripts/ci/frame-bench-web.mjs)
on the browser play page. They print tables and assert nothing: a frame-time gate
on a shared machine can only flake, so the numbers belong in a before / after
comparison on one machine, and not in the repo. What does gate is
[the cost ratchet](#cost-ratchet), on counts rather than times. The frame-hash oracle
that proves a browser change drew the same picture is
[its own page](frame-hash.md).

## Native

```bash
scripts/ci/frame-bench.py                                     # every scene, both fidelities
scripts/ci/frame-bench.py --scene town01 --mode enhanced --repeat 5
scripts/ci/frame-bench.py --json before.json                  # raw rows for a diff
scripts/ci/frame-bench.py --exe new --base-exe old --repeat 5 # the two builds alternating, run by run
```

The driver builds `legaia-engine` with `engine-shell`'s `bench-alloc` feature and
runs `play-window` tick-locked: one sim tick per drawn frame whatever the wall
clock did, so every run walks the same frames and only the time varies. The audio
director runs over a device-less output. It prints the fastest of `--repeat` runs
with the spread between them, which says how far to trust a row on a busy box.
Paired with `--base-exe`, the two builds alternate run by run so a busy box hits both
alike, and the table sets them side by side; both have to carry `bench-alloc`, or
allocations read zero.

| Table | Reads |
|---|---|
| Frame time | average, p99, worst, allocations and bytes per frame, draw counts, the process's peak RSS, then time per stage |
| Allocations by stage | which stage the allocations happen in |
| Startup by step | process up to first frame: renderer and pipelines, VRAM, each mesh family, actors |

The stages are the [frame profiler's](../subsystems/renderer-port.md#frame-profiler).
The worst frame's own stage breakdown goes to stderr beside each row, which is
how a hitch gets a name.

`LEGAIA_PROFILE_ALLOC_SITES=1` on a `bench-alloc` binary samples allocation call
stacks past the skip and prints the heaviest at the end, as `[alloc-site]` lines.

### Entry

```bash
scripts/ci/frame-bench.py --entry --base-exe old --repeat 7   # paired against another build
scripts/ci/frame-bench.py --entry --entry-scenes town01,koin1
```

`legaia-engine entry-bench` enters each scene and forces a fight from it with no
window and no GPU, one `[entry] <scene> <step> <ms>` line per step. The first round
enters through the scene picker and carries the once-per-process caches; the
second, labelled `scene+`, is what a door back into the scene costs. With a base
build the two binaries alternate run by run, so a busy box hits both alike. What a
window adds past these steps (surface, pipelines, mesh upload, draw lists) is the
frame table's startup row.

## Browser

```bash
scripts/ci/build-wasm.sh                                      # or point --site at a scratch copy
scripts/ci/frame-bench-web.mjs --scene town01,battle --profile
scripts/ci/frame-bench-web.mjs --site /path/to/other/site --json out.json
scripts/ci/frame-bench-web.mjs --paths --vsync                # the player's ways in; at the display's rate
scripts/ci/frame-bench-web.mjs --cpu-throttle 6               # a proxy, labelled one
```

It serves a `site/` directory itself, loads the disc named by `LEGAIA_DISC_BIN`
through the page's own file input, and enters each scene through the page's own
scene list. Each row reports the page's frame function as a main-thread cost,
every wasm export's time, calls and returned bytes per frame, GL calls per frame,
long tasks, and the scene-entry time. `--profile` adds a CPU profile: self time by
function, garbage collector included, and the call chains of the heaviest.

The frame function is main-thread time only. The default `--gl egl` asks for a real
GPU, and the output names the renderer it got; `--gl swiftshader` measures a software
rasteriser, where the page's cost is swamped by the fragment work.

### What the frame function leaves out

Each scene row carries these beside it, so one run answers where a weak device pays:

| Row | Reads | Read it as |
|---|---|---|
| `gpu span` | `EXT_disjoint_timer_query_webgl2` around the frame function | An upper bound on GPU work (below) |
| `cadence` | gaps between frame-function starts, and how many ran over 33 ms | What the display got; see `--vsync` |
| `main thread per frame` | the renderer's own `TaskDuration` over the window, less the frame function | What runs outside it, the audio pump named |
| `audio across the battle edge` | `audio_stats_json` before the force and after the hand-over | Dry frames a stall cost; the queue target's move |
| `audio over the timed frames` | `audio_stats_json` around the timed window | Whether the output stays fed at this frame rate, with no edge in it |
| `memory MB` | process RSS off `/proc`, wasm, resident disc bytes, JS heap, a GL tally | Who holds what |

**The GPU span is not GPU time.** The query runs from its begin to its end on the GPU's
clock, so where a flush falls between them it includes the wait for the main thread's
next commands: a begin, a flush, a 10 ms busy wait on the main thread and an end read
10 ms with the GPU idle. So its lower edge is the frame function. A span near the frame
function says the main thread is what limits it; one well above says the GPU is behind.
Where the driver has no timer query the output says so, and cadence is the only reading.

**Cadence needs `--vsync` to mean a display.** By default the driver runs with the
browser's frame limit off, which is what the frame table wants: requestAnimationFrame
then runs as fast as the page gets through it, so the cadence row reads throughput.
`--vsync` leaves the limit on, and the row reads whether the page holds the rate. The limit
moves the frame function too: uncapped, the main thread outruns the GPU and blocks on it inside
its GL calls, so that figure carries GPU backpressure, where at the display's rate it is much
closer to the main thread's own cost and the GPU span to the GPU's. Compare like with like.

**The audio pump is on the main thread.** The worklet only plays blocks. The mix runs in
the handler for its reports on the page's main thread
([`engine-audio/src/webaudio.rs`](../../crates/engine-audio/src/webaudio.rs), `pump`),
and the script-processor fallback mixes on the main thread too. The driver times that
handler directly, with no profiler. `--profile` adds what else is outside the frame
function, by function.

**A dry-frame row can read something here, and the run checks that.** The headless page's
`AudioContext` runs and plays at the device rate. A context that did not run would read
zero dry frames whatever the page did, so each row names the context's state, and the
run ends with a self-check: a 1000 ms stall on the main thread has to read dry frames, or
the output says the row proves nothing. The output counts dry frames only once its first
block has played, and its reports stack up behind a stall, so the row reads them after a
short wait. A real battle edge goes dry under `--cpu-throttle` and under SwiftShader.

**The GL tally is what the page asked for.** It adds up `bufferData`, `texImage*`,
`texStorage*` and `renderbufferStorage*` by object and subtracts deletes; a run checks it
on objects of known size. It is not the driver's account, and the canvas's own buffers are
not in it, so the row gives the canvas's size and whether it has MSAA. Renderer RSS holds
the wasm memory, the disc bytes and the JS heap; GL memory shows in the GPU process's.

### The player's ways in

`--paths` adds a New Game and a card load, each on a fresh page, beside the scene list's
entry. A New Game runs the title, its New Game row and the rules screen by the keyboard.
A card load goes through the saves bar's file input and picks a block; the default card is
`saves/library/cards/playthrough-ladder-pro00-14.mcr` and the first save on it (`--card`,
`--card-block`). Each row's `entry` is the synchronous stretch a player sits through, as a
picker row's is, then the first frame and the worst of the next `--skip` frames. Neither
path builds `ArrivalCatalog`, so a first picker row is not what either pays.

The `second_fight` scene plays a first fight to its end, results included (party at level
99, the confirm button held to a rhythm), then forces the fight the row times: the path
where a memo would show. It says so if the first did not end and the scene list took it
back to the field instead.

A battle row waits for the page's battle scene (`view._battle`), not a frame count. The
intro runs on sim ticks, so on a page drawing faster than the sim ticks a frame count ends
inside it, and the battle edge would land in the timed window or nowhere.

**`(program)` is not one thing.** A CPU profile's largest line outside the frame function is
`(program)`, native code it cannot see into. `--trace` names it: a browser trace over six
seconds, read as self time per trace event on the page's main thread, split by whether a
requestAnimationFrame callback holds the event, in milliseconds per frame. Style, layout, paint
and layerize show there by name, and a run that needs to know who dirtied them adds
`--trace-out file`, which writes the events, invalidation tracking and stacks included. The
page's own DOM writes are the usual cause: `play.html`'s HUD once replaced six text nodes per
frame with the same strings. `--profile` also prints self time by kind (`js`, `wasm`, `program`,
`gl call`, `canvas2d call`, `gc`, `bench wrapper`) and an inclusive table.

`--cpu-throttle n` slows the renderer's main thread through
`Emulation.setCPUThrottlingRate`, and the output calls it a proxy. It shows which costs
scale with the CPU. It says nothing of what a device does: it leaves the GPU process, the
compositor and the worklet as they were. With `--gl swiftshader` it is the pessimistic end
of what a desktop can stand in for. Read it with `--vsync`: uncapped, the page queues GPU
work until a backpressure block of over a second, and frame-function p99 and dry audio read
that queue, which no capped page builds.

`--uncheck id,id` clears those option boxes once the disc is up, `--no-msaa` asks for a
context without MSAA, and `--canvas-scale s` draws the GL canvas at `s` of its size. With
`--gl swiftshader --vsync` they read what one rung of quality costs on a GPU that is short of
it; the shape holds for a software rasteriser, and a tiler may rank them differently.
`--auto-quality` leaves the page's [quality step-down](../subsystems/browser-host.md#the-quality-step-down-play-quality-ladderjs)
armed; otherwise a slow run would read whatever rung the page had stepped to by then, so it is
off by default. A row says when the page stepped during it.

`--entry-profile` samples the disc load, each scene entry and each battle's first
frames and prints time by function, self and inclusive of callees. `--reenter`
enters each scene twice in a row, and `--reload` reloads the page once the disc is
up and times a returning visitor's load from the page's own disc cache. `--idle ms`
waits at the boot-mode prompt before the first pick, which is when the page prewarms
the first entry; without it the bench's pick beats the prewarm. The summary
line reports the head of the disc's read, the disc-load export (`load_disc_spans`) and
the module's memory size.

A bundle built with `web-viewer`'s `bench-alloc` feature adds wasm allocations per
frame (`wasm-pack build --target web --release -- --features bench-alloc`). A wasm
profile names its functions by index; `wasm-bindgen --keep-debug` over the cargo
artifact gives a file whose name section resolves them.

## Cost ratchet

```bash
scripts/ci/cost-ratchet.py --check                # the gate, against scripts/ci/cost-baseline.json
scripts/ci/cost-ratchet.py --measure --runs 10    # the spread between runs, nothing compared
scripts/ci/cost-ratchet.py --write-baseline       # after a real change, with its ledger entry
```

Times flake and counts should not, so this ratchets counts, each from a tick-locked run
that walks the same frames every time. It is the `cost-ratchet` gate in
[`full-gate.py`](build-and-test-speed.md#the-full-gate). A rise past a count's room fails;
raising the baseline is a loosening, which
[`check-baseline-loosening.py`](ratchet-integrity.md) sends to the ledger with its reason.

| Count | Read from | Spread over ten runs of one tree | Room |
|---|---|---|---|
| Bundle gzip bytes | The shipped wasm | None | 0.5% |
| Browser GL draws per frame | [`frame-hash-web.mjs --counts`](frame-hash.md) | None | None |
| Browser wasm bytes returned per frame | The same | None | 0.5% |
| Native draws per frame | `play-window`'s `[bench]` line | None | None |
| Native allocations per frame | The same, on a `bench-alloc` binary | Under 0.05% | 0.5% |
| Native KB allocated per frame | The same | Under 0.05% | 0.5% |

None of the room covers a wobble. It is the size of rise not worth a ledger entry: every
feature moves the bundle, and a field added to a per-frame JSON answer moves its bytes. A draw
always gets an entry. The browser counts cover the frame alone, because the page's own timers
(the memory card's write-back, the XA banks landing) fire when they fire, between frames, and
counted across them the bytes moved by a few dozen a run.

The bundle is read only off the shipped build: a `--fast` bundle is a `[skip]`, and a skip
is not a pass. Browser allocations are not ratcheted, since they would need a second
`bench-alloc` bundle built per gate, and native allocations cover the same kernels.

## What the numbers have shown

These are shapes that recur, not measurements:

- **A posed body re-parses its TMD every frame.** Both hosts' posed builders walk
  every primitive, which is why a primitive's corner streams are held inline
  (`legaia_prims::Corners`) rather than as four heap lists each.
- **A per-tick copy can live in one host only.** An owned read of a whole PROT
  entry (`entry_bytes_extended`) in a browser export's tick path is invisible on
  the native window, which holds its own copy. `entry_bytes` hands out the
  index's shared one. The per-export table is what shows the difference.
- **Shadow layers multiply the draw count.** A draw wholly outside a lamp's
  frustum writes no depth to that layer, so both hosts test each mesh's box per
  light. See [`renderer-port.md`](../subsystems/renderer-port.md#frame-profiler).

- **A first entry pays for what a scene never reads.** The 64 cast modules parse on
  a module's first read (`CastEffectPool`), the fog ground on the first tick with
  the toggle up (`FogVolume::set_ground_source`), and a scene load asks for an
  entry's class without the byte statistics a report carries
  (`categorize::classify_class`). Each deferred value is a pure function of what the
  load captured, so its reader gets what it would have had.
- **Whole-disc copies cost at the wasm boundary.** A wasm memory never shrinks. The
  page's image stays on its side of the boundary and `load_disc_spans` copies across
  only the files the runtime keeps.
- **A page need not read what it may never play.** The image is the largest read on
  the way in, and two thirds of it is movies and XA, which reach the page behind the
  engine ([the disc image](../subsystems/browser-host.md#the-disc-image)).
- **The scene picker's first pick walks every scene.** `ArrivalCatalog::build` reads
  each scene's MAN and `.MAP` to find its doors, once per process. It skips blocks with
  no `.MAP` and stops at a block's bundle (`Scene::load_through_bundle`), since
  classifying the rest was most of the walk and held those bytes in the index cache. A
  door, a New Game and a card load do not build it.
- **The capture frame's cost is the GPU, not the bytes.** `readPixels` on the battle
  transition's arming frame waits for that frame to finish, and the wait does not
  shrink with the size of the read. An asynchronous read would land the capture frames
  later, which moves the transition. The landing reads only the pixels the rects sample
  (`blit_bottom_up_rgba_into_vram`).
- **The worst battle-edge frame is the stage, not the capture.** Where the transition
  hands over, one `play_frame_ticks` builds the battle stage and party forms. That is the
  frame `--entry-profile`'s `battle entry` table names.

## See also

- [`frame-hash.md`](frame-hash.md) - proving a browser change drew the same picture.
- [`renderer-port.md`](../subsystems/renderer-port.md#frame-profiler) - the stages and the profiler's own switches.
- [`browser-host.md`](../subsystems/browser-host.md) - what the play page calls per frame.
- [`shipped-bundle-freshness.md`](shipped-bundle-freshness.md) - which bundle a local site is serving.
