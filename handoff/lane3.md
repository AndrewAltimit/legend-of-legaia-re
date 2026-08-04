# Lane 3 handoff — Seru-magic + summon viewer

## What landed

A new site page, `site/magic.html` ("Seru Magic & Summons"), driven by a new
WASM host `LegaiaSummons` (`crates/web-viewer/src/summon_view.rs`) and a new
parser entry point `legaia_asset::summon_readef::parse_cast`. All 32 player
Seru-magic casts (`0x81..=0xA0`) and all seven Ra-Seru / Sim-Seru summons play
from the visitor's own disc.

## Notes for the integrator

### `crates/web-viewer/src/lib.rs` (shared with LANE 2)

One line added, module declaration only:

```rust
pub mod summon_view;
```

It sits between `pub mod sfx_view;` and `pub mod texture_pack;`. Nothing else in
that file was touched.

### Out-of-scope changes I did NOT make (noted instead)

1. **`crates/asset/src/monster_archive/animation.rs` duplication.**
   `parse_animation_stream` / `effect_script_head` / `ANIM_RATE_OFFSET` are
   `pub(crate)` inside a **private** `mod animation`, so they are unreachable
   from `summon_readef.rs`. `monster_archive.rs` is not in this lane's scope, so
   `summon_readef` carries its own ~30-line `unpack_pose` / `parse_part_entry`
   mirroring the same `FUN_8004998C` bit layout.
   *Suggested follow-up:* add `pub(crate) use animation::{parse_animation_stream,
   effect_script_head, ANIM_RATE_OFFSET};` to `crates/asset/src/monster_archive.rs`
   and delete the copy. The unit test `unpack_pose_reads_the_twelve_bit_layout`
   plus the disc-gated clip assertions pin the behaviour either way.

2. **`crates/asset/src/bin/asset/summon.rs`** re-implements the 4bpp FX page
   decode inline (the `--texture-png-dir` arm). `summon_readef::decode_texture_slot`
   is now the shared version and the CLI could call it. Not touched — `src/bin/`
   is outside this lane's scope.

3. **`site/_content/home.html`** — the home launcher grid lists the explore
   pages by hand and does not yet include `magic.html`. The left rail, the
   explore sidebar and the nav all do (`site/js/layout.js`). Whoever owns
   `home.html` should add a tile.

4. **`crates/engine-core/src/summon.rs` `SummonScene`** (the move-VM stand-in)
   is untouched. This lane only added the naming tables (`BIG_SUMMONS`,
   `FLUTE_SUMMON_NAMES`, `summon_display_name`), which are pure data + lookups.

### Facts worth reusing

- The seven big summons' ids are pinned by **two** independent disc reads: the
  actor record's inline ASCII attack name at `rec[0]`, and the `+0x1D` element
  byte. Both are asserted in `crates/web-viewer/tests/summon_view_real.rs`.
- A big summon's texture pool + per-part keyframe entries live in the group's
  **third (raw) slot**, not the actor record; the raw slot's first `0x81E0`
  bytes are a monster texture pool byte-for-byte, at monster battle **slot 2**.
- A summon TMD's objects are object-local, so `centroid_bounds` over the raw
  mesh gives a huge radius. Framing must use the **posed** AABB — that is what
  made the first render draw every summon as a speck.
