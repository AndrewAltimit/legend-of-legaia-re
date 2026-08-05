# Lane A - `module-orphan` triage for `crates/engine-core/`

Scope: the 26 `module-orphan` findings `scripts/ci/check-port-provenance.py`
raised against `crates/engine-core/src/`. Every verdict below was reached by
reading the **disassembly** of the named address out of
`ghidra/scripts/funcs/` (main checkout; the worktree has no dumps), never the
decompiled C and never a committed doc alone. Where a committed text is cited
it is as a claim that the disassembly then confirmed or contradicted.

Two structural aids that did most of the work:

- `overlay_save_ui_801e4f40.txt` - the captured **save-screen handler table**
  (`PTR_FUN_801e4f40`, 0x21 entries, indexed by `DAT_801E46A4`). It pins the
  sub-screen number of six of these addresses directly, and each handler's own
  disassembly writes `0x801E46A4` with the next screen, so the table and the
  bodies corroborate each other.
- Per-address dump inventory by **header tag** (`== FUN_x (entry=y) [image] ==`),
  not by filename. Three findings turned out to be artifacts of a wrong-image
  or register-relative dump; see the "checker artifacts" section.

## Verdicts

Grade: `disasm` = read the routine's disassembly this pass. No row rests on a
committed doc alone.

| # | Address | File | Verdict | Grade | What changed |
|---|---|---|---|---|---|
| 1 | `FUN_801d4868` | `shop.rs:841` | CORRECT | disasm | none |
| 2 | `FUN_801d5de0` | `shop.rs:842` | CORRECT | disasm | none |
| 3 | `FUN_801d5de0` | `shop.rs:940` (same finding) | CORRECT | disasm | fixed "72 instructions" -> 151 |
| 4 | `FUN_801d5ae8` | `shop.rs:1059` | CORRECT | disasm | none |
| 5 | `FUN_801f8004` | `screen_fx.rs:351` | CORRECT | disasm | none |
| 6 | `FUN_801f88fc` | `screen_fx.rs:614` | CORRECT | disasm | none |
| 7 | `FUN_801f8e6c` | `screen_fx.rs:653` | CORRECT | disasm | none (checker artifact) |
| 8 | `FUN_801d6d38` | `save_subscreen.rs:472` | CORRECT | disasm | none |
| 9 | `FUN_801d98f0` | `save_subscreen.rs:575` | CORRECT | disasm | none |
| 10 | `FUN_801dafd4` | `save_subscreen.rs:658` | CORRECT | disasm | none |
| 11 | `FUN_801d59d4` | `baka_fighter_chrome.rs:165` | CORRECT | disasm | none |
| 12 | `FUN_801d21fc` | `baka_fighter_chrome.rs:403` | CORRECT | disasm | none |
| 13 | `FUN_801d65f8` | `baka_fighter_chrome.rs:1130` | CORRECT | disasm | none |
| 14 | `FUN_800267a8` | `world/frame_tick.rs:32` | CORRECT | disasm | none |
| 15 | `FUN_801d0748` | `world/frame_tick.rs:1858` | CORRECT | disasm | tag sharpened - see below |
| 16 | `FUN_80036d80` | `world/field_movement.rs:1531` | **MISPLACED** | disasm | `PORT:` -> `REF:` |
| 17 | `FUN_801d5c08` | `world/field_movement.rs:2702` | CORRECT | disasm | none |
| 18 | `FUN_801d2a28` | `baka_fighter.rs:1614` | CORRECT | disasm | none |
| 19 | `FUN_801d4c50` | `baka_fighter.rs:1725` | CORRECT | disasm | none |
| 20 | `FUN_800431d0` | `world/encounters.rs:350` | CORRECT | disasm | none |
| 21 | `FUN_801e6f70` | `slot_machine.rs:1012` | CORRECT | disasm | none |
| 22 | `FUN_801d688c` | `save_select.rs:1625` | **MISPLACED** | disasm | `PORT:` -> `REF:` |
| 23 | `FUN_801d7e50` | `pause_screens.rs:970` | CORRECT | disasm | none |
| 24 | `FUN_801d6028` | `minigame_floor.rs:285` | CORRECT | disasm | none |
| 25 | `FUN_801e6984` | `field_submode.rs:4` | CORRECT | disasm | `ink` field doc corrected |
| 26 | `FUN_801d6d38` | `field_menu.rs:219` | CORRECT | disasm | none |

