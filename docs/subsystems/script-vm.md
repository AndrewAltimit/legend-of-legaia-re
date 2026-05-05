# Field / event script VM

The bytecode interpreter that drives Legaia's overworld scripting — NPC movement, dialog triggers, cutscene sequencing, story-flag manipulation. Lives in PROT entry **`0897_xxx_dat`** (the town/field overlay), at `FUN_801DE840`. ~17.5 KB / 4099 instructions / 357 outgoing calls — the largest function in the corpus.

> **Why "field/event"?** Each running script has its own context (a struct passed around as `ctx_ptr`); contexts can target the player, NPCs, the camera, or "system" channels. The same VM drives both the per-frame field tick and event/cutscene sequences.

The decompiled source is at `ghidra/scripts/funcs/overlay_0897_801de840.txt`. References to `func_0x80xxxxxx` are calls into `SCUS_942.54`; `FUN_801xxxxx` are sister functions inside the 0897 overlay.

## Function signature

```c
int FUN_801DE840(int buffer_base, int pc_offset, int ctx_ptr);
```

- `buffer_base` — bytecode buffer base address.
- `pc_offset` — current program counter, byte offset into the buffer. The function returns the new PC offset (caller advances).
- `ctx_ptr` — script execution context (see "Context struct" below).

The VM is **not** a step-and-yield loop — each call executes from `pc_offset` until something forces a return (instruction halt, branch back into the caller, target script done). The host calls back in at the next frame (or when an external event fires) with the returned PC.

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

