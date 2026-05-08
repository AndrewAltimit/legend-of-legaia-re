# Save Screen Subsystem

Covers the save-slot selection and write flow used whenever the game writes
progress to the PSX memory card. The save UI lives inside the **menu overlay**
(same 129-function binary as shop, inn, and status screens — not a separate
overlay). Sources: `overlay_save_ui_select.bin` and `overlay_save_ui_saving.bin`
mednafen captures (mc1/mc2), both confirmed as the menu overlay by function-
address identity; decompiled functions at
`ghidra/scripts/funcs/overlay_menu_801dc6b4.txt`,
`overlay_menu_801daef4.txt`, `overlay_menu_801dafd4.txt`.

## Overlay structure

The save UI is hosted by the menu overlay paged into `0x801C0000..0x801EFFFF`.
No dedicated save-screen overlay exists. All three capture points (shop, save
slot select, saving in progress) produced identical function address sets with
only call-frequency differences in the inventory CSV — confirming a single
shared overlay.

## Key functions

### `FUN_801DC6B4` — save-screen outer dispatcher (856 bytes)

Entry: `()`. Returns `true` when the save flow has terminated (outer state
`> 5`). Drives a 9-case state machine on `_DAT_8007B43C`:

| State | Behaviour |
|---|---|
| 0 | Init: copies party pointers `_DAT_800846D0/D4` → `DAT_801EF0F0/F4`; decodes `_DAT_8007B450` (entry-context pointer) into `DAT_801E46A4` (sub-screen selector, see below); sets `_DAT_8007B440 = 0xF2` (full fade); advances to state 1. |
| 1 | Fade-in wait: advances to state 2 once `_DAT_8007B440 < 0x79`. |
| 2 | Sub-screen dispatch: calls `(*(DAT_801E46A4 * 4 + 0x801E4F40))(_DAT_8007B874)` — indirect function pointer table; pad input masked by `_DAT_8007B874`. |
| 3/4/5 | Fade-out (`_DAT_8007B9D8 = 2`); gated on `_DAT_8007B460 == 0` before advancing. |
| ≥ 6 | Terminal — returns `true`. |

The **entry-context pointer** `_DAT_8007B450` determines which sub-screen opens:

| `_DAT_8007B450` | Sub-screen ID (`DAT_801E46A4`) | Meaning |
|---|---|---|
| `(char*)1` sentinel | `0x2` | Save (from menu entry) |
| `*ptr == '\x01'` | `0x19` | Load from slot |
| `*ptr == '\x07'` | `0x20` | Auto-save path |
| `*ptr == '\r'` | `0x4` | Post-save return |
| `*ptr == '\x00'` | `0x1a` | Cancel / back |

Input is suppressed while `_DAT_8007B440 > 0x79` (mid-fade). After state 2
completes, the fade-out advances states 3 → 4 → 5. The four save-coordinate
words `DAT_801E46BC/C0/C4/C8` are zeroed on init and maintained across the
sub-screen lifetime.

### `FUN_801DAEF4` — save-slot selector (224 bytes, sub-screen 0x2 / 0x1)

Internal step counter in `DAT_801E46AC`:

| Step | Action |
|---|---|
| 0 | Set `_DAT_8007B44C = DAT_801C6EA0` (memory-card handle from overlay init); run actor VM with `&DAT_801E4E30` (slot-select menu bytecode). |
| 1 | Wait on `_DAT_8007BB80 != 0` (menu-active flag); advance to step 2. |
| 2 | Call `FUN_801DD35C(1, 1)` (confirm selection); advance to step 3 on success. |
| 3 | Clear `DAT_801E46A4 = 0` when `_DAT_8007B450 != 0` (return to previous screen). |

Each step calls `func_0x80031D00()` (text-actor tick / MES advance) before
returning.

### `FUN_801DAFD4` — save-slot confirm / saving-in-progress (584 bytes)

Internal step counter in `DAT_801E46AC`:

| Step | Action |
|---|---|
| 0 | Clear `_DAT_8007BB98/90/88`; set `_DAT_8007BB94 = 4` (3-slot scrolling list param); run actor VM with `&DAT_801E4E38`; mask `DAT_801E46BC &= 0xFFF`. |
| 1 | Call `FUN_801D688C(&DAT_801E46BC, 3, 1)` (3-item slot list + confirm). Button result: slot 0 → sub-screen 0x1B (card-full/error); slot 1 → validate then run actor VM `&DAT_801E4E54` (advance to step 2); slot 2 → cancel SFX; return 2 → close. |
| 2 | Clear state vars; set `DAT_801E46A4 = 0x1E` (advance to write sub-screen). |

**Save slot validation** (step 1, slot 1 path): scans the save-block existence
table at `&DAT_80084140 + slot * 2 + 0x1818` (byte 0 = slot present,
byte 1 = slot valid) over the range `_DAT_8007B5EA.._DAT_8007B5EC`. A fully
absent table yields error SFX (`func_0x80035bd0(0x23)`).

## Globals used