Totals: **24 CORRECT, 2 MISPLACED, 0 WRONG-ADDRESS, 0 WRONG-CODE.**

## The two real defects

Both are the same shape: a `// PORT:` tag on a **call site** whose routine is
already ported, canonically, in another module. That is what `// REF:` is for,
and the repo already uses it for one of these exact pairs one file away.

**`world/field_movement.rs:1531`** - `tick_field_npc_ambient` carried
`// PORT: FUN_80038158 (facing channel drive), FUN_80036D80 (ramp pool)`. The
body calls `vm.tick_with(code, speed, &blocking)` and publishes the result; the
VMs themselves are `crates/engine-vm/src/ambient_motion.rs:5`
(`//! PORT: FUN_80038158, FUN_80036d80, FUN_8003c5f0`). `world/frame_tick.rs:675`
already tags the same pair `// REF:` at its own call site. Retagged to `REF:`.

**`save_select.rs:1625`** - `tick_confirm` carried `// PORT: FUN_801d688c` while
its own tail already said "the shared navigator lives in `crate::menu_input`".
`crates/engine-core/src/menu_input.rs:3` is `//! PORT: FUN_801d688c`. Retagged
to `REF:`.

Both addresses stay `ported=1, live=1` in `port-catalog.py --live-only`
(verified before and after); `80036d80`'s `port_crates` column correctly
narrows from `engine-core|engine-vm` to `engine-vm`.

## Checker artifacts found while triaging (no code defect)

Three rows are the measurement, not the corpus. Recorded because the next
reader will hit them again.

1. **`FUN_801f8e6c` (`screen_fx.rs`)** - reported distinctive data
   `0x80077024 / 3c / 54 / 6c` and callee `0x801d5718`. Those come from
   `overlay_0897_801f8e6c.txt` alone, a 48-instruction window that opens on a
   bare `jal` with no prologue: `0x801F8E6C` is not a function entry in the
   0897 image. The other **six** dumps (baka_fighter, dance, debug_menu,
   fishing, muscle_dome, slot_machine) agree at 47 instructions and are the
   panel move/scale API exactly as the module claims - `FUN_8003CF04(_DAT_8007C34C,
   0x801F849C)` then `+0x3c/+0x3e` = x/y, `+0xb8/ba/bc * scale >> 12` into
   `+0x40/+0x42/+0x26`, `+0x9e` = duration.
2. **`FUN_801db8b4` (`dev_menu.rs`)** - reported distinctive data `0x801c94bc`,
   which exists in no dump. It is synthesised from the *battle* overlay's
   different body at the same VA: `lui 0x801d / addiu -0x6c90` forms
   `0x801C9370`, and the checker then adds the displacement of a later
   `lhu v0,0x14c(v0)` whose `v0` was reloaded from memory - `0x801C9370 +
   0x14C = 0x801C94BC`. Two defects stacked: VA aliasing across images, and a
   `lui`-provenance tracker that survives an intervening `lw`.
3. **`FUN_801db8b4` again** - the correct (overlay_0897) body reads its cursor
   register-relative (`lw v1,0x2e90(a1)`), so the `lui a1,0x801f` that forms
   `0x801F2E90` lives in the enclosing dispatcher, outside the fragment. Its
   sibling `FUN_801db8f4` forty bytes later *does* carry its own `lui`, which
   is the only reason the sibling corroborates and this one does not.

## Evidence highlights per address (the load-bearing reads)

- `FUN_801d4868` - three rows drawn from `0x801CEB94/9C/A4`; ink global
  `_DAT_8007B454` staged to `7` on entry and cleared to `0` **after** the Buy
  row, via the bag scan over `0x80085958` bounded by
  `_DAT_8007B5EA.._DAT_8007B5EC`. Confirms "an empty bag greys Sell and Quit
  together, Buy always renders white" exactly.
- `FUN_801d5de0` - prize table `0x801E4518`, block byte `_DAT_8007B450[1]`,
  stride `block*0x60 + row*8`, affordability against `0x800845A4` (the coin
  bank; `0x8008459C` appears nowhere). Row count `0x801EF0D0`, per-row index
  array `0x801EF0E0`, cursor word `_DAT_8007BB98`. The existing disclosure in
  `shop.rs` is accurate; only its instruction count was wrong (72 -> 151).
