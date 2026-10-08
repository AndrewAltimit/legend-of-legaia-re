//! Battle HUD plates and plaques: the readout bar and ring AP plate, the move
//! name and message bars, the plate glide offsets, the action / target
//! plaques with their element badges, and the Ra-Seru magic chips and ring
//! marks. Split out of `battle_hud.rs`; no logic change.

use super::*;

/// The party member whose full-width readout bar (placement record 7) is
/// up this frame, or `None`.
///
/// Command entry: the ring (step 1, `07/0`) and the item / magic target
/// steps (`0x18` / `0x1B`, `07/0` for the member under the cursor) raise
/// it; the attack-mode prompt, the target cursor, the arts-entry screen and
/// the browsed windows park it (`07/1`).
///
/// Action, both openers read off `FUN_801E295C`:
///
/// * the seed `0x0C` (`0x801E2F24..0x801E2F44`, again at `0x801E401C`):
///   `t2 = actor[+0x1DD]` is the action's target; `sltiu v0,t2,3` opens
///   record 7 for that member and stores it as `ctx[+0x18]` for the close.
///   Tail Fire and Glare on Vahn show `Vahn`; Vahn's Somersault on Gimard
///   shows nothing;
/// * the Item pre-arm `0x3C` (`0x801E3DA0..0x801E3DC0`) opens it for the
///   acting member when that member is a party slot. A Spirit action never
///   reaches `0x3C` - its seed arm goes straight to `0x46` - and raises the
///   AP bar + plate pair instead, so it has no readout bar.
pub fn battle_readout_bar_slot(world: &crate::world::World) -> Option<u8> {
    use legaia_engine_vm::battle_action::ActionCategory;
    let pc = party_count(world) as u8;
    match battle_hud_phase(world) {
        BattleHudPhase::CommandEntry => match battle_command_surface(world)? {
            CommandSurface::Ring => command_entry_actor(world).filter(|s| *s < pc),
            CommandSurface::ItemTarget(slot) | CommandSurface::SpellTarget(slot) => {
                slot.filter(|s| *s < pc)
            }
            _ => None,
        },
        // A dome play-out raises the fighter's bar while the opponent's
        // play lands on it - the seed `0x0C` opens record 7 for a party
        // target - and parks it while the fighter is the one acting.
        BattleHudPhase::Action if world.mode == crate::world::SceneMode::MuscleDome => world
            .muscle_playback_tally()
            .and_then(|(attacker, _)| (attacker == 1).then_some(0)),
        // The counterattack swap keeps the bar the monster's seed raised for
        // its target - the counterer ([`crate::world::BattleState::counter_hud`]).
        BattleHudPhase::Action if world.battle.counter_hud.is_some() => {
            world.battle.counter_hud.filter(|s| *s < pc)
        }
        BattleHudPhase::Action => {
            let a = world.battle_ctx.active_actor;
            let actor = world.actors.get(a as usize)?;
            let party_target = {
                let t = actor.battle.active_target;
                (t < pc).then_some(t)
            };
            let cat = actor.battle.action_category;
            if cat == ActionCategory::Spirit.as_byte() {
                // The Spirit arm (`0x801E2F54..0x801E3024`) sends category
                // `4` straight to `0x46` and jumps past both record-7 opens:
                // it raises the AP bar (`0x0F`) and the AP plate (`0x52`)
                // instead ([`crate::world::World::spirit_gauge_view`]). The
                // retail Spirit captures carry no readout bar.
                None
            } else if cat == ActionCategory::Item.as_byte() {
                if a < pc { Some(a) } else { party_target }
            } else {
                party_target
            }
        }
        _ => None,
    }
}

/// The value the ring's AP plate (placement record 82, at `(208, 174)`)
/// shows, or `None` when the plate is not up.
///
/// Step 1 slides it in (`52/0`) and every step that leaves the ring sends it
/// off (`52/1`) except the arts-entry screen, which keeps it and draws its
/// own copy in the port. `FUN_801D8DE8` case `0x52` (`0x801D9028`) reads
/// the acting member's actor `+0x170` - the Spirit gauge, `0` for a level-1
/// Vahn in the sparring fight's `v0_1_battle_command_submenu` frame.
pub fn battle_ring_ap_plate_value(world: &crate::world::World) -> Option<u8> {
    if battle_command_surface(world) != Some(CommandSurface::Ring) {
        return None;
    }
    if world.mode == crate::world::SceneMode::MuscleDome {
        return Some(world.minigames.muscle_dome.as_ref()?.spirit(0).min(100) as u8);
    }
    let actor = world.battle.command.as_ref()?.actor;
    Some(world.spirit_gauge(actor).min(100) as u8)
}

