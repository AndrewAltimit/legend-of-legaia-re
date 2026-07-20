# Monster (enemy) battle animation

Per-object rigid-transform keyframe animation for battle monsters. Distinct
from the [ANM container](anm.md) (which drives player / field actors): monster
animation lives **inside the monster's archive block** (extraction PROT entry
867 - retail-space CDNAME block `monster_data` under the
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
not just "spells" - each is an action descriptor whose head holds the action id
(`+0x00`), AGL (action) cost (`+0x74`), and a sub-id (`+0x77`), and whose **`+0x8c`**
field begins a packed transform-keyframe stream. (The runtime keyframe pointers
at entry `+0x04`/`+0x08` and the self-pointer at `+0x88` are zero on disc; the
loader reconstructs them, with `+0x88` pointing at the `+0x8c` stream.)

Action **index 0** (id `0x00`) is the neutral **idle** animation the engine
loops when the monster isn't acting; index 1 (id `0x01`) is the **move**
cycle played while the monster advances on a target (a walk for grounded
enemies, a flight cycle for fliers), and the rest correspond to the
monster's spell / special actions.

## Action tags and the `+0x1EF` reaction map

The entry's first byte (`+0x00`) is a semantic **tag**, not just an index:

| tag | meaning |
|---|---|
| `0` | idle loop |
| `1` | walk / approach cycle |
| `2`, `3` | light hit reactions (flinch variants) |
| `4` | knockdown (heavy hit / death fall) |
| `5` | get-up |
| `7`, `8`, `9` | ready / recover / defeat poses (player files) |
| `0x0B` | block |
| `0x0C..0x1F` | castable spell / special actions (monster AI roll space); within it `0x0D`, `0x0E`, `0x0F` are the monster's **attack moves** (each a distinct move per monster, gameplay-verified across the archive) |
| `0x20`, `0x21`, `0x22` | attack pre-approach / close-in / victory (monster files) |

At battle init the monster installer `FUN_80054CB0` scans the entry table and
caches the **entry index** of each tag in `{2,3,4,5,0x0B}` into battle-actor
bytes `+0x1EF..+0x1F3` (with a tag-4 → tag-2 fallback when no knockdown entry
exists); the party installer `FUN_80053CB8` hardcodes `[2,3,4,5,0xB]` because
the player files store the family identity-ordered. Consumers:

- the damage primitive `FUN_800402F4` stages the target's reaction from the
  map - a surviving target with no get-up entry queues `+0x1EF` (light
  flinch, with the exit-to-idle flag), any other hit queues `+0x1F1`
  (knockdown);
- the anim commit `FUN_8004AD80` chains a finished knockdown (record tag 4)
  into `+0x1F2` (get-up) while the actor lives, or anim id 7 for a downed
  party member, and tests the queued id against `+0x1EF/+0x1F0/+0x1F3` for
  the counter/guard window;
- the battle-action SM (`FUN_801E295C`) resolves monster attack anims by
  **first-byte search** over the entry table (`FUN_80050E2C` with tags
  `0x20`/`1`/`0x21`/`0x22`), staging the returned *index*.

## Anim selection (`actor +0x1D9/+0x1DA` → entry)

The per-actor anim state is a pair of bytes: `+0x1DA` = queued anim id,
`+0x1D9` = current. The id **is the entry index** - the commit function
`FUN_8004AD80` installs `node+0x4C = *(record_ptr + 0x4C + id*4)` for
monsters (record pointers at `0x801C9348 + (slot-3)*4`) and
`node+0x4C = *(table + id*4)` for party (per-character record[0] tables at
`0x801C9360 + slot*4`), then snaps `+0x1D9 = +0x1DA`. There is no remap
table and no special case for id 6: retail's idle id is **0**, and the
battle SM's `FUN_801D5854(actor, 6..9)` "pose" calls are a separate
camera/presentation program space that never touches the anim fields.
Party ids `≥ 0x10` (basic swings staged as direction bytes `0x0C..0x0F` are
still table-direct; art starters `0x19`/`0x1A` and art constants `0x1B+` are
not) trigger the dynamic-slot path instead - see
[`battle-data-pack.md` § Battle animations](battle-data-pack.md#battle-animations-record0).

The **player battle files** carry the same per-action entry family for the
party's assembled meshes - action-offset table at the head of `record[0]`,
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
pose** - the translations of a humanoid's left/right limb objects are mirror-symmetric (e.g. Gobu Gobu's arm sockets at `tx ≈ +120` / `-115`), and assembling
frame 0 spreads the collapsed model into its full silhouette.

## Playback

The renderer (`FUN_80048a08`) keeps a 12.4 fixed-point phase in the per-actor
draw struct (`+0x68`): integer frame index = `phase >> 4`, sub-frame fraction =
`phase & 0xf`. The decoder (`FUN_8004998c`) interpolates between frame `i` and
`i+1`: **linear** for translation, **shortest-path angle-wrap** for rotation
(`& 0xfff`, treating a `> 0x800` gap as a wrap). The result is written to a pose
buffer (6 shorts per object) and applied per object via the GTE in the draw
loop, then `FUN_800495c8` / `FUN_8005b038` blend it onto the object vertices.