- `FUN_801d5ae8` - item table `0x80074368` (+4 name, +8 description), price
  halfword `+2` printed **halved** (`srl 1`, i.e. the sell price), then the
  passive chain `0x801D5C5C..0x801D5CC8`: class `+0 == 1` -> equip record
  `0x80074F68 + idx*8` byte `+5`, else item-effect `0x800752C0 + idx*4` byte
  `+3`, `< 0x40` -> accessory table `0x8007625C`.
- `FUN_801d7e50` - class `0x80/0x81/0x82` -> screens `0xB/0xC/0xD`, else flag
  byte `+2 & 0x20` -> `9`, else `0xA`, written to `0x801E46A4`. Handler table
  slot `[0x06]`.
- `FUN_801e6f70` - eight digit cells at `0x801F35F0` accumulated
  least-significant-first (`x*10` per cell); cost `= coins * 100` built as
  `((3c)*8 + c)*4`; gold `0x8008459C`, stock `0x8007BB90` - both gates recolour
  to ink `9`. `COIN_PRICE_GOLD = 100` and `COIN_ENTRY_DIGITS = 8` are both
  right.
- `FUN_800267a8` - five `gp` cells (`+0x808` armed, `+0x80c` level,
  `+0x810` tag, `+0x814` deadline, `+0x81c` elapsed) from `_DAT_8007B910`, then
  `FUN_80062004(*(u16*)0x80070536, level >> 1, deadline | 1)`.
- `FUN_801d6028` - `_DAT_1F8003EC` grid + `0x8000`, actor `+0x14/+0x18 >> 6`,
  flag word `+0x10` with the `0x00800000` off-floor bit (`0xFF7FFFFF` mask).
- `FUN_801e6984` - ctx `_DAT_8007B450`, `+3` count, `+2` scroll; first row at
  `origin.y + (count-1)*0x10` counting **down**; highlight vs `_DAT_8007BB88`,
  glyph base `0x58` vs `0x4F` vs `_DAT_8007BB9C`; second run skipped for
  `entry == 0`.
- `FUN_801d4c50` - `FUN_80017888(0, 0x46000)` buffer, dev arm gated on
  `_DAT_8007B8C2 == 0`, PROT arm folds `roster >= 3` and entry `= folded +
  0x4B6`, chunk stride `((size >> 2) << 2) + 4` into `FUN_8001F05C(payload,
  hdr, 0, 1)`. All four Rust constants match.

## Out of scope - for other lanes / the orchestrator

