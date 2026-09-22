# Field VM - `0x4C` `MENU_CTRL` outer-nibble dispatch

This page details the field/event VM's longest-tail opcode, `0x4C` `MENU_CTRL`,
whose outer high nibble selects 16 sub-dispatchers. It is split out of
[`script-vm.md`](script-vm.md) for length; the opcode-reference table there links
here. Anchor links into this page (e.g. helper-function `#helper-functions`,
`#0x4c-sub-dispatch-coverage-matrix`) resolve against the live anchors below and
in `script-vm.md`.

### 0x4C MENU_CTRL - outer-nibble dispatch

The 0x4C dispatcher's **outer high nibble** of `op0` selects 16 sub-dispatchers:

| Outer nibble | Range | Theme |
|---|---|---|
| 0 | 0x00..0x0F | Party-leader change |
| 1 | 0x10..0x1F | Five-entry sub-table (global writes, screen tint, and the [actor clone](#0x4c-nibble-1-sub-4---the-actor-clone)). `addiu s8,s8,0x7` on entry, then `op0 - 0x10 < 5` selects an arm through the table at `0x801CEEA0`; the `0x14` arm (`0x801E0E80`) reads a sixth payload byte and adds one more, so `4C 14` is the one 8-byte form. |
| 2 | 0x20..0x2F | **Camera-octant / pad-rotation setter** - one arm, no sub-table. Full body: [nibble-2 camera-octant setter](#0x4c-nibble-2---the-camera-octant--pad-rotation-setter). |
| 3 | 0x30..0x3F | Sub-3 cluster (the [ambient-particle master gate](#0x4c-nibble-0x300x3f---the-ambient-particle-master-gate), no-op cluster, player-resync chain, party-state-clear, etc.) |
| 4 | 0x40..0x4F | Immediate-or-ramp cluster (write or ramp ctx slots / globals) |
| 5 | 0x50..0x5F | Five sub-ops off a 5-entry table: model select, NPC move-to-tile, **TAKE_ITEM**, and the two dialog polls. Full body: [nibble-5 sub-op table](#0x4c-nibble-0x500x5f---the-five-sub-op-table). |
| 6 | 0x60..0x6F | 6-word emitter (`func_0x80058490`) + 16-byte halt-acquire |
| 7 | 0x70..0x7F | **Collision-grid rectangular wall paint** (handler `0x801e1c64`); writes the per-scene walkability grid at `_DAT_1f8003ec + 0x4000`. Full body: [nibble-7 wall paint](#0x4c-nibble-0x700x7f---collision-grid-rectangular-wall-paint). |
| 8 | 0x80..0x8F | Large multi-purpose dispatcher (party-slot full heal, conditional jump on `+0x68`, actor model/anim set, actor-search jumps, …). Full body: [nibble-8 multi-purpose dispatcher](#0x4c-nibble-0x800x8f---large-multi-purpose-dispatcher). |
| 9 | 0x90..0x9F | **Floor-height ladder.** Sub-`0xE` installs all sixteen rungs (`-words[i]` into `0x1F80035C + i*2`); sub-`0..2` sets one rung oscillating via `FUN_801DDE34`; sub-`0xF` retires every oscillator (`func_0x8003CF40(_DAT_8007C34C, &LAB_801DA930)`, then PC += 2 - a retire sweep, not a callback registration, and it does not park). See [the detail below](#nibble-9-is-the-floor-height-ladder-not-a-fade). |
| A | 0xA0..0xAF | Conditional jump on flag bit. Sub-0 reads `ctx.flags`, sub-1 reads `ctx.local_flags`, sub-2 reads the global story flag word. Bit SET → take absolute jump from operand[2..4]; bit CLEAR (or sub-3..=0xF) → skip 5 bytes. (The asm dispatches on sub-op first at 0x801e2568, so sub-3..=0xF skip both the per-bank check and the take-jump path.) |
| B | 0xB0..0xBF | No valid sub-op: every one falls into retail's error printer (`jal 0x8001A068` at `0x801E3558`, arm `0x801E3550`). No shipped script carries any `4C Bx` - zero occurrences, clean **or** total, across the whole opcode census. |
| C | 0xC0..0xCF | Small per-actor / per-scene writes (slot table, camera-zone query at a named tile, sound trigger, `field_74` XOR, [camera-focus override](#4c-cf-is-the-script-camera-focus-override)). **All 16 sub-ops are now ported.** Full body: [nibble-C small per-actor / per-scene writes](#0x4c-nibble-0xc00xcf---small-per-actor--per-scene-writes). |
| D | 0xD0..0xDF | Party state + inverted-Y mirror cluster (field SE trigger, linked-list lookup gate, synchronous-spawn actor allocator, party-record search). Full body: [nibble-D party state + inverted-Y mirror cluster](#0x4c-nibble-0xd00xdf---party-state--inverted-y-mirror-cluster). |
| E | 0xE0..0xEF | Misc scene writes + emitter helpers (3-way state write, variable-length text balloon, FMV trigger, camera teleport/animate/zoom, XP add). All non-`P` cells in the matrix above are now ported. Full body: [nibble-E misc scene writes + emitter helpers](#0x4c-nibble-0xe00xef---misc-scene-writes--emitter-helpers). |
| F | 0xF0..0xFF | Only `op0 == 0xFF` valid (pass-through); other sub-ops print `"SUB_CMD_0F_ERROR"` |

The full per-sub-op table is in the field-VM dump (`overlay_0897_801de840.txt`). The from-scratch port mirrors the dispatcher shape with host hooks per sub-cluster - see [`crates/engine-vm/src/field.rs`](../../crates/engine-vm/src/field.rs). The side-effect-free disassembler (`legaia_asset::field_disasm`) carries the same per-sub widths for **all sixteen outer nibbles** so linear census walks stay in sync (nibble `B` is genuinely undefined in retail - no `case 0xb` exists - and decodes as an error).

#### 0x4C nibble 0x30..0x3F - the ambient-particle master gate

Sub-`0` and sub-`1` are a 2-byte pair that raise and clear one word,
`_DAT_8007B854`, then exit via the STATE_RESUME path. Both stores sit in a
jump delay slot: `0x801E0F38` is `sw v0,-0x47ac(v1)` with `v0 = 1`,
`0x801E0F44` is `sw zero,-0x47ac(v0)`, reached off the 16-entry jump table at
`0x801CEEB8`.

The word is **not an input lock**. It has six references disc-wide and none of
them is pad state:

| Site | Form | Role |
|---|---|---|
| `0x800259AC` | `sw zero` | SCUS clear |
| `0x8003B690` | `sw zero` | SCUS clear (field reset `FUN_8003AEB0`) |
| `0x80026EBC` | `lw` | field render pass - stages the `0x8007322C` particle table into scratchpad `0x1F8002D0` when the game mode is `3` and the word is set |
| `0x801D605C` | `lw` | the ambient particle emitter `FUN_801D6058`, its opening `lui`/`lw` pair |
| `0x801E0F38` | `sw` | this sub-`0` |
| `0x801E0F44` | `sw` | this sub-`1` |

So a field script decides, per scene, whether ambience emits at all. The
emitter itself and its spawn are in
[`field-ambient-fx.md`](field-ambient-fx.md#mechanism-4---the-ambient-particle-emitter).

Port: `FieldHost::set_ambient_particle_gate`, forwarded by `FieldHostImpl` to
`World::set_ambient_particles_enabled`, which fans the single word out over the
per-element copies the cutscene element channel carries.

NB the emitter at `0x801D6058` is a **phantom-VA case** in the dump corpus: a
second, unrelated 93-instruction routine occupies that address in another
image and reads nothing at `-0x47ac`. Confirm the 145-instruction field body
(`overlay_cutscene_dialogue_801d6058.txt` /
`overlay_cutscene_mapview_801d6058.txt`) before citing it.

#### 0x4C nibble 0x38..0x3E - the camera-zone arms

Four of outer-nibble 3's arms are the field camera's **only** script-side
grip on the camera parameter block (`0x8007B607..0x8007B627`, see
[`encounter.md`](../formats/encounter.md#man-section-3-the-camera-region-table)).
The arm addresses come from the disc's own nibble-3 jump table at
`0x801CEEB8` in PROT entry `0897`:

| Op | Arm | Body | Width |
|---|---|---|---|
| `[4C 38]` | `0x801E1048` | `FUN_801DE3E0((X - 0x40) >> 7, (Z - 0x40) >> 7)` - query + load at the player's tile. | 2 |
| `[4C 39]` | `0x801E1078` | the same query, then `FUN_80019278(player)` into `player[+0x16]`, then falls into the sub-`E` arm. | 2 |
| `[4C 3D]` | `0x801E10F8` | `FUN_800180EC` at the player's tile - the walk-region **attribute** refresh, not a camera load. | 2 |
| `[4C 3E]` | `0x801E10BC` | `FUN_801DB8EC(player)` snap + `FUN_801DAA50()` focus clamp. | 2 |

`[4C 3E]`'s table entry points **inside** `[4C 39]`'s arm - the two share one
tail. `0x801E10BC` is seventeen instructions past `0x801E1078`, so the snap arm
is the query arm with its query-and-footing head cut off; `[4C 39]` does not
branch to the snap, it falls through into it.
`FUN_801DE3E0` is query-and-load in one: `FUN_801DBA20` picks the record
covering the tile, `FUN_801DBC20` splits it into the block, and a miss
installs the fixed zone-miss set instead.

### Who else loads the block

Nothing re-queries on a bare tile crossing. Disc-wide, `FUN_801DE3E0` has
seven `jal` sites, and the per-frame one is the seventh rather than an eighth:
**two** of the arms above call it (`[4C 38]` at `0x801E1068` and `[4C 39]` at
`0x801E109C` - `[4C 3D]` calls `FUN_800180EC` instead, and `[4C 3E]` has no
call of its own because it is `[4C 39]`'s tail), plus `[4C C4]` at
`0x801E2884`, the player **seat / warp** path at `0x801D1FF4` (which runs the
`[4C 39]` sequence in code - query, `FUN_80019278`, `FUN_801DB8EC`,
`FUN_801DAA50`), its sibling at `0x801D2BCC`, and the SCUS field-init call at
`0x8003B800`. The seventh is in the field **per-frame** controller
`FUN_801D1344`, at `0x801D182C`, and it is gated: `_DAT_1F800394 & 0x400000` (scratchpad flag bit `22`) must
be set, or the frame only eases (`FUN_801DB510`) and clamps
(`FUN_801DAA50`). That bit is not in the per-mode seed of the flag word - the
seed copies a `u16` - so it starts clear every time the game mode changes and
only a script raises it, with op `0x2E` / `0x2F` operand `0x16`. **Eight** of
the disc's CDNAME scenes carry such a site: `ropeway`, `station`, `tunnela`,
`tunnelb`, `tunnelc`, `nilboa`, `nilboa2`, `noaru` (PROT `208`, `228`, `236`,
`273`, `310`, `638`, `648`, `717`), every one of them a `0x2E` SET.

An earlier count of fifteen came from a filter that tested the flag **bank and
bit** rather than the literal operand. The decoder masks (`bit = operand &
0x1F`), so several other operand bytes also reduce to bit 22; every site
carrying one of those sits inside an already-desynced record, and the fifteenth
needs op `0x30` (GFlag TEST) counted too, which raises nothing. Measured with
`asset field-op-census extracted/PROT --only 2E --context` over all 203
carriers, cross-checked against an independent walker.

The per-frame arm also uses the **other** tile convention: `(coord + 0x40)
>> 7` where the seat path and every arm above use `(coord - 0x40) >> 7`, one
tile apart on the same position.

Engine port: the arms queue a `CameraZoneRequest` on `World`
(`engine-core::world::camera_hooks`) which `Camera::tick` drains, because the
camera globals live on the host-owned `Camera` while the VM's host is
`World`. The census + behaviour oracle is
`crates/engine-core/tests/field_camera_zone_arms_disc.rs`.

#### 0x4C nibble 0x70..0x7F - collision-grid rectangular wall paint

**Collision-grid rectangular wall paint** (`[4C, 0x7s, col0, row0, col1, row1 (, mask)]`; handler `0x801e1c64`). Writes the walkability grid at `_DAT_1f8003ec + 0x4000` (the per-scene field buffer; one byte per 128-unit tile, **high nibble = 4 sub-cell wall bits**), the same grid the locomotion collision check `FUN_801cfe4c` reads.

Paints the rectangle `col ∈ [col0, col1+1)`, `row ∈ [row0+1, row1+2)` at index `_DAT_1f8003ec + col + row*0x80 + 0x4000` - note the **row** bounds carry an extra `+1` the column bounds do not (disasm `0x801e1cb4`: `addiu a2, v0, 1` for the row start vs the raw `lbu` for the column start).

Sub-op `s` (= `op0 & 0xF`):
- `0` = clear walls (`byte &= 0x0F`, make walkable)
- `1` = block all (`byte |= 0xF0`)
- `2` = clear `mask` bits (`byte &= ~(mask << 4)`)
- `3` = set `mask` bits (`byte |= mask << 4`).

**Op length depends on the sub-op:** `0`/`1` ignore the mask and are **6-byte** ops (they exit via the `s8 += 6` PC-delta idiom at `0x801e1d24`); `2`/`3` consume the trailing `mask` byte and are **7-byte** ops (`return param_2 + 7`).

These conditional deltas layer on top of the disc-streamed base grid (see [`field-locomotion.md`](field-locomotion.md)); they ride the scene event script and are commonly gated behind nibble-`5`/`7` system-flag tests (story-conditional terrain). (The byte's low nibble is a separate floor-elevation tier; the sibling `_DAT_1f8003ec + 0x8000` grid is a per-tile object/attribute map, not a terrain-flag grid.)

#### 0x4C nibble 0x80..0x8F - large multi-purpose dispatcher

Large multi-purpose dispatcher (party-slot full heal, conditional jump on `+0x68`, …).
- **Sub-2** (3-byte) is `[4C, 0x82, slot]` - **full HP/MP restore of one party slot**, the primitive every inn / rest / infirmary script is built on. Against the 0x414-stride record it writes `*(u16*)(rec+0x106) = *(u16*)(rec+0x104)` and `*(u16*)(rec+0x10A) = *(u16*)(rec+0x108)`, i.e. `hp_cur = hp_max; mp_cur = mp_max`. The slot is a literal operand, not "every active member". There is no inn opcode - the charge is a separate op-`0x4E` gate plus op-`0x3A` debit, which is why the price is per-scene script data; see [field-menu.md](field-menu.md#inn-stay-there-is-no-inn-screen). (The earlier "party-page inventory mirror" reading is superseded.)
- **Sub-1** (round 18, 9-byte) sets actor model + animation frame: `[4C, 0x81, m0..m2, anim_lo, anim_hi, frames_lo, frames_hi]` decodes via [`load_u24_le`](script-vm.md#helper-functions) + `load_u16_le×2`; host applies the immediate-or-tween path based on its actor pool state.
- **Sub-6** (15-byte) is `[4C, 0x86, w0..w5, actor_id]` - it **spawns the reflection controller**, not a transform write; see [below](#4c-86--4c-87-are-the-reflection-controllers-install-and-teardown). PC always += 15 (the `addiu s8,s8,0xf` sits in the resolve's delay slot, so an unresolved actor advances too).
- **Sub-7** (2-byte) is sub-6's **teardown**: `FUN_8003CF40(_DAT_8007C34C, 0x801E5154)` retires every reflection controller, then PC += 2. Not a registration and not a halt - see [below](#4c-86--4c-87-are-the-reflection-controllers-install-and-teardown).
- **Sub-4** (3-byte) is `[4C, 0x84, amplitude]` - **the screen-shake amplitude**. The whole arm is five instructions at `0x801E2134` (jump-table slot `0x801CEF58`): `addiu s8,s8,0x3` / `lbu v1,0x1(s6)` / `lui v0,0x8008` / `j 0x801e3624` / `_sw v1,-0x49d0(v0)`, i.e. `_DAT_8007B630 = operand` as a zero-extended word. That global is the only input to the LCG camera jitter `FUN_801D9D30` (`0` = no shake, `1..=0x15` widens the sample window) and this opcode is its only writer, which makes the field script the sole source of a camera shake. Port: [`FieldHost::op4c_n8_sub4_set_b630`]; engine sink `World::camera.shake_amplitude`.
- **Sub-9** writes `_DAT_80073F00 = i16(operand[1..3])` and advances by 4 (the dump's "FUN_801E3620 dispatch" was Ghidra mis-rendering an internal `goto code_r0x801e3620` label; see the gotcha note below).
- **Sub-B** (round 18, 5-byte) is a conditional jump: `[4C, 0x8B, type_byte, target_lo, target_hi]` jumps to absolute u16 if any actor of `type_byte` is active, else PC += 5.
- **Sub-D** (round 18, 6-byte) is a tristate per-character actor-search: `[4C, 0x8D, char_idx, marker, target_lo, target_hi]` returns one of [`ActorSearchResult::EmptySlot`](../../crates/engine-vm/src/field.rs) (advance 6), `Found` (jump to u16 at +3..=4), or `NoMatch` (halt).
- **Sub-5/E/F** (5-byte `[4C, op0, p0, p1, p2]`) share the standard halt-acquire idiom: on the predicate ([`FieldHost::field_halt_acquire_predicate`]: `saved_pc != 0` or the target is the player, and not already halted or the scene busy) it writes the target's `+0x94` payload pointer, clears `wait_accum`, sets the halt bit, then **advances the caller past the op** (`iVar24 = 5`, `overlay_0897_801de840.txt:6550` / `overlay_world_map_801de840.txt:7179`); on failure it halts the caller at PC (`LAB_801dee50`). Both operate on the resolved cross-context target - the cutscene timeline uses this to freeze its vignette actors, then pokes them beat by beat.

##### `4C 86` / `4C 87` are the reflection controller's install and teardown

The two sit at consecutive slots of the nibble-8 sub-table
(`0x801CEF48`, indexed by `op0 & 0xF` under a `sltiu` bound of `0x10`; the
`lui`/`addiu` pair that forms the base is at `0x801E1EAC`), and they are one
feature rather than two unrelated writes.

`4C 86`'s arm at `0x801E21E0` reads the **last** operand byte (`lbu a0,0xd(s6)`)
as a cross-context actor id, resolves it through `FUN_8003C83C`, and returns
early when it does not resolve - with the PC already advanced, because the
`addiu s8,s8,0xf` sits in that call's delay slot. On a hit it decodes the six
`s16` at operand `+1`, `+3`, `+5`, `+7`, `+9`, `+0xB` through `FUN_8003CE9C`
and calls `FUN_801E573C(executing_ctx, resolved_actor, w0..w5)`.

That spawner allocates from the descriptor at `0x801F2948` - whose `+0x08`
handler word is `0x801E5154`, the reflection tick - and writes
`+0x90 = executing ctx`, `+0x94 = resolved actor`, `+0x54 = 0` and the six
halfwords into `+0x80 .. +0x8A`. So the six words are the **controller's**
mirror line plus tracking rect, not a transform for the named actor.

Which end is which is decided by the tick, not by the spawner's argument
order: `FUN_801E5154` loads `+0x90` into `a3` and `+0x94` into `a2`, then
reads `a2` (`lhu 0x14(a2)`, `lhu 0x5c(a2)`, and the tile test on its
position) and writes `a3` (`sw 0x10(a3)`, `sh 0x14(a3)`, `sh 0x26(a3)`). So
`+0x94` - the **named** actor - is the source, and `+0x90` - the executing
script's own context - is the destination. A script that issues `4C 86`
makes itself the mirror image of the actor it names, which is what its
placement is: each of the ten sites sits in a talk record whose text is a
single parenthesised beat, the answer a reflection gives when addressed.
The image tracks only while the named actor stands inside the rect
(`legaia_engine_vm::field_actor_reflect`; engine seat
`World::spawn_reflection_controller`).

All ten shipped operand blocks take the `(0, zz)` arm - `w0 = 0`, so no X
mirror - and put the Z plane one or two tiles past the rect's own
`max_tile_z`, so the image stands beyond the far wall rather than inside the
room. `concnow`'s `p1[1]` is `w = (0, 0x37A0, 30, 90, 38, 110)` against
`0xF8`, the player; its `p1[4]` and `p1[5]` name two further actors.

`4C 87`'s arm at `0x801E2284` loads the same handler VA and the actor list
`_DAT_8007C34C` and tail-jumps to the shared exit `0x801E2DC4`, which is
`jal 0x8003CF40` with `addiu s8,s8,2` in its delay slot. `FUN_8003CF40`
**retires** rather than registers - the same mislabel `4C 9F` carries for the
floor-ladder oscillator - so the op stops every running reflection controller
and advances two bytes.

`4C 9F`'s arm at `0x801E2548` is the same five instructions against
`0x801DA930` and jumps to that same exit, so it too advances two bytes
unconditionally. Neither op parks: the `addiu s8,s8,2` is *in the call's
delay slot*, which means it has already run before `FUN_8003CF40` is
entered, and the shared tail returns the advanced cursor. Reading either as
a halt-until-callback strands every carrier - fifteen scenes issue `4C 9F`,
140 times in all.

Four shipped scenes issue `4C 86` - `concnow`, `conc2`, `urudre2`, `opurud`,
ten occurrences in all. No shipped scene issues `4C 87`; those scenes drop
their controllers on the scene boundary instead.

#### 0x4C nibble 0xC0..0xCF - small per-actor / per-scene writes

Small per-actor / per-scene writes (slot table, camera-zone query, sound trigger, `field_74` XOR). **All 16 sub-ops are now ported.** Sub-0 is a 2-byte move-table cancel via `func_0x800204F8`; the host gates on whether a move is currently active.
- **Sub-4** is the **camera-zone query at an explicit tile**: 4-byte `[4C, 0xC4, x, z]`, arm at `0x801E2878`, `FUN_801DE3E0(x & 0x7F, z & 0x7F)`. Same load as nibble-3 sub-8 but at a tile the script names, so a scene can frame a shot from a camera-region record the player is not standing in. (The earlier "sub-tile broadcast" label named the caller's shape, not the callee.) See [the camera-zone arms](#0x4c-nibble-0x380x3e---the-camera-zone-arms).
- **Sub-1** is a 1-byte trigger-flag record-array reset: walks `_DAT_80073ED8[..count]` (stride `0xB`), tests each record's 16-bit index via [`party_flag_test`](script-vm.md#helper-functions), writes the inverted bit to `record[0]`; PC always += 2.
- **Sub-3** is a 2-byte script-table teleport (resolves `func_0x8003C8F0(field_50, 0)` then writes `world_x/z` via the standard tile-center `b * 0x80 + 0x40` formula).
- **Sub-5/6** are 4-byte conditional-jump pair (jump-if-zero / jump-if-nonzero): both read a 16-bit flag index via [`load_u16_le`](script-vm.md#helper-functions), query the host's trigger-flag bank, and advance PC += 4 in both branches (the original's "joined" tail at `LAB_801E28C4` returns `param_2 + 4` either way).
- **Sub-0xA/0xB/0xC** are the 5-byte slot-table writes `[4C, 0xCN, slot, lo, hi]` on the u16 array at `0x801C6460`: sub-A sets, sub-B adds, sub-C subtracts (B/C substitute the per-frame tick `_DAT_1F800393` when the literal is `0xFFFF`). The read side is op `0x4E` sub-ops 5..8 (`slot = sub - 5`; [script-vm.md](script-vm.md) op table) - together they form script-visible counters/timers (e.g. cave01's interact counter gating the `0x15D` beat-key spawn).
- **Sub-0xF** is the **script camera-focus override**, not a "position broadcast": 4-byte `[4C, 0xCF, x, z]`, arm at `0x801E2A34`. See [below](#4c-cf-is-the-script-camera-focus-override) for where the two values go and who reads them.
- **Sub-9** is a 2-byte global-pair compare gate: PC += 2 unless `_DAT_8007BAB8 != _DAT_8007BA9C`, then halts.

##### `4C CF` is the script camera-focus override

The arm at `0x801E2A34` zeroes two halfwords and then conditionally rewrites
each from its own operand byte: `0xFF` takes the subject actor's live world
coordinate (`+0x14` for X at `0x801E2A54`, `+0x18` for Z at `0x801E2A8C`), a
zero byte leaves the halfword cleared, and any other byte is the tile-centre
conversion `(b << 7) + 0x40`. The PC advances by 4 through the dispatcher's
`+4` epilogue at `0x801E3620`.

The destinations are `_DAT_8007B628` (X) and `_DAT_8007B62A` (Z), and naming
them is what makes the arm legible, because **the write is not the point - the
reader is.** The focus clamp `FUN_801DAA50` ends by testing each of the two
halfwords and, when non-zero, storing its **negation** into the field camera's
focus point: `0x80089118` for X at `0x801DAB68`, `0x80089120` for Z at
`0x801DAB84`. Those are the two words the field view builder reads as the
MVMVA's translation input, so the op moves the camera's look-at target - and a
zero halfword is not a coordinate of zero, it is "no override", which is why
the arm clears both before writing either.

All eight references to the pair disc-wide are in the field overlay: the two
`lh` reads in the clamp, and the six stores of this one arm
(`find-gp-relative-refs.py 0x310 0x312`, `$gp = 0x8007B318`). An absolute-word
scan sees none of them - both forms here are `lui`+`sh` pairs, which is the
shape that scan is structurally blind to.

**One scene uses it.** The [op census](../tooling/field-op-census.md) puts 46
coherent occurrences in `uru` (PROT 0435) and none anywhere else, with the hits
sitting beside `0x45 C0` camera applies and `CamCfg` writes - a single scene's
hand-framed shots. So the override is live disc data rather than vestigial
code, and it is also not a mechanism the port needs for ninety-odd other
scenes.

#### 0x4C nibble 0xD0..0xDF - party state + inverted-Y mirror cluster

Party state + inverted-Y mirror cluster.
- **Sub-0** (round 18, 6-byte) is a field SE trigger with a conditional u16 pair: `[4C, 0xD0, a_lo, a_hi, b_lo, b_hi]` decodes both via [`load_u16_le`](script-vm.md#helper-functions); the original gates `func_0x8002B994(a, b)` on three flag globals (`_DAT_8007B874`, `_DAT_800846D0`, `_DAT_800846D4`); PC always += 6.
- **Sub-1** (1-byte) is a linked-list lookup gate via `FUN_8003CF04(_DAT_8007C34C, FUN_801DC0BC)` - host returns `Some(new_pc)` for the `LAB_801E360C` ce9c-jump path or `None` for PC += 4 on miss.
- **Sub-2** (`[4C, D2, channel]`) hands the byte after the sub-op to the channel resolver `func_0x8003C83C` and conditionally spawns a script context, then halts at PC - the spawned context is what moves the parent on. The stream footprint is three bytes (`rugi` runs `4C D2 0F .. 4C D2 16` back to back), which is the width a linear walk must take.
- **Sub-3** (14-byte) is `SCHEDULE_TIMED_FLAGS` - a timed-flag scheduler:
  `[4C, 0xD3, expiry_flag: u16, below_flag: u16, duration: u32, threshold: u32]`
  writes `_DAT_800845C0 = (expiry << 16) | below`, duration into
  `_DAT_800845B8`/`_DAT_800845A0`, threshold into `_DAT_800845BC`, snapshots
  the clock (`_DAT_80073ED4 = _DAT_80084570`); PC += 0xE. The per-tick
  consumer `FUN_801d2ebc` decrements by the clock delta, calls
  `FUN_8003CE08(expiry & 0xFFF)` + disarms on expiry, `FUN_8003CE08(below &
  0xFFF)` when under threshold (`0x88888889` magic divide for the seconds
  display). Retail use: `chitei2`'s collapsing-dungeon escape timer (flag
  `0x4C7`, duration 2400, threshold 910) + disarm records in
  `chitei2`/`map03`. The flag slots live in the `0x80084140` save-scratch
  block (persisted). Installer at `FUN_801DE840` case 0xD sub 3
  (`~0x801E2C08`, 0897 file `+0x143F0`). Ported end to end: the installer
  reaches `World::schedule_timed_flags` through
  `FieldHost::op4c_n_d_sub3_party_setup`, and `World::tick_escape_timer`
  drains it once per retail frame into the system-flag bank
  (`legaia_engine_vm::escape_timer::EscapeTimer`).
- **Sub-6** mutates `ctx.field_74`: 3-byte `[4C, 0xD6, b1]`, if `b1 == 4` clears top bit only, else sets bit 0x80000000 + shifts `b1` into the top byte; halts at PC.
- **Sub-7** (1-byte) registers a `FUN_801DC0BC` list-walk callback then halts at PC.
- **Sub-8** (9-byte) is a synchronous-spawn actor allocator: `[4C, 0xD8, vdf_idx, tmd_lo, tmd_hi, kind_lo, kind_hi, var_lo, var_hi]` decodes to `(vdf_idx: u8, tmd_idx: i16, kind: u16, variant: u16)` and routes through host hook [`FieldHost::op4c_n_d_sub8_call_d77f4`] (overlay-resident `FUN_801D77F4`, see `ghidra/scripts/funcs/overlay_cutscene_dialogue_801d77f4.txt`); host writes `actor[+0x3C] = kind` and `actor[+0x3E] = variant` on the allocated slot. Unlike the queue-based `0x4C 0x80` halt-acquire path, the spawn is synchronous - the host emits `FieldEvent::ActorSpawned` directly, with no `pending_actor_spawns` queueing. PC always += 9. What the spawner actually builds, and why `kind` / `variant` are narrower than their names, is [below](#what-the-0x4c-0xd8-spawner-builds).
- **Sub-0xB** (13-byte) calls `FUN_801E57F0(operand)` then PC += 13 (the call site falls through to `LAB_801E2EA0: return param_2 + 0xD`); the helper itself was not decompilable (Ghidra's dump for that address shows data masquerading as code).
- **Sub-0xC** (5-byte) and sub-0xE (5-byte) both call [`small_table_search`](script-vm.md#helper-functions) on a 1-byte needle, then loop over the active party records (stride `0x414`, byte at `+0x196`); on hit, both advance via the `LAB_801E360C` ce9c-jump path; sub-0xC additionally writes the matching slot. Both miss with PC += 5.

#### 0x4C nibble 0xE0..0xEF - misc scene writes + emitter helpers

Misc scene writes + emitter helpers. Ported sub-ops:

- **0** (3-byte 3-way state write `[4C, 0xE0, b1]`: `b1 == 0` sets `DAT_801F2744 = 1`, `b1 < 100` writes `DAT_801F2740 = b1`, `b1 >= 100` writes `picker[+0xE] = b1 - 100`; PC += 3 - the raw asm at `0x801E306C` exits every path through the `addiu s8,s8,0x3` entry at `0x801E00B8`, which the decompile hides as a no-advance `goto LAB_801e00bc`)
- **1** (variable-length text balloon spawn - the field VM's most user-visible opcode, drives the in-game text-encoding pipeline alongside [`crates/mes`](../formats/mes.md); PC = `pc + 3 + packet_length(operand+1)` via [`packet_length`](script-vm.md#helper-functions))
- **2 (FMV trigger, 7-byte: `[4C, 0xE2, lo, hi, _, _, _]`** - reads `(s16)bytecode[2..3]` as the FMV index, writes to `_DAT_8007BA78`, and pokes `_DAT_8007B83C = 0x1A` (next game mode = 26 = `StrInit`); the runtime str_fmv overlay then plays the resolved `MV*.STR`. The trailing 3 bytes are reserved by the dispatcher's PC math but unused. See [`subsystems/cutscene.md`](cutscene.md#field-vm-fmv-trigger-op) for the full Ghidra trace.)
- **3** (3-byte actor position-copy teleport: `[4C, 0xE3, actor_id]` resolves `actor_id` via
  `FUN_8003C83C` and copies that actor's `+0x14`/`+0x16`/`+0x18` position and `+0x26` facing **into
  the executing context** - in the ext form `CC <target> E3 <src>` this teleports the target actor
  onto the source actor's spot, the dolk2 market-swap seat primitive
  ([script-vm § mid-visit re-arrangement](script-vm.md#mid-visit-npc-re-arrangement-beats-dolk2-market-swap--garmel-boss-staging)).
  A ctx with the inverted-Y bit `0x20000000` also gets `+0x8E = -src_y`, and only a **player** ctx
  additionally refreshes the camera scroll (`0x801E3178..0x801E31AC` - the source of the earlier,
  superseded "syncs to the active camera" reading). Raw asm `0x801E3108..0x801E31B0` in
  `ghidra/scripts/funcs/overlay_0897_801de840.txt`; PC += 3 - advances in the `j 0x801E00BC`
  branch-delay slot on the player path and via the `0x801E00B8` +3 entry on the NPC path)
- **4** (9-byte BBox collision query - each operand byte goes through [`tile_center`](script-vm.md#helper-functions); halts via `FUN_801E3614` when the actor is outside the bbox, otherwise PC += 9)
- **5** (5-byte XP add - reads a 24-bit signed delta via [`load_u24_le`](script-vm.md#helper-functions) + `sign_extend_24`, then host clamps to `[0, 9999999]` and triggers party-stats refresh)
- **6** (FUN_801D8280, 8-byte)
- **7** (round 18, 7-byte camera animate: 24-bit LE target + 16-bit LE duration; host schedules `func_0x8003C5F0` tween or instant-write when duration is 0)
- **8** (round 18, 10-byte camera zoom: four 16-bit LE values for `zoom_x`/`zoom_y`/`zoom_z`/`mode`, dispatching to the camera struct's default zoom triplet (`+0x4C/+0x4E/+0x50`) for `mode=0`, or per-mode actor flag writes for `mode=1/2/3`)
- **9** (clear `_DAT_8007B9C4` then PC += 2 via `caseD_4`)
- **0xA** (call `func_0x8003C7EC` then halt)
- **0xB** (5-byte conditional actor lookup with embedded jump target - host returns `Some(())` to take the resolved-actor "pc + 5" path or `None` to jump to the absolute u16 at `operand+2..=3`; jump target read via [`load_u16_le`](script-vm.md#helper-functions))
- **0xC** (capture FUN_801DDF48 return, 2-byte)
- **0xD** (set `_DAT_8007BA66`, 3-byte)
- **0xE** (snapshot `_DAT_80084570 → _DAT_800845DC`, 2-byte).

All non-`P` cells in the matrix above are now ported.

#### Two outer nibbles are the error printer

The `0x4C` outer dispatch is a sixteen-entry jump table at `0x801CEE60`, indexed
by `op0 >> 4` (`srl v1,s3,4` at `0x801E0C44`) under a `sltiu v0,v1,0x10` bound
at `0x801E0C48`; the table base is the `lui`+`addiu` pair at
`0x801E0C50`/`0x801E0C54` (`FUN_801DE840`). The bound cannot actually fail - a
byte's high nibble is always below `0x10` - and its `beqz` lands on
`0x801E3550`, which is the nibble-`B` arm below, so even the unreachable exit is
the error printer. Two of the sixteen arms are not handlers:

- **nibble `B`** (arm `0x801E3550`) materialises a string pointer and falls into
  `jal 0x8001A068`, retail's message printer, then returns the PC unchanged.
- **nibble `F`** (arm `0x801E3538`) does the same with a second string, with one
  exception: sub-`F` (`4C FF`) branches away to the dispatcher's ordinary
  continue at `0x801DF098` first.

So "nibble `B` is undefined" understates it - `F` is an error arm too. What the
two share is a **tail**, not a preamble: each arm forms its own string pointer
first (`0x801CEC98` for `F` at `0x801E3538`..`0x801E354C`, `0x801CECAC` for `B`
at `0x801E3550`..`0x801E3554`), and `F` then jumps into `B`'s last three
instructions - the `jal 0x8001A068` at `0x801E3558` and the return. The disc
agrees: the
[opcode census](../tooling/field-op-census.md) finds **no** coherent
occurrence of any `4C Bx` or `4C Fx` in any scene (the `4C FF` and `4C FA/FB`
totals it reports are all inside records that had already desynced, i.e. text).

#### 0x4C sub-dispatch coverage matrix

The 0x4C cluster is the longest-tail opcode in the field VM - most outer nibbles fan out into 16 inner sub-ops with their own widths and semantics. The coverage matrix below tracks which sub-ops are fully ported (✓), pending an overlay-helper capture (P), or fall through to the dispatcher's default arm (-). "Default" for outer nibbles 1/5/6 means "halts at PC"; for 0/2/3/4/7/8/9/A/C/D/E/F it means PC advances by `1 + width` per the standard fall-through.

| Outer | 0   | 1   | 2   | 3   | 4   | 5   | 6   | 7   | 8   | 9   | A   | B   | C   | D   | E   | F   |
|-------|-----|-----|-----|-----|-----|-----|-----|-----|-----|-----|-----|-----|-----|-----|-----|-----|
| 0     | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   |
| 1     | ✓   | -   | ✓   | ✓   | ✓   | -   | -   | -   | -   | -   | -   | -   | -   | -   | -   | -   |
| 2     | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   |
| 3     | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   |
| 4     | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   |
| 5     | ✓   | ✓   | ✓   | ✓   | ✓   | -   | -   | -   | -   | -   | -   | -   | -   | -   | -   | -   |
| 6     | ✓   | ✓   | -   | -   | -   | -   | -   | -   | -   | -   | -   | -   | -   | -   | -   | -   |
| 7     | ✓   | ✓   | ✓   | ✓   | -   | -   | -   | -   | -   | -   | -   | -   | -   | -   | -   | -   |
| 8     | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   |
| 9     | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   |
| A     | ✓   | ✓   | ✓   | -   | -   | -   | -   | -   | -   | -   | -   | -   | -   | -   | -   | -   |
| B     | -   | -   | -   | -   | -   | -   | -   | -   | -   | -   | -   | -   | -   | -   | -   | -   |
| C     | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   |
| D     | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   |
| E     | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | -   |
| F     | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   |

All 16x16 cells are now either fully ported (`✓`) or fall through to the dispatcher's default arm (`-`). The previously-`P` cells resolved as follows:

- **Outer nibble 1 as a whole.** Its fifteen `P` cells were not fifteen unread arms: the entry bound is `sltiu v0,v1,5` at `0x801E0CA4`, so only `0x10`..`0x14` index the table at `0x801CEEA0` at all, and slot 1 of that table is the common exit `0x801E3624` - `0x11` and `0x15`..`0x1F` therefore advance seven bytes and do nothing, which is the `-` column, not a pending capture. The four real arms are `0x10` (`_DAT_8007B7B0`), `0x12` (screen tint), `0x13` (its sibling triple) and `0x14`, the [actor clone](#0x4c-nibble-1-sub-4---the-actor-clone).

- **`n3 sub-4` / `sub-B` / `sub-C`**: the original at `0x801df208` (in `overlay_0897_801de840.txt`) jumps with delay slot `_addiu s8, s8, 0x2` to `LAB_801df09c switchD_801e00f4::default()` - a 2-byte advance with no side effect (the inline `_DAT_8007b5f0 = uVar31` write is a no-op because `uVar31` was read from the same slot). The Rust port matches: `next_pc = pc + header_size + 1`, no host hook fires.
- **`n3 sub-D`**: the walk-region **attribute refresh** `FUN_800180EC`, hooked as [`FieldHost::region_attributes_refresh_at_player`]. It shares only the tile arithmetic with `sub-8`, which is a camera-zone query ([below](#0x4c-nibble-0x380x3e---the-camera-zone-arms)); routing both through one hook keyed on the sub-op byte hid that they call different routines.
- **`n4 sub-5`**: 11-byte instruction `[4C, 0x45, b1, w94_lo, w94_hi, w96_lo, w96_hi, w98_lo, w98_hi, ticks_lo, ticks_hi]`. The dispatcher splits on `ticks == 0` between [`FieldHost::op4c_n4_sub5_write_immediate`] (direct write) and [`FieldHost::op4c_n4_sub5_ramp`] (STATE_RESUME ramp).
- **`n4 sub-E` / `sub-F`**: no `case` arm in the original inner switch - the `default:` arm prints `"SUB_40_ERROR"` and routes via `switchD_801e00f4::default()`, which for opcode `0x4C` halts at PC. The Rust port returns `StepResult::Halt { final_pc: pc }`.
- **`n8 sub-3`**: 7-byte rectangular tile fill `[4C, 0x83, col_start, row_start, col_end, row_end, value]`. The original at dispatcher lines 6447-6493 walks the inclusive rectangle `[col_start..=col_end] × [row_start..=row_end]`, calling `FUN_801D5630(col, row, ...)` per tile to resolve a tile-record pointer; on hit it writes `tile[+0x3] = 0; tile[+0x2] = value`. The loop exits on `j 0x801e3624` with `_addiu s8,s8,0x7` in its delay slot (`0x801E212C`) and writes nothing else - an earlier reading of a post-loop `_DAT_8007B630 = col_start` trailer named the bytes of the **next** arm (sub-4 at `0x801E2134`), which that unconditional jump never reaches. The Rust port surfaces the rectangle through [`FieldHost::op4c_n_8_sub_3_rect_tile_fill`] and lets the engine implement its tile pool.

The STATE_RESUME-entangled cluster (`0x4C n5 sub-1`/`sub-2`, `n6 sub-0x61`, `n8 sub-0`) routes through the standard halt-acquire predicate ([`FieldHost::field_halt_acquire_predicate`] with new `which` tags `0x61` and `0x80`). On predicate success the dispatcher performs the ctx mutation (`saved_pc`, `wait_accum=0`, `flags |= 0x400`), calls the case-specific side-effect hook (`op4c_n5_sub1_npc_run`, `op4c_n5_sub2_take_item`, `op4c_n6_sub_61_emitter`, `op4c_n8_sub_0_actor_allocator`), and advances PC; on failure it halts at PC. The n5 cluster doesn't route through the predicate (no halt-acquire in the original): sub-1 is a side-effect-only move-table dispatcher, and so is sub-2 - both advance unconditionally.

The n6 sub-`0x61` emitter's retail payload is a **one-shot 16×1 VRAM CLUT-cell write** whose coordinates are the script operands: source `(x, y)` at `+5`/`+7` → libgpu `MoveImage` cell copy, or a flat BGR555 fill of all 16 entries when the source y is zero; destination `(x, y)` at `+9`/`+0xB`. It is the one-shot half of the world-map palette cycling (see [`functions.md` § 801E4C58](../reference/functions/script-vms.md#801e4c58) and [`world-map.md`](world-map.md) "Ocean animation").

The `n8 sub-0` host hook (`FieldHost::op4c_n8_sub_0_actor_allocator`) receives `(count, tail)`: `count` is the byte at `operand+1` and `tail` is the raw bytecode slice from `operand+2` onward. The host walks `count` variable-length child-actor records out of `tail` using the [`packet_length`](script-vm.md#helper-functions) rule (`FUN_8003CA38`): bytes `<= 0x1E` terminate a record; bytes whose top nibble is `0xC` consume one extra byte. The parent script's PC always advances by 3 regardless of how many records were walked - the records remain embedded in the bytecode buffer and become the spawned actors' own bytecode (retail stores the per-actor bytecode pointer at `actor[+0x90]`). The engine-core implementation (`FieldHostImpl::op4c_n8_sub_0_actor_allocator`) splits the records,
queues each one into `World::pending_actor_spawns`, and emits a `FieldEvent::ActorAllocate { records }` so engines can route them into their own actor pool.

Materializing the queued records into actor slots is a separate engine-side step. [`World::materialize_actor_spawns(start_slot)`] drains `pending_actor_spawns`, allocates the first inactive slot from `actors[start_slot..MAX_ACTORS]`, populates `Actor::spawn_record` with the raw bytecode bytes, and emits one `FieldEvent::ActorSpawned { slot, kind, variant, record }` per allocation. The retail allocator for this opcode (`overlay_world_map_801de840.txt:7080-7123`, case `8 sub-0`) allocates from pool `0x801f28a0` and writes `actor[+0x90]` (bytecode start), `actor[+0x94]` (parent back-pointer) and `actor[+0x54] = 0`; it does **not** write `actor[+0x3C]` (kind) or `actor[+0x3E]` (variant), so the event's `kind = 0` / `variant = 0` match retail - this is a faithful zero, not a placeholder.
The `0x4C 0xD8` path is the one that decodes explicit `(kind, variant)` u16 immediates and routes through `FUN_801D77F4`; the `0x4C 0x80` path is bytecode-only by design. When the slot range is exhausted, a `FieldEvent::ActorSpawnFailed { record }` event surfaces the dropped request instead.

#### 0x4C nibble 1 sub-4 - the actor clone

`4C 14` is the field VM's after-image: it duplicates one actor's transform
onto a fresh pool node that fades itself out and retires. It is the **only**
eight-byte instruction in outer nibble 1 and the only allocation site on the
disc for the static actor template at SCUS `0x80070644`.

```text
4C 14 <r> <g> <b> <rate_lo> <rate_hi> <src_id>
```

The arm at `0x801E0E80` reads `lbu a0,6(s6)` - a byte past the five every
other arm of this nibble uses - resolves it through the cross-context walk
`FUN_8003C83C`, and calls `FUN_801D835C(src, u24, s16)` with the two operands
the nibble's own prologue and this arm decode: `FUN_8003CEB8(&operand[1])`
(the 24-bit colour word) and `FUN_8003CE9C(&operand[4])` (the signed rate).
Its exit is `j 0x801E3624` with `addiu s8,s8,1` in the **branch delay slot**,
so the eighth byte is consumed on the unresolved-source path too.

`FUN_801D835C` (48 instructions, field overlay file `0x9B44`) stores the
source's `+0x64` into the descriptor's `+0x04` **low halfword** (`sh`, so the
`0xFFFF` marker half survives), allocates through `FUN_80020DE0` against the
generic effect-actor list, then copies `src[+0x14..+0x1B]` (position) and
`src[+0x24..+0x2B]` (rotation) through `lwl`/`lwr` pairs, copies `src[+0x4C]`
(the bound model word) and `src[+0x68]`, and writes `dst[+0x54] = rate`,
`dst[+0x74] = colour`. A null allocation ends the routine with nothing else
written.

The clone's own per-frame body is the descriptor's `+0x08` word,
`FUN_801D820C`: `+0x78 += (i16)+0x54 * DAT_1F800393` each frame, and at
`0x1000` it pins `+0x78` to `0xFFF` and sets the retire bit `+0x10 |= 8`.
So the rate is the clone's lifetime - `0x199` is about ten vsyncs - and
`+0x74` is the modulation colour the sprite / widget family reads as packed
RGB (`FUN_801F7A9C` draws from it, `FUN_801F8004` writes it; see
[`move-vm.md`](move-vm.md)).

Six shipped scenes issue the opcode - `vozz`, `retona`, `urudre3`, `kor5`,
`nilboa`, `noaru` - always as a short burst, joined by the `WaitFrames`
between the instructions. The census below is what the disc actually asks
for.

Port: `legaia_engine_core::field_actor_clone` (the plan kernel),
`World::spawn_actor_clone` (the allocation and the id resolve), and
`World::tick_handler_actors` (the clone's tick, through
`ActorHandler::ClipFade`).

##### What the disc asks for

Ninety-four clean sites across those six scenes, every one eight bytes wide,
walked off each scene's MAN carriers with the field-VM disassembler
(`legaia-engine-core` test `field_actor_clone_burst_disc`).

Three rates, five `(colour, rate)` pairs, and the rate is the clone's life:

| colour word (`r`,`g`,`b`) | rate | vsyncs alive | where |
|---|---|---|---|
| `0x32,0x28,0x1E` / `0x37,0x2D,0x23` / `0x3C,0x32,0x28` | `0x0199` | 11 | `vozz` only - one word per ramp step |
| `0x3C,0x3C,0x28` | `0x00B2` | 24 | every other scene's bursts |
| `0x2A,0x2A,0x3F` | `0x0080` | 32 | `noaru` only |

Nothing on the disc sets the word's top byte, so the `sw` at `0x801D83FC` is
a 24-bit write in practice, and every shipped rate is positive - a clone
always retires.

**Depth is the cadence against the lifetime**, not a property of the opcode.
`vozz`'s clones outlive their spacing by three frames, so its trail is two
copies deep; every other scene's rate is less than half `vozz`'s and its
trails run five or six deep. Replaying all ninety-four through the port's
own pool seats all ninety-four, so none of those depths is a pool clamp.

Two shorthands to retire. `vozz`'s first burst is **four** clones, not three:
it issues the ramp's foot twice (`0x32,0x28,0x1E` at `+0x0A2E` and `+0x0A39`)
before stepping, so "three, stepping the word" describes the ramp and not the
burst. And only two of its five bursts run at eight frames - the other three
run at six. The full shape of `vozz` P2[13] is one burst of four at eight
frames, then three, six, three, eight, three, six and three, six.

The **source id is per burst, not per scene**: `nilboa`'s P2 record switches
from `0x25` to `0x23` partway through, so a reader (or a test fixture) that
resolves one cross-context actor for a whole record silently loses every
clone the rest of the record asks for - the arm's `beqz s5` skips the helper
for an id nothing resolves, and the instruction still advances its eight
bytes.

#### What the `0x4C 0xD8` spawner builds

`FUN_801D77F4` ([`functions/renderer.md`](../reference/functions/renderer.md#801d77f4)
decodes the routine) is not a generic allocator: it allocates from the
**morph-weight descriptor** `0x8007068C`, whose `+0x8` handler word is `0x8002174C`
(`legaia_engine_core::morph_weight_apply`), so every actor this opcode spawns
is a mesh-morph actor. Its tail (`0x801D7848..0x801D79BC`, PROT 0897 file
`+0x8FDC`) wires four fields from the instruction's own operands:

| Actor field | Filled from |
|---|---|
| `+0x4C` morph block | a **VDF** body: the VDF buffer at `0x8007B7DC`, indexed by operand 1 as `base + u32_at(base + 4 + idx*4)`, opening with its own `u32` record count |
| `+0x48` TMD base | the resident-object table `0x8007C018` at operand 2 **plus the scene-bank base** `*(u16*)0x8007B6F8` - see [below](#the-model-operand-is-a-scene-bank-index) |
| `+0x90` rest pose | a **snapshot**, not an asset - see below |
| `+0x3C` / `+0x3E` | the two `u16` immediates, verbatim |

The rest pose is built, not loaded: the spawner sums `n_vert` over the
block's records through the object table's `0x1C` stride, allocates `sum * 8`
bytes via `FUN_80017888`, and copies each named group's live vertices into it
with `lwl`/`lwr` + `swl`/`swr` pairs.

The `+0x3C` / `+0x3E` row is the correction the field names hide. `FUN_8002174C`
reads them at `0x80021890` and `0x800218B4`, on the `+0x40` direction gate, as
the two per-frame steps of the morph weight envelope it drives at `+0x6E` - its
**rise and fall rates**, so
for this opcode the port's `kind` / `variant` are ramp speeds and carry no
actor-class meaning. `+0x56` (render mode), `+0x68` and `+0x6E` (live weight)
are all zeroed, so a freshly spawned morph actor starts at rest. Because the
rest pose is snapshotted at spawn, nothing on the disc carries one - which is
why a search for a rest-pose asset finds nothing.

#### The model operand is a scene-bank index

The operand is not a raw pool slot. The `0x4C` arm reads it and adds the
scene-bank base before the call - `lhu s0,-0x4908(v1)` (`0x8007B6F8`) then
`addu s0,s0,v0` at `0x801E2DE0..0x801E2DE8` - and `FUN_801D77F4` reads
`DAT_8007C018[slot]` with no further adjustment (`0x801D7854..0x801D7878`).
With the base at `5` (the five player meshes ahead of the scene's own, see
`engine-core::model_bank`), operand `n` names the scene's `n`th registered
model.

The disc agrees structurally. Every shipped morph block is one record
`[group][first_vertex][delta_count]`, and on all seventeen sites
`first_vertex + delta_count` equals the vertex count of object `0` of scene
model `n` exactly - `balden`'s operands `99` / `100` / `109` / `110` included,
through `balden2`'s count-`5` MAN-less table that the strict bundle detector
does not accept. Read as raw pool slots against the battle effect-model
library the engine keeps resident, `balden`'s operands resolve nothing and
`jagaroom` / `garmel` bind effect models whose vertex counts disagree on seven
of their eight sites; that reading is what left two carriers unseated.

#### Three record pitches over one morph block

Retail walks the `+0x4C` block three times, and the three loops disagree about
where record `n + 1` begins:

| Loop | Stride | Where |
|---|---|---|
| the spawner's size sum | `0xC` - the record header | `0x801D78D0..0x801D7900` |
| the spawner's rest-pose copy | `n_vert * 8` | `0x801D792C..0x801D799C` |
| the apply pass `FUN_8002174C`, both halves | `n_vert * 0x60` | `0x800217B4`, `0x80021860` |

None of the three can be inferred from the others, and only a block of a
**single** record makes them agree, because then no stride is ever consumed.
That is every block the disc ships: all seventeen sites below resolve a VDF
block whose leading count word is `1`, each naming TMD object `0`
(`morph_weight_disc_blocks_are_single_record` in
`crates/engine-core/tests/field_actor_spawn_disc_e2e.rs`). So the pitch
disagreement is real in the instruction stream and unobservable in the shipped
game; the port reproduces each loop at its own stride and pins the census, so a
block with two records reads as new territory rather than as covered ground.

The engine seats the whole chain on all five carriers:
`World::spawn_morph_weight_actor` resolves the model through
`World::field_pool_tmd` (the scene bank `SceneHost::enter_field_scene`
installs from its `SceneModelBank`), builds the
snapshot and stamps the `+0x0C` handler, `World::tick_handler_actors` steps the
`+0x3C`/`+0x3E`/`+0x40`/`+0x6E` envelope once per game tick, and both hosts
read the blended mesh back through `World::morph_weight_posed_tmd`.

#### Where `0x4C 0xD8` occurs on the disc

Retail uses the synchronous spawn sparingly and in one structural position. Disc-wide there are **17 sites in 5 scenes** - `balden` (4), `balden2` (4), `garmel` (2), `jagaroom` (6), `juui2` (1) - and every one of them sits in **partition 1 record 0** of a scene MAN, the scene-entry system script `Scene::field_man_entry_script` resolves. No per-actor interaction script and no cutscene-timeline record uses it. Within a scene the sites chain contiguously at the 9-byte stride, walking successive `vdf_idx` values.

Two facts about that census are worth keeping, because both were once read wrong:

- **The carrier is the MAN.** `0x4C` is a field-VM opcode, so its bytecode lives in the scene MAN (bundle MAN or streaming variant carrier), never in the `scene_event_scripts` entries - those carry move-VM prescripts. A census over event-script entries is aimed at the wrong carrier and reports zero. It once reported non-zero only because a one-sector prescript entry read under the superseded declared-span PROT size ran past itself into the neighbouring bundle, so the bundle MAN's opcodes were being filed under the prescript's name (see [`prot.md`](../formats/prot.md)).
- **`balden` and `balden2` are two carriers, not one seen twice.** Their clusters are byte-identical and sit at the same record offsets, but they are different PROT entries (183, the `balden` bundle MAN; 320, the `balden2` streaming variant) with different payloads. No two MAN carriers on the disc share bytes, which is what makes a per-scene census a partition of the corpus rather than a double count.

The census lives in `crates/engine-core/tests/field_actor_spawn_disc_e2e.rs`, with `examples/scan_4c_d8.rs` as the standalone form. Both take sites at decoded instruction boundaries and cross-check against a walker-independent raw byte-pair scan: on this opcode the two agree exactly, carrier by carrier, so neither an operand alias (which would inflate the byte scan) nor a decode desync (which would deflate the walk) is in play.

`SceneHost::tick` runs the materializer every frame with `start_slot = FIELD_SPAWN_START_SLOT` (defined in `engine_core::world`; currently `8`, brackets the party + small scripted-NPC reservation). Engines that drive `SceneHost::tick` (the `legaia-engine` binary's `play` / `play-window`, every engine-core integration test that ticks through a scene) get the queue drained automatically. The asset-viewer's `tick_field_frame` does the same materializer pass between `step_field` and the field-event histogram so the `ActorSpawned` / `ActorSpawnFailed` events surface on the HUD next to the `ActorAllocate` event that produced them. The bare `World::materialize_actor_spawns` is still public for tests and engines that want a custom `start_slot` policy.

### 0x4C nibble-2 - the camera-octant / pad-rotation setter

Outer nibble 2 has no sub-table: the outer table `0x801CEE60[2]` points straight
at one arm at `0x801E0EB8` (field overlay 0897), and every `0x20..0x2F` runs it.
The label "party-view-swap" was a guess at the theme; the arm's own operands
say what it is.

The arm writes the **pad-rotation octant** `gp+0x2D8` (absolute `0x8007B5F0`;
`gp = 0x8007B318`, set at `0x80026CA8`/`0x80026CAC`):

```text
801e0eb8  lui   v1, 0x8008
801e0ebc  lw    v0, -0x4a10(v1)   ; old octant
801e0ec0  andi  a1, s3, 7         ; new octant = sub_op & 7
801e0ec4  beq   a1, v0, <exit>    ; unchanged -> nothing to do
801e0ed0  sw    a1, -0x4a10(v1)   ; gp+0x2D8 = sub_op & 7
```

The value is the rotation amount the pad remapper `func_0x800467E8` applies -
it re-emits `ring[(index + gp[0x2D8]) & 7]` over the 8-entry compass ring
`DAT_800766FC`, so raising it turns "screen up" by that many eighth-turns.

The rest of the arm keeps the party **facing the same way on screen** across
the change, and only in one scene mode:

```text
801e0ed4  lw    v1, -0x4950(v0)   ; 0x8007B6B0
801e0ed8  addiu v0, zero, -0x3e8  ; -1000
801e0edc  bne   v1, v0, <exit>    ; only when 0x8007B6B0 == -1000
801e0ee4  lw    a0, -0x3c9c(v0)   ; 0x8007C364 = player actor
801e0ee8  subu  v0, a1, s7        ; delta = new - old
801e0ef0  sll   v0, v0, 9         ; delta * 0x200
801e0efc  sh    v1, 0x26(a0)      ; actor[+0x26] += delta * 0x200
```

`0x200` is an eighth of the `0x1000` full turn the actor's `+0x26` yaw uses, so
the actor is counter-rotated by exactly the octant the pad gained.

**The octant is scene-authored, not camera-derived.** Nothing anywhere computes
it from a camera azimuth. Its complete write set disc-wide is six stores, all
in the field overlay: this arm's `sw` at `0x801E0ED0`, a `sw zero` clear at
`0x801E5664`, the tile-board walker's delay-slot clear and two banded stores at
`0x801EF8B0` / `0x801EF8B8` / `0x801EF8CC` (see
[tile-board.md](tile-board.md#the-walkers-octant-store)), and the walker's
restore at `0x801EFE7C`.

The read set is four, and two of them are the point: the **pad remapper**
`func_0x800467E8` loads the word twice in `SCUS_942.54`, at `0x800467E8` and
`0x80046840`, which is what makes the octant a rotation at all. The other two
are this arm's own compare at `0x801E0EBC` and the walker's save at
`0x801EF320`. (An earlier sentence here listed only the two field-overlay
reads, which reads as though the value never leaves the overlay that writes
it; it is the SCUS remapper that consumes it. Measured with
`find-gp-relative-refs.py --va 0x8007b5f0` over `SCUS_942.54` and every based
overlay image - the absolute-word scan is blind to both forms here.) A port
that derives the octant from its camera is making a port decision, and should
say so.

### 0x4C nibble-0x50..0x5F - the five sub-op table

Nibble 5 is a small dispatcher and it is worth writing out, because the table
bounds it: the arm at `0x801E1780` runs `andi v1, s3, 0xf; sltiu v0, v1, 0x5`
against a **5-entry** jump table at VA `0x801CEF30` (field overlay 0897, file
`+0x718`). Sub-ops `5..0xF` therefore have no arm at all and halt at PC.

| Sub | Arm | Instruction | What it does |
|---|---|---|---|
| 0 | `0x801E17AC` | `[4C, 50, lo, hi]` | Actor model select; `>= 0xF0` sets ctx flag `0x01000000`. |
| 1 | `0x801E1828` | `[4C, 51, x, z, depth, move_id]` | NPC / player move-to-tile with run dispatch. |
| 2 | `0x801E1ABC` | `[4C, 52, item_id]` | **TAKE_ITEM** - see below. |
| 3 | `0x801E1AF8` | `[4C, 53]` | Dialog-wait poll, `FUN_801D65D8(1)`. |
| 4 | `0x801E1B0C` | `[4C, 54]` | Dialog-advance poll, `FUN_801D65D8(0)`. |

#### Sub-2 is TAKE_ITEM, not a menu poll

The arm is the give-side mirror of op `0x39` `GIVE_ITEM`: same inventory
active-window setup (`FUN_8004313C`), then the **consume** primitive
`FUN_80042310(item_id, 1)` instead of the adder `FUN_800421D4`. When the
consume returns the `0x100` not-found sentinel - the bag does not hold the id -
the arm falls through to `FUN_800430AC(item_id)`, the party accessory
unequip-by-id (`legaia_asset` side: [`equipment-table.md`](../formats/equipment-table.md);
engine side `engine-core::equipment::party_unequip_accessory_by_id`). So a
script that takes away an item the player is *wearing* still removes it.

Two properties an earlier reading of this arm inverted, both of them the
decompiler artifacts catalogued in [`ghidra.md`](../tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims):

- **`0x100` is the miss sentinel**, so the `== 0x100` branch is the fallback,
  not a success signal. Reading it as "activation finished" turns the unequip
  into a completion callback.
- **Both arms advance the PC by 3.** The `addiu s8, s8, 0x3` rides the
  `jal 0x800430AC` branch-delay slot, and the `bne` that skips that `jal`
  targets `0x801E00B8`, the shared advancing exit. There is no poll and no
  halt - which is exactly the `switchD_801e00f4::default()` trap
  [`script-vm.md`](script-vm.md#intra-function-label-catalogue) warns about.

The instruction is live disc data, not a curiosity:
`crates/engine-core/examples/scan_4c_n5.rs` decodes **63** sites (34 with a
clean walker run-up) across 13 scenes - `balden`, `balden2`, `deroa`, `geremi`,
`korb3`, `nilboa`, `ropeway`, `ropeway2`, `station3`, `town0c`, `vozz` among
them - so an arm that halts is a permanent field-VM stall on shipped scripts.

### 0x4C nibble-4 - immediate-or-ramp cluster

The unified 6-byte "write or ramp a slot" pattern: `[4C, op0, val_lo, val_hi, ticks_lo, ticks_hi]`. The inner sub-op = `op0 & 0x0F` selects the slot.

When `ticks == 0` the value is written directly to the slot; when `ticks != 0` the original schedules a `func_0x8003C5F0` ramp from the current value to `val` over `ticks` frames.

| Sub | Slot | Notes |
|---|---|---|
| 0 | `ctx[+0x72]` | Plain s16 write or ramp. |
| 1 | `ctx[+0x6A]` | Input is `(value >> 1).max(1)` (signed halve, floor 1). |
| 2 | `ctx[+0x8E]` | When ramp == 0 and `flags & 0x20000000`, also writes `world_y = -value`. |
| 3 | `ctx[+0x24]` *or* abs-jump | If `ticks == 0`, returns absolute PC = `s16(operand+1..3)`. Otherwise ramps `+0x24`. |
| 4 | `ctx[+0x28]` *or* abs-jump | Mirror of sub-3. If `ticks != 0`, returns abs PC. Otherwise immediate write. |
| 5 | `actor[+0x44].{0x9A,0x94,0x96,0x98}` | 11-byte instruction (overrides the 6-byte default). |
| 6 | `_DAT_8007B92C` | Gated by `_DAT_800845A8 == 0`; when set, the gate clears both 6 and 7. |
| 7 | `_DAT_8007B930` | Sister of sub-6. |
| 8 | `ctx[+0x26]` | Plain s16 write or ramp. |
| 9 | `_DAT_801C6EA4 + 0x4A` *or* player-relative *or* delta-bank | Branched on two bits of `_DAT_1F800394`. |
| A | `_DAT_8007BCD0` | Plain global write or ramp. |
| B | `_DAT_8007BCD4` | Sister of A. |
| C | `_DAT_8007BCD8` | Sister of A. |
| D | `_DAT_8007B910` | Same shape, value `(input * _DAT_8008457C) >> 12` - a fixed-point fraction of the configured **audio** level, so `0x1000` means 100% ([battle-action.md](battle-action.md#the-_dat_8007b910-ramps-are-an-audio-duck)). |
| E / F | - | Inner switch's `default:` arm prints `"SUB_40_ERROR"` and routes via `switchD_801e00f4::default()` - halts at PC. |

Sub-9's tristate dispatch:

| Bit `0x02000000` | Bit `0x01000000` | Path |
|---|---|---|
| clear | clear | `Default` - write/ramp `_DAT_801C6EA4 + 0x4A` |
| clear | set | `PlayerRelative` - write/ramp `value + player_anchor[+0x16]` into `+0x4A` |
| set | (ignored) | `Delta` - write/ramp both target slot **and** delta global at `_DAT_8007BCAC` |

**Sub-9 never jumps in the cutscene-dialogue overlay.** Its case 9 (`overlay_cutscene_dialogue_801de840.txt`, around the `_DAT_1f800394 & 0x1000000` test) selects a **write variant** and always advances 6 bytes; the bit-24 arm is the player-relative write above. The absolute-jump arm read from the field-overlay-0897 dump does not apply to the New-Game opening path (live probe: `opurud`'s entry script reaches its op-`0x44` at `+0x7A` with bit 24 set, unreachable under a jump arm). Engine: `legaia_engine_vm::field::Sub9State::PlayerRelative` replaces the earlier `AbsJump`.

### 0x4C nibble-D sub-4 / sub-5 - VRAM STP-bit set/clear

6-byte `[4C, 0xD4|0xD5, x_lo, x_hi, y_lo, y_hi]`. The operand is a `(vram_x, vram_y)` pair; the rect is hard-coded to `w = 0x10, h = 1`. The original (overlay dump lines 7621-7666) runs the PsyQ libgs sequence

```c
DrawSync(0);
StoreImage(rect, buf16);   // FUN_8005842c - read 16 u16 pixels from VRAM
DrawSync(0);
for (i = 0; i < 16; i++) {
    if (sub_4) {           // op 0xD4: set STP on non-zero pixels
        if (buf16[i] != 0)      buf16[i] |= 0x8000;
    } else {               // op 0xD5: clear STP unless already STP-only
        if (buf16[i] != 0x8000) buf16[i] &= 0x7FFF;
    }
}
LoadImage(rect, buf16);    // FUN_800583c8 - write 16 u16 pixels back
return iVar47 + 6;
```

`FUN_8005842c` / `FUN_800583c8` / `FUN_80058104` carry the string constants `s_StoreImage` / `s_LoadImage` / `s_DrawSync` respectively. The 16-element u16 buffer lives on the dispatcher's stack and is *not* present in the bytecode - it's pixels read from VRAM at runtime. The host hooks `op4c_n_d_sub_4_vram_stp_set(x, y)` / `op4c_n_d_sub_5_vram_stp_clear(x, y)` receive only the rect origin; a from-scratch renderer that maintains its own framebuffer can emulate the read-modify-write itself.

## Nibble 9 is the floor-height ladder, not a fade

The whole `4C 9x` family was filed as a "fade family" because the sub-`0..2`
tick's output was never resolved to its destination. It is
`0x1F800314 + 0x48 + rung * 2` = `0x1F80035C + rung * 2`, and that array is the
scene's 16-entry `i16` floor-elevation ladder - the one `FUN_8003AEB0` fills
from the MAN header, `FUN_80019278` interpolates for ground height, and
`FUN_8003A55C` adds to every placed object's Y
([`field-locomotion.md`](field-locomotion.md#where-the-collision-grid-comes-from)).
Sub-`0xE` writes the same sixteen entries directly, which is what makes the
three sub-ops one family rather than three.

`jou`'s scene-entry script is the clean example. `P1[0]` installs the linear
ramp `i * 0x20` with `4C 9E`, sweeps with `4C 9F`, re-installs a second ramp,
then issues a run of `4C 90 <rung> 50 00 16 00 <phase> 80` over consecutive
rungs - period `0x50`, amplitude `0x16`, and a burst-arm word whose count rises
by ten per rung. That is a travelling wave across the elevation ladder: the
floor of the organic Seru interior undulates. `jou` carries 57 such sites and
`concnow` 34.

Two sub-ops of the three are dead. `FUN_801DA930` seeds its phase from
`+0x6C + 1`, so sub-`1` and sub-`2` land on phases 2 and 3 - and only the
phase-1 arm decrements the tick's outer loop counter, so those phases spin
forever. Nothing ships them: `4C 91` and `4C 92` do not occur as a byte pair
in any of the 101 extractable scene MANs, while `4C 90` occurs 180 times
across 19 scenes.
