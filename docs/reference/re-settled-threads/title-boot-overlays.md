# Settled threads: Title / boot / overlays

One area of the [settled reverse-engineering threads](../re-settled-threads.md) register.
The evidence grades (`disassembly` / `capture` / `decompiled-C` / `inference`) are defined on
[the index page](../re-settled-threads.md#the-evidence-column).

This area covers what happens between power-on and the first playable scene - the boot logos, the
title screen and the New Game opening - and the code overlays the game pages into RAM on top of the
executable. Most game logic lives in those overlays, and several of them share one load address, so
knowing which disc entry an overlay is, where it links, and what loads it is the footing every other
subsystem's function addresses stand on. The entries below also settle the build-mode and debug
flags, the Muscle Dome's place inside the battle engine, and the bounds of the item bag.

## Detailed write-ups

Threads whose answer needs more than a table cell. Every other thread is a row of the table under [Threads](#threads).

- [A cold boot always shows title sub-mode `0x10`](#a-cold-boot-always-shows-title-sub-mode-0x10)
- [`_DAT_8007B98F` is byte +3 of the debug-mode word `_DAT_8007B98C`](#_dat_8007b98f-is-byte-3-of-the-debug-mode-word-_dat_8007b98c)
- [New-Game opening chain + narration roller](#new-game-opening-chain--narration-roller)
- [Overlay-loader index off-by-2 - remaining ripple](#overlay-loader-index-off-by-2---remaining-ripple)
- [Muscle Dome match shape: an ordinary battle ladder, not a card battle](#muscle-dome-match-shape-an-ordinary-battle-ladder-not-a-card-battle)
- [The dome runs two state machines; the outer one is the contest](#the-dome-runs-two-state-machines-the-outer-one-is-the-contest)
- [Battle arts-input UI decomposition (dome = standard battle input)](#battle-arts-input-ui-decomposition-dome--standard-battle-input)
- [Slot-B overlay cluster (`0900..0969`) per-entry identity](#slot-b-overlay-cluster-09000969-per-entry-identity)
- [`0x80010390` is the slot-B overlay destination pointer](#0x80010390-is-the-slot-b-overlay-destination-pointer)
- [PROT 0968 - the Cort battle stage overlay](#prot-0968---the-cort-battle-stage-overlay)
- [PROT 0977 / 0978 extraction + the dump re-key](#prot-0977--0978-extraction--the-dump-re-key)
- [Slot-B capture-module band `0935..0966` per-entry identity](#slot-b-capture-module-band-09350966-per-entry-identity)
- [New-game world-state seed store widths](#new-game-world-state-seed-store-widths)
- [`_DAT_8007B8C2` polarity, and its writer](#_dat_8007b8c2-polarity-and-its-writer)
- [Key-item area consumers](#key-item-area-consumers)
- [`title.pak` PROT entry](#titlepak-prot-entry)
- [Title screen mode-table PROT](#title-screen-mode-table-prot)
- [XP-table source + reader](#xp-table-source--reader)
- [Overlay identity from the disc (static extraction)](#overlay-identity-from-the-disc-static-extraction)
- [PROT 0896 (`bat_back_dat`) identity](#prot-0896-bat_back_dat-identity)
- [SCUS recomp gap - render/GTE + boot/init clusters](#scus-recomp-gap---rendergte--bootinit-clusters)
- [Full-window item-add OOB reachability](#full-window-item-add-oob-reachability)
- [Phantom-VA sweep of the PROT 0897 imports](#phantom-va-sweep-of-the-prot-0897-imports)
- [The publisher-logo quads](#the-publisher-logo-quads)
- [The title menu's law](#the-title-menus-law)
- [`FUN_801E5A08`, the per-slot equip applier](#fun_801e5a08-the-per-slot-equip-applier)
- [The dead Return view mode](#the-dead-return-view-mode)

## Threads

| Thread | Status | Evidence | Answer |
|---|---|---|---|
| What does `FUN_801E0418` draw, and which entry uploads its page at VRAM `(512, 256)`? | resolved (the title's own strips; PROT 0890) | `capture` + `disassembly` | It redraws the title TIM's strips behind the Load window when that window is opened from the title: records 0/3/4/2/5 of the descriptor table at `0x801E50A8` (wordmark, NEW GAME, CONTINUE, TM line, copyright) centred at `(0xA0, 0x50/0xA0/0xAE/0xBE/0xCC)`, the row the title cursor `_DAT_8007B820` is not on at half brightness, gated on `_DAT_8007BB00`. The page is the title TIM in PROT 0890 at `0x14228`: in `save_select_idle` all 256 VRAM rows of the page and the CLUT row are byte-equal to it. Ported and live: `engine-ui::title_strip_rows`. [`save-screen.md`](../../subsystems/save-screen.md). |
| How many rows does retail's title menu have? | resolved (two) | `disassembly` | The title tick wraps its row counter with `andi v1,v1,0x1` at `0x801DDC00` and the confirm arm branches on row 0 against everything else (`overlay_title_801dd6b8.txt`), so no title route yields Options; retail reaches Options from the pause menu ([`host-drift.md`](../../tooling/host-drift.md#the-boot-options-screen-is-the-pause-menus-on-both-hosts)). |
| How does a slot-B module hand a spawn call its record? | resolved (three shapes beyond an adjacent pair) | `disassembly` | The pair can complete in the `jal`'s delay slot, the pointer can arrive through a saved-register copy formed up to 256 words back, and `switch` arms can each load `$a2` in the delay slot of a `j` to one shared call. The delay-slot shape accounts for 52 of the 97 spawn calls an adjacent-pair-only resolver sends out of the image into a neighbour's records ([`slot-b-module-layout.md`](../../formats/slot-b-module-layout.md#resolving-the-pointer-a-spawn-call-is-handed)). |
| Which images take the slot-B layout walk? | resolved (the seventy at the link base, not the sixty-four in the index band) | `disassembly` | The walk's three regions are each recovered by resolving a word against `0x801F69D8`, so it belongs to every image mapped there: the 64-entry cast band plus 0900 / 0901 / 0967 / 0968 / 0969 / 0978. With the walk the six gain a head-table claim (accounted bytes 0968 `60.4% -> 99.8%`, 0969 `78.7% -> 99.4%`, 0978 `51.3% -> 91.6%`). `is_slot_b_module` answers the band question - which images the cast dispatcher reaches - and `is_slot_b_image` the layout one. |
| What is PROT 0967's residue? | resolved (a head table, dumped code, one leaf, a consumed prompt pool and a neighbour's tail) | `disassembly` | 408 bytes of head jump table, 2,744 bytes of dumped code, one 92-byte frameless leaf `FUN_801F7628` at file `0xC50..0xCAC`, 1,757 bytes of prompt pool and 1,143 bytes inherited from PROT 0966. The pool is consumed: twenty-eight strings formed by the module's own code at forty-one sites between file `0x278` and `0xA7C`. The leaf is reached by `jal` from `0x801F7184` and `0x801F7460` inside the image, so a slot-B image can call itself. The link-base layout walk admits 0967 at 96.6%; the cast band (`0903..=0966`) is a different predicate. [`byte-accounting.md`](../../tooling/byte-accounting.md#what-the-overlay-residue-that-is-left-actually-is) |
| What is in the PROT 0898 head block below `0xDF8`? | resolved (a string pool and **twenty-two** jump tables; two more sit above it) | `disassembly` | Each consumer is `sltiu` / `beqz` / base / `sll 2` / load / `jr`, so a table's extent is its `sltiu` bound times four: twenty-two bases tile the head, 850 arms in all, among them `0x801CF1CC` (179 arms, `jr` at `0x801EA9FC`) and `0x801CF49C` (5 arms, `ctx[+0x28A]`). Above the head sit nine arms at `0x801CF614` (`jr` at `0x801F3AC0`) and seven at `0x801CFA2C` (`jr` at `0x801F3EB4`), with the Seru side-effect banner pool between them. Counting VA-word runs instead yields nine, because abutting tables merge. `legaia_asset::battle_jump_tables`; [`byte-accounting.md`](../../tooling/byte-accounting.md#the-0898-head-is-twenty-two-jump-tables) |
| What is PROT 0970's 131,172-byte zero hole? | resolved (the overlay's own **uninitialised data region**) | `disassembly` | It decomposes exactly, in the image's own operands: the `0x50` decode context at `0x801D19A0`, two `0x7800` slice staging buffers at `0x801D19F0` and `0x801D91F0`, four globals at `0x801E09F0` and the `0x11000` STRv2 VLC table destination at `0x801E0A00`, ending flush against the unpacker. The loader transfers the whole sector extent, so an overlay's `.bss` travels with its code and reads as a hole. The table's packed source is in the same image at `0x801F1AE8`. [`byte-accounting.md`](../../tooling/byte-accounting.md#an-overlays-uninitialised-data-region-travels-with-its-code). |
| What is PROT 0974's 10,836-byte run of readable text? | resolved (an 81-record `0x84`-stride **roster**) | `disassembly` | Not a string pool: the loop operands in `FUN_801CED68` walk a table at `0x801CEF40` on a `0x84` stride, and the text inside each record is Shift-JIS read as little-endian `u16`. The image is a USA-build dev module carrying Japanese dev text - 34 of 37 SCUS `jal` targets land on this disc's function heads, where the foreign-build control scores 0 of 42 - so "Japanese text" is not evidence of a foreign build. Parser `legaia_asset::other3_roster`. |
| What is PROT 0975's undumped `0x801D4138` run? | resolved (PROT 0972's **code**, inherited at the same file offset) | `disassembly` | The 1,760 bytes from file `0x5920` are byte-identical to PROT 0972 at the same file offset - the packer's buffer is indexed by file offset and never cleared, so a short entry ends in the previous image's bytes. That is why the run has no prologue, no `jr ra` and no caller, and why it is not 0975's own jump table, data block or function interior. [`disc-coverage.md`](../../tooling/disc-coverage.md). |
| Which of PROT 0978 / 0979 / 0980 is the dance overlay? | resolved (**only** 0980) | `disassembly` | 0978 is `field_back_read`, the staged background loader; 0979 is the battle-intro image; 0980 is the dance overlay. |
| The front-end mode chain, re-derived independently | resolved (the six stores, the four-word edge and the INIT hand-offs all hold) | `disassembly` | A three-form opcode scan for stores to the mode word over 84 images finds **53** store sites, and every row of the boot chain, the mode-change edge's four cleared `gp` words and the per-handler INIT hand-offs match it. Mode 18's hand-off is `li v0,0x13` at `0x801CE8D4` then the store at `0x801CE8DC`, inside `0x801CE844` in PROT 0902. |
| Which mode does the retail title screen run under? | resolved (**card** mode `0x17`, not `0x10`) | `disassembly` | The front end is a chain of six mode stores, each written by the handler that hands off, not by a table's `next` field: `0x8001D5B8` -> `0x10`, `0x801CEC94` -> `0x11`, `0x801CF4D4` -> `0x16`, `0x80025974` -> `0x17`, `0x801DFC00` -> `0x02`, `0x80025E50` -> `0x03`. Mode `0x10` is one frame of logo INIT; the alternate arm at `0x801CF4E4` is the dev CONFIG route on the entry word. `0x80025974` is `li v0,0x17` + `sh v0,-0x47c4(at)` at `0x8002596C`; `0x801CEC94` is the `sh` in a `jal` delay slot. See [`boot.md`](../../subsystems/boot.md). |
| What reads the title entry word `_DAT_8007BB00`? | resolved (two readers, both real) | `disassembly` | `0x801CF4B0` in the boot image and `0x801DD97C` in the title overlay. The port's counterpart is `title_overlay::ENTRY_WORD_ADDR`. |
| What does a mode-change edge clear? | resolved (**four** gp words, not three) | `disassembly` | The edge at `0x800161F4` clears `0x8007B938` alongside the words at `gp+0x538` and `gp+0x55C`; `gp+0x564` and `gp+0x494` are mode *copies*, not cleared state. |
| `_DAT_8007B8C2` - how many writers? | resolved (exactly one) | `disassembly` | `main()` at `0x80015F08` (`sh v0,0x5aa(gp)`), confirmed across 70 access sites in 84 images. The build-mode selector is set once at startup and never re-written, which is what makes the dev/retail fork in `FUN_8001F87C` a load-time decision. |
| `_DAT_8007B8C2` polarity, and its writer | resolved (`!= 0` is retail, `== 0` is dev) | `disassembly` + `capture` | [details ↓](#_dat_8007b8c2-polarity-and-its-writer) |
| Actor-VM (`FUN_801D6628`) program source - which carrier, what selects one | resolved (menu-overlay-resident program table) | `disassembly` | The interpreted programs are data in PROT 0899's own data segment (file `0x16260..0x16740`), one per `jal FUN_801D6628` caller via `lui`+`addiu` (or a forwarded register); byte 1 of each instruction indexes the window descriptor table at `0x801E4738`, making the VM the menu's window-widget choreographer. No per-scene carrier exists - resolution is per-boot. Spec [window-script.md](../../formats/window-script.md); scanner `legaia_asset::widget_script::scan`; engine wiring `engine-core::menu_widget`. Sprite-VM readings: [re-do-not-re-walk.md](../re-do-not-re-walk.md#menus--ui). |
| `title.pak` PROT entry | resolved | `capture` | [details ↓](#titlepak-prot-entry) |
| Does `FUN_801CE9C0` pin the per-logo quads? | resolved (no - it uploads; the quads are a descriptor table in the same image) | `disassembly` | [details ↓](#the-publisher-logo-quads) |
| The title menu's law (`FUN_801DD35C` sub-mode `0x10`) | resolved + ported on both hosts | `disassembly` | [details ↓](#the-title-menus-law) |
| `FUN_801D71F0` and its "shared armament placer" `0x801E5AE8` | resolved (one routine, `FUN_801E5A08`, unreferenced) | `disassembly` | [details ↓](#fun_801e5a08-the-per-slot-equip-applier) |
| Save-screen info-panel view mode `4` ("Return") | resolved (dead - the grid has 15 cells) | `disassembly` | [details ↓](#the-dead-return-view-mode) |
| Title screen mode-table PROT | resolved (no row is named for it - it runs under the `CARD` mode pair 22/23; `FUN_801DD35C` is PROT 0899 `+0xEB44`) | `disassembly` | [details ↓](#title-screen-mode-table-prot) |
| Load-screen panel 9-slice geometry | resolved (engine renders byte-perfect) | `capture` | Retail composes the 81×29 panel at dst `(6, 4)` from 14 textured-sprite primitives (GP0 cmd `0x64`) sampling the system-UI sheet with CLUT `(32, 511)`; no interior fill sprite is drawn - the "marbled blue" look is the dimmed title art showing through the empty middle of the frame. Per-tile rects: [`save-screen.md`](../../subsystems/save-screen.md#pinned-9-slice-tile-rects-system-ui-tim-clut-row-2), exported as `legaia_asset::title_pak::OVERLAY_SYSTEM_UI_PANEL_*` and emitted by `legaia_engine_render::save_select_chrome_draws_for` (test `save_select_chrome_emits_9slice_panel_and_pills`). |
| Key-item area consumers (`0x800859E8..0x80085A40`) | resolved (narrow negative; the reader enumeration is closed) | `disassembly` | [details ↓](#key-item-area-consumers) |
| XP-table source + reader | resolved + ported | `capture` | [details ↓](#xp-table-source--reader) |
| New-game world-state seed store widths (`FUN_80034A6C`) | resolved (the port matches the disassembly) | `disassembly` | [details ↓](#new-game-world-state-seed-store-widths) |
| Overlay identity from the disc (static extraction) | resolved (pipeline `legaia_asset::static_overlay`) | `capture` | [details ↓](#overlay-identity-from-the-disc-static-extraction) |
| SCUS recomp gap - render/GTE + boot/init clusters | resolved (aliases + libgte residue + dev tooling; `main()` documented) | `disassembly` | [details ↓](#scus-recomp-gap---rendergte--bootinit-clusters) |
| Options/menu overlay PROT entry | resolved (RAM-verified; PROT 0899 @ `0x801CE818`) | `capture` | The options/pause/inventory-equipment-status menu overlay is **PROT 0899**, not 0896: `FUN_801CF650`'s signature byte-matches PROT 0899 file `0xe38`, and the `.text`+`.rodata` prefix is byte-identical across six menu-open saves. It is the VA-alias sibling of the field overlay 0897 in slot A - the menu overlay replaces the field overlay at the base. |
| PROT 0896 (`bat_back_dat`) identity | resolved | `capture` | The unique ~`0x9000`-byte head is the **vestigial Japanese-build field-menu / config / status overlay** - the debug-string sibling of the English retail menu overlay PROT 0899 (same `~0x801D0000` window-renderer VA family, a `"FWIN ERR %d"` printf at file `0x3D4`, `0x414`-byte char-record indexing). 0899 ships the English label set with zero `FWIN`; a signature scan finds 0896 resident in **0** of 140 states (control: English "Battle Voices" resident in 10), so the USA build never loads it. [details ↓](#prot-0896-bat_back_dat-identity) |
| Slot-A scene-overlay family beyond field/battle/menu | resolved (in the static map) | `disassembly` | The rest of the slot-A (`0x801CE818`) VA-alias family is read from the disc: **0970 cutscene_str** (STR/MDEC FMV, modes 26/27) and the minigame overlays **0972 fishing / 0975 slot_machine / 0976 baka_fighter / 0980 dance** (the mode-24 `0x3E` door-warp sub-id slots 0/3/4/6), each cross-checked by a documented function landing on a prologue at the base. Found with `asset overlay scan` + the leading dev string. "slot_machine = 0973 @ `0x801CA818`" is a phantom base: that image sits in 0973's over-read tail, and the canonical entry 0975 recovers `0x801CE818` and is the one the warp streams. |
| "world-map / save / shop" overlay PROT entries | resolved (not separate entries) | `disassembly` | The world-map / overworld controller `FUN_801E76D4` lives in the **field overlay 0897** (base+0x18EBC), and the save-slot dispatcher `FUN_801DC6B4` + the shop/buy session live in the **menu overlay 0899** (save at base+0xDE9C) - each function's instruction signature byte-matches only that one entry (`asset overlay find-sig`). So "world-map", "save", and "shop" are *subsystems* of existing slot-A overlays, not separate PROT entries; recorded in the 0897 / 0899 map notes. |
| PROT 0977 / 0978 extraction + the dump re-key | resolved | `disassembly` | [details ↓](#prot-0977--0978-extraction--the-dump-re-key) |
| Slot-B capture-module band `0935..0966` per-entry identity | resolved (statically derived, capture-corroborated) | `disassembly` | [details ↓](#slot-b-capture-module-band-09350966-per-entry-identity) |
| Phantom-VA sweep of the PROT 0897 imports | resolved | `disassembly` | [details ↓](#phantom-va-sweep-of-the-prot-0897-imports) |
| Debug flag `0x8007B98F` | resolved (the MSB of the debug-mode word `_DAT_8007B98C`) | `disassembly` + `capture` | [details ↓](#_dat_8007b98f-is-byte-3-of-the-debug-mode-word-_dat_8007b98c) |
| New-Game opening chain + narration roller | resolved (chain + caption + roller + prologue gold grade; far-geometry residual resolved-negative) | `capture` + `disassembly` | [details ↓](#new-game-opening-chain--narration-roller) |
| Overlay-loader index off-by-2 - remaining ripple | resolved (slot A reconciled; slot-B per-spell identity capture-pinned) | `capture` + `disassembly` | [details ↓](#overlay-loader-index-off-by-2---remaining-ripple) |
| Slot-B overlay cluster (`0900..0969`) per-entry identity | resolved for every entry | `capture` + `disassembly` | [details ↓](#slot-b-overlay-cluster-09000969-per-entry-identity) |
| PROT 0968 - what it is, who loads it, and how big it really is | resolved - identity and extent by disassembly, residency by capture | `capture` | [details ↓](#prot-0968---the-cort-battle-stage-overlay) |
| `0x80010390` - the SCUS word that looked like a lead on 0968 | resolved: it is the slot-B overlay destination pointer, shared by every slot-B entry | `disassembly` | [details ↓](#0x80010390-is-the-slot-b-overlay-destination-pointer) |
| Who registers the MDECin DMA callback wrapper `FUN_801CFE98`? | resolved (nobody - linked libpress residue; retail hooks only the MDEC-out twin) | `disassembly` (the residue reading `inference`) | Zero references in eight forms over 1234 images (SCUS, 31 based overlays, every raw PROT entry). Its twin `0x801CFEBC` - the same nine instructions with channel 1 - is what PROT 0970 calls, at `0x801CF524` (clear) and `0x801CF9C4` (install). See [`cutscene.md`](../../subsystems/cutscene.md). |
| PROT 0895 (`init.pak`) identity + base | resolved (slot A `0x801CE818`; mode 16's whole body is its `FUN_801CE9C0`) | `disassembly` | Base recovered from the image's own `jal` graph (8 votes, 22/25 pointer resolution, 7 string anchors); `0x801CE9C0` is a clean `addiu sp,sp,-0x230` entry at file `+0x1A8`, 784 bytes, building the four logo primitive records and storing mode `0x11` at `0x801CEC94`; the code region closes byte-exactly at `+0x216C` (20 functions) ahead of the first TIM at `+0x21C4`. SCUS `FUN_8002612C` (mode 16) is a frame, that `jal`, and an epilogue. |
| Does anything draw the `init.pak` WARNING screen? | resolved-negative (the TIM is uploaded and never drawn) | `disassembly` + `capture` | PROT 0895 uploads the health-warning TIM to VRAM `(704, 0)` and gives it descriptor `1` of the six-record sprite table at `0x801F369C`, but none of the five `FUN_801CFBB8` call sites in that image passes id `1`, nothing else on the disc references the table, and a boot capture never draws from that page. The USA build loads the screen and skips it. See [`boot.md`](../../subsystems/boot.md#the-health-warning-is-never-drawn). |
| Which title sub-mode does a cold boot show - `0x02` or `0x10`? | resolved (`0x10`, always) | `disassembly` + `capture` | [details ↓](#a-cold-boot-always-shows-title-sub-mode-0x10) |
| How the slot-B module pager turns a request into a PROT entry | resolved (`extraction entry = a0 + 895`, with no upper bound) | `disassembly` | `FUN_8003EC70` adds a constant `895` to its argument and pages that extraction entry into slot B; nothing clamps the top of the range. Its site at `0x8005269C` passes `_DAT_8007B64A + 71`, so selector byte values `2` and `3` reach PROT `0968` and `0969` while `0` skips the load entirely. The selector's writers are in [`battle.md`](../../subsystems/battle.md#stage-overlay-dispatch-the-0x47-loader-band); the one for `2` is [below](#prot-0968---the-cort-battle-stage-overlay). |
| Where does PROT 0896 link? | resolved (`0x801D4DF0` - and it calls no entry of this disc's executable) | `disassembly` | `recover_base` reports it on ten corroborating targets of eleven distinct internal `jal` targets; all 218 internal `j` instructions land inside the file there and none does at `0x801C5818` or `0x801C0000`; ten of the eleven `jal` targets land on an `addiu sp, sp, -X` prologue; and three runs of consecutive in-image VA words resolve every word, one of them holding the base itself. Of its 322 calls into the SCUS range, **zero** land on a `SCUS_942.54` function entry, where `0897` scores 1203 of 1307 and `0899` 793 of 884 - so no USA loader reaches it and no residency capture is owed. [details ↓](#prot-0896-bat_back_dat-identity) |
| What is PROT 0970's run above its one-shot init flag? | resolved (two **MDEC command packets**, then the register-pointer block) | `disassembly` | `0x801D0D58` holds the quant packet header `0x40000001`, with the luma and chroma matrices at `0x801D0D5C` / `0x801D0D9C`; `0x801D0DDC` holds the IDCT packet header `0x60000000` and a static IDCT matrix. `0x801D0E60..0x801D0E9B` is fifteen hardware-register pointers (`0x1F801080..0x1F8010B8` DMA channels 0-2, `0x1F801820` / `0x1F801824` MDEC0 / MDEC1, `0x1F8010F0` DPCR). Neither uninitialised data nor code; claimed off `legaia_asset::fmv_dispatch::MDEC_*_PACKET_VA`. |
| What does PROT 0970's `FUN_801CFCDC` do? | resolved (the MDEC **quant table upload** - not an output-rect stager) | `disassembly` | It copies sixteen words from `a0[0..0x40]` to `0x801D0D5C` and sixteen from `a0[0x40..0x80]` to `0x801D0D9C`, then calls `FUN_801CFFDC(pkt, 0x20)` twice - the quant packet `0x801D0D58` (`0x801CFD48`) and the static IDCT packet `0x801D0DDC` (`0x801CFD58`); only the quant packet is written. `FUN_801CFFDC` ORs `0x88` into DPCR, programs DMA0 (`MADR = pkt + 4`, `BCR = (n >> 5) << 16 \| 0x20`), writes the packet header to MDEC0 through the pointer at `0x801D0E90`, and starts DMA0 with `CHCR 0x01000201`. [`cutscene.md`](../../subsystems/cutscene.md) |
| What is PROT 0899's 3,552-byte zero run at file `0x1EB28`? | resolved (inter-asset fill, not uninitialised data) | `disassembly` | It sits between the save-menu atlas's end and the save-slot icon sheet at `0x1F908`, and no instruction in any image forms an address inside it (`find-gp-relative-refs.py --prot`, zero hits). An overlay's `.bss` is named by its own operands; this run is named by none. |
| What are PROT 0899's options-screen tables? | resolved (four, each bound to a consumer) | `disassembly` | The display layout `0x801E4404` (ten `[u16 row_id, u16 advance]`), the string-pointer table `0x801E442C`, the row-node list `0x801E44B8` (eight-byte nodes, zero-terminated - `lw` / `bnez` at `0x801D2A0C`) and the casino prize table `0x801E4518` (`0x60`-byte blocks of eight-byte rows). All four are formed by `lui`/`addiu` pairs in the options row renderer and the prize-exchange session. [`field-menu.md`](../../subsystems/field-menu.md#options-screen) |
| Where does PROT 0901's own content end? | resolved (file `0x252A`, donor PROT 0900) | `disassembly` | 0901's code ends at `jr ra` on file `0x24DC` (`0x801F8EB4`), and the run from `0x252A` opens mid-routine on 0900's epilogue of the routine at `0x801F8E6C` (its prologue at 0900 file `0x2494`); only 0900 references it. That is the packer-buffer inheritance shape; a cut at `0x26B0` is not the boundary, and 0900 / 0901 are not shifted copies of each other (an entry-size over-read artifact). |

### A cold boot always shows title sub-mode `0x10`

*Status:* resolved. Evidence: `disassembly` + `capture`.

`FUN_801DD35C`'s `Init` arm (`0x00`) writes `state[+0x204] = 0x02` and overwrites it with `0x11`
whenever the entry word `_DAT_8007BB00` reads non-zero. On retail that always happens, so the
`0x02` two-row menu (rows y 107 / 120, confirm mask `0x44`, advancing to `0x14`) is unreachable
from a cold boot.

- **Entry word is always raised.** The boot `init.pak` sets it unconditionally - `li s2,0x1` /
  `sw s2,-0x4500(s0)` at `0x801CEB84` with `s0 = 0x80080000`, in the mode-16 body. The three
  sites that store zero back are all behind dev-flag or pad-hold arms.
- **A surviving `0x02` is rewritten.** The `AttractDelay` (`0x11`) arm writes `0x10` at
  `0x801DDAC4` when its hold has drained; the shared epilogue's exits at `0x801DFED8` /
  `0x801DFEF8` do the same.
- **Capture.** A per-vsync cold-boot poll sees `_DAT_8007BB00` go `0 -> 1` in the frame the
  master mode steps `0x10 -> 0x11`; the sub-mode is written `0x11` the frame after the title mode
  is entered and `0x10` about 75 vsyncs later; `0x02` is never observed. Returning from the
  attract FMV the word reads `2`, so the overwrite holds on the second entry too.
- **The tick.** PROT 0899 file `+0xEB44`, 12 104 bytes / 3 026 instructions, dispatching
  `0x801F0204` through the 24-word jump table at `0x801CF244` (slot `0x11` = `0x801DDA90`), with
  56 stores to that word and one shared epilogue at `0x801DFC3C`. `_DAT_8007BAB4` is its
  pre-attract hold, not an active-submenu index.

Falsified readings:
[the sub-mode word's address](../re-do-not-re-walk.md#the-title-sub-mode-word-lives-at-0x801dd920-and-0x02-is-a-screen-a-player-can-see),
[the slider clamp](../re-do-not-re-walk.md#the-title-slider-state-0xeb4-is-clamped-to-0-0x2c).
Owning page: [`boot.md`](../../subsystems/boot.md#a-cold-boot-always-shows-sub-mode-0x10-never-0x02).

### `_DAT_8007B98F` is byte +3 of the debug-mode word `_DAT_8007B98C`

*Status:* resolved - no byte-granular reader exists; the 32-bit word is the consumer surface.
Evidence: `disassembly` + `capture`.

`0x8007B98F` is the MSB (little-endian byte +3) of the debug-mode word `_DAT_8007B98C`. Nothing
reads or writes the byte on its own, so `SELECT+START` / a GameShark write of `0x8007B98F = 1`
sets the word non-zero and every `_DAT_8007B98C != 0` gate reads debug mode active.

- **Byte references:** zero in SCUS and every overlay dump (`ghidra/scripts/funcs/` has no
  `8007b98f`).
- **Word readers, SCUS:** `FUN_8001822c` (`8001822c.txt:500/533`), plus `80016230` / `80016444` /
  `800173bc` / `800188c8` / `8003cbf8` / `8004ad80` / `80025cb4`.
- **Word readers, overlays:** an aligned word search of the 23 static overlays finds 14
  `lw ...,-0x4674(reg)` reads (base reg `0x80080000`) in the field overlay 0897.
- **Sole `sw` writer:** the shared menu/title/save-init routine (`overlay_menu_801de234` /
  `overlay_title_801ddccc`, internal offset `0x4158`). It clears the word, so the gate does not
  survive scene initialisation and has to be held asserted for a session.
- **Not BIOS-zeroed:** the PS-X EXE header carries `b_addr = 0, b_size = 0`, so no BSS is cleared
  for this executable; the same holds for
  [`_DAT_8007B8C2`](#_dat_8007b8c2-polarity-and-its-writer).
- **Runtime (static recomp):** asserting the word and pulsing `SELECT + △` on controller
  **port 2** opens the game-owned developer menu - port 2 follows from the
  `_DAT_8007B850 &= 0xFFFF` mask, which puts every debug binding in the upper half. Forcing game
  mode 0 loads PROT 0971's full-screen configuration tester instead, as the `CONFIG INIT` row in
  [`boot.md`](../../subsystems/boot.md#game-mode-state-machine) says. The developer menu's MAP
  CHANGE appliers are resident in field overlay 0897, matching the 14 gate reads there.

Owning pages: [`boot.md` § Debug flags](../../subsystems/boot.md#debug-flags); combo table in
[`builds.md` § Debug input bindings](../builds.md#debug-input-bindings).

### New-Game opening chain + narration roller

*Status:* resolved - chain, caption, roller and prologue gold grade; the far-geometry brightness
gap is resolved-negative. Evidence: `capture` + `disassembly`.

The opening is a five-scene chain - `opdeene` → `opstati` → `opurud` → `map01` → `town01` - all
under master mode 3 with zero input. Its narration is a bottom-up scrolling crawl that runs in a
child context while the parent timeline keeps cutting the camera, and its gold look is a
palette-space collapse, not a depth cue.

- **Chain.** Each leg's record spawn is an exec-BP hit on `FUN_8003BDE0` (exactly 5): field-VM op
  `0x44` SPAWN_RECORD in the first three legs' entry scripts, the walk-on tile trigger
  (`FUN_801D1EC4` → `FUN_801D5630`) for `map01` / `town01`. The `FUN_801D1344` `town01` packet is
  the intro skip, not the hand-off gate. Name entry auto-opens from op `0x49` STATE_RESUME sub-op
  3 at `town01` P2[3] body offset `0x02c6` (`_DAT_8007B450` parks there); retail order is
  establishing pan → name entry → Vahn's walk-out.
- **Crawl.** Roller actor `FUN_80037174`, spawned as a child context by `CC F8 80 N` (`N` = page
  count). Retail opens every crawl non-blocking and holds only at the record's terminal `0x3F`
  SceneChange while narration is active. A cold-boot capture
  (`scripts/pcsx-redux/autorun_crawl1_capture.lua`) shows the eye cutting through the
  Genesis-grove foliage to the villager tableau while the creation crawl scrolls. The one-caption
  presenter is the separate `4C E1` balloon op (`FUN_8003C764` / `FUN_801DA7F0`).
- **Roller config.** `CC F8 E8 ...` (nibble-`E` sub-8 `0xE8`; handler `0x801E3378` in
  `overlay_0897_801e0c3c.txt`, reader `80037174.txt`) reads four signed-16 LE words and stores
  three into `_DAT_801C6EA4` (`sh` at `0x801E34B0` / `34B4` / `34BC`): `+0x4C` window top Y,
  `+0x4E` visible line count, `+0x50` scroll-cadence divisor. `word3` selects seed / pause /
  resume / kill and is never stored. Not op0 `0x88`, which writes `_DAT_80084628/...`.
  `RollerParams::for_scene` derives from the scene bytecode.
- **Caption.** *"It was the Seru."* is not text: a pre-rendered 112×32 4bpp TIM (two CLUT
  palettes = the fade steps) in PROT 0749 (`opdeene`) at LZS-decoded offset `0x01EC30`, VRAM
  `fb=(384,0)`, drawn as a screen-space textured quad. Every UI text/image draw path fires zero
  times in the caption window and the string is in RAM in no encoding (`autorun_text_census.lua`,
  `autorun_seru_blit_probe.lua`, a full-RAM dump). `tim-scan extracted/PROT/0749_opdeene.BIN`
  renders it.
- **Camera.** The per-frame mover is `FUN_801DC0BC` (`FUN_801DB510` is the follow / scroll
  camera); `FUN_801DD310` attaches ten `(start, end)` pairs plus one shared progress / duration /
  curve to a mover actor, so a glide runs in parallel with the record that staged it and a beat
  landing mid-tween re-seeds every axis from the live pose. One curve applies to all ten axes -
  mode 1 is linear on pitch/yaw too (three independent beats, one a 2000+-frame yaw dolly) - and
  the `town01` arrival H glide (`P2[3] +0x00C4`, `apply` 600, H 412 → 512) is mode 4 ease-in-out
  (`op0 0x13 >> 2`; disc pin `town01_arrival_camera`). Eye-back depth is offset-trio slot 5,
  `0x800840B8` (no separate eye-distance scalar); the op-`0x45` params map to the camera globals
  and the rotation build is `FUN_800172C0` -> `FUN_80026988`.
- **Timing.** Record durations count retail display frames (op `0x4A` and the mover both
  accumulate `DAT_1F800393`); `FUN_8002519C` walks the actor lists in full every frame, so there
  is no hidden step parallelism
  ([`script-vm.md`](../../subsystems/script-vm.md#per-frame-scheduling)).
- **Gold grade.** The cutscene host rewrites every CLUT the `opdeene` bundle uploads to
  `L = max(r,g,b) → (L, max(L-1,0), L>>1)` (5-bit, STP preserved; 0 mismatches over 768 entries
  of terrain rows 509 / 508 / 501) and collapses authored colour packets to the amber family
  `~(M, 0.94M, 0.43M)`; neutral `0x80` ground quads stay neutral. Node `+0x78` (`IR0`) is 0 on
  every render node (`0x8007C34C..`) at every beat, so there is no per-node depth cue
  ([`re-do-not-re-walk.md`](../re-do-not-re-walk.md#field--locomotion)).
- **Far-geometry brightness (resolved-negative).** Retail spires / wings read
  `B/R ≈ 0.15..0.16` at brightness ~51 against the port's 0.27 at ~80; the tableau ground is
  identical. No CLUT-rewrite loop exists in overlay 0970 (28 functions, pure MDEC/STR), field
  0897 (690) or `SCUS_942.54` (945): the rewrite is a table/DMA upload. The gap is neutral
  packets on lit-descriptor prims, which retail draws through the GTE far/back colour
  `FUN_80029888` loads (`DAT_8007B788 = 0x00202020` in `opdeene`, `0x00FFFFFF` in `town01`) -
  the port's no-field-light-op boundary
  ([Field decoration path](world-map.md#field-decoration-path---does-it-dispatch-the-ncc-light-handlers)).

Port: a non-blocking crawl in the field timeline, paced off the 60 Hz sub-clock
(`opening_chain_wall_time`: within ~4 % of retail wall time, each leg short by the unmodelled
scene-load window, so its bands are asymmetric); `legaia_engine_vm::camera_mover` (2471 of 2480
sampled axis values against a live capture, the rest inside the probe's read skew;
`camera_mover_recomp_oracle` under `LEGAIA_RECOMP_TRACE_DIR` replays the snap / mode-1 / mode-2 /
mode-4 beats bit-exact); `Renderer::set_palette_grade` (`palette_law_word` /
`palette_collapse_prim`, staged by `play-window` when `World::scene_color_grade` is active;
tableau ground `G/R` 0.890 against retail 0.88); `play-window` renders through `psx_camera_mvp`.

Owning page: [`cutscene.md`](../../subsystems/cutscene.md#in-engine-3d-opening-the-five-scene-new-game-chain) -
[crawl roller](../../subsystems/cutscene.md#narration-playback---the-crawl-roller-fun_80037174),
[roller operands](../../subsystems/cutscene.md#roller-op-operands-ghidra-traced),
[sepia grade](../../subsystems/cutscene.md#full-scene-sepia-grade-the-gold-prologue-look).

### Overlay-loader index off-by-2 - remaining ripple

*Status:* resolved - slot A reconciled; per-spell slot-B identity capture-pinned for every block
(player, evolved, flutes, enemy). Evidence: `capture` + `disassembly`.

The overlay loaders (`FUN_8003EBE4` / `FUN_8003EC70` → `FUN_8003E8A8(param + 0x381)`) resolve
against the in-RAM TOC at `0x801C70F0`, which is raw `PROT.DAT` from byte 0 (byte-verified against
the `door_warp_town01_to_map01` state). Extraction indices slice entry starts two words higher, so
the loaded entry is **extraction `param + 0x37F`**; a `param + 0x381` attribution is 2 high.

**Slot A** (each content- or prologue-anchored; [`boot.md`](../../subsystems/boot.md)): field
0897 = mode 2, battle 0898, menu 0899 = mode 22, STR-path 0969, cutscene 0970, debug menu 0971 =
mode 0, the seven `0x3E` minigame slots, efect-test 0979 = mode 8.

**Slot B per-spell stagers.** The player span `0x81..=0xA0` is one unbroken linear run:
`loader id = spell - 0x79`, `extraction = loader id + 895 = 903 + (spell - 0x81)`.

| Spell ids | Loader ids | Extraction | What | Pin |
|---|---|---|---|---|
| `0x81..=0x8B` | `0x08..0x12` | 903..=913 | base Seru summons (0907 = Nighto) | one mid-cast state per spell |
| `0x8C..0x95` | `0x13..0x1C` | 914..923 | evolved Seru: Gola Gola 914, Mushura 915, Aluru 916, Barra 917, Kemaro 918, Spoon 919, Slippery 920, Iota 921, Puera 922, Gilium 923 | eight by mid-cast states (`evolved_summon_binding`); `0x90` / `0x91` by injected casts |
| `0x96` / `0x97` | `0x1D` / `0x1E` | 924 / 925 | rare-Seru flute summons Lippian / Spikefish | `flute_lippian_midcast` / `flute_spikefish_midcast`, probe `autorun_flute_cast.lua` |
| `0x98` | - | 926 | unused: a one-sector `jr ra` stub | disc bytes |
| `0x99` | `0x20` | 927 | Evil Seru Magic (Juggernaut) | mid-cast mednafen state |
| `0x9A..0x9D` | `0x21..0x24` | 928..931 | Sim-Seru Palma / Mule / Horn / Jedo | mid-cast mednafen states |
| `0x9E..0xA0` | `0x25..0x27` | 932..934 | Ra-Seru Meta / Terra / Ozma (untitled entries head with a pre-linked slot-B pointer table) | mid-cast mednafen states |

- **Method.** The loader-B current id (`gp+0x934` = `0x8007BC4C`) is read out of catalogued save
  states (`scripts/pcsx-redux/match_prim_groups_to_disc.py::extract_ram` walks the gzipped-protobuf
  `.sstate` to the RAM blob), together with the predicted entry byte-resident at slot B
  `0x801F69D8`. The id is a last-load tracker - an idle Begin/Run-menu state holds a stale `6` -
  so only in-cast states are evidence.
- **Gimard.** All three player cast states (`gimard_summon_start` / `_visible` /
  `_burning_attack`) hold `id = 8` → 0903 through spawn, steady render and attack; the id never
  moves off 8, so 0900 does not overwrite the stager mid-cast. The enemy Gimard "Fire Tail" frames
  (`battle_gimard_tail_fire_a/_b`) hold `id = 5` → 0900: the enemy special pages the move-FX
  module, not a stager.
- **Attack-titled heads are display names**, not separate uses: 0907 "Hell's Music" (Nighto; the
  dance overlay has no slot-B loader callsite), 0927 "Dark Eclipse", 0924 "Ultimate Rave" (the
  failed-kill banner; a landed kill shows "Ultimate Death"), 0925 "Blowfish", 0918 "Canine Fangs",
  0919 "Holy Eyes".
- **Evolved block.** All ten entries trim to move-VM stagers with 4..67 spawn sites
  (`EVOLVED_SUMMON_STAGER_PROT`, disc-gated `summon_overlay_block`). The injected casts
  (`autorun_evolved_cast.lua`; states `evolved_0x90_midcast` / `evolved_0x91_midcast`) write the
  spell into the caster's record spell list and MP into the record and battle-actor `+0x150`: the
  battle Magic submenu reads the record spell list live, the MP gate reads actor `+0x150`. Loader-B
  flips when the slot-B load is *queued*, so the probe saves 90 frames after the flip; the image
  then matches 100 % over the entry's full LBA footprint. The two `0x4000` render-mode carriers
  (916 Aluru, 921 Iota) are player casts, so neither seats a live render-mode part.
- **Flutes.** SummonFlute items (effect classes 126 / 127) enqueue the spell id directly, so they
  ride the same stager mechanism.
- **Enemy arm** (six catalogued final-boss Cort mid-cast states), id band `0x2B..0x47` →
  `938..966`: Mystic Circle `0x2B` → 938, Mystic Shield `0x2D` → 940, Guilty Cross `0x31` → 944,
  evolved-form Final Crisis / Ultra Charge `0x42` / `0x43` → 961 / 962, Cort's Evil Seru Magic
  `0x47` → 966. The player and enemy arms of one spell ship separate stagers (927 vs 966).

**The 0977 sub-id-5 minigame.** 0977 ("Ronginus") is the mode-24 case-5 door/init slot: the
`0x801CEA6C` init prologue, the arena monster-name roster and `other6` dev paths. The Muscle Dome
match SM `FUN_801D0748` and all its data live in the battle-action overlay PROT 0898 - the arena
is a mode of the battle engine. `asset overlay find-sig` of the controller prologue
(`lui v0,0x8008; lw v0,-0x42dc(v0)`, reading the ctx `_DAT_8007bd24`) lands on 0898 at base
`0x801CE818`, file offset `0x1F30`, and the deck / sub-draw / victory tables resolve in-overlay
(`legaia_asset::muscle_dome::verify_resident`); the Duckstation `overlay_muscle_dome.bin` capture
is that overlay's slot.

**Port.** `OVERLAY_PROT_BASE` carries the extraction-space `0x37F` (the host chain
`prot_one_shot_load` → `entry_start_lba_retail` consumes extraction indices; its `toc` array starts
at raw dword 2), with a unit test documenting the raw-vs-extraction shift.
`engine-core::summon::summon_stager_prot_entry` maps spell id to stager entry.

### Muscle Dome match shape: an ordinary battle ladder, not a card battle

*Status:* resolved. Evidence: `disassembly`.

The arena is a ladder of ordinary battles. The four "cards" are the four d-pad direction commands
`0xC..=0xF`, each carrying that fighter's own AP cost - the input a normal battle command screen
takes, bounded by AP.

- Course descriptor table `0x801D1A08` in PROT 0977: three `{ i32 rounds; ptr first }` records
  walking 29 `{ u32 label; u32 monster_id }` round records at `0x801D1920`.
- `FUN_801D1510` stores the round's id into formation slot 0 at `0x8007BD0C`.
- Courses are 8 / 8 / 13 rounds, matching the populated rows of the score table at `0x801D1860`;
  the 29 ids resolve against PROT 867 to the curated `casino.toml` line-ups 29 of 29 in order.
- The HP ratio is `x 100`: the "out of 108" (`0x6C`) reading consumes only part of the compiler's
  shift-add chain at `0x801d0f38..0x801d0f4c`.
- `0x8007BD0C` is the formation cell, not a battle-type byte: the strip's gate reads "the first
  enemy is monster `0xB6`" - Koru, the one four-turn timed boss - and no dome round fields that id.

Port: `engine-core::muscle_dome` (`parse_course_ladder`, `course_score_cell`, `resolve_turn`
playing whole strings per actor, `DomeDamageModel` - the one retail damage kernel both hosts
resolve through). Falsified readings:
[`re-do-not-re-walk.md`](../re-do-not-re-walk.md#muscle-dome-was-never-a-card-battle).
Owning page:
[`minigame-muscle-dome.md § Course ladder`](../../subsystems/minigame-muscle-dome.md#course-ladder-the-opponent-per-course-round).

### The dome runs two state machines; the outer one is the contest

*Status:* resolved. Evidence: `disassembly`.

The battle round driver `FUN_801D0748` is the inner machine and has exactly one contest-gated arm
(`0x801D322C`, the flee path). The contest - which `(course, round)` is staged, whether the run
continues, what a cleared leg is worth and what the run pays - is a second machine wholly in PROT
0977.

- **Contest machine:** `FUN_801CEA6C`, re-entered after every leg, and the hub `FUN_801CF870`
  dispatching `DAT_801D1A78` through a 51-entry jump table at `0x801CE990`.
- **Course / round:** packed in the low byte of the mode-24 sub-id word `_DAT_8007BAC0`
  (`course = ((w-1) & 0xFF) >> 4`, `round = (w-1) & 0xF`); a finished leg is `w += 1`.
- **Story flags:** `0x536` / `0x537` / `0x538` pick which course opens; only the Master course's
  length is clamped, by `0x378` / `0x382` / `0x471`.
- **Leg survived:** `DAT_8007BD60 & 0x80` at `0x801CEDD8`, cleared by the battle's own `0x5A`
  party-wipe scan - not an arm of `FUN_801D0CD4` or `FUN_801D0068`. `settle_contest`'s
  `continuing` input is therefore derived, not prompted.
- **Tally rows:** three of the six are HP recovery (`round*2`, `min(turns,8)`,
  `[8,12,4,2][outcome]`, each `× max_hp / 100`) draining into the restore accumulator
  `DAT_801D1AC8`; only the `(course, round)` score cell reaches the coin tally.
- A cleared course banks its whole score row: the Master reward is **13830** (the curated table's
  13856 is wrong).

Port: `engine-core::muscle_dome::DomeContest`, driven by `World::report_muscle_leg` /
`World::settle_muscle_contest` and the browser's `muscle_contest_*` bindings. Owning page:
[`minigame-muscle-dome.md § Two state machines`](../../subsystems/minigame-muscle-dome.md#two-state-machines-not-one).

### Battle arts-input UI decomposition (dome = standard battle input)

*Status:* resolved. Evidence: `capture`.

The arts command input (the `FUN_801D0748` state-`0x50` arm) is decomposed piece by piece from a
live dome match in the static recomp (slot-5 savestate + scripted pad), read through the runtime's
`gpu_frame_dump` GP0 ring plus a same-moment full-VRAM dump.

| Piece | Source |
|---|---|
| High / Left / Right / Low chips | widget-page hexagon pieces + baked label strips + diamond ends |
| Input bar | tiled maroon widget bar filling with command pennants at cost-wide pitch |
| AP plate (right) | reads the Spirit gauge; the entry budget's only visible form is the bar |
| Triangle list | 5-row-per-page learned-arts window: system-UI interior tiles under a `0x40..0x88` gouraud; name / arrows / AP columns are the SCUS arts-name table's, drawn through orange sub-palette 15 |
| Green Triangle circle | its own 64x32 gap TIM at `PROT.DAT 0x7B00` |

Behaviour: per-press `ctx+0x6dc` debit + `actor+0x1df` append; auto-end on exhaustion
(`0x50 -> 0x5a`); the `0x5a -> 0x6e` Begin|Reselect chain; Triangle inert at learned-art
constant 0.

Port: `engine-core::muscle_dome` (`selection_exhausted` / `reset_selection`),
`web-viewer::minigames_muscle` (`arts_input` pieces + `muscle_arts_list_json`). Owning page:
[`minigame-muscle-dome.md § Arts command input`](../../subsystems/minigame-muscle-dome.md#arts-command-input-packet-pinned).

### Slot-B overlay cluster (`0900..0969`) per-entry identity

*Status:* resolved for every entry. Evidence: `capture` + `disassembly`.

The slot-B buffer (link base `0x801F69D8`) timeshares the `0900..0969` blobs. Each is extracted
statically at the link base and cross-checked by in-file self-pointer resolution
(`static_overlay::pointer_resolution`, ≥70%). A shape census over the cluster (per-entry
`lui 0x801F/0x8020; addiu` in-file resolution, `FUN_80021B04` / `FUN_80050ED4` spawn-call counts,
damage-wrapper `jal` words) corroborates slot-B linkage for every entry except slot-A 0902. The
CDNAME label is `xxx_dat` (a dev placeholder) across the cluster and identifies nothing.

| Entries | Identity |
|---|---|
| 0900 / 0901 | the slot-B default render pair: `FUN_80025BA0` loads param 5 or 6 by flag `DAT_8007B6A8` (0900 field scenes, 0901 world-map scenes) |
| 0902 | GAME OVER - a **slot-A** image (`FUN_8003EBE4(7)` in the mode-18 init); a slot-B reading of it is a `pointer_resolution` false positive |
| 0903..0913 | player summon stagers, spells `0x81..=0x8B` |
| 0914..0923 | evolved-Seru stagers, `0x8C..0x95` |
| 0924 / 0925 / 0926 | rare-Seru flutes Lippian `0x96` / Spikefish `0x97`, and the unused-`0x98` one-sector `jr ra` stub |
| 0927..0934 | Evil Seru Magic (Juggernaut), the Sim-Seru quartet, the Ra-Seru trio (`0x99..0xA0`) |
| 0935..0966 | the capture-class cast-module band, `extraction = 935 + sub_id` - [its own section](#slot-b-capture-module-band-09350966-per-entry-identity) |
| 0967 | the battle sparring-tutorial overlay (capture-pinned, s5 needle sweep); battle-stage id `1` |
| 0968 | the evolved-Cort battle's stage overlay, battle-stage id `2` - [details ↓](#prot-0968---the-cort-battle-stage-overlay) |
| 0969 | the STR-path table the STR-mode init pages (`FUN_8003EC70(0x4A)`; [`boot.md`](../../subsystems/boot.md)), and Cort's form-transition module |

Per-spell pins for 0903..0934:
[the loader-index section](#overlay-loader-index-off-by-2---remaining-ripple).

**0969's battle-side load** uses the same gate as 0968's. At `0x801E6D04` the battle SM reads
`*(u8 *)0x8007BD0C` - the first formation monster id - compares it to `0xB5` and pages `0x4A`
(`jal 0x8003ec70` at `0x801E6D14`, `overlay_battle_action_801e6968`). The guard just above is
`lhu v0, 0x14C(actor)` on slot 3 of the actor table `0x801C9370`, taken only when that actor's HP
has reached zero - so 0969 is paged when a Cort form dies. The `0xB5` is not the Lapis Wave spell
id: the byte the branch reads is the formation id, and formation `0xB5` is Cort (monster-archive
id 181).

### `0x80010390` is the slot-B overlay destination pointer

*Status:* resolved - a slot constant, not a lead on 0968. Evidence: `disassembly`.

`0x80010390` is a SCUS-resident global holding the slot-B overlay load address `0x801F69D8`, and
`0x8001038C` is its slot-A twin. It is the one literal-word hit for `0x801F69D8` outside the
overlay band (`SCUS_942.54 +0x390`) in an address-reference sweep over all 1234 images, and it
says nothing about any one slot-B entry.

- `FUN_8003EBE4` reads `*(0x8001038C)` at `0x8003EC24`; `FUN_8003EC70` reads `*(0x80010390)` at
  `0x8003ECCC`. Both then run `FUN_8003E8A8(param + 0x381)` and `FUN_8003E800` into that buffer.
- The two loaders differ only in the residency tracker they stamp: `gp+0x924` (slot A) vs
  `gp+0x934` (slot B).
- No instruction in `SCUS_942.54` stores to either word: a sweep for `lui 0x8001` paired with a
  memory op at `+0x38C` / `+0x390` finds nine sites, all `lw`.
- In-band hits for the base are equally uninformative: ~70 sibling images share the VA, and every
  `jal` / `j` / branch to it is that sibling's own code. When overlays share a load base, a
  reference to the base is a reference to the slot, not to a tenant.

### PROT 0968 - the Cort battle stage overlay

*Status:* resolved. Evidence: `capture` (residency), `disassembly` (loader chain and extent).

PROT 0968 is the stage overlay of the evolved-form Cort fight: a 7-state scripted battle set-piece
paged into slot B by battle-stage id `2`. Only 2600 bytes of its two sectors are its own.

**Loader chain**

- Stage overlays are paged by a computed parameter, so no `0x49` constant exists anywhere:
  sub-states `0x0E` / `0x10` of the battle loader read the stage-id byte `_DAT_8007B64A` and call
  `FUN_8003EC70(stage_id + 0x47)` ([`battle.md`](../../subsystems/battle.md)).
- The selector is a hardcoded override ending the formation fix-up in `FUN_80055B6C`, the battle
  scene initialiser, at `0x80055D2C`:

```
lbu   v1, -0x42f4(v1)   ; v1 = *(u8 *)0x8007BD0C - the first formation monster id
addiu v0, zero, 0xb5
bne   v1, v0, 0x80055d48
addiu v0, zero, 2       ; delay slot
sb    v0, -0x49b6(at)   ; *(u8 *)0x8007B64A = 2 - the battle-stage id
```

- Formation id `0xB5` is monster-archive id 181, Cort's evolved second form (HP 65535). The
  first-form fight is a separate formation headed `0xB4` (id 180, HP 50000); both carry the
  display name "Cort" (`asset monster-archive --id 181` on PROT 867).
- So stage id `2` → param `0x49` → extraction 968. The same byte against the same constant pages
  0969 mid-battle when a Cort form's HP reaches zero.

**Residency**

- `cort_evolved_battle_first_menu` (PCSX-Redux; scene `jouine`, first command-input screen, before
  any cast; fingerprint in [`scenarios.toml`](../../../scripts/scenarios.toml)): loader-B tracker
  `0x8007BC4C = 0x49`, entry 968 100% byte-resident at `0x801F69D8` over its `0xA28` extent,
  formation head `*(u8 *)0x8007BD0C = 0xB5`. Script
  [`check-0968-residency.py`](../../../scripts/mednafen/check-0968-residency.py). It is the same
  observation pair that pins 0967 for the Tetsu tutorial.
- The bracketing field states `cort_evolved_approach_cutscene` / `cort_evolved_pre_battle` show
  the co-resident library 0900 at 100% with tracker `0x05`, so the page-in is the battle load.
- The six `cort_*_mid_cast` mednafen states read formation head `0xB4` (four first-form) or `0xB5`
  (two evolved-form), and in every one the slot-B page is 100% byte-identical over `0x1000` bytes
  to that state's cast stager (0938 / 0940 / 0944 / 0961 / 0962 / 0966) with the tracker reading
  the stager's id; 0968's window matches at chance (10.5-12.1%). A cast stager is a full slot-B
  page, so the fight's first special or summon evicts the stage overlay.
- `_DAT_8007B64A` reads `0x00` in all nine mid-fight / bracketing states: it is transient around
  the load, not a mid-battle marker.

**Extent**

- Own content is file `0x00..0xA28`: a 7-entry dispatch table at offset 0 (every target inside the
  window) and code from `0x1C`. Every `jal`, `j` and LUI+ADDIU materialisation in the window
  resolves inside it, into `SCUS_942.54`, or into the co-resident slot-A battle overlay; none
  reaches past `0xA28`. The first instruction reads the battle-context pointer `_DAT_8007BD24`
  and writes `ctx[+0x6D6] = 0x100`.
- The trailing 1496 bytes are byte-identical to PROT 0967 at the same file offsets, cut mid-string
  at the sector boundary - packer-buffer inheritance from the tutorial overlay. They contain
  `FUN_801F747C`, a text-box placement routine dispatching `jr *(0x801F6B48 + style*4)`: in 0967
  that address is a 10-entry jump table at file `0x170`, right after 0967's 91-entry step table;
  in 0968 file `0x170` is live code. The window also materialises `0x801F7C80`, a string that
  exists only in 0967, past 0968's end.
- Structural measures taken over all 4096 bytes ("pointer-table head, 10 of 11 self-pointers, 2+8
  spawn calls") mix the two modules.

**What it calls**

| Callee | Role | Calls |
|---|---|---|
| `FUN_80050ED4` | summon / effect-actor pool allocator | 8 |
| `FUN_80021B04` | actor spawn | 2 |
| `FUN_80024E80` | screen-fade spawn | 2 |
| `FUN_8003541C` | text actor | 1 |
| `FUN_8004FCC8` | cue / streamed-voice dispatch | 1 |
| `FUN_80058490` | `MoveImage` VRAM blit | 1 |
| `FUN_80035F04`, `FUN_80050E74` | - | 1 each |
| `0x801D829C` | in the co-resident slot-A battle overlay | 4 |

That is the external-call family of the tutorial overlay 0967 minus the tutorial's prompt helpers.
Owning page: [`battle.md`](../../subsystems/battle.md#stage-overlay-dispatch-the-0x47-loader-band).

### PROT 0977 / 0978 extraction + the dump re-key

*Status:* resolved - both entries are in the static overlay map and every `overlay_0977_*` /
`overlay_0978_*` dump resolves. Evidence: `disassembly`.

The static map ([`static-overlays.toml`](../../../crates/asset/data/static-overlays.toml)) carries
**0977** (`arena_init`, the Muscle Dome door/init slot-A overlay at `0x801CE818`, anchor
`FUN_801D0F60`) and **0978** (`field_back_read`, slot B `0x801F69D8`, pinned by the SCUS
`FUN_80025358` state-2 call into `FUN_801F6B24`); `asset overlay verify` reproduces both
fingerprints from the disc. With those images indexed, `check-dump-base-integrity.py` classifies
all 22 dumps in the two families and none is `NOT_FOUND`:

| Dumps | Verdict | Bytes live in |
|---|---|---|
| `801d050c` `801d08ec` `801d1288` `801d1308` `801d14b0`, `slotA_801d0f60` | MATCH | 0977 at the printed VA |
| `other_game_801f6b24` | MATCH | 0978 at the printed VA |
| `0977 801c085c` `801c0f48` `801c2748` | SHIFTED `+0xE818` | 0977 own code (`801C085C→801CF074`, `801C0F48→801CF760`, `801C2748→801D0F60` - the War God Icon settlement) |
| `0977 801c614c` `801c6268` `801c6804` `801c6cf8` | SHIFTED `+0xA018` | 0979 (`801C614C→801D0164`, `801C6268→801D0280`, `801C6804→801D081C`) |
| `0978 801c2b58` `801c3004` `801c39b8` | SHIFTED `+0xD818` | 0979 (`→801D0370` / `801D081C` / `801D11D0`) |
| `0978 801c5c58` `801c7b40` `801c82dc` `801c8b04` `801c8d0c` | SHIFTED `+0x9818` | dance 0980 (`801C5C58→801CF470` - the documented beat-clock SM) |

Each delta is one wrong base seen through an over-read footprint imported at `0x801C0000`:

- 0977's footprint holds its own `0x3800` bytes, then 0978 (`0x1000`), then 0979. Own-content
  prints re-key at `+0xE818` (`0x801CE818 − 0x801C0000`), 0979-stratum prints at
  `0xE818 − 0x4800 = +0xA018`.
- 0978's footprint holds 0979 from `+0x1000` (`+0xD818`) and the dance overlay from `+0x5000`
  (`+0x9818`).
- The two-hit `801c614c` signature (a duplicated 10-instruction run inside 0979) is disambiguated
  by the batch-constant delta: its program siblings resolve single-hit at `+0xA018`.
- A "`baka_fighter_0976` at `+0x5710`" hit is a cross-overlay duplicate of a sequence that MATCHes
  0977 at its printed VA.
- `801c2b58`, `801c3004`, `801c39b8`, `801c614c`, `801c6804` are four distinct routines of the
  field-battle-intro overlay 0979; two of the dumps are the same routine `FUN_801D081C` reached
  through two different wrong bases, which cross-checks the decode.

### Slot-B capture-module band `0935..0966` per-entry identity

*Status:* resolved - the per-entry map is static spell-table data in `SCUS_942.54`,
capture-corroborated. Evidence: `disassembly`.

A capture-class spell record (class byte `'c'` at stats `+0`) pages its cast module as
`FUN_8003EC70(record[+1] + 0x28)`, and the loader resolves extraction `param + 0x37F`, so the
module is **extraction entry `935 + record[+1]`**. Enumerating the `'c'`-class records yields the
complete map: the sub-id space covers `0935..=0966` exactly, with no orphan entry.

- Parser `legaia_asset::spell_names::capture_class_records` / `capture_module_prot`; the
  disc-gated `spell_names_real` test asserts band coverage and every independently pinned leg
  (capture-pinned boss stagers 938 / 940 / 944 / 961 / 962 / 966; playtest-pinned
  Delilas / Xain modules 952 / 953 / 958 / 959 / 960).
- Shape census: every band entry resolves its `lui 0x801F/0x8020; addiu` self-pointers in-file at
  the slot-B link base, spawns through `FUN_80021B04` / the `FUN_80050ED4` pool wrapper, and
  carries damage-wrapper `jal`s where the
  [battle-formulas wrapper census](../../subsystems/battle-formulas.md) puts them.
- **0957** = the Death Game / Thunder Storm module; its head strings
  `Dies/Puera/Both/Damage/Recover` are Death Game's roulette outcome labels, not a summon-effect
  descriptor or debug table.
- **0965** = the Doomsday module. It is not a shifted sibling of 0967: the claimed shift `0x5FE8`
  lies wholly past 0965's real `0x2000`-byte extent, and the two entries share no content.

Owning page:
[`spell-table.md § capture-class module index`](../../formats/spell-table.md#capture-class-module-index-prot-09350966).

### New-game world-state seed store widths

*Status:* resolved - the port matches the disassembly. Evidence: `disassembly`.

Every `(offset, width, value)` entry in `legaia_asset::new_game::new_game_seed_words` matches the
stores `FUN_80034A6C` issues, decoded from `SCUS_942.54`. Ghidra's `DAT_` / `_DAT_` naming is a
symbol-size heuristic and is not width evidence
([`ghidra.md`](../../tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims)).

- The routine holds the save-context base in `$s0` (`lui $s0, 0x8008` / `addiu $s0, $s0, 0x4140`
  = `0x80084140`) and issues each seed write as an `sb` or `sw` at `$s0 + off`.
- The C's absolute globals `DAT_80085958` / `DAT_80085959` are `sb $v0, 0x1818($s0)` /
  `sb $v0, 0x1819($s0)` - the starting-item pair at `INVENTORY_SC_OFFSET`, `SC`-relative and
  issued after the template expander, so not part of the pre-expander set.
- The story-flag clear is a downward walk from `$s0 + 0x1FF` over `sb $zero, 0x1618($v1)`,
  covering `SC + 0x1618..0x1817` - `0x200` bytes, the port's `STORY_FLAGS_LEN`.
- The disc-gated `new_game_seed_disc::world_state_seed_matches_the_routines_stores` re-derives
  the table from the instruction encodings in the user's executable and fails on a wrong offset,
  value or width.

Owning page:
[`new-game-table.md`](../../formats/new-game-table.md#world-state-seed-code-literals-not-a-table).

### `_DAT_8007B8C2` polarity, and its writer

*Status:* resolved - **`!= 0` is retail, `== 0` is dev**. Evidence: `disassembly` + `capture`.

The halfword at `0x8007B8C2` is the build-mode selector. `main()` stores `1` into it once at cold
boot; the non-zero arm of every reader is the path a retail disc can service.

- **Writer:** `main()` (`FUN_80015E90`) at `0x80015F08`, `sh v0,0x5aa(gp)` with
  `gp = 0x8007B318`, storing the return of `FUN_8003F084` - a two-instruction leaf (`jr ra` /
  `addiu v0,zero,0x1`) whose sole caller is `0x80015F00`. A stubbed-out build-mode predicate; the
  dev build presumably returned `0` (inference).
- **Readers:** every read is an `lh` - 43 sites in `SCUS_942.54` (40 in the absolute
  `lui 0x8008` / `lh -0x473e` form, three gp-relative `lh v0,0x5aa(gp)` at `0x80015FD4` /
  `0x80016038` / `0x8001631C`), 57 across the dump corpus including overlays.
- **`!= 0` arm:** resolves assets by PROT-TOC index (`FUN_8003E8A8` + `FUN_8003E800`, or
  `FUN_8003EB98`).
- **`== 0` arm:** opens a path through `FUN_800608F0`, whose entire body is `break 0x103` - a
  PsyQ dev-station host trap - on `h:\` paths that do not exist on a retail disc. No site
  dissents; at `0x80016038` the `bnez v0` at `0x80016040` skips the `jal FUN_8003E6BC` dev-path
  call when the flag is non-zero.
- **`FUN_8003E6BC` does no CDNAME name resolution.** Its body is `strcpy` → `break 0x103` →
  fseek / fread / fclose; the `path_opener` label is Ghidra's, not evidence.
- **Capture:** the halfword reads `1` in 60 of 60 Mednafen save states - field, battle,
  world-map, stock and randomized discs.
- **Not zero-initialised:** the PS-X EXE header carries `b_addr = 0, b_size = 0`, so the BIOS
  clears no BSS for this executable.

The store and three of the reads are gp-relative, so a sweep for only the absolute `lui 0x8008` /
`-0x473e` form reports zero writers and 40 readers:
[`ghidra.md`](../../tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims).

### Key-item area consumers

*Status:* resolved on the narrow negative; the reader enumeration is closed. Evidence:
`disassembly`.

The range is inventory slots `>= 72` of `&DAT_80085958`. No consumer treats a key-item byte as an
unguarded index, and no instruction on the disc names an address inside the band.

**Indexing is bounded by construction.** Readers use the id byte as an index into 256-entry,
12-byte-stride item tables: an `lbu` yields `0..255`, so the maximum offset is 3060 against a
3072-byte table. The only signed `lb` reads (`0x8004250C`, `0x80042510`, in `FUN_800423E0`) are a
compaction move immediately re-stored via `sb`, with no index use.

**The enumeration is closed by three structural facts.**

- The array sits `0xA640` above `gp` and ends at `gp + 0xA840`, so no `imm(gp)` instruction can
  address any byte of it.
- Neither `0x80085958` nor the block base `0x80084140` occurs as a literal 32-bit word anywhere
  in SCUS or the 1233 PROT entries, so no pointer table offers an indirect route.
- Every access materialises the block base inline (`lui` + `addiu 0x4140`, `sll slot,1`, `addu`,
  `lbu|lb|sb ...,0x1818/0x1819`).

Decoding for that displacement pair gives **125 sites over six images**:

| Image | Sites | Notes |
|---|---|---|
| `SCUS_942.54` | 51 | 13 functions, `0x8003004C..0x800430A0` |
| PROT 0899 (menu) | 55 | 19 functions; four hide behind a `lui` in a branch delay slot |
| PROT 0897 (field) | 5 | |
| PROT 0898 (battle) | 2 | |
| PROT 0941 (Steal) | 7 | no active window; clamps its random slot against a live count (`0x801F7828`) |
| PROT 0954 (Fatal Decision) | 5 | no window, no clamp: `rand & 0xFF` over all 256 slots (`0x801F81A4..0x801F81B0`, retry cap `0x400`) - the only reader that reaches the key-item band outside the menu window |

Every other based overlay and PROT entry has zero. A displacement-only match gives 156 hits across
11 files, of which 31 are data-byte coincidences in five scene entries. The two absolute sites are
the new-game seed's slot 0 (`0x80034B10` / `0x80034B18`).

**Other facts**

- The `& 0x3ff` mask belongs to four packed-handle sites, not to readers generally, and it admits
  slot 1023 - `0x5FE` past the array - so it is not a 256-slot bound.
- The add / find / consume helpers bound their *scans* by the live window, but the id store at
  `0x800422BC` is not bounded: when the free-slot loop at `0x80042270` finds no empty slot the
  index exits equal to the window limit and `sb` writes one slot past the scanned window. The
  `slt` guard at `0x800422C0` is downstream and gates only the quantity byte. The overflow index
  derives from the window-limit global, not from an item byte, so it is a bounded one-slot write -
  the range amplifies to game-state corruption, not a native chain step
  ([reachability](#full-window-item-add-oob-reachability)).
- `lb $reg,0x5aXX($zero)` "hits" in overlay dumps are mis-decoded data tables: 117 occurrences
  across 74 files, and SCUS's 7 sit at `0x80010AE4..0x80010AFC` as a stride-`0x10` progression - a
  pointer table, not code.

Owning page: [`inventory.md`](../../subsystems/inventory.md).

### `title.pak` PROT entry

*Status:* resolved. Evidence: `capture`.

There is no single `title.pak` bundle entry: the dev-tree `title.pak` content is split across two
PROT entries, both confirmed by fingerprinting a title-phase RAM snapshot (the
`title_screen_new_game` save state) - the method that identifies `0895_bat_back_dat` as
`init.pak`, with the same CDNAME-mislabel pattern.

- **Title wordmark TIM:** PROT 0890 at file `0x14228` (parser `legaia_asset::title_pak`); the
  big-logo RAM TIM at `0x80170DF8` fingerprint-matches it. Offsets quoted against 0888
  (`0x1AA28`) or 0889 (`0x19A28`) are the same absolute bytes reached through over-read entry
  sizes.
- **Options / config-menu bundle:** PROT 0899 (`xxx_dat`). Its indexed payload opens with the
  config-menu string pool ("Display Off / Gradual / Immediate / Field HP Display / Encounters /
  Vibration / Dual Shock / Voices / Battle Camera / Monaural / Stereo ...") followed by the small
  config TIMs (the four RAM TIMs at `0x8010FEF0..0x80110130`, CLUTs byte-matched at 0899 offsets
  `0x169DC` / `0x1F91C`+).
- **Title-overlay code:** the same entry - the title tick `FUN_801DD35C` is PROT 0899 file
  `+0xEB44` ([below](#title-screen-mode-table-prot)).

### Title screen mode-table PROT

*Status:* resolved - no row is named for it, but it is a mode: the `CARD` pair, 22/23. Evidence:
`disassembly` + disc bytes.

The title screen runs under the `CARD` mode pair. The mode table's 28 x 24-byte records at
`0x8007078C` carry fourteen even/odd name pairs - `CONFIG / MAIN / MONSTER / TMD / EFECT / TEST /
MAPDSIP / MAP / READ / GAME OVER / BATTLE / CARD / OTHER / STR` - and none says "title".

**Chain from `main()` to the title tick**

| Step | Site | Effect |
|---|---|---|
| Seed | `FUN_8001D424` at `0x8001D5B8` | `_DAT_8007B83C = 0x10` (mode 16 `READ`) |
| Pre-loop overlay load | `0x8001612C jal 0x8003ebe4`, `a0 = 0` | extraction 0895, the boot `init.pak` |
| Mode 16 init `FUN_8002612C` | calls `0x801CE9C0` (0895 `+0x1A8`) | publisher-logo pass; sets mode 17 at `0x801CEC94` |
| 0895 | `0x801CF4D4` | writes mode 22 |
| Mode 22 init `FUN_8002574C` | `0x800258B4`, `a0 = 4` | loads PROT 0899, spawns descriptor `0x800706D4`, sets mode 23 |
| Descriptor handler `0x801E36A0` (0899 `+0x14E88`) | `jal 0x801dd35c` at 0899 `+0x14E94` | calls the title tick every frame |

Loader param 0 is therefore producible - by `main()` - so 0895 is statically reachable; 0896 is
not ([`boot.md`](../../subsystems/boot.md)).

**Ownership of `FUN_801DD35C`**

- Its 48-byte prologue occurs exactly once in all of `PROT.DAT`, at extraction 0899 file
  `+0xEB44` (`0x801CE818 + 0xEB44` reproduces the VA); it is absent from `SCUS_942.54`.
- Its own master-mode stores: `0x801DDCF0` (`0x1A`, attract → STR) and `0x801DFC00` (`2`,
  NEW GAME → field).
- `overlay_801dd35c.txt` is a different routine: the 436-byte `FUN_801DD310` from PROT 0897, a VA
  alias.
- The engine carries two ports of the routine (`menu.rs`, `title_overlay.rs`):
  [`vm-inventory.md`](../../subsystems/vm-inventory.md#one-function-two-ports).

### XP-table source + reader

*Status:* resolved + ported. Evidence: `capture`.

The retail XP curve is the static-SCUS per-level delta table `DAT_80076AF4` (u16), and the delta
is the closed form `delta(n) = ⌊n²/4⌋ + 1`.

- **Reader:** the level-up applier `FUN_801E9504` (overlay-resident, called from the reward
  resolver `FUN_8004E568` at `0x8004F34C`). The running sum to the current level is scaled
  `(sum × 9999999) / 0x140FE` for `level < 0x11` (else `sum × 0x79`) and compared
  `≤ record cumulative XP` in a multi-level `do...while` loop.
- **Not `0x8007123C` / `0x80070A3C`:** those are an off-by-`0x800` file/virtual confusion and a
  sin-LUT slice. A New Game Status capture shows "Next Level 121", the real L2 threshold, not 50.
- **Port:** `legaia_save::RETAIL_XP_CUMULATIVE` / `retail_xp_table()` ship the derived base curve
  (`121, 365, 730, ..., 9_646_483`); the boot-time disc parse
  (`legaia_asset::level_up_tables::xp_thresholds_from_scus` → `BootSession`) cross-validates it
  byte-identically.
- **Capture:** library-wide record sampling (`+0x0` XP / `+0x4` next threshold / `+0x130` level
  at `0x80084708 + slot×0x414`) matches through L37 including the Noa / Gala ± corrections (New
  Game 121 / 102 / 140; L99 carries 0). The Status menu (`FUN_801D33D8`) draws `+0x0` / `+0x4`
  verbatim.

Owning page: [`level-up.md`](../../subsystems/level-up.md#xp-table).

### Overlay identity from the disc (static extraction)

*Status:* resolved (pipeline `legaia_asset::static_overlay` + `asset overlay ...`). Evidence:
`capture`.

PSX overlays are clean copies of a fixed-VA-linked blob (FlushCache + jump, no per-load
relocation), so each runtime overlay is extracted statically from its `PROT.DAT` entry and
disassembled at its load base, with identity taken from the source entry rather than a guessed
label. That resolves VA aliasing (`0x801DD864` = battle-action in one overlay, muscle-dome in
another).

- **Proof case:** the battle overlay (PROT 0898 @ `0x801CE818`) is byte-identical to its resident
  RAM image over the full `.text` + `.rodata` (`0x28800` of `0x29800` bytes; only the trailing
  `.bss` diverges).
- **Base recovery:** from the overlay's own internal `jal` call graph
  (`static_overlay::recover_base`). For a call graph too sparse, the cross-check is a documented
  function landing on a prologue (`anchor_va`, slot A) or the fraction of internal absolute
  self-pointers resolving in-file (`static_overlay::pointer_resolution`, slot B).
- **Committed map** (`crates/asset/data/static-overlays.toml`): the slot-A scene family
  (field / battle / menu, the cutscene/STR overlay 0970, the minigame overlays 0972 / 0975 / 0976
  / 0980) and the slot-B entries (render 0900, the summon stagers including 0903 Gimard, 0905
  spell `0x83`, 0907 Nighto "Hell's Music", 0924, 0927; GAME OVER 0902 is slot A; 0957, labelled
  `summon_effect_table`, is the Death Game cast module).
- **Reconnaissance:** `asset overlay scan` (range sweep: base + leading dev string) and
  `asset overlay find-sig` (locate a function-head signature → infer the host overlay).
- It complements the dynamic captures: runtime values still need live probes.

Owning page: [`static-overlay-pipeline.md`](../../tooling/static-overlay-pipeline.md).

### PROT 0896 (`bat_back_dat`) identity

*Status:* resolved. Evidence: `capture` (identity and non-residency), `disassembly` (link base).

The unique ~`0x9000`-byte head is the vestigial Japanese-build field-menu / config / status
overlay - the debug-string sibling of the English retail menu overlay PROT 0899. It links at
`0x801D4DF0` against an executable this disc does not carry, and the USA build never loads it.

**Identity** (decode of the head off extraction entry `896`, located by its `"FWIN ERR"` bytes)

- A Shift-JIS label pool: config toggles, the Item / Summon / Equip / Status / Config / Save top
  menu, the ATK / UDF / LDF / SPD / INT / AGL + EXP status labels.
- The `"FWIN ERR %d"` window-manager debug printf at file `0x3D4` (`FWIN` = Field WINdow); no
  `fwin` / `bat_back` reference exists in `SCUS_942.54`.
- Real MIPS (~54 prologues): a status / name-draw routine indexing the `0x414`-byte character
  records, and head function-pointer tables holding ~61 addresses across
  `0x801D81C0..0x801DC700` (the window / screen renderers) - the VA family of 0899's
  `0x801D33D8` status renderer and `0x801DC6B4` save SM. Also a large byte-map-like data block
  (rows of gradually shifting byte values).
- 0899 carries the English versions of the identical label set and zero `FWIN`.
- From file `+0x9000` an over-read footprint carries the field overlay's bytes; they are not
  0896's.

**Link base `0x801D4DF0`**

- Call-graph recovery over the `0x9000`-byte entry: ten corroborating targets of eleven distinct
  internal `jal` targets, ten of them `addiu sp, sp, -X` prologues.
- All 218 internal `j` instructions land inside the file there; none does at `0x801C5818` or
  `0x801C0000`.
- Three runs of consecutive in-image VA words resolve every word, one holding the base itself.
- It reconciles the corpus's two phantom import programs: the function printed at `0x801C6534`
  and at `0x801C0D1C` is one function at file `+0xD1C`.
- `0x801C5818` (60 `jal` votes) is an over-read artifact: the field overlay's self-consistent code
  at `0x801CE818` fixes a whole-footprint recovery to `0x801CE818 − 0x9000` by construction. A
  `lui`-pair resolution ratio is one-sided and is not a base test
  ([falsified](../re-do-not-re-walk.md#measurement-readings)).

**Never loaded by the USA build**

- **Foreign link.** Of the image's 322 calls into the SCUS address range, zero land on a function
  entry of this disc's `SCUS_942.54` (the same test scores 0897 at 1203 of 1307 and 0899 at 793
  of 884), and no constant shift of the executable within +-`0x20000` brings more than 7 of its
  42 distinct SCUS targets onto an entry.
- **No loader reaches it.** A full-image scan of `SCUS_942.54` for `jal FUN_8003EBE4` /
  `FUN_8003EC70` with the `a0` setup decoded finds 16 sites; every constant param maps to
  extraction 897..902, 969..981, or the spell- / stage-driven bands (`id - 0x79` summon stagers,
  `+0x28` special-attack, `+0x47` battle stage). Extraction 0896 needs `param == 1`, which no
  site produces (the three computed-param sites have `+0x74` / `+0x47` / `5-or-6` bases). The
  raw indices `0x381` / `0x382` appear as immediates only in the two loaders' own
  `param + 0x381` adds, so there is no direct `FUN_8003E8A8` / file-open path either. The `+0x47`
  site reaches only 967 / 968
  ([`battle.md` § Stage-overlay dispatch](../../subsystems/battle.md#stage-overlay-dispatch-the-0x47-loader-band)).
- **Not resident.** A distinctive-signature scan across 140 catalogued RAM states (37 PCSX
  `.sstate` + 98 gzipped mednafen states, all phases) finds 0896 in none; the positive control,
  English "Battle Voices" (live 0899 config), is resident in 10 menu-phase states. The
  `scenarios.toml` `save_select_idle` note "overlay 0896 paged in" uses the extraction-index name
  for what is the English 0899.
- **Not the mode-24 OTHER overlay.** A live capture of the Baka Fighter entry
  ([`autorun_minigame_overlay_capture.lua`](../../../scripts/pcsx-redux/autorun_minigame_overlay_capture.lua),
  triggered on the `0x8007B83C = 0x18` write; sub-id `0x8007BA34 = 4`, confirming the `0x3E`
  operand−100 model) dumps the overlay window at +0 / +10 / +30 vsyncs, spanning the
  SCUS-resident OTHER INIT handler's completion (its `"other init end"` print) and the minigame
  overlay streaming into slot A. 0896's bytes appear at no offset in any dump, nowhere in main RAM
  in the pre-transition save, and in none of 45+ parked library states.

The CDNAME label `bat_back_dat` is not corroborated: no captured battle state holds the data, and
under the raw-TOC index shift the `#define` covering 0896's extraction slot may belong to a
neighbour. With the base committed, the image's code region is dumped and accounted - a base buys
coverage, not a caller
([`byte-accounting.md`](../../tooling/byte-accounting.md#a-base-is-not-a-dump-and-a-dump-is-not-a-caller)).
Owning page:
[`static-overlay-pipeline.md`](../../tooling/static-overlay-pipeline.md#a-resolution-ratio-is-not-a-base-test).


### SCUS recomp gap - render/GTE + boot/init clusters

*Status:* resolved, the general-game band included. Evidence: `disassembly`.

The static recompilation's function inventory lists SCUS entries with no dump, doc or port tag,
clustered by VA band. Every member is attributed, mostly as block splits and `+4` entry skews of
documented functions; `disc-coverage.py` puts every `SCUS_942.54` code byte inside a dumped body.

**"COP2 render gap" band (`0x43000..0x47000`) - not render code**

| Inventory entry | Is |
|---|---|
| `0x800430D4..0x80043134` | interior of `FUN_800430AC` (party-wide accessory unequip-by-id) |
| `0x80043238..0x8004325C` | interior of `FUN_800431FC` (knows-spell) |
| `0x80043290` / `0x800432A8` | interior of `FUN_80043264` (accessory-equipped) |
| `0x80043580` / `0x8004361C` | interior blocks of the cluster-A renderer `FUN_80043390` (far-colour / ZSF setup + its custom-convention epilogue) |
| `0x80046498` | `FUN_80046494` + 4, the locomotion collision resolver (not a render→overlay draw seam) |
| `0x8004697C` | `FUN_80046978` + 4, palette fade |

**The 14 `gte_execute` entries** are statically linked libgte per-op wrappers (`MulMatrix0`,
`Square12/0`, `AverageZ3/4`, `OuterProduct12/0`, `DCPL` / `DPCT` / `INTPL`, the
`RotTransPers3`-shaped RTPT projector) with zero static callers and zero runtime hot-profile hits:
link residue, since the render paths issue COP2 inline. All ignore-listed; table in
[`functions.md` § libgte primitives](../functions/runtime-libs.md#libgte-primitives).

**Boot/init cluster - aliases of documented functions**

| Inventory entry | Is |
|---|---|
| `0x80016448` | `FUN_80016444` |
| `0x80016B74` | `FUN_80016B6C` |
| `0x800173C0` | `FUN_800173BC` (dev profiler HUD, ignored) |
| `0x80016998` | interior of `FUN_8001698C` |
| `0x80017914` | `FUN_80017910` |
| `0x80017A04` family | interior of `FUN_800179C0` |
| `0x8001A078` | interior of the dev printf `FUN_8001A068` |
| `0x8001A814` | interior of `FUN_8001A78C` (RGB→HSV) |
| `0x8001AA14..0x8001AA60` | the six hue-sextant jump-table arms inside `FUN_8001A8DC` (HSV→RGB) |
| `0x80019BC0..0x80019D48` | interior of the atan2 bearing resolver `FUN_80019B28` |
| `0x8005B2A4` / `0x8005B340` | interior of PushMatrix `0x8005B268` / PopMatrix `0x8005B308` |

**New identifications**

- `FUN_80015E90` = `main()`
  ([`boot.md` § The main loop](../../subsystems/boot.md#the-main-loop-fun_80015e90)).
- The dev draw cluster: `FUN_8001CE34` (3-D line), `FUN_8001CAD8` (wireframe box - the sole
  source of `8001CE34`'s in-degree of 12, so it is not a heavily used boot utility),
  `FUN_8001CCFC` (2-D line), `FUN_8001C7A0` (4x8 digit printer).
- `FUN_800430AC`, whose Ghidra auto-analysis body is degenerate until force-created.
- `FUN_8004CE2C`, the largest SCUS function in the inventory: the per-frame battle actor
  maintenance pass
  ([`battle.md` § Per-frame actor maintenance](../../subsystems/battle.md#per-frame-actor-maintenance-fun_8004ce2c)),
  not a mode dispatcher.

**General-game band**

- `0x8002A9F8` is the branch target past the header test of `FUN_8002A9CC`, the `"ME"`
  channel-delta codec: `beq v0,v1,0x8002a9f8` at `0x8002A9E8` takes it when
  `(b0 & 0xC0) == 0x40`, and the fall-through is the `jr ra; clear v0` reject. A block split,
  hence no static caller ([`functions.md` § battle](../functions/battle.md)).
- `0x80025DA4` is `FUN_80025DA0` + 4, the Mode 12 `MAPDSIP INIT` handler, split after its leading
  `lw v0,0x798(gp)` ([`boot.md`](../../subsystems/boot.md)).
- `0x8004DC68` is the near-camera ghost pass (see the Battle area); `0x80036D80` is the ambient
  ramp pool's own actor template (see the Animation area).
- `0x80056208` is not a libgpu-band bridge: it is a battle side-band tick (three submodes off
  `DAT_8007B64A`) at a PsyQ-adjacent address, ported in `engine-render`.
- `0x8002149C` and `0x80059E10` both carry full disassembly.
- The PsyQ sound-driver cluster is tracked under Audio.

### Full-window item-add OOB reachability

*Status:* resolved - the write primitive is real; normal play cannot reach it. Evidence:
`disassembly` (full window) + `inference` (the half-window sub-case).

`FUN_800421D4` can store an item id one slot past the active inventory window, but the retail add
call sites cannot drive it there in normal play.

**The write**

- The id store `sb t0,0x1818(a0)` at `0x800422BC` is unconditional and precedes the `slt` / `beq`
  guard (`0x800422C8` / `0x800422CC`), which gates only the count store at `0x80042300`.
- When the free-slot scan (`0x80042254..0x8004229C`) exhausts the window it leaves the index
  `== end`, so the id lands at `base + end*2`: `0x80085A58` for `end = 128`, `0x80085B58` for
  `end = 256`.
- The window is installed only by `FUN_8004313C`, as `[0,256)`, `[0,128)` or `[128,256)` - never
  a 72-slot span.
- No add caller pre-checks room; each loads an item id and `jal`s the helper directly (shop
  buy-confirm `0x801C38A4` loading `a0 = rec+8`; battle loot `0x8004F380` / `0x8004F608`; the
  menu / save / fishing / world-map / minigame / equip-refund helpers). The helper's scan is the
  only backstop.

**Reachability**

| Window | Installed when | Verdict |
|---|---|---|
| `[0,256)` | any party of `>= 2` (live-verified at 3 members) | unreachable: the merge pass keys on the id byte (`andi a3,t0,0xff` @ `0x800421F4`), so each non-zero id occupies at most one slot and `0` is the empty sentinel; at most 255 distinct ids fill 256 slots, a hole always remains |
| `[0,128)` / `[128,256)` | a single playable member with story flag 20 clear | not capacity-bounded: the static item-name table `0x80074368` carries 250 non-empty names over 256 ids (blank: `0x00`, `0x12`, `0x1A`, `0x52`, `0xB9`, `0xFD`), so 128 distinct live ids is arithmetically possible. The bound is how much of that population a lone character can obtain - a progress bound, unmeasured (the `inference`) |

- Cast module PROT 0954 (Fatal Decision) reads the whole array with no window and no live-count
  clamp (`rand & 0xFF` over 256 slots).
- A non-add path (debug menu, cheat engine, a crafted save seeding duplicate live ids) can still
  force the exit with an attacker-influenced byte; that is outside normal play.

Port: `legaia_save::retail_inventory` (`ItemWindow::oob_reachability`, `MAX_DISTINCT_ITEM_IDS`,
`OobReachability`). Provenance: `ghidra/scripts/funcs/{800421d4,8004313c,8004e568,8003ce64}.txt`,
`overlay_0971_801c36b0.txt`. Owning page: [`inventory.md`](../../subsystems/inventory.md).

### Phantom-VA sweep of the PROT 0897 imports

*Status:* resolved - the three residues the delta arithmetic leaves open are byte-decided.
Evidence: `disassembly`.

The two measured deltas (`0xE818` base error, `0x25000` over-read) re-key most 0897-import prints.
The rest are decided by a word-level comparison
([`resolve-phantom-va.py`](../../../scripts/ghidra-analysis/resolve-phantom-va.py)): each dump is
compared against every candidate (image, base) reading at the printed VA, with Ghidra's
data-as-instruction renderings (`nop`, `<load> rt,imm(zero)`) re-encoded into exact 32-bit words
so data regions decide at full strength. Every verdict is word- or token-exact against the
corrected-extent images, and each rival reading is excluded by the same comparison.

- **Boundary band `0x801E4000..0x801E6000`:** every dump resolves to exactly one reading, and the
  strata switch exactly at `0x801E5000`. The two open addresses are 0897 own-content data -
  pointer tables at true VAs `0x801F3308` / `0x801F3450` (14/14 and 13/13 words; the rival 0898
  reading scores 0). `0x801E5134` is printed by two programs with two owners: one print correct,
  one a phantom of 0898 `0x801CE94C`.
- **`0x8020D05C`:** 0898 rodata at true VA `0x801F6874`, a `(pointer, count)` table into 0898's
  `0x801CF9xx` band. Its words include values with no R3000 decoding, matching the dump's
  zero-instruction `halt_baddata`; every rival reading maps the VA to code that would decode. Not
  a function under any reading.
- **PROT 0896:** the `overlay_0896_*` prefix covers two imports of the over-read footprint -
  untagged at `0x801C0000` (three strata: own content `< 0x9000`; field `+ 0x5818`; battle
  `- 0x1F7E8`) and tagged `base=0x801C5818` (prints are 0896's own bytes at
  `printed - 0x801C5818`). Every addressed dump resolves under exactly one program, and the
  header-tag partition agrees with the byte partition dump for dump. The same function printed by
  both programs pins the pair: file `+0x5C90` at `0x801C5C90` / `0x801CB4A8`, file `+0xD1C` at
  `0x801C0D1C` / `0x801C6534`.
- **`0x801FD4C0`:** its dump starts at printed `0x801FD150` and is the battle image's
  `FUN_801E6968`; the printed VA is that body's interior at 0898 VA `0x801E6CD8`, not the field
  image's `FUN_801E6B34`.

Owning page:
[`overlay-va-aliases.md § the byte-level sweep`](../overlay-va-aliases.md#the-byte-level-sweep).

### The publisher-logo quads

*Status:* resolved - `FUN_801CE9C0` uploads and does not draw. Evidence: `disassembly`; the
descriptor table reproduces from the extracted image.

`FUN_801CE9C0` (PROT 0895 `+0x1A8`) uploads the four `init.pak` TIMs through `FUN_800198E0` after
writing their CLUT and pixel VRAM rects, selects the 640x480 display env (`FUN_8001DAF8(0x400)`)
and spawns the two boot actors. It contains no primitive emit.

- **Quads:** a six-record, 20-byte sprite-descriptor table at `0x801F369C` (file `+0x24E84`,
  immediately after the fourth TIM):
  `[u32 scale][u16 tpage][u16 clut][u8 u,v,w,h][rgb top][stp][rgb bottom][tpage adder]`.
- **Emit:** opaque `POLY_GT4`s from `FUN_801CFBB8`, sequenced by `FUN_801CEFD4` (13-arm jump
  table at `0x801CE8E8`).
- **Binding:** each record's `tpage` / `clut` matches a rect the uploader wrote - `0x9A/0x7ED4`,
  `0x9C/0x7F54`, `0x0A/0x7F14`, `0x0B/0x7E80`.
- **Order:** SCEA, Contrail, PROKION.
- **Fade:** the PSX texture blend `texel * colour / 128` over a vertex colour scaled by a
  `0..0x80` level, not alpha.

Owning page: [`boot.md`](../../subsystems/boot.md#the-per-logo-quads).

### The title menu's law

*Status:* resolved + ported on both hosts. Evidence: `disassembly`.

`FUN_801DD35C` sub-mode `0x10` (`0x801DDB74..0x801DDCF4`) is the two-row NEW GAME / CONTINUE menu.

- **Rows:** two (`andi 0x1` at `0x801DDC00`). Down `0x4000` +1 / Up `0x1000` -1, cue `0x21`.
- **Confirm:** mask `0x844`, cue `0x20`. Row 0 -> mode `0x16`; row 1 -> mode `0x18`, stashing
  `state[+0x200] = 1`.
- **Attract countdown:** `0x5DC`, re-armed whenever the held word `_DAT_8007B850` is non-zero and
  decremented by scratchpad `0x1F800393`. The input block is skipped while it reads below `0x11`;
  underflow writes `_DAT_8007BA78 = 0` and mode `0x1A`.
- **Entry:** a cold boot reaches `0x10` through `0x11`
  ([above](#a-cold-boot-always-shows-title-sub-mode-0x10)).

Port: `legaia_engine_vm::title_overlay`'s executable half, driven by `engine-core::title` on both
hosts.

### `FUN_801E5A08`, the per-slot equip applier

*Status:* resolved - one routine, unreferenced. Evidence: `disassembly` + bytes.

"`FUN_801D71F0`" and its "shared armament placer `0x801E5AE8`" are one 324-byte routine,
`FUN_801E5A08`, and nothing on the disc calls it.

- `ghidra/scripts/funcs/overlay_0897_801d71f0.txt` (and the `_801d7210` sibling) is mis-based by
  `0xE818`: the bytes are PROT 0897 file `+0x171F0`, true VA `0x801E5A08`.
- Its four class arms end on `j 0x801E5AE8` / `j 0x801E5AEC` - intra-function jumps whose targets
  print correctly while the body prints low. `0x801E5AE8` is the routine's own inline placer at
  `+0xE0`.
- Class -> equipment byte: 0 -> 0, 1 -> 1, 2 -> the weapon index (`_DAT_8007B42C`, `2/3/2`),
  3 -> 4 (an `addiu v1,zero,4` in the delay slot at `0x801E5AB0`).
- No `jal`, data word or `addiu` materialisation of `0x801E5A08` exists in any image. The live
  equip confirm is `FUN_801D9C14`'s candidate arm.

Port: `legaia_engine_vm::dev_equip_commit::commit_equip`.

### The dead Return view mode

*Status:* resolved - dead; the grid has 15 cells. Evidence: `disassembly` + bytes.

`FUN_801E3F74`'s mode `4` ("Return", `0x801CF384`) is real and unreachable.

- Its only caller forms the cell as `col + row*5` (`0x801E06D0`).
- The shared stepper clamps `col` to `0..=4` and `row` to `0..=2`.
- The linear seed `_DAT_8007B7CC` has three references on the disc, all in PROT 0899; its single
  writer (`0x801DED2C`) stores the same `col + row*5`.
