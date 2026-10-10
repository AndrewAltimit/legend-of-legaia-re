# Settled threads: Field / locomotion

One area of the [settled reverse-engineering threads](../re-settled-threads.md) register.
The evidence grades (`disassembly` / `capture` / `decompiled-C` / `inference`) are defined on
[the index page](../re-settled-threads.md#the-evidence-column).

This area covers what happens while the player walks a town, dungeon or cutscene scene: movement,
collision, warps and doors; the scene scripts, walk-on triggers and story flags that drive a scene; the
field camera, fog and ambient animation; the pause menu, shops and inns; and the minigames the field
hosts. Each row is a question about retail behaviour with its answer, the evidence that answer rests
on, and the port code and reference page that carry the detail. Check here before re-opening a field
question, or when a behaviour of the port needs its retail justification.

## Detailed write-ups

Threads whose answer needs more than a table cell. Every other thread is a row of the table under [Threads](#threads).

- [`opdeene` runs long after its `apply 4800` camera move](#opdeene-runs-long-after-its-apply-4800-camera-move)
- [The field follow camera's pose chain](#the-field-follow-cameras-pose-chain)
- [Chapter-1 scene frontier](#chapter-1-scene-frontier)
- [The Uru Mais chain and jouine exits](#the-uru-mais-chain-and-jouine-exits)
- [The upper-case destination fold](#the-upper-case-destination-fold)
- [The count-5 asset tables](#the-count-5-asset-tables)
- [The ledge-hop lock leak](#the-ledge-hop-lock-leak)
- [Clip-end latch for cross-context clip pokes](#clip-end-latch-for-cross-context-clip-pokes)
- [Ambient render-mode 4 - the VRAM-rect scroller](#ambient-render-mode-4---the-vram-rect-scroller)
- [Which op-`0x34` sub-3 installs fire at scene entry](#which-op-0x34-sub-3-installs-fire-at-scene-entry)
- [Master ambient record 0 - the per-scene SFX descriptor bank](#master-ambient-record-0---the-per-scene-sfx-descriptor-bank)
- [Rim Elm's south gate](#rim-elms-south-gate)
- [Town/field free-movement locomotion](#townfield-free-movement-locomotion)
- [Field collision-map source](#field-collision-map-source)
- [Field `.MAP` PROT resolution - `define − 2`, universal](#field-map-prot-resolution---define--2-universal)
- [game_mode 0x03 = field/town gameplay](#game_mode-0x03--fieldtown-gameplay)
- [Engine VRAM byte-exactness for town01](#engine-vram-byte-exactness-for-town01)
- [World-map CLUT cycling beyond the ocean head - CLOSED (operand table + emitter + cadence all pinned)](#world-map-clut-cycling-beyond-the-ocean-head---closed-operand-table--emitter--cadence-all-pinned)
- [`init_data` UI-tile pages - journey-dependent residency (resolved); map03 texture column (resolved - "not uploaded" premise falsified)](#init_data-ui-tile-pages---journey-dependent-residency-resolved-map03-texture-column-resolved---not-uploaded-premise-falsified)
- [CLUT row 510 population (boot-resident system-UI strip band)](#clut-row-510-population-boot-resident-system-ui-strip-band)
- [Scene-transition (`0x3F` door) destination indexing](#scene-transition-0x3f-door-destination-indexing)
- [Intra-town (house / interior) door mechanism](#intra-town-house--interior-door-mechanism)
- [Field/town environment-geometry placement](#fieldtown-environment-geometry-placement)
- [Region story-flag gate families](#region-story-flag-gate-families)
- [Extraction-0874 §2 (`player.lzs`) F-variant pixels - a one-shot opening face-frame stamp, not a menu writer](#extraction-0874-2-playerlzs-f-variant-pixels---a-one-shot-opening-face-frame-stamp-not-a-menu-writer)
- [What the op-`0x49` entry-context kind byte is, and which screens it selects](#what-the-op-0x49-entry-context-kind-byte-is-and-which-screens-it-selects)

## Threads

| Thread | Status | Evidence | Answer |
|---|---|---|---|
| Does a later keikoku variant MAN replace `P2[7]`'s pushback? | resolved (no - the inner doorway is a story wall on every visit) | `disassembly` | The Ravine ships one MAN carrier, its bundle MAN (no streaming variant). `P2[7]` has no header gate and dispatches on its own flags: `0x142` (Mt. Rikuroa post-Caruban) splits it into before / after halves, each counting visits on a one-shot trio (`0x2BB..=0x2BD`, `0x2BE..=0x2C0`, set nowhere else on the disc). Every arm's dialogue names another goal and ends in the player's compass walk back, and the `map01` portals close on `0x193`, so no enterable flag state passes the band. Test `keikoku_inner_doorway_record_has_no_variant_and_no_foreign_writer`. |
| Does retail step helper contexts while a timeline sits on an open text box? | resolved (yes; only a context on its own text waits) | `disassembly` | `FUN_80039B7C` gates on the context's own `+0x9C`, never on the box: `0` runs the slice, `1` (reached text) claims the box only while the state word `0x801F2734` reads `1` / `4` / `7` (`0x80039F9C..0x80039FD8`), `2` waits for `0` / `3` / `6` / `9` and resumes. The one whole-list stop is scratch `0x1F800394 & 0x400` in `FUN_8003BC08` (`0x8003BCF8`). The port runs off-text helpers every frame; a second texter waits without typing and the first claimant owns the box (`a_helper_off_text_keeps_its_slice_while_the_box_is_taken`). korb3's apparent 0-frame helper is the counter: a `WaitFrames` park is kept off `frames`. |
| Do retail's ending vignettes accept the pad while their entry record runs? | resolved (no) | `capture` + `disassembly` | All 5 end-credits field-run states hold the player's engaged bit (`+0x10 & 0x80000`) with a running count of 24 to 476; the scene-init gap state has both clear. `FUN_801D1344` skips locomotion on the bit (`0x801D1694..0x801D16A0`); `FUN_80039B7C` raises it on every stepped slice and clears it when the count drains. edlast's `P2[1]` then waits on a held Circle / Cross poll at `+0x2608`, which the port keeps as a wait ([`cutscene.md`](../../subsystems/cutscene.md)). |
| Which scene installs the code lock `FUN_801EED58`? | resolved (`doman` only) | `disassembly` + `capture` | Field-VM `49 02` maps to handler slot `0x21`; the census finds one shipped site, `doman` (extraction 0401, record 18, pc `0x0768`), whose next op tests flag 9 - the flag the lock's verdict writes. Phase 0 installs window 12. Port `field_submode_code_lock`, drawn on both hosts; disc test `code_lock_doman_disc`. |
| What is `FUN_801E6F70`? | resolved (the coin counter's entry panel, field overlay) | `disassembly` | The painter word of panel-window record 10 (PROT 0897, geometry `(0x40, 0x26, 196, 78)`): coin bank, six entry cells, gold and a total drawn in pen 9 when gold or the ceiling `_DAT_8007BB90` falls short. The confirm descriptor `0x801F3360` is `[1,11]`, so the entry panel stays drawn under the Yes/No. Port `slot_machine::coin_entry_panel`, both hosts through one builder. |
| How is the fishing venue's menu reached? | resolved (from the idle shore, on Triangle / Select) | `disassembly` | State `0x0C` (`0x801CF990`) tests the packed edge `_DAT_8007B874` twice: `& 0xC0` casts, `& 0x110` (`0x801CF9EC`) raises SFX `0x21`, zeroes the hub cursor `0x801D912C` and jumps to `0x64` (`FUN_801D0474(1)`). `0x65` / `0x66` are the help pages (`FUN_801D72A0(0x14, 0x10, page)`, turned on `& 0xF0`), `0x6E` the tackle list (`FUN_801D0F5C(1)`), `0x78..0x7A` the prize chain; both lists cancel to `0x64`. Retail also enters the pond directly. Port `engine-core::fishing_hub`, on all three hosts. |
| Do concurrent script contexts lock the pad, or only the cutscene timeline? | resolved (every stepped record locks it) | `disassembly` | `FUN_8003BDE0` sets `+0x100` on a spawned record (`0x8003C088..0x8003C0AC`), and the per-actor tick `FUN_8003BC08` steps such a record through `FUN_80039B7C` (`jal` at `0x8003BD34`; the only other caller is PROT 0897 `0x801DA7BC`). `FUN_80039B7C` raises the player's `0x80000` on every stepped frame (`0x80039DB8..0x80039DD4`) and clears it only when the running count drains at a raw `0x21` (`0x80039EE8..0x80039F14`), and `FUN_801D1344` skips the pad controller while it is up (`0x801D1694`). Being modal decides only the camera. Engine: `World::script_context_engages_player`. |
| Why does the menu button open nothing after a Door of Light arrival on `map01`? | resolved (a walk-on script holds the player; bounded) | `capture` + `disassembly` | The Door seats the player on map01's cave-mouth walk-on trigger `(37,109)`, which spawns `P2[9]`. For its 294 vsyncs `FUN_80039B7C` keeps the player's engaged bit `+0x10 & 0x80000` up (raised `0x80039DD4`, cleared `0x80039F14`) so `FUN_801D1344` never calls the pad controller `FUN_801D01B0` (`0x801D1694`). The first press after it opens the menu. The span is `WaitFrames 40`, the `B7 F8 00 81` compass walk (16 vsyncs) and an `AD F8 08` spin on scene-bank clip 13 (120 frames, latched by `FUN_800204F8`); the engine runs 296. [`field-locomotion.md`](../../subsystems/field-locomotion.md). |
| Who sets a field NPC's moving-class bit `+0x10 & 0x20000`? | resolved (the placement seater, and op `31 11`; nothing clears it) | `disassembly` + `capture` | `FUN_8003A1E4` ORs `0x20000` into every partition-1 placement's `+0x10` (`0x8003A3A4..0x8003A3B4`; 52 of 52 in a `town01` entry write-watch), and op `0x31` operand `0x11` (`0x801DED9C..0x801DEDB0`, PROT 0897) sets it at 26 partition-0 spawn-prologue sites (`town01` `P0[8]` captured). No op `32 11` exists on the disc, and the capture saw no clear in 17161 writes ([motion-vm.md](../../subsystems/motion-vm.md#the-motion-pause-kick)). |
| What does `FUN_801D79E8` do? | resolved (the per-actor visibility cull) | `disassembly` + `capture` | Unless `_DAT_8007BAF4` is set it raises `+0x10` bit 1 for an actor whose tile is outside the region box `0x1F800384..87` or the view window `0x1F8003E8..EB` (widened by `+0x58`) and clears it inside; `FUN_8003BC08`'s height arm then holds the Y, while the clip still advances. 600 of 600 captured decisions reproduce through `world::field_npc_cull` ([motion-vm.md](../../subsystems/motion-vm.md#the-driver-fun_8003bc08)). |
| What arms the place-name banner? | resolved (system flag `2`, a one-shot) | `disassembly` + `capture` | The kingdom MANs' entrance records (and two in `koroutx2`) set flag `2` ahead of their `0x3F`; `FUN_8003AEB0` spawns the `4C E1` balloon from `0x8003BB40` when it is set and clears it on every path (`andi 0xdf` at `0x8003BBD4`). The state 60 vsyncs before `doman` loads holds it set ([place-names.md](../../formats/place-names.md#site-3---the-scene-display-name)). |
| When does the system channel's clip-base store reach the settle? | resolved (on every tick the player is not movement-locked) | `capture` + `disassembly` | `FUN_801DA51C` runs the channel only with the player's `+0x10 & 0x80000` clear or its own `+0x100` up (`0x801DA78C..0x801DA7AC`), and `FUN_80039B7C`'s store at `0x80039D94` sits in the interaction-start arm. A talk opened running keeps the base its opening tick left (`2`), and a ledge hop sees no store ([field-locomotion.md](../../subsystems/field-locomotion.md#when-the-system-channels-store-reaches-the-settle)). |
| Where does a Door of Light / Door of Wind use go, tick by tick? | resolved (the pause-menu session hands off to a travel art) | `capture` + `disassembly` | The session `FUN_801ED308` runs phase 2 → 4 (level `0xF2`), twelve ramp ticks, phase 6 / 7, then installs `0x29` Riremito / `0x2B` Rula. Riremito gates 47 ticks on its effect, dwells to `0x50`, spawns the fade, dwells to `0x28` and resolves; a Door of Light goes to the region record's triple `0x80084624..2C`, refreshed when the menu button is pressed. Probe `autorun_door_item_use.lua`; engine test `door_item_retail_timeline` ([field-locomotion.md](../../subsystems/field-locomotion.md#a-door-use-captured)). |
| What does `_DAT_80084540` hold? | resolved (the loaded scene's raw CDNAME define) | `capture` + `disassembly` | Two above the extraction-frame `Scene::start` in all 98 states checked (`town01` `3`, `town0b` `0x0C`, `town0c` `0x15`, `map01` `0x55`); the formation roll's map arms and the intro style picker key on it. Engine `BattleState::map_id`. |
| Why can't the pause menu open over a shop? | resolved (the shop's overlay displaces the controller) | `disassembly` + `capture` | The menu accept lives in the field overlay's pad controller `FUN_801D01B0` (PROT 0897), and the gold shop's buy commit `FUN_801DB7F4` is PROT 0899's own code (its bytes `lui v0,0x801e; lw v1,0x46ac(v0); addiu sp,sp,-0x20` sit at 0899 file `0xCFDC` = `0x801DB7F4 - 0x801CE818`). Both images load at slot A `0x801CE818` ([`static-overlays.toml`](../../../crates/asset/data/static-overlays.toml)), so while a shop runs no field code is resident to read Start. The `casino_prize_shop` state holds game mode `0x17` with 0899 in slot A. Engine: `World::field_menu_open_allowed` refuses while a shop, the prize counter, narration or a title card is open. |
| Which clip does the player hold through a kind-0 warp? | resolved (idle) | `capture` + `disassembly` | The system channel `0x8007E694`, ticked by `FUN_801DA51C` after the player, runs `FUN_80039B7C`, whose `sw v0,-0x4228(v1)` at `0x80039D94` stores clip base `2` on every field tick the player is not movement-locked. The settle reads the base before it (`0x801D1D8C`, `0x801D1E08`), so while the pad step runs the reset is invisible; with the step skipped the settle reads `2`, and a write watch sees clip id `+0x5C` read `2` for the whole warp with Down held ([`field-locomotion.md`](../../subsystems/field-locomotion.md#retail-capture-of-the-base-writers)). |
| How long does the warp timer's `-1000` sentinel live? | resolved (the rest of the landing tick) | `capture` + `disassembly` | `FUN_801D1EC4` parks `_DAT_8007B6B0` at `-1000` on the landing, and the same tick `FUN_801DA51C`'s tail compares it with `-1000` and stores `0` (`0x801DA7D8`), so no next-tick reader sees the sentinel in a scene whose system channel runs it ([`field-locomotion.md`](../../subsystems/field-locomotion.md#retail-capture-of-the-warp)). |
| Where does an inn conversation end? | resolved (at the last page's close) | `capture` + `disassembly` | The sub-`5` acquire is the arm `0x801E2148..0x801E21DC` (table `0x801CEF48` entries `5`, `0xE`, `0xF`), refusing on `s7 = 0` (`beqz s7` at `0x801E21D0`). In `retock_innkeeper_talk_open` the stay ends when its last page closes, with the cursor parked on the `26 9D FE` loop-back; the next talk runs the loop-back and the acquire succeeds (`s7 = 5`) ([`script-vm.md`](../../subsystems/script-vm.md)). |
| What does the region battle-setup half of `FUN_801D9E1C` store? | resolved (a backdrop variant, two Door gates, an object-keep bit, a world-map return point) | `disassembly` | On every region hit (`0x801DA058..0x801DA12C`): `_DAT_8007BD60 = region[+8] & 0x1F`, the stage variant `FUN_800513F0` loads; `0x1F800394 \|= 0x300000`, cleared per Door of Light / Wind by bits 7 / 6; `_DAT_8007B64B` = bit 5, keep backdrop object 1; and a return triple at `0x80084624..0x8008462C` from `region[+5]`, `[+9..+11]` ([`script-vm.md`](../../subsystems/script-vm.md#the-region-battle-setup)). |
| Why does `kor5`'s `0x619` read set on entry and clear after the chain? | resolved (a spawn-section write) | `disassembly` + `capture` | `P1[2]`'s `SET 0x619` sits between its leading `0x25` and first `0x21`, so `FUN_8003A1E4` runs it at every MAN-loading entry; `FUN_801D6704` passes `loader mask & 4` to `FUN_8003AEB0`, which skips the spawn loop (`0x8003B8A0`) on a same-scene reload, so the post-battle reloads never re-set it after `P2[4]` clears it ([`script-vm.md`](../../subsystems/script-vm.md)). |
| Where does a talk begin in an NPC record? | resolved (the interaction cursor `+0x9E`, never `script_pc0`) | `disassembly` | The interaction dispatch runs `FUN_80039B7C` from the cursor, so the spawn section runs only at scene load. Entering at `script_pc0` instead re-runs the spawn section on every talk and skips the record's segment-selection prologue ([`script-vm.md`](../../subsystems/script-vm.md)). |
| Who writes the field clip base `_DAT_8007BDD8`, and which clip is run? | resolved (the pad step; run is bank slot 2) | `disassembly` | `FUN_801D01B0` stores it every frame (`0x801D0424..0x801D04A4`): idle `2`, walk `1`, any faster step `3`, `99` under `_DAT_8007B6A8`. The hop machine writes `6` / `7` / `1`; `FUN_801D1EC4` writes `2` on one arm. Run is reached by Cross or R1 (run mask `0x48`) ([`field-locomotion.md`](../../subsystems/field-locomotion.md#the-clip-base-and-the-settle-tail)). |
| What does a kind-0 walk-on tile do? | resolved (a timed warp, not an instant one) | `disassembly` + `capture` | `FUN_801D1EC4` arms `_DAT_8007B6B0 = 0x26` and two fades, counts it down without moving anything, and on the landing frame stores the pad hold `_DAT_8007B6B4 = 0x28`, re-rolls a spent encounter counter, seats the player and runs the landing tile's kind-1 record. The player tick `FUN_801D1344` drains the hold and skips the pad step while either word is live. From `s3_rimelm_freeroam` the landing comes `38` vsyncs after the crossing and the pad `40` after that ([`field-locomotion.md`](../../subsystems/field-locomotion.md#retail-capture-of-the-warp)). |
| What does op `0x43`'s acquire do when the target is mid-arc? | resolved (waits and retries) | `disassembly` | The acquire (`0x801DF384..0x801DF40C`) fails on halt bit `0x400` while the scene word `*(_DAT_801C6EA4) + 8` is zero, and the failure leaves the PC on the op (`beqz` to `0x801DEE4C`, `move s8,s4`). That word is non-zero only while a spawn section is being pre-run (SCUS `0x8003B73C` / `0x8003B928`, `0x801E2820` / `0x801E2BBC`) ([`script-vm.md`](../../subsystems/script-vm.md)). |
| Which `FUN_801CF8AC` arm do the ambient walkers take? | resolved (the class arm, all of them) | `disassembly` | `FUN_8003A1E4` ORs `0x20000` into every placement it seats (`0x8003A3A8..0x8003A3B4`), and `0x01000000` for a `>= 0xF0` party model, so the box test runs on `+0x10 & 0x01020000`; the no-class arm is for pool actors spawned elsewhere ([`motion-vm.md`](../../subsystems/motion-vm.md)). |
| What does op `0x3E` do with `op0 < 100`? | resolved (the scripted-battle install, one body with `op0 = 0xFF`) | `disassembly` | `FUN_801DE840`'s arm reads `op0` twice only (`beq 0xFF` at `0x801E06FC`, `sltiu 0x64` in its delay slot). Both sides force the player's cached region tile stale (`+0x8E` / `+0x8F = 0xFF`), call `FUN_801D9E1C(player, 0)`, skip on `_DAT_8007B868` or a missing `0xFB` context, set `sys[+0x8A] = 1`, point `sys[+0x94]` at MAN formation row `op1`, store the `FUN_801DDF48` reroll into `_DAT_8007B5FC` and request mode `0xE`. Ten clean non-`0xFF` sites (`town0b`, `stone`, `jagaroom`). Port `FieldHost::scripted_battle`, with the region half in `World::apply_region_battle_setup_at_player` ([`script-vm.md`](../../subsystems/script-vm.md#0x3e-scripted-battle-op0--100)). |
| How many sites re-roll the encounter step counter? | resolved (five, into one word) | `disassembly` | `find-address-word-refs.py --prot` finds four `jal 0x801DDF48` and no other reference: SCUS `0x8003AC90` (`FUN_8003AB2C` adds half a roll while the counter is below 487), `0x801D1F6C` in `FUN_801D1EC4` (a full roll when the counter is `<= 0`), the op-`0x3E` arm `0x801E076C` and `4C EC` at `0x801E34F8`; `FUN_801D9E1C` carries an inlined copy (`0x801DA2D4..0x801DA35C`). All five store `_DAT_8007B5FC`, one counter across doors. `FUN_801DDF48` is two BIOS draws, `r1 % 487 - r2 % 487 + 0x3CE`. Port `region_encounter::encounter_counter_reroll`, seated at all five sites (the warp landing through `World::tick_field_warp`). |
| Does P2[5] write `kor5`'s `0x436` organically? | resolved (yes) | `capture` (synthetic: two trigger-tile pokes, Gaza's HP held at `1`) | From `kor5_post_43a_checkpoint`, P1[0] spawns P2[5] on the battle's reload and it sets `0x436` at `+0xD0D` through `FUN_8003CE08` (`ra 0x801E3598`) 3,336 vsyncs after `0x464` clears; P2[8] then sets `0x6C4` on its first `(32, 86)` crossing (`kor5_post_436_organic`). Caveat: the capture ran without a staged disc copy, so it carries the [patched-disc caveat](../../tooling/pcsx-redux-automation.md). |
| Where does the op-`0x43` arc's chained record point, and what does op `0x34` sub-1 spawn? | resolved (a release watcher; an attached billboard) | `disassembly` | `FUN_801D25EC` allocates the arc actor from template `0x801F227C` (`0x801D2634`) and a second actor from `0x801F22AC` (`0x801D2760`), whose handler word is `FUN_801D5D60` - a watcher that clears the arm's halt bit `0x400` when the arc lands, not an emitter. Op `0x34` sub-1 calls `FUN_801E5668` at `0x801DFFE0`, which allocates from template `0x801F28B8` (handler `FUN_801E4470`) and copies the parent link, position and rect. |
| What is `FUN_801E58A8`? | resolved (an actor anim-clip pick) | `disassembly` | It writes `+0x5E = -2` and a clip index into `+0x5C` from `_DAT_8007BDD8`, the party leader `_DAT_8007B8F8` (stride 7) and the override `_DAT_8007B6AC`, then calls the clip selector `FUN_800204F8` (`0x801E58A8..0x801E59AC`). The same arithmetic is the tail of `FUN_801D1BA0` (`0x801D1D88..0x801D1EAC`). |
| When does `4C 86` install its reflection controller? | resolved (at scene entry, from the record's spawn prologue) | `disassembly` + `capture` | All ten shipped sites sit after the record's leading `0x25` and before its first `0x21` park, so no talk press is involved. A retail `conc` -> `conc2` crossing seats all three controllers on one frame, each call returning to `0x801E227C`. Over 132 in-rect tick pairs the image is `(x, y, 2*zz - z)` with facing `-0x800 - a`; 194 out-of-rect pairs leave it alone; the rect test quantises with `(v + 0x40) >> 7`. |
| Is `juui1` dark in retail outside its tint beats? | resolved (no) | `capture` (synthetic gate) | With `0x3E1` cleared and `0x3E5` set by RAM poke, the black run from vsync 1040 to about 1327 is the door fade - the departure push from `0x801DDE24`, then the arrival ramp from `0x80025034` - and after the last push the scene holds a dim purple vortex with the party visible, mean luma about 40 of 255. An organic save exists (the `rugi` block of the endgame card) but its route crosses the Rogue fight. |
| Which flags do the retock / doman / nilboa entries write? | resolved (per-scene entry families) | `capture` | Each from a pre-entry card-boot state, every write through the field VM's SET (`0x801E3598`) / CLEAR (`0x801E35C0`) arms: retock SETs `0x493`, `0x01F`, `0x52A` and clears `0x19B..0x1AA` and `0x527..0x52E`; doman SETs `0x49C`, `0x01F`, `0x6E7`; nilboa SETs `0x014`, `0x499`, `0x01F`, `0x52A`. retock's `0x502` is not an entry write - its one non-debug writer is spawned from Eliza's talk loop, which `0x33B` (set in `jagaroom`) unhides (`inference` from the card saves). |
| Why did a walk-on tile poke under the movement lock never fire? | resolved (a crossing made while locked is consumed) | `disassembly` + `capture` | `FUN_801D1EC4` stores the new tile into its last-tile mirror `0x8007BDC8` / `0x8007BDCC` on both failure branches - the cell test `cell & 0x600` at `0x801D2144` and the `+0x10 & 0x80000` lock at `0x801D214C..0x801D2158` - so a crossing made during the lock never fires later. The port's `dispatch_walk_on_trigger` consumes it the same way: it stores the tile and returns while a script holds the player. |
| Which records write the `kor5` tail, `doman` and `son` flags? | resolved (with `kor5`'s `0x436` input poked) | `capture` | `kor5`: P2[3] `0x43A` -> P2[4] `0x464` and battle 165 -> reload, P1[0] clears `0x464` and spawns P2[5] -> P2[8] writes `0x6C4` at `+0x75` (tile `(32,86)`, `C1 {0x6C4}`, `C2 {0x436}`). `doman`: P2[4] on tiles `(67,108..110)` writes `0x3FB` at `+0x98D`, organically from `korb2` across the world map by tile poke. `son`: the arrival seat fires P2[5] (`0x60D`); P2[3] `(18,85)` sets `0x3A6`, P2[4] `(24,76)` sets `0x3A7`; the entry clears `0x3A6..0x3A8` on every visit, so they are per-visit latches. |
| What are motion-VM ops `0x37` / `0x41`? | resolved (an eight-direction compass walk) | `disassembly` | Both step along the compass table at `0x80073F14`, not along one axis toward a target; `0x43` never completes, and target `0xF8` is the player, not the executing actor. |
| What do field-VM `4C E4` and `4C DB` do? | resolved (a box test with a relative skip; the CLUT blend fade) | `disassembly` | `4C E4` builds its box from tile corners `+0x20` / `+0x60` / `+0xA0` and takes a relative skip when the actor is outside (22 clean occurrences in three scenes). `4C DB` spawns from descriptor `0x801F2930` through `FUN_801E57F0` into the CLUT blend fade `FUN_801E4D8C`; jouine issues it four times. |
| What colour does retail clear the field to? | resolved (black) | `capture` | Uncovered pixels read `(0, 0, 0)` in five of five field-mode crops (mei_house_inside, keikoku_chest_pre, new_game_cutscene_intro_a, name_input_ui, v0_1_tetsu_dialogue_accept). |
| Does a scripted camera tile window survive into the next scene? | resolved (yes - by 78 vsyncs) | `capture` | A per-vsync poll across a real `map01` -> `town0c` door: the scene word flips at vsync `37` and the window at `0x1F8003E8..EB` keeps the previous scene's values until vsync `115`, when the entry stamps `(-7, -6, 5, 7)` and the per-region writes take over. The port re-stamps per entry as retail does, but with `FIELD_DEFAULT_VIEW_WINDOW` `(-8, -6, 6, 10)`, which is a later region's window. A Rim Elm house door is an intra-scene warp (`town0c` stays), so it is no fixture for this question. [`encounter.md`](../../formats/encounter.md#the-window-is-not-cleared-with-the-scene). |
| What is field-VM `4C 14`? | resolved (the **actor clone**, and it is eight bytes) | `disassembly` | The only eight-byte instruction in outer nibble 1: the nibble's prologue advances seven and the `0x14` arm at `0x801E0E80` reads a sixth payload byte and adds one more in a branch delay slot. That byte names a cross-context source actor; `FUN_801D835C` copies its position, rotation, bound model and `+0x68` onto a fresh pool node and writes the fade rate and modulation colour the two earlier operands carry. Ninety-four sites in six scenes, three rates; a seven-byte reading desyncs each record from its first occurrence. [`script-vm-menuctrl.md`](../../subsystems/script-vm-menuctrl.md#0x4c-nibble-1-sub-4---the-actor-clone). |
| What do `4C 86` and `4C 87` do? | resolved (the reflection controller's install and teardown; neither parks) | `disassembly` | `4C 86` reads its **last** operand byte as a cross-context actor id and spawns from descriptor `0x801F2948`, handler `0x801E5154`, writing `+0x90 = executing ctx`, `+0x94 = resolved actor` and six `s16` into `+0x80..+0x8A`. The tick reads `+0x94` and writes `+0x90`, so the **named** actor is the source and the executing script is the image; the six words are the mirror line and tracking rect. `4C 87` retires every live one and advances two, as `4C 9F` does against another handler. [`script-vm-menuctrl.md`](../../subsystems/script-vm-menuctrl.md#4c-86--4c-87-are-the-reflection-controllers-install-and-teardown). |
| Which model owns the field screen-effect fade? | resolved (the **push**) | `capture` + `disassembly` | `FUN_80024EE4(kind, blend, packed)` runs once per step from the effect actor op `0x34` sub-0 spawns, with the global multiply tint `DAT_8007BCB8..BA` neutral on all 900 measured vsyncs - a `(kind, blend, packed)` triple, the push's shape. The arm is a **pair**: a walk-out spawned from the previous target plus the walk-in, `blend = (op0 & 1) ? 2 : 1`, push kind `8` / `0` / `2` off the same sub-op byte, and an all-zero operand clears the live actor instead of ramping to black. [`cutscene.md`](../../subsystems/cutscene.md#the-arm-is-a-pair-and-the-sub-op-byte-carries-both-selectors). |
| Why does a `juui1` name hijack draw black? | resolved (the tint belongs to the **departing** script) | `disassembly` | Of 498 clean op-`0x3F` doors, 446 are preceded by `34 05 FF FF FF 41 00` - a white `ColorIntensity` beat the leaving scene's own record runs as its door prologue. A hijack rewrites the destination name and never executes that prologue, so the hijacked frame is black, and a hijack into a brightly-drawn control scene is equally black. The `conc` -> `conc2` -> `juui1` route is two ordinary doors (`conc` P2[11] `+0x001E`, `conc2` P2[20] `+0x0078`). |
| Do shipped scene scripts carry developer flag-setting menus? | resolved (nine scenes do) | `disassembly` | `map01`, `geremi`, `keikoku`, `suimon`, `town0b`, `town0c`, `doman`, `kor5` and `jou` ship records whose text is "Clear all flags", "Set all flags" / "Clear" / "Exit", "On" / "Off" / "Exit" or "=Back=", and whose arms are genuine `51` / `61` ops over nine-flag ladders. They are why a flag-writer census over-counts: the arms decode clean, so a beat and a menu row look identical to a scan that only reads opcodes. Classifier `man_field_scripts::debug_flag_menu_arm`; [`script-vm.md`](../../subsystems/script-vm.md#shipped-scene-scripts-carry-developer-flag-setting-menus). |
| What gates the fishing catch HUD's depth and tension block? | resolved (one word, `DAT_801d91b4`) | `disassembly` | The catch HUD `FUN_801d1580` gates the depth readout and the tension bar on the single word set at the hook - not on a phase or a separate visibility flag. A host that adds an idle-phase gate of its own draws a different HUD from retail's on exactly the frames between the strike and the hook. [`minigame-fishing.md`](../../subsystems/minigame-fishing.md). |
| Where does a field actor's heading live? | resolved (it is the **middle** of a rotation triple at `+0x24`) | `disassembly` | `FUN_8001ADA4` hands the whole vector to the rotation setter - `addiu a0,s0,0x24` / `jal 0x80026988` at `0x8001AF04`, and again at `0x8001B2A4` / `0x8001B2C8` / `0x8001B320` - so the actor carries pitch at `+0x24`, yaw at `+0x26` and roll at `+0x28`. A reader taking the yaw halfword alone sees a heading and loses the tilt. |
| What is `_DAT_8007B854`? | resolved (the **ambient-particle master gate**, not an input lock) | `disassembly` | Field-VM op `0x4C` outer nibble 3 raises it and clears it, both stores in a `j` delay slot off the 16-entry table at `0x801CEEB8`: `0x801E0F38` is `sw v0,-0x47ac(v1)` with `v0 = 1` and `0x801E0F44` is `sw zero,-0x47ac(v0)`. Six references exist disc-wide and none is pad state - two SCUS clears, the field render pass at `0x80026EBC` which stages the particle table into scratchpad when the word is set and the game mode is 3, and the ambient emitter's own opening load at `0x801D605C`. A field script decides per scene whether ambience emits at all. |
| teien's hedge-base ground fill - does retail draw a kind-2 cell? | resolved (**no** - retail has no `0x0800`-cell draw channel) | `capture` | A live `teien` field-run pass visits 1536 window cells and emits 370 - exactly the cells carrying `0x1000` - and none of the 42 `0x0800`-only cells. Only 8 of 84 images reach `*(0x1F8003EC)`, only PROT 0900 / 0901 hold a per-cell pass, and each one's `andi 0x800` reads an **object record's** `+0x12`, not a cell. teien's `0x0800` cells are a 6x6 platform block, a ten-cell row and three strays, the same shape `edteien` has. Cell bit `0x8000` is a per-tile depth-sort flag (own farthest `SZ` against a fixed far bucket `(0x3FF6 >> ot_shift) * 4`). |
| Actor `+0x16` - heading, facing, or footing? | resolved (**Y** of the position triple) | `disassembly` | `+0x16` is the Y of the actor's `(+0x14, +0x16, +0x18)` position triple. Nothing on the disc masks it as an angle: 0 of 77 accesses in the field overlay 0897 are angle-masked, and the four masked accesses in SCUS are `actor[+0x96]`. `FUN_8003BC08`'s second arm is therefore a **ground-follow**: skip on `& 0x2`, authored `-actor[+0x8E]` on `& 0x20000000`, a global off-switch when `& 0x20200` is clear and `_DAT_8007B6A8 == 0`, an outright snap to the sampler when `& 0x2000` is clear, and otherwise a ramp clamped to `+-6 * DAT_1F800393` per frame. Detail on [`functions/game-modes.md`](../functions/game-modes.md#8003bc08-ground-follow). |
| Which index does the fishing bring-up rewrite? | resolved (the **rod** index, not the lure) | `disassembly` | `FUN_801CF070`'s tail (`0x801CF35C..0x801CF39C`) probes the bag for item `0xA0 + _DAT_80084454` through `FUN_80042F4C`, steps the persistent rod index on a miss, wraps at 3 and gives up after six probes by storing `0` - so a rodless player fishes with rod `0`. The lure gate is a different routine over a different band: `FUN_801D712C` walks items `0x9D..0x9F` and rewrites `_DAT_80084450`. The same entry also seeds the 16-rung floor-height ladder at `0x1F80035C` and carries a dev grant of `999999` fishing points behind `_DAT_8007B9B0`. See [`minigame-fishing.md`](../../subsystems/minigame-fishing.md). |
| What writes a scene's kind-2 (height-override) trigger cells at runtime? | resolved (exactly one writer) | `disassembly` | Field-VM op `0x4C` sub `0x83` at `0x801E20A8` - a rectangle re-floor through `FUN_801D5630(2, x, z)` with the coarse step in `op[5]`. Trigger kinds 0 and 3 have no writer at all, and the `.PCH` sidecar's `+N` fixup words have none either: all 97 on-disc tables carry zero there. The per-kind record strides are `gp[0..3]` = `4, 4, 4, 8`, read by `FUN_801D5AE0`. See [`script-vm-menuctrl.md`](../../subsystems/script-vm-menuctrl.md). |
| The ending-scene widget family - how many sites, in how many scenes? | resolved (311 sites, **ten** scenes) | `disassembly` | The op-`0x43` sub-op is `InsnInfo::ActorCtrl`, not `Insn::extended` (that field is a cross-context target marker, `0x80`). Counted on the sub-op the family has 311 sites across `edteien`, `edbylon`, `edbalden`, `edlast`, `edretoin`, `edkorout`, `edson`, `edstati3`, `edbubu` and `eddoman`. |
| `FUN_801D6058` - a cutscene element? | resolved (a field-overlay template, spawned once) | `disassembly` | It is the `+0x08` handler of the plain template at descriptor `0x801F271C`, spawned exactly once by the field MAIN INIT `FUN_801D6704` (`addiu a1,0x271c` at `0x801D6FC0`, `jal 0x80024c88` at `0x801D6FD8`, `sh s0,0x1a(v0)` seeding the `+0x1A = 1` scene arm), and gated on `_DAT_8007B8B8 == 0`. It is not cutscene-specific. |
| Rim Elm's south gate - why a seated player exits and a walking one does not | resolved | `disassembly` | The exit record is ungated, the second walk-on band is five inert bytes, and the wall is a collision row the gate object's own script paints. [details ↓](#rim-elms-south-gate) |
| Town/field free-movement locomotion | resolved | `capture` | [details ↓](#townfield-free-movement-locomotion) |
| Field ambient animation - what makes jou's ground pulse and the water shimmer | resolved | `disassembly` | Three mechanisms: the bundle type-6 CLUT-walk table (12 carriers, 9 of them field scenes), the ambient move-VM tree the MAN P1 placements install at entry, and jou's flesh pulse = the mode-3 CLUT-cell HSV cycler (`FUN_80019D50`, lightning = flag `0x364`). Full chain: [`field-ambient-fx.md`](../../subsystems/field-ambient-fx.md). |
| Ambient render-mode 4 - what the op-`0x1E` seat animates | resolved | `disassembly` | A **cyclic VRAM-rect scroller**, and it is what makes waterfalls fall. Per fired period (`+0xC6` drained by the frame step alone, no speed scalar) the render tail rotates the seated rect `+0xD0..+0xD6` left by `+0xCC * frame_step` and up by `+0xCE * frame_step`, each axis as StoreImage / MoveImage / LoadImage over a bump-allocated strip (`80021df4.txt` `0x80022CB8..0x80022EE0`). Seventeen scenes carry one at plain entry; sixteen scroll upward over a texture-band rect, `tunnelc`'s second seat scrolls a CLUT row sideways. Ported as `engine-core::world::ambient::vram_scroll`. [details ↓](#ambient-render-mode-4---the-vram-rect-scroller) |
| Master ambient record 0 - what reads the 8-byte rows | resolved (it is not a stager record at all) | `disassembly` | The **per-scene sound-effect descriptor bank** for cue ids `>= 0x200`. Both SFX readers resolve those ids as `*(u32*)0x8007B8D0 + offsets[0] + (id - 0x200)*8` - i.e. record 0 of whatever bundle is installed at `0x8007B8D0`, which in field mode is the scene prescript bundle (`field_asset_loader` `0x8001F850..0x8001F864` stores scene buffer + `0x12800`). `offsets[0]` is the identical word `FUN_800252EC` reads for stager id 0. Rows are the 8-byte descriptor of [`sfx-table.md`](../../formats/sfx-table.md), category 3 (a variable VAB slot). [details ↓](#master-ambient-record-0---the-per-scene-sfx-descriptor-bank) |
| town0e's morph-record installer | resolved (partition-1 placement 29) | `disassembly` | The install is the ordinary `0x34` sub-3 arg 0 (stager record 1), carried by **partition-1 placement 29** - a full placed actor with dialogue - as that record's second instruction, ahead of its `SysFlag.Test 0x1A` park/seat branch. A census keyed on script *shape* misses it; the entry-slice rule in the next row finds it. The pre-run loop (`0x8003B8BC..0x8003B8EC`) covers every placement `1..count-1` unfiltered, gated on a MAN having been dispatched in the load (`0x801D6D98` tests the MAN arm's `ori s4,s4,4`), which town0e's bundle carries. [details](../../subsystems/field-ambient-fx.md#town0es-installer-is-a-placed-actor-not-an-effect-actor) |
| Which op-`0x34` sub-3 installs fire at **scene entry** | resolved | `disassembly` | The ones the placement spawn-prologue slice (`FUN_8003A1E4`) executes - not a distinguished kind of script. The pre-run is gated on the record's first opcode being `0x24`/`0x25`, and the slice breaks after an opcode whose full byte is `0x21`; both nops, only one ends the slice. Ported as `engine-core::man_field_scripts::scene_entry_ambient_installs`. [details ↓](#which-op-0x34-sub-3-installs-fire-at-scene-entry) |
| Scene bundle type-7 slot content (VDF) | resolved | `disassembly` | The scene's vertex-morph delta pack (61 bundles populated; jou = 17 sub-entries), installed at `DAT_8007B7DC` via `FUN_8001FBCC`, consumed by the morph stager `FUN_8001C604`. Parser `legaia_asset::scene_vdf`; format in [`field-ambient-fx.md`](../../subsystems/field-ambient-fx.md#mechanism-3---strip-cycling-and-vertex-morphs). |
| VDF morph render substitution - what draws the staged vertices, and what arms the lanes | resolved | `disassembly` | Per drawn group of a part whose flags carry op-`0x0A`'s bit `0x1000`, `FUN_8001ADA4` (`0x8001B424..`) calls `FUN_8001C604` (scratch copy + weighted-delta blend + group vertex-pointer retarget) and restores the pointer after the draw. Arming = op-`0x0A` **mesh** stager parts in the ambient tree (`pack slot = model_sel - 5`; rikuroa 69/70 behind flags `0x281`/`0x282`, town0e 10/11, jagaroom 20/21); weights ramp via `FUN_80020740` steered by op-`0x32` envelope flags. jou arms nothing at entry (cutscene op `0x1F` only). Ported to all three render surfaces; details in [`field-ambient-fx.md`](../../subsystems/field-ambient-fx.md#the-vdf-vertex-morph-chain). |
| What opens an inn stay in retail? | resolved (nothing - there is no inn session) | `capture` | Retail composes a stay inline in the scene MAN out of generic ops (dialogue, an MES picker, an op-`0x4E` gold gate, op-`0x3A` `ADD_MONEY`, fades) and then one `4C 82 <slot>` per member - the only inn-specific opcode in the engine. Charge and restore are decoupled, so free rests are the same tail minus the gate. Port `op4c_n8_sub2_restore_party_slot`. [details](../../subsystems/field-menu.md#inn-stay-there-is-no-inn-screen) |
| Field collision-map source | resolved (the `.MAP` supplies the base grid) | `disassembly` | [details ↓](#field-collision-map-source) |
| Tile-board grid mode | resolved | `disassembly` | The `_DAT_8007b450`/`DAT_801f35c0`/`801ef2b0` tile-grid walk is a puzzle / board minigame (procedural `rand`-filled board, per-cell drawn tiles), not town locomotion: a field-overlay (`0897`) construct driven from the field/event VM (op `0x49`). The `_DAT_8007b450` refs in the hub minigame overlays are only the shared equip-comparison layout hint `FUN_801e5b4c`. The `func_0x800467e8` facing remap is a quantized 45° octant rotation. Boards are always procedural; no fixed board exists. There is no `FUN_801e0b1c` - it is a mis-based dump alias of `0x801EF334`, interior to `FUN_801ef2b0`. Instruction detail and tile values: [`tile-board.md`](../../subsystems/tile-board.md). |
| game_mode 0x03 = field/town gameplay | resolved | `capture` | [details ↓](#game_mode-0x03--fieldtown-gameplay) |
| Scene prescript: field-VM event scripts vs move-VM stagers (dual consumer) | resolved | `capture` | **Single consumer.** The op-`0x34` sub-3 operand census across every scene MAN shows every prescript record is a **move-VM stager**: partition-1 effect-actor records stage the ambience on entry (record 0 = the master ambient record in 62 scenes), partition-2 cutscene timelines install the per-shot ids. Id space = record index (the RAM `[u16 count][u16 offsets]` relocation at `_DAT_8007b8d0`, live-pinned vs the file bundle). No retail path runs a prescript record through the field VM. See [scene-bundles](../../formats/scene-bundles.md) § consumer census. |
| Engine VRAM byte-exactness for town01 | resolved (major source); minor residue | `capture` | [details ↓](#engine-vram-byte-exactness-for-town01) |
| CLUT row 510 population (env meshes' `(64,510)` CBA) | resolved (boot-resident system-UI strip band); residue = the exact boot walker call site | `capture` | [details ↓](#clut-row-510-population-boot-resident-system-ui-strip-band) |
| Scene-transition (`0x3F` door) destination indexing | resolved | `capture` | [details ↓](#scene-transition-0x3f-door-destination-indexing) |
| Intra-town (house / interior) door mechanism | resolved | `disassembly` | [details ↓](#intra-town-house--interior-door-mechanism) |
| Field/town environment-geometry placement | resolved (renders) | `capture` | [details ↓](#fieldtown-environment-geometry-placement) |
| Overworld / town entrance story-flag gating | resolved | `capture` | An entrance's unlock is its own partition-2 record's C1/C2 gate (`FUN_8003BDE0`; C1 = one-shot latch, C2 = requires-all) against the system-flag bank `_DAT_80085758`. Ops `0x50/0x60/0x70` (SET/CLEAR/TEST) carry `idx = ((opcode & 0x8F) << 8) \| operand` (raw flag number). Disc-pinned via `man-scripts --system-flag-census`: map01 keikoku portals `C1=[0x193]` (setter `vozz` P1[7], the only `0x193` SET disc-wide, byte-pinned by `chapter1_hub_depth_oracle.rs`), mist walls `P2[34..36] C1=[0x482]`, town01 dinner chain `P2[4]`→550→`P2[5]`→551; the dinner does not re-fire. See [world-map.md](../../subsystems/world-map.md) + [field-locomotion.md](../../subsystems/field-locomotion.md). |
| Overworld story-conditional destination (`dolk`→`dolk2`) | resolved (mechanism + engine port) | `capture` | Beyond the record-level C1/C2 gate, an entrance record can switch its `0x3F` target by an in-record op-`0x70` `SysFlag.Test`. `map01`'s dungeon entrance (`P2[1]`/`P2[2]`) branches on flag `0x142`: clear → `dolk` (pre-boss), set → `dolk2` (post-boss), same trigger + arrival tile - so `dolk2` is not reached from a dungeon interior. `overworld_portal_sites` decodes the conditional `0x3F` pair (`ConditionalDest`); the seeder resolves via `World::system_flag_test` (`chapter1_boss_spine_oracle` Part D). The `0x142` setter is rikuroa's streaming-carrier script records. See [world-map.md](../../subsystems/world-map.md). |
| Retail-vs-engine NPC + story-flag state parity across the capture library | resolved (breadth oracle) | `capture` | The sweep oracle `crates/engine-core/tests/field_npc_state_parity_disc.rs` compares every catalogued field-mode library capture against a cold engine entry with the capture's `DAT_80085758` bank seeded byte-for-byte: park/place visibility, seat position within the patrol-locality bound, heading (diagnostic), post-entry flag neutrality. Divergences are classified in-test (`KNOWN_DIVERGENCES`); the dominant class is capture-mid-beat dynamics - a mid-visit choreography re-arranged NPCs after retail's own entry, while the engine reproduces the FRESH-entry arrangement (cross-pinned by sibling captures, e.g. rikuroa `pre_caruban`). |
| Entry pre-run channel slice ends on a no-mask `4C 70` wall paint | resolved (the slice continues) | `disassembly` | All four nibble-7 paints continue. `0x801E3624` is the *epilogue*: all four sub-ops return, and their advances differ (subs 0/1 `+6`, subs 2/3 `+7`); there is no shared continue label and no label-call idiom. The slice continues because the **caller loops**; breaks come only from an executed `0x21` NOP, a stalled PC, or a next opcode whose `& 0x7F` is `< 0x20`. Detail: [`script-vm.md`](../../subsystems/script-vm.md). |
| Writer of the Rim Elm opening flag (`549`) | resolved (a self-latching script SET) | `capture` | **town01 `P2[3]` itself**, one site: a plain `52 25` SET at body `+0x3` in the very record its C1 gates (the rikuroa-`P2[50]`/`0x142` self-latch shape). Runtime: a reader-watch from `s2_rimelm_town01` sees the SET at `ra 0x801E3598`, script-PC `+0xF`. Static: the walk reaches it only when the preceding `4C ED` op carries its width - without it the walk desyncs one byte short. Not a second site in a `gameover_data` "dev copy": that is town01's own MAN seen through a neighbouring block's window. See [script-vm.md](../../subsystems/script-vm.md); anchor `flag_549_writer_is_the_rim_elm_p2_3_self_latch`. |
| Field `.MAP` PROT resolution - which entry holds a scene's map | resolved (census-pinned) | `capture` | [details ↓](#field-map-prot-resolution---define--2-universal) |
| World-map CLUT cycling beyond the ocean head | closed (operand table + emitter + cadence pinned) | `capture` | [details ↓](#world-map-clut-cycling-beyond-the-ocean-head---closed-operand-table--emitter--cadence-all-pinned) |
| `init_data` UI-tile page residency; the map03 terrain column | resolved (both premises falsified) | `capture` | [details ↓](#init_data-ui-tile-pages---journey-dependent-residency-resolved-map03-texture-column-resolved---not-uploaded-premise-falsified) |
| What transitions retail into game over? | resolved | `capture` + `disassembly` | Retail has **no** mode-`0x12` transition. A wipe exits battle to mode 2; MAIN INIT `FUN_8003AEB0`'s back-from-battle arm stores `game_mode = 0x16` (CARD INIT) + `_DAT_8007BB00 = 1` at `0x8003B5D4`, landing on the **title screen** - no GAME OVER art, no menu. Three more sites carry the identical pair (`FUN_8003C7EC`, `FUN_801D84B4`, the STR attract exit `0x801CF048`). Mode 18/19 + PROT 0902 are an unreachable dev harness. Both port hosts hold, draw nothing and hand to the title. [details](../../subsystems/battle.md#party-wipe--the-game-over-overlay) |
| Mid-visit NPC re-arrangement beats (dolk2 market crowd; garmel pre-Zeto staging) | resolved | `disassembly` + `capture` | dolk2: the swap is `P2[11]`, spawned by the `.MAP` fallback walk-on-trigger rows (C1=[`0x27C`], C2=[`0x142`]) - eight `CC <crowd> E3 <day>` seats (op `4C` nE sub-3, `0x801E3108`) put P1[53..60] on the day cohort's tiles and `A3` parks the day cohort at `(127,127)`. garmel: the Zeto stager `P2[12]` materializes P1[3]/P1[4] beside the player (n3 sub-7 player-coord copy `0x801E0FB0`); post-battle re-entries run `P1[0]`'s flag-consume arms. See [script-vm.md](../../subsystems/script-vm.md#mid-visit-npc-re-arrangement-beats-dolk2-market-swap--garmel-boss-staging); pinned by `engine-core/tests/man_midvisit_rearrangement_disc.rs`. |
| Region story-flag gate families (record-header C1/C2 gates) | resolved (structure, play order by milestone brackets, the two undecided orders unobservable) | `capture` + `disassembly` | [details ↓](#region-story-flag-gate-families) |
| Extraction-0874 §2 (`player.lzs`) F-variant pixels | resolved - installing event named | `capture` + `disassembly` | [details ↓](#extraction-0874-2-playerlzs-f-variant-pixels---a-one-shot-opening-face-frame-stamp-not-a-menu-writer) |
| Which chapter-1 scenes the engine can load, script, walk and leave | resolved as a per-scene verdict; four of the five late "one-way" rooms leave in-engine | `disassembly` + `capture` | [details ↓](#chapter-1-scene-frontier) |
| How a player leaves the Uru Mais chain (`uru`, `urudre1..3`) and `jouine` | resolved (all five have walk-on exits carried by the scene's `.PCH` trigger sidecar) | `disassembly` + `capture` | [details ↓](#the-uru-mais-chain-and-jouine-exits) |
| Why did the port drop roughly half the disc's `0x3F` destinations? | resolved (the clean-label gate was lower-case-only; retail never compares the operand against a name table) | `disassembly` | [details ↓](#the-upper-case-destination-fold) |
| Why do `bubu1` and `edbubu` resolve no MAN? | resolved (both ship a `count = 5` asset table; the detector bounded `count` to 6 or 7) | `disassembly` + `capture` | [details ↓](#the-count-5-asset-tables) |
| Why did 28 scenes of the frontier closure become unwalkable at once? | resolved (a scene change mid-ledge-hop leaked the hop's one-way steering lock - an engine bug, fixed) | `disassembly` + `capture` | [details ↓](#the-ledge-hop-lock-leak) |
| What consumes the scratchpad window `0x1F8003E8..EB`? | resolved (the renderer's visible tile window; the `0x801F2778..84` mirrors are write-only) | `disassembly` | Four signed bytes `[nearX, nearZ, farX, farZ]`, tile offsets from the camera tile, written by the camera-zone loader `FUN_801DBC20` and field-VM op `0x46`. Read by the render library's cell emitters (`FUN_801F7088` at `0x801F7434..0x801F746C` and siblings), the camera scroll clamp `FUN_801DAA50`, the ambient emitter `FUN_801D6058` and dev-menu rows `0x12..0x15`. Invisible to the word scan: every access is `lui 0x1F80; ori 0x314; lb 0xD4(rX)`. Details: [`encounter.md`](../../formats/encounter.md#the-scratchpad-window-0x1f8003e8eb). |
| What is the field run-button mask `0x800846DC`? | resolved (`0x48` = Cross \| R1, seeded once by the new-game data init; not configurable) | `disassembly` | The last of four button-mask words at `0x80084140 + 0x590..0x59C` (`0x44`, `0x21`, `0x10`, `0x48`), stored by `FUN_80034A6C` (`0x80034AA0..0x80034AB8`) and read by the field mover at `0x801D0364` against the held mask `_DAT_8007B850`. No other writer in SCUS or any overlay; it sits inside the saved block. The engine's default mask is the retail pair plus Square (`FIELD_RUN_BUTTON_MASK_DEFAULT`); `FIELD_RUN_BUTTON_MASK_RETAIL` is the exact set ([`field-locomotion.md`](../../subsystems/field-locomotion.md)). |
| Who latches the clip-end bit for a conversation's cross-context clip pokes | resolved (port residual named) | `disassembly` + `capture` | The **poked actor's own anim tick**, on the poked actor's own `+0x62`. `FUN_8003C83C` short-circuits target `0xF8` to the live player object out of `_DAT_8007C364` before its actor-list walk, so an NPC record's `A2 F8 <clip>` / `AC F8 08` / `AD F8 08` reads and writes the *player's* clip words. [details ↓](#clip-end-latch-for-cross-context-clip-pokes) |
| What is the `scene_asset_table` header's `+0x04` word? | resolved (sum of the descriptors' decompressed sizes; never read) | `disassembly` | Equals `Σ descriptor.size` in all 105 containers of the family on the disc and exceeds the carrying entry in every one; `FUN_80020224` reads `+0x00` (`lw s3,0x0(s4)` at `0x80020288`) and steps descriptors from `+0x08`, and a corpus sweep for loads off `*(0x8007B85C)` finds offset `0` only. |
| How does a scene bundle reach `_DAT_8007B85C`? | resolved (whole-sector block copy) | `disassembly` | `FUN_8003D26C(*(0x8007B85C), *(0x8007B8C4), sectors << 6)` at `0x801D6918` (32 B per iteration = `sectors * 0x800`), into the `0x62C00` arena `FUN_8001E1B4` allocates at `0x8001E28C`. |
| What consumes the "field-pack" entries? | resolved (the scene texture pack at block `+4`) | `disassembly` | `FUN_800255B8` builds the path by mode (`tim.dat` `0x0A`, `move.mdt` `0x0F`, `<scene>.pac` `0x14`) and loads into `*(0x8007B85C)`; `FUN_8002541C` walks a bare pack into `FUN_800198E0` (mode `0x0A`) or the chunk chain into `FUN_8001F05C` (mode `0x14`). Not a format of its own - see [`field-pack.md`](../../formats/field-pack.md). |
| What do the asset-table `Flag(0x0A / 0x0F / 0x14)` descriptors do? | resolved (they are the streamed-file loader's mode argument) | `disassembly` | `FUN_8001F05C` returns `(case << 8)` for those types (`0x8001F574` / `0x8001F60C` / `0x8001F658`), `FUN_80020224` ORs the returns, and `0x801D6BF8` shifts right by 8 before `jal FUN_8002541C`. Corpus: 28 blocks `Flag(0x14)` carry a DATA_FIELD stream at `+4`, 4 `Flag(0x0A)` a bare pack, 64 without a flag a pochi filler. |
| Which screen opens menu window 46 (`FUN_801D603C`)? | resolved (the casino prize counter's Yes / No confirm) | `disassembly` | Script `0x801E4F2C` = `01 2E 00 00` (byte-verified in PROT 0899's widget-script pool), handed to `FUN_801D6628` by `FUN_801DC1CC` at `0x801DC408` / `0x801DC41C` - index `0x20` of the sub-screen pointer table at `0x801E4F40`, selected on entry-context kind 7 at `0x801DC8CC` - staging `_DAT_801E46D0` at `0x801DC414`, the state word the painter's marker decode reads. The `04 2E` closer at `0x801E4F38` has no reference in any image. |
| Which prescript copy does `FUN_800252EC` install from - the sister entry or the `.PCH` `+0x800` copy? | resolved (the next PROT entry, through the `efect.dat` window; there is no split) | `disassembly` | `FUN_800252EC` reads its `[count][offsets]` table from `_DAT_8007B8D0` (`0x800252F4`), which the field asset loader sets to `*(0x1F8003EC) + 0x12800` at `0x8001F864` - one sector past the `.PCH`, so the `.PCH` window at `+0x12000` is never the source. Writers of `gp+0x5B8`: `0x8001F864`, `0x8001FAC8` (the `bse.dat` battle buffer), `0x801CF018` (0975), `0x801CEEFC` (0977). |
| What does the `0x801F2858` template's tick do? | resolved (the scene **shutter blackout**, `FUN_801DD784` `0x801DD784..0x801DD9B4`) | `disassembly` | Two full-width bars ease in from the top and bottom and **meet**, holding the screen black - a scene-change shutter, not a cinematic letterbox ([falsified](../re-do-not-re-walk.md#field--locomotion)). Its one spawner is `FUN_801DE754`, from field-VM op `43 0C`; `FUN_801CFF3C` is the same routine printed `0xE818` low. |
| What is field-VM op `0x4C` nibble 9? | resolved (the scene floor-height ladder) | `disassembly` | Sub-`0xE` installs all sixteen rungs (`-words[i]` into `0x1F80035C + i*2`), subs `0..2` set one rung oscillating through `FUN_801DDE34` -> `FUN_801DA930`, and sub-`0xF` retires every oscillator. Not a fade family: the destination is the elevation LUT `FUN_80019278` and `FUN_8003A55C` read. See [`script-vm-menuctrl.md`](../../subsystems/script-vm-menuctrl.md#nibble-9-is-the-floor-height-ladder-not-a-fade). |
| What is `0x801D44CC` in the dance overlay? | resolved (the step-marker mesh flipbook) | `disassembly` | It selects the marker actor's mesh row from its `+0x50`, the `clip - 6` value the floor pass stamps at spawn - it flips the marker's picture and does not face a dancer. See [`minigame-dance.md`](../../subsystems/minigame-dance.md#the-sprite-part-emit-dispatch). |
| Who calls the move-VM extension dispatcher `FUN_801D362C`, and is the port live? | resolved (one caller disc-wide, and the port is reached) | `disassembly` | The only reference of any form is SCUS `0x80023AE0`, the move VM's own op-`0x2F` arm; the world-map controller does not call it directly. The port is live through `MoveHost::ext_dispatch`. See [`move-vm-overlay-ext.md`](../../subsystems/move-vm-overlay-ext.md#one-caller-and-it-is-ported). |
| How does the shop's buy list order and ink its rows? | resolved (a three-row Platinum Card hoist, with a last-rule-wins dim) | `disassembly` + `capture` | Case `0x0B` splits the walked rows at `record_count - 3`: rows below the split stage into `0x801C6220` tagged `0x3000` and are appended **after** the last three, which go straight into the row buffer tagged `0xA000` and are inked 5. The last three are walked only when `FUN_80042F4C(0xFF)` finds the Platinum Card: Retock's Items Shop emits 13 rows with it and 10 without (`autorun_shop_buy_list.lua`). A row dims when `purse < price` **or** `held >= 99`. See [`shop.md`](../../subsystems/shop.md#the-last-rows-come-first). |
| Where do the tile-board's cells live? | resolved (heap, one byte per cell, allocated at install) | `disassembly` | `DAT_801F35C0 = FUN_80017888(0, width * height)` at `0x801EF3E8..0x801EF3F4` - the one writer - with `width` / `height` re-read from `_DAT_8007B450[3]` / `[4]`. Nothing pads it. The walk SM's teardown state `0xE` frees it with the tile-actor table `DAT_801F35BC` through `FUN_80017B94` (`jal` at `0x801EFE78` / `0x801EFE88`); the per-scene control-block reset is a separate clear. `FUN_80017888` is the logging wrapper over the best-fit allocator `FUN_8002B468`. See [`tile-board.md`](../../subsystems/tile-board.md#where-the-board-comes-from). |
| What are the menu's HP / MP ink thresholds? | resolved (`FUN_800349EC` / `FUN_80035EA8`, with a fixed ailment arm order) | `disassembly` | The two routines carry the tier tests, and the ailment arms are evaluated in a fixed order, so a readout's colour is decided by the first arm that matches, not by a priority table. Rows in [`functions/menus.md`](../functions/menus.md); law in [`field-menu.md`](../../subsystems/field-menu.md#hp--mp-health-tier-inks). |
| Who clears the `-1` menu entry-context park? | resolved (the submode dispatcher's retire arm writes Done; no menu teardown clears it) | `capture` + `disassembly` | When the pause-menu session clears `+0x3E` (`0x801ED52C`), `FUN_801F159C` stores `1` into the still-live park (`0x801F16AC`) and op `0x49`'s Done arm zeroes it (`0x801E08D8`); a capture at the `town01` save point shows exactly one such store after the menu closed. Not a menu teardown leaf ([falsified](../re-do-not-re-walk.md#menus--ui)). Port `World::release_menu_entry_context_park`. |
| Does a save point open the pause menu by itself? | resolved (yes - op `0x49`'s `-1` rows are a scripted menu-button press) | `capture` + `disassembly` | The Idle arm spawns the menu button's subsystem actor (`0x801E09A0`); its enter half stores handler `7` before the table read (`0x801F140C`), and handler `7` picks the pause-menu session `0x30`. The `town01` capture logs park store, enter, state pick (`+0x50 = 7`), session (`+0x50 = 0x30`) and game mode `23` with no Start press. Ported as `World::scripted_menu_open_pending` on all three hosts; see [`field-menu.md`](../../subsystems/field-menu.md#which-screen-opens-a-window). |
| What keys the menu entry-context byte? | resolved (the record **kind**, not the screen's position) | `disassembly` | `0x00` shop -> `0x1A`, `0x01` save -> `0x19`, `0x07` casino -> `0x20`, `0x0D` -> `0x04`, and the sentinel `1` -> `0x02`, the debug character-parameter editor. Not positional: read by position, the save entry lands on `0x02` ([falsified](../re-do-not-re-walk.md#menus--ui)). |
| What arms `_DAT_8007B8B8`? | resolved (a one-shot entry-mode **argument**, not a latch) | `disassembly` | Ten writers and 25 readers over 84 images. One writer is `0x80016414` inside `FUN_80016230`, the mode-transition pass; two more are `0x80026094` and `0x80046E28` (the latter only when the word is already non-zero) and one is `0x801CEF18`; six sites write zero. Nothing retains it across a load, so the field MAIN INIT's `0x801F271C` spawn is **per-scene**, not boot-only. Its neighbours `_DAT_8007BACC` and `_DAT_8007B76C` have no writer at all, which makes the recentre-window form they gate unreachable. |
| Which way round is actor `+0x8A` bit 0? | resolved (it **suppresses** the scripted motion VM) | `disassembly` | `beq` at `0x80038194`: a zero byte runs the bytecode. The bit gates the player-engaged / actor-busy / off-map early returns at `0x8003819C..F4`, so setting it stops an actor rather than starting one. Op `0x12` waits for the bit to *change*, seeded `1`/`2` at `0x800396B4`, and consumes its tick unconditionally. |
| What are motion ops `0x06`, `0x0C` and `0x0E`? | resolved (a home-relative wander, a tint/draw-mode fade, and a two-base model bind) | `disassembly` | `0x06` draws `rand() & 6` and steps inside a box of signed 7-bit tile deltas taken from the actor's home at `+0x8C` / `+0x8D` - it never leaves one tile of where it was placed, and it is not a pad echo. `0x0C` fades `+0x74` (packed RGB tint, scheduler kind 3) and `+0x78` (draw mode), not a position channel. `0x0E` splits unsigned at `0xF0` between the scene model base `0x8007B6F8` and the second base `0x8007B824`. |
| How do motion ops `0x10` / `0x11` / `0x12` address their target? | resolved (one selector over five halfwords) | `disassembly` | The candidates are `+0x10`, `+0x12`, `+0x62`, `0x1F800394` and `0x1F800396`; byte 1's `0x30` field picks the low half, and `b1 & 0xC0 == 0xC0` asserts `0x3039` at `0x8007B828` and then dereferences null. Ops `0x02`, `0x0A` and `0x0B` are gated on the `0x801C6470` record's `0x8C` unset sentinel; op `0x09`'s callee `FUN_80035B50` is an SFX-cue enqueue; op `0x14` writes `+0x72`, the render scale `FUN_8001B964` reads. The table's slots `0x1A..0x1F` point at the loop test itself, so 26 op bytes cover 24 bodies. |
| How does the field follow camera get its pose? | resolved (record -> parameter block -> compose -> ease or snap) | `disassembly` + `capture` | `FUN_801DBC20` splits one MAN section-3 camera-region record into the parameter block at `0x8007B607..0x8007B627`; `FUN_801DAB90` turns that block, the player's position, the floor height from `FUN_80019278` and the scratchpad attribute box into a staging descriptor at `0x801F3580`; and `FUN_801DB510` walks the six-entry list at `0x801F2798` toward it by `delta >> shift` plus the sign, on frames the player moved. `FUN_801DB8EC` is the same walk as a copy. Over the nineteen walkable states the composed pose is exact in all eight settled free-roam ones. [details](#the-field-follow-cameras-pose-chain) |
| Who runs the camera zone query in retail? | resolved (the field VM, not the arrival actor) | `disassembly` | `FUN_801DBE9C` queries only on its `_DAT_8007B868 != 0` leg, and that word is the dev/dual-mode gate - zero in retail, with `FUN_80034A6C` storing `DAT_8007B606 = (B868 == 0)`. Retail's query is `FUN_801DE3E0(tile_x, tile_z)`, reached from field-VM arms `[4C 38]`, `[4C 39]` and `[4C C4 x z]` plus the op-`0x45` LOAD, and it installs a fixed miss set when no record covers the tile. |
| What draws field fog, and what raises it? | resolved (a pool of textured sheets, gated by one script word) | `disassembly` + `capture` | `FUN_801D629C` is a **spawner**: it maps the player's tile to a MAN section-4 region record (`[enable][x0][z0][x1][z1][angle][spread][speed][unread][u16 flag]`) and pops one of eighty `0x18`-byte records from the pool at `_DAT_8007B7E0`. The draw is SCUS - `FUN_8003F348` walks the pool, `FUN_8003F3FC` updates and emits, and `FUN_8003F86C` lays a ten-word `POLY_FT4` (code `0x2E`, texture page `0x27`, CLUT `0x7640`, OT `SZ2 >> 5`) per sheet. The emitter's width jitter `FUN_8003F838` is dead: the caller seeds the value with the rate first. The master gate is `_DAT_8007B854`, raised at 128 sites across 70 of 124 scenes, every raise in a P1 record. |
| What composes the field camera's `TR`? | resolved (the live eye trio **is** the eye-space translation; there is no eye-back depth constant) | `capture` + `disassembly` | `FUN_800172C0` builds the field view from the live globals, never from the composer's staging descriptor: `FUN_8005B4B8` copies the eye trio `0x800840B8/BC/C0` into a scratch matrix's `t` as three 32-bit words, `FUN_8003D344` MVMVAs the focus through the scaled rotation with `cv = TR` and writes `MAC1..3` back over that `t`, and `FUN_8005B6A8` uploads it. The focus is the **low signed halfwords** of `0x80089118/1C/20`. Held on every sampled frame of three field states. A `1x` renderer divides the trio by the base matrix's scale. [details](../../subsystems/renderer.md#the-field-view-matrix-where-tr-comes-from) |
| Retail's field camera focus Y | resolved (`0`, on every sampled frame) | `capture` | Only X and Z of the focus trio are ever written in the field - `FUN_801DBE9C`'s retail leg and the focus clamp `FUN_801DAA50` write that pair and nothing else - so `0x8008911C` reads `0` on 19 of 19 library states while the player's footing on those frames does not. The composer's **staging** focus at `+0x1A/+0x1E/+0x22` does carry the footing, and the view build never reads the staging descriptor. Vertical framing therefore rides the composed eye Y. |
| Why the port's field framing sat low against retail | resolved (entirely the focus-Y term) | `capture` | Over the states whose pitch, yaw and `H` match, mean horizontal error is 0.1 px of 320 and mean vertical error 15.8 px of 240, always low - and it tracks the anchor exactly: 0.0 px at footing `0` (five states, IoU >= 0.996), +12.1 at `-24`, +31.3 at `-128`, +33.5..+38.9 at `-192`, with a pixel cross-correlation on `town01` of +35 against an analytic +33.5. Both sides run the same projection kernel, and the composed eye trio is exact on seven of nineteen states with the four settled misses all mid-ease, so neither was the gap. |
| The two scene-entry eye-trio writers - which runs last? | resolved (they do not race, and the first field frame reads neither) | `capture` | Field MAIN INIT `FUN_801D6704` calls `FUN_80025C24` at `0x801D698C` and then `FUN_8003AEB0` at `0x801D6DA8`, which reaches `FUN_801DE37C` at `0x8003B01C`; A before B on 3 of 3 entries, so B's `(0, 0x200, 0x4000)` stands. It does not matter either way - the follow composer replaces the trio within one vsync, and `town0c`'s entry value equals the door-warp library state's trio exactly. |
| Which sites re-query the camera zone? | resolved (seven `jal` sites plus one gated per-frame site; a bare tile crossing is **not** one) | `disassembly` | `FUN_801DE3E0` (query + load in one) is reached from the three nibble-3 arms, `[4C C4]`, the player seat / warp path at `0x801D1FE8..0x801D2014`, its sibling at `0x801D2BCC` and the SCUS field init at `0x8003B800`. The field per-frame controller adds one at `0x801D17FC`, gated on scratchpad flag bit `22`; the per-mode seed copies a `u16` and cannot set that bit, so only a script raises it, in eight scenes (a count of fifteen is the masked flag bit matched inside desynced records). The re-query rule is disc data, not engine policy. [details](../../subsystems/script-vm-menuctrl.md#0x4c-nibble-0x380x3e---the-camera-zone-arms) |
| Is the port's camera-relative pad remap retail's? | resolved (exact over the whole input space) | `capture` | Eight camera octants times eight held directions, latched from the same call of `FUN_800467E8` on a live `town01` field state: 64 of 64 cells agree with `World::remap_pad_direction`, including the ring's not-found case, and the ring `DAT_800766FC` carries the same eight **values** in `SCUS_942.54`, in RAM and in the port's constant - not the same bytes: retail's ring is `u32[8]` and the port's `[u16; 8]`. Retail publishes no ring step to read back - `gp+0x2D8` is scene-authored (the free-roam state holds `0`) - so the probe supplies the index. Probe `scripts/pcsx-redux/autorun_field_pad_ring.lua`. |
| Is retail's actor facing the engine's heading plus a half turn? | resolved (`player+0x26 == render_26 + 0x800`) | `capture` | 61 of the same 64 cells agree exactly; the three misses are the sweep's three 1536-unit turns, each 512 short at the cell's last held vsync because the actor eases toward the new facing. |
| Does a zero field focus Y frame retail's field? | resolved (yes - it is the whole vertical error) | `capture` | Over the eight library states whose camera words match retail's, a focus anchored on the footing frames 15.8 px of 240 low on average and is exact only where the footing is zero; with focus Y zeroed the mean falls to 2.2 px, the four states with exact camera words are pixel-exact (player-box IoU 1.000), and a pure-shift pixel fit on `town01` agrees to 0.3 px. Oracle `crates/engine-shell/tests/field_camera_zone_oracle.rs` with `LEGAIA_CAMERA_ORACLE_DUMP`. |
| What is the bag-normalize gate's base register? | resolved (`0x80084140`, the live game-state block) | `disassembly` | `lui v0,0x8008` at `0x801E0580` and `addiu s0,v0,0x4140` at `0x801E0584` sit immediately above the `lbu 0x454(s0) == 2` / `lhu 0x458(s0) == 0x100` pair at `0x801E05B0..C8` that guards the sole `jal 0x800423E0` (`0x801E05D0`, PROT 0897). `FUN_800423E0` itself opens with the active-window setter `FUN_8004313C` and walks the bag over `gp[0x2D2]..gp[0x2D4]` skipping zero ids. |
| Is the resident scene-name string an input to the loader? | resolved (no - it is the loader's output) | `capture` | Overwriting `0x8007050C` leaves it reading the replacement for the rest of the run while the game loads the original scene anyway; a capture that reads a scene name out of RAM is reading what the loader said, not what it will do. |
| What does `edbylon` select when the tile query misses? | resolved (nothing selects - the block is **held** from an earlier tile) | `disassembly` + `capture` | The ending-vignette state stands on tile `(94, 43)`, outside the attribute box `[77, 34, 104, 50]`, and all three of the scene's section-3 records are kind `0` with anchors outside it, so no record covers the tile and 12,655 of 16,384 tiles select record `#0` anyway. The scene carries exactly one `[4C 38]` site, and with no per-frame re-query the parameter block simply survives from wherever the player last crossed a queried tile. |
| Is the Throw Out cursor a bag slot or a list row? | resolved (a **bag slot**, and the list hides empty slots) | `capture` + `disassembly` | `_DAT_8007BB88` is the list kernel's selected-row payload, and the pause menu's Throw Out confirm `FUN_801D8734` zeroes `bag[cursor * 2]` with it directly. A driven capture over a bag holed at slots 1/3/6 puts the cursor on 0, 2, 4, 5, 7, 8 and never on a zero-id slot across 561 vsyncs: the displayed list is compacted, the payload is not. The bag itself is **not** compacted on menu open - `FUN_800423E0` runs zero times, and its sole reference is a field-VM arm at `0x801E05D0` gated on two words of the live block. Removing by display row throws the wrong stack away on a holed bag. |
| Is `gp+0x2D8` (the pad-ring octant) camera-derived? | resolved (**authored** - it is scene content) | `disassembly` | Six writers disc-wide, all field-overlay: op `0x4C` nibble `2`'s arm `0x801E0EB8` storing `sub_op & 7`, the tile-board walker's four (`0x801EF8B0` / `0x801EF8B8` / `0x801EF8CC` / `0x801EFE7C`), and a clear at `0x801E5664`. Two of its four readers are in `SCUS_942.54`: the pad remapper loads the word at `0x800467E8` and `0x80046840`, which is what makes the octant a rotation at all. The arm also turns the player with it, so an author picks the octant to match the camera the same script installs. A free-orbiting port has no authored index to read and must derive one. [details](../../subsystems/field-locomotion.md#gp0x2d8-is-authored-not-computed) |
| Who reads `0455_urudre1`'s descriptor 0? | resolved (nobody - it is a switched-off duplicate) | `disassembly` | The descriptor's type byte is `0x0A`, a pure-flag arm the walk never dereferences, and its payload is SHA-256-identical to the first 343,480 bytes of the live copy the scene actually loads. It is an authoring leftover holding a valid pack, not a slot with an unfound consumer. The three towns' reserved descriptor-0 payloads hash alike for the same reason. |
| The `0x4C` outer dispatch table, and its two non-handler arms | resolved (sixteen entries at `0x801CEE60`; nibbles `B` and `F` are the error printer) | `disassembly` | The outer table sits at `0x801CEE60`, materialised by a `lui`+`addiu` pair and indexed by `op0 >> 4` (`srl v1,s3,4` at `0x801E0C44`, `sltiu v0,v1,0x10` at `0x801E0C48`). Arm `0x801E3550` (nibble `B`) and arm `0x801E3538` (nibble `F`) each materialise their own string pointer with a `lui`+`addiu` pair and converge on retail's message printer at `0x801E3558` (`jal 0x8001A068`), so the two share a tail rather than a body; only `4C FF` branches away first, to the ordinary continue at `0x801DF098` (`beq` at `0x801E3540`). The disc agrees - the opcode census finds no coherent `4C Bx` or `4C Fx` in any scene. |
| What the field state machine's slot 7 holds | resolved (the submode **return** state, not a mode of its own) | `disassembly` | The enter half installs it at `0x801F140C` and parks it in `scene[+0x40]` at `0x801F148C`, after which `+0x50` is overwritten with the op-`0x49` sub-op's own slot. So slot 7 is where the field returns *to* when a submode ends, which is why a port that collapses the chain and keeps no `scene[+0x40]` has nothing to return to. |
| What does a field submode return to? | resolved (nothing reads the parked word) | `disassembly` + `capture` | The op-`0x49` enter parks a state twice through the scene pointer `0x801C6EA4` - `scene[+0x2E] = -1`, then `scene[+0x40] = s4[+0x50]` at `0x801F1400` and again at `0x801F148C`; the pair idiom tiles PROT 0897 **22** times. Nothing reads `+0x40`: `SCUS_942.54` and all 86 extracted overlay images yield zero loads at that displacement off a register loaded from `0x801C6EA4`, and a two-byte read watch records none while the pointer still names the scene struct. Later hits are the GPU working buffer the block is recycled into (`0x80043F74`, `0x800455E8`, `0x80044054`, `0x80059DE4`). [details](../../subsystems/field-locomotion.md#the-submode-return-state---a-parked-word-nothing-reads) |
| Does the field passive-ability badge column have a suppress gate of its own? | resolved (no - it sits inside the party readout's) | `disassembly` | `FUN_801d095c` has exactly one caller, the `jal` at `0x801D130C` inside `FUN_801D0D38`, eight bytes above the epilogue that routine's suppress arm jumps to - so the badges carry the readout's suppression exactly and none of their own. The gate is the readout's entry block `0x801D0D38..0x801D0DBC`, four terms: `_DAT_8007B868`, `_DAT_800845C4 == 2`, `_DAT_8007B850 & 0xF000` and scratchpad `0x1F800394 & 0x00800000`. A host gating the two columns on different predicates draws one of them over dialogs, cutscenes and battles. |
| What do the fishing bite tick's two per-frame map reads do? | resolved (one gates water, the other drifts the lure) | `disassembly` | Both take the same point - the tick's own actor `+0x14` / `+0x18`, the lure the cast spawned - and are otherwise unrelated. The water gate is bit `0x4000` of the `+0x8000` cell halfword (`andi v1,v1,0x4000` at `0x801D3374`), whose class word `FUN_800180EC` rebuilds at `0x801D3384`. `FUN_801D7030`'s `+0x4000` walk-grid probe feeds none of that: a hit drifts the lure's 24.8 `x` accumulator `0x801D9174` by `frame_delta << 11`, signed by the low bit of the lifetime cast counter `_DAT_80084460`. [details](../../subsystems/minigame-fishing.md#the-lure-the-bite-tick-probes) |
| Which screen opens window 25, and which opens window 41? | resolved (different screens; no program opens both) | `disassembly` | The menu image holds exactly one open-op (`01 <win> 00 00`) for each window: `0x801E4DDC`, inside the Equip screen's candidate script `0x801E4DC8` (sub-screen `0x14`, beside window 24), and `0x801E4E7C` inside the shop-entry script `0x801E4E64`. No program names both, and the equipment-buy recipient sub-screen `FUN_801DB380` adds only window 36 over the set already up - the shop's recipient flow does not open both ([falsified](../re-do-not-re-walk.md#menus--ui)). |
| What is the equip compare panel's category byte? | resolved (the **accessory passive index**) | `disassembly` + `capture` | The lookup reads the item record's class byte first, then one of two tables indexed by that record's `+1`: class `1` takes the equipment bonus row `0x80074F68 + row*8` byte `+5`, everything else the item-effect descriptor `0x800752C0 + row*4` byte `+3`. Of 255 non-zero ids, 104 are class `1` and every bonus row they reach carries the `0x40` no-passive sentinel; 80 of the other 151 carry a real index - 9 under `6`, 4 in `10..=12`, the rest ATK / UDF / LDF. So the byte names the passive, and the panel shows the stats it moves. [details](../../subsystems/field-menu.md#both-category-arms-are-live-on-retail-data) |
| How many equip slots does the menu stat block sum? | resolved (**five**) | `disassembly` | `FUN_801CF650`'s loop counter is bounded by `slti a2, 5` at `0x801CF744`, so the block behind windows 22 / 25 / 41 sums the first five equip bytes only; the battle-side aggregator `FUN_80042558` walks all eight. On retail data the difference is inert - the accessory class the extra slots hold is absent from the equipment table - but the two are different routines, so a port sharing one kernel has to zero the tail for the menu side. |
| What is the equip browse row's index space? | resolved (a two-table **slot map**, not the equip-byte index) | `disassembly` | Row `0` is the **weapon** row and takes a per-character halfword from `0x8007B42C`, which reads `2, 3, 2` out of `SCUS_942.54` - Vahn and Gala's weapon byte is `2`, Noa's `3`. Rows `1` and up index `0x801E43E8`, whose bytes run `00 01 00 04 05 06 07`, so the on-screen order is weapon, helmet, body, footwear, then the three Goods slots. `slti v0, s0, 4` silences exactly the four gear rows, so retail resolves a compare category for the three Goods rows only, consistent with the `0x40` sentinel on all 104 class-`1` rows. |
| Which way does an obstructed sub-cell drift the fishing lure? | resolved (by the cast counter's low bit) | `capture` | Forcing the branch at `0x801D2E28..0x801D2E58` 949 times gives 447 ADD hits, every one on an odd `_DAT_80084460`, and 502 SUB hits, every one on an even value - zero violations - with the 24.8 accumulator `0x801D9174` moving by exactly `frame_delta << 11` per hit. Retail rarely sees it: at the catalogued fishing venue the walk-grid probe returned zero on all 949 calls, so the lure never drifts there. |
| What writes the fishing cast counter `_DAT_80084460`? | resolved (the bite tick itself, once per hook) | `disassembly` | Reached through the save-block base rather than its own address - `t0 = 0x80084140`, then `lw` / `addiu` / `sw 0x320(t0)` at `0x801D2954..0x801D296C` - which is why a displacement scan finds only the `0x4460` read. The increment sits on the hook arm, the one that raises cue `0x204` and sets SM state `0x19`, so it steps once per hooked fish and the lure's drift direction therefore alternates between casts. |
| What consumes the fishing bite tick's per-cell fish weight? | resolved (it is the modulus of the caught fish's **size** roll) | `disassembly` | One use: `div s0, s4` at `0x801D3728`, remainder taken with `mfhi a3` at `0x801D3750`, summed with three other terms and stored at `DAT_801D91B8` (`0x801D3814`). The same value plus `0x400` becomes the render scale of the object the catch just spawned (`sh a0, 0x72(s1)` at `0x801D381C`, on the object from the preceding `jal 0x80024C88`). It touches neither the species roll nor the bite credit - so a deeper cell makes a bigger fish, not a different one. |
| What is the Baka Fighter VRAM-rect table `DAT_801DBE84`? | resolved (exactly two records) | `disassembly` + disc bytes | `(0x340, 0xC8)` and `(0x340, 0xE0)`, both `6 x 0x18`, both blitted to the same fixed destination `(0x340, 0x86)`. Its eight bytes end at `0x801DBE8B`, and everything above that to PROT 0976's `0xE000` end is zero - so it is the last initialised data in the overlay, and an index past the second record reads the destination's own column back rather than a third rect. |
| Does retail's equip screen offer a Goods-slot candidate, and out of which id space? | resolved (yes - item class `2`, and the filter carries no character mask) | `disassembly` + measurement | The screen has **two** candidate families. The slot-browse step writes window 23's `+0` content id per row from the eight-byte table `0x801E4DC0` = `00 17 15 16 18 1C 1D 1E` (PROT 0899 file `0x165A8`), so the three Goods rows reach `FUN_80030628` cases `0x1C` / `0x1D` / `0x1E`, whose filter is item-record class `2` (`bne` at `0x800317D8`) plus item-effect `+3 != 0x41` (`beq` at `0x800317F8`). The armament cases `0xE` / `0xF` / `0x10` carry their own character mask; the Goods cases carry none. On the USA disc 80 of 151 class-`2` ids pass, and all 71 rejected carry exactly `0x41`. |
| What does the shop quantity window print? | resolved (quantity **/** bound, not quantity x price) | `disassembly` | Window 35's value row is a pair with a separator glyph between them (`FUN_8003C1F8(6)`): the second number call at `0x801D563C` loads `DAT_801E46B8`, the word the buy picker's phase 0 fills with `min(gold / price, 99, 99 - held)`. The price appears once, in the running total, and a currency pictogram draws at `(WX + 0x58, WY + 0x24)`. The window scripts are buy phase 0 `0x801E4EB0` (ending `[01 23]` -> window 35), buy confirm `0x801E4ED4` (`[04 23]`), sell phase 0 `0x801E4F08` (`[01 25]` -> window 37) and post-sale `0x801E4F10` (`[0A 26]`). |
| How does sub-screen `0x15` exchange two list rows? | resolved (a two-press latch in one word) | `disassembly` | Bit `0x1000` of the second cursor word means **nothing is latched**. A confirm taken with it set stores the hovered row index and clears the bit; the next confirm swaps the two rows and re-raises it; a cancel taken with a latch pending only drops the latch, leaving the list alone. So one word carries both the pending row and the fact that one is pending, and only the spell list's running step carries the arm at all. |
| Which equip byte is the Ra-Seru slot? | resolved (per character, from `0x8007B424`) | `disassembly` + disc bytes | The table reads `3, 2, 3` (`lui 0x8008` / `addiu -0x4bdc` at `0x801DA5D0`) - the exact complement of the weapon table `0x8007B42C` = `2, 3, 2`. So Vahn and Gala wear the Ra-Seru in byte `3` and the weapon in byte `2`, and Noa the other way round; a reader taking either table as a constant gets one of the three characters wrong. |
| Is the `0x801E43E8` byte run one table or two? | resolved (**three** tables and a pad byte) | `disassembly` | The browse row -> equip-byte map is seven bytes and stops. `0x801E43EF` is alignment - no word, no `lui`/`addiu` pair and no branch in any image reaches it - while `0x801E43F0` (4 bytes, the per-character equip **mask bits**, `and`ed with the equipment record's `+6`) and `0x801E43F4` (8 halfwords, the per-row slot **pictogram ids**) each have three materialisation sites of their own. Bytes `[7..10]` looking like the gear-slot indices is the pad plus the mask table's first three entries. [`field-menu.md`](../../subsystems/field-menu.md#the-0x801e43e8-run-is-three-tables-not-one) |
| What does retail's equip candidate step draw? | resolved (three stacked panels; the compare category follows the hovered **item**) | `capture` | One script, `0x801E4DC8`, opens window 25 (name + one stat-row set), window 24 (item name with owned count, description, bonuses) and window 24's reserved box at `(WX, WY + 0x38)` sized `0x90 x 0x28` for an accessory passive. A pad ladder driven to each of the seven rows populates all seven lists. The `slti v0, s0, 4` guard at `0x801D137C` silences the four gear rows, so only the three Goods rows resolve a category - and they disagree with each other, because the row set follows the hovered item. [`field-menu.md`](../../subsystems/field-menu.md#what-the-candidate-step-draws-measured) |
| Where is retail's own entry to sub-screen `0x15`, and which lists can it reach? | resolved (the root picker's row 3 = Status; steps 3 and 4 only) | `disassembly` + `capture` | `0x801D6C4C`, in `FUN_801D6B20`'s row-3 arm, is the only site in PROT 0899 that writes `0x15` into `DAT_801E46A4`. Window `0x15` is a **different id space** - the Equip screen's party window, opened by script `0x801E4DA0`. Inside the screen the character picker's confirm arm folds the second cursor and dispatches: folded `0` / `5` hop columns, `1` buzzes, `2` writes step `3` and `3` writes step `4`. Nothing writes step `2`, so the abilities list is decoded and has no door. [`save-screen.md`](../../subsystems/save-screen.md#which-screen-raises-it-and-which-of-the-three-lists-it-can-reach) |
| Does retail's wall slide fire in ordinary play? | resolved (yes) | `capture` | On `s3_rimelm_freeroam` the resolver `FUN_80046494` answers the held mask on 261 of 276 calls and widens it on 15, all on one held-`LEFT` run down the `town01` exterior wall (`0x8000` -> `0xC000`). At both pinned wall-press rest positions the resolver hands back the held cardinal, so those legs are slide-neutral. Port `World::resolve_field_slide`, called by the field stepper. [`field-locomotion.md`](../../subsystems/field-locomotion.md#the-skid-measured-on-retail) |
| Is the camera's visible-tile window a property of the scene? | resolved (**no** - of the region) | `capture` | A 3000-vsync pad-driven `town01` walk never holds the window constant and never holds the port's default, alternating between `(-8, -6, 8, 12)` and `(-10, -6, 8, 14)` four times each as the player crosses regions - the camera-region record's mask-kind side-write. The walk stays in one scene; the door case is the scripted-tile-window survival row of this table. [`encounter.md`](../../formats/encounter.md) |
| What does field-VM `4C D8` spawn? | resolved (a **morph-weight** actor; the two `u16`s are envelope rates) | `disassembly` | `FUN_801D77F4` allocates from the morph-weight descriptor `0x8007068C`, whose `+0x8` handler is `0x8002174C`; the tail wires a VDF body off `0x8007B7DC` by operand 1, a TMD off `0x8007C018` by operand 2 - a **scene-bank** index the op's arm has already offset by `0x8007B6F8` - and a rest pose *snapshotted* from the live vertices rather than loaded. The handler reads `+0x3C` / `+0x3E` at `0x80021890` / `0x800218B4` as the rise and fall steps of the weight at `+0x6E`, so the port's `kind` / `variant` names are the generic allocator's. [`script-vm-menuctrl.md`](../../subsystems/script-vm-menuctrl.md#what-the-0x4c-0xd8-spawner-builds) |
| Which target flow does a confirmed menu spell open? | resolved (the spell's own stats `+2` bit `0x20` picks) | `disassembly` | Set routes to the no-pick **group** sub-screen `0x10` (`FUN_801D9280`, `FUN_801D688C` called with `count = 0`); clear routes to the per-member picker `0x11` (`FUN_801D9594`). A party-wide heal is therefore one confirm and a per-member resolve, not a folded single-target cast. [`field-menu.md`](../../subsystems/field-menu.md#the-two-target-flows) |
| Does retail's cold entry into `conc` clear flag `0x6DE`? | resolved (yes, twice - and the flag is a live **position predicate**) | `capture` | A single-flag write watch from the memory-card load screen sees the scene load clear it from `P1[1]`'s spawn prologue (`+0x10`) and the `P1[0]` entry script (`+0x18`) - `ra 0x801E35C0`, the field overlay's CLEAR site - and then `P1[0]`'s per-frame body SET it every other frame from `+0x10B` (`ra 0x801E3598`) behind `CD F8 0A 0E 33 48` at `+0x100`, for as long as the player stands outside tiles `10..=51` x `14..=72`. Poking the player inside stops it (60 SETs before, 0 in 280 ticks after). The card-boot save stands at `(17, 97)`. [`script-vm.md`](../../subsystems/script-vm.md#a-system-flag-can-be-a-live-position-test-not-progress) |
| What position does a system script's `CD F8` box test read? | resolved (the live player's, on every evaluation) | `disassembly` + `capture` | Retail resolves cross-context target `0xF8` to the player object each time. The port re-seats its ctx-`0xFB` system context's anchor before each field frame slice (`World::sync_field_ctx_player_anchor`); without that, only the load-frame pre-run of `opdeene` / `opstati` / `opurud` seeds the anchor, the script install resets it to the origin, and every per-frame box test runs from tile `(-1, -1)` and answers "outside". 661 of the 668 clean `0x4D` sites disc-wide are the `CD F8` form, over 92 carriers. |
| What do the two screen-effect pushers of a `conc` -> `conc2` door write? | resolved (two **channels**, told apart by the OT bucket) | `capture` | Departure: 33 passes at a two-vsync cadence from the field overlay's `ra 0x801DDE24`, two concurrent pushes - bucket `2` an ambient `0x00003030` decaying, bucket `0` a white-out ramping to `0xFFFFFF`. Arrival: 15 passes from SCUS's `ra 0x80025034` on bucket `1`, `0xEEEEEE` falling to `0`. The first argument of `FUN_80024EE4` therefore selects a channel with two callers, and red sits in the colour word's low byte. |
| What does `4C D8`'s model operand index? | resolved (the **scene bank**, not the global pool) | `disassembly` | The arm adds the scene-bank base before the call - `lhu s0, -0x4908(v1)` (`0x8007B6F8`) then `addu` at `0x801E2DE0..0x801E2DE8` - and `FUN_801D77F4` reads `DAT_8007C018[slot]` unadjusted. On all seventeen sites the one-record block's `first_vertex + delta_count` equals the vertex count of object `0` of scene model `n` exactly, `balden`'s operands `99` / `100` / `109` / `110` included (through `balden2`'s count-5 MAN-less table). [`script-vm-menuctrl.md`](../../subsystems/script-vm-menuctrl.md#the-model-operand-is-a-scene-bank-index) |
| Does retail walk the morph block with one record pitch? | resolved (**three**, and they agree only on a one-record block) | `disassembly` | The spawner's size sum steps `0xC` (`0x801D78D0..0x801D7900`), its rest-pose copy `n_vert * 8` (`0x801D792C..0x801D799C`), and the apply pass `FUN_8002174C` `n_vert * 0x60` (`0x800217B4`, `0x80021860`). Every block the disc ships is one record naming TMD object `0`, so the disagreement is unobservable - a different claim from the pitches being equal. [`script-vm-menuctrl.md`](../../subsystems/script-vm-menuctrl.md#three-record-pitches-over-one-morph-block) |
| Does any beat take op `0x34` sub-0's forked arm? | resolved (**no** writer of the gate bit executes) | `disassembly` + `capture` | The arm forks on `_DAT_1F800394 & 0x800000` (`lui v1, 0x80` at `0x801DFD1C` and `0x801DFEB0` in PROT 0897; set -> `FUN_80024E80`, clear -> `FUN_801DE2B0`). A writer census of bit 23 over SCUS, every mapped overlay and every scene carrier finds no store the disc executes (`scratch_global_bit_writers_real`, and the field-op census), so the forked arm is never taken on retail. A block copy into the scratchpad is not excluded by a store census. |
| Does any MAN author motion-VM ops `0x10` / `0x11`? | resolved (no) | `disassembly` | Zero of the 573 section-1 stream variants carry either, so the `b1 & 0xC0 == 0xC0` assert arm that stores `0x3039` into `0x8007B828` has no shipped driver. |
| What does `FUN_8001FA00`'s one caller seed? | resolved (the **fog-particle pool's** free stack) | `disassembly` | Its only `jal` is at `0x801D7384` in MAIN INIT (PROT 0897), passing `(pool, pool + 4, 0x50)`: an identity list of eighty slots under a one-based top index. Not a cutscene sprite list. Port `engine-core::scus_leaf_kernels`, seeding `FogPool::reset`. |
| Is the 192-byte block at `0x801F21B4` one twelve-row probe table? | resolved (**three** tables) | `disassembly` | Three consumers form three bases: the actor-collision probes at `0x801F21B4` (six rows; `lui`/`addiu` at `0x801CFE74` and `0x801D5A70`), the leading-edge wall probes at `0x801F2214` (four rows; `0x801CFEE8`, `0x801CFFC0`, `0x801D009C`) and the interact facing compass at `0x801F2254` (eight `±64` points; `0x801D0834`). The wall and compass tables are not caller-less rows of the first. [`field-locomotion.md`](../../subsystems/field-locomotion.md), `legaia_asset::field_probe_tables`. |
| What clears the two halt bits an inn acquire sets? | resolved (the acquire's own FaceTarget leg in the walk kernel) | `capture` + `disassembly` | `FUN_8003BC08` runs the dialog SM and then, on `+0x10 & 0x400`, `FUN_8003774C` (`0x8003BD50`), which reads the acquire's `CC F8 85 14 00 33` as a `0x14`-frame player FaceTarget on bind `0x33`; its terminal frame clears the player's bit at `0x80038004` and its own actor's at `0x80038028`. A PCSX-Redux watch on `retock_innkeeper_talk_open` sees set at `0x801E21A4` / `0x801E21CC` and both cleared 18 vsyncs later, the box already open ([`script-vm.md`](../../subsystems/script-vm.md#the-interaction-cursor-one-record-two-consecutive-scripts)). |
| Whom does a talk acquire's FaceTarget turn toward? | resolved (the actor its bind names) | `disassembly` + disc census | The kernel resolves the bind at op `+3` like any cross-context id (`0x80037E00..0x80037EA8`): `0xF8` the player, `0xFB` the world-map entity, else the `_DAT_8007C354` node whose `+0x50` matches. Of the disc's `CC F8 85\|8E\|8F` acquires, 40 of the 146 in placement records bind another actor, and the 24 in object records and 861 in cutscene records have no own actor ([`script-vm.md`](../../subsystems/script-vm.md#the-interaction-cursor-one-record-two-consecutive-scripts)). |
| What does op `0x42` mode 1 test? | resolved (the held pad against a compass) | `disassembly` | `0x801DFBDC..0x801DFC9C` tests `_DAT_8007B850`, the packed held pad: `& 0xF000` against the compass word `0x801F28D0[op1 * 4]` for `op1 < 8`, Circle / Cross / Square / Triangle for `8..11`, and `op1 >= 0xC` takes the jump untested ([`script-vm.md`](../../subsystems/script-vm.md#0x37-0x42-yield-sound-rpg-state-dialog-jump)). |
| What is `0x1F800394 & 0x80000`? | resolved (the walk-on same-tile re-poll) | `disassembly` + disc census | While it is set and the movement lock clear, the dispatcher re-runs the tile's kind-1 record on an unchanged tile (`0x801D2090..0x801D20F8`); a crossing clears it first (`0x801D2110..0x801D2120`). `2E 13` is its only setter; its users are Rim Elm's stand-and-press-Down polls, `town01` `P2[12..14]` on tiles `(30..32, 19)` and their `town0b..0e` twins ([`field-locomotion.md`](../../subsystems/field-locomotion.md#the-same-tile-re-poll)). |
| What tag does the timed warp put on `0x801DA7F0`? | resolved (the `4C E1` text balloon) | `disassembly` | The timer-running arm (`0x801D1EF4..0x801D1F20`) tags for tear-down the first live actor whose handler is `0x801DA7F0`, the balloon op `4C E1` spawns ([`field-locomotion.md`](../../subsystems/field-locomotion.md#the-timed-kind-0-warp)). |
| What does `FUN_801DB510` do while `0x8007B606` is clear? | resolved (it pins the focus and runs the shake tail) | `disassembly` | `beq a1,zero,0x801DB820` at `0x801DB558`: the pin leg sets the focus to the negated player position, composes and eases nothing, and falls into the shake tail. The same leg serves `_DAT_1F800394 & 0x400`. The byte's writers are the new-game init (SCUS `0x80034B60`) and the developer menu's CAMERA row (`0x801EAD38`, `0x801EA1C8`). |
| Does `FUN_801CF8AC` change anything past its box test? | resolved (only the `+0x10 & 3` exempt early-out) | `disassembly` + disc census | Its no-class arm needs bit 17 clear, which nothing on the disc clears, and all three callers keep only `& 1` of its result (`0x800384C0`, `0x80038A78`, `0x80038F3C`), so that arm's `4` is dropped; the `+0x98` link is overwritten before any read ([`motion-vm.md`](../../subsystems/motion-vm.md)). |
| Is MAIN_INIT's `FUN_80017BEC` refresh observable? | resolved (yes, in three scenes) | `capture` + `disassembly` | Called unconditionally at `0x801D6BF8`, it changes cells in `retona` (1), `juui1` (1) and `noaru` (2, gaining the `0x800` elevation stamp). `retona_field_card_boot` holds `0x306B` at tile `(0x1D, 0x18)` where the disc's `.MAP` has `0x006B`; the `dolk2` and `chitei2` captures equal the refreshed map apart from `0x400` bind bits ([`field-locomotion.md`](../../subsystems/field-locomotion.md#floor-height-two-models)). |
| Who spawns the scripted-scene actor `FUN_801D4A60`'s openers? | resolved (the two world-map travel arts) | `disassembly` | The only other `jal 0x801D5A24` sites are `0x801EE110` (Riremito, program 1) and `0x801EE3A4` (Rula, program 0); the pair it parks on is the side-band bank pair `_DAT_8007BABC` / `_DAT_8007BAA0` ([`field-locomotion.md`](../../subsystems/field-locomotion.md#openers-and-closers)). |
| What does Incense do? | resolved (it suppresses encounters outright for a walk-tick window) | `disassembly` | Item class `0x82` reaches `FUN_80046870` (`jal` at `0x800421A0`), which adds `0x40` to `gp+0x2E8` (`_DAT_8007B600`), capped at `0x100`. The walk tick `FUN_801D0B90` drains it (`0x801D0CD4`), and the region roll `FUN_801D9E1C` skips its whole roll while it is non-zero (`0x801DA174`), step counter included. The Use list greys the row from `0xE0` (`FUN_80046898`, `slti 0xe0`) ([`field-menu.md`](../../subsystems/field-menu.md#command-sub-flows-use--throw-out--arrange)). |
| Does the field Magic screen charge the MP-saver discount? | resolved (yes, on every path) | `disassembly` | SCUS `0x8003118C..0x800311A4` inlines the discount and greys a row on `+0x10A < cost` (`0x80031204`); PROT 0899 debits the value `FUN_80035394` returns (single cast `0x801D93C0`, `0x801D9404..0x801D9418`; group cast `0x801D972C`), the re-cast gates compare against it and both panels display it. The engine prices field, battle and dome casts through one discounted kernel ([`battle-formulas.md`](../../subsystems/battle-formulas.md)). |
| When does an Incense leave the bag? | resolved (at the confirm, one copy) | `disassembly` | `FUN_801D8D94`'s Yes arm calls `FUN_80042310(0x8A, 1)` at `0x801D8E64..0x801D8E68` - before the class-`0x82` applier at `0x801D8EAC` - then runs the window script `0x801E4A80` and returns to the Use list (submenu 6). So one copy buys one top-up, and the list the route returns to no longer offers a copy the bag lacks. The port consumes the copy at the confirm (`engine-menus::pause_screens`) ([`field-menu.md`](../../subsystems/field-menu.md#command-sub-flows-use--throw-out--arrange)). |
| What does the Incense wear-off show? | resolved (a one-line notice) | `disassembly` | Entry kind `0x0B` maps through `0x801F33A4` to slot `0x32`, `FUN_801F1E48`: window record 16, painter `FUN_801F1B64`, one string naming item `0x8A` and saying its effect is gone; confirm plays cue `0x20`, hides, clears `_DAT_8007B450`. Ported as `engine-core::incense_notice` on both hosts ([`script-vm.md`](../../subsystems/script-vm.md)). |
| What is menu window 8? | resolved (the art-learned notice a Hyper-Art book raises) | `disassembly` | `FUN_801DCD58` renders the resident template `0x801E4700`, referenced only by its own `lui` / `addiu` at `0x801DCD68`. The book arm calls `FUN_80035C00(slot, art)` at `0x8004208C`, two stores into `gp+0x858` / `gp+0x860`; the Items screen seeds both to `0xFF` (`0x801D850C`) and opens window 8 through script `0x801E4C60` only when `_DAT_8007BB78` changed ([`field-menu.md`](../../subsystems/field-menu.md#command-sub-flows-use--throw-out--arrange)). |
| Which sub-screen does the menu driver open for an entry context? | resolved (a kind table, with a literal-1 escape) | `disassembly` | `FUN_801DC6B4` `0x801DC85C..0x801DC8E4` opens sub-screen `1` by default and maps kind `0` to `0x1A`, `1` to `0x19`, `7` to `0x20` and `0x0D` to `4`; a context pointer equal to the literal `1` opens sub-screen `2` and clears the pointer ([`field-menu.md`](../../subsystems/field-menu.md#which-screen-opens-a-window)). |
| What are `FUN_801DD330`'s arguments to `FUN_801DA9F8`? | resolved (a window id and an exit sub-screen) | `disassembly` | `(0, 9, 0x30, 1)`: `0x30` is the settings window's id, patched into bytes `+5` / `+9` of the script at `0x801E4E08` (`0x801DAA78`), and `1` is the sub-screen stored on exit (`0x801DAC24`), the root command picker - not an init word and not a slot selector ([`field-menu.md`](../../subsystems/field-menu.md#options-screen)). |
| Which Magic-list rows does the field menu grey? | resolved (every spell that fails any of three tests) | `disassembly` | The list build (`0x80031130..0x80031264`) writes each row as `0x5800 \| id` and rewrites it `0x5000 \| id` only when record `+2` bit `0x02` is set, MP covers the **discounted** cost, and the broadcast `FUN_8003053C` answers non-zero ([`field-menu.md`](../../subsystems/field-menu.md#magic-screen)). |
| How does the fishing strike credit count pad nudges? | resolved (one per mask hit on the newly-pressed word) | `disassembly` | PROT 0972 `0x801D343C..0x801D3468` loads `_DAT_8007B874` once and adds 1 for each of `0x8000` (Left), `0x2000` (Right) and `0xC0` (both reel bits, one mask), so a same-frame Cross + Square counts once and the cast press counts none ([`minigame-fishing.md`](../../subsystems/minigame-fishing.md#species-selection-and-the-band-4-gate)). |
| When does Baka Fighter book an exchange? | resolved (on the winner's strike keyframe) | `disassembly` | Fighter block `+0x0C` is the strike state (`0` armed, `1` landed, `2` consumed), `+0x90` the pre-step cursor and `+0x98` the landed keyframe. The combat tick `FUN_801D3F44` calls `FUN_801D6E5C` at `0x801D4334`; the resolver's arms require state `1` (`0x801D36DC`, `0x801D3730`, `0x801D378C`) and the damage kernel writes `2` (`0x801D3EB0`). A special commit sets `DAT_1F80037D = 4` (`0x801D4568`), halving both clip steps for the round ([`minigame-baka-fighter.md`](../../subsystems/minigame-baka-fighter.md#the-strike-clock)). |
| What is Baka Fighter's `FUN_801D49E8`? | resolved (the special attack's afterimage) | `disassembly` | Spawned on the special commit (`0x801D4538..0x801D4634`) with `+0x74 = 0x81000000` and `+0x78 = 0x800` (`0x801D4588..0x801D4594`); `+0x78` is a depth-cue level `FUN_8001B964` reads at `0x8001BC7C`, not a yaw. It draws two ghosts of the thrower, three and six frames behind, at cue `0x800` / `0xC00`, pushed `0x40` deeper ([`minigame-baka-fighter.md`](../../subsystems/minigame-baka-fighter.md#impact-cue-and-afterimage)). |
| Who is Baka Fighter's round-start cameo? | resolved (scene model 3, the ring girl, who winks) | `capture` + `disassembly` | `FUN_801D6310` spawns only with Triangle held at round setup (`0x801D0190..0x801D01C4`). The cabinet init stamps scene-bank base `+ 3` into its prototype (`0x801CF2C8..0x801CF2D8`) and the spawn store at `0x80020E70` writes `3`: the PROT 1203 stage pack's fourth TMD. Its cell blit swaps an open-eye patch for a closed one at the pose ([`minigame-baka-fighter.md`](../../subsystems/minigame-baka-fighter.md#the-round-start-cameo)). |
| Can a retail disc reach the Baka Fighter keyframe editor? | resolved (no) | `disassembly` | State `0x190` is written only from the developer-menu arm (`0x801D19DC`), and that menu (`0xC8`) is entered only while `_DAT_8007B868 != 0` (`0x801D08E8`); a disc-wide sweep finds two stores to that word, the constant `0` from `FUN_8002B92C` and a bit-clear ([`minigame-baka-fighter.md`](../../subsystems/minigame-baka-fighter.md#the-developer-keyframe-editor)). |
| Why does `opdeene` run long after its `apply 4800` camera move? | resolved (five record mechanisms; the port is within 2 % of retail) | `capture` + `disassembly` | NPC turns run on while the record advances, `AD <id> 08` waits on the poked clip's end latch, `B2 <id> 0A` ends the actor's leg, the `4C 45` ramp advances, and only `B3 F8 0A` waits for a crawl. [details ↓](#opdeene-runs-long-after-its-apply-4800-camera-move) |

### `opdeene` runs long after its `apply 4800` camera move

*Status:* resolved, evidence grade `capture` + `disassembly`.

The record's length is the sum of five field-VM pacing mechanisms, all in the engine. Ground truth is a
per-vsync capture of the zero-input chain from the `s1_newgame_field` state
([`autorun_opdeene_pacing.lua`](../../../scripts/pcsx-redux/autorun_opdeene_pacing.lua)): the record's
`ctx[+0x9E]` PC, the camera mover's progress, one vignette actor's flag word, clip ids,
`+0x62` / `+0x68` / `+0x6A`, position, heading and the crawl roller's retire count.

- **NPC turns do not park the record.** The op-`0x38` budget arm advances by 3 for every target
  (`li s7,3` in the delay slot at `0x801DEEFC`) and parks only for the player; the record reaches the
  next op after a `B8 08 82 21` on the same frame.
- **`AD <id> 08` spins on the poked actor's clip end latch.** A spin lasts the clip's remaining length
  (clamped) or the time to the next wrap (looping): actor `0x07`'s clip `0x18` holds `+0x4E4` for 90
  frames, actor `0x05`'s clips `8` / `9` for 27 / 57. The port binds the actor's clip cursor at the
  `0x22` poke from the scene bundle's record header (gate and divisor) and steps it at the live `+0x6A`,
  which `CC <id> 41` halves - a clip at rate `4` runs twice as long.
- **`B2 <id> 0A` ends the actor's leg.** The actor tick runs the walk kernel only while
  `+0x10 & 0x400` is up (`FUN_8003BC08`), so the halt clear stops a compass walk or turn where it
  stands. Two Seru legs cut this way account for about 300 frames.
- **The `4C 45` ramp does not yield.** The arm adds its 5 bytes at `0x801E12A4` and leaves through the
  common or the scheduler exit; the capture runs eight `CC <id> 45 .. 28 00` ramps inside one frame.
- **Only `B3 F8 0A` waits for a crawl.** The crawl spawn is a halt-acquire on the player
  (`0x801E1F24`) and the roller clears the bit as it retires its last page. `opurud`'s record tests it
  before its `3F` and changes scene four vsyncs after the last page; `opdeene`'s does not, and its `3F`
  runs with the 8-page roller three pages short of retiring
  ([`cutscene.md`](../../subsystems/cutscene.md#where-a-record-waits-for-its-crawl)).
- **An NPC compass walk on an actor nothing has moved** starts from the context's seat: actor `0x05`'s
  `C1 05 00 C4` takes 129 frames.

Measured: from the record's second opening wait (`+0x289`) to its `3F` the engine runs 3773 frames
against retail's 3828; from the `apply 4800` beat to the `3F`, 2268 against 2313. The gap is the frame
step: the capture holds `DAT_1F800393` at `3`, so retail's waits and clip ticks land on three-frame
boundaries the engine's one-frame step does not reproduce; at `+0xB4D` that decides whether a looping
clip's latch lands before or after the `AC 04 08` that clears it (one loop, 60 frames). The
retail-compare state parked on `+0x6F6` keeps its exact camera. Pinned by `opdeene_record_pacing.rs`
(3 % band).


### The field follow camera's pose chain

*Status:* resolved - the chain is pinned by disassembly and the pose measured over the walkable state population

Retail derives the follow camera per scene and per tile through a four-stage chain; none of its
outputs is a constant.

| Stage | Routine | What it does |
|---|---|---|
| Load | `FUN_801DBC20` | Splits one 18-byte MAN section-3 camera-region record into the parameter block at `0x8007B607..0x8007B627`, choosing among three layouts on the high nibble of `record[5]`. |
| Compose | `FUN_801DAB90` | Reads that block, the player's position and floor height, and the walk-region attribute box at scratchpad `0x1F800384..87`; writes the staging descriptor at `0x801F3580`. |
| Ease | `FUN_801DB510` | Walks the six-entry descriptor list at `0x801F2798` toward the staging fields by `delta >> shift` plus the sign of the delta; shift from the sixteen-byte table at `0x801F2804` indexed by `DAT_8007B60B >> 4`. |
| Snap | `FUN_801DB8EC` | The same compose plus list walk with a plain copy. |

- Compose samples the floor height through `FUN_80019278` with the MAN's **static** elevation LUT
  swapped in, so a scripted floor-tier bob moves the player without shaking the camera. The pitch
  couples to that floor height, not to a heading.
- Ease runs only on frames the player's position changed, so the camera stays short when he stops
  mid-glide.
- The trig LUT pointers are `_DAT_8007B81C` sine and `_DAT_8007B7F8` cosine.
- `FUN_8005B0B8` is a PsyQ-shaped `SquareRoot0` over the 192-entry mantissa table at `0x80078E84`,
  not a bit-packing helper.

Owning page: [`encounter.md`](../../formats/encounter.md#from-the-block-to-the-live-camera-compose-ease-snap).


### Chapter-1 scene frontier

*Status:* resolved as a per-scene verdict; the five scenes that read as sealed are not - see [the Uru Mais and jouine exits](#the-uru-mais-chain-and-jouine-exits)

The chapter-1 reachable set is the BFS closure of `town01` over each scene's own decoded `0x3F`
destinations. It is 27 scenes, it ends at exactly one kingdom boundary (`jiji -> map02`), and it holds
the whole Drake kingdom past the Ravine: the boss chain, the four-deep Drake Castle interior, the Uru
Mais rooms. All 27 load their assets, parse their MAN, enter in `Field` / `WorldMap`, settle their
entry script, carry a door, and can be walked out by pad alone.

- The closure is a reachability partition over `0x3F` only. Scenes reached by the sibling `0x3E` door
  warp (a scene-type selector, not a name) are outside it.
- `urudre2` walks out once op `0x45` CAMERA APPLY is decoded correctly
  ([falsified reading](../re-do-not-re-walk.md#field-vm-op-0x45-sub-0xc0-returns-the-operand-s16-as-the-next-pc)).

Door shapes, from three decoders over the same MAN (the clean per-partition fall-through walk, the
recovering destination-table pass, and the `.MAP` gate-1 trigger -> partition-2 record -> `0x3F` join):

| Shape | Scenes | What has a door |
|---|---|---|
| op, table and walk-on trigger | 22 of 27 | all three decoders |
| op behind text pages, trigger in the `.PCH` sidecar | `uru`, `urudre1`, `urudre2`, `urudre3` | all three decoders, plus the sidecar for the trigger; the clean walk crosses the inline `0x1F` text kilobytes ahead of the `0x3F` because a text segment is one decoded stride |
| FMV hand-off, trigger in the `.PCH` sidecar | `jouine` | no `0x3F` at all - the exit is `4C E2 08` (FMV 8 -> `town0e`), on the FMV dispatch table |

Probe limits that read as "sealed" on the bottom two rows:

- The walk-on sweep stops at 48 deduped gate-1 tiles; `uru` carries 118 with its exit band at
  positions 63..66, `urudre2` carries 186.
- The 24-tick post-step budget cannot run a record that spends 300+ frames in explicit waits or, for
  `jouine`, a 6.8 KB boss cutscene.
- The `.MAP` gate-1 join is half the join: these exits are carried by the scene's `.PCH` trigger
  sidecar, which the fallback read of `FUN_801D5630` reaches.

On a **first visit** `izumi`'s C1-gated spring record relocates the player about thirty tiles with the
pad released, so a driven probe there cannot beat its own released-pad control; a revisit probe walks
normally. That is a script, not a locomotion defect.

Measured by `crates/engine-core/tests/chapter1_frontier_ladder.rs`; nine of the closure's scenes are
cross-checked against the capture library's main RAM and all nine enter in-engine.

### The Uru Mais chain and jouine exits

*Status:* resolved - grade `disassembly` + `capture` (the `uru` exit fired live).

All five scenes carry a walk-on exit, and every exit band lives in the scene's `.PCH` sidecar
([`scene-v12-table.md`](../../formats/scene-v12-table.md)), not its `.MAP`; only `urudre2`'s is doubled
into the `.MAP`.

| Scene | Exit record | Gate-1 band | Tail op (MAN offset) | Destination |
|---|---|---|---|---|
| `uru` | `P2[42]` | `(36..39, 5)`, `.PCH` rows 23..26 | `0x3F` at `0x0D4B7` | `MAP03` `(0x24, 0x46)` |
| `uru` | `P2[37]` (`C2 = [0x36F]`) | `(37..39, 44)` | `4C E2 07` at `0x0CB11` | `uru2` via FMV 7 |
| `urudre1` | `P2[2]` | `(35..37, 22..24)` | `0x3F` at `0x01804` | `uru` `(0x40, 0x40)` |
| `urudre2` | `P2[9]` | `(26, 14)` + `(24, 13)` | `0x3F` at `0x01D78` | `map01` `(0x26, 0x51)` |
| `urudre3` | `P2[0]` | `(51, 90)` | `0x3F` at `0x02461` | `uru` `(0x40, 0x40)` |
| `jouine` | `P2[16]` | `(17, 17..19)`, `.PCH` rows 3..5 | `4C E2 08` at `0x03E90` | `town0e` via FMV 8 |

- `uru` also carries the three dream entrances (`P2[29]` -> `urudre1`, `P2[33]` -> `urudre2`,
  `P2[31]` -> `urudre3`), all `.PCH`-only and ungated.
- The four `0x3F` records share one byte-exact tail: `B1 F8 13` (set flag 19),
  `34 05 FF FF FF 41 00` (white fade), the `0x3F`, then the `26 FF FF` / `21` / `26 FE FF` park pair.
- `jouine`'s exit is the FMV hand-off of [`str-fmv-table.md`](../../formats/str-fmv-table.md)
  (`0689_jouine` -> `fmv_id 8` -> `MV6.STR` -> `town0e`, door `0x2E5`), mapped by the engine's
  `fmv_post_play_handoff`. It cannot be pad-probed from the catalogued state, which is already inside
  `P2[16]` running the evolved-Cort fight.
- **Live pin.** From `uru_field_run` (tile `(38, 6)`), holding the pad toward `(38, 5)` fires
  `FUN_8003BDE0(36, 5, 42, 1)` and then `FUN_8001FD44("MAP03")` with `ra = 0x801DEB1C` (the field VM's
  `0x3F` arm); probe `scripts/pcsx-redux/autorun_uru_exit_probe.lua`. The row `(36, 5, 42, 1)` exists
  only in the `.PCH`; `uru`'s `.MAP` gate-1 records are `[1, 3, 43]`.
- **Ruled out** in all five: a `0x3E` door warp with `op0 >= 100`, a `0x4C` staged-menu-warp arm, a
  kind-0 teleport row in the `.PCH`. The scripted-motion VM has no scene-change opcode.
- Unexplained: `urudre2` returns to `map01` (Drake) while `uru` exits to `MAP03` (Karisto).

Owning page: [`world-map.md`](../../subsystems/world-map.md#uru-mais-and-jouine-exits-carried-by-the-pch-sidecar).

### The upper-case destination fold

*Status:* resolved - grade `disassembly`

A `0x3F` destination name is case-insensitive in effect, because it ends in an ISO 9660 file open and
ISO identifiers are upper case.

- `FUN_8001FD44` `strcpy`s the operand into `0x80084548`; the field asset loader `FUN_8001F7C0`
  (`0x8001F7E8..0x8001F88C`) `strcat`s it into `DATA\FIELD\<name>.MAP` for `FUN_8003E6BC` ->
  `FUN_800608F0`, the ISO file open.
- 40+ distinct upper-case destination names appear across the 99 scene MANs (`MAP01/02/03`, `KOR*`,
  `DREAM`, `RETOCK*`, `ROPEWAY`, `NILBOA`, the `ED*` ending chain), every one a CDNAME label. The disc
  carries **no** mixed-case `0x3F` name run.
- `legaia_asset::field_disasm::clean_scene_name` accepts a uniformly-cased 3..=12-byte alphanumeric
  label and folds it. With the fold the `town01` closure is 68 scenes with 20 kingdom-boundary edges.

### The count-5 asset tables

*Status:* resolved - grade `disassembly` + `capture`

Retail bounds the descriptor count nowhere: `FUN_80020224` reads `+0x00` and loops that many
descriptors. The reliable signal is descriptor 0's anchor at `8 + count * 8` (`0x30` for count 5).

- The count-5 tuple is `(TimList, Man, Move, Anm, Flag(0x14))` - the canonical seven minus `Tmd` and
  `Vdf`.
- Census of every PROT entry parsing as this table: 1 / 2 / 10 / 4 / 8 / 80 entries at counts
  1 / 3 / 4 / 5 / 6 / 7. Of the sub-6 set only two carry a MAN.
- The detector admits `count >= 4` gated on a type-3 descriptor, which reclassifies exactly those two
  entries.

Owning page: [`scene-bundles.md`](../../formats/scene-bundles.md).

### The ledge-hop lock leak

*Status:* resolved (engine bug, fixed) - grade `disassembly` + `capture`

`start_field_ledge_hop` ORs `0x0008_0000` into the player's `move_state.flags` (retail
`0x801D25A8..0x801D25B8` on the player context's `+0x10`) and only the hop phase machine's end arm
clears it. A scene transition landing mid-hop tears the machine down without that arm, and the
locomotion step returns early on the bit in every later scene. Retail cannot reach the state: its hop
always finishes before a transition.

- Reproduction: walking the closure in one host, `tower` hops and ends at tile `(0, 0)`, and all 28
  scenes after it report the flag with zero driven tiles while each walks normally on a fresh host.
- Fix: `SceneHost::enter_field_scene` drops the hop and the lock together.

### Clip-end latch for cross-context clip pokes

*Status:* resolved from the disassembly of both halves and ported; residual is
port fidelity, not an open question

`ctx[+0x62]` is the clip-control word and bit `8` (`0x0100`) is its end latch
([`script-vm.md`](../../subsystems/script-vm.md#0x2b-0x33-flag-manipulation-triplets)). When a
flag triple carries a `0x80`-prefix target, the op runs against a **different actor record**, and the
latch is written by that actor's own anim tick `FUN_800204F8` - the same routine a prop reaches.

Resolver `FUN_8003C83C`:

- `0xF8` returns `_DAT_8007C364`, the live player object, without walking any list (`li v0,0xf8` /
  `bne a0,v0,0x8003c858` / `lw v0,-0x3c9c(v0)` / `jr ra`).
- `0xFB` walks a second list for the entry whose `+0xC` handler is `0x801DA51C`.
- Every other id walks `_DAT_8007C354` matching `*(u16*)(ctx+0x50)`.

Anim tick `FUN_800204F8` (`ghidra/scripts/funcs/800204f8.txt`):

| Half | What it does |
|---|---|
| Binder (`0x80020570..0x800205A8`) | Only when `+0x5C != +0x5E`: remember the id, `sh zero,0x68` (cursor to frame 0), point `+0x4C` at the clip. `+0x62` is **not** touched - hold / clamp / reverse are the script's to set, and clearing the latch is its `AC <t> 08`. |
| Advancer (`0x800205AC..`) | Consume `+0x62` bit `0x200` (restart), clear bit `0x100`, step `+0x68` by `+0x6A` unless bit `0x2` (hold) is set, then wrap or clamp at either end and set bit `0x100` there. |

- The advancer scales its step by the scratchpad frame-step byte `_DAT_1F800393` (`mult a0,v0` at
  `0x80020660` / `0x80020680`). `PropAnim::tick` advances one step per call, which is identical
  whenever that byte is `1`.
- Worked example, the retock innkeeper's Yes branch: `AC F8 01` (un-hold), `A2 F8 04` (poke the clip),
  `AC F8 08` / `AD F8 08` (clear, spin), `AC F8 03` (un-clamp), `4A 03 00`, `AB F8 03` (re-clamp),
  `AC F8 08` / `AD F8 08` again, then `A2 F8 02` handing the player back to the locomotion move.
  Every bit is an `ANIM_*` bit; none is a per-record local flag.

**Port.** `engine-core::field_env::PropAnimBank` holds a cross-context cursor per target byte
(`actor_clips`, keyed the way the resolver keys its walk). `World::step_inline_dialogue` binds the
poked actor's `+0x62` into the executing context around each `2B` / `2C` / `2D`, mirrors it back, and
parks on the spin - the bind / re-sync / mirror-back discipline `World::step_prop_interaction` runs a
prop's whole record under, narrowed to one word. `PropAnim::tick` is the port's only latch writer, and
the cursor advances once per frame whichever driver reaches it first (the field frame's
`tick_prop_interactions`, or the runner itself for a host that drives only a conversation). Pinned by
`crates/engine-core/tests/inline_clip_latch.rs`: the latch appears on the poked actor's cursor, never
on the record's own flag word, and a stalled cursor never lets the spin through.

**Port fidelity.** An actor's drawn clip and its latch cursor are two objects in the port where retail
has one struct: the player's gesture is played by the host's `FieldPlayerAnim` off
`World::locomotion.player_move_cues`, an NPC's by the host's clip player, and the latch is timed by the
bank's cursor. Both take their step from `field_anim::clip_step` with the record's gate and divisor
and are sized from the scene ANM bundle where it resolves the poked id; a clip the bundle cannot name
falls back to a stand-in length. No capture rules out another runtime writer of bit `8`; a script
could set it with a `2B <t> 08` of its own, and none of the records read does.

### Ambient render-mode 4 - the VRAM-rect scroller

*Status:* resolved - decoded from the disassembly and ported

Render mode 4 is a cyclic VRAM-rect rotation, seated by move-VM op `0x1E` and fired by the render
tail on a countdown.

- **Seat** (`80023070.txt` `0x80023694..0x800236F0`): `+0x5A = 4`, then seven operands into `+0xC4`
  (period reload), `+0xCC` / `+0xCE` (horizontal / vertical step) and the rect `+0xD0..+0xD6`. The
  live countdown `+0xC6` is not seated, so a new part fires on its first tick.
- **Fire** (`80021df4.txt` `0x80022CB8..0x80022EE0`): drains `+0xC6` by `DAT_1F800393` alone (the
  mode-3 sibling also folds in the `DAT_1F80037D` speed scalar) and fires on the tick the stored
  halfword's sign bit sets (`sll v0,0x10; bgez`).
- **Rotate**, horizontal then vertical: `FUN_8005842C` captures the leading strip into a buffer
  bump-allocated off `0x1F8003A0`, `FUN_80058490` slides the remainder, `FUN_800583C8` re-inserts the
  strip at the far edge.
- **Carriers.** Seventeen scenes seat one from their plain scene-entry ambient tree. Sixteen scroll
  vertically only, upward, over a rect at `x >= 0x200` (falling water, energy columns); `tunnelc`'s
  second seat is a full-width one-row rect at `(0, 508)` stepping **right** - a CLUT row. Count
  carriers by walking the records through the move VM: the records jump, so a linear scan finds
  `0x1E`-shaped bytes inside operand streams.
- **Port.** `engine-core::world::ambient::vram_scroll`, applied in tick order by
  `World::step_ambient_fx` (the rotate is destructive; the mode-3 write is recomputed each frame from
  a cached capture). Disc-gated `crates/engine-core/tests/ambient_mode4_scroll_disc.rs`.

Owning page (per-scene rect table):
[`field-ambient-fx.md`](../../subsystems/field-ambient-fx.md#the-vram-rect-scroller-render-mode-4).

### Which op-`0x34` sub-3 installs fire at scene entry

*Status:* resolved - decoded from the disassembly and ported

`FUN_8003A1E4`, the pre-run the placement spawn loop calls per just-spawned placement, carries its own
copy of the per-actor script runner's frame slice rather than calling `FUN_80039B7C`. An install fires
at entry exactly when it sits inside that slice.

- `0x8003A480`: `lbu` the first opcode, `addiu v0,v1,-0x24`, `sltiu v0,v0,0x2`, `beq v0,zero,<skip>` -
  unless the first byte is `0x24` or `0x25` the VM loop is skipped and the script never runs at load.
- `0x8003A498..0x8003A4F4`: run while `(opcode & 0x7F) >= 0x20`. After dispatching, `beq s1,s4`
  against `li s4,0x21` breaks the slice on the **raw** byte `0x21` (a cross-context `0xA1` does not
  break), as does an unchanged returned PC.
- `0x21` and `0x25` both disassemble as "nop" and only `0x21` ends the slice. A record written
  `25 / 34 30 00 / ...` fires its install in the load slice whatever follows, so a dialogue-bearing
  placed actor installs its ambient tree like a dedicated effect-actor script; an install placed after
  a `0x21` (`edkorout` P1[15]) does not fire at plain entry.

Port: the scene-entry prologue pre-run executes the slice itself, so each install fires once at its
context's position. The static census
`engine-core::man_field_scripts::scene_entry_ambient_installs` takes the **unconditional prefix** of
the slice for tests and the `.glb` export - a deliberate under-approximation, since a flag-gated
install deeper in a record (`nilboa` P1[3], `suimon` P1[4]) depends on runtime state. Disc-gated
coverage `crates/engine-core/tests/ambient_entry_install_census_disc.rs`.

Owning page:
[`field-ambient-fx.md`](../../subsystems/field-ambient-fx.md#which-installs-fire-at-scene-entry).

### Master ambient record 0 - the per-scene SFX descriptor bank

*Status:* resolved - the premise ("a stager record with unknown rows") was wrong

`0x8007B8D0` is a shared **current-bundle** pointer, and record 0 of the bundle it points at is the
runtime sound-effect descriptor bank for cue ids `>= 0x200`.

- The field asset loader points the slot at the scene prescript bundle (`0x8001F850..0x8001F864`:
  `lw v0,0xd8(s3)` with `s3 = 0x1F800314`, plus `0x12800`), which is how `FUN_800252EC` reaches stager
  records through it.
- `FUN_800250D4` (`0x800250FC..0x8002514C`): `desc = base + offsets[0] + (id - 0x200)*8`, then
  `SpuKeyOn`s `+3 & 0x1F` consecutive voices.
- `FUN_80016B6C` (`0x80016C24..0x80016CB0`), the cue-ring drain: same address arithmetic; it hands
  bytes `+0..+4` to the debug print `"setbl p:%d t:%d l:%d n:%d id:%d"`.
- `offsets[0]` is the same word `FUN_800252EC` reads for stager id 0.
- Disc shape: every populated row has category `+4 = 3` (a variable VAB slot), voice count 1-2, a
  level in the low 60s and a zero `+5..+7` trailer - the layout of the static table in
  [`sfx-table.md`](../../formats/sfx-table.md). Size is per scene: `jou` reserves 96 rows and
  populates 40, `rugi` carries 21. `jou`'s own tree cues `0x20B` and `0x20E..0x211` - rows 11 and
  14..17 of its record 0.
- The boot sound-bank loader `FUN_8001FA88` writes the same slot and immediately saves its bank's
  record-0 address at `gp+0x678`, because the next scene load overwrites the slot.

### Rim Elm's south gate

*Status:* resolved

The game's first scene exit is held shut by the collision grid, not by a trigger gate. The exit
record is ungated; grid row 47 walls `z in [5888, 5951]` across the doorway until a story-flag paint
opens it.

The gate's two `.MAP` kind-1 gate-1 bands:

| Record | Tiles | Script |
|---|---|---|
| `P2[10]` | `(24..26, 45)`, `(25, 44)` | `21 21 26 FE FF` - `Nop; Nop; JmpRel`-to-self. Five bytes, no scene change (inert). |
| `P2[0]` | `(24..26, 46)` | `CFlag.Set`, an `Effect` fade, `0x3F` naming `map01` at entry `(0x60, 0x19)`. `C1=[] C2=[]` (ungated). |

The wall is cut by `town01` `P0[20]`, the gate object's own record, bound by the gate-0 kind-1 trigger
at tile `(23, 43)` and executed by the scene-init bind prologue (`FUN_8003A55C`). It clears the
approach with three `4C 70` paints, then branches on system flags `327` / `321`:

| `327` | `321` | Paints | Gate |
|---|---|---|---|
| clear | - | none; the base row-47 wall stands | shut |
| set | clear | re-blocks rows 44..46, seats the gate at `(24, 44)` | shut |
| set | set | `4C 70 18 2D 19 2E` - cols `24..25`, rows `46..47` | **open** |

- A cold boot cannot leave Rim Elm in retail or in the port. On the port's loaded grid the three flag
  states give exactly the three collision states above, with col 26 re-blocked in the open one.
- An oracle that seats the player onto `(25, 46)` fires the exit; a player walking toward it is held
  by the wall, so the walk-on dispatch is not at fault.
- Carrier: `town0c` holds the paint sequence twice (entry script `P1[0]` and `P0[20]`); `town01` holds
  it only in `P0[20]`. Applying nibble-7 deltas from entry scripts alone leaves `town01`'s gate sealed
  in every story state.
- Pinned by `crates/engine-core/tests/south_gate_disc.rs`; the pad-driven exit is a rung of
  `crates/engine-shell/tests/critical_path_replay.rs`.

Owning page:
[`script-vm.md`](../../subsystems/script-vm.md#rim-elms-gate-paints-are-carried-by-an-object-record-not-an-entry-script).

### Town/field free-movement locomotion

*Status:* resolved

The player free-movement controller is `FUN_801d01b0` (field overlay 0897), pinned by a runtime
write-watchpoint on `*(0x8007c364) + 0x14/0x18` (`autorun_player_pos_watch.lua`). It camera-remaps the
held pad, computes a per-frame speed, and steps the player 2 units at a time with per-axis collision.
The `801db81c..801dbf9c` cluster is the field *camera* system, not movement.

- **Controller.** Pad remap `func_0x800467e8` + `FUN_80046494` -> direction bits `& 0xf000`; speed
  `base_step * player[+0x72] >> 12 * DAT_1f800393` with terrain-slow and diagonal modifiers; facing in
  `player[+0x26]`.
- **Wall collision, `FUN_801cfe4c`** (overlay `0897` @ `0x801CE818`, bias table `DAT_801f2214`): three
  leading-edge footprint probes (48 units ahead in the positive directions, 47 in the negative, ±16
  lateral), each sub-cell derived as `zc = (z>>6)+2`, `xc = ((x+0x3f)>>6)-1`. The `+2` Z bias is
  authored into the wall bits; the floor sampler `FUN_80019278` reads the same bytes with plain floor
  indexing. Pinned by the `rimelm_wall_press_down` (step-exact 47-unit standoff) and
  `rimelm_wall_press_left` captures.
- **Actor collision, `FUN_801cfc40`** (bits `1` / `4`): walks the active-actor table `DAT_801c93c8`,
  box-testing the three `DAT_801f21b4` probe points (64 / 63 ahead, ±32 lateral). A static entity
  anchors at its MAN object record (`tile*128 + sub*16`, half-extent `0x40+0x10`, footprint offset
  including the `+0x52 & 8` correction from record flag bit `0x8`, verified against four captures'
  spawned static actors); a moving actor uses its live
  position with caller extents (±40). Each 2-unit step is gated on the actor bits and the wall bit
  together.
- **Village NPCs take the moving-actor arm** (bit `1`, ±40 box). In `rimelm_npc_press_tetsu` the mutual
  `+0x98` collision link is active both ways and the NPC's `flags+0x10 = 0x08020884` carries the
  `0x20000` bit.
- **Touch / interact dispatch, `FUN_801d5b5c`** (decoded from a live overlay image; the static 0897
  copy is garbled at that VA): sets the player engaged flag `0x80000` and the actor touched mark
  `0x100`, bumps the counters, saves facing to `+0x5A`, and kicks the `FUN_8003c9ac` NPC-motion pause.
  It fires per contact step for static props (bit `4`), and for NPCs on the just-pressed interact
  button through the probe table `DAT_801f2254` (overlay file `0x23A3C`: a radius-64 compass point per
  45-degree facing sector, extents `0x20` -> ±72 NPC box) with a face-the-NPC turn
  (`func_0x80019b28`).
- **Interaction end.** The dialog SM `FUN_80039b7c` exit path restores the actor facing from `+0x5A`,
  drains the `+0x2A` / `+0xA` touch-counter pair, and clears the player's `0x80000` flag and
  `ctrl+0x60` when no interactions remain.

**Port.**

- Walls: `World::field_tile_is_wall` (retail's derivation; `sample_field_floor_height` keeps floor
  indexing) and `World::field_dir_blocked` (the three-probe footprint), gated by
  `World::locomotion.leading_edge_wall_probes` - on by default in `play-window`, `--no-edge-collision`
  clears it; the centre test stays the `World` default for the oracles and nav drivers. The engine
  stepper reproduces both retail rest positions byte-exactly, also through a real `enter_field_live`
  scene entry.
- Actors: `World::field_actor_dir_blocked` over `World::npcs.positions`, gated by `World::npcs.solid`
  (`--no-solid-npcs` clears it); props via `Scene::field_object_placements` collider centres
  (`field_prop_colliders_live.rs`); interact probe `World::field_interact_probe_slot`.
- NPC motion: `man_field_scripts::placement_motion_route` decodes each placement's pre-text
  `0x4C 0x51` move-to-tile waypoints and `World::tick_field_npc_motions` drives them through the ported
  motion VM (`FUN_8003774C`), writing live positions back. Autonomous patrol is gated by
  `World::npcs.animate` (`--no-live-npcs` clears it); an interaction prologue's `0x4C 0x51` walks the
  interacted NPC regardless.
- Cutscene cross-context walks: a partition-2 record's targeted `0x47` yield
  (`C7 <id> <tx> <tz> <mode>`) parks the record on `CutsceneTimeline::walk_wait` and glides the target
  (NPC channel or the `0xF8` player anchor) to the tile at the op's own speed; the paired
  `A2 <id> <move_id>` ExecMove surfaces the walk / idle clip cue (the `town01` Mei walk-in; see
  [script-vm.md](../../subsystems/script-vm.md) § yield family).
- Walk-touch: `placement_walk_touch_event` classifies `0x3E` door warps and cross-context
  player-channel `0x23` teleports; `World::check_field_walk_touch` posts once per ±80-box contact
  through `trigger_field_interact` and applies the effect.
- Not modelled one-to-one: the full `FUN_801d5b5c` post-kernel state (engaged flag, facing
  save / restore, touch counters), per-actor field-VM channel execution for yield-paced patrol scripts
  (the engine loops the decoded waypoints), the exact retail NPC glide speed, and prop scripts beyond
  the two decoded walk-touch classes.

Tests: `engine-shell/tests/field_collision_discriminator.rs` (probe-model + engine-rest legs,
including `npc_press_pins_moving_actor_arm`); `field_npc_motion_disc.rs` / `field_walk_touch_disc.rs`;
unit equivalence `world.rs::tests::field_tile_is_wall_matches_retail_subcell_derivation`; standoff
`leading_edge_wall_probes_rest_at_retail_standoff`. Both wall-press sessions park in `town0c`, whose
`.MAP` (PROT 0019) is byte-identical to `town01`'s.

Owning page: [`field-locomotion.md`](../../subsystems/field-locomotion.md).

### Field collision-map source

*Status:* resolved

The collision grid at `*(_DAT_1f8003ec) + 0x4000` (1 byte per 128-unit tile) arrives whole from the
scene's `.MAP` - the live grid byte-matches PROT 0109 with zero diffs - and the field VM paints
story-conditional **deltas** over it.

- **Paint op:** `0x4C` outer-nibble 7 (`op0` in `0x70..0x7F`), inline operands
  `[4C, 0x7s, col0, row0, col1, row1, mask]`; sub-op = clear-walkable / block-all / clear-mask /
  set-mask. 6 bytes for subs 0 / 1, 7 for subs 2 / 3.
- The handler `0x801e1c64` is entry `[7]` of the jump table at `0x801CEE60` - an intra-function
  label, not a function.
- **High nibble** = 4 sub-cell wall bits. **Low nibble** = floor-elevation tier, a 4-bit index into a
  16-entry `short` height LUT at scratchpad `0x1f80035c`, filled at scene entry by `FUN_8003aeb0` from
  the MAN header (`_DAT_8007b898+2`, 16 negated values) and consumed by the object spawn iterator
  `FUN_8003a55c` to offset each placed object's Y.
- **`+0x8000`** is a per-tile `u16` object / attribute map, not a terrain-flag grid: low 9 bits =
  object-record index into the `+0x0000` table; bit `0x400` = footprint flag ORed in by `FUN_8003aeb0`.
- Nothing zero-initialises the grid; there is no `+0x4000` init site to find.
- `town01` runs at game mode `0x03`, the same as the runtime-pinned field `map03`.

Owning page:
[`field-locomotion.md`](../../subsystems/field-locomotion.md#where-the-collision-grid-comes-from).

### Field `.MAP` PROT resolution - `define − 2`, universal

*Status:* resolved (census-pinned; engine resolver corrected)

A scene's field `.MAP` is its retail block's **first entry** - extraction index `define - 2`, because
CDNAME defines are raw-TOC indices shifted `+2` from the extraction frame
([cdname.md](../../formats/cdname.md#numbering-space)) - for **every** field scene. `Scene::load`
converts windows to the retail frame, so `Scene::field_map_index` is the block's first entry and
`walk_field_map_index` is an alias.

- **Census** (`crates/engine-shell/examples/field_grid_census.rs`): each save's live field buffer
  (scratchpad `_DAT_1f8003ec` -> `+0x4000` grid) classified against candidate on-disc bases. `keikoku`
  sessions match PROT 0109 (`define 111 - 2`) with zero diffs while 0118 differs by 3855 bytes; `koin3`
  matches 0559 exactly (0568 differs by 531); `town01` sessions match 0010, which equals 0001.
- **The in-block decoy.** In an unshifted window the first `0x12000` entry is the *next* scene's map;
  corpus-wide, every block's in-block `0x12000` hit is exactly the next block's `define - 2` entry.
  The per-entry extractor's shifted filename labels attribute a map to the previous block's tail.
- **Object-index grid** (`+0x8000`, the `Scene::field_object_placements` / `field_terrain_tiles`
  source): residuals of 0..96 bytes against the resolved entry across `town01` / `town0c` / `keikoku` /
  `koin3` sessions (story-conditional cell mutations - opened chests, prescript object toggles),
  thousands against every other candidate. Guarded by the disc + save-library gated
  `engine-shell/tests/field_map_object_grid_live.rs`.
- `town0c`'s `.MAP` is PROT 0019, byte-identical to `town01`'s (0001 / 0010). PROT 0028 is `izumi`'s
  (`define 30 - 2`), not `town0c`'s.
- **The footprint is corroboration, never the resolver.** 111 entries are exactly `0x12000` bytes and
  only 101 are maps. Five of the ten others sit inside named scene blocks (`dolk+5`, `dolk2+5`,
  `taiku+9`, `taiku+10`, `rugi+7`) and are `scene_tmd_stream` entries (`[u32 size]` then the
  `0x80000002` TMD magic), so a footprint scan within a block can land on a mesh stream.

Owning page:
[`field-map.md`](../../formats/field-map.md#the-footprint-is-necessary-not-sufficient).

### game_mode 0x03 = field/town gameplay

*Status:* resolved

`_DAT_8007B83C = 0x03` is the in-town / on-field gameplay mode, and the in-field pause menu runs
under mode `0x17` (23, the CARD pair).

- **Captures.** `v0_1_pre_battle_tetsu` (Vahn walking in Rim Elm / `town01`) and the runtime-pinned
  free-movement controller on `map03` both hold `0x03`. All six menu-open library captures
  (equipment / status / options, field `map01` + town `town01`) hold `0x17`.
- **Handler map.** The index -> handler / param / name map is read off the disc by
  [`legaia_asset::mode_table`](../../../crates/game-tables/src/mode_table.rs) (`asset mode-table`;
  disc-gated `mode_table_real`). Field / town is modes 2 / 3 MAIN; `MAPDSIP` (12 / 13) is the
  **world-map display** mode, not the field. 12 of the 14 per-frame modes share the generic per-frame
  handler `0x80025EEC`; only mode 13 (world map) and mode 23 (memory card) carry their own.
- **Next-mode field.** The retail `+0x0A` field is `ModeEntry::next_mode`: `-1` = self-managed, `0` =
  fall back to mode 0. The word `0xFFFF0000` is `-1` over a zero low half, not a sentinel.
- **Port.** `engine_core::mode::GameMode::scene_mode()` maps `MainMode (3) -> SceneMode::Field`;
  `engine_core::mode` holds `SceneMode::Field` for both modes 2 / 3 (an init mode holds its
  successor's scene mode, as the Mapdisp / Battle / Str pairs do) and maps the CARD pair to
  `SceneMode::Menu`. `BootSession` hosts the field-menu session headlessly (`open_field_menu` / the
  Start-edge path in `tick`), and the windowed host layers its sub-session UI on the same session.
- **Oracles.** `mode_trace_e3` + `v0_1_playthrough` drive the engine into the field
  (`enter_field_live`) and converge on the retail `0x03` snapshot; `mode_trace_e3` drives menu
  scenarios with a scripted Start press and asserts scene mode, active scene and the engine-emitted
  `game_mode = 0x17`. The table's name / param / next fields are cross-checked against the disc map by
  the disc-gated `mode_table_reconcile`.

Owning page: [`boot.md`](../../subsystems/boot.md#full-handler-map-recovered-from-the-disc).

### Engine VRAM byte-exactness for town01

*Status:* resolved (major source); minor residue

Single-snapshot byte-exact VRAM is not achievable: about 40% of the texpage band is dynamic or
residual (two `town01` captures disagree on about 40%). The oracle `vram_oracle_e1` compares the
**static mask** - words stable across same-scene captures, excluding the runtime NPC / character CLUT
band - and with the field pre-pass doing DMA-every-TIM (`BuildOptions.upload_all_tims`) `town01`
passes byte-exact on every static pixel it uploads.

- **Extraction-0874 §2 TIMs** (retail `player_data` / `player.lzs` §2, the field-character texture
  band; `etim.dat` is extraction 0870, a different file): 4bpp pages at `fb(320/384,256)` etc.,
  field-resident, pixel-matched 256 rows byte-exact. The engine uploads them at field entry
  (`scene::upload_effect_textures_into_vram`) - image pages only, since retail uploads their CLUTs at
  battle entry.
- **The menu-glyph atlas (`PROT.DAT[0x11218]`) is boot-resident**: its image page and flat-strip CLUT
  match the disc bytes in every captured phase, title included. The `(960,400)` 60x24 rect belongs to
  the **next bundle TIM** (`PROT.DAT[0x19438]`), which retail uploads after the atlas and which
  overlays that part of the atlas image. Uploading the whole system-UI bundle in on-disc order
  reproduces the retail band; see
  [CLUT row 510 population](#clut-row-510-population-boot-resident-system-ui-strip-band).
- **Residue,** `x=896..1024, y=256` (about 12k words): (a) the boot-resident system-UI band (the
  `(960,256)` atlas page + its overlay TIMs; static disc bytes), and (b) the character / party-texture
  region uploaded by the battle / character targeted-CLUT pass, which the field pre-pass excludes by
  design, plus about 2.5k words of UI residue.

"Stable across same-scene captures" is not the same as static in two capture-pinned cases:

- The extraction-0874 §2 texture band is **global, history-dependent** state. Row 271 holds a 3-word
  F-variant in some lineages (`(853,271)`: `0xFFFF` words) and the disc bytes (`0x3333`) in others, and
  the first battle effect use restores the disc bytes - see
  [the F-variant stamp](#extraction-0874-2-playerlzs-f-variant-pixels---a-one-shot-opening-face-frame-stamp-not-a-menu-writer).
  The oracle demands cross-scene staticity inside `scene::effect_texture_image_rects`.
- The world-map walk view **palette-cycles** specific columns of the kingdom terrain CLUT rows
  506 / 508 / 509 in place. `vram_oracle::WORLD_MAP_CLUT_CYCLE_CELLS` excludes exactly those columns for
  world-map scenes; row 507 and the static columns of 506 / 508 / 509 are asserted.


### World-map CLUT cycling beyond the ocean head - CLOSED (operand table + emitter + cadence all pinned)

*Status:* closed. The head-walk operands are a literal disc table - **kingdom-bundle slot 5** (type byte `0x06`), a 516-byte 8-entry CLUT-walk animation table byte-identical across all three kingdoms; the emitter is the SCUS actor walker, not the script-driven CLUT-cell family; the cadence is the table's own per-frame hold bytes.

The chain, each link byte-verified against live RAM and the disc:

| Step | Routine | What it does |
|---|---|---|
| Load | `FUN_8001F05C` case 6 | Sets `DAT_8007B7C8` to the decoded slot-5 table. |
| Spawn | field-init `FUN_801D6704` | One render-mode-`0xB` actor per entry via `FUN_80024CFC`; entry pointer at `actor+0x4C`, accumulator `actor+0x68` seeded `100` so the first copy fires at scene entry. |
| Emit | `FUN_8001ADA4` case `0xB` | `acc += DAT_1F800393`; on `acc >= frame.hold` issues a 16x1 `MoveImage` from the frame's source cell to the entry's destination cell, resets `acc = 0`, advances the frame index. |

- **Cadence** (PCSX-Redux `MoveImage` exec-BP traces on all three kingdoms): intervals are constant at
  `ceil(hold/dt)*dt` vsyncs - hold 8 -> 9, hold 10 -> 12, hold 20 -> 21 at overworld `dt = 3`. The
  non-multiples rule out subtract-remainder semantics.
- All eight entries fire their first frame on the same vsync at world-map entry, then free-run
  independent phases with zero drift.
- The 18-step head cycle is `A,B,f0..f7,(f6,f7)x2,f8..f11`: two extra wave frames parked before the
  `OCEAN_ANIM_FRAME0_HEAD` signature in kingdom slot 0; ocean frame 12 is never shown.
- The field overlay's script-driven CLUT-cell family (`FUN_801E4C58` / `FUN_801E4794`) is not the
  emitter. It carries only the **row-498 park one-shots / fades** (`map01`'s eight `4C 61` ops;
  `scene_clut_cell_fx`, disc-gated `map01_clut_fx_disc`). At overworld idle row 498 is a *source*
  strip for the `(32,508)` / `(48,500)` walkers; the `map01`-only row-508 "mirror" is slot-5 entry 6
  copying from the script-parked row-498 cells.
- Row-506 cols 32..47 are written wholesale by slot-5 entries 3 / 4 from the row-503 / 502 strips -
  parked disc bytes walked in place, not runtime colour math.
- Destination cells are literal u16s in the table, not `park row + 8`.
- **Port.** The engine consumes the table directly (`WaterAnim::Walk` in `play-window`;
  `vram_oracle::WORLD_MAP_CLUT_CYCLE_CELLS` = the slot-5 destination fold). The park strips are raw
  CLUT-block records, not TIMs, so the scene pre-pass does not upload them; the `map02` / `map03`
  bundles ship only rows `{501, 503, 505}` and retail relies on VRAM residency from the `map01`
  upload, which the engine mirrors by parking the byte-identical Drake complement.

Owning page: [`world-map.md`](../../subsystems/world-map.md) "Ocean animation"; parser
`legaia_asset::clut_walk`.

### `init_data` UI-tile pages - journey-dependent residency (resolved); map03 texture column (resolved - "not uploaded" premise falsified)

*Status:* the keikoku oracle drift is resolved (residency class pinned); the map03 texture divergence is resolved - the "engine fails to upload PROT 0392" premise is **falsified**, the current pre-pass does write the real terrain

`init_data` (PROT 0) carries two 64-word x 256 UI-tile TIMs at fb `(704, 0)` / `(704, 256)`, and
those rects are **journey-dependent residency**, not stable shared texture.

- Overworld transit leaves kingdom-bundle content over parts of the rect: every Drake-stage capture
  (`keikoku`, the field-menu states) holds the same kingdom bytes at `(704, 256)` where the boot-fresh
  `town01` states hold the disc tiles.
- Town scenes mask this because their own scene TIM overwrites the slot; `keikoku` carries none.
- The parity oracle pools captures across all scenes against
  `scene::block_image_rects(index, "init_data")`, the same cross-scene treatment as the befect band.

The `map03` terrain rect is `map03`'s own content, and the engine pre-pass writes it.

- `asset tim-scan`: PROT 0392 uploads 8 real 4bpp TIMs into fb `x=576..640, y=320..448`. The
  `fbx=576 fby=320` 96x96 4bpp TIM (PROT 0392, `lzs0_off 0x03BDEC`) byte-matches the retail resident
  VRAM at `(576,320)`, 2304 / 2304 halfwords.
- Direct pre-pass measurement: `map03` uploads 58 TIMs, and the `576..640 x 320..448` region holds 7945
  real terrain texels with 37 stray `0x3332` cells scattered in-tile - no hole.
- PROT 0392 slot 0 is byte-identical to 0391 slot 0, which the engine uploads (the kingdom
  sibling-skip in `crates/engine-core/src/scene_resources.rs`), so uploading 0392 as well would write
  identical bytes to identical cells.
- Limit of the evidence: the comparison is a pre-pass measurement, not a full VRAM oracle, because the
  capture corpus has no `map03`-WorldMap-resident save.

### CLUT row 510 population (boot-resident system-UI strip band)

*Status:* resolved (source + upload semantics + retail residency pinned; engine pre-pass uploads the bundle - `legaia_asset::system_ui_bundle`); residue = the exact boot-time walker call site only

CLUT rows 510 / 511 are the **flat-strip CLUT band of the boot-resident system-UI TIM bundle**, so the
prims that sample them are validly textured in retail. The prims in question: `town01` env-pack slots
21 / 26 / 74 and `rikuroa` slots 50 / 51 / 63, CBA `(64, 510)`, texpage `(960, 256)` 4bpp; no scene
TIM uploads row 510.

- **Source.** The `prot::timpack` at **raw PROT TOC entry 0** (LBA words `toc[0]=3` / `toc[1]=55`
  precede `init_data`'s 121 - indexed, just below the extraction space; CDNAME's
  `#define init_data 0` names this block). A second single-TIM pack sits at raw entry 1.
- **Upload semantics.** The per-TIM uploader `FUN_800198E0` uploads every TIM CLUT block as a
  `w*h x 1` strip at the declared origin (`ghidra/scripts/funcs/800198e0.txt`). The atlas at
  `PROT.DAT[0x11218]` (declared CLUT `(0,510,16,16)`, image `(960,256)` 64x256) lands as the 256-entry
  strip on row 510 `x=0..255`; the `0x19438` UI-strip TIM adds `x=256..319`; three more bundle TIMs
  tile row 511 `x=0..319`.
- **Residency census.** Across mednafen library states in every phase - `title_screen_new_game`,
  `new_game_cutscene_intro_a`, `v0_1_pre_battle_tetsu`, `keikoku_chest_pre`, `mei_house_inside`,
  `sebucus_overworld_resident`, `v0_1_battle_start_tetsu` - the row-510 / 511 strips are byte-identical
  to the on-disc CLUT data (256/256 + 64/64 + 256/256 + 48/48 + 16/16 halfwords per strip), and the
  `(960,256)` image page matches the disc TIM on every row not covered by a later bundle member.
- **Row patches.** Compositing the bundle in on-disc order (images at declared rects, CLUTs as
  strips) reproduces the whole retail `(960, 256..511)` band. The six 64-word rows at
  `y=456..458` / `460..462` are bare row-patch members of the same pack: raw-entry-0 members 10..15 at
  `PROT.DAT 0x1A018..0x1AA7C`, a `[u32, u32]` preamble + TIM-style `[u32 bnum][u16 x,y,w,h]` block
  declaring `(960, y, 256, 1)`, byte-exact against live captures (`RowPatch` in
  `legaia_asset::system_ui_bundle`).
- **What the prims sample.** CBA `(64,510)` = atlas strip entries 64..79; UVs (u `0..2`, v `240..242`)
  hit a constant mid-grey texel patch - a flat material through the textured pipeline.
- **Not** scene-loaded or a runtime targeted upload (it is resident before the title screen), and not
  a CBA misread: `x=(cba&0x3F)*16, y=(cba>>6)&0x1FF` is correct.
- **Unpinned:** which boot routine issues the `byindex`-style read of raw TOC entries 0 / 1 and walks
  the pack into `FUN_800198E0`. A cold-boot write-watch on the row-510 upload
  (`scripts/pcsx-redux/autorun_town01_vram_upload_census.lua`) would name it.

Owning page:
[`npc-palette.md`](../../formats/npc-palette.md#boot-resident-strip-band-rows-510511).

### Scene-transition (`0x3F` door) destination indexing

*Status:* resolved

A field scene reaches another through the field-VM **`0x3F` named-scene-change** op, which carries the
destination scene name inline and is selected by partition-2 slot, never by absolute reference.

- **Dispatch trace** (`autorun_door_dispatch_trace.lua` on `drake_castle_to_worldmap`): the `0x3F` ops
  are partition-2 MAN records reached through the partition-2 record-offset table. The controller sets
  the VM bytecode base to `man_base + data_region + partition2[slot]` and runs the record by
  fall-through (`a0 - man_base == data_region + partition2[0]` exactly).
- The op's `index` field is only the destination-scene id passed to the warp packet (`FUN_8001FD44`).
- **Census** (clean partition walk): 160 destination ops in 48 scenes, 153 in partition 2, zero
  absolute-reference ops at or after any destination op.
- **Consequence.** Variable-length door editing is safe: resizing a destination name is a
  partition-table + section-offset + intra-record-jump-delta + descriptor-size fixup, implemented in
  `legaia_asset::man_edit` and shipped as the door randomizer
  ([`man-relocation.md`](../../formats/man-relocation.md)).
- **The `0x3E` door warp** (7-id `map_id`) is SCUS-resident end to end: `FUN_80025980` (mode-24 OTHER
  INIT entry), `FUN_80026018` (exit). It carries no destination name; the sub-id selects a minigame
  overlay (extraction PROT 972..977, 980 via the loader math `param + 0x37F`). Its name handling is a
  backup / restore of the *current* scene name (`0x80084548` <-> `0x8007BAE8`, plus `_DAT_80084540` <->
  `0x8007BAC4`) so the exit re-enters mode 2 on the original scene.

Owning page:
[`script-vm.md § 0x3E warp`](../../subsystems/script-vm.md#0x3e-warp-mode-24-minigame-door-warp).


### Intra-town (house / interior) door mechanism

*Status:* resolved

Entering a house is not a scene change. It is an **intra-scene reposition**: the scene-name buffers
`0x8007050C` / `0x80084548` stay put and only the player's position jumps to an interior sub-area of
the same loaded scene.

- **Writer.** Field-VM dispatcher `FUN_801de840` `case 0x23` (`0x801debc4 sh v0,0x14(s5)`), converting
  the tile operand to world as `tile*128 + 0x40`. Pinned by the `probe.step.find_writer` Lua primitive,
  a width-correct range write-watch over the player position block. A width-2 watch at `+0x14` catches
  only a 2-byte no-op re-store in the ledge-hop `FUN_801d1878`.
- **Captures.** `door_warp_rim_elm_to_mei_house` / `mei_house_inside` (mednafen),
  `mei_house_door_pcsx` / `mei_house_inside_pcsx` (PCSX).
- **Door marker.** House-door warps use the cross-context form `0xA3 0xF8 xb zb`: opcode
  `0x23 | 0x80` dispatched into the player system channel `0xF8`. Plain `0x23` moves the executing
  actor (NPC / prop positioning).
- **Carrier records.** Partition-0 records with their own header form
  (`[u8 n][n x 2 SJIS name][u8 attr]`, distinct from partition 1) and a naming convention pairing
  entries with exits: fullwidth `ＩＮ` / `ＯＵＴ`, `入口` / `出口` gates, `Ａ` / `Ｂ` elevator
  endpoints, optional digit suffixes. The Mei's-house warp is `0xA3 0xF8 0x61 0x36` in `town01`
  partition-0 record 34, an `ＩＮ` record.
- **Three player-move forms.** `A3 F8 <xb> <zb>` (op `0x23`, instant),
  `CC F8 51 <xb> <zb> <depth> <mv>` (op `0x4C` nibble-5 sub-1, teleport + move anim), and
  `C7 F8 <xb> <zb> <mode>` (op `0x47`, animated walk). A door record is a branching script whose arm is
  selected by story flags, so a door can also be a `0x44` SPAWN_RECORD of a partition-2 choreography
  that does the seating itself.
- The bind position is the `.MAP` **object's** contact box, not the trigger tile, which is a lookup
  key and usually a wall.
- **Kind-0 tile doors.** The `.MAP` trigger block's kind-0 sub-table is a second, larger door class:
  `[tile_x][tile_z][dest_x][dest_z]`, no object and no script. Crossing the tile seats the player at
  `(dest_x*64 + 64, (dest_z + 1)*64)` (`FUN_801D1EC4`'s kind-0 arm at `0x801d21c0`). 2330 records across
  73 scenes; most house *exits* are these. Vahn's house has an `ＩＮ` record and no `ＯＵＴ` because
  its exit is the kind-0 tile `(97,9)` inside the room, ungated by any story flag.
- **Randomizer.** `legaia_patcher::house_door` shuffles only the classified door warps,
  class-preserving (`ＩＮ` among `ＩＮ`, `ＯＵＴ` among `ＯＵＴ`), so every exit still lands outside
  ([`randomizer.md`](../../tooling/randomizer.md)).

Owning page:
[`field-locomotion.md`](../../subsystems/field-locomotion.md#intra-scene-doorways---the-walk-touch-teleport-family).


### Field/town environment-geometry placement

*Status:* resolved (renders)

A town's environment meshes (terrain, buildings, props) are object-local Legaia TMDs in the LZS
streams of the scene's `scene_asset_table` PROT entry (`town01` = entry 4), placed by `FUN_8003a55c`
from the field `.MAP`.

- **Placement.** The object-index grid at `+0x8000` (`cell & 0x1FF` = object id) selects a `0x20`-byte
  record in the `+0x0000` table. Placed tiles (record `+0x12` bit `0x4`) give the world transform:
  `world_y = -floorHeightLUT[nibble] + y_off`, the LUT being 16 `s16` at the MAN header `+0x02`.
- **Mesh per object** is the record's `+0x10`, for **every** object id (retail `FUN_80020f88`,
  `actor+0x64 = record[+0x10] + prefix`). Ids `1` / `2` / `3` are protagonist / NPC meshes from the
  shared pool; `anim_id` only animates.
- **Validation.** A live `town01` save (Vahn's house id `137` -> mesh 36), and the retail GPU prim
  pool: `town0c` cell `(30, 17)` (id `99`, record `+0x10 = 2`) draws its surface from env mesh **2** -
  the quad's `cba` / `tsb` / UVs match that mesh's primitive byte-for-byte - not from mesh `94`.
- **Not** a positional "field-actor band" (`obj_idx - 5`, ids `93..=118`): that rule swaps ten town
  meshes per Rim Elm map, drops the terrain slab south-east of the spawn and leaves a clear-colour
  hole in the ground.
- **Port.** Parser `legaia_asset::field_objects`; `Scene::field_object_placements`; `play-window`
  renders the town via `resolve_field_placement_draws`.

All 46 `town01` placements draw, on two pipelines (disc-gated
`field_object_placement_disc::town01_dropped_placements_split_untextured_vs_missing_clut`):

- **45 on the textured VRAM path.** That includes pack `74` / obj `347` (6 placements, 4 textured
  prims), whose CBA row 510 / texpage `(960,256)` source is the boot-resident system-UI bundle layered
  under the field build (`BuildOptions::system_ui`); no placement drops for a missing CLUT. Object
  `114` resolves through `+0x10` to the textured pack `84`.
- **1 on the vertex-colour path:** pack `31` / obj `315`, 30 untextured (per-vertex-RGB) prims. The
  textured-only builder `tmd_to_vram_mesh_filtered` skips prims with no UVs, so
  `legaia_tmd::legaia_prims` decodes the colour blocks into `Prim::colors`,
  `legaia_tmd::mesh::tmd_to_color_mesh` builds a `ColorMesh`, and `engine-render` draws it through a
  dedicated pipeline (`upload_color_mesh` / `Scene::color_draws`). Pack `109` (12 untextured prims)
  builds a colour mesh the same way.
- **Mixed meshes** render both halves: the colour mesh is built unconditionally and is disjoint from
  the VRAM mesh (`tmd_to_color_mesh` skips textured groups).
- A textured prim whose CLUT is not resident is dropped rather than drawn flat with `CLUT[0]`; a
  per-vertex-RGB fallback would render it wrong.
- Untextured colour block (per-mode record layouts F4 / G3 / G4, the `00 01 03 02` quad winding remap,
  no per-prim normal):
  [`tmd.md` § Per-prim color / texture block](../../formats/tmd.md#per-prim-color--texture-block).

Owning page:
[`field-locomotion.md`](../../subsystems/field-locomotion.md#object-record-format-0x0000-0x20-byte-stride).

### Region story-flag gate families

*Status:* resolved as structure across the chapter-2/3 regions. Play order is capture-confirmed for `retona`, `dohaty`, `taiku`, the Sebucus spine, `korb3`, the `kor5` chain head and the `map03` hub latch; in-bracket order is pinned by disassembly for `rayman`, `bubu2`, retock and deroa's `0x46D`; the two orders no gate decides are unobservable (see [the play-order residual](#the-play-order-residual)).

Every field scene's MAN carries one **partition-2 record** per cutscene or story beat. Each record's
*header* holds two flag lists the spawn evaluator `FUN_8003BDE0` checks before running it:

- **C1**, one-shot: the record is suppressed once any listed flag is set.
- **C2**, requires-all: the record spawns only when every listed flag is set.

Regional progression is expressed almost entirely through these header gates. They are not inline
`0x50` / `0x60` / `0x70` opcodes, so the inline flag census (`man-scripts --system-flag-census`) cannot
see them, and a flag read only by a header gate looks write-only there.
`legaia_engine_core::man_field_scripts::partition2_record_gates` decodes them; the anchor tests named
below pin each region's exact lists.

**Self-latches.** A record that both sets a flag and lists it as its own C1 gate.

- `0x1BE` (Jeremi's arrival): `geremi P2[0]` (anchor `geremi_p2_0_is_the_0x1be_self_latch`).
- `549` / `0x225` (the Rim Elm opening): the same shape across the Rim Elm variants, readable once the
  `4C 0xE_` op widths are decoded.

**Chapter 2 - Sebucus (`map02` and its dungeon spokes).** The spine needs no chapter-specific engine
code: each beat's script latches its flag through the ordinary field-VM `SysFlag.Set` path, so the
generic seeder drives the arc. Chain: `teien` (`0x1C8` -> `0x1C9` -> `0x332`) into `tower` (`0x1C7`,
gated on the teien arc) into a post-tower `geremi` beat, with `balden` self-latching `0x5B3` and
`map02 P2[9]` mirroring the teien arc onto the overworld. Proven by `chapter2_sebucus_spine_oracle`,
`chapter2_sebucus_gate_spine` and `chapter2_sebucus_hub_sweep_disc` (which drives the arc through real
`0x3F` scene transitions).

| Spoke | Family | Anchor |
|---|---|---|
| `taiku` / `doman` / `rayman` | Self-latch pairs plus the linear `rayman` chain `0x201` -> `0x1FB` -> `0x200` -> `0x1FC`. `rayman2` is the same MAN with a shared C1 on the low flag `0x7`, a variant discriminator. | `chapter2_dungeon_gate_families` |
| `balden` / `balden2` / `station` | `balden` is an arc around its reached-flag `0x1D5`; `balden2` is a sibling carrier with an identical gate family, selected by streaming slot rather than a flag. `balden` gates on the `ropeway2` switches; `station` / `station3` gate on `taiku`'s `0x38F`. | `chapter2_balden_station_gate_families` |
| `ropeway` / `ropeway2` / `jiji` | `ropeway2` hosts a four-bit switch puzzle (`0x3FF`-`0x402`); see below. | `chapter2_ropeway_jiji_gate_families` |
| `retona` | Five-step ladder `0x353` -> `0x354` / `0x355` -> `0x356` -> `0x357`; see below. | - |
| `dohaty` / `retock` / `retockin` / `stone` | `dohaty` opens with a six-record `0xF` first-visit group; `retock` depends cross-scene on `balden`'s `0x1D5` and gates on retona's `0x357` before its own `0x502`; `retockin` is the `0x7`-gated interior variant sharing `0x502` / `0x357`; `stone` is a single one-shot. | `chapter2_dohaty_retock_stone_gate_families` |
| `tunnelb` / `tunnelc` | Small internal one-shots, read back only by the tunnels themselves. | - |
| `map02` hub | A router: two gated records, both overworld mirrors of a dungeon-arc completion. | `chapter2_map02_hub_gate_family` |

Sebucus detail:

- **`rayman` streaming variant** adds a `P2[18..20]` tail latching `0x34D` / `0x34C` (`P2[18]` body
  `+0x2C2`, at a `JmpRel` branch arm after `0x1FE` / `0x1FF` tests).
- **`taiku` variant `P2[16]`** SETs the pair `0x380` + `0x382` at its head (body `+0x11` / `+0x21`,
  between `SceneFade` and the particle emitters). `0x382` is a **cross-chapter gate**: `son P1[14]`
  branches its NPC dialogue on it (body `+0x4A`), and the clean census reads span `doman(V)` /
  `retockin` / `ropeway` / `ropeway2` / `map03` / `koin2` / `korout`.
- **`ropeway2`** payoff records `P2[31..=34]` are C2-gated on all four switches plus the `0x359`
  commit - an internal consumer the inline census cannot see.
- **`jiji P2[8]`** latches `0x304` from three branch arms of one cutscene (each `4C CD` -> `Set` ->
  `JmpRel` to the shared tail; bodies `+0x912` / `+0xCD6` / ..).
- **`retona` records.** `P2[8..14]` gate on `0x353` / `0x354` / `0x356`; `P2[15]` chains
  C2=`0x354` / C1=`0x355`; `P2[17]` (C1=`0x357`, C2=`0x356`) is the pre-beat rendition; `P2[18]`
  (C2=`0x356`) is the beat that SETs `0x357` (body `+0x5EF`, after the `4C 73` tile run + BGM cue).
  `P2[10]` separately latches `0x354` (`+0x673`), read by `rugi`.
- **`retona` backstop.** The entry script `P1[0]` normalises: `Test 0x357` -> skip; `Test 0x3AD` ->
  `Set 0x357` at `+0xF4`. `0x3AD` is also the C2 of `map02 P2[10]`, the overworld mirror `0x357`
  retires.
- **`0x357` is the Jeremi-arc cross-scene gate** - clean reads in `retock` / `retockin` / `map02` /
  `geremi` / `edretoin` - so the `0x357` half of retock's `0x357 -> 0x502` chain is retona's output.
- **`stone`** partition-0 walk-on scripts latch a local band: `P0[2]` -> `0x32B`, `P0[3]` -> `0x32A`,
  `P0[4]` -> `0x32D`, `P0[5]` -> `0x32C` (`+0xB7F`, then `SpawnRecord 0x1E`).
- **`0x32C` is a write-only latch; no reader exists.** Every census read (about 50 scenes) is the
  ASCII `s,` bigram in dialogue, and no C1 / C2 list in the pinned regions carries it. A word-aligned
  scan of `SCUS_942.54` plus all 15 static overlay images (`crates/asset/data/static-overlays.toml`)
  finds no immediate `0x32C` load into any register, no access to the flag byte `0x800857BD` under any
  viable `lui` / `addiu` encoding, and no constant `0x32C` argument at any flag-helper call site
  (`FUN_8003CE08` set / `FUN_8003CE34` clear / `FUN_8003CE64` test). The only remaining reader is the
  0897 dev-menu flag browser, which reads any flag on demand.
- **`tunnelb P2[34]`** latches `0x322` / `0x326`. **`tunnelc P1[4]`** latches `0x360` + `0x362` from
  two branch arms (bodies `+0x107..+0x110` / `+0x2AB..+0x2B4`); **`tunnelc P2[6]`** latches `0x34A`.

**Rim Elm town variants.** `town01`, `town0b` and `town0c` share the opening chain (`549` -> `0x226`
-> `0x227`, plus sub-chains) byte-for-byte in `P2[3..=11]`: story-state renditions of one town.
`town0d` is the `0x7`-gated later variant. A `town0c` visit in the chapter-2 capture is a revisit (the
"scene" listed beside it in the poll is the capture CSV's column header, not a map).
Anchor `town0c_is_a_rim_elm_state_variant_not_a_ch2_spoke`.

**Rim Elm revisit chain (`town0b` band `0x228..0x233`).** A second flag band alongside the opening
chain.

| Record | C1 | C2 | Writes |
|---|---|---|---|
| `town0b P2[7]` | `0x22B`, `0x141` | `0x147` | self-latches `0x22B` at its head (`+0x26`, before the flash + waits); SETs `0x228` / `0x229` / `0x22A` from branch arms (`+0x377` / `+0x804` / `+0x8F9`, each at a `JmpRel` boundary inside camera / emitter choreography) |
| `town01` / `town0c P2[7]` | `0x22B` | `0x147` | the same gate shell; only `town0b`'s copy mints the band |
| `town0b P2[8]` | `0x231` | `0x22F` | sets `0x231` |
| `town0b P2[9]` | `0x232` | `0x141` | sets `0x232` |
| `town0b P2[10]` | `0x233` | `0x232` | sets `0x233` |
| `town0b P2[11]` | `0x141` | `0x231` | - |

- `P1[1]` is the state seeder: sets `0x22F` + `0x147`, clears `0x141` (same record in `town0c`).
- Readers: `town01 P0[1]` (the entry walk-on) branches on `0x22F` / `0x229` (`+0x69` / `+0x6D`); the
  NPC record `town0b P1[39]` selects dialogue over `0x22F` / `0x148` / `0x147` / `0x228` / `0x229` /
  `0x22A` in sequence.
- Late one-shots `town0b P2[30]` / `town0c P2[29]` latch `0x5C4` (`+0x3CD`, behind a `Test 0x35`
  battle-victory guard), read by the ending scene `edlast`.
- There are three renditions, not four. `gameover_data`'s CDNAME window is a subset of `town01`'s and
  holds no asset-table bundle; a MAN read out of it is `town01`'s own, reached by an entry-size
  over-read
  ([script-vm.md](../../subsystems/script-vm.md#a-second-script-byte-carrier-the-streaming-variant-man)).

**Rim Elm final variant (`town0e`), per-NPC band `0x5DC..0x5F0` + `0x6DC`.** Every NPC interaction
record `P1[1..24]` opens with the same head: `Test <own flag>` -> skip, `Set <own flag>`, then `Test`
the neighbouring NPCs' flags (`P1[2]`: `Set 0x5DC` at `+0x20`, then tests `0x5D8..0x5DB`). It is a
talked-to-everyone tracker - scene-local flavour state, not progression; record indices map 1:1 onto
the band.

**Uru Mais (`uru` / `uru2`) beat band.** `uru`'s cutscene tail latches `0x3BE` (`P2[30]`), `0x3BF`
(`P2[34]`), `0x3C0` (`P2[32]`) and `0x3FC` (`P2[38]`, body `+0x8B7` after a BGM cue). `P2[30]` is the
party-recompose beat: `PartyAdd char 1` + `Set 0x11`, `PartyAdd char 2` + `Set 0x12`, then
`Set 0x3BE` (`+0x72`) under a camera reconfigure. All four flags read back only within `uru`.

**Nivora Ravine (`nilboa`).** An entry group sharing `0x456`, a `0x47x` puzzle cluster, and a
cross-scene successor gated on `0x370`; `nilboa2` is the `0xF`-gated variant carrier. Anchor
`nilboa_nivora_ravine_gate_family`.

- `0x456`: `nilboa P2[11]` both SETs and CLEARs it (`Set 0x455` + `Set 0x456` at `+0x37..+0x39`,
  inside a `CC .. C3` per-actor run).
- `0x370` writer: `doman` variant `P1[15]` at MAN offset `0x06397`, a `53 70` SET in a clean
  choreography run whose loop-back `JmpRel` re-enters the record's gate-test head; the head's own
  `Test 0x370 -> +0x301E` jump lands on the very next op (the Dr. Usha "Do you understand? The first
  TimeSpace..." briefing branch) - the self-latch shape. The record's other three `53 70` occurrences
  are "Time**Sp**ace Bomb" prose aliases.
- `0x370` readers: the `doman` `P1[3..=18]` clean head TESTs (arc-gate dispatch chain, alias-immune
  operands). Pinned by
  `man_variant_carrier_census_disc.rs::flag_0x370_writer_is_the_doman_p1_15_usha_latch`; a live
  organic SET (the poll auto-snapshots flag 880) confirms play order.

**Chapter 3 - Karisto (`map03` and its spokes).** Anchor `map03_karisto_region_gate_families`.

- `map03` is a router with no gated records. Ungated hub state exists as inline latches:
  `map03 P2[15]` SETs `0x378` (`+0x9E`, between a 180-frame camera hold and the particle emitters),
  read back by `doman` and `map03` itself.
- `bubu2`: a small requires-all chain. `bubu1` carries no field MAN.
- `son` and `deroa`: sparse one-shots; `deroa` leads to the underground `chitei2`. `son`'s NPC records
  use the per-NPC one-shot head (`P1[14]`: `Test 0x62E` -> skip / `Set 0x62E` at `+0x52`) and branch
  on taiku's `0x382`.
- `korb3`, the Karisto castle approach: a nine-record collection group `P2[5..=13]`, each record
  gated on a distinct flag under one shared `0x403` "all done" latch.

**Chapter 3 - Karisto castle depth (`kor` / `koin` cluster + `chitei2`).** Anchors
`chapter3_karisto_castle_gate_families` + `chapter3_koin_family_and_writer_pins`.

- `kor`: one-shot beats (`0x408` read by `korout`, self-latches `0x409` / `0x40A`) plus a **door
  group** C2-gated on `0x612`, an arm-then-consume mechanic - the partition-0 entry scripts SET
  `0x612` and each door record clears it. `kor3` / `kor4` gate their doors on the same flag.
- `kor5`: a three-step chain `0x43A -> 0x436 -> 0x6C4`.
- `koin1b` is `koin1`'s story-state sibling (same gate shape + a spliced `0x00B` toggle pair); it
  owns the `0x3DA` SET `koin1` gates on. `koin1`'s `P2[9..10]` are a `0x50A` set / clear toggle pair.
- `chitei2`: the `0x470` / `0x4F0` and `0x4C4` / `0x4C6` / `0x4C8` / `0x4C9` families; `0x4C8` is
  co-written by `map03 P2[19]`.
- `korb2` / `koin2` / `koin6` are gateless.
- `koin3 P2[8]` and its stale sibling copy `other7 P2[5]` co-latch `0x430` (`koin3` body `+0xA40`, a
  `JmpRel` branch-arm set inside `CC` camera choreography), read by the ending scene `edlast`.
- **`0x50A` is the Sol game-hall minigame result toggle, written natively by the mode-24 minigame
  overlays** - a space the MAN script census cannot see. The Muscle Dome module (PROT 0977) CLEARs it
  in the post-match settle (`0x801D0FF8`) and win-re-SETs it (`0x801D101C`, labelled by the overlay's
  own `WIn on` / `WIn off` debug strings). The dance trio (0978..0980) SETs it at session start
  (`0x801CF968`) and CLEARs it on a missed goal (`0x801CFF10`). `koin1` hosts the Muscle Dome + Baka
  doors (`3E 69` / `3E 68`), `koin3` the dance doors (`3E 6A`), and `koin1 P2[9]` (C2=[`0x50A`]) is the
  returned-victorious beat.
- **`0x5D6` is a script self-latch.** `koin4 P1[15]` sets the flag it gates on: behind the "If you
  have money" line sit `48` (a one-byte no-op) and `55 D6`, and its `P2[3]` twin carries the same
  block. The bytes to search for are `55 D6`, not `D6 05`, and a walk that stops at a record's first
  text segment misses the site. The native-space sweep
  (`scripts/asset-investigation/flag_helper_call_sweep.py`, the move-VM ext flag sub-ops, the
  motion-VM census) is a negative for **native** writers only. See
  [script-vm.md](../../subsystems/script-vm.md) § native flag-bank writers; guard
  `koin_gates_0x50a_writer_less_0x5d6_self_latched`.
- Runtime oracle `chapter3_karisto_spine_oracle.rs`: the Conkram -> deroa -> chitei2 bridge, the
  `kor5` chain, the door arm-then-consume and the koin toggle all sequence through
  `p2_record_gates_pass` + `install_gated_p2_record` with no chapter-specific engine code.

**Chapter 3 - Conkram (`conc*`, the "past" arc).** Anchor `chapter3_conkram_gate_families`.

- Pivot pair `0x3E1` / `0x3E5`: `conc2 P2[12]` SETs `0x3E1`, the flag `deroa` C2-gates the `chitei2`
  descent on (the cross-region bridge). `conc3` self-latches `0x3E5` (`P2[10]`) and SETs `0x3F9`
  (ungated `P2[9]`); `conc P2[10]` chains on both.
- `conc` / `concnow` carry `r1..rN` **soldier rows**, all C1-gated on the low flag `0x007`, which is
  SET by `concnow P0[34]` + `conc2 P0[21]` (a "soldiers disperse" beat).
- `conc` has eleven doors on `0x6DE`, armed by the entry script's player-position BBoxTest run (the
  same mechanic as kor's `0x612`). All four carriers' entry scripts (`conc` / `conc2` / `conc3` /
  `concnow P1[0]`) SET `0x6DE`.
- `concend` is a single ungated epilogue record.
- `concnow` one-shot ladder, each C1 gate a self-latch in its own record: `P2[13]` -> `0x3ED`,
  `P2[14]` -> `0x3EE`, `P2[15]` -> `0x3D2` (at its tail `+0x1483`), `P2[16]` -> `0x3CE`, `P2[18]` ->
  `0x423`, plus `P2[20]` -> `0x3CF`.
- **`0x3EF` is the chapter-wide "Conkram revelation" gate.** `concnow P2[15]` SETs it from a branch
  arm (`+0xDDD`, after the emitter run + BGM cue, jumping straight to the record tail). Its operand
  byte is outside ASCII, so census reads are alias-immune: clean `Test` sites in fifteen scenes
  spanning Sebucus (`balden` / `balden2` / `bylon` / `dolk2` / `geremi` / `jiji` / `rayman` /
  `rayman2` / `retock` / `ropeway`) and Karisto (`koin1` / `koin2` / `son` / `doman`).
- **`0x423` is a cross-scene message, not a one-shot.** `conc2 P1[0]` consumes it on entry
  (`Test 0x423` -> `Clear 0x423`, `Set 0x664`, `SpawnRecord 0x69` at `+0xDB..+0xE8`): the `concnow`
  beat posts the flag and the next `conc2` visit converts it into `0x664` (read by `conc`) plus a
  spawned follow-up record.

**Cross-cutting patterns.**

- Two low-numbered flags are variant discriminators, gating nearly every record of an alternate or
  interior carrier: `0x7` (`rayman2`, `retockin`, `town0d`) and `0xF` (`dohaty`, `nilboa`, `nilboa2`).
- `0x7` latches inside the opening-commit beat (the `opdeene` -> `town01` handoff of a fresh new game,
  alongside `549` / `0x226`; `captures/state_poll/2026-07-29T20-20-05Z`), so the rendition selection
  is armed from the start of play.
- Region hubs hold little or no gate state; the progression logic lives in the spoke dungeons.
- The story-numbered band `0x522..0x531` is engine scratch: a one-hot exit selector + fade handshake
  repeated in nearly every scene's entry script ([script-vm.md](../../subsystems/script-vm.md) § the
  `0x527..0x531` scene-transition scratch band).
- Clean-tagged census rows over flags whose operand byte is printable ASCII can be dialogue bigrams
  (`ta` / `s,` / `Sp`); the wide reader lists of `0x461` and `0x32C` dissolve entirely under that check
  ([script-vm.md](../../subsystems/script-vm.md) § ASCII dialogue aliases).

**Play-order captures (poll tier).** Corpus `captures/state_poll/2026-07-29T20-20-05Z` /
`2026-07-29T22-21-04Z` / `2026-07-29T22-53-56Z`, mined via `analyze_state_poll.py --only flags`.
Screening rule: a save-state load emits a sub-bulk flag delta whose signature is a mode churn plus
`flagclr` rows plus a full inventory re-key at one tick, with the destination scene registering about
43 ticks later; every burst with that signature is excluded.

| Region | Live SET order (tile) |
|---|---|
| `retona` | `0x354` @(67,80) -> `0x355` @(69,78) -> `0x356` @(68,80) -> `0x3AD` @(66,79); `0x357` latches during the next scene entry (mode-2 transition frame after an overworld round trip - the `P1[0]` backstop converting `0x3AD`); then `0x367` @(67,78). `0x353` is already latched in the starting state. |
| `dohaty` | One corridor walk: `0x343` + `0x63D` @(23,42) -> `0x344` @(23,50) -> `0x345` @(23,53), leads `0x39F` / `0x65A` interleaved; the `P2[10]` `0x344` one-shot and the `0x63D` pair both fire. The last beat @(23,56) latches `0x1D4`, a flag in `balden P2[0]`'s C1 list (a cross-scene edge). |
| `taiku` | `0x517` @(54,17) -> `0x519` @(54,7) -> `0x38F` @(54,40) -> the `P2[16]` pair `0x380` + `0x382` @(16,28), one beat one tick. The other self-latch pair `0x390` does not fire (branch / optional content). |
| `kor5` | `0x43A` @(32,92) -> `0x436` @(32,40). `0x6C4` appears only inside a load delta. |
| `korb3` | `0x41D` @(36,21) (its C2 includes kor5's `0x436`, satisfied organically beforehand) -> `0x41E` @(36,23) -> `0x41F` @(37,24). |
| `map03` hub | `P2[15]` `0x378` @(93,69). |
| Sebucus spine | teien `0x1C8` @(41,45) -> `0x1C9` @(44,43) -> `0x332` @(44,47) -> tower `0x1C7` @(13,69) -> geremi `0x1BF` @(34,117). `tunnelc P2[6]` `0x34A` @(23,45); its `P1[4]` `0x360` / `0x362` do not fire (branch arms). balden's `0x1D5` latches at the `tunnelc` exit tile (60,16); its `0x5B3` self-latch does not fire. |
| Rim Elm opening | `549` at the opening commit, with `0x226` in the same beat. |

- **`korb3`'s collection group plays backwards from its gloss.** `0x403` latches on the `map03`
  overworld at the castle-approach node (72,41) *before* any collection flag, and all nine C2 flags
  (`0x43E..=0x444`, `0x459`, `0x45C`) mint together in a single `korb3` arrival burst (BGM stop -> nine
  sets -> new BGM, no load signature). The group's records are C1-retired before ever spawning.
- `ropeway` / `ropeway2` / `jiji` are walked organically in the capture corpus as well.
- Walked in this corpus **without** an organic family SET: `retock` / `retockin` (entered with `0x357`
  latched; `0x502` does not fire), `doman` (only the unpinned lead `0x379` fires; `0x3FB` does not),
  `nilboa` / `nilboa2` and `son` (entered mid-arc from loaded states; the only nilboa flag burst is a
  load frame).

**In-bracket write order (disassembly).** The playthrough cards place each remaining family between
two milestone saves; where a family writes more than once inside one bracket, the scripts fix the
order. Counting only story SET sites (clean, not a prose alias, not a developer flag-menu arm):

- **`rayman`: `0x201 -> 0x1FB -> 0x200 -> 0x1FC`, forced.** `0x201` is set by the talk record
  `P1[47]`; `0x1FB` by `P2[12]` (C2 `0x201`), `0x200` by `P2[18]` (C2 `0x1FB`), `0x1FC` by `P2[19]`
  (C2 `0x200`). Each link has one writer and each writer requires the link before.
- **`bubu2`: `0x609` then `0x3D3`, one beat.** Once `0x608` is set (the bracket before), the live
  writer is `P2[0]` (C1 `0x3D3`, C2 `0x608`), which SETs `0x609` and `0x3D3` on adjacent instructions
  (body `+0x1E` / `+0x20`). `P2[2]` carries `0x608` in its C1 and `P2[3]` is retired by `P2[0]`'s
  `0x609`.
- **retock: `0x502` before `0x33B`.** `0x502`'s writer is `retock P2[33]` (C1 `0x502`, C2 `0x357`),
  spawned only by Eliza's talk dispatch `P1[31]`, which tests `0x33B` first and diverts to her
  post-`0x33B` lines when it is set, then spawns `P2[33]` once `0x63C` (set by `P2[16]`, the first
  talk) is up. `0x33B`'s writer is `jagaroom P2[8]` (C2 `0x351`, which the `jagaroom P1[8]` actor sets
  when it spawns), and nothing there waits on `0x502`. A play that latches `0x33B` first can never
  write `0x502`; the Dohati save holds both. Full chain: `0x357 -> 0x63C -> 0x502 -> 0x33B -> 0x34F`
  (`jagaroom P2[9]`, C2 `0x33B`).
- **deroa: `0x3E1` before `0x46D`.** `P2[4]` (C1 `0x46D`, C2 `0x3E1`); `P2[5]` / `P2[6]` carry only
  their own latch.
- `retock P1[1]` / `retockin P1[1]` also write `0x33B`, `0x502` and `0x357`, but as developer
  flag-menu arms (flag-op runs ending in a `JmpRel` back to the picker), not story writers.

Pinned by `man_variant_carrier_census_disc.rs::region_gate_in_bracket_write_order`.


#### The play-order residual

What is not played organically, and why none of it leaves a question open. The generic C1 / C2
seeder drives every family.

**Arrival measured, family SETs unplayed:** `rayman`, `station`, `station3`, `bubu1` / `bubu2` and
`deroa`, each entered from a card-boot state
([`script-vm.md`](../../subsystems/script-vm.md#what-arriving-in-a-spoke-writes)).

- No gate family fires on arrival. Each spoke writes only its own arrival latch in `0x491..0x49C` at
  entry-script `+0x18`, `map03` keeps a one-hot story-stage word in `0x56D..0x570`, and every entry
  clears the band `0x526..0x52E`.
- The chains fire only on walk-on and talk beats: `rayman`'s `0x201 -> 0x1FB -> 0x200 -> 0x1FC`,
  `bubu2`'s requires-all list, `station` / `station3` behind `taiku`'s `0x38F`, `deroa`'s
  `0x3E1`-gated descent. `rayman2` also needs flag `0x1D5`, which every ladder save from Mt. Letona on
  carries.

**Walked without an organic family SET:** `retock` / `retockin` - `0x357` is pre-latched and `0x502`
does not fire, its writer hanging off Eliza's talk loop. Captured outside the poll corpus: `doman`'s
`0x3FB` (`P2[4]`), `son`'s arrival family, and the `kor5` tail's `0x6C4` (`P2[8]`, with its `0x436`
input poked rather than played). The retock / doman / nilboa entry families are measured
([field / locomotion](field.md)). `retock`, `doman`, `nilboa`, `son` and the `kor5` tail are walkable
from a catalogued card block; the remaining spokes are the ones no card reaches.

**Card brackets place every remaining family between two milestones.** The two playthrough cards hold
one retail save per story milestone, so a flag clear in one save and set in the next is written by
the play between them (`region_gate_card_brackets`, save-library gated).

| Flags | Between |
|---|---|
| `rayman`'s `0x201`, `0x1FB`, `0x200`, `0x1FC` | Sky Gardens and the Fire Path |
| `0x1D5` | the Fire Path and Mt. Letona |
| retock's `0x357` | Mt. Letona and Ratayu |
| `0x33B`, `0x502` | Ratayu and Dohati's Castle |
| `bubu2`'s `0x608` | Dohati and Sol Tower |
| `bubu2`'s `0x3D3` / `0x609`, doman's `0x3FB` | the Sol Tower B2 and Usha saves |
| the `kor5` tail | the two Sol Tower saves |
| `0x370` | Usha and Nivora |
| `0x378`, `0x3A6`, `0x60D` | Nivora and Zora |
| `0x38F`, `0x3A7` | Zora and Conkram |
| deroa's `0x3E1` / `0x46D..0x46F` | Rogue's Tower and Jette's Fortress |

**Two in-bracket orders are not script facts,** and neither is observable:

- **doman's `0x3FB` against bubu2's pair.** `doman P2[4]` carries only its own C1 latch, so the order
  is the route the player takes between the two scenes.
- **deroa's `0x46E` / `0x46F` against `0x46D`.** `P2[5]` and `P2[6]` carry only their own C1 latch and
  no op `0x44` in deroa spawns them, so a walk-on tile does and the order is where the player walks.
- Every reader of the three flags reads that one flag alone. `0x3FB` has no field-VM `TEST` anywhere;
  its only C1 holders are its writer `doman P2[4]` and the two ending epilogues `edbalden` / `eddoman`
  `P2[3]`, each listing `[0x3FB]` alone.
- `0x46E`'s one reader is `deroa P2[8]`, whose whole body is that test in front of `[4C 38]` (the
  camera-region query at the player's tile) and an idle loop.
- `0x46F`'s two readers are deroa's scene objects: `P0[0]` parks its object at `MoveTo 7F 7F` when the
  flag is set, and `P0[5]` takes `[4C 42]` target `-600` when it is set and `-1500` when it is clear.
- No record lists any of the three in a C2 gate, and the SCUS and overlay images carry no literal call
  of the flag helpers `FUN_8003CE08` / `_CE34` / `_CE64` with these ids. Each flag's value is pinned by
  the milestone saves on both sides of the bracket. Pinned by
  `region_gate_unordered_flags_have_no_joint_reader`.

**Measuring a spoke's arrival without a walk.** A walk-on door is an exact tile match in the `.MAP`
kind-1 trigger table, so writing the player object's position onto a door tile crosses it in about
ninety vsyncs
([`autorun_w5a_poke_walk.lua`](../../../scripts/pcsx-redux/autorun_w5a_poke_walk.lua); door tiles come
from each `.MAP`'s `+0x10000` / `+0x12000` gate rows). A door that opens a Yes / No picker takes one
confirm press on top. Paired with the single-flag write watch
([`autorun_w5a_flag_watch.lua`](../../../scripts/pcsx-redux/autorun_w5a_flag_watch.lua)) that is one
run per spoke from a card block that reaches its region; it measures arrival, never the walk. A flag
written every other frame from an entry script's per-frame body is a one-hot selector or a position
predicate, not a progress latch (`chitei2`, `conc`;
[`script-vm.md`](../../subsystems/script-vm.md#a-system-flag-can-be-a-live-position-test-not-progress)).

### Extraction-0874 §2 (`player.lzs`) F-variant pixels - a one-shot opening face-frame stamp, not a menu writer

*Status:* resolved - the installing event is named

The three F-variant halfwords in the field-character texture band are stamped once per game by the
Rim Elm opening cutscene, through a field-VM `MoveImage` op.

- **The words.** `(853,271)` `3333 -> ffff`, `(856,271)` `3333 -> fff3`, `(857,271)` `1e33 -> 1e3f` -
  row 271 cols 1 / 4 / 5 of the Noa strip (TIM 2 at `(852,256)` 20x128; rows 271 / 273 = its rows
  15 / 17).
- **Installer.** `town01` MAN `P2[3]` (`★ＯＰ`, the Rim Elm opening timeline record, C1-gated on the
  opening latch `0x225`), body `+0x392` / `+0x3A0`. After the opening's white flash + 60-frame wait it
  stamps the Noa face cell: `MoveImage (852,336,6,16) -> (852,268)` and `(852,368,4,8) -> (853,284)`.
- **Op.** `4C 60`, a literal-operand MoveImage `[4C 60 src_x src_y w h dst_x dst_y]` with six
  misaligned u16s read via `FUN_8003CE9C`; handler arm `0x801E1B28..0x801E1B90`,
  `jal FUN_80058490` at `0x801E1B84`.
- **Disc location.** MAN offsets `0x735A` / `0x7368` (PROT 0004 §1, LZS at container `0x25BEB`). The
  operands are misaligned u16s, so an aligned scan does not find them.
- **Live catch.** At `ra = 0x801E1B8C` the stamp reproduces the s3 anchor band byte-exact
  (`autorun_s2s3_atlas_stamp.lua`). The parked alternate frame differs from the boot cell at exactly
  the three halfwords.
- **Lifetime.** The `0x225` C1 gate fires once per game, so every post-opening save carries the
  variant; the first battle effect-texture re-upload restores the disc bytes. A freshly booted game
  does not hold it: the title screen is all-zero and the mode-2 field-entry load uploads the disc
  bytes.
- **Not the pause menu** (grade `capture`, exhaustive): with every DMA2 kick chain-walked for
  `A0/80/E3/E4/E5` packets and GP0 PIO stores hooked, the whole pause walk issues zero image transfers
  and the band is byte-identical before and after. A 49-state library census shows plain field saves
  carrying the variant with no menu in their lineage while `s1` / `s2` hold disc bytes; the flip
  brackets inside the `town01` opening (s2 -> s3).
- **Not a parked wrap-scroll phase.** The three words equal the disc words at `(x,273)` by
  frame-content coincidence: the Noa strip is not shift-invariant, so a parked +2-row rotation would
  move dozens of rows. The wrap-scroll installers (move-VM op `0x1E`, body `0x80023694`; the op `0x45`
  sibling) and the `FUN_80021DF4` dispatch-4 arm never fire across a full s2 -> s3 replay while the
  flip reproduces (`autorun_s2s3_scroll_installer.lua`).

Owning page:
[character-mesh.md](../../formats/character-mesh.md#runtime-scroll-cell-residue-why-a-live-vram-dump-can-differ-from-the-tim).

### What the op-`0x49` entry-context kind byte is, and which screens it selects

*Status:* resolved (disassembly + disc measurement) -
[`save-screen.md § Root command picker`](../../subsystems/save-screen.md#root-command-picker-fun_801d6b20)

The pause / save driver `FUN_801DC6B4` routes on `*_DAT_8007B450`, and the byte it reads there is the
op-`0x49` sub-op.

- **Writers.** Ten `sw rt,0xb450(rs)` sites exist across `SCUS_942.54` and every extracted PROT entry.
  Two store a dereferenceable pointer: the field VM's op-`0x49` Idle arm (`0x801E09A8`, storing the
  script's *operand pointer*, whose first byte the arm reads at `0x801E0984`) and `FUN_801D0B90`'s
  countdown expiry (`0x801D0D04`, pointing at the static record `DAT_801F2278`, kind `0x0B`). The other
  eight store `0` or the `1` Done sentinel; the resume clears the slot at `0x801E08D8`.
- **Kinds that select a screen,** each at exactly one selector write in PROT 0899 (of 66
  `sw rt,0x46a4(rs)` writers): `0` -> sub-screen `0x1A`, `1` -> `0x19`, `7` -> `0x20` (the casino
  prize exchange), `0x0D` -> `4`. Kind `0x0D` also sends the root picker's cancel to sub-screen `3`
  (`0x801D6D18`).
- **Kind `0x0D` is a scripted pre-battle party menu,** briefed on entry and ready-checked on exit.
  Sub-screen `4` draws window `6`, six static VAs in the overlay pool loaded at
  `0x801d636c..0x801d6448` (a pre-battle briefing); sub-screen `3` draws window `5`, whose two headings
  (`0x801CEC78` / `0x801CEC94`) are a battle-start ready check, not a "really leave?" gate.
- **Reachability.** `crates/engine-core/tests/op49_sub_op_census.rs` walks every scene MAN's field-VM
  script and tallies the sub-op operands twice - a bounded, offset-deduped opcode walk and a raw byte
  upper bound - and kinds `7` and `0x0D` appear in both. The walk decodes no tile-board sub-op (`5`)
  that the byte bound finds, so an absent walk row means "not decoded here", not "not on the disc".
- **Port.** `World::record_op49_park` / `World::menu_entry_context_kind`,
  `FieldMenuSession::{open_entry_screen, Notice, ReadyConfirm}`.
