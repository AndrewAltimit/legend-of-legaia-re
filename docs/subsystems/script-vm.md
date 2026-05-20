# Field / event script VM

The bytecode interpreter that drives Legaia's overworld scripting - NPC movement, dialog triggers, cutscene sequencing, story-flag manipulation. Lives in PROT entry **`0897_xxx_dat`** (the town/field overlay), at `FUN_801DE840`. ~17.5 KB / 4099 instructions / 357 outgoing calls - the largest function in the corpus.

> **Why "field/event"?** Each running script has its own context (a struct passed around as `ctx_ptr`); contexts can target the player, NPCs, the camera, or "system" channels. The same VM drives both the per-frame field tick and event/cutscene sequences.

The decompiled source is at `ghidra/scripts/funcs/overlay_0897_801de840.txt`. References to `func_0x80xxxxxx` are calls into `SCUS_942.54`; `FUN_801xxxxx` are sister functions inside the 0897 overlay.

## On-disc form: `scene_event_scripts`

The on-disc carrier for field-VM bytecode is the `scene_event_scripts` /
`scene_scripted_asset_table` PROT-entry shape (see
[`formats/scene-bundles.md`](../formats/scene-bundles.md)). Both share a
`[u16 count][u16 offsets[count]]` prescript at offset 0; each offset points
to one record's bytecode, and most records open with the four-byte
`0xFFFF 0x0000` frame-divider sentinel.

99 of 124 CDNAME scenes carry an event-script entry (~80% of the disc's
scenes). Per-scene record counts run from 1 to 71+ records.

The Rust API for walking these records lives in `legaia-asset`:

```rust
use legaia_asset::scene_event_scripts;
let ranges = scene_event_scripts::record_ranges(buf)?;
for (start, end) in ranges {
    let record_bytes = &buf[start..end];
    // record_bytes opens with the frame divider 0xFFFF 0x0000 in most cases
}
```