/// The name label retail draws under the action (placement records 76 /
/// 77, `Somersault` / `Tail Fire` / `Glare` / `Healing Leaf`), or `None`.
///
/// Both records sit at `y = 150` with `w = 0`; the four X fields
/// (`0x722 / 0x72A / 0x73A / 0x742`) are written to `0xA0 - width / 2`
/// before the open, so the label is centred and never glides
/// (`engine-ui::battle_name_banner::banner_x`). Sources, per category:
///
/// * a party member's attack band names the **art** whose constant the
///   strike cursor has passed - `FUN_8004C650` places the record when the
///   art's animation commits, so `player_steal_skeleton_pre` (`0x1E`, chain
///   start) has no label and `battle_melee_hit_spark` (`0x20`, mid-chain)
///   has `Somersault`; a plain swing never shows one;
/// * a **monster** cast names the spell: the `0x28` arm stores the spell
///   table's `+8` name pointer into both records before
///   `FUN_801D8DE8(0x4C, 0)` (`0x801E4430..0x801E4458`), behind a seat test
///   that skips the block for a party caster;
/// * an item names the item (the `0x3C` arm's `(0x4C, 0)` at `0x801E3DC8`).
pub fn battle_move_name(world: &crate::world::World) -> Option<String> {
    use legaia_engine_vm::battle_action::ActionCategory;
    // A dome play-out has no battle-action record behind it to name.
    if battle_hud_phase(world) != BattleHudPhase::Action
        || world.mode == crate::world::SceneMode::MuscleDome
    {
        return None;
    }
    let a = world.battle_ctx.active_actor;
    let actor = world.actors.get(a as usize)?;
    let pc = party_count(world) as u8;
    let cat = actor.battle.action_category;
    if cat == ActionCategory::Attack.as_byte() || cat == ActionCategory::TacticalArts.as_byte() {
        if a >= pc || world.battle.move_label_closed {
            return None;
        }
        let staged = usize::from(actor.battle.strike_index).min(actor.battle.params.len());
        let character = crate::battle_arts::character_for_slot(a);
        actor.battle.params[..staged]
            .iter()
            .rev()
            .filter_map(|&b| legaia_art::ActionConstant::from_byte(b))
            .find(|c| c.is_art())
            .and_then(|c| legaia_art::tables::art_name(character, c))
            .map(str::to_string)
    } else if cat == ActionCategory::Magic.as_byte() {
        // PROT 0954's arm 9 re-opens the label on the landed outcome's name
        // (`FUN_801D8DE8(0x4C, 0)` at `0x801F7CD4`, the string at
        // `0x801F8D50 + id * 0x28`).
        if let Some(name) = world.casting.fatal_banner.as_ref() {
            return Some(name.clone());
        }
        // The `0x28` arm's spell-name write is gated on the caster's seat:
        // `lbu v0,0x2(s5); sltiu v0,v0,3; bne v0,zero,0x801E4460`
        // (`0x801E43D0..0x801E43DC`) skips it for a party caster, so a
        // party cast has no name label of the band's own.
        if a < pc {
            // A summon module prints its own line at the label's place
            // (`FUN_8003541C(.., 0x96, ..)`): the spell name, then the
            // attack name, until the band's exit.
            use legaia_engine_vm::cast_module_camera::ModuleCaption;
            let id = actor.battle.params[0];
            return match world.casting.module_caption? {
                ModuleCaption::SpellName => world
                    .menu
                    .text
                    .as_ref()
                    .and_then(|t| t.spell_name(id))
                    .map(str::to_string),
                ModuleCaption::AttackName => world.tables.summon_attack_names.get(&id).cloned(),
            };
        }
        let id = actor.battle.params[0];
        world
            .menu
            .text
            .as_ref()
            .and_then(|t| t.spell_name(id))
            .map(str::to_string)
    } else if cat == ActionCategory::Item.as_byte() {
        let id = actor.battle.params[0];
        world
            .menu
            .text
            .as_ref()
            .and_then(|t| t.item_name(id))
            .map(str::to_string)
    } else {
        None
    }
}

