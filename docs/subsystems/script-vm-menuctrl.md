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
| 1 | 0x10..0x1F | Complex sub-switch on whole byte (menu sub-dispatcher) |
| 2 | 0x20..0x2F | Party-view-swap |
| 3 | 0x30..0x3F | Sub-3 cluster (input lock, no-op cluster, player-resync chain, party-state-clear, etc.) |
| 4 | 0x40..0x4F | Immediate-or-ramp cluster (write or ramp ctx slots / globals) |
| 5 | 0x50..0x5F | Sound directional / dialog query / NPC movement halt-acquire. Sub-3 (round 18, 2-byte) is `[4C, 0x53]` - dialog-wait poll via `FUN_801D65D8(1)`; halts at PC + 2 always (the original `goto joined_r0x801E28C4` returns halt-style after `param_2 += 2`). Sub-4 (round 18, 2-byte) is `[4C, 0x54]` - dialog-advance poll via `FUN_801D65D8(0)`; halts at PC when dialog still active, else advances PC + 2. Sub-1 (5-byte halt-acquire dialog-position) and sub-2 (2-byte menu activation) remain Pending pending the STATE_RESUME-pair refactor. |
| 6 | 0x60..0x6F | 6-word emitter (`func_0x80058490`) + 16-byte halt-acquire |
| 7 | 0x70..0x7F | **Collision-grid rectangular wall paint** (handler `0x801e1c64`); writes the per-scene walkability grid at `_DAT_1f8003ec + 0x4000`. Full body: [nibble-7 wall paint](#0x4c-nibble-0x700x7f---collision-grid-rectangular-wall-paint). |
| 8 | 0x80..0x8F | Large multi-purpose dispatcher (party-slot full heal, conditional jump on `+0x68`, actor model/anim set, actor-search jumps, …). Full body: [nibble-8 multi-purpose dispatcher](#0x4c-nibble-0x800x8f---large-multi-purpose-dispatcher). |
| 9 | 0x90..0x9F | Fade family (sub-0..2 via `FUN_801DDE34`), 16-word table copy (sub-0xE), callback registration (sub-0xF: `func_0x8003CF40(_DAT_8007C34C, &LAB_801DA930)` then halt at PC). |
| A | 0xA0..0xAF | Conditional jump on flag bit. Sub-0 reads `ctx.flags`, sub-1 reads `ctx.local_flags`, sub-2 reads the global story flag word. Bit SET → take absolute jump from operand[2..4]; bit CLEAR (or sub-3..=0xF) → skip 5 bytes. (The asm dispatches on sub-op first at 0x801e2568, so sub-3..=0xF skip both the per-bank check and the take-jump path.) |
| C | 0xC0..0xCF | Small per-actor / per-scene writes (slot table, sub-tile broadcast, sound trigger, `field_74` XOR). **All 16 sub-ops are now ported.** Full body: [nibble-C small per-actor / per-scene writes](#0x4c-nibble-0xc00xcf---small-per-actor--per-scene-writes). |
| D | 0xD0..0xDF | Party state + inverted-Y mirror cluster (field SE trigger, linked-list lookup gate, synchronous-spawn actor allocator, party-record search). Full body: [nibble-D party state + inverted-Y mirror cluster](#0x4c-nibble-0xd00xdf---party-state--inverted-y-mirror-cluster). |
| E | 0xE0..0xEF | Misc scene writes + emitter helpers (3-way state write, variable-length text balloon, FMV trigger, camera teleport/animate/zoom, XP add). All non-`P` cells in the matrix above are now ported. Full body: [nibble-E misc scene writes + emitter helpers](#0x4c-nibble-0xe00xef---misc-scene-writes--emitter-helpers). |
| F | 0xF0..0xFF | Only `op0 == 0xFF` valid (pass-through); other sub-ops print `"SUB_CMD_0F_ERROR"` |

The full per-sub-op table is in the field-VM dump (`overlay_0897_801de840.txt`). The clean-room port mirrors the dispatcher shape with host hooks per sub-cluster - see [`crates/engine-vm/src/field.rs`](../../crates/engine-vm/src/field.rs). The side-effect-free disassembler (`legaia_asset::field_disasm`) carries the same per-sub widths for **all sixteen outer nibbles** so linear census walks stay in sync (nibble `B` is genuinely undefined in retail - no `case 0xb` exists - and decodes as an error).

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
- **Sub-6** (round 18, 15-byte) is `[4C, 0x86, x..rz, actor_id]` - six 16-bit position+rotation values plus a 1-byte actor selector; host returns whether the actor was found, PC always += 15.
- **Sub-7** (`func_0x8003CF40(_DAT_8007C34C, &LAB_801E5154)`) registers an actor-list callback then halts at PC via the dispatcher default.
- **Sub-9** writes `_DAT_80073F00 = i16(operand[1..3])` and advances by 4 (the dump's "FUN_801E3620 dispatch" was Ghidra mis-rendering an internal `goto code_r0x801e3620` label; see the gotcha note below).
- **Sub-B** (round 18, 5-byte) is a conditional jump: `[4C, 0x8B, type_byte, target_lo, target_hi]` jumps to absolute u16 if any actor of `type_byte` is active, else PC += 5.
- **Sub-D** (round 18, 6-byte) is a tristate per-character actor-search: `[4C, 0x8D, char_idx, marker, target_lo, target_hi]` returns one of [`ActorSearchResult::EmptySlot`](../../crates/engine-vm/src/field.rs) (advance 6), `Found` (jump to u16 at +3..=4), or `NoMatch` (halt).
- **Sub-5/E/F** (5-byte `[4C, op0, p0, p1, p2]`) share the standard halt-acquire idiom: on the predicate ([`FieldHost::field_halt_acquire_predicate`]: `saved_pc != 0` or the target is the player, and not already halted or the scene busy) it writes the target's `+0x94` payload pointer, clears `wait_accum`, sets the halt bit, then **advances the caller past the op** (`iVar24 = 5`, `overlay_0897_801de840.txt:6550` / `overlay_world_map_801de840.txt:7179`); on failure it halts the caller at PC (`LAB_801dee50`). Both operate on the resolved cross-context target - the cutscene timeline uses this to freeze its vignette actors, then pokes them beat by beat.

#### 0x4C nibble 0xC0..0xCF - small per-actor / per-scene writes

Small per-actor / per-scene writes (slot table, sub-tile broadcast, sound trigger, `field_74` XOR). **All 16 sub-ops are now ported.** Sub-0 is a 2-byte move-table cancel via `func_0x800204F8`; the host gates on whether a move is currently active.
- **Sub-1** is a 1-byte trigger-flag record-array reset: walks `_DAT_80073ED8[..count]` (stride `0xB`), tests each record's 16-bit index via [`party_flag_test`](script-vm.md#helper-functions), writes the inverted bit to `record[0]`; PC always += 2.
- **Sub-3** is a 2-byte script-table teleport (resolves `func_0x8003C8F0(field_50, 0)` then writes `world_x/z` via the standard tile-center `b * 0x80 + 0x40` formula).
- **Sub-5/6** are 4-byte conditional-jump pair (jump-if-zero / jump-if-nonzero): both read a 16-bit flag index via [`load_u16_le`](script-vm.md#helper-functions), query the host's trigger-flag bank, and advance PC += 4 in both branches (the original's "joined" tail at `LAB_801E28C4` returns `param_2 + 4` either way).
- **Sub-0xA/0xB/0xC** are the 5-byte slot-table writes `[4C, 0xCN, slot, lo, hi]` on the u16 array at `0x801C6460`: sub-A sets, sub-B adds, sub-C subtracts (B/C substitute the per-frame tick `_DAT_1F800393` when the literal is `0xFFFF`). The read side is op `0x4E` sub-ops 5..8 (`slot = sub - 5`; [script-vm.md](script-vm.md) op table) - together they form script-visible counters/timers (e.g. cave01's interact counter gating the `0x15D` beat-key spawn).
- **Sub-0xF** is a position broadcast: 4-byte `[4C, 0xCF, b1, b2]` resolves each byte to either the actor's world coord (`0xFF`), the tile-center conversion (non-zero), or 0; advances by 4.
- **Sub-9** is a 2-byte global-pair compare gate: PC += 2 unless `_DAT_8007BAB8 != _DAT_8007BA9C`, then halts.

#### 0x4C nibble 0xD0..0xDF - party state + inverted-Y mirror cluster

Party state + inverted-Y mirror cluster.
- **Sub-0** (round 18, 6-byte) is a field SE trigger with a conditional u16 pair: `[4C, 0xD0, a_lo, a_hi, b_lo, b_hi]` decodes both via [`load_u16_le`](script-vm.md#helper-functions); the original gates `func_0x8002B994(a, b)` on three flag globals (`_DAT_8007B874`, `_DAT_800846D0`, `_DAT_800846D4`); PC always += 6.
- **Sub-1** (1-byte) is a linked-list lookup gate via `FUN_8003CF04(_DAT_8007C34C, FUN_801DC0BC)` - host returns `Some(new_pc)` for the `LAB_801E360C` ce9c-jump path or `None` for PC += 4 on miss.
- **Sub-2** (2-byte) calls the channel resolver `func_0x8003C83C` and conditionally spawns a script context, then halts at PC.
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
- **Sub-8** (9-byte) is a synchronous-spawn actor allocator: `[4C, 0xD8, vdf_idx, tmd_lo, tmd_hi, kind_lo, kind_hi, var_lo, var_hi]` decodes to `(vdf_idx: u8, tmd_idx: i16, kind: u16, variant: u16)` and routes through host hook [`FieldHost::op4c_n_d_sub8_call_d77f4`] (overlay-resident `FUN_801D77F4`, see `ghidra/scripts/funcs/overlay_cutscene_dialogue_801d77f4.txt`); host writes `actor[+0x3C] = kind` and `actor[+0x3E] = variant` on the allocated slot. Unlike the queue-based `0x4C 0x80` halt-acquire path, the spawn is synchronous - the host emits `FieldEvent::ActorSpawned` directly, with no intervening `pending_actor_spawns` queueing. PC always += 9.
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

#### 0x4C sub-dispatch coverage matrix

The 0x4C cluster is the longest-tail opcode in the field VM - most outer nibbles fan out into 16 inner sub-ops with their own widths and semantics. The coverage matrix below tracks which sub-ops are fully ported (✓), pending an overlay-helper capture (P), or fall through to the dispatcher's default arm (-). "Default" for outer nibbles 1/5/6 means "halts at PC"; for 0/2/3/4/7/8/9/A/C/D/E/F it means PC advances by `1 + width` per the standard fall-through.

| Outer | 0   | 1   | 2   | 3   | 4   | 5   | 6   | 7   | 8   | 9   | A   | B   | C   | D   | E   | F   |
|-------|-----|-----|-----|-----|-----|-----|-----|-----|-----|-----|-----|-----|-----|-----|-----|-----|
| 0     | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   | ✓   |
| 1     | ✓   | P   | P   | P   | P   | P   | P   | P   | P   | P   | P   | P   | P   | P   | P   | P   |
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

- **`n3 sub-4` / `sub-B` / `sub-C`**: the original at `0x801df208` (in `overlay_0897_801de840.txt`) jumps with delay slot `_addiu s8, s8, 0x2` to `LAB_801df09c switchD_801e00f4::default()` - a 2-byte advance with no side effect (the inline `_DAT_8007b5f0 = uVar31` write is a no-op because `uVar31` was read from the same slot). The Rust port matches: `next_pc = pc + header_size + 1`, no host hook fires.
- **`n3 sub-D`**: routed alongside `sub-8` through [`FieldHost::player_subtile_refresh`], a host hook that distinguishes the two via the inner sub-op byte.
- **`n4 sub-5`**: 11-byte instruction `[4C, 0x45, b1, w94_lo, w94_hi, w96_lo, w96_hi, w98_lo, w98_hi, ticks_lo, ticks_hi]`. The dispatcher splits on `ticks == 0` between [`FieldHost::op4c_n4_sub5_write_immediate`] (direct write) and [`FieldHost::op4c_n4_sub5_ramp`] (STATE_RESUME ramp).
- **`n4 sub-E` / `sub-F`**: no `case` arm in the original inner switch - the `default:` arm prints `"SUB_40_ERROR"` and routes via `switchD_801e00f4::default()`, which for opcode `0x4C` halts at PC. The Rust port returns `StepResult::Halt { final_pc: pc }`.
- **`n8 sub-3`**: 7-byte rectangular tile fill `[4C, 0x83, col_start, row_start, col_end, row_end, value]`. The original at dispatcher lines 6447-6493 walks the inclusive rectangle `[col_start..=col_end] × [row_start..=row_end]`, calling `FUN_801D5630(col, row, ...)` per tile to resolve a tile-record pointer; on hit it writes `tile[+0x3] = 0; tile[+0x2] = value`, and the post-loop trailer writes `_DAT_8007B630 = col_start`. The Rust port surfaces the rectangle through [`FieldHost::op4c_n_8_sub_3_rect_tile_fill`] and lets the engine implement its tile pool.

The STATE_RESUME-entangled cluster (`0x4C n5 sub-1`/`sub-2`, `n6 sub-0x61`, `n8 sub-0`) routes through the standard halt-acquire predicate ([`FieldHost::field_halt_acquire_predicate`] with new `which` tags `0x61` and `0x80`). On predicate success the dispatcher performs the ctx mutation (`saved_pc`, `wait_accum=0`, `flags |= 0x400`), calls the case-specific side-effect hook (`op4c_n5_sub1_npc_run`, `op4c_n5_sub2_menu_activation`, `op4c_n6_sub_61_emitter`, `op4c_n8_sub_0_actor_allocator`), and advances PC; on failure it halts at PC. The n5 cluster doesn't route through the predicate (no halt-acquire in the original): sub-1 is a side-effect-only move-table dispatcher, sub-2 polls the host's menu-activation state.

The n6 sub-`0x61` emitter's retail payload is a **one-shot 16×1 VRAM CLUT-cell write** whose coordinates are the script operands: source `(x, y)` at `+5`/`+7` → libgpu `MoveImage` cell copy, or a flat BGR555 fill of all 16 entries when the source y is zero; destination `(x, y)` at `+9`/`+0xB`. It is the one-shot half of the world-map palette cycling (see [`functions.md` § 801E4C58](../reference/functions.md#801e4c58) and [`world-map.md`](world-map.md) "Ocean animation").

The `n8 sub-0` host hook (`FieldHost::op4c_n8_sub_0_actor_allocator`) receives `(count, tail)`: `count` is the byte at `operand+1` and `tail` is the raw bytecode slice from `operand+2` onward. The host walks `count` variable-length child-actor records out of `tail` using the [`packet_length`](script-vm.md#helper-functions) rule (`FUN_8003CA38`): bytes `<= 0x1E` terminate a record; bytes whose top nibble is `0xC` consume one extra byte. The parent script's PC always advances by 3 regardless of how many records were walked - the records remain embedded in the bytecode buffer and become the spawned actors' own bytecode (retail stores the per-actor bytecode pointer at `actor[+0x90]`). The engine-core implementation (`FieldHostImpl::op4c_n8_sub_0_actor_allocator`) splits the records,
queues each one into `World::pending_actor_spawns`, and emits a `FieldEvent::ActorAllocate { records }` so engines can route them into their own actor pool.

Materializing the queued records into actor slots is a separate engine-side step. [`World::materialize_actor_spawns(start_slot)`] drains `pending_actor_spawns`, allocates the first inactive slot from `actors[start_slot..MAX_ACTORS]`, populates `Actor::spawn_record` with the raw bytecode bytes, and emits one `FieldEvent::ActorSpawned { slot, kind, variant, record }` per allocation. The retail allocator for this opcode (`overlay_world_map_801de840.txt:7080-7123`, case `8 sub-0`) allocates from pool `0x801f28a0` and writes `actor[+0x90]` (bytecode start), `actor[+0x94]` (parent back-pointer) and `actor[+0x54] = 0`; it does **not** write `actor[+0x3C]` (kind) or `actor[+0x3E]` (variant), so the event's `kind = 0` / `variant = 0` match retail - this is a faithful zero, not a placeholder.
The `0x4C 0xD8` path is the one that decodes explicit `(kind, variant)` u16 immediates and routes through `FUN_801D77F4`; the `0x4C 0x80` path is bytecode-only by design. When the slot range is exhausted, a `FieldEvent::ActorSpawnFailed { record }` event surfaces the dropped request instead.

`SceneHost::tick` runs the materializer every frame with `start_slot = FIELD_SPAWN_START_SLOT` (defined in `engine_core::world`; currently `8`, brackets the party + small scripted-NPC reservation). Engines that drive `SceneHost::tick` (the `legaia-engine` binary's `play` / `play-window`, every engine-core integration test that ticks through a scene) get the queue drained automatically. The asset-viewer's `tick_field_frame` does the same materializer pass between `step_field` and the field-event histogram so the `ActorSpawned` / `ActorSpawnFailed` events surface on the HUD next to the `ActorAllocate` event that produced them. The bare `World::materialize_actor_spawns` is still public for tests and engines that want a custom `start_slot` policy.

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
| D | `_DAT_8007B910` | Same shape but value is `(input * _DAT_8008457C) >> 12` (fixed-point scale; host owns the transform). |
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

`FUN_8005842c` / `FUN_800583c8` / `FUN_80058104` carry the string constants `s_StoreImage` / `s_LoadImage` / `s_DrawSync` respectively. The 16-element u16 buffer lives on the dispatcher's stack and is *not* present in the bytecode - it's pixels read from VRAM at runtime. The host hooks `op4c_n_d_sub_4_vram_stp_set(x, y)` / `op4c_n_d_sub_5_vram_stp_clear(x, y)` receive only the rect origin; a clean-room renderer that maintains its own framebuffer can emulate the read-modify-write itself.
