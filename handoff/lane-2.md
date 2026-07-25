# Lane 2 handoff - worklist classification, ignore-list merges, `--audit-ignored`

Findings that land in files this lane does not own. Nothing here is a request to
change lane 2's own files (`classify-worklist.py`, the classification CSV,
`port-catalog-ignore.toml`, `proposed-ignore-additions.toml`,
`worklist-classification.md`, `phantom-print-index.md`, `re-do-not-re-walk.md`).

## Stand-down list for the porting lanes: empty

Every one of the 25 addresses reserved for L3-L6 passes the entry-boundary test -
some extracted image begins a routine at the VA at its mapped base. None is a
phantom, a label-call slice or a data region. No lane should stand down.

The same is true of the whole worklist: of the 44 rows the wave started from,
exactly one (`801dfb10`) was not a port site, and it is now ignored. Treat the
residue as real work.

## `docs/subsystems/field-locomotion.md` carries a phantom address

Line ~1055 describes `FUN_801dfb10` as "a scripted player-turn cutscene state
machine (also keyed on `+0x54`)". The behaviour is accurately described; the
address does not exist. `0x801DFB10` is a `+0xE818` mis-based print from the
`overlay_0897_xxx_dat` batch and its bytes are field (0897) `0x801EE328` - the
world-map `ON RULA` travel-art actor, already documented in
`docs/subsystems/world-map.md` and ported in `engine-vm::travel_art_actor`. In
the images the printed VA is interior everywhere: the fall-through of
`bnez v0,0x801dfb28` in 0898, a branch label in 0897, the delay slot of
`jal 0x8003ce64` in 0899.

The same routine is also printed at `0x801E8B10` by the `overlay_0896` batch at
its `+0x5818` delta, which is the cross-check that pins it. Both phantoms are
now on `docs/tooling/phantom-print-index.md`; the field-locomotion sentence
should either re-address to `FUN_801EE328` or drop, and if it drops the
`documented` flag for `801dfb10` goes with it.

## `801d84b4` is now a worklist row again

It was ignored as `worklist_minigame_structural` with the reason "PADDING, not a
routine". That is true of the fishing / dance / debug-menu / slot-machine
extractions (17 `nop`; 32 in baka_fighter) and false of the field overlay, which
is the image the ignore was costing. Read out of `overlay_field_0897.bin` at base
`0x801CE818`: `jr ra; addiu sp,sp,0x20` closes the predecessor at
`0x801D84AC`/`0x801D84B0`, then a six-instruction leaf stores master game mode
`_DAT_8007B83C = 0x16` (22, CARD INIT), raises `_DAT_8007BB00 = 1`, and returns.
Exactly one `jal 0x801D84B4` in that image. Two base-tagged dumps carry the same
seven-word body - `overlay_cutscene_dialogue_801d84b4` and
`overlay_cutscene_mapview_801d84b4`, both field-overlay captures - so the padding
reason was not even the only dump evidence at the VA.

That is the overlay-local twin of the SCUS scripted game-over trigger
`FUN_8003C7EC`, which the engine already ports (`engine-core::world`
`op4c_n_e_sub_a_call_c7ec`). A porting lane may reasonably fold it into that port
with a second `// PORT:` tag rather than write a new function - the call is
theirs, not this lane's. It classifies `VA_ALIASED`, so the ignore list will not
take it back.

## Three unassigned rows that are cheap and real

Not on any lane's list, all confirmed `REAL` from the images:

- **`801d0290`** - battle overlay (0898) overlay-local PRNG. Twelve instructions,
  no frame: `v = s*12 + 2` built as `s<<2 + s<<3`, then `s = (v<<16) + (v>>16)`
  over the single state word `0x801F6950`. Already correctly written up in
  `docs/reference/functions/battle.md`, which warns that the `overlay_0897` dump
  holds a different body at the VA. The classifier used to call this `UNCERTAIN`
  because its only dump is the field VM's `addiu s8,s8,2` label-call idiom
  printed at the wrong base.
- **`801d56e4`** - fishing overlay (0972) 2-D segment clipper, documented in
  `docs/subsystems/minigame-fishing.md` and `functions/minigames-debug.md`. Its
  dump is **truncated**: Ghidra bounded the function at 524 bytes and the stream
  stops mid-body at `0x801D58EC` with no `jr ra`. A re-dump would help whoever
  ports it.
- **`801cf5d0`** - menu overlay (0899) per-character equipment snapshot, a
  32-instruction frameless leaf. The earlier `INTERIOR` reading came from the
  battle-overlay image, where the VA decodes data as code. The ignore list
  already records the removal; this run confirms it independently from the
  entry-boundary test.

## Eight rows arrived mid-wave from a concurrent lane's dumps

The worklist was 44 at the start of this lane and 52 by the end, with no port
landing and no ignore change accounting for the difference. The added rows are
`8002149c`, `8002174c`, `80059e10`, `80062f98`, `8006320c`, `8006352c`,
`80063aa8`, `80063cec`: each was already `documented` and became `dumped` when
new dumps appeared under `ghidra/scripts/funcs/` during the wave (3812 dumps at
the start, 3896 at the end). All eight classify `REAL` with substantial bodies.

Five of them are the SsAPI sequencer cluster `docs/subsystems/audio.md` documents
as per-slot fan-out and per-channel note/expression handlers - real logic rather
than libsnd shims, and that page explicitly corrects an older "fixed-point div"
label on `8006320C` / `8006352C`. Whether the engine's own sequencer makes them a
scope exclusion is the audio owner's call, not a classification question, so this
lane left them on the worklist.

Two operational notes for the coordinator:

- The shared `target/port-catalog/catalog.csv` in the main checkout is rewritten
  by whichever lane runs `port-catalog.py` there, so a worklist count read off it
  is not reproducible mid-wave. Symlinking `ghidra/scripts/funcs` into a worktree
  (it is in `.git/info/exclude`) lets a lane run the catalog against its own tree
  and its own `target/`, which is what this lane did.
- Any wave-level "worklist went from N to M" claim needs the dump corpus pinned,
  or the denominator moves under it.