/// The full-width message bar's line (HUD element `0x5B`, placement record
/// 91), or `None`: the death-spoils caption a slain monster raised - the
/// Evil God Icon's steal, or a thief's loot handed back
/// ([`crate::battle_steal`]). Composed at the death commit from the
/// templates on the user's executable, and held for the rest of the action
/// that raised it.
pub fn battle_message_bar(world: &crate::world::World) -> Option<String> {
    if world.mode != crate::world::SceneMode::Battle {
        return None;
    }
    world.battle.steal_caption.as_ref().map(|c| c.text.clone())
}

/// Where a gliding plate sits relative to its rest seat: `FUN_801D9BBC`'s
/// linear step from `seat_a` to `seat_b` (`a + (b - a) * elapsed / total`,
/// snapped once settled), less `seat_b`. `0` with no glide recorded.
pub(super) fn plate_glide_dy(
    glide: Option<&legaia_engine_vm::battle_commit_log::LogLaunch>,
    seat_a: i32,
    seat_b: i32,
) -> i32 {
    let Some(g) = glide.filter(|g| !g.settled()) else {
        return 0;
    };
    (seat_a - seat_b) - (seat_a - seat_b) * i32::from(g.elapsed) / i32::from(g.total.max(1))
}

/// Seat A / seat B rows of the level-up window (records `0x45..=0x4B`:
/// `y = -24` -> `14`) and of the report / loss window (`0x41` / `0x42`:
/// `236` -> `160`), off the placement table.
pub(super) const LEVEL_UP_WINDOW_SEATS_Y: (i32, i32) = (-24, 14);

pub(super) const REPORT_WINDOW_SEATS_Y: (i32, i32) = (236, 160);

/// How far the level-up window and the report (or loss) window sit below
/// their rest rows this frame, `(level_up, report)`: the raise glide the
/// results frame starts ([`crate::world::BattleState::result_windows_glide`]).
pub fn battle_result_windows_dy(world: &crate::world::World) -> (i32, i32) {
    let g = world.battle.result_windows_glide.as_ref();
    let (a, b) = LEVEL_UP_WINDOW_SEATS_Y;
    let (c, d) = REPORT_WINDOW_SEATS_Y;
    (plate_glide_dy(g, a, b), plate_glide_dy(g, c, d))
}

/// Seat A / seat B rows of the actor-name plaque (record `0x44`: `(16,
/// -24)` -> `(16, 14)`, read off the placement table in every battle state).
pub(super) const ACTION_PLAQUE_SEATS_Y: (i32, i32) = (-24, 14);

/// Seat A / seat B rows of the target plaque (record `0x51`: `y = 236` ->
/// `194`).
pub(super) const TARGET_PLAQUE_SEATS_Y: (i32, i32) = (236, 194);

/// How far the **action** phase's actor-name plaque sits below its rest
/// seat this frame (negative = above), in screen pixels.
///
/// The action seed raises record `0x44` with `FUN_801D8DE8(0x44, 0)`
/// (`FUN_801E6D84`, the `jal` every category arm of state `0x0C` falls into
/// at `0x801E3028`), which spawns it at seat A, off the top edge, and
/// `FUN_801D9BBC` glides it down over `ctx[+0x1C] = 0x10` frames. So a frame
/// taken the step the seed ran - the `super_queue_*` captures at `0x14` -
/// shows no plaque, and one taken half a glide later shows it half in.
/// Outside the action phase (the command ring's `Begin | <name>` trail) the
/// plaque is a different record and this is `0`.
pub fn battle_action_plaque_dy(world: &crate::world::World) -> i32 {
    if battle_hud_phase(world) != BattleHudPhase::Action
        || world.mode != crate::world::SceneMode::Battle
    {
        return 0;
    }
    let (a, b) = ACTION_PLAQUE_SEATS_Y;
    plate_glide_dy(world.battle.action_plaque_glide.as_ref(), a, b)
}

/// How far the target plaque sits below its rest seat this frame - the
/// same raise glide as [`battle_action_plaque_dy`], on record `0x51`, from
/// below the bottom edge.
pub fn battle_target_plaque_dy(world: &crate::world::World) -> i32 {
    if world.mode != crate::world::SceneMode::Battle {
        return 0;
    }
    let (a, b) = TARGET_PLAQUE_SEATS_Y;
    plate_glide_dy(world.battle.target_plaque_glide.as_ref(), a, b)
}