The high bit (0x80) of an opcode means "this instruction targets a different script context" — `*(pbVar43+1)` is the script ID, resolved through `func_0x8003C83C` to a context pointer. The original (caller's) context is preserved; the dispatch operates on the resolved one.

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

`_DAT_8007C364` is the **player context pointer** — many opcodes branch on `ctx_ptr == _DAT_8007C364` to switch behavior. `_DAT_801C6EA4` is the current world/scene pointer.

## Opcode reference

### Shared NOP cluster

| Op | Encoding | Effect |
|---|---|---|
| 0x21 / 0x24 / 0x25 / 0x48 | 1 byte | PC += 1. Four distinct opcode bytes share one handler — likely reserved/historical. |

### 0x22-0x26 (action / control flow)

| Op | Mnemonic | Encoding | Effect |
|---|---|---|---|
| 0x22 | `EXEC_MOVE` | `[22, move_id]` | Schedule move-table playback on the current ctx. Sets `ctx[+0x5C] = move_id`, `ctx[+0x5E] = 0xFFFE`, then calls `func_0x800204F8(ctx)` — the **move-table consumer** that [`crates/mdt`](../formats/mdt.md) targets. Player path has special cases around `+0x10` bit 0x1000000 (move chaining) and `move_id == 99` (auto-cancel). |
| 0x23 | `MOVE_TO` | `[23, x_byte, z_byte]` | Teleport ctx to grid position. World coords: `(b & 0x7F) * 0x80 + 0x40`, plus 0x40 if high bit set. Player path also calls `func_0x80017EC8` (camera/scroll). NPC path sets `+0x8C/+0x8D` facing, calls `FUN_801D81E0` and `FUN_801D79E8` (movement init). PC += 3. |
| 0x26 | `JMP_REL` | `[26, lo, hi]` | Relative jump: `PC = pc_offset + 1 + (lo + hi*0x100)`. Unconditional. |

### 0x2B-0x33 (flag manipulation triplets)

The cleanest group — three separate 1-bit-flag banks each with set / clear / test+skip:

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
- **`ctx[+0x62]`** — per-script local flags (sub-routine state, conditional dialog branches).
- **`_DAT_1F800394`** — global story flags (persistent across script runs; PSX scratchpad means cheap to access).
- **`ctx[+0x10]`** — script context flags (halt state, move-chain state, render-gate state).

### 0x34-0x36 (effects, music, scene transitions)

These are sub-dispatchers — the operand byte selects a sub-command.

#### 0x34 EFFECT (nibble-dispatched)

`op0 >> 4` selects sub-op:

| Sub | Encoding | Effect |
|---|---|---|
| 0 | `[34, op0, r, g, b, intensity_lo, intensity_hi]` (7 bytes) | Effect-global colour + intensity setup. Rewrites `_DAT_8007BCCC..BCE0` colour-mode globals. Fade pipeline gated on `_DAT_1F800394 & 0x800000`. |
| 1 | base 13 bytes; +2+payload when peek-at-`pc+13` byte is 0x40 | Effect / sprite spawn with optional captured-PC. Walks actor list at `_DAT_8007C354`; if found, skips spawn. Otherwise calls `FUN_801E5668(ctx, ..., pos, packed24, mode)`; `mode = 1 + (op0 & 1)`. When `capture_flag == 0x40`, captures payload bytes onto the spawned actor's `+0x94`. |
| 2 | 3 bytes | Actor-pool capture-and-yield. Walks list looking for entry whose `+0x90 == ctx`; if found AND `b1 == 0x40`, captures forward-PC and emits `caseD_4` (STATE_RESUME → Yield). |
| 3 | 4 bytes | Play 3D animation via `func_0x800252EC(operand1+1, ctx+0x14, ctx+0x24)`. Looks up an offset in the buffer at `_DAT_8007B8D0` (= the `bse.dat` master bank) using `*(u16*)(buf + 2 + idx*2)`, then spawns an actor via `FUN_80021B04(pos, ?, buf+ofs, 0x1000)`. Buffer layout matches the [ANM container shape](../formats/anm.md). |
| 4..=15 | — | No `case` arm in `FUN_801de840`; falls through `if (bVar35 != 2) { if (bVar35 != 3) { return param_2; } }` — halts at PC. |

#### 0x35 BGM

`[35, lo, hi, sub]`. Operand 2 selects sub-op:

| Sub | Effect |
|---|---|
| 1 | Start field BGM — sets `_DAT_8007BAC8 = signed16(operand0, operand1)` then debug-prints `"Field BGM %d"`. The BGM-id-to-PROT mapping is asynchronous in `FUN_800243F0` (see [BGM lookup](#bgm-lookup-table)). |
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
| 0x38 | `CAM_CFG` | `[38, op0, op1]` | Camera/visual register write. If `op1 & 0x7F == 0`: simple path — copy `*(short*)(0x80073F04 + (op0 & 0xF) * 2)` into `ctx[+0x26]`. Else: halt-acquire path — same predicate as op 0x43 sub-0/1/A/B (`saved_pc != 0 \|\| ctx==player`) AND (`!(flags & 0x400) \|\| scene_busy`); on success set HALT + saved_pc + wait_accum=0 (mirror to caller when ctx is player), yield with `resume_pc = pc + 3`; on fail fall through to dispatcher default. |
| 0x39 | `PLAY_SFX` | `[39, sfx_id]` | Calls `func_0x8004313C()` then `func_0x800421D4(sfx_id, 1)`. |
| 0x3A | `ADD_MONEY` | `[3A, b0, b1, b2]` | 24-bit signed delta: `_DAT_8008459C += sext24(operand)`. Clamp to `[0, 9999999]`. |
| 0x3B | `SET_ITEM_COUNT` | `[3B, slot, count]` | Set inventory entry: `*(byte*)(0x80084340 + (slot & 0xF) + (slot >> 4) * 0x414) = count`, then `func_0x80042558()` to refresh inventory display. Inventory pages of 0x414 bytes. |
| 0x3C | `PARTY_ADD` | `[3C, char_id]` | Add character to party (sorted insertion into `_DAT_80084598..` array, count at `DAT_80084594`). Caps at 4 members. Updates `_DAT_8007B8F8` (party leader) when count was 0. Calls `FUN_801DE190()` (refresh display). Special: if count becomes 2 with `_DAT_80084598 == 0x100`, calls `func_0x800423E0()` and returns. |
| 0x3D | `PARTY_REMOVE` | `[3D, char_id]` | Remove character (linear search, shift, count--). Updates leader if affected. Refresh via `FUN_801DE190()`. |
| 0x3E | `WARP / INTERACT` | `[3E, op0, op1, …]` | If `op0 == 0xFF` or `op0 < 100`: trigger field interact at index `op1` on system context (`func_0x8003C83C(0xFB)`); writes `sys_ctx[+0x94] = scene_data + op1 * stride + 1`, calls `func_0x8003CE08(0xE)`. Else (`op0 >= 100`): scene transition — `_DAT_8007BA34 = op0 - 100` (map id), `_DAT_8007B83C = 0x18`, clears `player[+0x10] & 0x80000`, calls `func_0x8003CE08(0xE)`. |
| 0x3F | `DIALOG` | `[3F, lo, hi, len, [len bytes inline], x, z, depth_id]` | Opens a dialog box. Reads 16-bit text id, copies `len` bytes from operand+3 into a local 16-byte buffer (null-terminated), calls `func_0x8001FD44(local_buf, text_id)` — the dialog/MES opener. Sets `_DAT_1F800394 \|= 0x40` ("dialog active" lock). Writes box position via `_DAT_80073EF4`/`_DAT_80073EF8` (formula `(b & 0x7F) * 0x80 + 0x40`, +0x40 if high bit). PC += 7 + len. |
| 0x40 | `DATA_BLOCK` | `[40, len, ...len bytes]` | Skips `len` bytes after header — embeds raw inline data. PC += 2 + len. |
| 0x42 | `COND_JMP` | `[42, mode, op1, op2, op3]` | Multi-mode conditional. `mode == 0`: test `_DAT_8007B8F4 & (1 << (op1 & 0x1F))` — if clear, return `pc + 5` (skip). `mode == 1`: test screen-mode (`_DAT_8007B850`) against `_DAT_801F28D0[op1*4]` (8-entry table) for `op1 < 8`, bit 0x20 for `op1 == 8`, 0x40 for 9, 0x80 for 10, 0x10 for 11; **`op1 >= 0xC` falls through to the unconditional take-jump path** (no test). `mode >= 2` hits the dispatcher's default arm — halts at PC. Successful jump target = `pc + 3 + LE_u16(op2,op3)`; skip target = `pc + 5`. |

### 0x43 ACTOR_CTRL — sub-dispatcher

22+ sub-ops, keyed on operand byte 0:

#### 0x43 sub-0/1/A/B — halt-acquire dispatcher

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

#### 0x43 sub-2/3-6/7/8/9/C/D/E/F — actor / sound / face / position cluster

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
| 0x16+ | — | No `case` arm in the original `case 0x43` inner switch; falls through with `iVar45 = param_2` (the dispatcher-default initialiser at line 4511 of the dump) — halts at PC. |

#### 0x43 sub-0x10..0x15 — emitter setup family

Each dispatches into the `FUN_801F8xxx` particle/emitter cluster:

| Sub-op | Encoding | SCUS call | PC delta |
|---|---|---|---|
| 0x10 | `[43, 0x10, 19 bytes]` | `FUN_801F8004(operand+1)` (19-byte struct) | +21 |
| 0x11 | `[43, 0x11, 5 × u16]` | `FUN_801F8D4C(u0..u4)` | +12 |
| 0x12 | `[43, 0x12, 6 × s16]` | `func_0x800468A4(6, …)` — **dual call** when `words[2] > 0xFF`, with offset shifts `(+0xF0, _, -0xE0, _, +0x100, _)` and a 0x100 clamp | +14 |
| 0x13 | `[43, 0x13, 12 bytes]` | `FUN_801F88FC(operand)` — passes the whole 13-byte slice | +14 |
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
| 0x44 | `COUNTER` | `func_0x8003D064` 3-int return + `func_0x8003BDE0` — likely a per-frame counter / score / hit-counter tick. |
| 0x45 | `CAMERA` | Sub-dispatch on `op0 & 0xC0`: `0x00` = configure 10 sub-words, `0x40` = LOAD (`FUN_801DBC20`), `0x80` = SAVE (`FUN_801DE004`), `0xC0` = APPLY (`FUN_801DAB90` + `FUN_801DAA50` then absolute jump). |
| 0x46 | `RENDER_CFG` | Fog/render params. `op0 == 0x24` writes 4 bytes (DAT_1F8003E8-EB); else short 2-byte form. |
| 0x49 | `STATE_RESUME` | Multi-frame state machine on `_DAT_8007B450`: tristate (Idle / Armed / Done) with sub-cases 0..0xD. Done-state sub-6/8/9/C/D all jump through `LAB_801df898` for PC += 5. Done-state sub-0 walks an inline MES-shape payload via `func_0x8003CA38` (counts bytes > 0x1E with one-byte peek-extension for `0xCx` prefix bytes); `length = pbVar47[2]` selects the arg-stream length and PC advances by `5 + length + walked`. |
| 0x4A | `WAIT_FRAMES` | `ctx[+0x54] += scratch_delta; if (sum < operand) return; else PC += default`. Frame timer. |
| 0x4B | `ANIMATE` | Multi-keyframe setup. Writes `ctx[+0xB0+N] / +0xB8 / +0xC8`, sets `+0x10` bit 0x1000 (animation flag). PC += 3 + count*4. |
| 0x4C | `MENU_CTRL` | Outer-nibble-dispatched (16 sub-dispatchers). See below. |
| 0x4D | `BBOX_TEST` | Inside-box advances PC by 7; outside-box jumps to `pc + header_size + 4 + LE_u16(operand[4..6])` via `FUN_801E3614`. |
| 0x4E | `INVENTORY_CMP` | Compare-and-jump across page-banked inventory state and party-money/XP banks. Sub-ops 0/1 (page-banked compare, 7 bytes), 2/3/5/6/7/8/9 (absolute jump to operand[2..4]), 10/11 (party-bank u32 compare, 9 bytes), 12..=15 (no test, fall through default arm with PC += 7). Sub-op 4 calls `func_0x80056798` (BIOS Rand thunk = `jr 0xA0; t1=0x2F`) and uses the returned value as the next PC; ported as a side-effect-only host hook (`FieldHost::op4e_sub4_bios_rand`, default returns 0) — almost certainly a dev/debug stub. |
| 0x4F | `SCENE_REGISTER_WRITE` | Writes three `u16` values to `_DAT_801C6EA4 + 0x10/+0x12/+0x14`. |

### 0x4C MENU_CTRL — outer-nibble dispatch

The 0x4C dispatcher's **outer high nibble** of `op0` selects 16 sub-dispatchers:

| Outer nibble | Range | Theme |
|---|---|---|
| 0 | 0x00..0x0F | Party-leader change |
| 1 | 0x10..0x1F | Complex sub-switch on whole byte (menu sub-dispatcher) |
| 2 | 0x20..0x2F | Party-view-swap |
| 3 | 0x30..0x3F | Sub-3 cluster (input lock, no-op cluster, player-resync chain, party-state-clear, etc.) |
| 4 | 0x40..0x4F | Immediate-or-ramp cluster (write or ramp ctx slots / globals) |
| 5 | 0x50..0x5F | Sound directional / dialog query / NPC movement halt-acquire |
| 6 | 0x60..0x6F | 6-word emitter (`func_0x80058490`) + 16-byte halt-acquire |
| 7 | 0x70..0x7F | VRAM tile-flag bulk SET/CLEAR via 7-byte operand |
| 8 | 0x80..0x8F | Large multi-purpose dispatcher (party page mirror, conditional jump on `+0x68`, …). Sub-7 (`func_0x8003CF40(_DAT_8007C34C, &LAB_801E5154)`) registers an actor-list callback then halts at PC via the dispatcher default. Sub-9 writes `_DAT_80073F00 = i16(operand[1..3])` and advances by 4 (the dump's "FUN_801E3620 dispatch" was Ghidra mis-rendering an internal `goto code_r0x801e3620` label; see the gotcha note below). Sub-5/E/F share a single halt-acquire idiom: writes `ctx.saved_pc = pc`, clears `wait_accum`, sets the halt bit, then halts. |
| 9 | 0x90..0x9F | Fade family (sub-0..2 via `FUN_801DDE34`), 16-word table copy (sub-0xE), callback registration (sub-0xF: `func_0x8003CF40(_DAT_8007C34C, &LAB_801DA930)` then halt at PC). |
| A | 0xA0..0xAF | Conditional jump on flag bit. Sub-0 reads `ctx.flags`, sub-1 reads `ctx.local_flags`, sub-2 reads the global story flag word. Bit SET → take absolute jump from operand[2..4]; bit CLEAR (or sub-3..=0xF) → skip 5 bytes. (The asm dispatches on sub-op first at 0x801e2568, so sub-3..=0xF skip both the per-bank check and the take-jump path.) |
| C | 0xC0..0xCF | Small per-actor / per-scene writes (slot table, sub-tile broadcast, sound trigger, `field_74` XOR). Sub-0xF is a position broadcast: 4-byte `[4C, 0xCF, b1, b2]` resolves each byte to either the actor's world coord (`0xFF`), the tile-center conversion `b * 0x80 + 0x40` (non-zero), or 0; advances by 4. Sub-9 is a 2-byte global-pair compare gate: PC += 2 unless `_DAT_8007BAB8 != _DAT_8007BA9C`, then halts. |
| D | 0xD0..0xDF | Party state + inverted-Y mirror cluster. Sub-6 mutates `ctx.field_74`: 3-byte `[4C, 0xD6, b1]`, if `b1 == 4` clears top bit only, else sets bit 0x80000000 + shifts `b1` into the top byte; halts at PC. Sub-8 is a 9-byte 4-arg call to overlay-resident `FUN_801D77F4` (host hook); advances by 9. |
| E | 0xE0..0xEF | Misc scene writes + emitter helpers. Ported sub-ops: 0 (3-way state write, halt), 2 (set globals, 6-byte), 6 (FUN_801D8280, 8-byte), 9 (clear `_DAT_8007B9C4` then PC += 2 via `caseD_4`), 0xA (call `func_0x8003C7EC` then halt), 0xC (capture FUN_801DDF48 return, 2-byte), 0xD (set `_DAT_8007BA66`, 3-byte), 0xE (snapshot `_DAT_80084570 → _DAT_800845DC`, 2-byte). |
| F | 0xF0..0xFF | Only `op0 == 0xFF` valid (pass-through); other sub-ops print `"SUB_CMD_0F_ERROR"` |

The full per-sub-op table is in the field-VM dump (`overlay_0897_801de840.txt`). The clean-room port mirrors the dispatcher shape with host hooks per sub-cluster — see [`crates/engine-vm/src/field.rs`](../../crates/engine-vm/src/field.rs).

### 0x4C nibble-4 — immediate-or-ramp cluster

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
| E / F | — | Inner switch's `default:` arm prints `"SUB_40_ERROR"` and routes via `switchD_801e00f4::default()` — halts at PC. |

Sub-9's tristate dispatch:

| Bit `0x02000000` | Bit `0x01000000` | Path |
|---|---|---|
| clear | clear | `Default` — write/ramp `_DAT_801C6EA4 + 0x4A` |
| clear | set | `AbsJump` — return `signed_16(operand)` as new PC |
| set | (ignored) | `Delta` — write/ramp both target slot **and** delta global at `_DAT_8007BCAC` |

## Default-case "extension" opcodes — the fourth flag bank

The default arm of the dispatcher checks `*pbVar43 & 0x70`:

- `0x50`: `func_0x8003CE08((*pbVar43 & 0x8F) << 8 | pbVar43[1])` — **SET bit**.
- `0x60`: `func_0x8003CE34(...)` — **CLEAR bit**.
- `0x70`: `func_0x8003CE64(...)` — **TEST bit** (returns `0xFF` if set, `0` if clear). When non-zero, the dispatcher consumes two more operand bytes (`pbVar43[2..4]`) as the post-test action target.

The three SCUS dispatchers all operate on the **same 256-bit bitfield array at `DAT_80086D70`** (= `&DAT_80085758 + 0x1618`). Each does `index >> 3` to pick the byte and `0x80 >> (index & 7)` to pick the bit. So the `0x5x/0x6x/0x7x` opcode space encodes a 12-bit operand: the low 4 bits of the opcode plus the next operand byte form an 8-bit (1-byte) flag index — but with the "extended" prefix bit (0x80) preserved into the high bits, the addressable space is 12-bit, suggesting per-script-context banks within the same array.

This is a **fourth flag bank**, distinct from the three above (per-script local at `ctx[+0x62]`, 32-bit globals at `_DAT_1F800394`, ctx flag word at `ctx[+0x10]`). Likely "system" / engine-wide event flags.

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

There isn't really a "BGM → file" lookup table — the BGM ID is a PROT-relative offset. From `FUN_800243F0` (the per-frame BGM/asset poller):

```c
if (_DAT_8007BAC8 < 2000) {
    _DAT_8007BAB8 = _DAT_80084540 + 6;          // scene-local: current scene PROT base + 6
} else {
    _DAT_8007BAB8 = _DAT_8007BC64 - 2000;        // global pool: separate base
}
_DAT_8007BAB8 = _DAT_8007BAC8 + _DAT_8007BAB8;   // final PROT index
```

- `_DAT_8007BAC8` — set by op 0x35 sub-1 (the BGM ID from the script).
- `_DAT_80084540` — current scene's PROT base index (set by the field loader; offset +6 lands at the per-scene BGM block).
- `_DAT_8007BC64` — global BGM pool base for IDs ≥ 2000.
- `_DAT_8007BAB8` — final PROT index, consumed downstream by the asset loader.

So:
- `bgm_id < 2000`: scene-local — lives at PROT `current_scene + 6 + bgm_id`. Different scenes have different BGM at the same script ID.
- `bgm_id ≥ 2000`: global — lives at PROT `_DAT_8007BC64 + bgm_id - 2000`. Shared across scenes (cutscene / title / event music).

The "table" *is* the [CDNAME.TXT name map](../formats/cdname.md)'s per-scene block layout. There's no separate BGM index in `SCUS_942.54`.

## Connection to other crates

- [`crates/mdt`](../formats/mdt.md) — opcode `0x22` `EXEC_MOVE` drives the move-table consumer at `FUN_800204F8`. Move IDs in scripts feed straight into the .mdt parsers.
- [`crates/mes`](../formats/mes.md) — opcode `0x3F` `DIALOG` calls the dialog opener `func_0x8001FD44`. The bytecode inside the dialog buffer is what `crates/mes` parses.
- [`crates/anm`](../formats/anm.md) — opcode `0x34` sub-op 3 plays 3D animations via `func_0x800252EC` — likely the ANM consumer.
- [`crates/engine-vm`](../../crates/engine-vm/src/field.rs) — destination for the clean-room Rust port. Adds a `field_vm` module sister to the existing actor VM. Reuses the `Host` trait pattern.

## Decompile quirks worth knowing

- **`switchD_801e00f4::default()` is misleading**. Ghidra renders the function-epilogue tail block as a synthetic function call; in the original asm, opcodes that "fall through to default" actually advance `param_2` via the `addiu s8, s8, N` instruction in the **MIPS branch-delay slot** of the `j 0x801df09c` jump. So 0x39, 0x3B, 0x44, 0x4C and friends DO advance the PC — just not in a way the C-level decompile makes obvious. Always check the raw asm before deciding "this opcode doesn't advance".
- **`LAB_801df09c`** is just `j 0x801e3628; move v0, s8` — return `s8` unchanged. Most callsites jump there with an `addiu s8, s8, N` in the **delay slot of the j**, supplying the per-callsite PC delta. **`code_r0x801df098`** is the *preceding* instruction `addiu s8, s8, 0x2` — jumping there gives PC += 2 with no per-callsite delta. **`switchD_801e0f24::caseD_4`** has its entry at `0x801df098` and so always does PC += 2 then return.
- **`LAB_801e00b8` = `addiu s8, s8, 0x3; j 0x801e00bc`**. **`LAB_801e00bc` = `j epilogue`** with no advance, used by paths that already incremented `s8` upstream.
- **0x42 mode 0 jump-take target** is `pc + 3 + LE_u16(operand[2..4])` (non-extended), found via the join point `LAB_801e35fc: return iVar18 + uVar31 + iVar24` — not the obvious `pc + 2 + delta`.
### Intra-function label catalogue

`FUN_801de840` is a 17.5 KB function. Several `iVar = FUN_801xxxxx(); return iVar;` patterns in its C decompile look like calls into separate helpers but are actually **intra-function `j` targets** that Ghidra promoted to fake function names. Each label is a `addiu s8, s8, N; j epilogue` block (or a small variant); calling "into" it just supplies the PC delta and falls through to the dispatcher's tail.

Use this table as the lookup when interpreting the dump:

| Label | Aliases in C decomp | Semantic |
|---|---|---|
| `0x801df098` | `code_r0x801df098`, `switchD_801e0f24::caseD_4` | `addiu s8, s8, 0x2; j 0x801df09c` → **PC += 2** |
| `0x801df09c` | `LAB_801df09c`, `switchD_801e00f4::default()` | `j 0x801e3628; move v0, s8` → **PC unchanged** (function epilogue) |
| `0x801df8dc` | `FUN_801df8dc()` (lines 6250, 6284, 6384, 6449) | `addiu s8, s8, 0x6; j epilogue` → **PC += 6** |
| `0x801dee50` | `LAB_801dee50` | "halt-acquire failed" path — **halts at PC** (resets to loop start) |
| `0x801e00b8` | `LAB_801e00b8` | `addiu s8, s8, 0x3; j 0x801e00bc` → **PC += 3** |
| `0x801e00bc` | `LAB_801e00bc` | `j epilogue` — **PC unchanged** for callers that already did `addiu s8, s8, N` upstream |
| `0x801e212c` | `code_r0x801e212c`, `FUN_801e212c()` (lines 4749, 4772, 7285) | `return param_2 + 7;` → **PC += 7** |
| `0x801e35fc` | `LAB_801e35fc` | Join point: `return iVar18 + uVar31 + iVar24` → **PC = pc + 3 + LE_u16(operand[2..4])** for 0x42 mode 0 |
| `0x801e3614` | `FUN_801e3614()` (lines 7252, 7416) | `addiu v0, v0, -2; j 0x801e3624; addu s8, s8, v0` → **PC = s8 + skip - 2** (= `pc + 5 + skip` in the standard 0x4D / nE sub-4 BBOX outside-box context) |
| `0x801e3620` | `code_r0x801e3620`, `FUN_801e3620()` (lines 5021, 6606, 6923, 6928) | `iVar45 = param_2 + 4; ... break;` → **PC += 4** |

Pitfalls when verifying:

1. The misleadingly-named dump file `ghidra/scripts/funcs/overlay_0897_801e3620.txt` shows entry `0x801e3578` — the address `0x801e3620` is just inside that function's epilogue (`lw ra, 0x14(sp)`). The dump filename uses Ghidra's call-site rendering, not the actual entry. Same trap for `overlay_0897_801e212c.txt` if you ever generate one.
2. **Always cross-check `grep -n "0x<addr>" overlay_0897_801de840.txt`** before treating an `FUN_xxxxxxxx` reference as a separate function. Inside the FUN_801de840 dump, `j 0x<addr>` and `beq …, 0x<addr>` instructions reveal intra-function targets that Ghidra mis-promotes.
3. The C decomp sometimes collapses sub-op-first dispatch ordering. Round 11's 0x4C nibble-A bug was an inversion that only became visible after reading raw asm at `0x801e2568` (`bne a1, zero, 0x801e258c` dispatching on sub-op BEFORE the ctx[+0x10] check). When tests pass but the C reads suspicious, walk the asm.

A standing audit pass — picking 5 random ported sub-ops and cross-checking against the dump — turned up **no further inversion bugs** as of round 15.
