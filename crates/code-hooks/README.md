# legaia-code-hooks

The disc patcher's MIPS code-injection toolkit: the instruction encoders, the
test simulator, the shared injection-arena map, and the hook mods that need
nothing else.

No Sony bytes: every routine is assembled from the encoders here, and nothing
in this crate embeds game data.

## What lives here

| Module | Covers |
|---|---|
| `mips` | The R3000 instruction encoders, register aliases and `lui` / `ori` immediate-split helpers every hand-assembled routine is built from. |
| `mips_sim` | A tiny R3000 subset simulator (delay slots and `hi` / `lo` included) that unit tests run assembled words against. |
| `arenas` | The injection map every code mod plans against: the same-size `Edit`, the verified-dead SCUS regions and the menu-overlay run that host injected code, the live-table ranges a region must stay out of, and the retail sites several mods share. |
| `shiny_seru` | The Shiny Seru hook. |
| `seru_overlay` | The in-shop Seru-trade screen hosted in the menu overlay: the loader routine, the trade UI routine and their layout constants. |
| `seru_trade` | The Seru-trade config blob the overlay reads. |
| `enemy_hp_bar` | The enemy HP gauge drawn with the game's own AP-gauge primitives. |
| `bonus_drop` | The battle-end bonus equipment drop routine. |
| `flee_exp` | The run-away EXP reward spliced into the escape teardown. |
| `approach_fix` | The attack-approach softlock fix. |
| `jewel_fix` | Boss cinematic casts made to respect elemental guards. |
| `conkram_freeze_fix` | The move-VM parts' pose-block and render-node allocations made NULL-safe and quiet (the Conkram mirror freeze). |

The techniques are described in
[`docs/tooling/randomizer-internals.md`](../../docs/tooling/randomizer-internals.md).

## What decides that a module sits here

Two things live in this crate:

- **The toolkit** (`mips`, `mips_sim`, `arenas`) - what any crate that
  injects code needs. A feature finds free space by naming `arenas`; it never
  imports another feature to do so.
- **A hook mod that is only a plan.** Its builder takes image bytes as slices
  and returns `arenas::Edit`s, using the encoders, the arena map and table
  addresses from `legaia-game-tables`. It belongs to no larger feature family.

A hook that is part of a feature family lives with that family instead:
the Tactical Arts hooks in [`legaia-arts-patch`](../arts-patch/README.md), the
swapped hero's cast route in
[`legaia-delilas-party`](../delilas-party/README.md), the dome course in
[`legaia-delilas-challenge`](../delilas-challenge/README.md). A hook whose
builder reaches into the randomizer's own modules (`enemy_ally`, `charm_fix`,
`super_arts_pack`) stays in [`legaia-patcher`](../patcher/README.md).

This crate must not depend on any other patcher crate - it opens no disc, so
it does not even need [`legaia-disc-patch`](../disc-patch/README.md). Which
flag owns which arena region is recorded there, in `space_ledger`; a test in
`legaia-patcher` holds the ledger rows and the constants here in step. The
`apply` layer in `legaia-patcher` writes each plan onto a disc, and the
disc-gated oracles (`crates/patcher/tests`) stay there because they drive the
hooks through it.

## See also

- [`crates/patcher`](../patcher/README.md) - the `apply` layer and the CLI.
- [`crates/disc-patch`](../disc-patch/README.md) - the sector write-back and
  the free-space ledger the arenas are recorded in.