/// Seat A / seat B rows of the active-actor bar (record 7: `y = 234` ->
/// `192`, the glide slot a retail cast capture holds mid-raise).
pub(super) const READOUT_BAR_SEATS_Y: (i32, i32) = (234, 192);

/// How far the active-actor bar sits below its rest seat this frame - the
/// raise glide record 7 runs when the action seed (or the item pre-arm)
/// opens it ([`crate::world::BattleState::readout_bar_glide`]). `0` outside
/// the action phase, where the bar is the command ring's or a target step's.
pub fn battle_readout_bar_dy(world: &crate::world::World) -> i32 {
    if battle_hud_phase(world) != BattleHudPhase::Action
        || world.mode != crate::world::SceneMode::Battle
    {
        return 0;
    }
    let (a, b) = READOUT_BAR_SEATS_Y;
    plate_glide_dy(world.battle.readout_bar_glide.as_ref(), a, b)
}

/// Whether `FUN_801E6D84`'s target arm runs for this category: it returns
/// early for Run (`li v0,0x5; beq` at `0x801E6DEC`) and, past the actor
/// plaque, for categories `0` and `4` (`beq s0,zero` / `beq s0,v0` with
/// `v0 = 4`, `0x801E6FA0..0x801E6FA8`).
pub(super) fn seed_plates_reach_the_target_arm(category: u8) -> bool {
    !matches!(category, 0 | 4 | 5)
}

/// The three Seru-magic ids `FUN_801E6D84` sends down its **row** arm
/// instead of the single-target plaque, whatever their target byte
/// (`li v0,0x8d` / `0x86` / `0x82` and the three `beq` at
/// `0x801E6E4C..0x801E6E68`): Mushura, Zenoir and Theeder. The row arm
/// counts the living monsters and stages their names
/// (`0x801E6E70..0x801E6F90`) and opens no plate of its own, so
/// `theeder_summon_mid_cast`, `zenoir_summon_mid_cast` and
/// `mushura_summon_mid_cast` hold only the actor plaque in their handle
/// lists while the single-target casts beside them (`nighto`, `swordie`)
/// also hold record 81.
pub const ROW_PLATE_SPELL_IDS: [u8; 3] = [0x8D, 0x86, 0x82];

/// The bottom-right target plaque (placement record 81): the monster a
/// party member's attack is aimed at, with its element badge - or `None`.
///
/// The record's `x` is written to right-align the plate's cap at `x = 312`
/// (`241` for `Gimard` behind its badge, `w = 63`; `245` for `Skeleton A`,
/// `w = 59`; `249` for `Gobu Gobu`, `w = 55`), it rises from `y = 236` to
/// `194`, and its name payload carries the `0xCE` badge escape in front of
/// the name. Live from the strike loop (`0x1E`) through the Done band in
/// every party-attack capture, absent from every monster-action one, and
/// closed in `0x51` only for a monster target under categories `1..=3`
/// (`0x801E6314..0x801E6348`).
pub fn battle_target_plaque(world: &crate::world::World) -> Option<(String, Option<u8>)> {
    use legaia_engine_vm::battle_action::ActionCategory;
    if battle_hud_phase(world) != BattleHudPhase::Action || world.battle.target_plate_cleared {
        return None;
    }
    // A dome play-out: the fighter's plays name the opponent here, the
    // opponent's plays raise the fighter's bar instead
    // ([`battle_readout_bar_slot`]) - the party-attack-only rule above.
    if world.mode == crate::world::SceneMode::MuscleDome {
        let (attacker, _) = world.muscle_playback_tally()?;
        let name = world.minigames.muscle_dome.as_ref()?.opponent_name()?;
        return (attacker == 0).then(|| (name.to_string(), None));
    }
    let a = world.battle_ctx.active_actor;
    let pc = party_count(world) as u8;
    if a >= pc {
        return None;
    }
    let actor = world.actors.get(a as usize)?;
    let cat = actor.battle.action_category;
    if cat != ActionCategory::Attack.as_byte()
        && cat != ActionCategory::TacticalArts.as_byte()
        && cat != ActionCategory::Magic.as_byte()
        && cat != ActionCategory::Item.as_byte()
    {
        return None;
    }
    if cat == ActionCategory::Magic.as_byte()
        && ROW_PLATE_SPELL_IDS.contains(&actor.battle.params[0])
    {
        return None;
    }
    let t = actor.battle.active_target;
    if t < pc {
        return None;
    }
    let target = world.actors.get(t as usize)?;
    if target.battle.max_hp == 0 {
        return None;
    }
    Some((monster_name(world, t), monster_element_badge(world, t)))
}

