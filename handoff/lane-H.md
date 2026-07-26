# Lane H - the scene mesh pool, rebuilt from the descriptor walk

Follows [`lane-C.md`](lane-C.md). Three of the eleven red tests it left were
mine; all three are green, none of their expectations were re-pinned to what
the new code prints, and one of them turned out to be guarding a hypothesis the
corrected entry size falsifies.

## The change

**The engine builds a scene's mesh pool from the asset-table descriptor walk,
the way retail does, instead of byte-scanning every entry in the CDNAME block.**

`FUN_80020224` reads `count` from the bundle's table base and dispatches each
descriptor through `FUN_8001F05C`. Two cases register meshes:

- **type `0x02` (`TMD`)** - LZS-decode the payload to an `asset::pack`, then
  `for i in 0..count: FUN_80026B4C(buf + offsets[i] * 4)`. `FUN_80026B4C`
  stores at `DAT_8007C018 + DAT_8007b774 * 4` and post-increments the cursor.
- **type `0x09` (`TMD2`)** - one bare mesh, one registration.

That is the whole pool. `legaia_asset::scene_asset_table::mesh_pool` is the
port; `legaia-engine-core::scene_resources::descriptor_walk_pool` lifts it into
`ResolvedTmd`s.

Landed:

- **`crates/asset/src/scene_asset_table.rs`** - `mesh_pool` + `PoolMesh`.
  Note in its doc that `FUN_80026B4C` only *checks* the `0x80000002` magic - a
  member without it logs `Model Version Err` and is registered anyway - so the
  pool size is the pack's declared count, and a caller that drops unparseable
  members is changing the index space.
- **`crates/engine-core/src/scene_resources.rs`** - `descriptor_walk_pool`
  (main scene) and `shared_head_pool` (the `player_data` character pack) run in
  `SceneLoadKind::Field`; the TMD-magic sweep is now the **fallback**, reached
  only when neither applies.
- **`crates/asset/src/scene_event_scripts.rs`** - `record_ranges_positional`,
  the prescript reader for a caller that identified the entry by its slot in
  the block rather than by its contents. See "edteien" below.
- **`crates/engine-core/tests/scene_mesh_pool_walk_disc.rs`** (new) - pins the
  *method*: one mesh-pack carrier per block, every registered member a
  well-formed TMD, and `town01`'s 114 read straight off the pack header.