`legaia-engine-core::scene::Scene::find_event_scripts()` resolves the
correct entry within a loaded `Scene` and exposes per-record byte ranges.
`legaia-engine-core::world::World::load_field_record()` skips the leading
frame-divider sentinel and resets the VM cursor.

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
| +0x10 | u32 | Flag word. Bit 0x400 = "halted". Bit 0x100 has special handling in op 0x31. Bit 0x1000000 toggles op 0x22 behavior. Bit 0x20200 / 0x20000000 gate the Y-collision lookup in op 0x23. |
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
| 0x37 / 0x41 / 0x47 | `YIELD` family | 1 byte (0x47 is 4 bytes) | Save PC into `ctx[+0x94]`, clear `ctx[+0x54]`, set `ctx[+0x10]` bit 0x400 (HALT). If ctx is the player, also propagate the halt to the caller's ctx. |
| 0x38 | `CAM_CFG` | `[38, op0, op1]` | Camera/visual register write. If `op1 & 0x7F == 0`: simple path - copy `*(short*)(0x80073F04 + (op0 & 0xF) * 2)` into `ctx[+0x26]`. Else: halt-acquire path - same predicate as op 0x43 sub-0/1/A/B (`saved_pc != 0 \|\| ctx==player`) AND (`!(flags & 0x400) \|\| scene_busy`); on success set HALT + saved_pc + wait_accum=0 (mirror to caller when ctx is player), yield with `resume_pc = pc + 3`; on fail fall through to dispatcher default. |
| 0x39 | `PLAY_SFX` | `[39, sfx_id]` | Calls `func_0x8004313C()` then `func_0x800421D4(sfx_id, 1)`. |
| 0x3A | `ADD_MONEY` | `[3A, b0, b1, b2]` | 24-bit signed delta: `_DAT_8008459C += sext24(operand)`. Clamp to `[0, 9999999]`. |
| 0x3B | `SET_ITEM_COUNT` | `[3B, slot, count]` | Set inventory entry: `*(byte*)(0x80084340 + (slot & 0xF) + (slot >> 4) * 0x414) = count`, then `func_0x80042558()` to refresh inventory display. Inventory pages of 0x414 bytes. |
| 0x3C | `PARTY_ADD` | `[3C, char_id]` | Add character to party (sorted insertion into `_DAT_80084598..` array, count at `DAT_80084594`). Caps at 4 members. Updates `_DAT_8007B8F8` (party leader) when count was 0. Calls `FUN_801DE190()` (refresh display). Special: if count becomes 2 with `_DAT_80084598 == 0x100`, calls `func_0x800423E0()` and returns. |
| 0x3D | `PARTY_REMOVE` | `[3D, char_id]` | Remove character (linear search, shift, count--). Updates leader if affected. Refresh via `FUN_801DE190()`. |
| 0x3E | `WARP / INTERACT` | `[3E, op0, op1, …]` | If `op0 == 0xFF` or `op0 < 100`: trigger field interact at index `op1` on system context (`func_0x8003C83C(0xFB)`); writes `sys_ctx[+0x94] = scene_data + op1 * stride + 1`, calls `func_0x8003CE08(0xE)`. Else (`op0 >= 100`): scene transition - `_DAT_8007BA34 = op0 - 100` (map id), `_DAT_8007B83C = 0x18`, clears `player[+0x10] & 0x80000`, calls `func_0x8003CE08(0xE)`. |
| 0x3F | `DIALOG` | `[3F, lo, hi, len, [len bytes inline], x, z, depth_id]` | Opens a dialog box. Reads 16-bit text id, copies `len` bytes from operand+3 into a local 16-byte buffer (null-terminated), calls `func_0x8001FD44(local_buf, text_id)` - the dialog/MES opener. Sets `_DAT_1F800394 \|= 0x40` ("dialog active" lock). Writes box position via `_DAT_80073EF4`/`_DAT_80073EF8` (formula `(b & 0x7F) * 0x80 + 0x40`, +0x40 if high bit). PC += 7 + len. |
| 0x40 | `DATA_BLOCK` | `[40, len, ...len bytes]` | Skips `len` bytes after header - embeds raw inline data. PC += 2 + len. |
| 0x42 | `COND_JMP` | `[42, mode, op1, op2, op3]` | Multi-mode conditional. `mode == 0`: test `_DAT_8007B8F4 & (1 << (op1 & 0x1F))` - if clear, return `pc + 5` (skip). `mode == 1`: test screen-mode (`_DAT_8007B850`) against `_DAT_801F28D0[op1*4]` (8-entry table) for `op1 < 8`, bit 0x20 for `op1 == 8`, 0x40 for 9, 0x80 for 10, 0x10 for 11; **`op1 >= 0xC` falls through to the unconditional take-jump path** (no test). `mode >= 2` hits the dispatcher's default arm - halts at PC. Successful jump target = `pc + 3 + LE_u16(op2,op3)`; skip target = `pc + 5`. |

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

#### 0x43 sub-0x10..0x15 - emitter setup family

Each dispatches into the `FUN_801F8xxx` particle/emitter cluster:

| Sub-op | Encoding | SCUS call | PC delta |
|---|---|---|---|
| 0x10 | `[43, 0x10, 19 bytes]` | `FUN_801F8004(operand+1)` (19-byte struct) | +21 |
| 0x11 | `[43, 0x11, 5 × u16]` | `FUN_801F8D4C(u0..u4)` | +12 |
| 0x12 | `[43, 0x12, 6 × s16]` | `func_0x800468A4(6, …)` - **dual call** when `words[2] > 0xFF`, with offset shifts `(+0xF0, _, -0xE0, _, +0x100, _)` and a 0x100 clamp | +14 |
| 0x13 | `[43, 0x13, 12 bytes]` | `FUN_801F88FC(operand)` - passes the whole 13-byte slice | +14 |
| 0x14 | `[43, 0x14, 4 × s16]` | `FUN_801F8E6C(s0..s3)` | +10 |
| 0x15 | `[43, 0x15, 12 bytes]` | `FUN_801F8F28(operand+1)` (12-byte struct) | +14 |

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

### 0x44-0x4F (counter / camera / render / state / move-block)

