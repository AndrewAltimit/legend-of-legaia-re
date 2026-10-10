# Settled threads: Animation

One area of the [settled reverse-engineering threads](../re-settled-threads.md) register.
The evidence grades (`disassembly` / `capture` / `decompiled-C` / `inference`) are defined on
[the index page](../re-settled-threads.md#the-evidence-column).

This area covers how characters, monsters and effects move: the field clip tick and its two-frame
blender, the player ANM record layout, the battle animation id space, and the move-VM arms that
drive effect ribbons. These answers decide whether a pose in the port matches retail frame for
frame, and they give tool authors the exact byte layout of an animation record.

## Detailed write-ups

Threads whose answer needs more than a table cell. Every other thread is a row of the table under [Threads](#threads).

- [Player ANM per-record layout](#player-anm-per-record-layout)

## Threads

| Thread | Status | Evidence | Answer |
|---|---|---|---|
| At what cadence does retail step a gated field clip? | resolved (`(rate*2 + div - 1) / div`) | `capture` + `disassembly` | `FUN_800204F8` steps a gated clip `(rate*2 + div - 1) / div` (`0x800205B4..0x800205EC`, `div` = clip byte `+6`) and an ungated one `rate`, each times `DAT_1F800393`, wrapping to 0 or clamping at the end. A Rim Elm capture matched 13455 of 13455 checkable calls. Port `field_anim::clip_step` in `FieldClipPlayer`, shared by both hosts ([`anm.md`](../../formats/anm.md)). |
| Does `FUN_8001BE80`'s blend gate equal the clip tick's scaled-step bit? | resolved (yes - one header bit) | `disassembly` | Both test clip byte `+1` bit 0 through `actor+0x4C`: the blender at `0x8001BF70` / `0x8001C0EC`, the clip tick at `0x800205B4`. |
| What does the field frame blender interpolate? | resolved (translations linearly, angles through `FUN_8001D088`, with a retry) | `disassembly` | Translations blend as `cur + ((next - cur) * frac >> 4)`; angles go through `FUN_8001D088(next, cur)`. The next entry is frame + 1; on the last frame it is the entry itself under `+0x62 & 8`, else frame 0. The fraction is `+0x68 & 0xF`. Past a summed arc of `0xC00` the angles are re-blended against `(x+0x800, -(y+0x800), z+0x800)` (`0x8001C170..0x8001C1CC`). Port `PlayerAnmBundle::sample_bone`, live on both hosts ([`anm.md`](../../formats/anm.md)). |
| Does `FUN_80021DF4`'s default motion block run for dispatch `3`? | resolved (no - arms 3 and 5 both branch past it) | `disassembly` | `beq` to `0x80022B80` at `0x800228A8` (arm 3) and `0x800228B0` (arm 5) skip the block `0x800228B8..0x80022B80`; the `+0x9C` counter gates only arm 3's `FUN_80019D50` call. Port `engine-vm::actor_tick::tick_actor` skips the block for both ([actor-vm.md](../../subsystems/actor-vm.md#dispatch-byte-values)). |
| Player ANM per-record layout | resolved (byte-4 low nibble carries `high4(t_z)`) | `disassembly` | [details ↓](#player-anm-per-record-layout) |
| Battle anim-id space + record[0] "strike family" | resolved | `capture` | Anim ids are entry indices (commit `FUN_8004AD80`; idle id = `0`; `FUN_801D5854` ids 6..9 = a camera program space). Tags `2/3/4/5/0xB` = the hit-reaction family (`+0x1EF..+0x1F3` map; `FUN_800402F4` stages flinch/knockdown). Swings = the equipment-section splice (slots `0xC..0xF`) + dynamic art slots `0x10`/`0x11` from the `+0x58` art bank. Capture-pinned + disc census. See [monster-animation.md](../../formats/monster-animation.md) / [battle-data-pack.md](../../formats/battle-data-pack.md). |
| `FUN_80047430` caller | resolved | `capture` | A single dispatch site (capture `autorun_anim_node_tick_caller.lua`, mid-battle save): `jalr v0` at `0x800252B4` inside `FUN_8002519C`, the per-frame actor-list tick iterator, calling the node's `+0x0C` handler slot with the node pointer in `a0`. The anim-node tick is an ordinary list-node tick handler; no other caller fires. See [functions.md](../functions.md). |
| Record[0] `+0x5C` pointer + art-anim bank stream source | resolved (`+0x5C` = vestigial paired-relocation) | `disassembly` (SCUS exhaustive; overlays partial) | Art streams = `"ME"` archives in `readef.DAT` slots `3*char+1`/`3*char+2`. `+0x5C` is a self-relative pointer rebased at load, paired with `+0x58`, by `FUN_80052FA0`. `+0x58` has a reader; **no `+0x5C` reader exists in SCUS** - a word-wise sweep of all 110,080 text words finds one non-`sp` load at that offset, the relocation itself. The overlay side is not exhaustive: 11 overlay images are dump-only, and a dump sweep cannot establish a negative ([dump-corpus-integrity.md](../../tooling/dump-corpus-integrity.md)). See [battle-data-pack.md](../../formats/battle-data-pack.md#me-stream-archives-readefdat). |
| Which carriers arm the effect ribbon `FUN_801CFA48`? | resolved (op-`0x42` nodes in four cast modules, battle only) | `disassembly` | Move-VM op `0x42` stages the ribbon block through the `+0xAC`-relative anim block (`s1 = actor + 0x80`, `0x80023088`); the carriers are one node in PROT 0923, two in 0934, one in 0957 and one in 0964, each seeding `0x3039` with a plain step cap in `+0x9C` (12, 10, 10, 10 and 7 across the five nodes) and no `+0xCA` growth rate, so every bolt draws at full length from its first frame. The draw-kind-4 emitter reaches the ribbon through `jal 0x801CFA48` at `0x8001B120`, a SCUS call into slot-A code, so it lands on the ribbon only while the battle overlay is resident ([`move-vm.md`](../../subsystems/move-vm.md)). |
| Is the ambient ramp pool `FUN_80036D80` part of the motion VM? | resolved (no - an actor template of its own) | `disassembly` | Its only reference is the tick word at `0x800742F4`, so ramps keep landing on frames where `FUN_80038158`'s op loop is gated off. |
| How is the effect ribbon drawn? | resolved (as the actor's own TMD model) | `disassembly` | `FUN_801CFA48` builds a Legaia TMD object header at `out + 0xC` - one group, count `steps * 6`, flags `0x26`, ilen 9, mode `0x3C` - and `FUN_8001ADA4` writes it into every slot of the actor's `+0x44` model list (`0x8001B08C..0x8001B0B4`), so the ordinary TMD renderer draws it. Each step emits the core strip twice, then two flare and two fringe packets, all at UVs `(0..2, 0xF0..0xF2)`, tpage `0x001F`, CLUT `0x7F84` (`0x801CFFF4..0x801D025C`). Engine: `effect_ribbon::ribbon_mesh_for_actor`, drawn on both battle hosts; Gilium (spell `0x95`, PROT 0923) is the cast that shows it in play. |
| Where does move-VM op `0x34` store? | resolved (through `actor + 0x90`) | `disassembly` | Arm `0x80023B64` (jump-table word `0x80010848`) stores its `lh` operands as sign-extended words - `+0xAC`, `+0x9C` (sign half in `+0x9E`), `+0xA0`, `+0xA4`, `+0xA8` - and its `lhu` operands as halfwords at `+0xB0`, `+0x90`, `+0x92`. All eight operands are stored; none wraps to an 8-bit block offset ([`move-vm.md`](../../subsystems/move-vm.md)). |

### Player ANM per-record layout

*Status:* resolved (container + per-`(bone, frame)` semantic). Evidence: `disassembly`.

A record is a 16-byte head followed by one 8-byte entry per `(bone, frame)`; each entry is a
nibble-packed translation plus three byte angles, decoded as the retail sampler `FUN_8001BE80`
(`ghidra/scripts/funcs/8001be80.txt`) does.

- **Size:** `record_size = 16 + 8 * (a & 0xFF) * b`, with `a & 0xFF` the bone count and `b` the
  frame count. It holds byte-exact on all 296 records of the 5 pinned scenes, and on every other
  scene bundle the corpus sweep finds.
- **Head:** 8-byte `(a, b, marker_1 = 0x080C, flag)` header + 8-byte per-anim prologue. Offsets in
  the bundle's offset table are **absolute** byte offsets, not relative to `+4`.
- **Translation (bytes 0..4):** three signed 12-bit values `(t_x, t_y, t_z)`. Byte 2 is
  `high4(t_y) << 4 | high4(t_x)`; byte 4's **low** nibble is `high4(t_z)` (`andi v0,v0,0xf` at
  `0x8001BF38`) and its high nibble is unused. Sign-extend on bit 11.
- **Rotation (bytes 5/6/7):** three `u8` angles `(r_x, r_y, r_z)`, each `<< 4` to a PSX 12-bit
  angle (`4096` = 360 degrees), composed Z, then Y, then X through `FUN_8004638C` /
  `FUN_8004629C` / `FUN_800461A4`.
- **Posing:** a piece poses `R*v + T` about its own object origin (no centroid subtraction); frame
  0 of an idle clip is the rest pose.
- **Blend:** when clip byte `+1` bit 0 is set, `FUN_8001BE80` blends two frames on the 4-bit
  fraction `*(u16*)(actor+0x68) & 0xF`: translations as `a + (((b - a) * frac) >> 4)`, angles
  through the wraparound-aware `FUN_8001D088`, composing into scratchpad `0x1F8002C0`.
- **Not this layout:** `FUN_80021DF4`'s `+0x5A == 6` block uses a separate 24-byte-per-bone
  keyframe layout.

Port: `legaia_asset::player_anm` - `BoneTransform::decode` (one entry, pinned by
`bone_transform_decode_signed_12bit` on town01 record 17) and `PlayerAnmBundle::sample_bone` (the
two-frame blend); the size invariant is the disc-gated `crates/asset/tests/player_anm_real.rs`.
The site characters page runs the same `(t, r)` pipeline.

Owning page: [`anm.md`](../../formats/anm.md).
