# Field / event script VM

The bytecode interpreter that drives Legaia's overworld scripting - NPC movement, dialog triggers, cutscene sequencing, story-flag manipulation. Lives in PROT entry **`0897_xxx_dat`** (the town/field overlay), at `FUN_801DE840`. ~17.5 KB / 4099 instructions / 357 outgoing calls - the largest function in the corpus.

It has **43 opcodes** (`0x21..0x4F`, with gaps) over a byte stream, plus a
`0x5x`/`0x6x`/`0x7x` default route. Port:
[`legaia_engine_vm::field`](../../crates/engine-vm/src/field.rs). This is the
biggest of Legaia's five runtime VMs - see
[the runtime VM family](move-vm.md#the-runtime-vm-family) for how it relates to the
other four, and don't confuse it with the [move VM](move-vm.md), which it *invokes*
via op `0x22` `EXEC_MOVE`.

> **Why "field/event"?** Each running script has its own context (a struct passed around as `ctx_ptr`); contexts can target the player, NPCs, the camera, or "system" channels. The same VM drives both the per-frame field tick and event/cutscene sequences.

**What catches people out:** the on-disc carrier is the **scene MAN**, not
`scene_event_scripts` - see [the next section](#on-disc-form-the-scene-man---not-scene_event_scripts).
The VM's longest-tail opcode, `0x4C` `MENU_CTRL`, is large enough to have its own
page: [`script-vm-menuctrl.md`](script-vm-menuctrl.md).

The decompiled source is at `ghidra/scripts/funcs/overlay_0897_801de840.txt`. References to `func_0x80xxxxxx` are calls into `SCUS_942.54`; `FUN_801xxxxx` are sister functions inside the 0897 overlay.

## On-disc form: the scene MAN - NOT `scene_event_scripts`

The on-disc carrier for field-VM bytecode is the **scene MAN** sub-asset
(asset type `0x03`, the third descriptor in each scene's asset-table bundle;
see [`formats/man-relocation.md`](../formats/man-relocation.md) and
`legaia_asset::man_section`). `FUN_8003A1E4` walks the MAN's partition-1
actor-placement records, derives each actor's script pointer, installs it at
`actor[+0x90]`, and runs the field VM (`FUN_801DE840`) on the body that begins
`1 + N*2 + 4` bytes into the record (after the `[u8 N][N*2 locals][4-byte
header]` prefix). Partition-1 record 0 is the scene-entry **system script**;
records `1..` are per-actor interaction scripts. The engine mirrors this:
`Scene::field_man_entry_script` → `man_section::ManFile::scene_entry_script` →
`World::load_field_script_at`. These MAN scripts disassemble cleanly as
field-VM (~8% linear-walk error on the retail town MANs).

### Record headers are per-partition; the record index space is flat

Every partition prefixes its script with a **different** header, so the offset
of a record's first opcode (`pc0`) depends on which partition the record is in:

| Partition | Record header | `pc0` |
|---|---|---|
| 0 (objects) | `[u8 n][n*2 SJIS name][u8 attr]` | `1 + 2n + 1` |
| 1 (actor placements) | `[u8 N][N*2 locals][4-byte placement header]` | `1 + 2N + 4` |
| 2 (named / cutscene) | name + three condition blocks (`FUN_8003BDE0`) | parsed |

Reading a partition-0 record with the partition-1 formula starts the walk three
bytes late - mid-op - so it resyncs somewhere arbitrary and silently drops ops.
That is not a decode nicety: partition-0 records are the *door and prop scripts*
(see [`field-locomotion.md`](field-locomotion.md#intra-scene-doorways---the-walk-touch-teleport-family)),
so the mis-start is the difference between a door that works and one that does
not.

The record **index** space, on the other hand, is **flat**: `FUN_8003C8F0(id,
partition_base)` indexes the concatenated `[P0..P1..P2]` record-offset table,
and the consumers that name a record by number pass base `0` - the `.MAP` kind-1
trigger's `record` byte (`FUN_8003A55C`), and the op-`0x4C` nibble-C sub-3
script-table teleport. Op `0x44` SPAWN_RECORD is the exception that proves it:
its operand is also flat, and the dispatcher re-bases it into partition 2
(`- N0 - N1`) itself. Engine: `man_field_scripts::flat_record_span`.

### Placement header: model + animation resolution

The 4-byte header after the locals block is `[model][anim_id][bx][bz]`
(`legaia_asset::man_section::ActorPlacement`). Runtime-pinned against the
town01 field anchor (live actor pool at stride `0xD8`, 53/53 animated actors
byte-consistent):

- **`model < 0xF0`** selects **scene TMD index `model`** - retail registers
  the party/savepoint head into `0x8007C018` slots `0..5` and then the
  scene's TMD list in scene order from slot 5, so the byte is pool slot
  `model + 5`. **`model >= 0xF0`** selects global-pool head slot
  `model - 0xF0` (`0xF0`..`0xF3` = Vahn / Noa / Gala / savepoint).
- **`anim_id`** is installed into the actor's `+0x5C` halfword and names the
  actor's clip: **scene-bundle ANM record index + 1** (`0` = no clip). Every
  live animated actor's `+0x5C` equals its `+0x4C` anim-record pointer's
  bundle index + 1; walkers drift ±1 as their scripts switch clips. Special
  models resolve the id against the **PROT 0874 §1 locomotion bundle**
  instead - the Noa/Gala placements carry ids 9/16 = locomotion records 8/15,
  exactly their standing-idle bank slots ([`formats/anm.md`](../formats/anm.md)).
- Placements parked at world `(16320, 16320)` (tile `(127, 127)` + half-tile
  bits) are conditional spawns the scripts place later.

Disc-gated pin: `engine-core/tests/field_npc_placements_disc.rs`.

> **The `scene_event_scripts` / `scene_v12_table` prescript is a MOVE-VM stager
> table, not field-VM bytecode.** The `[u16 count][u16 offsets[count]]` prescript
> (offset 0, or `+0x800` behind the v12 header) was long assumed to carry
> field-VM scripts because its records open with `0xFFFF 0x0000`. It does not:
> the records are **move-VM (`FUN_80023070`) records in the summon-stager format**
> `[i16 model_sel][u16 flags][move-VM bytecode]` - the `0xFFFF 0x0000` lead is
> `model_sel = -1` (a transform/pivot node) + `flags = 0`, and the `0x0008`
> terminator is move-VM opcode `0x08` (Halt). The runtime chain: the field VM
> itself (`FUN_801DE840`) calls the installer **`FUN_800252EC(id)`**, which
> resolves `record = _DAT_8007b8d0 + offsets[id]` and hands it to the part-stager
> **`FUN_80021B04`** (`actor[+0x48] = record`, `actor[+0x70] = 2` PC, tick fn
> `FUN_80021DF4`); the move VM `FUN_80023070` then runs `record+4` each frame. So
> the prescript is the *per-scene* sibling of the summon stagers (same record
> format, same consumer). See `legaia_asset::scene_event_scripts`
> (`move_stager_records` + module note) and the disc-gated tests
> `scene_event_records_word_aligned_real` + `prescript_move_stager_records_real`
> (78 entries / 1855 records, 100 % valid stager-kind leads). The engine's field
> VM correctly does not run these as field-VM scripts; the genuine per-scene
> field-VM *scripts* are in the scene MAN (`FUN_8003A1E4`), and they are what spawn
> from this stager table (see [`field-locomotion.md`](field-locomotion.md)).

The `asset-viewer field <scene>` subcommand drives this end-to-end -
it loads a scene, finds the event-script entry, ticks the VM frame-by-frame
against record N (default 0), and surfaces the running step-result tally
in the HUD so missing `FieldHost` hooks are visible at a glance.

## Function signature

```c
int FUN_801DE840(int buffer_base, int pc_offset, int ctx_ptr);
```

- `buffer_base` - bytecode buffer base address.
- `pc_offset` - current program counter, byte offset into the buffer. The function returns the new PC offset (caller advances).
- `ctx_ptr` - script execution context (see "Context struct" below).

The VM is **not** a step-and-yield loop - each call executes from `pc_offset` until something forces a return (instruction halt, branch back into the caller, target script done). The host calls back in at the next frame (or when an external event fires) with the returned PC.

## Top-level dispatch

```c
pbVar43 = (byte *)(buffer_base + pc_offset);   // PC pointer
pbVar47 = pbVar43 + 1;                          // operand cursor

// Extended/script-target prefix
if (*pbVar43 & 0x80) {
    // Switch context to script targeted by pbVar43[1]
    if (pbVar43[1] != ctx_ptr[+0x50]) {        // not already the target?
        ctx_ptr = func_0x8003C83C(pbVar43[1]); // resolve by ID
        if (ctx_ptr == 0) {
            // print "UNFIND INDICATION %d" if dev flag set
            return pc_offset + 1;
        }
    }
    // Halt-flag check (with op 0x32 carve-out)
    if (ctx_ptr[+0x10] & 0x400 && /* not opcode 0x32 with bit 0x400 */) {
        return pc_offset;                       // halted, don't dispatch
    }
    pbVar47 = pbVar43 + 2;                      // operands start at +2
    pc_offset += 1;                             // ID byte consumed
}

switch (*pbVar43 & 0x7f) {
    // 43 unique opcodes 0x21-0x4F (with gaps at 0x27-0x2A)
}
```

The high bit (0x80) of an opcode means "this instruction targets a different script context" - `*(pbVar43+1)` is the script ID, resolved through `func_0x8003C83C` to a context pointer. The original (caller's) context is preserved; the dispatch operates on the resolved one.

`func_0x8003C83C` itself special-cases two IDs:
- `id == 0xF8` → returns the cached pointer at `_DAT_8007C364` (one of the standard non-script channels).
- `id == 0xFB` → walks the linked list at `_DAT_8007C34C`, looking for the entry whose `+0xC` slot holds `0x801DA51C` (the system-channel handler in the 0897 town overlay). So `0xFB` is the "system" channel.
- otherwise → ID is a regular script-table index.

A clean-room port exposes this trio as `FieldHost::resolve_ctx(id: u8) -> Option<ScriptCtx>` with the special-case branches preserved.

## Context struct

Per-script state, passed as `ctx_ptr`. Offsets identified so far:

| Offset | Type | Meaning |
|---|---|---|
| +0x10 | u32 | Flag word. Bit 0x400 = "halted". Bit 0x100 has special handling in op 0x31 (and is the "touched" mark `FUN_801D5B5C` sets). Bit 0x1000000 toggles op 0x22 behavior. Bit 0x20200 / 0x20000000 gate the Y-collision lookup in op 0x23. Bits 0/1 = **collision/touch exempt** (`FUN_801CF754` / `FUN_801CF9F4` skip `flags & 3` actors - a door's touch pass runs `31 00` as its swing starts). Bits 0x20000 / 0x40000000 = the `FUN_801CFC40` contact class (result bit 1, button-gated - a cupboard's spawn prologue runs `31 1E`); 0x20000 / 0x1000000 also select the moving-arm contact box. See [`field-locomotion.md`](field-locomotion.md#collision---fun_801cfe4c). |
| +0x14 | u16 | World X (in 0.5-tile units, formula `(b & 0x7F) * 0x80 + 0x40`). |
| +0x16 | u16 | World Y (computed from collision via `func_0x80019278`). |
| +0x18 | u16 | World Z. |
| +0x26 | u16 | Source value copied to `+0x5A` by op 0x31 bit-8 path. |
| +0x42 | u16 | Generic per-actor scalar slot. Written by op 0x4C nibble-C sub-2. |
| +0x50 | u16 | Script ID. `0xFB` = "system" channel. |
| +0x54 | u16 | Wait/timer accumulator. Cleared by YIELD; ticked by WAIT_FRAMES. |
| +0x56 | u16 | Move-table sub-state (op 0x22 sets to 5 if move==0, else 1, on non-player). |
| +0x58 | u16 | Generic per-actor scalar slot. Written by op 0x4C nibble-D sub-D. |
| +0x5A | u16 | Saved counterpart of `+0x26` (op 0x31 bit-8 path). |
| +0x5C | u16/i16 | Move-table index (op 0x22). |
| +0x5E | u16 | Set to 0xFFFE by op 0x22. |
| +0x62 | u16 | Local flag bank (16 bits). Manipulated by ops 0x2B / 0x2C / 0x2D. |
| +0x68 | i16 | Local guard slot. Read by op 0x4C nibble-8 sub-C to skip a forward jump when zero. |
| +0x6A | i16 | Generic scalar. Written / ramped by op 0x4C nibble-4 sub-1 (input is `(value >> 1).max(1)`). |
| +0x6D | u8 | Face/body rotation index (op 0x43 sub-7). |
| +0x72 | u16 | Generic scalar. Written / ramped by op 0x4C nibble-4 sub-0. |
| +0x74 | u32 | Composite control word. XOR-toggled by op 0x4C nibble-C sub-8 (flips bit 0x10000000). |
| +0x8B | u8 | Cleared by op 0x23 NPC path. |
| +0x8C | u8 | NPC X grid coord (op 0x23). |
| +0x8D | u8 | NPC facing (op 0x23). |
| +0x8E | i16 | Inverted-Y mirror slot. Written / ramped by op 0x4C nibble-4 sub-2 (which also conditionally writes `world_y = -value` when `flags & 0x20000000` is set). |
| +0x90 | u32 | Opaque actor-handle field. |
| +0x94 | u32 | Saved PC (set by YIELD; the dispatcher reads this on resume). |

`_DAT_8007C364` is the **player context pointer** - many opcodes branch on `ctx_ptr == _DAT_8007C364` to switch behavior. `_DAT_801C6EA4` is the current world/scene pointer.

## Opcode reference

### Shared NOP cluster

| Op | Encoding | Effect |
|---|---|---|
| 0x21 / 0x24 / 0x25 / 0x48 | 1 byte | PC += 1. Four distinct opcode bytes share one handler - likely reserved/historical. |

### 0x22-0x26 (action / control flow)

| Op | Mnemonic | Encoding | Effect |
|---|---|---|---|
| 0x22 | `EXEC_MOVE` | `[22, move_id]` | Schedule move-table playback on the current ctx. Sets `ctx[+0x5C] = move_id`, `ctx[+0x5E] = 0xFFFE`, then calls `func_0x800204F8(ctx)` - the **move-table consumer** that [`crates/mdt`](../formats/mdt.md) targets. Player path has special cases around `+0x10` bit 0x1000000 (move chaining) and `move_id == 99` (auto-cancel). |
| 0x23 | `MOVE_TO` | `[23, x_byte, z_byte]` | Teleport ctx to grid position. World coords: `(b & 0x7F) * 0x80 + 0x40`, plus 0x40 if high bit set. Player path also calls `func_0x80017EC8` (camera/scroll). NPC path sets `+0x8C/+0x8D` facing, calls `FUN_801D81E0` and `FUN_801D79E8` (movement init). PC += 3. |
| 0x26 | `JMP_REL` | `[26, lo, hi]` | Relative jump: `PC = pc_offset + 1 + (lo + hi*0x100)`. Unconditional. |

### 0x2B-0x33 (flag manipulation triplets)

The cleanest group - three separate 1-bit-flag banks each with set / clear / test+skip:

| Op | Mnemonic | Encoding | Effect |
|---|---|---|---|
| 0x2B | `LFLAG_SET` | `[2B, bit]` | `ctx[+0x62] \|= 1 << (bit & 0x1F)`. Per-script local flag bank (16 bits). |
| 0x2C | `LFLAG_CLR` | `[2C, bit]` | `ctx[+0x62] &= ~(1 << (bit & 0x1F))`. |
| 0x2D | `LFLAG_TST` | `[2D, bit]` | If `ctx[+0x62] & (1 << bit)` is 0, return `pc_offset` (halt); else continue. |
| 0x2E | `GFLAG_SET` | `[2E, bit]` | `_DAT_1F800394 \|= 1 << (bit & 0x1F)`. Global story flag bank (32 bits, in PSX scratchpad). |
| 0x2F | `GFLAG_CLR` | `[2F, bit]` | `_DAT_1F800394 &= ~(1 << (bit & 0x1F))`. |
| 0x30 | `GFLAG_TST` | `[30, bit]` | If `_DAT_1F800394 & (1 << bit)` is 0, return; else continue. |
| 0x31 | `CFLAG_SET` | `[31, bit]` | `ctx[+0x10] \|= 1 << (bit & 0x1F)`. Ctx flag word (32 bits). **Bit 8 special case**: copies `ctx[+0x26] → ctx[+0x5A]`, returns immediately. |
| 0x32 | `CFLAG_CLR` | `[32, bit]` | `ctx[+0x10] &= ~(1 << (bit & 0x1F))`. The bit-0x400 form is the only opcode that bypasses the dispatch-prelude halt check on extended opcodes. |
| 0x33 | `CFLAG_TST` | `[33, bit]` | If `ctx[+0x10] & (1 << bit)` is 0, return; else continue. |

The three banks line up with conventional event-script roles:
- **`ctx[+0x62]`** - per-script local flags (sub-routine state, conditional dialog branches).
- **`_DAT_1F800394`** - global story flags (persistent across script runs; PSX scratchpad means cheap to access).
- **`ctx[+0x10]`** - script context flags (halt state, move-chain state, render-gate state).

### 0x34-0x36 (effects, music, scene transitions)

These are sub-dispatchers - the operand byte selects a sub-command.

#### 0x34 EFFECT (nibble-dispatched)

`op0 >> 4` selects sub-op:

| Sub | Encoding | Effect |
|---|---|---|
| 0 | `[34, op0, r, g, b, intensity_lo, intensity_hi]` (7 bytes) | Effect-global colour + intensity setup. Rewrites `_DAT_8007BCCC..BCE0` colour-mode globals. Fade pipeline gated on `_DAT_1F800394 & 0x800000`. |
| 1 | base 13 bytes; +2+payload when peek-at-`pc+13` byte is 0x40 | Effect / sprite spawn with optional captured-PC. Walks actor list at `_DAT_8007C354`; if found, skips spawn. Otherwise calls `FUN_801E5668(ctx, ..., pos, packed24, mode)`; `mode = 1 + (op0 & 1)`. When `capture_flag == 0x40`, captures payload bytes onto the spawned actor's `+0x94`. |
| 2 | 3 bytes | Actor-pool capture-and-yield. Walks list looking for entry whose `+0x90 == ctx`; if found AND `b1 == 0x40`, captures forward-PC and emits `caseD_4` (STATE_RESUME → Yield). |
| 3 | 4 bytes | Play 3D animation via `func_0x800252EC(operand1+1, ctx+0x14, ctx+0x24)`. Looks up an offset in the buffer at `_DAT_8007B8D0` (= the `bse.dat` master bank) using `*(u16*)(buf + 2 + idx*2)`, then spawns an actor via `FUN_80021B04(pos, ?, buf+ofs, 0x1000)`. Buffer layout matches the [ANM container shape](../formats/anm.md). |
| 4..=15 | - | No `case` arm in `FUN_801de840`; falls through `if (bVar35 != 2) { if (bVar35 != 3) { return param_2; } }` - halts at PC. |

#### 0x35 BGM

`[35, lo, hi, sub]`. Operand 2 selects sub-op:

| Sub | Effect |
|---|---|
| 1 | Start field BGM - sets `_DAT_8007BAC8 = signed16(operand0, operand1)` then debug-prints `"Field BGM %d"`. The BGM-id-to-PROT mapping is asynchronous in `FUN_800243F0` (see [BGM lookup](#bgm-lookup-table)). |
| 2 | Pause (`func_0x800266E0(0x8007052C)`). |
| 3 | Resume (`func_0x80026740`). |
| 4 | Stop (`func_0x80026478`). |
| 5 | Set volume. |
| 6 | Flag set. |
| 7 | Target-sound-set (`_DAT_8007B880`). |
| 8 | `func_0x80019898`. |
| 9 | Queue. |
| 10 | Unhalt-pause toggle. |
| 11 | `_DAT_8007BA9C = -1`. |

PC += 4.

#### 0x36 SCENE_FADE

`[36, lo0, hi0, lo1, hi1]`. Reads two 16-bit operands. `0xFFFF` = wait for `_DAT_8007BC20` load flag. Bit-15-clear values do `func_0x8003D53C` fade or `func_0x80019794` lookup. Bit-15-set sub-cases 0..4 dispatch various transitions including `FUN_801D8450`.

### 0x37-0x42 (yield, sound, RPG state, dialog, jump)

| Op | Mnemonic | Encoding | Effect |
|---|---|---|---|
| 0x37 / 0x41 / 0x47 | `YIELD` family (**motion ops**) | `[op, b0, b1]` (resume pc+3); `[47, xb, zb, b2]` (pc+4) | Park the script and hand the op to the walk kernel. [Detail](#0x37--0x41--0x47-yield-family-motion-ops). |
| 0x38 | `CAM_CFG` | `[38, op0, op1]` | Camera/visual register write. If `op1 & 0x7F == 0`: simple path - copy `*(short*)(0x80073F04 + (op0 & 0xF) * 2)` into `ctx[+0x26]`. Else: halt-acquire path - same predicate as op 0x43 sub-0/1/A/B (`saved_pc != 0 \|\| ctx==player`) AND (`!(flags & 0x400) \|\| scene_busy`); on success set HALT + saved_pc + wait_accum=0 (mirror to caller when ctx is player), yield with `resume_pc = pc + 3`; on fail fall through to dispatcher default. |
| 0x39 | `GIVE_ITEM` | `[39, item_id]` | Adds one inline item `item_id` to the inventory; the **treasure-chest item-give** path (the granted item is this single operand byte, **not** a per-scene table). Full behaviour in [§ 0x39 GIVE_ITEM](#0x39-give_item) below. |
| 0x3A | `ADD_MONEY` | `[3A, b0, b1, b2]` | 24-bit signed delta: `_DAT_8008459C += sext24(operand)`. Clamp to `[0, 9999999]`. |
| 0x3B | `SET_ITEM_COUNT` | `[3B, slot, count]` | Set inventory entry: `*(byte*)(0x80084340 + (slot & 0xF) + (slot >> 4) * 0x414) = count`, then `func_0x80042558()` to refresh inventory display. Inventory pages of 0x414 bytes. |
| 0x3C | `PARTY_ADD` | `[3C, char_id]` | Add character to party (sorted insertion into `_DAT_80084598..` array, count at `DAT_80084594`). Caps at 4 members. Updates `_DAT_8007B8F8` (party leader) when count was 0. Calls `FUN_801DE190()` (refresh display). Special: if count becomes 2 with `_DAT_80084598 == 0x100`, calls `func_0x800423E0()` and returns. |
| 0x3D | `PARTY_REMOVE` | `[3D, char_id]` | Remove character (linear search, shift, count--). Updates leader if affected. Refresh via `FUN_801DE190()`. |
| 0x3E | `WARP / INTERACT` | `[3E, op0, op1, …]` | If `op0 == 0xFF` or `op0 < 100`: trigger field interact at index `op1` on system context (`func_0x8003C83C(0xFB)`); writes `sys_ctx[+0x94] = scene_data + op1 * stride + 1`, calls `func_0x8003CE08(0xE)`. Else (`op0 >= 100`): **minigame door-warp** - `_DAT_8007BA34 = op0 - 100` (sub-id), `_DAT_8007B83C = 0x18` (mode 24 OTHER INIT), zero the session-winnings accumulator `_DAT_80084440` and `0x8007BAC0`, clear `player[+0x10] & 0x80000`, call `func_0x8003CE08(0xE)`. The op carries **no destination name**; full pre-warp/return behaviour in [§ 0x3E WARP](#0x3e-warp-mode-24-minigame-door-warp) below. |
| 0x3F | `SCENE_CHANGE` (named warp) | `[3F, idx_lo, idx_hi, name_len, [name_len name bytes], entry_x, entry_z, dir]` | **Named scene-change ("warp by name"), NOT a dialog op.** Full encoding + behaviour in [§ 0x3F SCENE_CHANGE](#0x3f-scene_change-named-warp) below. |
| 0x40 | `DATA_BLOCK` | `[40, len, ...len bytes]` | Skips `len` bytes after header - embeds raw inline data. PC += 2 + len. |
| 0x42 | `COND_JMP` | `[42, mode, op1, op2, op3]` | Multi-mode conditional. `mode == 0`: test `_DAT_8007B8F4 & (1 << (op1 & 0x1F))` - if clear, return `pc + 5` (skip). `mode == 1`: test screen-mode (`_DAT_8007B850`) against `_DAT_801F28D0[op1*4]` (8-entry table) for `op1 < 8`, bit 0x20 for `op1 == 8`, 0x40 for 9, 0x80 for 10, 0x10 for 11; **`op1 >= 0xC` falls through to the unconditional take-jump path** (no test). `mode >= 2` hits the dispatcher's default arm - halts at PC. Successful jump target = `pc + 3 + LE_u16(op2,op3)`; skip target = `pc + 5`. |

#### 0x37 / 0x41 / 0x47 YIELD family (motion ops)

All three park the running script: they save the op's own PC into `ctx[+0x94]`,
clear the cursor `ctx[+0x54]`, and set `ctx[+0x10]` bit 0x400 (HALT). A player
context propagates the halt to the caller.

The parked op is then interpreted **in place, each frame, by the walk kernel
`FUN_8003774C`** (which resolves the `0x80` ext-target convention). The operand
decode differs per op:

- **0x37 / 0x41** - glide-step. Axis `DAT_80073F14[b0 & 7]`, base step `4 << ((b0>>5 & 4) | (b1>>6))`, distance `(b1 & 0x3F) * base`.
- **0x47** - walk-to-tile. Base step `4 << (b2 & 7)`, approach mode `b2 >> 4`.

**No "synthesised motion bytecode" exists** - the record bytes *are* the stream the
walk kernel reads. See [`motion-vm.md`](motion-vm.md).

#### 0x39 GIVE_ITEM

`[39, item_id]` - adds one of inline item `item_id` to the inventory: `func_0x8004313C()` (select the active inventory window/page bounds) then `func_0x800421D4(item_id, 1)` (the capacity-checked add-item-by-id primitive). PC advances by 2 (`addiu s8,s8,0x2` at `0x801E044C`; `lbu a0,0(s6)` reads the inline id at `0x801E0450`). This is the **treasure-chest item-give** path - the **granted** item is this single inline operand byte, **not** a per-scene table. `FUN_800421D4` is the inventory adder (see [`functions.md`](../reference/functions.md)), so the earlier `PLAY_SFX` / `func_0x800421D4(sfx_id, 1)` label was wrong. (The standalone `FUN_801D71F0` add-item copy has zero callers - dead/duplicate;
the live give-item is inlined in the dispatcher here.) NB the chest's announcement *text* ("There is a {item}…") names the item from a **separate** `0xC2 <id>` MES item-name token (display only), distinct from this give operand - editing one without the other makes the on-screen message disagree with what lands in the bag (see [randomizer.md](../tooling/randomizer.md)).

#### 0x3F SCENE_CHANGE (named warp)

`[3F, idx_lo, idx_hi, name_len, [name_len name bytes], entry_x, entry_z, dir]` - **Named scene-change ("warp by name"), NOT a dialog op.**

- Copies the `name_len`-byte destination scene NAME from operand+3 into a local buffer (null-terminated) and calls `func_0x8001FD44(name, idx)` - the **scene-change packet** (writes the name into the active scene-name buffers `0x8007050C` / `0x80084548`; sets the transition flag `_DAT_1F800394 |= 0x40`).
- `idx` is the sign-extended `i16` at operand[0..2] (a story/entry id; distinct from the `0x3E` 7-id `map_id`).
- Writes the destination entry tile via `_DAT_80073EF4`/`_DAT_80073EF8` (formula `(b & 0x7F) * 0x80 + 0x40`, `+0x80` if the high bit is set - the far half of the tile) and the arrival facing into `_DAT_80073EFC` from `dir & 7` through the 8-entry i16 compass table at SCUS `0x80073F04` (`[0, 0x200, .. 0xE00]` - facing = `(dir & 7) * 0x200` in the 12-bit angle space). Engine: `World::seat_player_at_tile` + `World::face_player_sector` apply both on warp arrival.
- PC += 7 + name_len.

A scene's controller script lists every reachable destination as one of these ops - see [world-map § scene destinations](world-map.md). (This op only *looks* like dialog when the over-approximating walk desyncs on a literal `?` = `0x3F` inside message text. Field **dialogue** has no dedicated opcode - see [§ Field dialogue](#field-dialogue-has-no-opcode).)

#### 0x3E WARP (mode-24 minigame door-warp)

The `op0 >= 100` arm of op `0x3E` is the **minigame entry warp**. Unlike the named `0x3F` scene-change, it carries **no destination scene name** - the destination is a code overlay selected by `sub_id = op0 - 100`, and the "destination-name handling" is a backup/restore of the *current* scene so the minigame can warp back. The whole chain is **SCUS-resident** (no overlay capture needed):

1. **VM arm** (`case 0x3e` in `FUN_801DE840`, field overlay PROT 0897): `_DAT_8007BA34 = op0 - 100`; `_DAT_8007B83C = 0x18` (mode 24 OTHER INIT); `_DAT_80084440 = 0` (session-winnings accumulator); `_DAT_8007BAC0 = 0`; clears `player[+0x10]` bit `0x80000`. `see ghidra/scripts/funcs/overlay_0897_801de840.txt`.
2. **Mode-24 OTHER INIT** `FUN_80025980` (static `SCUS_942.54`): **backs up the active scene name** - `memcpy(0x8007BAE8, 0x80084548, 8)` - and the companion scene-id word `_DAT_80084540` into the gp-pool slot `0x8007BAC4` (`gp+0x7ac`, `gp = 0x8007B318`). Then loads the per-sub-id minigame overlay into slot A via `FUN_8003EBE4(sub_id + 0x4D)` (`sub_id >= 6` adds 2 first), calls the sub-id's init entry in the freshly loaded overlay (switch on `_DAT_8007BA34`, bracketed by the `"other init"` / `"other init end"` debug prints), and hands off to mode 0x19 (OTHER MODE run). `see ghidra/scripts/funcs/80025980.txt`.
3. **Return warp** `FUN_80026018` (static SCUS; the minigame overlays call it on exit): **restores the scene name** - `memcpy(0x80084548, 0x8007BAE8, 8)` - and `_DAT_80084540` from `0x8007BAC4`, commits the session winnings into the casino-coin bank (`_DAT_800845A4 += _DAT_80084440`, saturating at 9,999,999), and sets `_DAT_8007B83C = 2` (mode 2 MAIN INIT), whose per-scene initializer `FUN_801D6704` reloads the restored scene - completing the round trip. `see ghidra/scripts/funcs/80026018.txt`.

Sub-id → overlay dispatch (init VAs are entries in the loaded overlay at slot-A base `0x801CE818`; each verified by the init VA landing on a function prologue in exactly that PROT entry):

| sub_id | init VA | PROT entry | Content |
|---|---|---|---|
| 0 | `0x801CF070` | 0972 | Fishing minigame (dev `other1`) |
| 1 | `0x801CE8A0` | 0973 | 1-sector dev module `OTHER2` (runtime slice is a single sector; leading strings `OTHER2 / CICLE1 / SPRITE1 / SPREAD / GT4 DIV16` - identity open) |
| 2 | `0x801CEE80` | 0974 | Dev module `OTHER3` (7-sector slice, leading strings `OTHER3 / SELECT NO %d DEPTH %d` - identity open) |
| 3 | `0x801CEC94` | 0975 | **Casino slot machine** (dev `other4`; the documented reel-SM overlay - `FUN_801CF0D8`/`FUN_801D13E8` land on prologues in this entry, and the `"insert 3 coins"` / `"game_coin %d"` help text sits inside the runtime slice; see [`minigame-slot-machine.md`](minigame-slot-machine.md)) |
| 4 | `0x801CF00C` | 0976 | Baka Fighter (dev `other5`; live-confirmed - the mode-24 entry capture holds `_DAT_8007BA34 = 4`, `autorun_minigame_overlay_capture.lua`) |
| 5 | `0x801CEA6C` | 0977 | Monster-roster minigame (dev `other6`; arena monster-name table - NOT the Muscle Dome SM, whose `FUN_801D0748` does not land in this image) |
| 6 | `0x801CEF54` | 0980 | Noa dance rhythm minigame (Disco King) |

The PROT indices follow the corrected overlay-loader arithmetic - `prot_index = param + 0x37F` in extraction index space (see [boot.md § overlay loaders](boot.md#game-mode-state-machine)): the in-RAM TOC at `0x801C70F0` is raw `PROT.DAT` from byte 0 (byte-verified against the `door_warp_town01_to_map01` save state), so the resolver's `toc[idx+2]` start-LBA read sits 2 entries above the extraction's per-entry indexing. The runtime image for each sub-id is the slice `[entry_start, next_entry_start)` (the resolver's size return), which is why the minigame entries' larger extraction footprints over-read into their neighbours.

### 0x43 ACTOR_CTRL - sub-dispatcher

22+ sub-ops, keyed on operand byte 0:

#### 0x43 sub-0/1/A/B - halt-acquire dispatcher

```c
// Acquire halt if not already halted (or if system channel can override):
if (((ctx[+0x94] != 0) || ctx == _DAT_8007C364) &&
    (((ctx[+0x10] & 0x400) == 0) || (_DAT_801C6EA4[+8] != 0))) {
    ctx[+0x94] = pc;            // save resume PC
    ctx[+0x54] = 0;
    ctx[+0x10] |= 0x400;        // set HALT flag
    if (ctx == _DAT_8007C364) { // system channel: also halt caller
        caller[+0x94] = pc;
        caller[+0x54] = 0;
        caller[+0x10] |= 0x400;
    }
}
```

If `pbVar47[1] == 0 && pbVar47[2] == 0`: use ctx's current position (read `+0x14/+0x16/+0x18`); store negated-Y at `ctx[+0x8E]`. Else: decode target XZ from operand bytes via `(b & 0x7F) * 0x80 + 0x40` (or `+0x80` if high bit set); call `func_0x80019278(ctx)` for collision lookup of Y.

Resume PC source:
- **Sub-0 / sub-1** (`*pbVar47 <= 9`): `func_0x8003CE9C(pbVar47 + 3)` (signed 16-bit at offset +3). 5-byte instruction.
- **Sub-A / sub-B** (`*pbVar47 > 9`): `func_0x8003CE9C(pbVar47 + 7)` (signed 16-bit at offset +7). 9-byte instruction.

If halt was *not* acquired: falls through to a generic skip-and-return path.

#### 0x43 sub-2/3-6/7/8/9/C/D/E/F - actor / sound / face / position cluster

| Sub | Encoding | Effect |
|---|---|---|
| 2 | `[43, 2, a1, a2, a3, lo, hi, b6]` (8 bytes) | 3-actor talk via `FUN_801D2D38`. |
| 3..6 | 10 bytes (`[43, sub, b1..b4, lo_ticks, hi_ticks, lo_curve, hi_curve]`) | Sound register ramp on slot `_DAT_8007B610` (sub-6) / `B614` (sub-4) / `B60C` (sub-5) / `B618` (sub-3). |
| 7 | 17 bytes | Face / body rotation setup. Writes a 12-byte struct at `&DAT_80087E68 + face_id * 12`, schedules a `func_0x8003C5F0` ramp. |
| 8 | 2 bytes | Face / rotation reset: clears `+0x6D` and `+0x7A`. |
| 9 | 10 bytes (`[43, 9, x, y, z, ticks]`) | Explicit position with optional collision tween via `FUN_801DE698`. When `ticks == 0`, immediate writes (skipping `0xFFFF` sentinel). |
| 0xC | 5 bytes | Allocate scripted actor via `FUN_801DE754` → `FUN_80020DE0(&DAT_801F2858, _DAT_8007C34C)`. |
| 0xD / 0xF | 6 bytes | Allocate actor via `FUN_801DE7BC` with mode (3 for 0xD, 0 for 0xF). |
| 0xE | 2 bytes | Mark currently-iterating actor with flag bit 0x8 (`*(int *)(actor + 0x10) \|= 0x8`). |
| 0x16+ | - | No `case` arm in the original `case 0x43` inner switch; falls through with `iVar45 = param_2` (the dispatcher-default initialiser at line 4511 of the dump) - halts at PC. |

#### 0x43 sub-0x10..0x15 - screen-widget family + VRAM blit

These sub-ops are the script-side drivers of the PROT-0900 **screen-effect
widget family** ([move-vm.md § consumers](move-vm.md); engine port
`engine-core::screen_fx`) plus one VRAM rect-copy op. Dispatch: the op-0x43
arm (`0x801DF354`, main-JT slot for opcode 0x43 at `0x801CECC0`)
bounds-checks the sub-op (`< 0x16`) and jumps through the 22-entry JT at
`0x801CEDA8` (PROT 0897 file `0x590`, base `0x801CE818`); entries
0x10..0x15 land on the arms below.

| Sub-op | Encoding | Callee (PROT 0900 unless noted) | PC delta |
|---|---|---|---|
| 0x10 | `[43, 0x10][x][y][w][h][tex_x][tex_y][clut_x][clut_y]` i16s + `rgb` u24 | `FUN_801F8004(operand+1)` - **sprite-widget spawn** (inline 19-byte record) | +21 |
| 0x11 | `[43, 0x11][l][t][r][b][dur]` u16s | `FUN_801F8D4C(l,t,r,b,dur)` - **screen-mask (iris) rect tween** | +12 |
| 0x12 | `[43, 0x12][src_x][src_y][w][h][dst_x][dst_y]` s16s | `FUN_800468A4(6, …)` (SCUS) - **GP0 `0x80` VRAM→VRAM rect copy** into OT slot 6 (packet builder `FUN_80057914`; `src_y += 0xF0` under the back-buffer flag `DAT_8007B74C`); **dual call** when `w > 0xFF` with offset shifts `(+0xF0, _, -0xE0, _, +0x100, _)` and a 0x100 clamp - the same >256-wide two-page split as the panel widget. No on-disc scene script uses it. | +14 |
| 0x13 | `[43, 0x13][x][y][w][h][tex_x][tex_y]` i16s | `FUN_801F88FC(operand)` - **image-panel spawn** (record read from operand+1) | +14 |
| 0x14 | `[43, 0x14][x][y][scale][dur]` s16s | `FUN_801F8E6C(x, y, scale, dur)` - **panel move/scale** (`scale` 4.12 fixed) | +10 |
| 0x15 | `[43, 0x15][x_left][x_right][y0][y1][y2][y3]` i16s | `FUN_801F8F28(operand+1)` - **letterbox config** | +14 |

On disc the family is exclusive to the eight ending-sequence scenes
(`edteien`, `edbylon`, `edbalden`, `edlast`, `edretoin`, `edkorout`,
`edson`, `edstati3`), always in partition-2 (cutscene-timeline) records:
mask-to-black (`0x11` with the degenerate rect `[0x20,0x20,0x20,0x20]`) →
fullscreen photo panel (`0x13`, every retail record `[0,0,0x140,0xE0,
0x200,0]` - the >0x100-wide two-page split is exercised by every use) →
shrink-to-corner (`0x14`, scale `0x700`) → credit-name sprite strips
(`0x10`, all 218 records CLUT row 475); `edlast` is the credits crawl and
the only letterbox consumer. Sub-0x12 never appears in on-disc scene
bytecode (which leaves its natural reading - staging the panel's source
image - an inference).

Sub-0x12 detail (the only one with non-trivial logic):

```c
c = signed16(operand[5..7]);
if (c > 0xFF) {
    func_0x800468a4(6, signed16(operand[1..3])+0xF0, signed16(operand[3..5]),
                       signed16(operand[5..7])-0xE0, signed16(operand[7..9]),
                       signed16(operand[9..11])+0x100, signed16(operand[11..13]));
    c_clamped = 0x100;
} else {
    c_clamped = c;
}
func_0x800468a4(6, signed16(operand[1..3]), signed16(operand[3..5]),
                   c_clamped, signed16(operand[7..9]),
                   signed16(operand[9..11]), signed16(operand[11..13]));
```

### 0x44-0x4F (record-spawn / camera / render / state / move-block)

| Op | Mnemonic | Notes |
|---|---|---|
| 0x44 | `SPAWN_RECORD` | `[44, global_index]`, 2 bytes. Spawns a MAN partition-2 record as a new field-VM context. [Detail](#0x44-spawn_record). |
| 0x45 | `CAMERA` | Sub-dispatch on `op0 & 0xC0`: `0x00` = configure 10 sub-words, `0x40` = LOAD (`FUN_801DBC20`), `0x80` = SAVE (`FUN_801DE004`), `0xC0` = APPLY (`FUN_801DAB90` + `FUN_801DAA50` then absolute jump). |
| 0x46 | `RENDER_CFG` | Fog/render params. `op0 == 0x24` writes 4 bytes (DAT_1F8003E8-EB); else short 2-byte form. |
| 0x49 | `STATE_RESUME` | Tristate state machine on `_DAT_8007B450`, sub-cases 0..0xD. [Detail](#0x49-state_resume). |
| 0x4A | `WAIT_FRAMES` | `ctx[+0x54] += scratch_delta; if (sum < operand) return; else PC += default`. Frame timer. |
| 0x4B | `ANIMATE` | Multi-keyframe setup. Writes `ctx[+0xB0+N] / +0xB8 / +0xC8`, sets `+0x10` bit 0x1000 (animation flag). PC += 3 + count*4. |
| 0x4C | `MENU_CTRL` | Outer-nibble-dispatched (16 sub-dispatchers). See [`script-vm-menuctrl.md`](script-vm-menuctrl.md). |
| 0x4D | `BBOX_TEST` | Inside-box advances PC by 7; outside-box jumps to `pc + header_size + 4 + LE_u16(operand[4..6])` via `FUN_801E3614`. |
| 0x4E | `INVENTORY_CMP` | Compare-and-jump on party state; every sub-op 0..9 is the 7-byte compare-and-skip. [Detail](#0x4e-inventory_cmp). |
| 0x4F | `SCENE_REGISTER_WRITE` | Writes three `u16` values to `_DAT_801C6EA4 + 0x10/+0x12/+0x14`. |

#### `0x44` SPAWN_RECORD

Spawns a MAN partition-2 record as a new field-VM context. It unpacks a packed
triple via `func_0x8003D064`, then calls `func_0x8003BDE0` (ra `0x801DF098`) with
the gate forced to 1.

The operand is a **GLOBAL** record index, re-based into partition 2 (`- N0 - N1`) by
the dispatcher itself. The record's own C1/C2 story-flag gates still apply.

Live-probe-pinned: the opening chain's `opdeene` / `opstati` / `opurud` entry
scripts launch their prologue timelines this way (`44 23` / `44 21` / `44 32`). See
[`cutscene.md`](cutscene.md#record-spawn-mechanisms-live-probe-pinned).

The earlier `COUNTER` reading of this opcode is superseded.

#### `0x49` STATE_RESUME

A tristate state machine on `_DAT_8007B450`, with sub-cases 0..0xD:

- **Idle** (`== 0`): arms it - spawns an effect-actor `func_0x80020DE0(0x8007065C,…)` and sets `_DAT_8007B450 = operand_ptr`.
- **Armed** (`!= 0, != 1`): `return param_2` - re-enters the SAME PC each frame until the actor writes `_DAT_8007B450 = 1` (the Done writer is field-overlay `FUN_801F159C`-class).
- **Done** (`== 1`): clears it and advances the PC.

Done sub-6/8/9/C/D jump through `LAB_801df898` (PC += 5). Done sub-0 walks an inline
MES-shape payload via `func_0x8003CA38` (`length = pbVar47[2]`, PC +=
`5 + length + walked`).

The Armed park is the town01 name-entry hand-off (P2[3] `+0x02C6`); see
[`playthrough-coverage.md`](../tooling/playthrough-coverage.md#s3-captured-the-town01-opening-is-the-name-entry-screen).

##### The name-entry screen (op-`0x49` `49 03 <char>`)

The town01 hand-off's operand names the party slot (`_DAT_8007B450 + 1` -
`03` sub, `00` = Vahn); the field overlay's SM runs the screen and writes
the typed name **live** into the character record's name field at `+0x2A7`
(record base `0x80084708 + n*0x414`). Renderer `FUN_801E6B34`
(`ghidra/scripts/funcs/801e6b34.txt`), cursor cell at `_DAT_8007BB88`, SM
state at `_DAT_8007BB94` (1 = editing, 4 = the Yes/No confirm).

Traced geometry (draw-stream-pinned, 320x240 framebuffer pixels; overlay
base `(32, 99)` from the context's `+0xA/+0xC`):

- Two windows in the pause-menu filigree skin: the grid window at footprint
  `(24, 91, 272, 120)` and the name-field window at `(196, 71, 88, 28)` -
  the latter is the renderer's own `FUN_8002C69C(base_x+0xAC, base_y-0x14,
  0x48, 0xC)` centre rect plus the 8 px skin border.
- Charset grid (7 rows x 17 at `0x801F29F0`): glyphs from `base + (4, 4)`,
  15 px column pitch, 14 px row pitch, ink 7 white; `|` separators at
  columns 5/11 skipped; blank cells are selectable space glyphs.
- Working name at `(208, 79)` ink 7; a teal (`ink 5`) `_` caret 6 px after
  the name, blinking at 75% duty (`frame & 0x18`), gated to the 57 px field.
- Control bar at `y = 191`, ink 6 gold, three buttons resolved through a
  **`grid[cell + 2]`** sentinel read (`0x66`/`0x64`/`0x65`): "BS" at
  `x = 36` (backspace), the quoted default name at `x = 148` (**restores
  the template name** - live-verified against `+0x2A7`; there is no space
  button), "Select" at `x = 244` (end). Cursor anchors = cells 102/108/114;
  the SM opens on Select (initial cell `0x74`).
- Prompt "Select your name." at `(176, 32)`; the confirm state replaces it
  with "Is this name okay?" at `(172, 24)` + stacked teal "Yes" `(204, 38)`
  / "No" `(204, 50)` rows, hand on **No** at open, up/down moving it.

After the Done resume the timeline's post-naming beats animate the lead:
`A2 F8 30` then `A2 F8 31` (op-`0x22` ExecMove, P2[3] `+0x030B`/`+0x0352`)
land the player's `+0x4C` anim pointer on the scene ANM bundle's records
47/48 (`record = move_id - 1`, the same record space as op-`0x4B` NPC
cues) for one playthrough each before the walk-out moves resume the
locomotion clips. Engine port: `engine-core::name_entry` (SM),
`engine-ui::ui_menu::name_entry` (draw builders + traced constants), and
the `field_player_move_cues` scripted-clip queue on
`engine-core::field_anim::FieldPlayerAnim`.

#### `0x4E` INVENTORY_CMP

Compare-and-jump on party state. **Every sub-op 0..9 is the 7-byte
compare-and-skip** - a raw jump table at `0x801CEE30` whose value loaders all join a
shared compare:

| Sub-op | Compares |
|---|---|
| 0 / 1 | char HP / MP `(cur, max)` pair - the only `max * arg >> 8`-scaled form |
| 2 | level byte `+0x130` |
| 3 | gold vs u16 (the inn gold gate, `legaia_asset::inn_costs`) |
| 4 | **BIOS `Rand() & 0xFF`** (random-chance branch) |
| 5..8 | **slot table `0x801C6460[sub - 5]`** (read side of the `4C CA/CB/CC` writes) |
| 9 | coins vs u16 |
| 10 / 11 | gold / coin u32 compare (9 bytes) |
| 12..=15 | fall through |

The old "5..8 absolute jump" and "4 rand = next PC" readings were the collapsed
decomp switch ([threads doc](../reference/open-rev-eng-threads.md), op-0x4E
details). Ported: `field_disasm::decode_subops` + `engine-vm` `flow::op_4e`.

### 0x4C MENU_CTRL - outer-nibble dispatch

The 0x4C dispatcher's **outer high nibble** of `op0` selects 16 sub-dispatchers (party-leader change, menu sub-dispatch, party-view-swap, immediate-or-ramp slot writes, the collision-grid wall-paint at nibble 7, the large multi-purpose nibble-8 cluster, the inverted-Y / actor-spawn nibble-D cluster, the FMV-trigger and emitter nibble-E cluster, and more).

The **full** per-outer-nibble table, the 16×16 sub-dispatch coverage matrix, the actor-allocator + materializer wiring (`0x4C nibble-8 sub-0`), the immediate-or-ramp nibble-4 cluster, and the VRAM STP-bit nibble-D sub-4/sub-5 ops are in **[script-vm-menuctrl.md](script-vm-menuctrl.md)**.


## Default-case "extension" opcodes - the fourth flag bank

The default arm of the dispatcher checks `*pbVar43 & 0x70`:

- `0x50`: `func_0x8003CE08((*pbVar43 & 0x8F) << 8 | pbVar43[1])` - **SET bit**.
- `0x60`: `func_0x8003CE34(...)` - **CLEAR bit**.
- `0x70`: `func_0x8003CE64(...)` - **TEST bit** (returns `0xFF` if set, `0` if clear). When non-zero, the dispatcher consumes two more operand bytes (`pbVar43[2..4]`) as the post-test action target.

The flag-op cluster's helper calls sit at fixed VAs shared by **every**
slot-A sibling's copy of the dispatcher (disc-byte-verified `jal` words in
PROT 0897 at file `+0x14D78/+0x14DA0/+0x14DC8`): SET `jal` at `0x801E3590`
(return `ra 0x801E3598`), CLEAR at `0x801E35B8` (`ra 0x801E35C0`), TEST at
`0x801E35E0` (`ra 0x801E35E8`). At these sites register `s0` holds
`pbVar43` (the VA of the current opcode byte in the script buffer) and
`s8` holds the running `pc_offset` (the byte offset the dispatcher was
entered with) - the anchor the runtime provenance probe
(`autorun_flag_reader_watch.lua`) uses to attribute each live flag
set/clear/test to its exact bytecode offset. Three secondary TEST call
sites (`jal`s at `0x801E26B4`/`0x801E28A0`/`0x801E28BC`, the high-byte /
gate op families) use different register allocation; the probe recovers
the op pointer there by scanning the saved registers for a pointer whose
bytes decode as the matching op + operand.

The three SCUS dispatchers all operate on the **same bitfield array based at `0x80085758`**:

- The disassembly of `FUN_8003ce64` (TEST) is `lui v1,0x8008; addiu v1,v1,0x4140` (`v1 = 0x80084140`) then `lbu v1, 0x1618(v0)` with `v0 = (idx >> 3) + v1`, i.e. the byte address is `0x80084140 + 0x1618 + (idx >> 3)` = `0x80085758 + (idx >> 3)`. Each does `index >> 3` to pick the byte and `0x80 >> (index & 7)` to pick the bit.
- So the `0x5x/0x6x/0x7x` opcode space encodes a 12-bit operand: the low 4 bits of the opcode plus the next operand byte form an 8-bit (1-byte) flag index - but with the "extended" prefix bit (0x80) preserved into the high bits, the addressable space is 12-bit, suggesting per-script-context banks within the same array.
- (An earlier draft mislabeled the base as `DAT_80086D70` by double-counting the `0x1618` displacement onto `0x80085758`; the Ghidra symbol `DAT_80085758` is itself `0x80084140 + 0x1618`, and the array is indexed directly from there - no further `+0x1618`.)

This is a **fourth flag bank** (per-script local at `ctx[+0x62]`, 32-bit globals at `_DAT_1F800394`, ctx flag word at `ctx[+0x10]` are the other three). It is **not** a wholly separate region: base `0x80085758` falls inside the story-flag RAM window `0x80085600..0x80085800` (at `+0x158`) and the bank extends past `0x80085800` (flag indices up to ~`0xFFF` reach `0x80085758 + 0x1FF`). In a retail SC save block the bank therefore lives at SC offset `0x1618` (= `0x200 + (0x80085758 - 0x80084340)`, via the `SAVE_GAME_DATA_RAM_BASE` formula in `crates/save`), overlapping the story-flag bitmap (`SC 0x14C0`, 512 bytes) and continuing to the inventory array (`SC 0x1818`). Seeding the engine's `World::system_flags` from `sc_block[0x1618..0x1818]` reproduces the live bank as of the save.
Note this bank is **not** sufficient on its own to drive a scene's collision: see [`field-locomotion.md`](field-locomotion.md) - the `0x4C` nibble-7 wall paints reached through it are story-conditional collision *deltas*, not the base walkable grid.

Decompiled bodies:

```c
// 0x8003CE08 (SET):
(&DAT_80085758)[(int)idx >> 3] |= (byte)(0x80 >> (idx & 7));
// 0x8003CE34 (CLEAR):
(&DAT_80085758)[(int)idx >> 3] &= ~(byte)(0x80 >> (idx & 7));
// 0x8003CE64 (TEST):
return ((&DAT_80085758)[(int)idx >> 3] & (0x80 >> (idx & 7))) ? 0xFF : 0;
```

Effective opcode space therefore includes the explicit 0x21-0x4F range *and* any byte whose high nibble is 0x5/0x6/0x7 (potentially 192 more "wide" opcodes routed to three SCUS dispatchers).

### Disc-wide SYSTEM-flag census tooling

An overworld progress gate reads a SYSTEM flag (`0x7x` TEST) in one scene, but the **setter** that opens it (`0x5x` SET / `0x6x` CLEAR) almost always lives in a *different* scene's MAN. To resolve a gate to its writer, `legaia_engine_core::man_field_scripts` walks the flag ops out of the decoded MAN:

- `walk_partition_gflag_sites` reports every flag site in a MAN partition at real opcode boundaries, tagging each with its bank (`FlagBank::Scratchpad` for the `0x2E`/`0x2F` global ops, `FlagBank::System` for the `0x50..=0x7F` ops), the full flag number (`(lead & 0x8F) << 8 | operand` for system flags), and the SET/CLEAR/TEST kind.
- `system_flag_census` runs that walk over **every** CDNAME scene's MAN across all three partitions and returns `flag -> [(scene, partition, record, op, kind)]`, sorted by flag number - the setter-vs-gate map the progress-gate RE consumes.

CLI: `legaia-engine man-scripts --scene <name> --gflag-partition <N>` lists both banks for one scene; `legaia-engine man-scripts --system-flag-census` runs the disc-wide census. The flag-index arithmetic mirrors the dispatchers above, and the engine's own bit helpers are `World::system_flag_set`/`_clear`/`_test`.

### A second script-byte carrier: the streaming variant MAN

A live whole-playthrough capture (PCSX-Redux exec-bps on `0x8003CE08`/`0x8003CE34`, probe `autorun_flag_firehose.lua`) shows every story-flag write across the chapter-1 scenes returning to the dispatcher's own `0x5x`/`0x6x` arms (`ra 0x801E3598` / `0x801E35C0`, field overlay resident) - the ops above are the **only** story-flag writers observed. The remaining callers touch only low system indices: `0`/`3` staged by the world-map entity SM (`FUN_801DA51C`), `0x35` set at battle-end victory (`FUN_8004E568`) and cleared by the entity SM, `0xB`/`0xC`/`0x18` interaction/engagement locks, `0xE` by two dispatcher spawn ops.

The executed script bytes at the Mt. Rikuroa post-Caruban beat live in a heap-resident carrier that is **not** the scene's asset-table bundle MAN: it is a second, plain MAN shipped as the type-3 chunk of a standalone `data_field_streaming` PROT entry (the chunk header is the ordinary sub-asset descriptor `[u24 size][u8 type=0x03]`; the payload parses with `legaia_asset::man_section` like any MAN).
The resident copy byte-matches PROT `0157_rikuroa`'s chunk, and it carries the story-flag `0x142` SET (`51 42`) at four record sites - `P1[10..12]` plus the post-victory cutscene record `P2[50]`, whose C1 gate is `0x142` itself (the self-latching one-shot).
The carrier's records also pin **how** `P2[50]` runs: the boss stager `P1[3]` SETs the transient marker `0x289` (`52 89`) right before its battle-entry op (`3E FF 11`),
and the scene-entry system script `P1[0]` tests that marker on the post-battle scene re-entry (`72 89` at `+0x13A`) - its taken arm (`+0x7E6`: fade, BGM, `44 5C`) issues the op-`0x44` spawn of global record `0x5C` = `P2[50]`, C1-gate-checked by the dispatcher.
The same shape sits one branch level up: `P1[0]`'s first-arrival arm spawns `P2[43]` (`44 55`) while flag `0x2FB` is clear, and that record's own `52 FB` latches it.
The clean-room engine executes this chain organically - the host re-runs the entry script on the battle-to-field mode edge (`SceneHost::tick`) and the spawned record's own script bytes land `0x142`; disc-gated oracle `engine-core/tests/organic_beat_records_disc.rs`.
Thirteen retail blocks ship such a **streaming variant MAN** (extraction indices: `dolk2` 70, `rikuroa2` 122, `rikuroa` 157, `rayman` 201, `station` 228, `balden2` 320, `ropeway2` 339, `taiku` 373, `doman` 401, `taiku2` 427, `nilboa2` 648, `edbalden` 792, `eddoman` 817); for the v12-family dungeons (`rikuroa` / `dolk2`, whose own bundle is the MAN-less `count=4` form) the streaming carrier is the scene's **only** MAN.

`system_flag_census` (and the motion / op-`0x49` censuses) walk **every** carrier per scene - the bundle MAN plus the streaming variants, enumerated by `legaia_engine_core::man_field_scripts::scene_man_carriers` - so the variant-resident writers surface: the `0x142` setters above, the `0x63A` beat writers. Disc-gated pins: `crates/engine-core/tests/man_variant_carrier_census_disc.rs`. CLI: `legaia-engine man-scripts --scene <name> --variant <entry_idx>` targets a variant carrier directly (census rows tag them `VARIANT-MAN`); `--p2-gates` prints every partition-2 record's C1/C2 header gate lists + name (the `FUN_8003BDE0` spawn-condition surface the inline-op censuses cannot see).

**Decode-coherence flag.** The census walker desyncs inside unframed Shift-JIS dialogue and inline data tables, where text bytes alias the `0x50..=0x7F` flag ops
(the full-width digit run `82 54 82 4F` aliases `SysFlag.Set idx=0x482`; a repeating full-width `ＥＸＩＴ` label table aliases `64 82` clears).
Every census site therefore carries `GFlagSite::clean`: `true` only when at least `CLEAN_RESYNC_INSNS` instructions decoded error-free between the walker's last decode error (or record start) and the site.
The CLI prints `DESYNCED?` on non-clean rows - treat those as byte noise until verified by hand disasm or a live capture.
This falsified the earlier "`0x482` set by the `other7` pool / cleared by the `edbalden`/`eddoman` epilogue variants" reading: all 37 of `0x482`'s census sites are non-clean text aliases, while the live-confirmed `0x142` writer arms decode clean.

### Door-choreography record families: the `0x00F` busy-mutex + the jouind per-visit band

Two partition-2 record shapes in the Drake-castle cluster (`jouinc` `[43,18,60]`, `jouind` `[.,.,17]`) look
like story-gate families in the `--p2-gates` output but are **mechanism state, not story state**. Both rest
on the C1 polarity: a C1 flag **blocks the record while SET** (the one-shot mechanism, see
[`field-locomotion.md`](field-locomotion.md) kind-1 triggers).

**The `0x00F` busy-mutex family.** `jouinc` P2[2..59] (SJIS names `Ｊ０１`..`Ｊ５８`) and `jouind`
P2[0..1]/[6..9] are all gated `C1=[0x00F]`, and every record's **first op sets `0x00F`** (`50 0F`) while its
**last clears it** (`60 0F`) before parking on a `JmpRel -2`. `0x00F` is a transient low system flag (the
same band as the `0xB`/`0xC`/`0x18` interaction locks), so the C1 gate is a **mutual-exclusion lock**: no
door record spawns while another is mid-flight, and the running record cannot re-trigger itself. The body is
door-walk choreography, one record per castle door: an extended nibble-A conditional (`CC <door_actor> A1 0A
target`) branches on the door actor's local flag 10 (open state), the door actor animates (`CB` Animate) and
flips its local-flag pose bits, the player channel `0xF8` gets the `ExecMove 6` / `ExecMove 7` / `ExecMove 2`
walk-through sequence, and the room transition is a `36 00 80 xx` + `36 04 80 00` SceneFade pair (an
intra-scene reposition, not a `0x3F` scene change). The record names are door ids, not grid coordinates.

**The jouind per-visit door band `0x4BE..0x4C2`.** `jouind` P2[10..13] are gated on the *story-numbered*
band but the band is **reset by `jouina` P1[0]** (entry script clears all five flags), so it is per-castle-
visit door/lift state, not chapter-persistent progress: P2[10]/P2[11] (each `C1=[0x4C1]`, i.e. live while
`0x4C1` is clear) are door choreographies that SET `0x4BE`/`0x4BF` respectively and share a first-use latch
`0x4C2` (in-body `74 C2` test-skip + `54 C2` set); P2[12] (`C1=[0x4C0,0x4C1]`) is a two-door camera cutscene
that one-shots itself via `54 C0`; P2[14] SETs `0x4C1`, retiring the whole family for the visit; and P2[13]
(SJIS name `セット`) re-applies the door-open visuals on entry by branching on `0x4BE`/`0x4BF`. All
sites are census-clean (`--system-flag-census`, scenes `jouina`/`jouind`/`jouine`).

**Width blindness is the desync's second face.** A missing/wrong sub-op *width* in the disassembler desyncs
the walk even in clean non-dialogue code, and a site hidden that way looks identical to "no writer exists".
Flag `549` (`0x225`, the Rim Elm opening one-shot) was exactly this: town01 `P2[3]` SETs it from its own
script bytes (`52 25` at body `+0x3`, the record its own C1 gates - the `P2[50]`/`0x142` self-latch shape),
but the preceding `4C ED` op (`_DAT_8007BA66` write, retail `param_2 + 3`) had no width in the disassembler,
so the walk mis-read `ED 01 52` as a phantom Clear and swallowed the SET. Caught live first (reader-watch
script-PC capture: SET `ra 0x801E3598`, `vm` offset `+0xF`), then fixed statically: the whole `4C 0xE_` sub-op
width family is now pinned from the retail dispatcher's `param_2 + N` advances (subs 4/5/7/8/9/A/B/C/D/E),
and the last two delay-slot-hidden legs (sub-0/3) from the raw asm: both arms (`0x801E306C` / `0x801E3108`,
case targets confirmed against the outer-0xE jump table at VA `0x801CF008`) advance +3 - sub-0 through the
`addiu s8,s8,0x3` entry at `0x801E00B8`, sub-3 there or in the `j 0x801E00BC` branch-delay slot. Neither is
a halt (the decompile's `goto LAB_801e00bc` folds both entries into the no-advance label). Anchor
`flag_549_writer_is_the_rim_elm_p2_3_self_latch`. Before trusting any "flag F has no script writer" verdict,
confirm the ops *around* the expected site decode with known widths.

Width blindness also comes at **whole-nibble granularity**: the disassembler once had no decoder at all for
`0x4C` outer nibbles `9`/`A`/`C`/`D`/`F`, so every record crossing one (e.g. the `CC 06 A1 ..` extended
nibble-A conditional jumps that pepper the jou-castle door records) desynced exactly like the `4C ED` case,
hiding thousands of clean flag sites and minting phantom ones from the resync garbage. All sixteen outer
nibbles now decode (`legaia_asset::field_disasm::decode_subops`), with widths mirrored from the executing
VM's `menu_ctrl` port (itself pinned from the retail dispatcher's `param_2 + N` advances); nibble `B` is
genuinely undefined in retail (no `case 0xb` - the default arm halts) and stays a decode error. The pinned
spine-flag verdicts are unchanged under the full-width walk: `0x482` stays all-alias, and the koin gates
`0x50A`/`0x5D6` still have no script writer disc-wide.

**Width blindness's third face is a *wrong* width in an already-decoded arm.** The `0x4C` nibble-8
sub-widths are pinned from the raw asm of the nibble-8 switch (same overlay, base `0x801CE818`): sub-1
(actor model+anim set) advances `+9` unconditionally (`addiu fp,fp,9` at `0x801E1FC4`), sub-3 (rect tile
fill) `+7` (exit `addiu fp,fp,7` at `0x801E2130`), sub-5/E/F (halt-acquire) `+5` on acquire (`li s7,5` at
`0x801E21B8`, `addu fp,fp,s7` in the `beqz` delay slot at `0x801E21D4` - only the predicate-failure path
halts), sub-6 `+15` (`addiu fp,fp,0xf` in the `jal` delay slot at `0x801E21E8`), sub-0 `+3`, sub-C `+4` -
matching the executing VM's `menu_ctrl/nibble_8.rs` port. A sub-1 width one byte over is what the
`vozz P1[7]` `.byte 0x05` decode error was: each `CC 0B 81 ..` op swallowed its follower's lead byte,
minting a phantom `Clear 0x400` where the retail stream reads the op's 10-byte extended form followed by
`35 64 00 05` (a BGM op) / `4A 1E 00` (WaitFrames). Under the pinned widths those followers decode in
place, the phantom rows disappear, and the spine verdicts above hold row-identical (`549`/`0x142` site
sets unchanged; `0x482` all-alias; `0x50A`/`0x5D6` writer-less, `0x50A` gaining one more clean koin3 TEST
reader and still no writer).

**Width blindness's fourth face is a *variable*-width arm read as fixed.** The `0x4C` **nibble-7**
collision-grid wall paint has **two** operand shapes (`FUN_801DE840` case 7): sub-0 (`byte &= 0x0F`, clear
walls) and sub-1 (`byte |= 0xF0`, block all four sub-cells) ignore the mask, so they carry only the four
range bytes and are **6-byte** ops; sub-2 (`&= ~(mask << 4)`) and sub-3 (`|= mask << 4`) consume a trailing
mask byte and are **7-byte** ops. Reading a fixed 7 for all four subs makes every sub-0/sub-1 paint swallow
its follower's lead byte, so a linear walk desyncs the moment it crosses one. Rim Elm's scene-entry script
is the case that exposes it: `town0c` `P1[0]` runs three sub-0 clears in a row, and past the first the walk
minted phantom `SysFlag.Test` rows with absurd operands (indices `24`/`280`, jump deltas of `~7000` in a
`0x242`-byte record) and hid the record's real gate logic. Under the pinned widths the record reads clean
and the Rim Elm gate becomes legible (below). The executing VM (`legaia-engine-vm`, field `menu_ctrl`
nibble 7) always advanced by the correct per-sub widths - it decodes the raw stream and never consulted the
disassembler - so this was a **static-walker-only** defect: it corrupted `scene_destinations`,
`scene_bgm_starts`, `scene_fmv_triggers`, `scene_stager_installs`, `boss_stager_placements` and the door
randomizer's MAN edits (all `LinearWalker` consumers), while runtime behaviour was unaffected. Fixed in
`legaia_asset::field_disasm::decode_subops`; the general lesson is that a sub-op family whose *width
depends on the sub* must be decoded per-sub, and the executing VM's port is the reference when the two
disagree.

**ASCII dialogue aliases survive the `clean` tag.** The US build's dialogue is plain ASCII, and the wide
flag ops land exactly on the letter ranges: `Set` leads `0x53..0x57` = `S..W`, `Clear` leads `0x61..0x67` =
`a..g`, `Test` leads `0x71..0x77` = `q..w`, each followed by one operand byte. So common English bigrams
mint flag ops - `ta` = `Test 0x461`, `s,` = `Test 0x32C`, `Sp` = `Set 0x370` - and because every such
2-byte pair *decodes* without error, a run of prose keeps the walker's error counter at zero and the
resulting sites carry `clean=true`. The `DESYNCED?` tag catches text only when a non-decodable byte
happens to precede the site within the resync window. Triage rules that follow from this:

- A flag whose **operand byte is outside printable ASCII** (`< 0x20` or `> 0x7E`) cannot be minted by
  dialogue - its census rows are trustworthy as sites (e.g. `0x382`, `0x3EF`, `0x304`, `0x5DC`).
- A flag whose operand byte is a letter/punctuation needs the site's *context window* checked in the
  record disasm (`--disasm-record`): trust sites embedded in choreography ops (`Camera`, `WaitFrames`,
  `SceneFade`, `4C`-family, `ExecMove`, emitter runs, or a `JmpRel` branch-arm boundary); reject sites
  whose neighbours decode as further letter-pair flag ops or `.byte` errors.
- Mirrored runs are self-proving: a `Set` run over a flag band whose exact mirror `Clear` run appears in
  the same record (the rikuroa `0x281..0x287`+`0x142` pairs) is real even when the census tags it
  `DESYNCED?` - the clean tag is conservative in both directions.

Hand-checks that applied these rules: the "chapter-wide readers" the census reports for `0x32C` (~50
scenes) and `0x461` (~30 scenes) are the `s,` / `ta` bigrams in NPC dialogue - both flags are real but
scene-local (see [open-rev-eng-threads](../reference/open-rev-eng-threads.md#region-story-flag-gate-families));
the Nivora successor gate `0x370` shows the context-window rule cutting **both ways inside one record**
(`doman` variant `P1[15]`): three `Sp` = `53 70` sites are the "Time**Sp**ace Bomb" dialogue (rejected),
but the fourth, at MAN offset `0x06397` (`+0x3018`), sits in a choreography run (`WaitFrames` / `MoveTo` /
`Effect` / `4C CD`) with a loop-back `JmpRel` to the record's gate-test head - and the head's own
`Test 0x370 -> +0x301E` jump target lands on the very next op after that `JmpRel`, so both sites decode
on the same op grid: this is the **genuine writer** (the Dr. Usha briefing self-latch; pinned by
`man_variant_carrier_census_disc.rs::flag_0x370_writer_is_the_doman_p1_15_usha_latch` - the earlier
"writer-less, the candidate is Space-Bomb prose" verdict predated the nibble-width pinning and
adjudicated a prose sibling, not this site); and the once-reported `Clear 0x400`
inside `vozz P1[7]` was the nibble-8 sub-1 width bug above - under the pinned width the bytes are the
op's own operand tail and its `35` BGM follower, and the census row disappears entirely.

**The census self-identifies the ASCII prose aliases.** Every site also carries `GFlagSite::text_alias`
(CLI marker `TEXT-ALIAS?`, on both the census and `--gflag-partition` rows): `true` when the site's raw
operand byte is printable ASCII **and** the surrounding `TEXT_ALIAS_WINDOW` (16 bytes each side)
contains a consecutive printable-ASCII run of at least `TEXT_ALIAS_MIN_RUN` (10) bytes **and** the
window puts two lowercase letters side by side - the sentence signature prose always has and bytecode
does not. This mechanizes the triage rules: the `ta`/`Sp`/`s,` bigram rows carry the marker even where
they decode error-free (`clean=true`), while the runtime-pinned real sites with printable operands stay
unmarked. The three conditions each carry weight: the town01 `P2[3]` `52 25` self-latch and the rikuroa
`51 42`/`61 42` ladders render as printable byte-streams themselves (`R.R.R.QB`) but break the run every
1-5 bytes on a non-printable operand (run length beats printable *density*), and the `0x527..0x52E`
one-hot selector clears (`65 27 65 28 ..`) sustain a 16-byte printable run but alternate op/operand so
they never form an adjacent lowercase pair. Like `clean`, the marker is triage, not suppression -
non-printable operands are alias-immune by construction, mirrored Set/Clear runs stay self-proving, and
a marked row means "check the record disasm", not "discard". The split it produces on a mixed flag is
the point: `0x527`'s census population is real one-hot ladder sites (unmarked, clean) *plus* the `e'`
prose bigram (marked), separated row by row. The advisory direction also occurs: two **runtime-pinned
real** `0x142` sites (the dolk `P1[26]` Clear and the dolk2 variant-carrier `P1[1]` Set) sit inside
dialogue-adjacent bytecode and carry the marker - exactly the "check by hand" case, and why the marker
never suppresses a row.

### The `0x527..0x531` scene-transition scratch band

A story-numbered flag band that is **engine scratch, not story state** - the census surfaces it as SETs in
nearly every scene once the full-nibble widths decode, which is exactly the signature of a shared idiom
rather than a beat. Every field scene's `P1[0]` entry script (and the exit-choreography arms of many
`P0`/`P2` records) repeats two patterns, byte-stereotyped across the disc (hand-verified in `deene P1[0]`
at body `+0x5B`/`+0x15D`/`+0x8F9` and its siblings):

- **One-hot selector `0x527..0x52E`**: clear all eight (`65 27` .. `65 2E`), then SET exactly one. Each
  block precedes a `SceneFade` (`36 xx`) / `4C 12` fade sequence, so the selected slot rides a scene
  transition - a departure-choice latch the arrival script can branch on.
- **Fade handshake `0x52F`/`0x530`/`0x531`**: `Set 0x52F` → `Test 0x52F` → `Clear 0x52F` ping-pong
  around `4C CA`/`4C CB` (screen-widget open/close) and `4C 12` fade ops, with `0x530`/`0x531` as the
  busy/latch pair. Same shape every time; never read outside the idiom.

The conc-family entry scripts keep an adjacent private slot in the same style (`0x522`: set at entry,
conditionally cleared on `0x3E5`/`0x4EE`). Treat any census row in `0x522..0x531` as this band's
mechanism traffic - like the `0x00F` door busy-mutex above, it lives in the story-numbered space without
being story progress.

### Drake-castle interior beat band: jouinb `0x44E..0x450` + the `0x461` record-state flag

`jouinb`'s cutscene records `P2[6..8]` (SJIS door-id names, the same family styling as the jouinc door
records) each end in a one-shot latch - `P2[6]` SETs `0x44E`, `P2[7]` `0x44F`, `P2[8]` `0x450`
(`Camera`/`WaitFrames`/`4C CD` choreography then `Set` + park-jump, hand-verified at `P2[6]` body
`+0x4A3`). `P2[8]` additionally runs an in-body state machine on `0x461`: `Test 0x461` at `+0xBC` (skip
to the already-done arm at `+0x1BC`, which starts with `Clear 0x461`), a second `Test` at `+0x70C`, and
`Set 0x461` at `+0xBC2` inside the closing camera choreography. All four flags are jouinb-local; the
census's wide `0x461` reader list across other scenes is the `ta` bigram (see the alias rules above).

## BGM lookup table

There isn't really a "BGM → file" lookup table - the BGM ID is a PROT-relative offset. From `FUN_800243F0` (the per-frame BGM/asset poller):

```c
if (_DAT_8007BAC8 < 2000) {
    _DAT_8007BAB8 = _DAT_80084540 + 6;          // scene-local: current scene PROT base + 6
} else {
    _DAT_8007BAB8 = _DAT_8007BC64 - 2000;        // global pool: separate base
}
_DAT_8007BAB8 = _DAT_8007BAC8 + _DAT_8007BAB8;   // final PROT index
```

- `_DAT_8007BAC8` - set by op 0x35 sub-1 (the BGM ID from the script).
- `_DAT_80084540` - current scene's PROT base index (set by the field loader; offset +6 lands at the per-scene BGM block).
- `_DAT_8007BC64` - global BGM pool base for IDs ≥ 2000.
- `_DAT_8007BAB8` - final PROT index, consumed downstream by the asset loader.

So:
- `bgm_id < 2000`: scene-local - lives at PROT `current_scene + 6 + bgm_id`. Different scenes have different BGM at the same script ID. Rare in retail: scenes carry almost no local SEQ data (`teien` is the one scene with a local copy).
- `bgm_id ≥ 2000`: global - lives at PROT `_DAT_8007BC64 + bgm_id - 2000`. The global pool is the **`music_01` bank** (extraction `990..=1071`), whose slot order is the **debug sound-test order** - so `2000 + i` plays sound-test track `i` and every global id resolves to a curated human name. Pinned by the per-scene op-`0x35` census joining ids to their scenes' known music (`town01` starts `2016` = "Rim Elm theme"); see [music-tracks](../reference/music-tracks.md#the-disc-side-join-the-music_01-bank-is-the-sound-test-order). Engine resolver: `legaia_engine_core::music_labels`.

The "table" *is* the [CDNAME.TXT name map](../formats/cdname.md)'s per-scene block layout. There's no separate BGM index in `SCUS_942.54`.

## Helper functions

A growing set of small leaf helpers in the dispatcher's call graph are pure arithmetic - no globals, no overlay calls - so they get clean-room ports in [`crates/engine-vm/src/field_helpers.rs`](../../crates/engine-vm/src/field_helpers.rs) instead of host hooks. The dispatcher arms call into them directly.

| Helper                  | Original          | Source dump                              | Used by                                    |
|-------------------------|-------------------|------------------------------------------|--------------------------------------------|
| `packet_length`         | `FUN_8003CA38`    | `ghidra/scripts/funcs/8003ca38.txt`      | `0x4C nE sub-1`, `0x49`                    |
| `party_flag_test`       | `FUN_8003CE64`    | `ghidra/scripts/funcs/8003ce64.txt`      | `0x4C nC sub-1` (host-side)                |
| `small_table_search`    | `FUN_80042EE0`    | `ghidra/scripts/funcs/80042ee0.txt`      | `0x4C nD sub-C/E`                          |
| `load_u16_le`           | `FUN_8003CE9C`    | `ghidra/scripts/funcs/8003ce9c.txt`      | `0x4C nC sub-5/6`, `nD sub-0/1`, `nE sub-B`, `n8 sub-1/6/B/D`, `nE sub-8` |
| `load_u24_le`           | `FUN_8003CEB8`    | `ghidra/scripts/funcs/8003ceb8.txt`      | `0x4C nE sub-5` (XP add), `n8 sub-1`, `nE sub-7` |
| `load_u32_le`           | `FUN_8003CED8`    | `ghidra/scripts/funcs/8003ced8.txt`      | 32-bit immediate decoding                  |
| `tile_center`           | inline (multi-arm) | dispatcher lines 6534, 7202, 7790, …    | `0x4C nE sub-3/4`, MOVE_TO, dialog spawn   |

**`packet_length(buf)`** - measures one variable-length packet of the in-game text encoding. Walks `buf` until any byte `<= 0x1E` (terminator); bytes `>= 0x1F` count as 1 each; bytes whose top nibble is `0xC` consume the next byte unconditionally and count as 2 (escape sequence). The returned count does *not* include the terminator. The dispatcher adds the opcode-prefix bytes and terminator separately when computing the PC delta.

**`party_flag_test(idx, flags)`** - reads bit `idx` of a packed bit array. Bit ordering is MSB-first per byte (bit 7 of `flags[0]` is index 0). Returns `0xFF` when set, `0` otherwise. Out-of-range indices return `0` (the original would read uninitialised bytes; engine callers have already validated bounds by the time they reach this helper). The dispatcher exposes the trigger-flag bank to `0x4C nC sub-5/6` via the `op4c_n_c_party_flag_test(flag_idx)` host hook (the dispatcher reads the index via `load_u16_le` then asks the host whether that bit is set), so the helper itself ends up referenced both directly (sub-1) and indirectly (sub-5/6 via the host).

**`small_table_search(needle, table, lo, hi)`** - searches `table[i * 2]` (stride 2, low byte of each short) for `needle` across indices `[lo, hi)`. Returns the matching index or [`SEARCH_NOT_FOUND`](../../crates/engine-vm/src/field_helpers.rs) (`0x100`) on miss. Negative bounds or `lo >= hi` produce `SEARCH_NOT_FOUND` without scanning.

**`load_u16_le(buf)` / `load_u24_le(buf)` / `load_u32_le(buf)`** - the LE byte-load family. Each helper assembles its result from sequential bytes (`b0 | (b1 << 8) | …`) and returns 0 for missing bytes (matching the dispatcher's `try_get`-style operand reads). The 24-bit version is paired with `sign_extend_24(value)` for the few opcodes (notably `0x4C nE sub-5`'s XP-add) that need a signed 24-bit immediate.

**`tile_center(b)`** - the field VM's grid-byte → world-coord conversion. Formula: `b == 0` returns 0; otherwise `(b & 0x7F) << 7 | 0x40`, plus `0x40` if the high bit is set. The original inlines this conversion in nine separate dispatcher arms (most prominently `0x4C nE sub-3/4` for camera-anchored teleport / bbox queries, MOVE_TO at op 0x23, the scene-change entry tile at op 0x3F, and the position-broadcast `0x4C nC sub-F`). Lifting it to a shared helper avoids the closure-per-arm pattern that drift-prone copy-paste was producing - round 18 introduced the helper and migrated `nE sub-4`'s closure to it; future arms can pick it up directly.

The Rust ports are exhaustively tested (39 tests covering escape sequences, terminator placement, bit ordering, search bounds, LE byte assembly across short / full-width / over-long buffers, and tile-center high-bit and zero-input edge cases). Tests live alongside the ports in `field_helpers.rs`.

## Field dialogue has no opcode

There is **no dedicated "open dialogue" field-VM opcode.** Talking to a field
NPC is the **interaction pipeline**, not a text-carrying instruction:

1. **Trigger** - the field-interact op (`0x3E` with `op0 < 100`) arms the actor's
   interaction context: it sets `sys_ctx[+0x94]` to the actor's interaction-script
   pointer (`scene_data + op1*stride + 1`) and `sys_ctx[+0x8a] = 1`. (`0x3E` with
   `op0 >= 100` is the door-warp; `0x3F` is the named scene-change - neither is
   dialogue.)
2. **Text source** - the dialogue text is the **actor's own inline
   interaction-script MES** at `actor[+0x90] + actor[+0x9e]` (the actor's script
   buffer base + the running text offset). Confirmed by `FUN_80039b7c`, which sets
   the pager's text pointer `_DAT_801f3538 = *(actor+0x90) + (short)*(actor+0x9e)`.
   This is the same `0x1F`-lead / glyph stream the placement classifier finds
   structurally.
3. **Display** - the per-frame **actor-dialog SM `FUN_80039b7c`** advances
   `actor[+0x9c]` through `0 → 1 → 2` in lockstep with the pager state
   `_DAT_801f2734`, walking MES glyph bytes (the `0xC0`-stride / `< 0x20`-terminator
   rule), and feeds the **dialog pager `FUN_801D84D0`** (line-pointer array
   `&DAT_801f3540[line]`, line count `_DAT_801f2740`). `FUN_80039b7c` runs per
   frame, per entity, from the entity SM `FUN_801DA51C`'s interaction tail.

   The engine ports this SM as `engine_core::inline_dialogue` /
   `World::step_inline_dialogue` (PORT `FUN_80039b7c`): it drives the actor's
   inline interaction script through the **real field VM** (`vm::field::step`),
   executing the control bytecode between text segments (story-flag tests,
   `SET`/`CLEAR`, scene changes) and pausing at each `0x1F` segment to show a
   box, applying a menu choice's relative jump (`FUN_80038050`) so the branch
   handler's side effects run before its reply. Gated by `World::use_vm_dialogue`
   (default `false` at the engine-core level so unit-test worlds keep the simple
   path; the shell's `play-window` sets it **on by default**, with
   `--simple-dialogue` opting back into the simplified `OwnedDialogPanel`
   typewriter). See [`formats/mes.md`](../formats/mes.md#dialog-window-pager---fun_801d84d0).

   Interaction records are **resident conversation drivers**: each story-state
   branch exits by jumping to a shared tail that loops back to the top selector
   (town01's Val record - "hands are full" sets its own one-shot SysFlag, the
   next talks give "(Silence)", then the permanent line), and retail parks the
   context there until the next talk. The runner ends one conversation pass at
   that loop-back - a VM `Advance` jumping backward onto an already-executed PC
   (`InlineDialogue::visited`) - rather than replaying the branch forever; the
   map is cleared on every picker commit so menu records that re-emit their
   menu by jumping back after a branch reply still cycle.

An earlier engine model drove `0x3F → open_dialog(text_id, inline, …)`, which is
wrong twice over: `0x3F` is the named scene-change, and field dialogue is the
interaction-driven actor-text pipeline above, not an inline-text opcode. (The
`0x4C` nibble-5 sub-3/4 op - `FUN_801d65d8` - is an actor-script wait/sync,
**not** the dialog open/poll an earlier note assumed.)

**Engine wiring (re-grounded).** The clean-room engine now matches this:
`field_interact` (`0x3E` with `op0 < 100`) opens the interacted actor's inline
dialogue from `World::field_npc_dialog` (the per-actor inline interaction-script
text, keyed by `slot` = the actor's MAN record index, populated at field-scene
entry), via the host's `open_dialog` primitive. `0x3F` is now a **live named
scene-change** (`host.scene_transition_named` → `SceneHost::tick`), no longer a
dialog opener. The dialog-dismiss gate stays on the `0x4C` nibble-5 sub-4 poll.

## Connection to other crates

- [`crates/mdt`](../formats/mdt.md) - opcode `0x22` `EXEC_MOVE` drives the move-table consumer at `FUN_800204F8`. Move IDs in scripts feed straight into the .mdt parsers.
- [`crates/mes`](../formats/mes.md) - field **dialogue** has no dedicated opcode (see [§ Field dialogue](#field-dialogue-has-no-opcode)): it is the **actor's inline interaction-script MES text**, shown by the per-frame actor-dialog SM (`FUN_80039b7c`) + pager (`FUN_801D84D0`), triggered by the **field-interact op** (`0x3E` with `op0 < 100`). The text `crates/mes` parses is that inline `0x1F`/glyph stream. (Opcode `0x3F` is the named scene-change, not a dialog opener.)
- [`crates/anm`](../formats/anm.md) - opcode `0x34` sub-op 3 plays 3D animations via `func_0x800252EC` - likely the ANM consumer.
- [`crates/engine-vm`](../../crates/engine-vm/src/field.rs) - destination for the clean-room Rust port. Adds a `field_vm` module sister to the existing actor VM. Reuses the `Host` trait pattern.

## Decompile quirks worth knowing

- **`switchD_801e00f4::default()` is misleading**. Ghidra renders the function-epilogue tail block as a synthetic function call; in the original asm, opcodes that "fall through to default" actually advance `param_2` via the `addiu s8, s8, N` instruction in the **MIPS branch-delay slot** of the `j 0x801df09c` jump. So 0x39, 0x3B, 0x44, 0x4C and friends DO advance the PC - just not in a way the C-level decompile makes obvious. Always check the raw asm before deciding "this opcode doesn't advance".
- **`LAB_801df09c`** is just `j 0x801e3628; move v0, s8` - return `s8` unchanged. Most callsites jump there with an `addiu s8, s8, N` in the **delay slot of the j**, supplying the per-callsite PC delta. **`code_r0x801df098`** is the *preceding* instruction `addiu s8, s8, 0x2` - jumping there gives PC += 2 with no per-callsite delta. **`switchD_801e0f24::caseD_4`** has its entry at `0x801df098` and so always does PC += 2 then return.
- **`LAB_801e00b8` = `addiu s8, s8, 0x3; j 0x801e00bc`**. **`LAB_801e00bc` = `j epilogue`** with no advance, used by paths that already incremented `s8` upstream.
- **0x42 mode 0 jump-take target** is `pc + 3 + LE_u16(operand[2..4])` (non-extended), found via the join point `LAB_801e35fc: return iVar18 + uVar31 + iVar24` - not the obvious `pc + 2 + delta`.
- **Relative-jump deltas wrap at 16 bits.** Each script's PC is stored as a signed 16-bit value (`*(short *)(ctx + 0x9e)`), so every relative branch (`0x26` JMP_REL, the `0x7x` flag-TEST conditional jump, `0x42` COND_JMP, the `0x4E` compare jumps) computes `(base + delta) mod 0x10000`. A delta with the high bit set is a **backward** jump, e.g. `0xFFFE` = -2 - the per-frame "park here" wait loop idiom (`[21] [26 FE FF]` ping-pongs two bytes until a story flag flips a guarded TEST). Computing `base + delta` in a wider int without the 16-bit truncation turns every backward jump into a `+0xFFxx` forward overrun, the "PC runs away to 0x10102" symptom that derails a script after its first wait loop. The clean-room port models this with a `rel_jump(base, lo, hi)` helper that wraps in `u16`.
### Intra-function label catalogue

`FUN_801de840` is a 17.5 KB function. Several `iVar = FUN_801xxxxx(); return iVar;` patterns in its C decompile look like calls into separate helpers but are actually **intra-function `j` targets** that Ghidra promoted to fake function names. Each label is a `addiu s8, s8, N; j epilogue` block (or a small variant); calling "into" it just supplies the PC delta and falls through to the dispatcher's tail.

Use this table as the lookup when interpreting the dump:

| Label | Aliases in C decomp | Semantic |
|---|---|---|
| `0x801df098` | `code_r0x801df098`, `switchD_801e0f24::caseD_4` | `addiu s8, s8, 0x2; j 0x801df09c` → **PC += 2** |
| `0x801df09c` | `LAB_801df09c`, `switchD_801e00f4::default()` | `j 0x801e3628; move v0, s8` → **PC unchanged** (function epilogue) |
| `0x801df8dc` | `FUN_801df8dc()` (lines 6250, 6284, 6384, 6449) | `addiu s8, s8, 0x6; j epilogue` → **PC += 6** |
| `0x801dee50` | `LAB_801dee50` | "halt-acquire failed" path - **halts at PC** (resets to loop start) |
| `0x801e00b8` | `LAB_801e00b8` | `addiu s8, s8, 0x3; j 0x801e00bc` → **PC += 3** |
| `0x801e00bc` | `LAB_801e00bc` | `j epilogue` - **PC unchanged** for callers that already did `addiu s8, s8, N` upstream |
| `0x801e212c` | `code_r0x801e212c`, `FUN_801e212c()` (lines 4749, 4772, 7285) | `return param_2 + 7;` → **PC += 7** |
| `0x801e35fc` | `LAB_801e35fc` | Join point: `return iVar18 + uVar31 + iVar24` → **PC = pc + 3 + LE_u16(operand[2..4])** for 0x42 mode 0 |
| `0x801e3614` | `FUN_801e3614()` (lines 7252, 7416) | `addiu v0, v0, -2; j 0x801e3624; addu s8, s8, v0` → **PC = s8 + skip - 2** (= `pc + 5 + skip` in the standard 0x4D / nE sub-4 BBOX outside-box context) |
| `0x801e3620` | `code_r0x801e3620`, `FUN_801e3620()` (lines 5021, 6606, 6923, 6928) | `iVar45 = param_2 + 4; ... break;` → **PC += 4** |

Pitfalls when verifying:

1. The misleadingly-named dump file `ghidra/scripts/funcs/overlay_0897_801e3620.txt` shows entry `0x801e3578` - the address `0x801e3620` is just inside that function's epilogue (`lw ra, 0x14(sp)`). The dump filename uses Ghidra's call-site rendering, not the actual entry. Same trap for `overlay_0897_801e212c.txt` if you ever generate one.
2. **Always cross-check `grep -n "0x<addr>" overlay_0897_801de840.txt`** before treating an `FUN_xxxxxxxx` reference as a separate function. Inside the FUN_801de840 dump, `j 0x<addr>` and `beq …, 0x<addr>` instructions reveal intra-function targets that Ghidra mis-promotes.
3. The C decomp sometimes collapses sub-op-first dispatch ordering. Round 11's 0x4C nibble-A bug was an inversion that only became visible after reading raw asm at `0x801e2568` (`bne a1, zero, 0x801e258c` dispatching on sub-op BEFORE the ctx[+0x10] check). When tests pass but the C reads suspicious, walk the asm.

A standing audit pass - picking 5 random ported sub-ops and cross-checking against the dump - turned up **no further inversion bugs** as of round 15.

## Disassembler tool: `field-disasm`

`crates/engine-vm/src/bin/field_disasm.rs` is a CLI that walks a field-VM bytecode buffer and prints one mnemonic per encoded instruction. The decoder mirrors the *width* logic of `crate::field::step` without executing host calls or mutating ctx state, so it's safe to point at any byte buffer - it stays linear, recovers from unknown sub-ops one byte at a time, and never follows jumps.

The binary ships in release archives and builds to `target/release/field-disasm`
(`cargo build --release`; or `cargo run -p legaia-engine-vm --bin field-disasm -- …`).

```bash
# Walk a raw script body, print each opcode + operand:
field-disasm file <PATH>

# Detect a [u16 count][u16 offsets[count]] prescript at the start of <PATH>
# and walk every record body individually:
field-disasm scene-event-scripts <PATH> [--summary]

# Walk every PROT.DAT entry and report 0x4C 0xE2 byte-pattern hits with
# their CDNAME label and decoded fmv_id (filtered to the retail valid
# range 0..=8 unless --no-filter is passed; the FMV dispatch table at
# 0x801D0A6C carries 23 32-byte slots - the nine retail slots 0..=8
# dispatch every movie on the disc, slots 9..=22 point at dev files).
# --prot takes an extracted PROT.DAT, not a raw .bin disc image:
field-disasm scan-prot --prot <PROT.DAT> --cdname <CDNAME.TXT> --bytewise
```

The library exposes `legaia_engine_vm::field_disasm::{decode, LinearWalker, find_fmv_triggers, format_instruction}` for downstream tooling. `decode()` returns `Result<Insn, DisasmError>`; `LinearWalker` is the iterator shape that wraps `decode` plus single-byte recovery. The `InsnInfo::MenuCtrl { kind: MenuCtrlKind::FmvTrigger { fmv_id }, .. }` variant carries the operand of the `0x4C 0xE2` op for callers who want to grep for cutscene triggers across the corpus.

> **CAVEAT - `scene-event-scripts` / `scan-prot` walk a NON-field-VM
> structure.** The `0xFFFF 0x0000` lead is the stager-record header
> (`model_sel = -1`), and the `scene-event-scripts` mode skips it before
> walking the record body - but those records are **move-VM stager records**,
> not field-VM bytecode (see the "On-disc form" note above), so the field-VM
> disassembly is mostly `decode error` with coincidental matches. Any
> `0x4C 0xE2` FMV trigger these modes report inside a prescript record is a
> **false positive** (a stager-record byte that equals `0x4C` followed by one
> equal to `0xE2`). The genuine FMV triggers are the literal `fmv_id` operands
> in the scene MAN scripts, recovered statically for all eight trigger scenes
> (`man_field_scripts::scene_fmv_triggers`; see
> [`cutscene.md`](cutscene.md)), plus the disc-decoded `fmv_dispatch` table.

## FMV-trigger sites - exhaustive backward sweep

A grep across every Ghidra dump in the corpus for writes to the global game-mode word `_DAT_8007B83C = 0x1A` (the `StrInit` mode that boots the str_fmv overlay) finds only the field-VM op plus the title-attract path (pinned at two PCs in the title overlay). The sites are codified in [`legaia_engine_vm::cutscene_trigger`](../../crates/engine-vm/src/cutscene_trigger.rs) as `FMV_TRIGGER_SITES`:

| Label | Function | Mode-write addr | FMV-id source | Trigger condition |
|---|---|---|---|---|
| `field_vm_op_4c_e2` | `FUN_801DE840` | `0x801E3104` | `decode_u16_be(pc+1)` from field-VM bytecode | Field-VM hits the byte sequence `0x4C 0xE2 lo hi`; reached via JT chain `0x801CEE60` (high nibble 0xE) → `0x801CF008` (low nibble 0x2) → label `0x801E30E4`. |
| `title_attract_loop` | `FUN_801DE234`, case `0x10` | `0x801E0F50` | Hardcoded `0` (= `MV1.STR`, intro) | Title-screen idle countdown `DAT_801ef16c` underflows. |
| `title_tick_inline` | `FUN_801DD35C` | `0x801DDCF0` | Hardcoded `0` (= `MV1.STR`, intro) | Same attract countdown, inline fall-through past the decrement at `0x801DDCCC` - the PC a live watchpoint reports. |

**`FUN_801E30E4` has zero static callers.** It is a label inside `FUN_801DE840`, not a callable subroutine. Ghidra promotes it to a `FUN_` symbol because the JT entry at `0x801CF008[2]` resolves there; the actual control flow is the dispatch chain above. A direct `grep -rn 'jal 0x801e30e4' ghidra/scripts/funcs/` returns zero matches.

The per-scene trigger assignment is **disc-sourced**: the `0x4C 0xE2` ops live LZS-compressed inside each scene's MAN (which is why a raw bytewise PROT scan misses them - it finds only `PROT[371] taiku, fmv_id=5` in a non-MAN structure). Walking the decompressed partition-1 scripts recovers the literal `fmv_id` operands for all eight trigger scenes (`town01`, `garmel`, `deroa`, `chitei2`, `dohaty`, `town0d`, `uru`, `jouine`).
The FMV overlay's own seven-label scene list at `0x801CE8AC` (`town0b`, `map01`, `chitei2`, `map02`, `jou`, `uru2`, `town0e`) is the **post-play return-scene** table the master dispatch `FUN_801CEA3C` hands control to after playback, not the trigger-scene set. See [`cutscene.md`](cutscene.md) and [`../formats/str-fmv-table.md`](../formats/str-fmv-table.md#per-scene-trigger-assignment-disc-sourced).

## See also

**Reference** -
[Actor VM](actor-vm.md) ·
[Move-table VM](move-vm.md) ·
[Motion VM](motion-vm.md) ·
[Effect VM](effect-vm.md) ·
[Scene v12 table](../formats/scene-v12-table.md)