| Op | Mnemonic | Notes |
|---|---|---|
| 0x44 | `COUNTER` | `func_0x8003D064` 3-int return + `func_0x8003BDE0` - likely a per-frame counter / score / hit-counter tick. |
| 0x45 | `CAMERA` | Sub-dispatch on `op0 & 0xC0`: `0x00` = configure 10 sub-words, `0x40` = LOAD (`FUN_801DBC20`), `0x80` = SAVE (`FUN_801DE004`), `0xC0` = APPLY (`FUN_801DAB90` + `FUN_801DAA50` then absolute jump). |
| 0x46 | `RENDER_CFG` | Fog/render params. `op0 == 0x24` writes 4 bytes (DAT_1F8003E8-EB); else short 2-byte form. |
| 0x49 | `STATE_RESUME` | Multi-frame state machine on `_DAT_8007B450`: tristate (Idle / Armed / Done) with sub-cases 0..0xD. Done-state sub-6/8/9/C/D all jump through `LAB_801df898` for PC += 5. Done-state sub-0 walks an inline MES-shape payload via `func_0x8003CA38` (counts bytes > 0x1E with one-byte peek-extension for `0xCx` prefix bytes); `length = pbVar47[2]` selects the arg-stream length and PC advances by `5 + length + walked`. |
| 0x4A | `WAIT_FRAMES` | `ctx[+0x54] += scratch_delta; if (sum < operand) return; else PC += default`. Frame timer. |
| 0x4B | `ANIMATE` | Multi-keyframe setup. Writes `ctx[+0xB0+N] / +0xB8 / +0xC8`, sets `+0x10` bit 0x1000 (animation flag). PC += 3 + count*4. |
| 0x4C | `MENU_CTRL` | Outer-nibble-dispatched (16 sub-dispatchers). See below. |
| 0x4D | `BBOX_TEST` | Inside-box advances PC by 7; outside-box jumps to `pc + header_size + 4 + LE_u16(operand[4..6])` via `FUN_801E3614`. |
| 0x4E | `INVENTORY_CMP` | Compare-and-jump across page-banked inventory state and party-money/XP banks. Sub-ops 0/1 (page-banked compare, 7 bytes), 2/3/5/6/7/8/9 (absolute jump to operand[2..4]), 10/11 (party-bank u32 compare, 9 bytes), 12..=15 (no test, fall through default arm with PC += 7). Sub-op 4 calls `func_0x80056798` (BIOS Rand thunk = `jr 0xA0; t1=0x2F`) and uses the returned value as the next PC; ported as a side-effect-only host hook (`FieldHost::op4e_sub4_bios_rand`, default returns 0) - almost certainly a dev/debug stub. |
| 0x4F | `SCENE_REGISTER_WRITE` | Writes three `u16` values to `_DAT_801C6EA4 + 0x10/+0x12/+0x14`. |

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
| 7 | 0x70..0x7F | **Collision-grid rectangular wall paint** (7-byte op `[4C, 0x7s, b1, b2, b3, b4, mask]`; handler `0x801e1c64`). Writes the walkability grid at `_DAT_1f8003ec + 0x4000` (the per-scene field buffer; one byte per 128-unit tile, **high nibble = 4 sub-cell wall bits**), the same grid the locomotion collision check `FUN_801cfe4c` reads. Paints the rectangle `col ∈ [b1, b3+1)`, `row ∈ [b2+1, b4+2)` at index `_DAT_1f8003ec + col + row*0x80 + 0x4000`. Sub-op `s` (= `op0 & 0xF`): `0` = clear walls (`byte &= 0x0F`, make walkable), `1` = block all (`byte |= 0xF0`), `2` = clear `mask` bits (`byte &= ~(mask << 4)`), `3` = set `mask` bits (`byte |= mask << 4`). This is the on-disc **source** of field collision: walls are authored as inline operands in the scene event script, not a separate grid blob (same "data is an operand of the op" pattern as encounters / tile-board). (The byte's low nibble is a separate floor-elevation tier; the sibling `_DAT_1f8003ec + 0x8000` grid is a per-tile object/attribute map, not a terrain-flag grid.) See [`field-locomotion.md`](field-locomotion.md). |
| 8 | 0x80..0x8F | Large multi-purpose dispatcher (party page mirror, conditional jump on `+0x68`, …). Sub-1 (round 18, 9-byte) sets actor model + animation frame: `[4C, 0x81, m0..m2, anim_lo, anim_hi, frames_lo, frames_hi]` decodes via [`load_u24_le`](#helper-functions) + `load_u16_le×2`; host applies the immediate-or-tween path based on its actor pool state. Sub-6 (round 18, 15-byte) is `[4C, 0x86, x..rz, actor_id]` - six 16-bit position+rotation values plus a 1-byte actor selector; host returns whether the actor was found, PC always += 15. Sub-7 (`func_0x8003CF40(_DAT_8007C34C, &LAB_801E5154)`) registers an actor-list callback then halts at PC via the dispatcher default. Sub-9 writes `_DAT_80073F00 = i16(operand[1..3])` and advances by 4 (the dump's "FUN_801E3620 dispatch" was Ghidra mis-rendering an internal `goto code_r0x801e3620` label; see the gotcha note below). Sub-B (round 18, 5-byte) is a conditional jump: `[4C, 0x8B, type_byte, target_lo, target_hi]` jumps to absolute u16 if any actor of `type_byte` is active, else PC += 5. Sub-D (round 18, 6-byte) is a tristate per-character actor-search: `[4C, 0x8D, char_idx, marker, target_lo, target_hi]` returns one of [`ActorSearchResult::EmptySlot`](../../crates/engine-vm/src/field.rs) (advance 6), `Found` (jump to u16 at +3..=4), or `NoMatch` (halt). Sub-5/E/F share a single halt-acquire idiom: writes `ctx.saved_pc = pc`, clears `wait_accum`, sets the halt bit, then halts. |
| 9 | 0x90..0x9F | Fade family (sub-0..2 via `FUN_801DDE34`), 16-word table copy (sub-0xE), callback registration (sub-0xF: `func_0x8003CF40(_DAT_8007C34C, &LAB_801DA930)` then halt at PC). |
| A | 0xA0..0xAF | Conditional jump on flag bit. Sub-0 reads `ctx.flags`, sub-1 reads `ctx.local_flags`, sub-2 reads the global story flag word. Bit SET → take absolute jump from operand[2..4]; bit CLEAR (or sub-3..=0xF) → skip 5 bytes. (The asm dispatches on sub-op first at 0x801e2568, so sub-3..=0xF skip both the per-bank check and the take-jump path.) |
| C | 0xC0..0xCF | Small per-actor / per-scene writes (slot table, sub-tile broadcast, sound trigger, `field_74` XOR). **All 16 sub-ops are now ported.** Sub-0 is a 2-byte move-table cancel via `func_0x800204F8`; the host gates on whether a move is currently active. Sub-1 is a 1-byte trigger-flag record-array reset: walks `_DAT_80073ED8[..count]` (stride `0xB`), tests each record's 16-bit index via [`party_flag_test`](#helper-functions), writes the inverted bit to `record[0]`; PC always += 2. Sub-3 is a 2-byte script-table teleport (resolves `func_0x8003C8F0(field_50, 0)` then writes `world_x/z` via the standard tile-center `b * 0x80 + 0x40` formula). Sub-5/6 are 4-byte conditional-jump pair (jump-if-zero / jump-if-nonzero): both read a 16-bit flag index via [`load_u16_le`](#helper-functions), query the host's trigger-flag bank, and advance PC += 4 in both branches (the original's "joined" tail at `LAB_801E28C4` returns `param_2 + 4` either way). Sub-D is a 2-byte script-context allocator that registers an actor with `FUN_8003CF04` then halts at PC. Sub-0xF is a position broadcast: 4-byte `[4C, 0xCF, b1, b2]` resolves each byte to either the actor's world coord (`0xFF`), the tile-center conversion (non-zero), or 0; advances by 4. Sub-9 is a 2-byte global-pair compare gate: PC += 2 unless `_DAT_8007BAB8 != _DAT_8007BA9C`, then halts. |
| D | 0xD0..0xDF | Party state + inverted-Y mirror cluster. Sub-0 (round 18, 6-byte) is a field SE trigger with a conditional u16 pair: `[4C, 0xD0, a_lo, a_hi, b_lo, b_hi]` decodes both via [`load_u16_le`](#helper-functions); the original gates `func_0x8002B994(a, b)` on three flag globals (`_DAT_8007B874`, `_DAT_800846D0`, `_DAT_800846D4`); PC always += 6. Sub-1 (1-byte) is a linked-list lookup gate via `FUN_8003CF04(_DAT_8007C34C, FUN_801DC0BC)` - host returns `Some(new_pc)` for the `LAB_801E360C` ce9c-jump path or `None` for PC += 4 on miss. Sub-2 (2-byte) calls the channel resolver `func_0x8003C83C` and conditionally spawns a script context, then halts at PC. Sub-6 mutates `ctx.field_74`: 3-byte `[4C, 0xD6, b1]`, if `b1 == 4` clears top bit only, else sets bit 0x80000000 + shifts `b1` into the top byte; halts at PC. Sub-7 (1-byte) registers a `FUN_801DC0BC` list-walk callback then halts at PC. Sub-8 (9-byte) is a synchronous-spawn actor allocator: `[4C, 0xD8, vdf_idx, tmd_lo, tmd_hi, kind_lo, kind_hi, var_lo, var_hi]` decodes to `(vdf_idx: u8, tmd_idx: i16, kind: u16, variant: u16)` and routes through host hook [`FieldHost::op4c_n_d_sub8_call_d77f4`] (overlay-resident `FUN_801D77F4`, see `ghidra/scripts/funcs/overlay_cutscene_dialogue_801d77f4.txt`); host writes `actor[+0x3C] = kind` and `actor[+0x3E] = variant` on the allocated slot. Unlike the queue-based `0x4C 0x80` halt-acquire path, the spawn is synchronous - the host emits `FieldEvent::ActorSpawned` directly, with no intervening `pending_actor_spawns` queueing. PC always += 9. Sub-0xB (13-byte) calls `FUN_801E57F0(operand)` then PC += 13 (the call site falls through to `LAB_801E2EA0: return param_2 + 0xD`); the helper itself was not decompilable (Ghidra's dump for that address shows data masquerading as code). Sub-0xC (5-byte) and sub-0xE (5-byte) both call [`small_table_search`](#helper-functions) on a 1-byte needle, then loop over the active party records (stride `0x414`, byte at `+0x196`); on hit, both advance via the `LAB_801E360C` ce9c-jump path; sub-0xC additionally writes the matching slot. Both miss with PC += 5. |
| E | 0xE0..0xEF | Misc scene writes + emitter helpers. Ported sub-ops: 0 (3-way state write, halt), 1 (variable-length text balloon spawn - the field VM's most user-visible opcode, drives the in-game text-encoding pipeline alongside [`crates/mes`](../formats/mes.md); PC = `pc + 3 + packet_length(operand+1)` via [`packet_length`](#helper-functions)), **2 (FMV trigger, 7-byte: `[4C, 0xE2, lo, hi, _, _, _]`** - reads `(s16)bytecode[2..3]` as the FMV index, writes to `_DAT_8007BA78`, and pokes `_DAT_8007B83C = 0x1A` (next game mode = 26 = `StrInit`); the runtime str_fmv overlay then plays the resolved `MV*.STR`. The trailing 3 bytes are reserved by the dispatcher's PC math but unused. See [`subsystems/cutscene.md`](cutscene.md#field-vm-fmv-trigger-op) for the full Ghidra trace.), 3 (round 18, 2-byte camera-anchored teleport: `[4C, 0xE3, actor_id]` syncs the resolved actor's position to the active camera), 4 (9-byte BBox collision query - each operand byte goes through [`tile_center`](#helper-functions); halts via `FUN_801E3614` when the actor is outside the bbox, otherwise PC += 9), 5 (5-byte XP add - reads a 24-bit signed delta via [`load_u24_le`](#helper-functions) + `sign_extend_24`, then host clamps to `[0, 9999999]` and triggers party-stats refresh), 6 (FUN_801D8280, 8-byte), 7 (round 18, 7-byte camera animate: 24-bit LE target + 16-bit LE duration; host schedules `func_0x8003C5F0` tween or instant-write when duration is 0), 8 (round 18, 10-byte camera zoom: four 16-bit LE values for `zoom_x`/`zoom_y`/`zoom_z`/`mode`, dispatching to the camera struct's default zoom triplet (`+0x4C/+0x4E/+0x50`) for `mode=0`, or per-mode actor flag writes for `mode=1/2/3`), 9 (clear `_DAT_8007B9C4` then PC += 2 via `caseD_4`), 0xA (call `func_0x8003C7EC` then halt), 0xB (5-byte conditional actor lookup with embedded jump target - host returns `Some(())` to take the resolved-actor "pc + 5" path or `None` to jump to the absolute u16 at `operand+2..=3`; jump target read via [`load_u16_le`](#helper-functions)), 0xC (capture FUN_801DDF48 return, 2-byte), 0xD (set `_DAT_8007BA66`, 3-byte), 0xE (snapshot `_DAT_80084570 → _DAT_800845DC`, 2-byte). All non-`P` cells in the matrix above are now ported. |
| F | 0xF0..0xFF | Only `op0 == 0xFF` valid (pass-through); other sub-ops print `"SUB_CMD_0F_ERROR"` |

The full per-sub-op table is in the field-VM dump (`overlay_0897_801de840.txt`). The clean-room port mirrors the dispatcher shape with host hooks per sub-cluster - see [`crates/engine-vm/src/field.rs`](../../crates/engine-vm/src/field.rs).

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

The `n8 sub-0` host hook (`FieldHost::op4c_n8_sub_0_actor_allocator`) receives `(count, tail)`: `count` is the byte at `operand+1` and `tail` is the raw bytecode slice from `operand+2` onward. The host walks `count` variable-length child-actor records out of `tail` using the [`packet_length`](#helper-functions) rule (`FUN_8003CA38`): bytes `<= 0x1E` terminate a record; bytes whose top nibble is `0xC` consume one extra byte. The parent script's PC always advances by 3 regardless of how many records were walked - the records remain embedded in the bytecode buffer and become the spawned actors' own bytecode (retail stores the per-actor bytecode pointer at `actor[+0x90]`). The engine-core implementation (`FieldHostImpl::op4c_n8_sub_0_actor_allocator`) splits the records, queues each one into `World::pending_actor_spawns`, and emits a `FieldEvent::ActorAllocate { records }` so engines can route them into their own actor pool.

Materializing the queued records into actor slots is a separate engine-side step. [`World::materialize_actor_spawns(start_slot)`] drains `pending_actor_spawns`, allocates the first inactive slot from `actors[start_slot..MAX_ACTORS]`, populates `Actor::spawn_record` with the raw bytecode bytes, and emits one `FieldEvent::ActorSpawned { slot, kind, variant, record }` per allocation. The retail allocator for this opcode (`overlay_world_map_801de840.txt:7080-7123`, case `8 sub-0`) allocates from pool `0x801f28a0` and writes `actor[+0x90]` (bytecode start), `actor[+0x94]` (parent back-pointer) and `actor[+0x54] = 0`; it does **not** write `actor[+0x3C]` (kind) or `actor[+0x3E]` (variant), so the event's `kind = 0` / `variant = 0` match retail — this is a faithful zero, not a placeholder. The `0x4C 0xD8` path is the one that decodes explicit `(kind, variant)` u16 immediates and routes through `FUN_801D77F4`; the `0x4C 0x80` path is bytecode-only by design. When the slot range is exhausted, a `FieldEvent::ActorSpawnFailed { record }` event surfaces the dropped request instead.

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
| 9 | `_DAT_801C6EA4 + 0x4A` *or* delta-bank *or* abs-jump | Branched on two bits of `_DAT_1F800394`. |
| A | `_DAT_8007BCD0` | Plain global write or ramp. |
| B | `_DAT_8007BCD4` | Sister of A. |
| C | `_DAT_8007BCD8` | Sister of A. |
| D | `_DAT_8007B910` | Same shape but value is `(input * _DAT_8008457C) >> 12` (fixed-point scale; host owns the transform). |
| E / F | - | Inner switch's `default:` arm prints `"SUB_40_ERROR"` and routes via `switchD_801e00f4::default()` - halts at PC. |

Sub-9's tristate dispatch:

| Bit `0x02000000` | Bit `0x01000000` | Path |
|---|---|---|
| clear | clear | `Default` - write/ramp `_DAT_801C6EA4 + 0x4A` |
| clear | set | `AbsJump` - return `signed_16(operand)` as new PC |
| set | (ignored) | `Delta` - write/ramp both target slot **and** delta global at `_DAT_8007BCAC` |

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

## Default-case "extension" opcodes - the fourth flag bank

The default arm of the dispatcher checks `*pbVar43 & 0x70`:

- `0x50`: `func_0x8003CE08((*pbVar43 & 0x8F) << 8 | pbVar43[1])` - **SET bit**.
- `0x60`: `func_0x8003CE34(...)` - **CLEAR bit**.
- `0x70`: `func_0x8003CE64(...)` - **TEST bit** (returns `0xFF` if set, `0` if clear). When non-zero, the dispatcher consumes two more operand bytes (`pbVar43[2..4]`) as the post-test action target.

The three SCUS dispatchers all operate on the **same bitfield array based at `0x80085758`**. The disassembly of `FUN_8003ce64` (TEST) is `lui v1,0x8008; addiu v1,v1,0x4140` (`v1 = 0x80084140`) then `lbu v1, 0x1618(v0)` with `v0 = (idx >> 3) + v1`, i.e. the byte address is `0x80084140 + 0x1618 + (idx >> 3)` = `0x80085758 + (idx >> 3)`. Each does `index >> 3` to pick the byte and `0x80 >> (index & 7)` to pick the bit. So the `0x5x/0x6x/0x7x` opcode space encodes a 12-bit operand: the low 4 bits of the opcode plus the next operand byte form an 8-bit (1-byte) flag index - but with the "extended" prefix bit (0x80) preserved into the high bits, the addressable space is 12-bit, suggesting per-script-context banks within the same array. (An earlier draft mislabeled the base as `DAT_80086D70` by double-counting the `0x1618` displacement onto `0x80085758`; the Ghidra symbol `DAT_80085758` is itself `0x80084140 + 0x1618`, and the array is indexed directly from there - no further `+0x1618`.)

This is a **fourth flag bank** (per-script local at `ctx[+0x62]`, 32-bit globals at `_DAT_1F800394`, ctx flag word at `ctx[+0x10]` are the other three). It is **not** a wholly separate region: base `0x80085758` falls inside the story-flag RAM window `0x80085600..0x80085800` (at `+0x158`) and the bank extends past `0x80085800` (flag indices up to ~`0xFFF` reach `0x80085758 + 0x1FF`). In a retail SC save block the bank therefore lives at SC offset `0x1618` (= `0x200 + (0x80085758 - 0x80084340)`, via the `SAVE_GAME_DATA_RAM_BASE` formula in `crates/save`), overlapping the story-flag bitmap (`SC 0x14C0`, 512 bytes) and continuing to the inventory array (`SC 0x1818`). Seeding the engine's `World::system_flags` from `sc_block[0x1618..0x1818]` reproduces the live bank as of the save. Note this bank is **not** sufficient on its own to drive a scene's collision: see [`field-locomotion.md`](field-locomotion.md) - the `0x4C` nibble-7 wall paints reached through it are story-conditional collision *deltas*, not the base walkable grid.

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
- `bgm_id < 2000`: scene-local - lives at PROT `current_scene + 6 + bgm_id`. Different scenes have different BGM at the same script ID.
- `bgm_id ≥ 2000`: global - lives at PROT `_DAT_8007BC64 + bgm_id - 2000`. Shared across scenes (cutscene / title / event music).

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

**`tile_center(b)`** - the field VM's grid-byte → world-coord conversion. Formula: `b == 0` returns 0; otherwise `(b & 0x7F) << 7 | 0x40`, plus `0x40` if the high bit is set. The original inlines this conversion in nine separate dispatcher arms (most prominently `0x4C nE sub-3/4` for camera-anchored teleport / bbox queries, MOVE_TO at op 0x23, dialog spawn at op 0x3F, and the position-broadcast `0x4C nC sub-F`). Lifting it to a shared helper avoids the closure-per-arm pattern that drift-prone copy-paste was producing - round 18 introduced the helper and migrated `nE sub-4`'s closure to it; future arms can pick it up directly.

The Rust ports are exhaustively tested (39 tests covering escape sequences, terminator placement, bit ordering, search bounds, LE byte assembly across short / full-width / over-long buffers, and tile-center high-bit and zero-input edge cases). Tests live alongside the ports in `field_helpers.rs`.

## Connection to other crates

- [`crates/mdt`](../formats/mdt.md) - opcode `0x22` `EXEC_MOVE` drives the move-table consumer at `FUN_800204F8`. Move IDs in scripts feed straight into the .mdt parsers.
- [`crates/mes`](../formats/mes.md) - opcode `0x3F` `DIALOG` calls the dialog opener `func_0x8001FD44`. The bytecode inside the dialog buffer is what `crates/mes` parses.
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

```bash
# Walk a raw script body, print each opcode + operand:
cargo run -p legaia-engine-vm --bin field-disasm -- file <PATH>

# Detect a [u16 count][u16 offsets[count]] prescript at the start of <PATH>
# and walk every record body individually:
cargo run -p legaia-engine-vm --bin field-disasm -- scene-event-scripts <PATH> [--summary]

# Walk every PROT.DAT entry and report 0x4C 0xE2 byte-pattern hits with
# their CDNAME label and decoded fmv_id (filtered to the retail valid
# range 0..=8 unless --no-filter is passed; the runtime FMV-state table
# at 0x801D0A6C carries 12 slots — slots 5..=11 point at cut paths):
cargo run -p legaia-engine-vm --bin field-disasm -- scan-prot \
    --disc <PROT.DAT> --cdname <CDNAME.TXT> --bytewise
```

The library exposes `legaia_engine_vm::field_disasm::{decode, LinearWalker, find_fmv_triggers, format_instruction}` for downstream tooling. `decode()` returns `Result<Insn, DisasmError>`; `LinearWalker` is the iterator shape that wraps `decode` plus single-byte recovery. The `InsnInfo::MenuCtrl { kind: MenuCtrlKind::FmvTrigger { fmv_id }, .. }` variant carries the operand of the `0x4C 0xE2` op for callers who want to grep for cutscene triggers across the corpus.

The frame-sentinel `0xFFFF 0x0000` that opens many scene-event-scripts records is **not** itself an opcode - the `scene-event-scripts` mode skips past it before walking the opcode stream of each record.

## FMV-trigger sites — exhaustive backward sweep

A grep across every Ghidra dump in the corpus for writes to the global game-mode word `_DAT_8007B83C = 0x1A` (the `StrInit` mode that boots the str_fmv overlay) finds **only two distinct writers**. Both are codified in [`legaia_engine_vm::cutscene_trigger`](../../crates/engine-vm/src/cutscene_trigger.rs) as `FMV_TRIGGER_SITES`:

| Label | Function | Mode-write addr | FMV-id source | Trigger condition |
|---|---|---|---|---|
| `field_vm_op_4c_e2` | `FUN_801DE840` | `0x801E3104` | `decode_u16_be(pc+1)` from field-VM bytecode | Field-VM hits the byte sequence `0x4C 0xE2 lo hi`; reached via JT chain `0x801CEE60` (high nibble 0xE) → `0x801CF008` (low nibble 0x2) → label `0x801E30E4`. |
| `title_attract_loop` | `FUN_801DE234`, case `0x10` | `0x801E0F50` | Hardcoded `0` (= `MV1.STR`, intro) | Title-screen idle countdown `DAT_801ef16c` underflows. |

**`FUN_801E30E4` has zero static callers.** It is a label inside `FUN_801DE840`, not a callable subroutine. Ghidra promotes it to a `FUN_` symbol because the JT entry at `0x801CF008[2]` resolves there; the actual control flow is the dispatch chain above. A direct `grep -rn 'jal 0x801e30e4' ghidra/scripts/funcs/` returns zero matches.

The corollary for §2.7's seven mid-game scenes (`town0b`, `map01`, `chitei2`, `map02`, `jou`, `uru2`, `town0e`): they **must** trigger via the same `0x4C 0xE2` op, but the byte sequence is not in their on-disc PROT entries (a bytewise scan of every PROT entry finds only `PROT[371] taiku, fmv_id=5`). The bytecode is therefore reconstructed at scene-load time from the field-pack preamble's runtime-projected slot — the lift is blocked on the same intra-transition byte-level capture that gates [`docs/formats/field-pack.md`](../formats/field-pack.md).
