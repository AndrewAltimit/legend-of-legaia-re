# Field / event script VM

The bytecode interpreter that drives Legaia's overworld scripting - NPC movement, dialog triggers, cutscene sequencing, story-flag manipulation. Lives in PROT entry **`0897_xxx_dat`** (the town/field overlay), at `FUN_801DE840`. ~17.5 KB / 4099 instructions / 357 outgoing calls - the largest function in the corpus.

> **Why "field/event"?** Each running script has its own context (a struct passed around as `ctx_ptr`); contexts can target the player, NPCs, the camera, or "system" channels. The same VM drives both the per-frame field tick and event/cutscene sequences.

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

> **The `scene_event_scripts` / `scene_v12_table` prescript is a DIFFERENT
> structure - not field-VM bytecode.** The `[u16 count][u16 offsets[count]]`
> prescript (offset 0, or `+0x800` behind the v12 header) was long assumed to
> carry field-VM scripts because its records open with `0xFFFF 0x0000`. It does
> not: running the field-VM disassembler over those records yields a 65–88 %
> decode-error rate, the bytes are 16-bit **word-aligned** (low byte = opcode,
> high byte 0 on ~83 % of body words), records terminate with a `0x0008` word,
> and the opcodes sit mostly below the field VM's `0x22` opcode floor. The
> `0xFFFF 0x0000` lead is a per-record **header sentinel**, not a frame-divider
> opcode, and record 0 is a fixed 768-byte dispatch table. The consuming
> command VM is not yet identified. See `legaia_asset::scene_event_scripts`
> (module note + `record_words`) and the disc-gated falsification test
> `scene_event_records_word_aligned_real`. The engine never feeds these
> prescript bytes to its field VM; the vestigial `Scene::find_event_scripts()`
> / `World::load_field_record()` diagnostic path that does is why ticking a
> prescript record as field-VM "halts at pc 0 / yields immediately" rather than
> running scene logic (see [`field-locomotion.md`](field-locomotion.md)).

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
| 0x39 | `GIVE_ITEM` | `[39, item_id]` | Adds one inline item `item_id` to the inventory; the **treasure-chest item-give** path (the granted item is this single operand byte, **not** a per-scene table). Full behaviour in [§ 0x39 GIVE_ITEM](#0x39-give_item) below. |
| 0x3A | `ADD_MONEY` | `[3A, b0, b1, b2]` | 24-bit signed delta: `_DAT_8008459C += sext24(operand)`. Clamp to `[0, 9999999]`. |
| 0x3B | `SET_ITEM_COUNT` | `[3B, slot, count]` | Set inventory entry: `*(byte*)(0x80084340 + (slot & 0xF) + (slot >> 4) * 0x414) = count`, then `func_0x80042558()` to refresh inventory display. Inventory pages of 0x414 bytes. |
| 0x3C | `PARTY_ADD` | `[3C, char_id]` | Add character to party (sorted insertion into `_DAT_80084598..` array, count at `DAT_80084594`). Caps at 4 members. Updates `_DAT_8007B8F8` (party leader) when count was 0. Calls `FUN_801DE190()` (refresh display). Special: if count becomes 2 with `_DAT_80084598 == 0x100`, calls `func_0x800423E0()` and returns. |
| 0x3D | `PARTY_REMOVE` | `[3D, char_id]` | Remove character (linear search, shift, count--). Updates leader if affected. Refresh via `FUN_801DE190()`. |
| 0x3E | `WARP / INTERACT` | `[3E, op0, op1, …]` | If `op0 == 0xFF` or `op0 < 100`: trigger field interact at index `op1` on system context (`func_0x8003C83C(0xFB)`); writes `sys_ctx[+0x94] = scene_data + op1 * stride + 1`, calls `func_0x8003CE08(0xE)`. Else (`op0 >= 100`): **minigame door-warp** - `_DAT_8007BA34 = op0 - 100` (sub-id), `_DAT_8007B83C = 0x18` (mode 24 OTHER INIT), zero the session-winnings accumulator `_DAT_80084440` and `0x8007BAC0`, clear `player[+0x10] & 0x80000`, call `func_0x8003CE08(0xE)`. The op carries **no destination name**; full pre-warp/return behaviour in [§ 0x3E WARP](#0x3e-warp-mode-24-minigame-door-warp) below. |
| 0x3F | `SCENE_CHANGE` (named warp) | `[3F, idx_lo, idx_hi, name_len, [name_len name bytes], entry_x, entry_z, dir]` | **Named scene-change ("warp by name"), NOT a dialog op.** Full encoding + behaviour in [§ 0x3F SCENE_CHANGE](#0x3f-scene_change-named-warp) below. |
| 0x40 | `DATA_BLOCK` | `[40, len, ...len bytes]` | Skips `len` bytes after header - embeds raw inline data. PC += 2 + len. |
| 0x42 | `COND_JMP` | `[42, mode, op1, op2, op3]` | Multi-mode conditional. `mode == 0`: test `_DAT_8007B8F4 & (1 << (op1 & 0x1F))` - if clear, return `pc + 5` (skip). `mode == 1`: test screen-mode (`_DAT_8007B850`) against `_DAT_801F28D0[op1*4]` (8-entry table) for `op1 < 8`, bit 0x20 for `op1 == 8`, 0x40 for 9, 0x80 for 10, 0x10 for 11; **`op1 >= 0xC` falls through to the unconditional take-jump path** (no test). `mode >= 2` hits the dispatcher's default arm - halts at PC. Successful jump target = `pc + 3 + LE_u16(op2,op3)`; skip target = `pc + 5`. |

#### 0x39 GIVE_ITEM

`[39, item_id]` - adds one of inline item `item_id` to the inventory: `func_0x8004313C()` (select the active inventory window/page bounds) then `func_0x800421D4(item_id, 1)` (the capacity-checked add-item-by-id primitive). PC advances by 2 (`addiu s8,s8,0x2` at `0x801E044C`; `lbu a0,0(s6)` reads the inline id at `0x801E0450`). This is the **treasure-chest item-give** path - the **granted** item is this single inline operand byte, **not** a per-scene table. `FUN_800421D4` is the inventory adder (see [`functions.md`](../reference/functions.md)), so the earlier `PLAY_SFX` / `func_0x800421D4(sfx_id, 1)` label was wrong. (The standalone `FUN_801D71F0` add-item copy has zero callers - dead/duplicate;
the live give-item is inlined in the dispatcher here.) NB the chest's announcement *text* ("There is a {item}…") names the item from a **separate** `0xC2 <id>` MES item-name token (display only), distinct from this give operand - editing one without the other makes the on-screen message disagree with what lands in the bag (see [randomizer.md](../tooling/randomizer.md)).

#### 0x3F SCENE_CHANGE (named warp)

`[3F, idx_lo, idx_hi, name_len, [name_len name bytes], entry_x, entry_z, dir]` - **Named scene-change ("warp by name"), NOT a dialog op.**

- Copies the `name_len`-byte destination scene NAME from operand+3 into a local buffer (null-terminated) and calls `func_0x8001FD44(name, idx)` - the **scene-change packet** (writes the name into the active scene-name buffers `0x8007050C` / `0x80084548`; sets the transition flag `_DAT_1F800394 |= 0x40`).
- `idx` is the sign-extended `i16` at operand[0..2] (a story/entry id; distinct from the `0x3E` 7-id `map_id`).
- Writes the destination entry tile via `_DAT_80073EF4`/`_DAT_80073EF8` (formula `(b & 0x7F) * 0x80 + 0x40`, +0x40 if high bit) and facing from `dir & 7`.
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

The 0x4C dispatcher's **outer high nibble** of `op0` selects 16 sub-dispatchers (party-leader change, menu sub-dispatch, party-view-swap, immediate-or-ramp slot writes, the collision-grid wall-paint at nibble 7, the large multi-purpose nibble-8 cluster, the inverted-Y / actor-spawn nibble-D cluster, the FMV-trigger and emitter nibble-E cluster, and more).

The **full** per-outer-nibble table, the 16×16 sub-dispatch coverage matrix, the actor-allocator + materializer wiring (`0x4C nibble-8 sub-0`), the immediate-or-ramp nibble-4 cluster, and the VRAM STP-bit nibble-D sub-4/sub-5 ops are in **[script-vm-menuctrl.md](script-vm-menuctrl.md)**.


## Default-case "extension" opcodes - the fourth flag bank

The default arm of the dispatcher checks `*pbVar43 & 0x70`:

- `0x50`: `func_0x8003CE08((*pbVar43 & 0x8F) << 8 | pbVar43[1])` - **SET bit**.
- `0x60`: `func_0x8003CE34(...)` - **CLEAR bit**.
- `0x70`: `func_0x8003CE64(...)` - **TEST bit** (returns `0xFF` if set, `0` if clear). When non-zero, the dispatcher consumes two more operand bytes (`pbVar43[2..4]`) as the post-test action target.

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
   handler's side effects run before its reply. Opt-in via `World::use_vm_dialogue`
   (`play-window --vm-dialogue`); the default path stays the simplified
   `OwnedDialogPanel` typewriter. See [`formats/mes.md`](../formats/mes.md#dialog-window-pager---fun_801d84d0).

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

```bash
# Walk a raw script body, print each opcode + operand:
cargo run -p legaia-engine-vm --bin field-disasm -- file <PATH>

# Detect a [u16 count][u16 offsets[count]] prescript at the start of <PATH>
# and walk every record body individually:
cargo run -p legaia-engine-vm --bin field-disasm -- scene-event-scripts <PATH> [--summary]

# Walk every PROT.DAT entry and report 0x4C 0xE2 byte-pattern hits with
# their CDNAME label and decoded fmv_id (filtered to the retail valid
# range 0..=8 unless --no-filter is passed; the runtime FMV-state table
# at 0x801D0A6C carries 12 slots - slots 5..=11 point at cut paths):
cargo run -p legaia-engine-vm --bin field-disasm -- scan-prot \
    --disc <PROT.DAT> --cdname <CDNAME.TXT> --bytewise
```

The library exposes `legaia_engine_vm::field_disasm::{decode, LinearWalker, find_fmv_triggers, format_instruction}` for downstream tooling. `decode()` returns `Result<Insn, DisasmError>`; `LinearWalker` is the iterator shape that wraps `decode` plus single-byte recovery. The `InsnInfo::MenuCtrl { kind: MenuCtrlKind::FmvTrigger { fmv_id }, .. }` variant carries the operand of the `0x4C 0xE2` op for callers who want to grep for cutscene triggers across the corpus.

> **CAVEAT - `scene-event-scripts` / `scan-prot` walk a NON-field-VM
> structure.** The `0xFFFF 0x0000` lead is a per-record header sentinel, and
> the `scene-event-scripts` mode skips it before walking the record body - but
> those records are the word-aligned actor/event structure, not field-VM
> bytecode (see the "On-disc form" note above), so the disassembly is mostly
> `decode error` with coincidental matches. Any `0x4C 0xE2` FMV trigger these
> modes report inside a prescript record is a **false positive** (a word-table
> byte that equals `0x4C` followed by one equal to `0xE2`). The genuine FMV
> triggers are pinned structurally instead - see the exhaustive sweep below and
> the disc-decoded `fmv_dispatch` table - and the per-scene FMV-id remains
> capture-blocked.

## FMV-trigger sites - exhaustive backward sweep

A grep across every Ghidra dump in the corpus for writes to the global game-mode word `_DAT_8007B83C = 0x1A` (the `StrInit` mode that boots the str_fmv overlay) finds **only two distinct writers**. Both are codified in [`legaia_engine_vm::cutscene_trigger`](../../crates/engine-vm/src/cutscene_trigger.rs) as `FMV_TRIGGER_SITES`:

| Label | Function | Mode-write addr | FMV-id source | Trigger condition |
|---|---|---|---|---|
| `field_vm_op_4c_e2` | `FUN_801DE840` | `0x801E3104` | `decode_u16_be(pc+1)` from field-VM bytecode | Field-VM hits the byte sequence `0x4C 0xE2 lo hi`; reached via JT chain `0x801CEE60` (high nibble 0xE) → `0x801CF008` (low nibble 0x2) → label `0x801E30E4`. |
| `title_attract_loop` | `FUN_801DE234`, case `0x10` | `0x801E0F50` | Hardcoded `0` (= `MV1.STR`, intro) | Title-screen idle countdown `DAT_801ef16c` underflows. |

**`FUN_801E30E4` has zero static callers.** It is a label inside `FUN_801DE840`, not a callable subroutine. Ghidra promotes it to a `FUN_` symbol because the JT entry at `0x801CF008[2]` resolves there; the actual control flow is the dispatch chain above. A direct `grep -rn 'jal 0x801e30e4' ghidra/scripts/funcs/` returns zero matches.

The corollary for §2.7's seven mid-game scenes (`town0b`, `map01`, `chitei2`, `map02`, `jou`, `uru2`, `town0e`): they **must** trigger via the same `0x4C 0xE2` op, but the byte sequence is not in their on-disc PROT entries (a bytewise scan of every PROT entry finds only `PROT[371] taiku, fmv_id=5`). The bytecode is therefore reconstructed at scene-load time from the field-pack preamble's runtime-projected slot - the lift is blocked on the same intra-transition byte-level capture that gates [`docs/formats/field-pack.md`](../formats/field-pack.md).

## See also

**Reference** -
[Actor VM](actor-vm.md) ·
[Move-table VM](move-vm.md) ·
[Motion VM](motion-vm.md) ·
[Effect VM](effect-vm.md) ·
[Scene v12 table](../formats/scene-v12-table.md)
