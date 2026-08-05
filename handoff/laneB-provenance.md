# Lane B - `module-orphan` provenance triage outside `crates/engine-core/`

All 40 rows the gate raised for `engine-vm` / `engine-ui` / `engine-audio` /
`engine-render` / `asset` were read against the routine's **disassembly** (the
`--- DISASSEMBLY ---` section of the dumps in the main checkout's
`ghidra/scripts/funcs/`, never the decompiled C except as a search index, and
never a committed doc as the primary evidence).

**Counts: 38 `CORRECT`, 2 `WRONG-CODE`, 0 `WRONG-ADDRESS`, 0 `MISPLACED`.**
Gate rows for these five crates went **40 -> 38**; the 38 that remain are
reviewed-and-correct and their waiver stanzas are ready in
[`laneB-port-provenance-waivers.toml`](laneB-port-provenance-waivers.toml).

Every verdict below is `disassembly-grounded`. Where a committed doc was the
*subject* rather than the evidence it is called out in the last column.

## The two heavy files: is the convention systematically wrong?

**No - and the reason is the same one in both files, which is worth stating
because it is a property of the checker meeting a property of the subsystem.**

`ui_menu_window_painters.rs` (14 rows) ports the menu overlay's
window-descriptor content painters. Each is a leaf whose *only* distinctive
datum is its own private overlay string-pool VA, and whose only callees are the
corpus-wide text writer `0x80036888`, number writer `0x80034B78` and marker
blitter `0x8002B994` - all far above the checker's distinctiveness cut. The two
"corroborating siblings" (`801d603c` / `801d61b0`) corroborate each other only
because they both branch on the choice-state word `DAT_801E46D0`, which the
other twelve have no reason to read. A painter in this table is therefore
*structurally* an orphan.

`baka_hub_actors.rs` (11 rows) is the same shape one level up. Its four
corroborating siblings share the submode cursor context `0x801C6EA4` and the
panel-install callee `0x801E9B3C` - but both are common enough corpus-wide to
sit above the cut, so even the two orphans that genuinely *do* form `0x801C6EA4`
and *do* `jal 0x801E9B3C` (`801f1138`, `801f1e48`) read as sharing nothing. The
rest are panel painters with only their own string VA.

So the answer to "14 independent mistakes or one wrong convention?" is neither:
one structural interaction, 25 correct tags. The signal is doing what
`port-provenance.md` says it does - ranking suspicion, not proving anything -
and its precision on this slice is 2/40.

## Verdicts