/// The **target-select** plaque (placement record `0x29`): the monster name
/// the open target cursor rests on, or `None` while no
/// picker's cursor is on the enemy row.
///
/// Retail's target-cursor arm of `FUN_801D5854` (`0x801D5B28..0x801D5BAC`)
/// resolves the acting actor's target `+0x1DD`, measures that monster's name
/// payload `+0x1BC` (`FUN_80035F04`) and seats record `0x29` from it; the
/// commit arms of `FUN_801D388C` later copy that record into the commit log's
/// target column (`jal 0x801d5718` with `a1 = 0x29`, `0x801D3E64..0x801D3E70`).
/// Captured on `party_basic_attack_vs_gobu_gobu`: record `0x29` holds
/// "Gobu Gobu", width `55`, second seat `(205, 162)`.
///
/// Every port picker that can park on the enemy row is consulted: the command
/// session (Attack), the arts list, the spell list and the arts-input bar's
/// own cursor. Seat law: `legaia_engine_ui::battle_chrome::target_select_plaque_x`.
///
/// REF: FUN_801D5854 (`0x801D5B28..0x801D5BAC`)
pub fn battle_target_select_plaque(world: &crate::world::World) -> Option<(String, Option<u8>)> {
    use crate::target_picker::{CursorRow, PickerState, TargetPickerSession};
    let b = &world.battle;
    let picker: Option<&TargetPickerSession> = b
        .command
        .as_ref()
        .and_then(|c| c.picker())
        .or_else(|| b.arts_input.as_ref().and_then(|a| a.picker()))
        .or_else(|| {
            b.arts_menu.as_ref().and_then(|a| match &a.phase {
                crate::battle_arts::ArtsPhase::Targeting { picker, .. } => Some(picker),
                _ => None,
            })
        })
        .or_else(|| {
            b.spell_menu.as_ref().and_then(|s| match &s.phase {
                crate::battle_magic::SpellPhase::Targeting { picker, .. } => Some(picker),
                _ => None,
            })
        });
    let PickerState::Cursor {
        row: CursorRow::Enemy,
        slot,
    } = picker?.state()
    else {
        return None;
    };
    let pc = party_count(world) as u8;
    let t = pc.checked_add(slot)?;
    let target = world.actors.get(t as usize)?;
    if target.battle.max_hp == 0 {
        return None;
    }
    // No element badge: the captured record-0x29 width (55 for "Gobu Gobu")
    // is the bare name's advance, so retail's measured payload carries no
    // badge escape here, unlike the top-left plaque's.
    Some((monster_name(world, t), None))
}

/// The element badge a monster slot's plaque wears (`None` for none).
///
/// Retail's target plaque prints the actor's name payload `+0x1BC`, which
/// battle load copies verbatim from the record's name (`FUN_80054CB0`,
/// `0x80054D0C..0x80054D34`), so the badge is the name's own `0xCE` escape
/// (`battle_melee_hit_spark`: `CE 14 " Gimard"`, escape `0x14` = the fire
/// plate) - [`crate::monster_catalog::MonsterDef::plaque_badge`], the same
/// selector [`battle_plaque_element_badge`] reads. Indexing the strip with
/// the record's `+0x1D` element byte drew Gimard (element `2`) under the
/// wind plate, and badged monsters whose names carry no escape.
pub(super) fn monster_element_badge(world: &crate::world::World, slot: u8) -> Option<u8> {
    let actor = world.actors.get(slot as usize)?;
    let def = world.tables.monster_catalog.get(actor.battle_monster_id?)?;
    def.plaque_badge
        .filter(|b| usize::from(*b) < BATTLE_PLAQUE_BADGE_COUNT)
}

/// Character record byte the magic chip's gate reads, as an index into the
/// eight equipment bytes at `+0x196..+0x19D`: `+0x199` for every character
/// but Noa, whose arm reads `+0x198`.
pub(super) const RASERU_EQUIP_SLOT: usize = 3;