| Address | Role |
|---|---|
| `_DAT_8007B43C` | Outer state machine discriminant (0..≥6). |
| `_DAT_8007B440` | Screen fade level: `0xF2` = full opaque; `0` = transparent. |
| `_DAT_8007B450` | Entry-context pointer; value determines sub-screen ID. |
| `_DAT_8007B9D8` | Mode discriminant: `1` = save-menu active, `2` = fade-out. |
| `_DAT_8007B44C` | Memory-card handle set to `DAT_801C6EA0` at slot-select init. |
| `_DAT_8007BB80` | Menu-active flag; step 1 waits while zero. |
| `_DAT_8007B5EA` | Save-slot scan start index. |
| `_DAT_8007B5EC` | Save-slot scan end index. |
| `DAT_80084140` | Save-block existence table; stride 2 bytes per slot. Bytes `+0x1818/+0x1819` = present/valid flags. |
| `DAT_801E46A4` | Sub-screen function index (into pointer table at `0x801E4F40`). |
| `DAT_801E46AC` | Sub-screen internal step counter. |
| `DAT_801E46BC/B0/B4` | Per-column save-slot state / pad-input buffer. |

## Sub-screen function pointer table

`FUN_801DC6B4` case 2 dispatches via `0x801E4F40[DAT_801E46A4]`. Full table
read from `overlay_menu.bin` offset `0x24F40` (table base `0x801C0000`):

| ID | Function | Role |
|---|---|---|
| `0x00` | `FUN_801DD12C` | (unknown) |
| `0x01` | `FUN_801D6B20` | `FUN_801DAEF4` slot selector path |
| `0x02` | `FUN_801D6E18` | save entry (from menu entry-context `(char*)1`) |
| `0x03` | `FUN_801D6D38` | (unknown) |
| `0x04` | `FUN_801DD1B8` | post-save return path |
| `0x05` | `FUN_801D7C00` | (unknown) |
| `0x06` | `FUN_801D7E50` | (unknown) |
| `0x07` | `FUN_801D8734` | (unknown) |
| `0x08` | `FUN_801DD26C` | (unknown) |
| `0x09` | `FUN_801D7FF8` | (unknown) |
| `0x0A` | `FUN_801D8308` | (unknown) |
| `0x0B` | `FUN_801D8A58` | (unknown) |
| `0x0C` | `FUN_801D8B90` | (unknown) |
| `0x0D` | `FUN_801D8D94` | (unknown) |
| `0x0E` | `FUN_801D8F10` | (unknown) |
| `0x0F` | `FUN_801D9110` | (unknown) |
| `0x10` | `FUN_801D9280` | (unknown) |
| `0x11` | `FUN_801D9594` | (unknown) |
| `0x12` | `FUN_801D98F0` | (unknown) |
| `0x13` | `FUN_801D99F0` | (unknown) |
| `0x14` | `FUN_801D9C14` | per-character record serialisation (0x414 bytes, `char_id` stride) |
| `0x15` | `FUN_801DA2A0` | (unknown) |
| `0x16` | `FUN_801DD310` | (unknown) |
| `0x17` | `FUN_801DD330` | (unknown) |
| `0x18` | `FUN_801DAE24` | (unknown) |
| `0x19` | `FUN_801DAEF4` | load-from-slot path (entry-context `*ptr == '\x01'`) |
| `0x1A` | `FUN_801DAFD4` | save-slot confirm / saving-in-progress — advances to `0x1E` on confirm |
| `0x1B` | `FUN_801DB21C` | card-full / error screen |
| `0x1C` | `FUN_801DB380` | (unknown) |
| `0x1D` | `FUN_801DB7F4` | (unknown) |
| `0x1E` | `FUN_801DBC5C` | write-step confirmation spinner — advances to `0x1F` on slot confirm |
| `0x1F` | `FUN_801DBD94` | write-step quantity select / save serialisation |
| `0x20` | `FUN_801DC1CC` | auto-save path (entry-context `*ptr == '\x07'`) |

The table ends at `0x1F`; entries past `0x20` are the start of the MES bytecode
section (`0x85826B82` etc.) and are not function pointers.

### Save data serialisation (`FUN_801D9C14`, sub-screen `0x14`)

Copies the per-character save record (stride `0x414` bytes) to a staging buffer
at `DAT_801EF0C8` using `char_id * 0x414 + 0x80084A9E` (character record base).
8 bytes are copied first, then the full record in `do { ... } while (iVar16 < 8)`
chunks. Calls `FUN_801CF650` (memory-card write primitive) as a setup step at
`DAT_801e46ac == 0`. The exact offsets for `story_flags` and `inventory` within
the save block — i.e., what follows the per-character records — are in
`FUN_801DBD94` (sub-screen `0x1F`); see dump at
`ghidra/scripts/funcs/overlay_menu_801dbd94.txt`.

## Relationship to `legaia_save`

The memory-card write calls through `_DAT_8007B44C` (PSX LibC card handle set
from `DAT_801C6EA0`). The in-engine LGSF format (`legaia_save::SaveFile` with
`SaveExt`) is the clean-room counterpart. The exact offsets for `story_flags`
and `inventory` within the retail save block are in `FUN_801DBD94` (sub-screen
`0x1F`); the dump at `ghidra/scripts/funcs/overlay_menu_801dbd94.txt` shows
money (`_DAT_8008459C`), slot count, and save-block existence scan. A full trace
of the global-state bytes (story flags, inventory) requires following
`func_0x80042310` and the staging buffer beyond the per-character record.