| Address | File | Verdict | Evidence | What changed |
|---|---|---|---|---|
| `801dcfe4` | `engine-ui/ui_menu_window_painters.rs:220` | CORRECT | disassembly | - |
| `801dca0c` | `engine-ui/ui_menu_window_painters.rs:221` | CORRECT | disassembly | - |
| `801dca50` | `engine-ui/ui_menu_window_painters.rs:221` | CORRECT | disassembly | - |
| `801dca94` | `engine-ui/ui_menu_window_painters.rs:221` | CORRECT | disassembly | - |
| `801dcad8` | `engine-ui/ui_menu_window_painters.rs:221` | CORRECT | disassembly | - |
| `801dcb1c` | `engine-ui/ui_menu_window_painters.rs:221` | CORRECT | disassembly | - |
| `801dcf14` | `engine-ui/ui_menu_window_painters.rs:248` | CORRECT | disassembly | - |
| `801dccb4` | `engine-ui/ui_menu_window_painters.rs:352` | CORRECT | disassembly | - |
| `801dce20` | `engine-ui/ui_menu_window_painters.rs:402` | CORRECT | disassembly | - |
| `801dcc20` | `engine-ui/ui_menu_window_painters.rs:484` | CORRECT | disassembly | - |
| `801d6360` | `engine-ui/ui_menu_window_painters.rs:767` | CORRECT | disassembly | - |
| `801d4a80` | `engine-ui/ui_menu_window_painters.rs:854` | CORRECT | disassembly | - |
| `801d56fc` | `engine-ui/ui_menu_window_painters.rs:952` | CORRECT | disassembly | - |
| `801d5944` | `engine-ui/ui_menu_window_painters.rs:1054` | CORRECT | disassembly | - |
| `801f0adc` | `engine-vm/baka_hub_actors.rs:574` | CORRECT | disassembly | - |
| `801f1138` | `engine-vm/baka_hub_actors.rs:885` | CORRECT | disassembly | - |
| `801f1e48` | `engine-vm/baka_hub_actors.rs:925` | CORRECT | disassembly | - |
| `801f16c0` | `engine-vm/baka_hub_actors.rs:1056` | CORRECT | disassembly | - |
| `801f17d8` | `engine-vm/baka_hub_actors.rs:1139` | CORRECT | disassembly | - |
| `801f1890` | `engine-vm/baka_hub_actors.rs:1162` | CORRECT | disassembly | - |
| `801f1950` | `engine-vm/baka_hub_actors.rs:1198` | CORRECT | disassembly | - |
| `801f1a1c` | `engine-vm/baka_hub_actors.rs:1243` | CORRECT | disassembly | - |
| `801f1b64` | `engine-vm/baka_hub_actors.rs:1265` | CORRECT | disassembly | - |
| `801f1ab0` | `engine-vm/baka_hub_actors.rs:1280` | CORRECT | disassembly | - |
| `801f90dc` | `engine-vm/baka_hub_actors.rs:1318` | CORRECT (label defect fixed) | disassembly | `CAPTION_MONEY_ID` -> `CAPTION_POINT_CARD_ID`; "money pseudo-item" -> Point Card, in the crate and in `minigame-baka-fighter.md` |
| `801d0fa8` | `asset/minigame_slot_scene.rs:204` | CORRECT | disassembly | - |
| `80063aa8` | `engine-audio/seq_calc.rs:8` | CORRECT | disassembly | - |
| `800638d8` | `engine-audio/seq_events.rs:89` | CORRECT | disassembly | - |
| `801d1a20` | `engine-render/battle_intro.rs:721` | CORRECT | disassembly | - |
| `801d0148` | `engine-ui/ui_menu/field_panels.rs:316` | CORRECT | disassembly | - |
| `801d0d18` | `engine-ui/ui_menu/pause_lists.rs:436` | CORRECT | disassembly | - |
| `801d1b20` | `engine-ui/ui_menu/pause_lists.rs:519` | CORRECT | disassembly | - |
| `801da34c` | `engine-vm/battle_action/queue_applier.rs:4` | CORRECT | disassembly | - |
| `801dba90` | `engine-vm/battle_cast_dispatch.rs:6` | CORRECT | disassembly | - |
| `801dbc30` | `engine-vm/battle_party_panel.rs:4` | CORRECT | disassembly | - |
| `801ed710` | `engine-vm/world_map_overlay.rs:442` | CORRECT | disassembly | - |
| `801edf00` | `engine-vm/world_map_panel_actors.rs:441` | CORRECT | disassembly | - |
| `801d0d38` | `engine-vm/world_map_panel_actors.rs:1041` | CORRECT (operand defect fixed) | disassembly | countdown delta is `_DAT_1F800393`, not `_DAT_1F80038F`; fixed in the crate and in `world-map.md` |
| `801db380` | `engine-ui/ui_menu_window_painters_large.rs:780` | **WRONG-CODE** | disassembly | `PORT:` -> `REF:` with the reason |
| `801dd9d4` | `engine-vm/field.rs:59` | **WRONG-CODE** | disassembly | moved out of the module `PORT:` list into `REF:` with the reason |

## The two defects, in full

### `FUN_801DB380` in `ui_menu_window_painters_large.rs`

