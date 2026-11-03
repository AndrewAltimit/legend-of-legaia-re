# Frame benchmark

Two drivers measure what the engine costs a player, on a fixed set of scenes and
on the way into them:
[`scripts/ci/frame-bench.py`](../../scripts/ci/frame-bench.py) on the native
window and [`scripts/ci/frame-bench-web.mjs`](../../scripts/ci/frame-bench-web.mjs)
on the browser play page. They print tables and assert nothing: a frame-time gate
on a shared machine can only flake, so the numbers belong in a before / after
comparison on one machine, and not in the repo.

## Native

```bash
scripts/ci/frame-bench.py                                     # every scene, both fidelities
scripts/ci/frame-bench.py --scene town01 --mode enhanced --repeat 5
scripts/ci/frame-bench.py --json before.json                  # raw rows for a diff
```

The driver builds `legaia-engine` with `engine-shell`'s `bench-alloc` feature and
runs `play-window` tick-locked: one sim tick per drawn frame whatever the wall
clock did, so every run walks the same frames and only the time varies. The audio
director runs over a device-less output. It prints the fastest of `--repeat` runs
with the spread between them, which says how far to trust a row on a busy box.

| Table | Reads |
|---|---|
| Frame time | average, p99, worst, allocations and bytes per frame, draw counts, then time per stage |
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
```

It serves a `site/` directory itself, loads the disc named by `LEGAIA_DISC_BIN`
through the page's own file input, and enters each scene through the page's own
scene list. Each row reports the page's frame function as a main-thread cost,
every wasm export's time, calls and returned bytes per frame, GL calls per frame,
long tasks, and the scene-entry time. `--profile` adds a CPU profile: self time by
function, garbage collector included, and the call chains of the heaviest.

The frame function is main-thread time only. What the GPU process does with the
frame is not in it. The default `--gl egl` asks for a real GPU, and the output names
the renderer it got; `--gl swiftshader` measures a software rasteriser, where the
page's cost is swamped by the fragment work.

`--entry-profile` samples the disc load, each scene entry and each battle's first
frames and prints time by function, self and inclusive of callees. `--reenter`
enters each scene twice in a row, and `--reload` reloads the page once the disc is
up and times a returning visitor's load from the page's own disc cache. The summary
line reports the disc read, the `load_disc_image` export and the module's memory
size.

A bundle built with `web-viewer`'s `bench-alloc` feature adds wasm allocations per
frame (`wasm-pack build --target web --release -- --features bench-alloc`). A wasm
profile names its functions by index; `wasm-bindgen --keep-debug` over the cargo
artifact gives a file whose name section resolves them.

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
  page's image stays on its side of the boundary and `load_disc_image` copies across
  only the files the runtime keeps.
- **The scene picker's first pick walks every scene.** `ArrivalCatalog::build` loads
  and classifies each CDNAME block to find its doors, once per process. A door, a New
  Game and a card load do not build it.

## See also

- [`renderer-port.md`](../subsystems/renderer-port.md#frame-profiler) - the stages and the profiler's own switches.
- [`browser-host.md`](../subsystems/browser-host.md) - what the play page calls per frame.
- [`shipped-bundle-freshness.md`](shipped-bundle-freshness.md) - which bundle a local site is serving.
