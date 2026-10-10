# legaia-code-hooks

The disc patcher's MIPS code-injection layer, split out of `legaia-patcher`:
the instruction encoders, the test simulator, the injection arenas and the
hook mods that need nothing above them. `legaia-patcher` depends on this
crate, never the other way round, and re-exports every module at its old path
(`legaia_patcher::shiny_seru`, `legaia_patcher::enemy_hp_bar`, ...), so the
CLI, the browser patcher and the disc-gated tests name the same paths they
always did.

No Sony bytes: every routine is assembled from the encoders here, and nothing
in this crate embeds game data.

## What lives here

| Module | Covers |
|---|---|
| `mips` | The R3000 instruction encoders, register aliases and `lui` / `ori` immediate-split helpers every hand-assembled routine is built from. |
| `mips_sim` | A tiny R3000 subset simulator (delay slots and `hi` / `lo` included) that unit tests run assembled words against. |
| `shiny_seru` | The Shiny Seru hook, and the `Edit` type plus the SCUS-gap and overlay arena bounds the other hooks seat themselves against. |
| `seru_overlay` | The custom loadable overlay: the loader routine, the Seru-trade UI routine and their layout constants. |
| `seru_trade` | The Seru-trade config blob the overlay reads. |
| `enemy_hp_bar` | The enemy HP gauge drawn with the game's own AP-gauge primitives. |
| `bonus_drop` | The battle-end bonus equipment drop routine. |
| `flee_exp` | The run-away EXP reward spliced into the escape teardown. |
| `approach_fix` | The attack-approach softlock fix. |
| `jewel_fix` | Boss cinematic casts made to respect elemental guards. |
| `delilas_cast` | Routing a swapped hero's signature art into the retail enemy cast module. |
| `item_name` | The injected display name for the unnamed accessory, and the string region the hooks above must stay clear of. |
| `monster` | The monster-archive slot re-pack (`[u32 size][LZS]`, padded to the slot) the drop hook's table edits go through. |

The techniques are described in
[`docs/tooling/randomizer.md`](../../docs/tooling/randomizer.md).

## The split line

A hook module sits here when its builder closes over only the encoders, the
arena bounds, table addresses from `legaia-asset` and - for `delilas_cast` -
the `DiscPatcher` of [`legaia-disc-patch`](../disc-patch/README.md). The crate
root aliases `disc` and `space_ledger` at the `crate::<module>` paths the
files were written against.

What stays in `legaia-patcher`: the `apply` layer that sequences every edit
onto a disc, the randomizer's data shuffles, and the hook mods that reach into
those - `enemy_ally` / `charm_fix`, `custom_items` and the Delilas dome.
Their builders use `mips` and `mips_sim` from here through the patcher's
crate-private re-exports. The Tactical Arts hooks (`arts_ap_grant`,
`oscillating_ap`, `super_art_*`) sit in
[`legaia-arts-patch`](../arts-patch/README.md), which builds on this crate. The disc-gated oracles (`crates/patcher/tests`) stay
with the patcher too, since they drive hooks through `apply`.

## Visibility

`mips` and `mips_sim` are `pub` here because their callers sit on both sides
of the crate boundary. The patcher re-exports them `pub(crate)`, so they are
not part of its public surface.

## See also

- [`crates/patcher`](../patcher/README.md) - the patcher this was split from.
- [`crates/disc-patch`](../disc-patch/README.md) - the sector write-back and
  the free-space ledger the arenas are recorded in.
