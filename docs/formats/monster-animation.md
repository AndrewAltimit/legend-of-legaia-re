# Monster (enemy) battle animation

Per-object rigid-transform keyframe animation for battle monsters. Distinct
from the [ANM container](anm.md) (which drives player / field actors): monster
animation lives **inside the monster's archive block** (extraction PROT entry
867 — retail-space CDNAME block `monster_data` under the
[−2 numbering correction](cdname.md#numbering-space); see
[monster stat archive](../subsystems/battle.md) and `legaia_asset::monster_archive`).
The archive is **not** the [player battle files](battle-data-pack.md)
(extraction 863..866, retail `battle_data`) whose extended extraction windows
historically over-read into it.

Implementation: `legaia_asset::monster_archive` (`MonsterAnimation`, `PartPose`,
`animations`, `idle_animation`).

## Where it lives

Each monster's decoded archive block is `[stat record + +0x4C action-offset
array][name][TMD mesh @ +0x04][per-action entries][texture pool @ +0x08]`. The
`magic_count` (`+0x4A`) **action entries** the `+0x4C` u32 array points at are
not just "spells" — each is an action descriptor whose head holds the action id
(`+0x00`), SP cost (`+0x74`), and a sub-id (`+0x77`), and whose **`+0x8c`**
field begins a packed transform-keyframe stream. (The runtime keyframe pointers
at entry `+0x04`/`+0x08` and the self-pointer at `+0x88` are zero on disc; the
loader reconstructs them, with `+0x88` pointing at the `+0x8c` stream.)

Action **index 0** (id `0x00`) is the neutral **idle** animation the engine
loops when the monster isn't acting; index 1 (id `0x01`) is the **move**
cycle played while the monster advances on a target (a walk for grounded
enemies, a flight cycle for fliers), and the rest correspond to the
monster's spell / special actions.

The **player battle files** carry the same per-action entry family for the
party's assembled meshes — action-offset table at the head of `record[0]`,
packed stream at entry `+0xAC` instead of `+0x8C`, `parts` = the
character's skeleton bone count. See
[`battle-data-pack.md` § Battle animations](battle-data-pack.md#battle-animations-record0).

## Packed stream (entry `+0x8c`)

```
u8  part_count    // animated objects per frame == TMD object count
u8  frame_count
frames[frame_count]:
  parts[part_count]:
    u8 b[9]       // six packed 12-bit fields (see below)
```

Each part record is 9 bytes encoding six 12-bit fields. Low bytes sit at
`[0,1,3,4,6,7]`; the high nibbles are packed into `[2,5,8]`:

```
v0 = b0 | (b2 & 0x0f) << 8     tx  (translation X)
v1 = b1 | (b2 & 0xf0) << 4     ty
v2 = b3 | (b5 & 0x0f) << 8     tz
v3 = b4 | (b5 & 0xf0) << 4     rx  (rotation X)
v4 = b6 | (b8 & 0x0f) << 8     ry
v5 = b7 | (b8 & 0xf0) << 4     rz
```

- `tx, ty, tz` are **sign-extended** 12-bit (`-2048..2047`) translation in TMD
  model units.
- `rx, ry, rz` are **unsigned** 12-bit Euler angles (`0..4095`, where `4096` =
  a full turn); values near `4095` are small negative rotations.

One part maps to one [TMD](tmd.md) object (a rigid body part). Across the retail
roster the part count equals the TMD object count for >98% of actions (one model
carries an extra non-animated object).

The transform is **absolute model-space**, not a delta from a rest pose: each TMD
object is modelled at its own local origin (all parts overlap near `(0,0,0)`),
and the per-part `[tx, ty, tz]` places that object at its socket while
`[rx, ry, rz]` orients it about its local origin. The assembled vertex is
`world = Rz·Ry·Rx · v_local + t`. **Frame 0 is therefore the assembled rest
pose** — the translations of a humanoid's left/right limb objects are mirror-symmetric (e.g. Gobu Gobu's arm sockets at `tx ≈ +120` / `-115`), and assembling
frame 0 spreads the collapsed model into its full silhouette.

## Playback

The renderer (`FUN_80048a08`) keeps a 12.4 fixed-point phase in the per-actor
draw struct (`+0x68`): integer frame index = `phase >> 4`, sub-frame fraction =
`phase & 0xf`. The decoder (`FUN_8004998c`) interpolates between frame `i` and
`i+1`: **linear** for translation, **shortest-path angle-wrap** for rotation
(`& 0xfff`, treating a `> 0x800` gap as a wrap). The result is written to a pose
buffer (6 shorts per object) and applied per object via the GTE in the draw
loop, then `FUN_800495c8` / `FUN_8005b038` blend it onto the object vertices.

## Provenance

- `FUN_8004998c` — packed-stream decoder + frame interpolation (`ghidra/scripts/funcs/8004998c.txt`).
- `FUN_80048a08` — per-actor battle draw; reads the phase, drives the decoder, applies the pose per object (`ghidra/scripts/funcs/80048a08.txt`).
- `FUN_800495c8` / `FUN_8005b038` — GTE vertex blend of the decoded pose (`ghidra/scripts/funcs/800495c8.txt`, `8005b038.txt`).
- `FUN_80054cb0` — monster init; copies the action/effect pointer (record `+0x04`) into actor `+0x230` (`ghidra/scripts/funcs/80054cb0.txt`).

## Engine playback

The clean-room engine plays this stream for battle actors. At battle entry the
shell decodes each monster's idle clip (`idle_animation`, action 0) into a
`legaia_engine_core::battle_anim::MonsterAnimPlayer` — an 8.8 fixed-point loop
cursor whose `tick()` interpolates the keyframes (translation linear, rotation
shortest-path 12-bit step, matching `FUN_8004998C`) into a
`legaia_anm::PoseFrame` (one `(translation, rotation)` per object, the same
shape the field ANM player produces). `World::tick_battle_animations` advances
every battle actor's player each frame, and the renderer deforms the mesh with
the rigid `legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot` builder (`R·v + T`,
`Rz·Ry·Rx` about each object's local origin — the same composition as the
glTF export below and the site animator). The disc-gated `battle_anim_real`
test drives the whole decode → player → deform path on a real monster and
asserts the posed mesh moves frame-to-frame. The per-tick phase advance is a
display-rate default, not pinned to retail's exact `actor[+0x68]` delta.

## Export

`legaia_asset::monster_gltf::export_glb(entry, id)` packs a monster's mesh, its
baked texture, and **every** action animation into one binary glTF (`.glb`) — the
universal interchange format. The rigid-per-object model maps directly onto glTF
node animation: each TMD object becomes a node, the keyframe stream's
translation + Euler rotation drive that node's `translation` / `rotation`
channels (the `Rz·Ry·Rx` order recomposed as a quaternion), and a root node
rotates the rig 180° about X to convert the PSX `+Y`-down space to glTF's
`+Y`-up. The per-prim CLUTs (`cba & 0x3F`) that a single glTF material can't
index are baked into a vertical palette atlas, with each vertex's `V` remapped
into its palette band. CLI: `asset monster-archive --id N --glb <out>`; the
enemy-table web page exposes the same export as a download button.

## See also

- [Legaia TMD](tmd.md) - the mesh whose vertices these keyframes morph.
- [ANM animation](anm.md) - the player/field-actor animation container.
- [Player battle files](battle-data-pack.md) - the sibling `battle_data` block (party-character containers, a distinct format from this archive).
- [`subsystems/battle.md`](../subsystems/battle.md) - the battle scene that drives the playback.