- Docs: `docs/formats/scene-bundles.md` (new § "The mesh pool is the descriptor
  walk"; the stale `town01 = 121 meshes` corrected to 114),
  `docs/subsystems/asset-loader.md` (§ "The walk is what fills the mesh pool").

## Where the 119 comes from now

Rim Elm's pool is 119 slots. Under the byte sweep that number was
`init_data 29 + bundle 114 + player_data 5 = 148`, and before the entry-size
correction the same sweep happened to total 119 - which is the coincidence
lane C flagged. It is now **derived**, from two numbers neither of which comes
from this code:

| | value | where it comes from |
|---|---|---|
| scene pack | 114 | the `u32 count` at the head of `town01` entry 4's type-`0x02` descriptor payload - retail's own enumeration, read off the disc |
| resident head | 5 | `DAT_8007b6f8`, the prefix `FUN_80020f88` adds to every placement's mesh id (`field_objects::FIELD_ACTOR_PACK_BIAS`), pinned 14/14 against a live walk capture |

The live populated-slot count agreeing with `5 + 114` is now a *check* on two
independent derivations rather than the thing being fitted to.

`init_data`'s 29 dropping out is a consequence of the method, not a tuned
exclusion: PROT 0 carries no asset table and no character pack, so nothing in
the field-load path registers from it. (It *is* a DATA_FIELD stream with a
type-`0x02` chunk, which the boot streaming walker registers - before the
per-scene pool is rebuilt. The pinned 5-slot prefix is what says those meshes
are not in the field pool.)

Corpus shape of the change, walk vs sweep per CDNAME block (124 blocks): 72
agree exactly, 8 the sweep **under**-collected (`taiku2` 46 → 129, `koin1b`
68 → 156, `town0e` 84 → 114, `retock` 84 → 108, `dohaty` 57 → 65, `edkorout`
82 → 83, `son`/`edson` 69 → 70), 21 blocks have no table and keep the sweep
(the v12-family dungeons: `rikuroa`, `dolk2`, `balden`, `rayman`, `ropeway`,
`station`, `tunnelc`, `nilboa`, …). Only `town01`-shaped blocks where the
sweep also reached a sibling `field_pack` / the shared `init_data` stream lose
meshes.

## Scope of the new path

Deliberately `SceneLoadKind::Field` only:

- **`WorldMap`** already does a descriptor read - the kingdom bundle's slot-1
  landmark pack, decoded explicitly - and walking it again double-counts.
- **`Battle`** is a *different* retail loader: `FUN_8001FE70` over the block's
  `scene_tmd_stream` entries, which is where a battle's backdrop dome comes
  from. Gating on `Field` was not optional here - the first cut suppressed the
  sweep for every kind and turned `battle_stage_entries_real::town01_battle_
  build_surfaces_the_stage_mesh` red.

`SceneResources::build_with_shared` (the non-targeted boot path) is untouched
and still sweeps.

## Two corrections to lane C's diagnosis table

Both of lane C's other two rows for me were attributed to "same sweep, same
cause". Measured, neither is.

### 1. `catalogued_field_states` was not the sweep - `edteien` has a 2-record prescript

The three divergences were all `enter_field_scene failed: scene 'edteien' has
no event scripts`. `edteien`'s block is
`[.MAP 778][v12 779][prescript 780][bundle 781][pochi 782]`, and entry 780 is a
perfectly ordinary prescript: `count = 2`, `offsets = [0x0006, 0x0146]`, first
offset anchored at the table end, monotonic, both records in-buffer, and record
0 opening with the **same `00 00 3C 01 03 00 00 00 00 01` lead as `geremi`'s**.

The standalone detector rejected it on `MIN_PRESCRIPT_COUNT = 3`. That floor is
there because `[count][offsets]` is a weak shape on a context-free buffer - the
same reason as the frame-opener rate gate that lane C's positional fallback
already works around. The fallback has the context (the entry seated between
the v12 header and the bundle), so it now reads through
`record_ranges_positional`, which keeps every structural check and drops only
the floor. The classifier's own floor is unchanged, so the
`scene_event_scripts` census is untouched.

`edteien` and `other1` were the only two blocks with a bundle and no resolvable
prescript; both resolve now.

### 2. `pochi_leftovers` was guarding a hazard that does not exist

The test asserted, as its first step, that the hazard is real on the disc -
that `geremi`'s block carries a pochi-filler slot whose leftover TIM targets the
ground page. It does not, and neither does any other block:

- **266 of 266 `Class::PochiFiller` entries are exactly one 2048-byte sector.**
- **Zero of them carry a parseable TIM**, raw or in an LZS section.

There is no stale scratch behind the `pochipochi...` prefix because there is
nothing behind the prefix. What there is, is the *next entry*: `geremi`'s pochi
slot 167 is followed by `scene_tmd_stream` 168, which carries exactly the two
pages the bug report describes - `64 x 256` at fb `(768, 0)` (tpage `0x0C`) and
at `(832, 0)` (tpage `0x1D`). Under the old `toc[p+5] - toc[p+3] + 4` window,
entry 167's buffer ran into 168 and a sweep of the "pochi" slot uploaded the
battle-character atlas over the ground atlas. The rendering bug was real; the
attribution to pochi bytes was the over-read.

The test now asserts the corpus invariant that dissolves the hazard, then
demonstrates the real source positively (the successor entry *does* carry the
page at the ground page's origin), then keeps the original render guarantee
(the built field VRAM has the scene's own atlas there, not that page). The
count assertions are disc measurements, independent of the engine change.

**Out-of-scope follow-up, and the one thing I would do next:**
`docs/formats/pochi.md` still states the falsified claim as its headline, and
`CLAUDE.md`'s pochi row and `docs/formats/overview.md` repeat it. Neither is in
this lane's file scope. Someone should rewrite that page around "a pochi slot
is one reserved sector of fill; the stale-TIM reading was an over-read of the
next entry", and move the falsified version into
`docs/reference/re-do-not-re-walk.md` with the reasoning intact - it is a
textbook case of the entry-size defect producing a plausible, self-consistent,
wrong format claim. The engine-side doc (`scene_resources::pochi_filler_skip`)
and the test are already corrected, so the two now contradict each other until
that lands.

## Verification

- `cargo test -p legaia-engine-core --release --no-fail-fast`: red targets
  7 → 4, red tests 11 → 8. Gone: `field_npc_placements_disc`,
  `field_npc_state_parity_disc`, `field_ground_texture_pages_disc`. Nothing new
  went red - `battle_stage_entries_real` regressed on the first cut and is
  green after the `Field`-only gate.
- Still red, all outside this lane and unchanged in cause:
  `field_actor_spawn_disc_e2e` (2 - the `0x4C` census is pointed at
  event-script entries, but `0x4C` is a field-VM opcode and its scripts are in
  the MAN), `man_variant_carrier_census_disc` (3), `op49_window_census_disc`
  (1), `summon_scene_real` (2 - PROT 0905 mesh parts; lane C's advice to settle
  it with the `summon_binding_base_high` capture rather than a re-pin stands).
- `cargo test -p legaia-asset --release --no-fail-fast` clean.
- `cargo fmt` clean; `cargo clippy --all-targets -- -D warnings` clean on
  `legaia-asset` and `legaia-engine-core`.
- Disc gating proven by **contrast**, not pass count: the three target files
  plus `scene_mesh_pool_walk_disc` print **0** `[skip]` lines with
  `LEGAIA_DISC_BIN` set and **6** with it unset, 4/4 targets ok either way.

## Still open

- **The TIM side is still a byte sweep.** Only the mesh pool moved to the
  descriptor walk. The type-`0x00` (`TIM`) and type-`0x01` (`TIM_LIST`) slots
  are the retail VRAM source in the same way type `0x02` is the mesh source,
  and doing that would finish the "scene loading does not depend on entry
  boundaries" goal. It touches every VRAM parity oracle, so it wants its own
  pass.
- **`SceneResources::build_with_shared`** still sweeps. It is the boot /
  general-purpose path with no scene-kind hint; converging it on
  `build_targeted_with_options` would remove the second reader.
- **The `Battle` pool is still the historical parse-everything sweep.** Its own
  retail walker (`FUN_8001FE70`) is documented and ported in
  `legaia_asset::scene_tmd_stream`; nothing swept the two together yet.
