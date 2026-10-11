# legaia-party-swap

The Party <-> Delilas battle-model swap kernels behind the patcher's
[Delilas party swap family](../../docs/tooling/patcher-modules.md#delilas-party-swap-delilas_party-module-family):
a playable character's assembled battle model rebuilt on a Delilas monster
rig, and a Delilas model on a player rig. Both animation systems pose parts
by index, so a swap reduces to an anatomy permutation, an extras merge and a
pivot-anchored rest-pose bake (the crate docs walk each step).

## What decides that a module sits here

It is a **pure transform**: decoded disc assets in, rebuilt bytes out, with
no disc I/O. This crate depends on the parser crates only
(`legaia-actor-formats`, `legaia-container-formats`,
`legaia-tmd`, `legaia-tim`, `legaia-lzs`, `legaia-bytes`) and must not depend
on [`legaia-disc-patch`](../disc-patch/README.md) or any patcher crate. The
layer that reads the inputs off a disc and writes the results back is
[`legaia-delilas-party`](../delilas-party/README.md).

The crate root is the model swap itself (the shared rig math and the
player-on-monster-rig direction). Its modules are the other passes built on
the same kernels:

- `playerize` - a Delilas model onto a player rig.
- `fieldize` - the party's field walking models rebuilt from the siblings'.
- `nivora_field` / `event_field` - the mirror direction for the scenes that
  show the siblings: their NPC field meshes rebuilt as the mapped heroes'.
- `winpose`, `moveset`, `weapon_fuse`, `cast_stage` - victory and idle clips,
  the sibling move set, the fused weapon hand and the staged caster rows.
- `enemy_anim` - the enemy-animation remap.

## See also

- [`docs/tooling/randomizer-delilas.md`](../../docs/tooling/randomizer-delilas.md) -
  the party-swap section and its measured anatomy.