`overlay_menu_801db380.txt` is 285 instructions and **draws nothing**. It is a
three-phase state machine over `DAT_801E46AC`: phase 0 clears the cursor word
`0x801E46C0`, clears `0x8007BB94` and hands the actor VM the script `0x801E4E84`
through `FUN_801D6628`; phase 1 navigates `party_count + 1` rows with
`FUN_801D688C` and commits a buy (bag add `FUN_800421D4`, gold `0x8008459C -=`
price, Point Card credit gated on `FUN_80042F4C(0xFE)`) or an equip swap, then
opens `0x801E4EA8`; phase 2 waits a confirm, plays cue `0x20` and returns to
sub-screen `0x1B`.

`recipient_picker_draws_for` is a **composition of the window painters that
those scripts open** - it calls `equip_target_list_draws_for` (window 36) and
the party-compare builder (window 41), both of which carry their own tags for
their own routines. Nothing in it implements any instruction of `FUN_801DB380`.
The old tag's parenthetical, "the sub-screen's draw half", invented a half the
body does not have.

The canonical port already exists: `crates/engine-core/src/shop.rs:679` carries
`PORT: FUN_801DB380` for `BuyRecipientSession`, and the checker's own
corroboration line names it. So the engine-ui tag was a duplicate that
mis-described this function; it is now a `REF:` that says what the routine is
and where its port lives. No coverage is lost.

### `FUN_801DD9D4` in `engine-vm/src/field.rs`

Seven base-tagged field-image dumps (baka_fighter, cutscene_dialogue,
cutscene_mapview, dance, debug_menu, fishing, slot_machine) agree instruction
for instruction on a 69-instruction body at this VA, and it is a **per-actor GPU
primitive emitter**: it stages a `0x05000000` header plus a `0x28808080`
flat-colour packet into `_DAT_1F8003A0`, copies `actor[+0xB8/+0xBA/+0xBC]` in as
RGB, and walks the 8-entry jump table at `0x801CEC40` calling `func_0x8003D2C4`
once per slot. (`overlay_0897_801dd9d4.txt` is *not* evidence here: its header
reads `== FUN_801dd8f0 801dd9d4 (entry=801dd8f0) ==` - the requested address is
an interior address of a different function, the trap
`dump-corpus-integrity.md` describes.)

`field.rs` ports the field/event VM. It never implements that emitter. The
address appears in the crate only as a **token**: op `0x43` sub-`0xE` hands it to
`func_0x8003CF04(_DAT_8007C34C, FUN_801DD9D4)` as an actor-list search
predicate, and both places that mention it say so -
`FieldHost::op43_mark_actor_flag_8` has an empty default body, and the
`0x801DF8D8` tail note in `step/menu_ctrl/nibble_5_6_7.rs` says outright "Not
modelled". `docs/reference/functions/script-vms.md:75` already describes the
routine correctly, so no doc needed correcting - only the tag.

Consequence to expect: `801DD9D4` now reads as *documented but not ported* in
`port-catalog.py`, which is its true state.

## Out of scope - for whoever owns these

1. **`scripts/ci/port-provenance-waivers.toml`.** The 38 reviewed-correct rows
   need waiver stanzas there. They are written and TOML-validated in
   [`laneB-port-provenance-waivers.toml`](laneB-port-provenance-waivers.toml) -
   append verbatim. They were not written into the real file because it is
   outside Lane B's edit scope and other lanes are adding rows to it.

2. **`docs/reference/functions/battle.md:215`** (`FUN_801DA34C` row) says the
   saved command block comes "from `record+0x1B7`". The routine has **two**
   slots, and the first one is `+0x1A7`: the literals are
   `0x80084140 + (id-1)*0x414 + 0x76F` and `+0x77F`, which are `+0x1A7` and
   `+0x1B7` off the `0x80084708` record base. The port's own doc in
   `queue_applier.rs` already states both correctly; only the functions-directory
   row names one. Left alone because `docs/reference/functions/` belongs to the
   doc-citation lane.

3. **`FUN_801D1A20` wears a `PORT:` tag in two crates** -
   `engine-render/battle_intro.rs` (packet emit) and
   `engine-vm/battle_intro_swirl.rs` (addressing + path choice). This is a
   deliberate split of one routine and both halves disclose it, so it is not a
   defect; noted only so a future reader does not "fix" it into one tag.