/// Noa's arm of the same gate (`char_id == 2`) reads `+0x198`.
pub(super) const RASERU_EQUIP_SLOT_NOA: usize = 2;

/// The character id `DAT_8007BD10` carries for Noa.
pub(super) const CHAR_ID_NOA: u8 = 2;

/// Does the member at battle ordinal `ordinal` carry a Ra-Seru?
///
/// Retail's per-member gate `ctx[+0x25F + member]` is written once, by the
/// party battle-actor init `FUN_80053CB8` (`0x800541D0..0x80054270`). It
/// loads the member's character id (`DAT_8007BD10[member]`), and for every
/// id but `2` reads the record's `+0x761` through the `0x80084140` display
/// alias - the live record's `+0x199`, the Ra-Seru equipment slot
/// (`0x800541EC..0x80054218`); for `2` (Noa) it reads `+0x760`, the byte
/// before (`0x80054228..0x80054258`). Either way `1` is stored when the
/// byte is non-zero. The catalogued states agree byte for byte: the gate is
/// `1` exactly for the members whose byte is set (Vahn with Meta from
/// `rim_elm_gimard_seru_capture_after` on, all three at
/// `evil_medallion_rage_battle`) and `0` for the sparring fight, for Noa in
/// `terra_party_battle` and for the zeroed `zora_glare` party.
pub fn battle_member_has_raseru(world: &crate::world::World, ordinal: u8) -> bool {
    let roster = world.party_roster_slot(ordinal as usize);
    let char_id = roster as u8 + 1;
    let slot = if char_id == CHAR_ID_NOA {
        RASERU_EQUIP_SLOT_NOA
    } else {
        RASERU_EQUIP_SLOT
    };
    world
        .party
        .roster
        .members
        .get(roster)
        .is_some_and(|m| m.equipment().slots[slot] != 0)
}

/// The command ring's right arm for the member at battle ordinal
/// `ordinal`: `(label, enabled)`.
///
/// `FUN_801D8DE8`'s case for record 10 (`0x801D8EC8..0x801D8F2C`) writes
/// the record's name pointer as `0x801F4B9E + char_id * 10` when the
/// member's gate [`battle_member_has_raseru`] is set - the character's
/// Ra-Seru (`Meta` / `Terra` / `Ozma` for `char_id` `1..=3`) - and index 4
/// of the same run, a lone `-`, when it is clear; a character past the
/// three (Terra is `char_id` 4) lands on the `-` entry. The label comes off
/// the disc (`World::battle.ui_strings`) and falls back to the port's own
/// word only when the overlay strings were not read. `enabled` is the
/// same gate: retail draws the `-` chip and refuses the arm. It is also
/// cleared while the battle's special word carries
/// [`legaia_engine_vm::battle_formulas::SPECIAL_RASERU_FORBIDDEN`] - retail
/// keeps the name, crosses the chip out (`FUN_801DBC30(0xF8, 0x42)` at
/// `0x801D12F0`, [`battle_magic_chip_mark`]) and refuses the arm
/// (`0x801D1448..0x801D1454`).
pub fn battle_magic_chip(world: &crate::world::World, ordinal: u8) -> (String, bool) {
    let has_raseru = battle_member_has_raseru(world, ordinal);
    let forbidden = battle_raseru_forbidden(world);
    let roster = world.party_roster_slot(ordinal as usize);
    let char_id = roster as u8 + 1;
    let idx = if has_raseru && (1..=3).contains(&char_id) {
        char_id
    } else {
        4
    };
    let label = world
        .battle
        .ui_strings
        .raseru_label(idx)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            if idx == 4 {
                "-".to_string()
            } else {
                crate::battle_input::BattleCommand::Magic
                    .label()
                    .to_string()
            }
        });
    (label, has_raseru && !forbidden)
}

/// The special-battle word's Ra-Seru bit is up for this battle (the Rim Elm
/// ambush, monster `0xAF`; see [`crate::world::BattleState::special_word`]).
pub fn battle_raseru_forbidden(world: &crate::world::World) -> bool {
    world.battle.special_word & legaia_engine_vm::battle_formulas::SPECIAL_RASERU_FORBIDDEN != 0
}