The per-frame cursor advance lives in the anim-node tick `FUN_80047430`:
`phase += (frame_dt * actor[+0x21D] * record[+0x78]) >> 1`, where `+0x21D`
is the actor's speed scale (normally `4`) and the entry's `+0x78` byte is
its **playback rate** (`1` or `2` across the retail corpus - `rate/8`
keyframes per 60 Hz tick in the normal case). When the cursor passes the
stream's frame count (or the `+0x1DC` event flags fire mid-clip) the tick
calls the commit `FUN_8004AD80`, which swaps the entry, zeroes the cursor,
and converges `+0x1D9 = +0x1DA`. On the last frame of a clip the decoder
cross-blends toward **frame 0 of the queued entry's stream** (looked up by
`+0x1DA`), so anim transitions tween rather than snap. Entry `+0x84` seeds a
loop-hold counter (`actor +0x176`) and `+0x85`/`+0x86` bound a loop window
(e.g. the player defeat entries hold a 2-frame loop); `+0x87` is a sound
cue fired at install.

## Provenance

- `FUN_8004998c` - packed-stream decoder + frame interpolation (`ghidra/scripts/funcs/8004998c.txt`).
- `FUN_80048a08` - per-actor battle draw; reads the phase, drives the decoder, applies the pose per object (`ghidra/scripts/funcs/80048a08.txt`).
- `FUN_800495c8` / `FUN_8005b038` - GTE vertex blend of the decoded pose (`ghidra/scripts/funcs/800495c8.txt`, `8005b038.txt`).
- `FUN_80054cb0` - monster init; copies the action/effect pointer (record `+0x04`) into actor `+0x230` and builds the `+0x1EF..+0x1F3` tag map (`ghidra/scripts/funcs/80054cb0.txt`).
- `FUN_80047430` - per-frame anim-node tick: cursor advance, end-of-clip detect, commit dispatch (`ghidra/scripts/funcs/80047430.txt`). Its own caller is not in the dump corpus (open).
- `FUN_8004AD80` - anim commit/transition: id → entry install, `+0x1D9` convergence, reaction chaining, dynamic party art slots (`ghidra/scripts/funcs/8004ad80.txt`).
- `FUN_800402F4` - damage primitive; stages the target's hit reaction from the `+0x1EF` map (`ghidra/scripts/funcs/800402f4.txt`).
- `FUN_80050E2C` - first-byte tag search over the entry-pointer array (`ghidra/scripts/funcs/80050e2c.txt`). Signature `(table, tag, count) -> idx_or_0xFF`; both `count` and the result are byte-truncated, so a table longer than 255 entries is unrepresentable and index `0xFF` is indistinguishable from the "not found" sentinel. Ported as `legaia_asset::monster_archive::find_action_by_tag` (sentinel surfaced as `None`), with the tag map at `reaction_map`.

Both take their tags from `action_tags`, which walks **every** entry in the `+0x4C` array. That matters: `animations` skips entries whose keyframe stream is empty or malformed, and since the engine addresses animations by raw entry index (`+0x1DA`), pairing an index against the filtered list mis-maps it.

On the retail disc every monster carrying a light-flinch entry also carries a real tag-4 knockdown, so the tag-4 → tag-2 fallback never fires - it is defensive code, pinned by the disc-gated `monster_reaction_maps_resolve_over_real_archives`.

## Engine playback

The clean-room engine plays this stream for battle actors. At battle entry the
shell decodes each monster's idle clip (`idle_animation`, action 0) into a
`legaia_engine_core::battle_anim::MonsterAnimPlayer` - an 8.8 fixed-point loop
cursor whose `tick()` interpolates the keyframes (translation linear, rotation
shortest-path 12-bit step, matching `FUN_8004998C`) into a
`legaia_anm::PoseFrame` (one `(translation, rotation)` per object, the same
shape the field ANM player produces). `World::tick_battle_animations` advances
every battle actor's player each frame, and the renderer deforms the mesh with
the rigid `legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot` builder (`R·v + T`,
`Rz·Ry·Rx` about each object's local origin - the same composition as the
glTF export below and the site animator). The disc-gated `battle_anim_real`
test drives the whole decode → player → deform path on a real monster and
asserts the posed mesh moves frame-to-frame. The per-tick phase advance is
retail-pinned through the entry's rate byte
(`battle_anim::step_for_rate`, the `FUN_80047430` formula reduced to the
normal `frame_dt = 1`, `+0x21D = 4` case); the engine also plays the
hit-reaction family - `World::queue_battle_reaction` mirrors the
`FUN_800402F4` staging and `tick_battle_animations` the knockdown → get-up
chain. The decoder's cross-blend into the queued clip is a known engine
simplification (transitions restart at frame 0 without the tween).

## Export

`legaia_asset::monster_gltf::export_glb(entry, id)` packs a monster's mesh, its
baked texture, and **every** action animation into one binary glTF (`.glb`) - the
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
