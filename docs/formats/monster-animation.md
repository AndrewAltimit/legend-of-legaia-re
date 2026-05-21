# Monster (enemy) battle animation

Per-object rigid-transform keyframe animation for battle monsters. Distinct
from the [ANM container](anm.md) (which drives player / field actors): monster
animation lives **inside the monster's archive block** (PROT entry 867, see
[monster stat archive](../subsystems/battle.md) and `legaia_asset::monster_archive`).

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
loops when the monster isn't acting; index 1 (id `0x01`) is the basic attack,
and the rest correspond to the monster's spell / special actions.

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
