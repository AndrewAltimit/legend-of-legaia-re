# Save Screen Subsystem

Covers the save-slot selection and write flow used whenever the game writes
progress to the PSX memory card. The save UI lives inside the **menu overlay**
(same 129-function binary as shop, inn, and status screens - not a separate
overlay). Sources: `overlay_save_ui_select.bin` and `overlay_save_ui_saving.bin`
mednafen captures (taken at the slot-select and writing-in-progress states),
both confirmed as the menu overlay by function-address identity; decompiled functions at
`ghidra/scripts/funcs/overlay_menu_801dc6b4.txt`,
`overlay_menu_801daef4.txt`, `overlay_menu_801dafd4.txt`.

## Contents

- [Overlay structure](#overlay-structure) · [Key functions](#key-functions) · [Globals used](#globals-used)
- [Sub-screen function pointer table](#sub-screen-function-pointer-table) - [load/save dispatch](#loadsave-dispatch-fun_801dd35c) · [libcd I/O state machine](#libcd-io-state-machine-fun_801e3294) · [save-block directory enumeration](#save-block-directory-enumeration-fun_801e1208) · [equip-candidate list handler](#equip-candidate-list-handler-fun_801d9c14-sub-screen-0x14)
- [Relationship to `legaia_save`](#relationship-to-legaia_save) · [story-flag persistence vs. scratchpad word](#story-flag-persistence-vs-scratchpad-word) · [retail SC block layout](#retail-sc-block-layout)
- [Sprite asset sources (Continue → Load screen)](#sprite-asset-sources-continue--load-screen) - [9-slice tile rects](#pinned-9-slice-tile-rects-system-ui-tim-clut-row-2) · [how the panel TIM was pinned](#how-the-panel-tim-was-pinned)
- [Slide-in UI primitive (`FUN_801E1C1C`)](#slide-in-ui-primitive-fun_801e1c1c) · [messagebox panel geometry (`FUN_801E36C4`)](#messagebox-panel-geometry-fun_801e36c4) · [bottom info panel renderer (`FUN_801E08D8`)](#bottom-info-panel-renderer-fun_801e08d8)

## Overlay structure

The save UI is hosted by the menu overlay paged into `0x801C0000..0x801EFFFF`.
No dedicated save-screen overlay exists. All three capture points (shop, save
slot select, saving in progress) produced identical function address sets with
only call-frequency differences in the inventory CSV - confirming a single
shared overlay.

## Key functions

### `FUN_801DC6B4` - save-screen outer dispatcher (856 bytes)

Entry: `()`. Returns `true` when the save flow has terminated (outer state
`> 5`). Drives a 9-case state machine on `_DAT_8007B43C`:

| State | Behaviour |
|---|---|
| 0 | Init: copies party pointers `_DAT_800846D0/D4` → `DAT_801EF0F0/F4`; decodes `_DAT_8007B450` (entry-context pointer) into `DAT_801E46A4` (sub-screen selector, see below); sets `_DAT_8007B440 = 0xF2` (full fade); advances to state 1. |
| 1 | Fade-in wait: advances to state 2 once `_DAT_8007B440 < 0x79`. |
| 2 | Sub-screen dispatch: calls `(*(DAT_801E46A4 * 4 + 0x801E4F40))(_DAT_8007B874)` - indirect function pointer table; pad input masked by `_DAT_8007B874`. |
| 3/4/5 | Fade-out (`_DAT_8007B9D8 = 2`); gated on `_DAT_8007B440 >= 0xF2` **and** `_DAT_8007B460 == 0`, then advances by `+3` into the terminal range. |
| ≥ 6 | Terminal - returns `true`. |

The **entry-context pointer** `_DAT_8007B450` determines which sub-screen opens:

| `_DAT_8007B450` | Sub-screen ID (`DAT_801E46A4`) | Meaning |
|---|---|---|
| `(char*)1` sentinel | `0x2` | Save (from menu entry) |
| `*ptr == '\x01'` | `0x19` | Load from slot |
| `*ptr == '\x07'` | `0x20` | Auto-save path |
| `*ptr == '\r'` | `0x4` | Post-save return |
| `*ptr == '\x00'` | `0x1a` | Cancel / back |

Input is suppressed while `_DAT_8007B440 > 0x79` (mid-fade), and state 1 hands
over to dispatch one level lower, at `< 0x79` - two distinct constants, not one.

The fade runs in **both directions off the same level word**: `0xF2` is opaque
and `0` transparent, so the fade-in ramps `0xF2 → 0` under a negative delta and
the fade-out ramps back `0 → 0xF2` under a positive one. An exiting sub-screen
writes `0xF2` to the *delta* `DAT_801E46A0`, not to the level - reading that
write as a level assignment inverts the exit fade. The four save-coordinate
words `DAT_801E46BC/C0/C4/C8` are zeroed on init and maintained across the
sub-screen lifetime.

### `FUN_801DAEF4` - load-from-slot driver (224 bytes, sub-screen 0x19)

The load half of the `0x18` / `0x19` card-driver pair - `0x18` writes the card,
this one reads it. It is *not* the slot selector; that is sub-screen `0x01`
(`FUN_801D6B20`), which this screen returns to.

The op selector is what distinguishes the two: both install the same card handle
and both drive `FUN_801DD35C`, but `0x18` calls it `(1, 2)` to save and `0x19`
calls it `(1, 1)` to load (see
[Load/save dispatch](#loadsave-dispatch-fun_801dd35c)).

Internal step counter in `DAT_801E46AC`:

| Step | Action |
|---|---|
| 0 | Set `_DAT_8007B44C = DAT_801C6EA0` (memory-card handle from overlay init); run actor VM (`FUN_801D6628`) with `&DAT_801E4E30` (the load-slot menu bytecode). |
| 1 | Wait while `_DAT_8007BB80 != 0` (menu-active flag); advance to step 2 once it reads zero. |
| 2 | Call `FUN_801DD35C(1, 1)` - the load (card → RAM) direction; advance to step 3 on success. |
| 3 | Write `DAT_801E46A4 = 1` unconditionally, returning to the `0x01` slot selector; then, only when `_DAT_8007B450 != 0`, overwrite it with `0` (the `0x00` final-exit screen). The unconditional write sits in the branch's delay slot, so the `0x01` return is the default and the exit is the override.

Each step calls `func_0x80031D00()` (text-actor tick / MES advance) before
returning.

### `FUN_801DAFD4` - save-slot confirm / saving-in-progress (584 bytes)

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

### `FUN_801D688C` - shared list-cursor navigator

The menu / shop / save-slot state-handlers funnel their list-cursor navigation
through one overlay helper, `FUN_801D688C(cursor: *u32, count, mode)` (see
`ghidra/scripts/funcs/overlay_save_ui_select_801d688c.txt`). It reads the
overlay confirm / cancel pad masks (`_DAT_8007B874 & DAT_801EF0F0` /
`DAT_801EF0F4`) and the held-pad word `_DAT_8007BB84`, mutates the caller's
cursor cell in place, enqueues a UI SFX cue through `FUN_80035B50`, and returns
a small result enum:

| Result | Meaning | SFX cue | Condition |
|---|---|---|---|
| `1` | Confirm | `0x36` | confirm mask held (tested first, even when `count == 0`) |
| `2` | Cancel | `0x37` | cancel mask held |
| `3` | Moved | `0x21` | `count != 0` and a direction moved the cursor |
| `0` | None | - | otherwise |

The cursor cell is packed: the **low 12 bits** are the list index and the
**high nibble** (`0xF000`) carries caller-private flags the navigator preserves
across a move. Held-pad `0x1000` decrements (move-left) and `0x4000` increments
(move-right). `mode == 0` clamps at the ends; `mode != 0` wraps (every ported
call site passes `1`) - a right move whose new index equals `count` snaps the
index back to `0`, a left move from index `0` wraps to `count - 1`. Call sites
in this overlay: the 3-item slot list `FUN_801D688C(&DAT_801E46BC, 3, 1)`
(`FUN_801DAFD4`), the 2-item Yes/No confirm `FUN_801D688C(&DAT_801E46D0, 2, 1)`
(sub-screen `0x03`), and the party-count picker
`FUN_801D688C(&DAT_801E46C4, DAT_80084594, 1)` (sub-screen `0x12`).

**Engine port:** `legaia_engine_core::menu_input::menu_cursor_nav(cursor,
count, wrap, NavButtons)` reproduces the primitive as a plain function over a
caller-owned cursor cell and a `NavButtons` snapshot (host derives the four
booleans from `input::InputState`), returning a `CursorNav` enum whose
`sfx_cue()` surfaces the retail cue id for the host to play through its
`SfxBank` (engine-core surfaces sound cues as return values, not a global
enqueue). The `CURSOR_INDEX_MASK` / `CURSOR_FLAGS_MASK` constants expose the
same low-12 / high-nibble split. `SaveSelectSession::tick_confirm` consumes it
for the Yes/No confirm cursor (the retail `FUN_801D688C(&DAT_801E46D0, 2, 1)`
call site).

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
| `0x00` | `FUN_801DD12C` | 2-state final-exit screen: state 0 invokes actor `&DAT_801E4A78` (terminal display); state 1 waits `_DAT_8007BB80 == 0`, then sets `DAT_801E46A0 = 0xF2` and exit code `_DAT_8007B43C = 3` |
| `0x01` | `FUN_801D6B20` | `FUN_801DAEF4` slot selector path |
| `0x02` | `FUN_801D6E18` | save entry (from menu entry-context `(char*)1`) |
| `0x03` | `FUN_801D6D38` | 2-state Yes/No confirm with default cursor `1`: actor `&DAT_801E4BD4`, picker `FUN_801D688C(&DAT_801E46D0, 2, 1)`; cursor `1` returns to current sub-screen (`0x01`), cursor `0` advances to `0x00` (exit), cancel returns to `0x01` |
| `0x04` | `FUN_801DD1B8` | post-save "press any button" return: state 0 invokes actor `&DAT_801E4BE0`; state 1 waits `_DAT_8007BB80 == 0` AND a button **held** (`_DAT_8007B874 & (_DAT_800846D0 \| _DAT_800846D4) != 0`), plays sfx `0x20` and returns to `0x01`. Mirror of `0x08`, which waits for the same mask to read **zero** |
| `0x05` | `FUN_801D7C00` | pause-menu **Items command window** SM (Use / Throw Out / Arrange) - see [field-menu.md](field-menu.md#items-screen); port `engine-core::pause_screens` |
| `0x06` | `FUN_801D7E50` | Items **Use list** + effect-class dispatch - see [field-menu.md](field-menu.md#items-screen) |
| `0x07` | `FUN_801D8734` | Items **Throw Out** list + confirm - see [field-menu.md](field-menu.md#items-screen) |
| `0x08` | `FUN_801DD26C` | 2-state actor + pad-release-wait: state 0 invokes actor `&DAT_801E4CA4`; state 1 waits `_DAT_8007BB80 == 0` AND no button held (`_DAT_8007B874 & (_DAT_800846D4 \| _DAT_800846D0) == 0`), advances to `0x05` |
| `0x09` | `FUN_801D7FF8` | Use flow **all-party apply** (`FUN_801D688C(&DAT_801E46C4, 0, 0)` - confirm/cancel only, no target rows; preview via `FUN_801D6A54`, apply loop through `FUN_800402F4` + `FUN_80042558`, one bag decrement, cancel → `0x06`). Port `engine-core::pause_screens` (the `ApplyAll` route) |
| `0x0A` | `FUN_801D8308` | Use flow **single-target apply** (party-row picker + `FUN_8003FB10` revalidation buzz). Port `engine-core::pause_screens` (the `ApplySingle` route) |
| `0x0B` | `FUN_801D8A58` | 3-state Yes/No confirm with exit branch: state 0 invokes actor `&DAT_801E4CBC`; state 1 picker on cursor `0` invokes second actor `&DAT_801E4A78` + sfx via `func_0x80042310(0x88, 1)` and advances to state 2, otherwise goes to `0x06`; state 2 waits `_DAT_8007BB80 == 0`, then sets `DAT_801E46A0 = 0xF2`, exit code `_DAT_8007B43C = 4` |
| `0x0C` | `FUN_801D8B90` | Door of Wind destination list (port `pause_screens::SpecialUseSession`) |
| `0x0D` | `FUN_801D8D94` | Incense confirm + class-`0x82` apply (port `pause_screens::SpecialUseSession`) |
| `0x0E` | `FUN_801D8F10` | Magic screen **caster picker**: `FUN_801D688C` over the roster; confirm gated on spell count `record[0x13C]` AND the Ra-Seru equip slot (`record[0x196 + *(i16*)(0x8007B424 + char*2)]`), buzz `0x23` on either; pass → SFX `0x20` + `0x0F`. Port `engine-core::spell_menu` |
| `0x0F` | `FUN_801D9110` | Magic **spell list** (kind-4 list window, content id `5`); cancel parks the list back to `0x0E`; confirm routes on spell-stat byte `+2` bit `0x20`: set → `0x10`, clear → `0x11` (`spell_menu::spell_targets_group`) |
| `0x10` | `FUN_801D9280` | Magic **group-cast flow** (`FUN_801D688C(count = 0)` - confirm/cancel only); SFX `0x25` on commit; cancel → `0x0F` |
| `0x11` | `FUN_801D9594` | Magic **single-target pick + apply**: target rows over the party count; commit revalidates via `FUN_8003FB10`, costs MP through `FUN_80035394` and applies through `FUN_800402F4` + `FUN_80042558` |
| `0x12` | `FUN_801D98F0` | 2-state scrollable picker: state 0 sets `_DAT_8007BB94 = 4`, clears `DAT_801E48A8`, masks the cursor to its index bits (`DAT_801E46C4 &= 0xFFF`) and raises flag `0x4000` on `DAT_801E46C0`, then actor `&DAT_801E4D88`; state 1 picker `FUN_801D688C(&DAT_801E46C4, DAT_80084594, 1)` (count from save-block existence table). Confirm → sfx `0x20` + `0x13`, cancel → `0x01` |
| `0x13` | `FUN_801D99F0` | Equip screen **slot browse** (8 rows: Best Equipment + 7 slots via `FUN_801D688C(&DAT_801E46C0, 8, 1)`; confirm row 0 auto-equips best - `FUN_801CF88C` candidates + `FUN_801CF760` applier, SFX `0x24`/buzz `0x23`; rows 1..7 → `0x14`; cancel → `0x12` character picker) - see [field-menu.md](field-menu.md#equip-screen) |
| `0x14` | `FUN_801D9C14` | Equip screen **candidate list + commit** (see [field-menu.md](field-menu.md#equip-screen); the old "record serialisation" label is falsified - the `0x414`-stride reads are the live party record, the `DAT_801EF0C8` staging is the trial-equip save/restore) |
| `0x15` | `FUN_801DA2A0` | multi-state 2D picker walking a per-character bitfield (`record` word array indexed `bit >> 5` / `bit & 0x1F`) with a left/right grid cursor - body identified, screen identity not yet pinned |
| `0x16` | `FUN_801DD310` | no-op tick: tail-calls `func_0x80031D00` (frame-end / actor-tick flush) with no other work |
| `0x17` | `FUN_801DD330` | thin wrapper invoking the generic picker `FUN_801DA9F8(start=0, end=9, init=0x30, return_subscreen=1)` |
| `0x18` | `FUN_801DAE24` | save-card driver entry. State 0 installs the card handle (`_DAT_8007B44C = DAT_801C6EA0`) and invokes actor `&DAT_801E4E28`; state 1 waits `_DAT_8007BB80 == 0`; state 2 calls `FUN_801DD35C(1, 2)` (saving-overlay main; drives `FUN_801E3294` libcd state machine via the per-frame ticker `FUN_801E1114`); state 3 returns to sub-screen `0x01` |
| `0x19` | `FUN_801DAEF4` | load-from-slot path (entry-context `*ptr == '\x01'`) |
| `0x1A` | `FUN_801DAFD4` | shop **Buy / Sell / Quit mode select** (the earlier "save-slot confirm" reading is superseded - the "existence table at `0x80084140 + 0x1818`" its row-1 validation scans is the **inventory array** at `0x80085958`, i.e. "own anything to sell"). Row `0` → `0x1B` buy list; row `1` → the sell list `0x1E` on a non-empty bag (empty bag buzzes `0x23` in place); row `2` / cancel → `0x00` exit. See [shop.md](shop.md) |
| `0x1B` | `FUN_801DB21C` | shop **buy list** (the earlier "card-full / error screen" label is falsified): kind-4 shop list; confirm checks gold `0x8008459C` against the item price (buzz `0x23` + stay), then routes on the item **kind** byte - `1` → `0x1C` recipient picker, `2` → `0x1D` quantity picker, else back to `0x1A`; cancel → `0x1A`. Port `engine-core::shop::buy_list_confirm_route` |
| `0x1C` | `FUN_801DB380` | shop **buy recipient picker** (equipment buys; port `engine-core::shop`) |
| `0x1D` | `FUN_801DB7F4` | shop **buy quantity + commit** (port `shop::BuyQuantitySession`; quantity law `min(gold/price, 99, 99-held)`) |
| `0x1E` | `FUN_801DBC5C` | 4-state spinner: state 0 raises flag `0x1000` on `DAT_801E46BC` + calls `FUN_801D6628(&DAT_801E4EE4)`; state 1 waits `_DAT_8007BB80 == 0`, sets `_DAT_8007BB94 = 1` and falls into state 2. States 1-settling and 2 **share** the staging read of the two inventory bytes at `0x80084140 + 0x1818 + _DAT_8007BB88*2` into `DAT_801E46B0/B4`, then branch on `_DAT_8007BB94`: `3` re-runs actor `&DAT_801E4EFC` and parks at state 3, `2` advances to `0x1F`. State 3 waits `_DAT_8007BB80 == 0`, then returns to `0x1A`. In context this is the shop **sell list** - the "inventory bytes" it stages are the bag slot `[id, count]` the sell-quantity screen consumes |
| `0x1F` | `FUN_801DBD94` | D-pad quantity-input screen (state 0 init + actor invoke; state 1 ±1/±10 on the dpad clamped to `[1, DAT_801E46B8]`, on confirm applies money delta `_DAT_8008459C += (price * qty) >> 1` and walks live inventory at `0x80084140 + 0x1818` for a non-empty slot; state 2 returns to `0x1A` after a brief delay). NOT the save-card writer - actual libcd I/O lives in `FUN_801E3294` (see "Libcd I/O state machine" section below); `FUN_8001A8B0(SC_base=0x80084140, staging=0x801E5120, 0x1A18)` is plain memcpy used in both directions (post-read or pre-write staging copy) |
| `0x20` | `FUN_801DC1CC` | **casino prize-exchange session** (entry-context `*ptr == '\x07'`; `ptr+1` = prize block). 4-state SM: build visible rows from the `0x801E4518` table (walk stops at the first zero id; a non-zero gate flag already set hides the one-shot row), browse (`FUN_801D688C`; confirm gated on coin bank `0x800845A4 >= price` and held `< 0x63`, buzz `0x23`), Yes/No with **No default** (`DAT_801E46D0 = 1`), commit (SFX `0x25`, grant 1, debit coins, `FUN_8003CE08(gate)`, rebuild). Port `engine-core::prize_exchange`. The earlier "auto-save path" label is falsified - nothing here touches the card |

The table ends at `0x1F`; entries past `0x20` are the start of the MES bytecode
section (`0x85826B82` etc.) and are not function pointers.

### Engine port of the sub-screen graph

`legaia_engine_core::save_subscreen` lifts the graph out of the
pointer-table indirection into a plain state machine. `SaveScreenMachine`
is the outer dispatcher: it holds the phase, the current `SaveSubScreen`,
that screen's step counter and the fade level, and `tick` runs one frame.
A sub-screen transition is a write to the screen field, exactly as retail
writes the id global - the step counter resets with it.

Two shapes recur across the sub-screens and the port keeps them explicit:

- **Script-then-wait.** Step 0 emits `SubScreenEffect::RunScript` and
  advances; the next step blocks until the display script goes idle.
  Every pinned screen opens this way.
- **Transition-by-write.** A screen never returns a destination; it
  writes one. `ConfirmYesNo` is the case worth reading twice - retail
  stores the exit screen *first* and overwrites it when the cursor sits
  on the default row, so the exit is the fallthrough rather than the
  choice.

`SaveSubScreen` covers the whole id space, with `Unpinned(id)` for table
slots whose behaviour is not yet traced, so a transition into one is
expressible and round-trips. Ticking an unpinned screen parks rather than
guessing. The card drivers `0x18` / `0x19` share one implementation
parameterised by `CardOp`, which is what the decompile shows: identical
four-step machines differing only in the op selector.

The module is control flow only. Screen *content* is
[`save_select`](#relationship-to-legaia_save)'s `SaveSelectSession`,
which models the same UI as player-facing phases; a host drives the
session for content and can key retail-exact chrome off the sub-screen
id.

### Load/save dispatch (`FUN_801DD35C`)

The saving-overlay's main routine is shared between the load and save paths.
Sub-screens `0x18` (save) and `0x19` (load) are structurally identical
3-state drivers - they install the card handle, invoke a direction-specific
display actor, then call `FUN_801DD35C(1, op)` repeatedly until it returns
non-zero. The op selector distinguishes direction:

| Sub-screen | Driver | Display actor | Call | Direction |
|---|---|---|---|---|
| `0x18` | `FUN_801DAE24` | `&DAT_801E4E28` | `FUN_801DD35C(1, 2)` | save (RAM → card) |
| `0x19` | `FUN_801DAEF4` | `&DAT_801E4E30` | `FUN_801DD35C(1, 1)` | load (card → RAM) |

Both install `_DAT_8007B44C = DAT_801C6EA0` (PSX libC card handle) on state 0,
so the same global handle is used in both directions. On success both return
to sub-screen `0x01` (the slot picker). Both directions share the same
saving-overlay state machine; the load branch's bulk memcpy
`FUN_8001A8B0(SC_base=0x80084140, staging=0x801E5120, 0x1A18)` is the
post-libcd-read copy (staging buffer → SC RAM).

### Libcd I/O state machine (`FUN_801E3294`)

The actual PSX memory-card calls live in `FUN_801E3294` (in the menu
overlay, also captured in the saving overlay), a 5-state libcd
state-machine driver:

| State (`DAT_801EF188`) | Action |
|---|---|
| `0` | Init: call BIOS-A thunk `FUN_8006EE14(chan)`, advance to `1`. |
| `1` | Poll `FUN_801E3900()`; on result `4` finalise with `FUN_8006EE34` (calls BIOS-B `_card_write` thunk pair); on `1` advance to `2`. |
| `2` | Step: call `FUN_801E39A8` + BIOS-A thunk `FUN_8006EE24(chan)`, advance to `3`. |
| `3` | Wait; same dispatch shape as state 1. |
| `4` | Cleanup: stash result in `DAT_801EF184/180`, reset to `0`. |

The channel argument is `chan = port * 16 + sub_op`. Status strings
printed during the loop (`"NOT_CARD"`, `"card_sts:%d old:%d"`,
`"not card count:%d"`) confirm this drives the libcd lifecycle.
`FUN_8006EE34` is the actual write helper: it calls BIOS-B(0x50) via
`FUN_8006EE7C`, then BIOS-B(0x4E) via `FUN_8006EE6C` with `(chan, 0x3F, 0)`.

Beyond the 5-state skeleton the machine carries a shared **retry budget**
(`DAT_801E4FC4`): a failing phase re-runs the whole two-op cycle with
result `0` until the budget hits 5, then commits `-1` (no card, with the
`"not card count"` print), `-2` (a stray complete event in phase two) or
`-3` (abort / timeout). A `both-acked` latch (`DAT_801EED20`) records
that phase two acknowledged, letting the next cycle short-circuit to the
success result `1` off the first ack alone. Ported as
`save_select::CardIoMachine` (states, retry law, latch, result codes;
the BIOS thunk calls surface as `CardIoEffect` values).

#### The per-frame status poll (`FUN_801E3900`)

States 1 and 3 both branch on `FUN_801E3900`, which is where the "Now
checking" beat actually ends. It calls the `TestEvent` thunk
`FUN_80056658` on four card event handles (`0x8007B9F0`, `..F4`, `..F8`,
`..FC`) in turn and overwrites its running status with `1`, `2`, `3`, `4`
respectively whenever a handle reports `1`. The overwrites are
unconditional, so the **last handle to fire wins** - the priority is call
order, not severity, and only handle 0 can leave the status at `0`.

It then applies a backstop against the frame counter `DAT_801EF17C`,
which state 0 clears on the way in:

```
lw   v1, counter        ; v1 = value on ENTRY
addiu v1, v1, 1
slti a0, <entry>, 0x78  ; entry < 120 ?
bne  a0, zero, skip
 sw  v1, counter        ; delay slot - the store happens either way
li   s0, 0x2            ; else force status 2
```

Two details the shape depends on. The comparison is against the value the
call was *entered* with, so the first frame to force the timeout is the
one entered at `120` - the 121st poll. And the increment sits in the
branch's delay slot, so the counter advances on both paths.

Status `2` therefore has two distinct origins - handle 1 firing, and the
timeout - and `FUN_801E3294` treats them identically (tear the read down
with result `-3`). Status `3` is the "NOT CARD" failure (result `-1`),
`4` completes the read.

Ported as `legaia_engine_core::save_select::card_status_poll`, which the
save-select session runs on every frame of its `NowChecking` phase.
`FUN_801E39A8` is the sibling drain - the same four `TestEvent` calls with
their results discarded. Since `TestEvent` consumes the pending event as it
tests it, the drain is exactly a clear of all four flags: ported as
`save_select::card_events_drain`, which the second-op step of the I/O
machine runs before arming the next BIOS call.

### Save-block directory enumeration (`FUN_801E1208`)

After `FUN_801E3294` finishes a directory scan, `FUN_801E1208` walks the
15-entry libcd directory table at `0x801F32A8` (entry stride `0x28`),
matching each filename against the region-specific Legend of Legaia
prefix using BIOS-A(0x18) `strncmp` (`FUN_80056748`):

| Prefix string | Region |
|---|---|
| `BASCUS-94254PRO_` | USA (Legend of Legaia, SCUS-94254) |
| `BISCPS-10059PRO_` | JP (Legend of Legaia, SCPS-10059) |

The 2-digit slot number is parsed from positions `[10..11]` of the
matched entry and used to write a per-slot record at
`slot_idx * 0x40 + 0x801F2A88` plus a present-marker at
`0x801F2A48 + slot_idx`. `_DAT_801F01F0` carries the available block
count from the prior `FUN_801E3BA0` call.

The per-frame ticker `FUN_801E1114` is the single static caller wiring
the trio together: it calls `FUN_801E3294(DAT_801EF18C, 0)` every frame
to advance the libcd state machine (gated on `_DAT_801F329C < 3`,
latching any non-zero result into `0x801F3800/3804`), and when
`_DAT_801F021C == 3` (save commit) and the rebuild request
`_DAT_801F0224` is up it sequences `FUN_801E3AF0` → `FUN_801E3BA0` →
`FUN_801E1208` and clears the request. Ported as
`save_select::card_frame_tick`, which chains the three ported stages
(`card_directory_scan` / `card_free_blocks` / the classify walk).

#### Filling the table (`FUN_801E3AF0`) and costing it (`FUN_801E3BA0`)

`FUN_801E3AF0` is a **directory enumeration**, not a channel open. It
formats `"bu%1d%1d:*"` from its two arguments - the wildcard makes the
pattern select a *device*, not a name, so every file on the chosen card
matches - then zeroes all fifteen table slots (name bytes `0x13..=0x0`
and the size word at `+0x18`; the other `DIRENTRY` fields are left as
they lie) and walks the BIOS-B `firstfile` / `nextfile` thunks
(`FUN_800566F8` / `FUN_80056708`) across the table. It returns the file
count, which is what bounds every later pass over the table.

Its count loop increments in the `beq`'s delay slot, so the increment
runs on the exiting iteration too and the function subtracts one before
returning. The net result is a plain file count; the correction is not
an off-by-one to reproduce.

`FUN_801E3BA0` is not a query either - it is arithmetic over the table
`FUN_801E3AF0` just filled. It sums each entry's `size` word over the
first `count` entries, applies the MIPS signed-division bias
(`if (sum < 0) sum += 0x1fff`) so the following arithmetic `>> 13`
truncates toward zero, and returns `0xf - blocks`: fifteen usable blocks
minus the blocks the files occupy, at `0x2000` bytes per block. The
result is **not clamped**, so an over-full card returns a negative count.
Its first argument is dead - overwritten before use.

Ported as `card_directory_scan` and `card_free_blocks`;
`SaveSelectSession::from_card_directory` chains both into
`classify_card_directory` so the free-block budget below comes from the
card rather than from a caller's guess.

#### Classification order is load-bearing

The walk is what *writes* the class byte `FUN_801E3F74` later reads, and
the order it writes in is the reason a foreign save is never mistaken
for a free block:

1. Clear both per-slot arrays. Class `0` is the cleared state, and class
   `0` means "occupied by something unreadable".
2. Walk the directory. Every frame whose filename matches a regional
   prefix stamps class `1` on the slot its two digits name.
3. **Only then** spend the card's reported free-block count
   (`_DAT_801F01F0`, from the preceding `FUN_801E3BA0` query) marking
   still-unclassified slots class `2`.

Step 3 is a budget, not a sweep: it decrements a counter rather than
testing a bound, so it stops when the card's free blocks run out. A slot
the walk neither matched nor could afford to call free keeps class `0`.
Absence of a match is therefore never by itself evidence that a block is
free - which is what stops the Save path inviting an overwrite into a
block whose contents were never read.

Ported as `legaia_engine_core::save_select::classify_card_directory`,
returning the per-slot `SlotContent` the info-panel mode selector already
consumes; `card_directory_slots` pairs it with the content-keyed
`SlotSnapshot` constructors to build a session's slot list straight off a
directory.

#### Save-block checksum (`FUN_801E38D8`)

A save block is exactly one card block (`0x2000` bytes = `0x800` u32
words). `FUN_801E38D8` is the block's additive checksum: it sums the first
`0x7FF` little-endian words with a wrapping (`addu`) accumulator and
returns the total, stopping one word short of the block's final word at
byte `0x1FFC`. That final word is where the write path stores the sum; the
load direction of `FUN_801DD35C` reloads it (`0x801df888`:
`lw v1,0x1ffc(s1); beq v1,v0`) and branches on stored-equals-computed to
route the slot to the valid or the corrupt state. Ported as
`save_select::save_block_checksum` with the compose helper
`save_block_checksum_valid` mirroring that load-path compare.

### Equip-candidate list handler (`FUN_801D9C14`, sub-screen `0x14`)

Not a save-screen function at all: sub-screen `0x14` is the pause menu's
**Equip candidate list + commit** handler, sharing the `0x801E4F40` table
with the save flow. It browses the equip-candidate list through the
kind-4 list-kernel protocol, derives the stat-compare preview by
**trial-equipping** the hovered candidate (save the record's 8-byte
equip array at `+0x196` into the staging buffer `DAT_801EF0C8`, write
the candidate, re-run the stat aggregator `FUN_801CF650`, restore), and
on confirm commits the swap through the bag. Full walkthrough on
[field-menu.md](field-menu.md#equip-screen); the earlier readings of the
`0x414`-stride access as "record serialisation" and of `DAT_801EF0C8` as
a "displayed stat read-back buffer" are both superseded - the stride is
the live party record and the staging is the trial-equip save/restore
window. It is **not** a memory-card write primitive.

## Slot list: memory-card slots, not save blocks

Retail's save UI is **two-stage**, and the two stages live in different id
spaces - conflating them is the easy mistake:

| Stage | What the player picks | Count | Retail anchor |
|---|---|---|---|
| Pill row (`SLOT 1` / `SLOT 2`) | a **memory-card port** | 2 | the libcd channel's `port` (`chan = port * 16 + sub_op`, `FUN_801E3294`) |
| 5x3 preview grid | a **save block** on the chosen card | 15 | the directory walk `FUN_801E1208`; per-slot buffer `0x801EF1B8 + N * 0x100` |

Between them sits the card read - the "Now checking. Do not remove MEMORY
CARD" dialog - which is why that beat exists at all.

`SaveSelectSession` is renderer-agnostic and models the phases, not the id
space, so a host picks which reading its slot list carries:

- **Flat** (default): the slot list *is* the save blocks; the pills show the
  first two and Save picks a block straight off the pill row. The native
  shell drives this against its on-disk LGSF slots.
- **Card slots** (`set_card_slots_mode(true)`): the slot list is the two
  ports. Save then crosses the same `NowChecking` beat Load does and raises
  its overwrite prompt from the preview rather than from the pill row, and
  `present` on a pill means "a card is inserted", not "this holds a save".
  The browser play page (`legaia_web_viewer::cards` + `play_menu`) drives
  this against the player's own card images.

The **grid cursor** is the host's, not the session's: `SlotPreview` ignores
directions, so which of the fifteen blocks is focused - and therefore which
block a confirm commits - is host state.

## Relationship to `legaia_save`

The memory-card write calls through `_DAT_8007B44C` (PSX LibC card handle set
from `DAT_801C6EA0`). The in-engine LGSF format (`legaia_save::SaveFile` with
`SaveExt`) is the clean-room counterpart. The `crates/save` constants
`RETAIL_STORY_FLAGS_OFFSET`, `RETAIL_INVENTORY_OFFSET`, and `SAVE_GAME_DATA_RAM_BASE`
expose all confirmed offsets; use `read_retail_story_flags` / `read_retail_inventory`
to slice them from a raw SC block.

## Story-flag persistence vs. scratchpad word

Two distinct global-state stores share the *name* "story flags" but live in
unrelated regions, and **the SC save/load path does not sync between them**:

| Store | Address | Size | Persists in SC? | Touched by save/load |
|---|---|---|---|---|
| Wide bitmap | RAM `0x80085600..0x80085800` | 512 B (4096 bits) | Yes - at SC offset `0x14C0` | Yes, via the bulk RAM→card transfer at `FUN_8001A8B0(0x80084340, card, ...)` (live RAM region containing the bitmap is part of the linear SC body) |
| Scratchpad word | RAM `0x1F800394` | 4 B (32 bits) | No | No |

The scratchpad word `_DAT_1F800394` is the field-VM transient that opcodes
`0x2E` (set bit), `0x2F` (clear bit), and `0x30` (test bit) operate on.
Static-reader sweep across `ghidra/scripts/funcs/*.txt` (`python3
scripts/ghidra-analysis/scan_funcs_for_addr_range.py --lo 0x1F800394 --hi 0x1F800398`)
finds **one** non-RMW writer: `FUN_8001DCF8` at PC `0x8001E17C`, which
seeds it from the game-mode descriptor table:

```c
_DAT_1f800394 = (uint)*(ushort *)(&DAT_800707a0 + _DAT_8007b83c * 0x18);
```

`DAT_800707A0` is `mode_table[0].param` (the mode table at `0x8007078C` has
24-byte stride; the `param` field sits at offset `+0x14`). So the scratchpad
word's lower 16 bits are re-initialised on every mode switch from the
mode's `param` constant; the upper 16 bits start zeroed and are only ever
written by the script-VM bit ops. No retail code path copies between
`0x80085600..0x80085800` and `0x1F800394` in either direction.

In `legaia_save::SaveExt`, `story_flag_bits` mirrors the wide bitmap and
round-trips through the LGSF v3 extension block; `story_flags` mirrors the
scratchpad word and round-trips through the LGSF prelude. The two fields
are independently populated - that matches retail.

## Retail SC block layout

Verified by cross-referencing mednafen save-state RAM dumps against real MCR saves.
The game data region (`block+0x200` onward) is a contiguous linear copy of live RAM
starting at `0x80084340` (`SAVE_GAME_DATA_RAM_BASE`). Any live-RAM field can be
located via `block_offset = 0x200 + (ram_addr - 0x80084340)`.

| Offset in SC block | Size | Field |
|---|---|---|
| `0x0000` | 2 | `SC` magic |
| `0x0002` | 1 | icon flags (`0x11` = 1 frame, 16-color) |
| `0x0004` | 92 | save title (Shift-JIS, null-padded) |
| `0x0060` | 32 | 16-color icon palette (16 × u16 LE BGR5) |
| `0x0080` | 128 | icon pixels (16×16 @ 4bpp) |
| `0x0100` | 256 | (duplicate icon frame or padding) |
| `0x0200` | 0x3C8 | display/global header (see below) |
| `0x05C8` | 0x414 × 4 | character records (Vahn, Noa, Gala, Terra) - base `game+0x3C8` = live RAM `0x80084708` |
| `0x14C0` | 0x200 | story-flag bitmap (mirrors RAM `0x80085600..0x80085800`) - overlaps record [3]'s tail |
| `0x1818` | 0x90 | inventory array - 72 × `(item_id: u8, count: u8)` (mirrors RAM `0x80085958..0x800859E8`) - overlaps record [3]'s tail |

**Display header** (`0x0200..0x05C7`):

| Offset | Size | Field |
|---|---|---|
| `+0x000` | 8 | Current location name (ASCII, null-padded), e.g. `Rim Elm` |
| `+0x054` | 12 | Primary character display name (for save-select screen) |
| `+0x208` | 8 | CDNAME label of most-recently-visited scene (e.g. `town0b`) |
| `+0x218` | 8 | CDNAME label of previous scene (e.g. `town01`) |
| `+0x25C` | 4 | Party gold (mirrors RAM `0x8008459C`) |

**Character records**: `CHARACTER_RECORD_SIZE` (0x414) bytes each. The SC block is a
verbatim dump of the resident save state, so the record array's base is `game+0x3C8`
(live RAM `0x80084708`), *not* the name field. Each record's **display name** is at
internal offset `+0x2A7` (`legaia_save::NAME_OFFSET`), so the visible "Vahn"/"Noa"/"Gala"/
"Terra" strings sit at `game+0x66F + n*0x414` (SC `+0x86F` for slot 0). Four roster slots
exist; the array runs into the global story-flag / inventory region, so slot 3 (Terra)'s
tail (record offset ≥ `+0x2BC`) aliases the story-flag bitmap - her meaningful fields
(name, live stats at `+0x104`, RecordStats at `+0x11C`) sit before that boundary. Empty
slots are all-zero; `read_retail_char_records` stops at the first all-zero record.

`legaia_save::card::read_retail_char_records(sc_block, max_records)` implements extraction.
Constants: `RETAIL_GAME_DATA_OFFSET` (0x200), `RETAIL_CHAR_RECORD_HEADER_SIZE` (0x3C8 = the
true record base), `RETAIL_CHAR_RECORD_STRIDE` (0x414). All re-exported from the
`legaia_save` crate root.

## Sprite asset sources (Continue → Load screen)

The retail Continue → Load screen overlays a "Load" header panel and
N blue SLOT pills on top of the dimmed title art. Asset sources:

| Visible element | Confirmed source | Notes |
|---|---|---|
| Title art behind (wordmark, NEW GAME / CONTINUE, copyright) | `PROT 0888` title TIM | Same atlas the title menu samples; rendered dimmed during SaveSelect. |
| **`Load` panel TIM + CLUT** | **`PROT.DAT[0x018E0]` system-UI sprite sheet, CLUT row 2** | 4bpp 256x192 TIM in the unindexed pre-`init_data` PROT.DAT gap. CLUT block uploads to VRAM `(fb_x=0, fb_y=511)`; the panel-specific row (row 2 of the 16x16 CLUT block) uploads to VRAM `(32, 511)`. Byte-confirmed: the 32-byte CLUT signature appears at exactly one place in the disc corpus (PROT.DAT offset 0x1934). Constants exported by `legaia_asset::title_pak::OVERLAY_SYSTEM_UI_TIM_*`. |
| `Load` panel **9-slice tile geometry** | **PINNED - engine renders byte-perfect** | Retail composes the 81x29 panel at dst `(6, 4)` from 14 textured-sprite primitives (GP0 cmd `0x64`) sampling the system-UI sheet with CLUT `(32, 511)`. Per-tile rects below; all exported as `legaia_asset::title_pak::OVERLAY_SYSTEM_UI_PANEL_*` and rendered by `legaia_engine_render::save_select_chrome_draws_for`. |
| `Load` panel **interior fill** | **PINNED** | Retail fills the 9-slice interior with 3 gouraud-shaded textured quads (GP0 cmd `0x3C`) sampling the same TIM's 32x29 marbled region at `(128, 0)` with a vertical gray gradient `rgb(64,64,64) -> rgb(136,136,136)` (2 full 32-wide copies + 1 17-wide remainder). Constants `OVERLAY_SYSTEM_UI_PANEL_INTERIOR` / `_TOP_RGB` / `_BOT_RGB`; the engine bakes the gradient into the composed atlas (`save_menu_atlas::bake_panel_interior_gradient`). |
| **"Load" text glyphs** | **PINNED to the dialog font (`legaia_font`)** | Drawn from the dialog font, not a menu-glyph atlas. Details in [Load-text glyph decode](#load-text-glyph-decode) below. |
| `SLOT 1` pill | `PROT 0899 + 0x16908 (33, 97, 45, 15)` decoded with CLUT 7 | Saturated blue baked label; byte-equal to retail. |
| `SLOT 2` pill | `PROT 0899 + 0x16908 (33, 113, 45, 15)` decoded with CLUT 7 | Stacked directly below the SLOT 1 pill in the source atlas. |
| Hand cursor | **PINNED** | The pointing-finger cursor lives in the same system-UI TIM as the panel chrome, source `(152, 64, 16, 16)`, CLUT row 7 (white-ink; VRAM `(112, 511)`), dispatched as a single textured sprite at dst `(114, 100)`. Constants `OVERLAY_SYSTEM_UI_CURSOR` / `_CLUT_ROW` / `OVERLAY_SAVE_CURSOR_RETAIL_DST` in `legaia_asset::title_pak`; composed into the engine's save-menu atlas (`save_menu_atlas::band_cursor`). |

#### Load-text glyph decode

The `Load` text glyphs are **PINNED to the dialog font (`legaia_font`)**:

- Retail emits 4 GP0 `0x64` textured-sprite primitives at dst stage `(35, 13)`,
  `(42, 13)`, `(48, 13)`, `(55, 13)`, each `14x15`, sampling **tpage 14** (VRAM
  `(896, 0)` - the dialog font's runtime VRAM upload) with CLUT @ VRAM `(208, 510)`.
- Source UVs `(192,32)`, `(240,64)`, `(16,64)`, `(64,64)` map to `L`/`o`/`a`/`d`
  via `col = (ascii − 0x20) % 16`, `row = (ascii − 0x20) / 16`, `x = col * 16`,
  `y = row * 16` (retail uploads the dialog font with a **16×16 cell pitch**, not
  the `14×15` cell pitch used in `extracted/font/dialog_font_atlas.png` - same
  glyphs, different packing).
- CLUT entry `[15]` = `(206, 206, 206)` - exactly the bright "Load" pixel colour
  in the framebuffer. Per-glyph dst deltas (`+7, +6, +7, +6`) are byte-equal to
  `legaia_font::widths[c] + INTER_GLYPH_PAD = 1`.
- Engine port: `legaia_engine_render::save_select_draws_for` now emits the title
  at `SAVE_SELECT_TITLE_POS` with `SAVE_SELECT_TITLE_COLOR` tint over the
  whitewashed dialog-font stencil (see `legaia_font::Font::load_paths`).
- The earlier "menu-glyph atlas at `PROT.DAT[0x11218]` CLUT row 13" pin is
  **falsified** - that atlas has zero glyph indices at all four documented rects
  (`scripts/pcsx-redux/verify_menu_glyph_load_rects.py` confirms; left here as a
  negative finding).

### Live-pinned screen geometry (GP0 draw list)

Frame dumps of the running load screen (the GP0 command ring, i.e. the sprite
rects the GPU actually receives) pin the remaining layout:

- **Slot pills** dispatch as 48x16 textured quads at `(136, 96)` /
  `(136, 112)` sampling atlas `(32, 96)` / `(32, 112)` of the menu texture
  page; the tile's content starts one texel in on both axes, so the visible
  pills sit at `(137, 97)` / `(137, 113)`. While Browsing each pill is drawn
  **twice semi-transparent** (two CLUT variants layered) over the dimmed
  title art; once a slot is committed only the picked pill draws, opaque,
  parked at quad `(24, 40)` under the Load panel.
- **The 5x3 block grid** is pre-staged **off-screen right** during Browsing
  (cell quads parked at x = 354..1386 with a 104px stagger) and slides left
  on commit. Landed: 32x32 cell quads with row 0 at `(98, 28)`, pitch
  `(40, 20)`, and each successive row shifted **+4 px right** (the grid
  slants). The grid renderer is `FUN_801E06C0`: per cell it interpolates a
  slide base `0x15A + slot*64 → 0x5A` in 12-bit fixed point (same rounding
  as `FUN_801E1C1C`), adds `col*40 + row*4`, and the cell drawer
  `FUN_801E0FD0` adds a `+8` content inset - which resolves both the
  landed pins and the parked 104 px fan-out (`64` slot + `40` column).
  Ported as `engine-ui` `slot_grid_quad_x` / `slot_preview_grid_draws_for`.
  Sibling draw helpers under `FUN_801DD35C`: `FUN_801E02A4` re-emits the
  title art dimmed (two `0x64` sprites split at x = 192 across texture
  pages 8/9, RGB = the brightness parameter; port
  `backdrop_dim_sprites`; its brightness byte is not a constant - the
  caller computes a per-frame ramp and clamps it to `0..=0xFF` before
  passing it to both this and `FUN_801E0418`, so the dim **is** the fade,
  done as RGB modulation rather than as an alpha), `FUN_801E3FF0` stamps one record of the
  12-byte sprite-record table at `0x801E5048` as a `0x2C` quad at a pen
  with an RGB word (port `save_ui_record_quad`), and `FUN_801E0418`
  draws the five-row card-message / two-choice text stack (prompt at
  y = 0x50, choices at 0xA0/0xAE - the unselected choice at half
  brightness off the selector `_DAT_8007B820` - trailing rows at
  0xBE/0xCC; port `card_message_rows`, which also documents the
  function's dead triangle-wave pulse computation). The focused cell draws at full `0x80` modulation, every other
  cell dimmed to `0x60` (75%); portraits are 16x16 quads at cell `+8, +8`;
  the pointing-finger cursor sits at cell quad `+(-10, +4)`.
- **The bottom info panel** footprint lands at `(8, 136, 300, 80)`
  (= the messagebox model applied to `FUN_801E36C4(160, 138, 0x11C, 0x40)`).
  The `LV` / `HP` / `MP` row markers are 16x10 **label sprites** from the
  system-UI sheet, not font glyphs; current / `/` / max emit at fixed
  columns `+16 / +49 / +61` (HP) and `+24 / +49 / +69` (MP) off each
  character column base, and the play-time digits use the small 8x12 digit
  glyphs at x = 236/244, colon 252, 260/268, colon 276, 284/292.

### Pinned 9-slice tile rects (system-UI TIM CLUT row 2)

All rects are `(u, v, w, h)` in 256x192 source-page-pixel coords;
all exported as `legaia_asset::title_pak::OVERLAY_SYSTEM_UI_PANEL_*`.

| Tile | dst (fb_x, fb_y) | src (u, v, w, h) |
|---|---|---|
| Top-left corner | (6, 4) | (160, 0, 4, 4) |
| Top-right corner | (83, 4) | (188, 0, 4, 4) |
| Bottom-left corner | (6, 29) | (160, 28, 4, 4) |
| Bottom-right corner | (83, 29) | (188, 28, 4, 4) |
| Top edge ×3 | (10, 4) / (34, 4) / (58, 4) | (164, 0, 24, 4) |
| Top edge remainder | (82, 4) | (164, 0, 1, 4) |
| Bottom edge ×3 | (10, 29) / (34, 29) / (58, 29) | (164, 28, 24, 4) |
| Bottom edge remainder | (82, 29) | (164, 28, 1, 4) |
| Left edge | (6, 8) | (160, 4, 4, 21) |
| Right edge | (83, 8) | (188, 4, 4, 21) |

### How the panel TIM was pinned

A capture+decode pipeline against PCSX-Redux save state slot 9 (parked
on the load screen):

1. `bash scripts/pcsx-redux/run_probe.sh --lua scripts/pcsx-redux/autorun_load_screen_dump.lua --sstate ~/Tools/pcsx-redux/SCUS94254.sstate9 --frames 180`
   writes `load_screen_fb.{raw,meta}` (the rendered 320×228 framebuffer)
   and `load_screen_ram.bin` (full 2 MiB main RAM).
2. `python3 scripts/pcsx-redux/extract_vram_from_sstate.py ~/Tools/pcsx-redux/SCUS94254.sstate9 captures/load_screen_dump/<iso>/`
   gunzips the save state, finds the `GPU.vram` protobuf field (tag
   `0x1A 0x80 0x80 0x40`), and writes the 1 MiB raw BGR555 VRAM blob.
3. `python3 scripts/pcsx-redux/decode_vram.py vram.bin vram.png`
   renders the 1024×512 VRAM as a PNG so texture pages and CLUT rows
   are visible.
4. Cross-reference the panel-CLUT bytes at VRAM (32, 511) against
   `extracted/PROT.DAT` byte-by-byte: the 32-byte signature matches
   exactly one location (offset 0x1934 = CLUT row 2 of the TIM at
   0x018E0). That TIM's pixel block decoded with CLUT row 2 contains
   the full in-game menu UI atlas (HP/MP panels, money displays,
   battle chrome, equipment frames, and the load-screen panel
   chrome).

### Current engine port status

The engine port (`legaia_engine_core::save_menu_atlas` +
`legaia_engine_render::save_select_chrome_draws_for`) composes the
panel from the pinned 9-slice tiles of the byte-confirmed system-UI
TIM (`legaia_asset::title_pak::OVERLAY_SYSTEM_UI_TIM_OFFSET` /
`OVERLAY_SYSTEM_UI_PANEL_CLUT_ROW`) and ships the SLOT pills
byte-equal to retail. The title word is mode-derived
(`Load` / `Save` from `SaveSelectMode`, mirroring the retail
`_DAT_801f0200` string toggle), never hardcoded per host screen.

## Slide-in UI primitive (`FUN_801E1C1C`)

The save-UI overlay's slide-in animations all flow through a single
primitive,
`FUN_801E1C1C(mode, anim_t, start_x, start_y, target_x, target_y)`.
The function inlines its own 12-bit fixed-point linear interpolation,
then dispatches per mode to emit the slid-in content at the
interpolated `(x, y)`.

```c
// Entry-point interpolation, ghidra/scripts/funcs/overlay_save_ui_select_801e1c1c.txt:
iVar10 = (param_5 - param_3) * param_2;       // (target_x - start_x) * t
if (iVar10 < 0) iVar10 += 0xfff;               // round-toward-zero
param_3 = param_3 + (iVar10 >> 0xc);           // start_x + delta * (t/4096)
// same for param_4 vs param_6
```

`anim_t` is 12-bit fixed-point in `[0, 0x1000]`: `t=0` → at start,
`t=0x1000` → at target. Each animated element has its own dedicated
timer global, and each timer ramps `+0x100` per frame (16-frame
slide). The five animator timers + their modes:

| Mode | Timer global | Element | (start) → (target) |
|---|---|---|---|
| `0` | `DAT_801ef160` | "Now checking. Do not remove MEMORY CARD" dialog | `(416, 112) → (160, 112)` (slides in from right) |
| `1` | (constant `0`) | Static header tabs (Load / Save) | held at `(48, 6)` (no animation) |
| `2` | `DAT_801ef194` | "Load" tab + active-slot pill composite | `(160, 96) → (48, 40)` (slides up-left to upper-left, with `-0x18 = -24` x post-shift) |
| `3` | `DAT_801ef1a4` | "Do you wish to load? / save? / overwrite?" confirm dialog | `(160, 344) → (160, 88)` (slides up from below stage) |
| `4` | `_DAT_801f01cc` | Card-init / format dialog (variant of mode 0) | `(576, 112) → (160, 112)` (slides in from further-right) |

The dispatcher increments each timer per frame and clamps:

```c
// pattern from FUN_801DD35C dispatcher loop:
DAT_801ef194 = DAT_801ef194 + (uint)DAT_1f800393 * 0x100;
if (0x1000 < DAT_801ef194) DAT_801ef194 = 0x1000;
// slide-out direction: subtract until clamped to 0.
```

`DAT_1f800393` is the **adaptive frame-skip factor** - the number of
vsyncs the current game tick spans. The frame-flip path rewrites it
every frame from the measured frame cost (`1` baseline, `2` past
`0xF0`, `3` past `0x1FE`, `4` past `0x2D0`), clamped up to the
per-mode floor `_DAT_8007B9D8`
(`ghidra/scripts/funcs/80016b6c.txt`); live polls show field/town
scenes at `2` (30 fps) and the overworld at `3` (20 fps). Scaling the
per-tick increment by it keeps the slide's *real-time* speed constant
under frame-skip.

### Engine port

`legaia_engine_core::save_select::SaveSelectSession::slide_anim_t()`
mirrors retail timers 0/2 (collapsed into one timer since the engine
doesn't currently break the slide into independent elements). The
free function `interpolate_anim((start, target, t))` implements the
12-bit fixed-point formula and the method `session.interpolate(start,
target)` forwards it using the live `t`. The shell driver
(`legaia-engine play-window --boot-ui`) interpolates two retail
animations:

- Slot composite pill: `(136, 96) → (24, 40)` (matches retail mode-2 with the inlined `-24` x-shift applied to the start).
- NowChecking dialog: panel + text both interpolate `x ∈ {416 → 160}` via `now_checking_{panel,text}_draws_for`'s new `slide_offset` parameter.
- Confirm dialog ("Do you wish to save?"): `confirm_dialog_{panel,text}_draws_for` interpolate `y ∈ {344 → 88}` (mode 3), drawing **two** panels - see [Messagebox panel geometry](#messagebox-panel-geometry-fun_801e36c4).

## Messagebox panel geometry (`FUN_801E36C4`)

Every save-UI panel rect flows through one drawer:

```c
void FUN_801E36C4(int center_x, int y, int w, int h) {
  if (y < 0xf1) {                       // off-stage panels are skipped
    func_0x80034b6c(0x44);              // box style
    func_0x8002c69c((center_x - w / 2) + -2, y + 6, w, h);
  }
}
```

Its `x` is a **centre**, not a left edge, and the box emitter it forwards to
inflates the centre rect by a uniform **8px** on every side - the same
inflation the dialog reading box (`FUN_8002C69C`) applies everywhere - so the
drawn 9-slice footprint is:

```
footprint = (center_x - w/2 - 10,  y - 2,  w + 16,  h + 16)
```

Pinned against the live GP0 draw list (frame dumps of the running load
screen), which reads the sprite rects the GPU actually receives rather than
scanning framebuffer pixels: the header tab `(48, 6, 65, 13)` predicts
`(6, 4, 81, 29)` - exactly the Load panel's byte-pinned 14-sprite
composition - and the parked "Now checking" dialog `(160, 97, 169, 26)`
predicts `(66, 95, 185, 42)`, matching the dump's edge-tile extents. An
earlier `+14 / -9 / -1` model measured off gold-border pixel scans was 1px
short on every side: the outermost tile ring reads as background in a
framebuffer scan (the mid-slide capture's "left 147" is the gold ring one
pixel inside the true footprint at 146).

### Confirm dialog panels (mode 3)

The confirm prompt is **two** panels plus stacked options - not one box with
Yes/No side by side. Mode 3 draws, at slide y `param_4`:

| Element | Retail call | Parked rect (`y = 88`) |
|---|---|---|
| Prompt bar | `FUN_801E36C4(160, y, 284, 13)` | `(8, 86, 300, 29)` |
| Prompt text | `FUN_801E3EE0(msg, 160 + 0x1a, y)` | centred x=186, glyph top y=95 |
| `Yes` row | `FUN_801E3EE0(.., 160 + 4, y + 0x20)` | centred x=164, glyph top y=127 |
| `No` row | `FUN_801E3EE0(.., 160 + 4, y + 0x30)` | centred x=164, glyph top y=143 |
| Options box | `FUN_801E36C4(160, y + 0x20, 42, 26)` | `(129, 118, 58, 42)` |
| Row cursor | `func_0x8002c488(160 - 0x1a, y + ((_DAT_801f01fc + 1) & 1) * 0x10 + 0x24, 0x4e)` | x=134, y=124 (Yes) / 140 (No) |

The prompt bar spans nearly the full stage because its left end carries the
`No.NN` block badge; the message is centred in the remaining space, hence the
`+0x1a` shift. The options box is only 42px wide, so the two rows share one
centre - any flanking layout would fall outside the panel it belongs to.

Both rects are measured from a framebuffer captured with the prompt parked
(`scripts/pcsx-redux/autorun_confirm_dialog_dump.lua` walks a field state to
the prompt and grabs the frame; `scan_panel_rects.py` measures the borders).
Two traps that capture has to clear, both of which silently produce a
confident wrong answer:

- **The mode-3 slide timer `DAT_801ef1a4` is uninitialised** until the confirm
  sub-screen first runs, so polling it as "is the dialog up?" reads stale
  overlay bytes that compare `>= 0x1000` and capture the wrong screen. Trigger
  on a breakpoint at `FUN_801E1C1C` with `a0 == 3` instead.
- **`takeScreenShot` returns the displayed buffer**, which lags the draw (a
  tick spans several vsyncs at 30fps). Capturing on the first parked vsync
  yields a *last-slide-step* frame whose panels sit one 16px step low per
  frame of lag - a plausible-looking rect that is simply wrong. The dialog is
  static once parked, so settle for a dozen vsyncs first.

**Tick rate is load-bearing.** Every timer here - the NowChecking countdown,
each slide - counts 60 Hz frames, so a host must tick the session on a real
60 Hz clock rather than once per rendered frame. Retail makes the same
correction from the other side, scaling each per-tick increment by the
adaptive frame-skip factor `DAT_1f800393` to hold a slide's *real-time* speed
constant. A host that ticks only on input never finishes the card read at
all; one that ticks per rendered frame stretches the ~2 s beat by however far
below 60 fps it runs (the browser play page clocks the menu independently for
exactly this reason).

## Bottom info panel renderer (`FUN_801E08D8`)

After the NowChecking dialog dismisses and the slot-preview screen
appears, the save-UI overlay emits a bottom info panel showing the
selected slot's kingdom name, game time, party leader portrait, and
per-character stats. This is **`FUN_801E08D8(slot_index,
view_mode)`** in the save_ui_select overlay (decompiled C at
`ghidra/scripts/funcs/overlay_save_ui_select_801e08d8.txt`). It's
called once per frame by the grid-renderer wrapper `FUN_801E06C0`,
which iterates the 5×3 portrait grid and emits the info panel for
the focused slot.

### Slide-in animation (vertical)

The info panel has its own bespoke vertical slide-in, distinct from
the FUN_801E1C1C primitive - the primitive can only animate ONE
element, while the info panel propagates a single `panel_y` across
15+ separate sprite/text emit calls. Inlined math at the function
entry:

```c
iVar4 = DAT_801ef1a0 * -0x100;
if (iVar4 < 0) iVar4 += 0xfff;
iVar4 >>= 0xc;
local_34 = iVar4 + 0x18a;   // panel chrome top-y
```

`local_34` ramps from **394 (off-screen below)** at `anim_t = 0`
down to **138 (parked under load chrome)** at `anim_t = 0x1000` -
the SIXTH save-UI slide timer (after the four catalogued for the
primitive). The timer `DAT_801ef1a0` is held to 0 while
`DAT_801ef160` (NowChecking) is up, then increments once the
NowChecking dialog has retracted.

### View modes

`view_mode` selects what fills the panel body:

| Mode | Content |
|---|---|
| `1` | Normal slot preview (kingdom + time + per-character stats). |
| `2` | "Not a Legend of Legaia save." - the block holds something unreadable. |
| `3` | "Able to save." (Save) / "No data" (Load) - the block is free. |
| `4` | "Return" prompt. |
| `100` | Blank panel - forced when `DAT_801ef160 != 0` (NowChecking dialog up) or `_DAT_801f0204 - 0xC < 2`. |

Modes `2`/`3`/`4` all render as **one centred line** through
`FUN_801E3EE0(caption, 0xA0, local_34 + 0x18)`; only mode `1` fills the panel
with rows. `FUN_801E3EE0(text, x, y)` measures the string and hands the raw
emitter `x - width/2` at `y + 7`, so a caption's drawn position is
centre-x 160, y = `local_34 + 31`. Every other element on this panel goes
straight to the raw emitter, so the caption is the only one carrying that
`+ 7`.

### Which mode a slot gets (`FUN_801E3F74`)

The grid wrapper `FUN_801E06C0` calls `FUN_801E3F74(slot)` per cell and passes
the result to the panel as `view_mode`. Branch order:

| Test | Mode |
|---|---|
| `slot == 0xF` | `4` - the sixteenth cell is the Return row, not a block. |
| `0x801F2A68[slot] == 0` | `2` - the slot has not been read off the card yet. |
| `0x801F2A48[slot] == 1` | `1` - a readable Legaia save. |
| `0x801F2A48[slot] == 0` | `2` - occupied by a save the game cannot read. |
| otherwise (class `>= 2`) | `3` - a free block. |

Two per-slot arrays, easily conflated: **`0x801F2A68` is a scanned flag**,
written `1` per slot as the card read walks the directory (and all sixteen at
once on completion), while **`0x801F2A48` is the class byte** that says what
the block holds. Only the latter distinguishes a free block from a foreign
save.

`_DAT_801f0200` gates mode 3's wording, and is `0` on the **Save** path: it is
the branch that goes on to stamp `BASCUS-94254PRO_00` into the chosen free
block (`FUN_801DD35C` case `0xE`), which is save-creation, and it is set from
`FUN_801DD35C`'s second parameter (`1` → `0`, `2` → `1`).

Ported as `engine-core::save_select::{SlotContent, SlotInfoMode}` +
`engine-ui::slot_info_caption_draws_for`. The port has no mode `4` (its block
grid has no Return cell) and models mode `100` as a phase that skips the panel.

The class byte's job falls to whichever scanner builds the `SlotSnapshot`, and
both answer it the same way: **only positive evidence of absence yields
`SlotContent::Free`** - a directory frame no save claims (card path,
`web-viewer::cards`), or a `NotFound` on the slot file (disk path,
`scan_save_dir`). Every other failure to read a slot means something occupies
it, so it classifies `SlotContent::Foreign` and captions as mode `2` rather
than inviting a save into a block whose contents were never read. Both build
foreign slots through `SlotSnapshot::foreign` so the two paths cannot drift.

### Title row layout (mode 1, valid save)

All emit at y = `local_34 + 4` (= 142 fully-landed). Pinned via the
RDATA-loaded string `0x801CF340 "Time "` and the inline sprite emit
constants:

| Element | x | y |
|---|---|---|
| "No.X" badge (sprite via `FUN_801E3FF0` modes 2/3, CLUT row = `slot_index << 4`) | 8 / 30 | local_34 − 8 |
| Kingdom name (from per-slot data buffer `+0`) | 48 | local_34 + 4 |
| `Time` label | 208 | local_34 + 4 |
| Hours digit | 236 | local_34 + 4 |
| Colon `:` | 252 | local_34 + 4 |
| Minutes digit | 260 | local_34 + 4 |
| Colon `:` | 276 | local_34 + 4 |
| Seconds digit | 284 | local_34 + 4 |

### Per-character row layout (mode 1, char_count > 0)

Iterates `i = 0..slot_buf[+0x28]` (party member count). Per-character
horizontal stride = `+0x60 = 96 px` starting at base_x = `0x10 = 16`,
so columns 0/1/2 emit at x = 16 / 112 / 208. Per-character vertical
base `s3 = local_34 + 20` (= 158 fully-landed):

| Element | x (relative to col base) | y |
|---|---|---|
| Character portrait icon (16×16) | base_x | s3 − 4 (= 154) |
| Character name | base_x + 24 | s3 (= 158) |
| `LV` separator + value | base_x / base_x + 32 | s3 + 13 (= 171) |
| `HP` separator + current/max | base_x / +16 / +61 | s3 + 26 (= 184) |
| `MP` separator + current/max | base_x / +24 / +69 | s3 + 39 (= 197) |

HP / MP value colour ramp via `_DAT_8007b454`: 7 (green, default), 6
(yellow, `cur ≤ max/2`), 9 (red, `cur ≤ max/4`).

### Per-slot data buffer

`FUN_801E08D8` reads slot N from `0x801EF1B8 + N * 0x100`:

| Offset | Type | Field |
|---|---|---|
| `+0x00` | char[24] | Kingdom name (null-padded) |
| `+0x10` | char[14] | Save-card filename prefix (`BISCPS-10059PRO`) for validity check |
| `+0x24` | u32 | Game time in seconds (capped at `99:59:59 = 357599`) |
| `+0x28` | u8 | Party member count |
| `+0x2C+i` | u8 | Per-character party ID (0=Vahn, 1=Noa, 2=Gala) |
| `+0x30+i` | u8 | Per-character level (0..99) |
| `+0x34` | s16 | Char 0 MP current |
| `+0x3C` | s16 | Char 0 HP current |
| `+0x44` | s16 | Char 0 MP max |
| `+0x4C` | s16 | Char 0 HP max |
| `+0x54 + i*0x0C` | char[8] | Per-character name |

### Engine port

`legaia_engine_core::save_select::SaveSelectSession::info_panel_slide_anim_t()`
holds at 0 during Browsing / NowChecking / Done, ramps during
SlotPreview / Confirm (matching retail's two-stage flow).
`legaia_engine_render::INFO_PANEL_OFFSCREEN_Y = 394` and
`INFO_PANEL_PARKED_Y = 138` drive the interpolation. The renderer
functions `slot_info_panel_draws_for` (chrome + portrait) and
`slot_info_panel_text_draws_for` (text rows) now take a
`panel_y_offset: i32` parameter - caller-provided delta from the
parked y. The shell driver
(`legaia-engine play-window --boot-ui`) wires this via
`info_panel_slide_offset(session)`. All per-element offset constants
(`SLOT_INFO_LOCATION_OFFSET`, `SLOT_INFO_TIME_LABEL_OFFSET`,
`SLOT_INFO_PORTRAIT_OFFSET`, `SLOT_INFO_NAME_OFFSET`,
`SLOT_INFO_LV_*`, `SLOT_INFO_HP_*`, `SLOT_INFO_MP_*`) are exported
panel-y-relative so future slides / layout shifts only need to touch
the parked-y constant.

## See also

**Reference** -
[Save record](../formats/save-record.md) ·
[Inn](inn.md) ·
[Shop UI](shop.md)