1. **`FUN_801d0748`'s narrow labels.** `docs/subsystems/minigame-muscle-dome.md`
   handles this correctly already - its key-functions row says the routine is
   "byte-identical to the main battle round loop" and that the page documents
   only its dome role. Two other texts do not carry that qualifier:
   `crates/asset/src/muscle_dome.rs:4` ("state machine `FUN_801d0748` **and all
   its data** (the deck/hand tables at ...)") and `crates/asset/README.md:296`,
   both of which read as if the SM formed the dome tables. It does not: across
   its 2781 instructions `0x801F4B8C` and `0x801F4D34` appear nowhere - what it
   forms is the battle context `_DAT_8007BD24` with the phase byte at `ctx+6`.
   Co-residency in overlay 0898 is a true and separate statement; only the
   "its data" phrasing overreaches. `crates/asset/` is another lane's file, so
   this is a note, not an edit; I sharpened the engine-core call site's own tag
   instead.
2. **Waivers.** `scripts/ci/port-provenance-waivers.toml` is outside this
   lane's edit scope (four lanes would collide in one TOML). The 24 reviewed
   `CORRECT` rows are ready to paste; keys and reasons are in the block below.

## Ready-to-paste waivers

```toml
[[waiver]]
key = "module-orphan:crates/engine-core/src/shop.rs:801d4868"
reason = "Disassembly (overlay_menu_801d4868.txt): three rows from 0x801CEB94/9C/A4, ink _DAT_8007B454 staged 7 then cleared after the Buy row by the 0x80085958 bag scan bounded by _DAT_8007B5EA.._DAT_8007B5EC. This is the shop root command window; orphan only because its siblings are the sub-screen drivers."

[[waiver]]
key = "module-orphan:crates/engine-core/src/shop.rs:801d5de0"
reason = "Known, disclosed kernel reuse. The routine is the casino prize row renderer (prize table 0x801E4518, block byte _DAT_8007B450[1], coin bank 0x800845A4) and shop.rs says so in the tag itself; the shared cursor decode at 0x801D5E40..0x801D5E9C is genuinely common with FUN_801d4868."

[[waiver]]
key = "module-orphan:crates/engine-core/src/shop.rs:801d5ae8"
reason = "Disassembly (overlay_menu_801d5ae8.txt): item table 0x80074368 name/description, price halfword +2 printed halved (sell price), passive chain 0x801D5C5C..0x801D5CC8 into 0x80074F68/0x800752C0/0x8007625C. Exactly the item-detail / sell panel the module claims."

[[waiver]]
key = "module-orphan:crates/engine-core/src/screen_fx.rs:801f8004"
reason = "Disassembly: FUN_80020DE0(0x801F8FE4, _DAT_8007C34C) - the sprite widget's own binding descriptor, which the module header tabulates. Orphan because a spawn API forms the descriptor its handler never does."

[[waiver]]
key = "module-orphan:crates/engine-core/src/screen_fx.rs:801f88fc"
reason = "Disassembly: FUN_80020DE0(0x801F9014, _DAT_8007C34C) - the panel widget's binding descriptor. Same spawn-API shape as 801f8004."

[[waiver]]
key = "module-orphan:crates/engine-core/src/screen_fx.rs:801f8e6c"
reason = "Checker artifact. The reported data (0x80077024/3c/54/6c, callee 0x801d5718) comes only from overlay_0897_801f8e6c.txt, a window opening on a bare jal - not a function entry in that image. Six tagged dumps agree at 47 instructions: FUN_8003CF04(_DAT_8007C34C, 0x801F849C) then +0x3c/+0x3e = x/y, +0xb8/ba/bc * scale >> 12 into +0x40/+0x42/+0x26, +0x9e = duration. The panel move/scale API."

[[waiver]]
key = "module-orphan:crates/engine-core/src/save_subscreen.rs:801d6d38"
reason = "Save-screen handler table PTR_FUN_801e4f40 slot [0x03], and the body agrees: seeds window 0x801E4BD4, cursor 1, FUN_801d688c(&0x801E46D0, 2, 1), writes 0x801E46A4. Orphan because the window descriptor is not shared with its sibling screens."

[[waiver]]
key = "module-orphan:crates/engine-core/src/save_subscreen.rs:801d98f0"
reason = "Handler table slot [0x12]; body opens window 0x801E4D88 and navigates a list of _DAT_80084594 (party count) entries, confirm -> screen 0x13, cancel -> screen 1. The party-count picker."

[[waiver]]
key = "module-orphan:crates/engine-core/src/save_subscreen.rs:801dafd4"
reason = "Handler table slot [0x1A]; body opens window 0x801E4E38, three rows, row 0 -> 0x1B, row 1 gated on the 0x80085958 bag scan, row 2 and cancel -> screen 0 with the extra 0x37 cue. Matches the port row for row."

[[waiver]]
key = "module-orphan:crates/engine-core/src/baka_fighter_chrome.rs:801d59d4"
reason = "Disassembly: t<30 / t<100 gates, brightness (t-30)*8 clamped at 0x10, widget 0x28 at (0xA0,0x80) through FUN_801D5ED0, latch 0x801DBE8C, announcer FUN_8003d53c(0x20, 0x0E, 0x3F). The intro title card."

[[waiver]]
key = "module-orphan:crates/engine-core/src/baka_fighter_chrome.rs:801d21fc"
reason = "Disassembly: state 0x801DC134, timer 0x801DC138, loading flag 0x8007BC20, banner level 0x801DBEB4 gated at 0x11, round counter 0x801DC110 final 0xE, frame step 0x1F800393. The READY/FIGHT countdown."

[[waiver]]
key = "module-orphan:crates/engine-core/src/baka_fighter_chrome.rs:801d65f8"
reason = "Disassembly: table 0x801DBE84 + index*4, pitch = byte0>>2 + 0x340, pan = byte1 + 0x80, trailing constants (6, 0x18) written only in the mode==0 arm, func_0x80058490(&rec, 0x340, 0x86). The positional SFX helper, exactly as tagged."

[[waiver]]
key = "module-orphan:crates/engine-core/src/world/frame_tick.rs:800267a8"
reason = "Disassembly (SCUS): five gp cells +0x808/+0x80c/+0x810/+0x814/+0x81c from _DAT_8007B910, tail-call FUN_80062004(*(u16*)0x80070536, level>>1, deadline|1). The timed sound-source auto-release arm."

[[waiver]]
key = "module-orphan:crates/engine-core/src/world/frame_tick.rs:801d0748"
reason = "The battle overlay's round / flow SM (context _DAT_8007BD24, phase byte ctx+6). A dome leg is an ordinary battle, so the host dispatcher reaches it on the retail chain; the tag now says so explicitly. Orphan because a 2781-instruction dispatcher shares nothing with the small minigame SMs beside it."

[[waiver]]
key = "module-orphan:crates/engine-core/src/world/field_movement.rs:801d5c08"
reason = "Disassembly (overlay_cutscene_dialogue_801d5c08.txt): cursor +0x9c stepped by +0x9e * DAT_1F800393, clamped at 0x1000, result written into the parent actor at +0x90's +0x14/+0x18 via FUN_801e45bc. The ledge-hop arc helper, as tagged."

[[waiver]]
key = "module-orphan:crates/engine-core/src/baka_fighter.rs:801d2a28"
reason = "Disassembly: combo 0x801DBEC8 clamped to 0x13, combo table 0x801D70C4 into row 0x801DBED8; HP 0x801DBFC4 == 0xC80 -> +50000, else HP/0x140 into halfword table 0x801D711C, both into row 0x801DBEDC. The per-round score accumulation."

[[waiver]]
key = "module-orphan:crates/engine-core/src/baka_fighter.rs:801d4c50"
reason = "Disassembly: FUN_80017888(0, 0x46000), dev arm gated on _DAT_8007B8C2 == 0, PROT arm folds roster >= 3 and takes entry folded + 0x4B6, chunk walk stride ((size>>2)<<2)+4 into FUN_8001F05C(payload, hdr, 0, 1). All four ported constants match."

[[waiver]]
key = "module-orphan:crates/engine-core/src/world/encounters.rs:800431d0"
reason = "Eleven instructions, one-to-one with the port: (&DAT_80074358)[index >> 5] & 1 << (index & 0x1F). A genuinely unrelated leaf living beside the encounter consumers that call it."

[[waiver]]
key = "module-orphan:crates/engine-core/src/slot_machine.rs:801e6f70"
reason = "Disassembly: eight digit cells at 0x801F35F0 accumulated x10 per cell, cost = coins*100, gold gate 0x8008459C, stock gate 0x8007BB90, both recolouring to ink 9. The coin-exchange counter."

[[waiver]]
key = "module-orphan:crates/engine-core/src/pause_screens.rs:801d7e50"
reason = "Handler table slot [0x06]; body reads item-effect record 0x800752C0 + idx*4 and routes class 0x80/0x81/0x82 to screens 0xB/0xC/0xD, else flag byte +2 & 0x20 to 9, else 0xA. The Use-list phase-2 dispatch, as tagged."

[[waiver]]
key = "module-orphan:crates/engine-core/src/minigame_floor.rs:801d6028"
reason = "Disassembly: _DAT_1F8003EC grid + 0x8000 indexed by actor +0x14/+0x18 >> 6, flag word +0x10 with the 0x00800000 off-floor bit, step-layer lookup FUN_801D79E0. The ground-height solver."

[[waiver]]
key = "module-orphan:crates/engine-core/src/field_submode.rs:801e6984"
reason = "Disassembly: ctx _DAT_8007B450 (+3 count, +2 scroll), first row at origin.y + (count-1)*0x10 counting down, highlight vs _DAT_8007BB88, glyph base 0x58/0x4F vs _DAT_8007BB9C, second run skipped for entry 0. The submode list panel layout."

[[waiver]]
key = "module-orphan:crates/engine-core/src/field_menu.rs:801d6d38"
reason = "Same routine as the save_subscreen row and the same disassembly; it runs under two entry contexts, which is why two modules model it. Window 0x801E4BD4, FUN_801d688c(&0x801E46D0, 2, 1), next-screen word 0x801E46A4 written 0 unconditionally then overwritten with 1 on the default row."

[[waiver]]
key = "module-orphan:crates/engine-core/src/dev_menu.rs:801db8b4"
reason = "Checker artifact plus VA aliasing. The reported 0x801c94bc exists in no dump: it is synthesised from the battle overlay's different body at the same VA (lui/addiu forms 0x801C9370, plus a 0x14c displacement off a register reloaded by lw). The overlay_0897 body is the flag-list cursor increment on 0x801F2E90 / table 0x801F2E94 stride 0xA with the 'X' sentinel - it reads the cursor register-relative, which is the only reason it fails to corroborate its sibling FUN_801db8f4 forty bytes later."
```
