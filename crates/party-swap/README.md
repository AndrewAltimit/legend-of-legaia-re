# legaia-party-swap

The Party <-> Delilas battle-model swap kernels behind the patcher's
[Delilas party swap family](../patcher/README.md#delilas-party-swap-delilas_party-module-family):
a playable character's assembled battle model rebuilt on a Delilas monster
rig, and a Delilas model on a player rig. Both animation systems pose parts
by index, so a swap reduces to an anatomy permutation, an extras merge and a
pivot-anchored rest-pose bake (the module docs walk each step).

Pure transforms over decoded disc assets - no disc I/O. The disc writes are
[`legaia-delilas-party`](../delilas-party/README.md)'s, the layer above this
one. [`legaia-patcher`](../patcher/README.md) re-exports this crate's one
module at its old path (`legaia_patcher::party_swap`).

- `party_swap` - the model swap itself; its submodules: `playerize` (Delilas
  model onto a player rig), `fieldize` / `event_field` / `nivora_field` (the
  field form), `winpose`, `moveset`, `weapon_fuse`, `cast_stage` and
  `enemy_anim` (the enemy-animation remap).

## See also

- [`docs/tooling/randomizer.md`](../../docs/tooling/randomizer.md) - the
  party-swap section and its measured anatomy.