/// The mark the Ra-Seru chip wears in a regular battle: the red cross-out
/// ([`crate::muscle_dome::ChipMark::Forbidden`], `FUN_801DBC30`) while the
/// special word forbids it, else none. The phase-`0x28` arm draws the mark
/// before it tests the pad (`0x801D12DC..0x801D12F4`).
///
/// REF: FUN_801D0748 (the mark test; the draw-side port is
/// [`battle_raseru_cross_out`])
pub fn battle_magic_chip_mark(world: &crate::world::World) -> Option<crate::muscle_dome::ChipMark> {
    battle_raseru_forbidden(world).then_some(crate::muscle_dome::ChipMark::Forbidden)
}

/// Whether this frame draws the red cross-out X over the command ring's
/// Ra-Seru chip: the ring is up ([`CommandChipPhase::CommandRing`], retail's
/// phase `0x28`) and the special word forbids the chip
/// ([`battle_magic_chip_mark`]). Retail's arm tests the bit and calls
/// `FUN_801DBC30(0xF8, 0x42)` every frame of the phase, before it reads the
/// pad (`0x801D12DC..0x801D12F4`).
///
/// [`battle_ring_marks`] folds this into its `raseru_forbidden` field, which
/// both play hosts hand to `engine-ui`'s
/// `battle_command_ui::battle_command_menu_sprites` (native `window/hud.rs`,
/// page `play_battle.rs`); the mark draws out of the chrome atlas cell
/// `save_menu_atlas::add_cross_out_mark` bakes from the effect page.
///
/// PORT: FUN_801D0748 (phase-`0x28` arm, the `0x200` cross-out at `0x801D12DC..0x801D12F4`)
pub fn battle_raseru_cross_out(world: &crate::world::World) -> bool {
    battle_magic_chip_mark(world).is_some()
        && battle_command_chips(world).is_some_and(|c| c.phase == CommandChipPhase::CommandRing)
}

/// Every mark the command ring draws this frame: all clear unless the ring
/// is up ([`CommandChipPhase::CommandRing`], retail's phase `0x28`).
///
/// Retail's arm runs four tests every frame of the phase, before it reads
/// the pad (`0x801D12C0..0x801D1360`): the special word's `0x100` crosses the
/// Item chip out (`FUN_801DBC30(0xCC, 0x22)`), its `0x200` the Ra-Seru chip
/// (`FUN_801DBC30(0xF8, 0x42)`); the acting member's `+0x16E & 0x38 == 0x38`
/// stamps Rot on the Attack chip (`FUN_801DBD04(0xA0, 0x42)`) and its
/// `+0x16E & 0x1000` lays the Curse plate on the Magic chip
/// (`FUN_801DBEC4(0xF8, 0x42)`). The two status tests are the ones the ring
/// refuses a press on (`crate::world` `ring_arm_refused`); the member word is
/// [`crate::world::World::battle_command_status_word`], the same composed
/// word the refusal reads.
///
/// Both play hosts pass this to `engine-ui`'s
/// `battle_command_ui::battle_command_menu_sprites` (native `window/hud.rs`,
/// page `play_battle.rs`), which draws each mark out of the chrome atlas
/// cells `save_menu_atlas::add_cross_out_mark` bakes from the effect page.
///
/// PORT: FUN_801D0748 (phase-`0x28` arm, the four ring marks at `0x801D12C0..0x801D1360`)
pub fn battle_ring_marks(
    world: &crate::world::World,
) -> legaia_engine_vm::battle_party_panel::RingMarks {
    if !battle_command_chips(world).is_some_and(|c| c.phase == CommandChipPhase::CommandRing) {
        return legaia_engine_vm::battle_party_panel::RingMarks::default();
    }
    if world.mode == crate::world::SceneMode::MuscleDome {
        return world
            .minigames
            .muscle_dome
            .as_ref()
            .map(|s| s.ring_marks())
            .unwrap_or_default();
    }
    let word = world.battle.special_word;
    let status = world.battle_command_status_word().unwrap_or(0);
    legaia_engine_vm::battle_party_panel::RingMarks {
        item_forbidden: word & legaia_engine_vm::battle_formulas::SPECIAL_ARENA != 0,
        raseru_forbidden: battle_raseru_cross_out(world),
        attack_rotted: status & legaia_engine_vm::battle_formulas::ROT_ALL_LIMBS
            == legaia_engine_vm::battle_formulas::ROT_ALL_LIMBS,
        magic_cursed: status & 0x1000 != 0,
    }
}
