use super::*;
use legaia_engine_battle_menus::battle_input::CommandChipPhase;
use legaia_engine_core::world::test_drivers::BattleTestDrivers;

/// Identical adjacent monsters must collapse into one dedup-labelled
/// retail row (FUN_801D9D3C via `target_picker::enemy_menu_rows`), and a
/// dead slot must contribute nothing (retail's zero id).
#[test]
fn enemy_target_rows_collapse_runs_and_skip_dead_slots() {
    use legaia_engine_battle::monster_catalog::MonsterDef;
    use legaia_engine_core::world::{Actor, World};
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party.party_count = 1;
    w.tables
        .monster_catalog
        .insert(MonsterDef::new(7, "Gimard", 40, 5));
    w.tables
        .monster_catalog
        .insert(MonsterDef::new(9, "Zenoir", 40, 5));
    // Slots 1..=3: Gimard, Gimard, Zenoir. Slot 2's twin is dead.
    for (i, (id, hp)) in [(7u16, 40u16), (7, 40), (9, 40)].iter().enumerate() {
        let a = &mut w.actors[1 + i];
        a.battle.hp = *hp;
        a.battle.max_hp = 40;
        a.battle.liveness = 1;
        a.battle_monster_id = Some(*id);
    }
    let rows = battle_enemy_target_rows(&w);
    assert_eq!(rows.len(), 2, "the Gimard pair collapses into one row");
    assert_eq!(rows[0].first_slot, 0);
    assert_eq!(rows[0].members, 2);
    // The first twin's display name is `Gimard A`; the second member
    // drops the letter and appends the composer's `* 2`.
    assert_eq!(rows[0].label, "Gimard * 2");
    assert_eq!(rows[1].label, "Zenoir");
    assert_eq!(rows[1].first_slot, 2);

    // Kill the second Gimard: the run breaks, and the survivor keeps the
    // instance letter the seated formation gave it.
    w.actors[2].battle.hp = 0;
    let rows = battle_enemy_target_rows(&w);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].label, "Gimard A");
    assert_eq!(rows[0].members, 1);
}

/// The HUD row must carry the **ramping** HP (`BattleActor::hp_display`,
/// retail actor `+0x172` / FUN_80047430), not the live value the sim
/// already settled - otherwise the drain animation is computed every
/// frame and never shown.
#[test]
fn sync_reads_ramped_display_hp_not_live_hp() {
    use legaia_engine_core::world::{Actor, World};
    let mut w = World::new();
    while w.actors.len() < 4 {
        w.actors.push(Actor::default());
    }
    w.party.party_count = 1;
    w.actors[0].battle.hp = 100;
    w.actors[0].battle.max_hp = 200;
    w.actors[0].battle.liveness = 1;
    // Mid-ramp: live HP already at 100, bar still showing 160.
    w.actors[0].battle.hp_display = Some(160);
    // Monster slot mid-ramp too.
    w.actors[1].battle.hp = 10;
    w.actors[1].battle.max_hp = 50;
    w.actors[1].battle.liveness = 1;
    w.actors[1].battle.hp_display = Some(30);

    let mut hud = BattleHud::new();
    sync_battle_hud_rows(&mut hud, &w);
    assert_eq!(hud.slots[0].hp, 160, "party row shows the ramping bar");
    assert_eq!(hud.slots[1].hp, 30, "monster row shows the ramping bar");

    // Settled (`None`) falls back to live HP.
    w.actors[0].battle.hp_display = None;
    sync_battle_hud_rows(&mut hud, &w);
    assert_eq!(hud.slots[0].hp, 100, "settled bar reads live HP");
}

// ------------------------------------------------------------------
// The per-phase rule (retail's sub-draw script + action-SM seed arms)
// ------------------------------------------------------------------

fn battle_world(party: u8) -> legaia_engine_core::world::World {
    use legaia_engine_battle::monster_catalog::MonsterDef;
    use legaia_engine_core::world::{Actor, World};
    use legaia_engine_mode::mode::SceneMode;
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.mode = SceneMode::Battle;
    w.party.party_count = party;
    w.load_party(legaia_save::Party::zeroed(3));
    for i in 0..usize::from(party) {
        w.actors[i].battle.hp = 100;
        w.actors[i].battle.max_hp = 100;
        w.actors[i].battle.liveness = 1;
    }
    let mut gimard = MonsterDef::new(7, "Gimard", 40, 5);
    gimard.element = 2;
    // The disc name is `^A Gimard`: caret `A` = badge 0, the fire plate
    // (escape `0x14`) - not the `+0x1D` element byte's index.
    gimard.plaque_badge = Some(0);
    w.tables.monster_catalog.insert(gimard);
    w.actors[3].battle.hp = 40;
    w.actors[3].battle.max_hp = 40;
    w.actors[3].battle.liveness = 1;
    w.actors[3].battle_monster_id = Some(7);
    w
}

#[test]
fn round_prompt_is_panels_only() {
    use legaia_engine_battle_menus::battle_input::BattleCommandSession;
    let mut w = battle_world(1);
    w.battle.command = Some(BattleCommandSession::new_round_open(0, 0, false));
    assert_eq!(battle_hud_phase(&w), BattleHudPhase::RoundPrompt);
    assert!(battle_panels_visible(&w));
    assert_eq!(battle_readout_bar_slot(&w), None);
    assert_eq!(battle_active_actor(&w), None);
    assert!(!battle_begin_tab_visible(&w));
    assert_eq!(battle_ring_ap_plate_value(&w), None);
    let chips = battle_command_chips(&w).expect("prompt chips");
    assert_eq!(chips.phase, CommandChipPhase::RoundPrompt);
    assert_eq!(chips.chips.len(), 2);
}

/// Sub-draw step 0 spawns its five records at seat A: the chips on the
/// cluster centre, the panels under the screen, and no D-pad glyph until the
/// sixteen-vsync glide lands.
#[test]
fn the_round_prompt_opens_on_a_glide_from_seat_a() {
    use legaia_engine_battle_menus::battle_input::BattleCommandSession;
    let mut w = battle_world(1);
    w.battle.command = Some(BattleCommandSession::new_round_open(0, 0, false));
    // No glide recorded: everything at rest.
    assert_eq!(battle_round_prompt_panels_dy(&w), 0);
    assert_eq!(battle_chip_offsets(&w), [(0, 0); 4]);
    assert!(battle_chip_dpad_visible(&w));

    w.start_round_prompt_glide();
    // Records 0 / 3: `x = 142` to `104` / `180`. Records 6 / 0x4E / 0x4F:
    // `y = 236` to `170`.
    assert_eq!(battle_chip_offsets(&w)[..2], [(38, 0), (-38, 0)]);
    assert_eq!(battle_round_prompt_panels_dy(&w), 66);
    assert!(!battle_chip_dpad_visible(&w));

    // Half way: `FUN_801D9BBC`'s linear step.
    for _ in 0..8 {
        w.step_action_plate_glides();
    }
    assert_eq!(battle_chip_offsets(&w)[..2], [(19, 0), (-19, 0)]);
    assert_eq!(battle_round_prompt_panels_dy(&w), 33);
    assert!(!battle_chip_dpad_visible(&w));

    for _ in 0..8 {
        w.step_action_plate_glides();
    }
    assert_eq!(battle_chip_offsets(&w), [(0, 0); 4]);
    assert_eq!(battle_round_prompt_panels_dy(&w), 0);
    // The landing pass still leaves ahead of the state's arm (the stepper
    // returns the record's flag); the glyph draws from the pass after.
    assert!(!battle_chip_dpad_visible(&w));
    w.step_action_plate_glides();
    assert!(battle_chip_dpad_visible(&w));

    // The capture seat: a retail record landing on a chip's or a panel's
    // seat places the glide; the combo cluster's `(168, 168)` is not one.
    w.start_round_prompt_glide();
    assert!(!w.seat_battle_hud_glide([168, 168], 4));
    assert!(w.seat_battle_hud_glide([114, 168], 12));
    assert_eq!(battle_round_prompt_panels_dy(&w), 66 - 66 * 12 / 16);
    assert!(w.seat_battle_hud_glide([104, 86], 16));
    assert!(battle_chip_dpad_visible(&w));
}

/// Sub-draw step 1 spawns the ring's records at seat A: the four arms `168`
/// to the right, the `Begin` tab on the prompt's `Begin` seat, the plaque
/// above the top edge, the bar under the bottom one, the AP plate off the
/// right edge - and sinks the roster panels from their rest row.
#[test]
fn the_ring_opens_on_a_glide_from_seat_a() {
    use legaia_engine_battle_menus::battle_input::BattleCommandSession;
    let mut w = battle_world(1);
    w.battle.command = Some(BattleCommandSession::new(0, 0));
    assert_eq!(battle_command_surface(&w), Some(CommandSurface::Ring));
    // No glide recorded: everything at rest, the panels parked.
    assert_eq!(battle_chip_offsets(&w), [(0, 0); 4]);
    assert_eq!(battle_begin_tab_offset(&w), (0, 0));
    assert_eq!(battle_ring_ap_plate_dx(&w), 0);
    assert!(!battle_panels_visible(&w));
    assert!(battle_chip_dpad_visible(&w));

    w.start_ring_open_glide();
    // Records 9 / 0x0A / 8 / 0x0B: `x = seat + 168`. Record 1: `(104, 88)`
    // to `(16, 14)`. Record 0x1A: `y = -24` to `14`. Record 7: `236` to
    // `194`. Record 0x52: `x = 328` to `208`. Records 6 / 0x4E / 0x4F on
    // mode 1: `170` back to `236`.
    assert_eq!(battle_chip_offsets(&w), [(168, 0); 4]);
    assert_eq!(battle_begin_tab_offset(&w), (88, 74));
    assert_eq!(battle_action_plaque_dy(&w), -38);
    assert_eq!(battle_readout_bar_dy(&w), 42);
    assert_eq!(battle_ring_ap_plate_dx(&w), 120);
    assert!(
        battle_panels_visible(&w),
        "the panels are on their way down"
    );
    assert_eq!(battle_round_prompt_panels_dy(&w), 0);
    assert!(!battle_chip_dpad_visible(&w));

    // Half way: `FUN_801D9BBC`'s linear step.
    for _ in 0..8 {
        w.step_action_plate_glides();
    }
    assert_eq!(battle_chip_offsets(&w), [(84, 0); 4]);
    assert_eq!(battle_begin_tab_offset(&w), (44, 37));
    assert_eq!(battle_action_plaque_dy(&w), -19);
    assert_eq!(battle_readout_bar_dy(&w), 21);
    assert_eq!(battle_ring_ap_plate_dx(&w), 60);
    assert_eq!(battle_round_prompt_panels_dy(&w), 33);

    for _ in 0..8 {
        w.step_action_plate_glides();
    }
    assert_eq!(battle_chip_offsets(&w), [(0, 0); 4]);
    assert_eq!(battle_begin_tab_offset(&w), (0, 0));
    assert_eq!(battle_action_plaque_dy(&w), 0);
    assert_eq!(battle_readout_bar_dy(&w), 0);
    assert_eq!(battle_ring_ap_plate_dx(&w), 0);
    assert!(!battle_panels_visible(&w), "parked under the screen");
    // The landing pass leaves ahead of the ring's arm: no glyph yet.
    assert!(!battle_chip_dpad_visible(&w));
    w.step_action_plate_glides();
    assert!(battle_chip_dpad_visible(&w));

    // The capture seat: a retail record landing on an arm's, the plaque's
    // or the plate's seat places the glide.
    w.start_ring_open_glide();
    assert!(w.seat_battle_hud_glide([204, 32], 4));
    assert_eq!(battle_chip_offsets(&w)[0], (168 - 168 * 4 / 16, 0));
    assert!(w.seat_battle_hud_glide([208, 172], 16));
    assert_eq!(battle_ring_ap_plate_dx(&w), 0);
    assert!(battle_chip_dpad_visible(&w));
}

/// A no-escape fight still labels the prompt's right chip `Run`: the
/// flow SM draws the static SCUS word and never reads `ctx[+0x287]`.
#[test]
fn a_no_escape_round_prompt_still_reads_run() {
    use legaia_engine_battle_menus::battle_input::BattleCommandSession;
    let mut w = battle_world(1);
    w.battle.no_escape = true;
    w.battle.command = Some(BattleCommandSession::new_round_open(0, 0, true));
    let chips = battle_command_chips(&w).expect("prompt chips");
    assert_eq!(
        chips.chips,
        vec![("Begin".to_string(), true), ("Run".to_string(), true)]
    );
}

#[test]
fn the_ring_is_bar_tab_plaque_and_ap_plate() {
    use legaia_engine_battle_menus::battle_input::BattleCommandSession;
    let mut w = battle_world(1);
    w.actors[0].battle.spirit_gauge = 37;
    w.battle.command = Some(BattleCommandSession::new(0, 0));
    assert_eq!(battle_command_surface(&w), Some(CommandSurface::Ring));
    assert!(!battle_panels_visible(&w));
    assert_eq!(battle_readout_bar_slot(&w), Some(0));
    assert_eq!(battle_active_actor(&w).map(|(s, _)| s), Some(0));
    assert!(battle_begin_tab_visible(&w));
    assert_eq!(battle_breadcrumb_third_tab(&w), None, "no arm chosen yet");
    assert_eq!(battle_ring_ap_plate_value(&w), Some(37));
}

/// The arts entry (`0x50`) keeps the trail the attack-mode prompt built,
/// `Begin | <name> | Attack`, and its chips wear the retail words: the
/// weapon arm `Arms`, the Ra-Seru arm `RaSeru`, an empty arm its plain
/// direction word.
#[test]
fn the_arts_entry_keeps_the_attack_trail_and_names_its_arm_chips() {
    use legaia_engine_battle::arts_command_input::{ArtsCommandInputSession, chip_icon};
    let mut w = battle_world(1);
    w.battle.arts_input = Some(ArtsCommandInputSession::new(0, 0, 100, [30; 4], 0));
    assert_eq!(battle_command_surface(&w), Some(CommandSurface::ArtsInput));
    assert!(battle_begin_tab_visible(&w));
    assert_eq!(battle_active_actor(&w).map(|(s, _)| s), Some(0));
    assert_eq!(battle_breadcrumb_third_tab(&w).as_deref(), Some("Attack"));
    // Nothing equipped: both arms read their direction words.
    let view = w.arts_input_view().expect("entry view");
    assert_eq!(
        view.chip_icons,
        [
            chip_icon::LEFT,
            chip_icon::RIGHT,
            chip_icon::LOW,
            chip_icon::HIGH
        ]
    );
    let mut eq = w.party.roster.members[0].equipment();
    eq.slots[2] = 0x1B;
    eq.slots[3] = 0x09;
    w.party.roster.members[0].set_equipment(eq);
    let view = w.arts_input_view().expect("entry view");
    assert_eq!(
        view.chip_icons[0],
        chip_icon::ARMS,
        "Vahn's Left is the weapon arm"
    );
    assert_eq!(view.chip_icons[1], chip_icon::RASERU);
}

#[test]
fn the_magic_chip_reads_dash_without_a_raseru_and_the_disc_name_with_one() {
    use legaia_engine_battle_menus::battle_input::BattleCommandSession;
    let mut w = battle_world(1);
    w.battle.command = Some(BattleCommandSession::new(0, 0));
    let chips = battle_command_chips(&w).expect("ring chips");
    assert_eq!(chips.phase, CommandChipPhase::CommandRing);
    assert_eq!(chips.chips.len(), 4);
    // Ring order: Item, Attack, Magic, Spirit (`BattleCommand::MENU`).
    let (label, enabled) = &chips.chips[2];
    assert_eq!(label, "-");
    assert!(!enabled, "no Ra-Seru: the arm is the `-` chip");
    // Equip a Ra-Seru in the record's `+0x199` slot: the gate flips and
    // the label leaves the `-` entry. Without the overlay strings the
    // port's own word stands in for the disc name.
    let mut eq = w.party.roster.members[0].equipment();
    eq.slots[RASERU_EQUIP_SLOT] = 1;
    w.party.roster.members[0].set_equipment(eq);
    assert!(battle_member_has_raseru(&w, 0));
    let chips = battle_command_chips(&w).expect("ring chips");
    let (label, enabled) = &chips.chips[2];
    assert_ne!(label, "-");
    assert!(enabled);
}

#[test]
fn the_raseru_forbidden_bit_greys_the_chip_and_marks_it() {
    use legaia_engine_battle_menus::battle_input::BattleCommandSession;
    let mut w = battle_world(1);
    w.battle.command = Some(BattleCommandSession::new(0, 0));
    let mut eq = w.party.roster.members[0].equipment();
    eq.slots[RASERU_EQUIP_SLOT] = 1;
    w.party.roster.members[0].set_equipment(eq);
    assert!(battle_command_chips(&w).expect("ring chips").chips[2].1);
    assert_eq!(battle_magic_chip_mark(&w), None);
    // The Rim Elm ambush / monster 0xAF: the name stays, the chip greys
    // and wears the red cross-out.
    w.battle.special_word = legaia_engine_battle_vm::battle_formulas::SPECIAL_RASERU_FORBIDDEN;
    let (label, enabled) = battle_command_chips(&w).expect("ring chips").chips[2].clone();
    assert_ne!(label, "-");
    assert!(!enabled);
    assert_eq!(
        battle_magic_chip_mark(&w),
        Some(legaia_engine_battle_menus::muscle_dome_session::ChipMark::Forbidden)
    );
    assert!(battle_raseru_cross_out(&w), "the ring is up: the X draws");
    // Off the ring (no command session) the arm does not run.
    w.battle.command = None;
    assert!(!battle_raseru_cross_out(&w));
}

/// The ring's four marks, each off its own test: the special word's two
/// bits, then the acting member's Rot limbs (all three, not any) and
/// Curse - and nothing at all off the ring.
#[test]
fn the_ring_marks_follow_the_word_and_the_members_status() {
    use legaia_engine_battle_menus::battle_input::BattleCommandSession;
    let mut w = battle_world(1);
    w.battle.command = Some(BattleCommandSession::new(0, 0));
    assert_eq!(
        battle_ring_marks(&w),
        legaia_engine_battle_vm::battle_party_panel::RingMarks::default()
    );
    w.actors[0].battle.field_flags = 0x18;
    assert!(
        !battle_ring_marks(&w).attack_rotted,
        "two limbs leave a direction"
    );
    w.actors[0].battle.field_flags = 0x38 | 0x1000;
    w.battle.special_word = 0x300;
    assert_eq!(
        battle_ring_marks(&w),
        legaia_engine_battle_vm::battle_party_panel::RingMarks {
            item_forbidden: true,
            raseru_forbidden: true,
            attack_rotted: true,
            magic_cursed: true,
        }
    );
    // The round prompt is not the ring: no marks.
    w.battle.command = Some(BattleCommandSession::new_round_open(0, 0, true));
    assert_eq!(
        battle_ring_marks(&w),
        legaia_engine_battle_vm::battle_party_panel::RingMarks::default()
    );
}

#[test]
fn noa_gate_reads_the_byte_before_everyone_elses() {
    // `FUN_80053CB8`'s `beq v0,a3` arm: character id 2 reads `+0x198`,
    // every other id `+0x199`.
    let mut w = battle_world(2);
    let mut eq = w.party.roster.members[1].equipment();
    eq.slots[RASERU_EQUIP_SLOT] = 1;
    w.party.roster.members[1].set_equipment(eq);
    assert!(
        !battle_member_has_raseru(&w, 1),
        "Noa's gate does not read +0x199"
    );
    let mut eq = w.party.roster.members[1].equipment();
    eq.slots[RASERU_EQUIP_SLOT] = 0;
    eq.slots[RASERU_EQUIP_SLOT_NOA] = 1;
    w.party.roster.members[1].set_equipment(eq);
    assert!(battle_member_has_raseru(&w, 1), "Noa's gate reads +0x198");
    // Vahn's arm is unaffected by the +0x198 byte.
    let mut eq = w.party.roster.members[0].equipment();
    eq.slots[RASERU_EQUIP_SLOT_NOA] = 1;
    w.party.roster.members[0].set_equipment(eq);
    assert!(!battle_member_has_raseru(&w, 0));
}

#[test]
fn attack_mode_keeps_bar_tab_plaque_and_ap_plate() {
    use legaia_engine_battle_menus::battle_input::{BattleCommandSession, CommandPhase};
    let mut w = battle_world(1);
    let mut cmd = BattleCommandSession::new(0, 0);
    cmd.phase = CommandPhase::AttackMode { cursor: 0 };
    w.battle.command = Some(cmd);
    assert_eq!(battle_command_surface(&w), Some(CommandSurface::AttackMode));
    assert_eq!(battle_readout_bar_slot(&w), Some(0));
    assert!(!battle_panels_visible(&w));
    assert!(battle_begin_tab_visible(&w));
    assert!(battle_active_actor(&w).is_some());
    assert_eq!(battle_breadcrumb_third_tab(&w).as_deref(), Some("Attack"));
    assert!(battle_ring_ap_plate_value(&w).is_some());
    assert_eq!(
        battle_command_chips(&w).map(|c| c.phase),
        Some(CommandChipPhase::AttackMode)
    );
}

#[test]
fn item_window_shows_panels_and_its_target_step_shows_the_pointed_bar() {
    use legaia_engine_menus::inventory_use::{
        InventoryContext, InventoryUseSession, InventoryUseState, TargetRow,
    };
    let mut w = battle_world(2);
    w.battle_ctx.active_actor = 1;
    let mut menu = InventoryUseSession::new(
        legaia_engine_menus::items::ItemCatalog::default(),
        Vec::new(),
        vec![TargetRow::new(0, "Vahn"), TargetRow::new(1, "Noa")],
        InventoryContext::Battle,
    );
    w.battle.item_menu = Some(menu.clone());
    assert_eq!(battle_command_surface(&w), Some(CommandSurface::ItemBrowse));
    assert!(
        battle_panels_visible(&w),
        "browsing: the panels come back up"
    );
    assert_eq!(
        battle_readout_bar_slot(&w),
        None,
        "browsing: the bar is parked"
    );
    assert!(
        !battle_begin_tab_visible(&w),
        "the item window draws its own trail"
    );
    assert_eq!(battle_ring_ap_plate_value(&w), None);
    assert_eq!(battle_command_chips(&w), None);

    menu.state = InventoryUseState::TargetSelect {
        item_cursor: 0,
        cursor: 0,
    };
    w.battle.item_menu = Some(menu);
    assert_eq!(
        battle_command_surface(&w),
        Some(CommandSurface::ItemTarget(Some(0)))
    );
    assert!(!battle_panels_visible(&w), "target step: the panels park");
    assert_eq!(
        battle_readout_bar_slot(&w),
        Some(0),
        "target step: the bar names the pointed member, not the actor"
    );
}

fn arm_action(w: &mut legaia_engine_core::world::World, actor: u8, category: u8, target: u8) {
    w.battle.command = None;
    w.battle_ctx.active_actor = actor;
    w.battle_ctx.action_state = 0x20;
    let a = &mut w.actors[usize::from(actor)].battle;
    a.action_category = category;
    a.active_target = target;
    // The action's record-7 openers, as the seed and the Item pre-arm run
    // them: the seed for an Attack / Magic on a party seat, `0x3C` for a
    // party member's item.
    use legaia_engine_battle_vm::battle_action::ActionCategory;
    let pc = w.party.party_count;
    let opened = ((category == ActionCategory::Attack.as_byte()
        || category == ActionCategory::Magic.as_byte())
        && target < pc)
        || (category == ActionCategory::Item.as_byte() && actor < pc);
    w.battle.readout_bar_glide =
        opened.then(|| legaia_engine_battle_vm::battle_commit_log::LogLaunch::new(false));
}

#[test]
fn a_group_cast_rewritten_to_a_slot_raises_no_bar() {
    // Holy Eyes from a lone Vahn: the seed reads the party code `8` and opens
    // nothing; the band then rewrites the byte to slot `0`
    // (`evolved_0x91_midcast`), which must not raise the bar mid-cast.
    use legaia_engine_battle_vm::battle_action::ActionCategory;
    use legaia_engine_battle_vm::battle_cue_group::TARGET_PARTY_WIDE;
    let mut w = battle_world(1);
    arm_action(
        &mut w,
        0,
        ActionCategory::Magic.as_byte(),
        TARGET_PARTY_WIDE,
    );
    w.actors[0].battle.active_target = 0;
    w.battle_ctx.action_state = 0x34;
    assert_eq!(battle_readout_bar_slot(&w), None);
}

#[test]
fn a_party_attack_on_a_monster_shows_plaque_target_plaque_and_no_readout() {
    use legaia_engine_battle_vm::battle_action::ActionCategory;
    let mut w = battle_world(1);
    arm_action(&mut w, 0, ActionCategory::Attack.as_byte(), 3);
    assert_eq!(battle_hud_phase(&w), BattleHudPhase::Action);
    assert_eq!(battle_active_actor(&w).map(|(s, _)| s), Some(0));
    assert_eq!(
        battle_readout_bar_slot(&w),
        None,
        "no party participant: no bar"
    );
    assert!(!battle_panels_visible(&w));
    // The badge is the name's caret letter (`^A` -> cell 0, the Fire
    // badge), not the element byte (`2`, which is the Wind cell).
    assert_eq!(
        battle_target_plaque(&w),
        Some(("Gimard".to_string(), Some(0))),
        "the name's own badge (fire), not the element byte's strip index"
    );
    // A name with no escape wears no badge (`Skeleton A`).
    let mut bare = w.tables.monster_catalog.get(7).unwrap().clone();
    bare.plaque_badge = None;
    w.tables.monster_catalog.insert(bare);
    assert_eq!(battle_target_plaque(&w), Some(("Gimard".to_string(), None)));
    assert_eq!(battle_combo_style(&w), Some(ComboStyle::HitTotal));
    assert!(!battle_begin_tab_visible(&w));
    assert_eq!(battle_ring_ap_plate_value(&w), None);
    // No art has committed yet: a plain swing carries no move name.
    assert_eq!(battle_move_name(&w), None);
}

/// The Done band's teardown wipes the handle list and re-spawns the target
/// plaque on its closing glide: it slides from its row back under the
/// screen over sixteen vsyncs and is then gone until the next action's seed.
#[test]
fn the_done_teardown_slides_the_target_plaque_out_and_drops_it() {
    use legaia_engine_battle_vm::battle_action::ActionCategory;
    use legaia_engine_battle_vm::battle_cue_group::{HUD_CASTER_BANNER, HUD_TARGET_BANNER};
    let mut w = battle_world(1);
    arm_action(&mut w, 0, ActionCategory::Attack.as_byte(), 3);
    w.note_action_plate_raise(HUD_CASTER_BANNER, 0);
    w.note_action_plate_raise(HUD_TARGET_BANNER, 0);
    w.land_battle_hud_glides();
    assert!(battle_target_plaque(&w).is_some());
    assert_eq!(battle_target_plaque_dy(&w), 0);

    // The teardown: the plaque's unload (the wipe), then the sweep's
    // `FUN_801D8DE8(0x51, 1)`.
    w.note_action_plate_raise(HUD_CASTER_BANNER, 1);
    assert!(battle_target_plaque(&w).is_none(), "wiped with the list");
    w.note_action_plate_raise(HUD_TARGET_BANNER, 1);
    assert!(battle_target_plaque(&w).is_some(), "re-spawned, closing");
    assert_eq!(battle_target_plaque_dy(&w), 0);
    for _ in 0..8 {
        w.step_action_plate_glides();
    }
    // Record 0x51: `y = 194` back to `236`.
    assert_eq!(battle_target_plaque_dy(&w), 21);
    for _ in 0..8 {
        w.step_action_plate_glides();
    }
    assert!(battle_target_plaque(&w).is_none(), "landed off screen");

    // The next action's seed raises it afresh.
    w.note_action_plate_raise(HUD_CASTER_BANNER, 0);
    w.note_action_plate_raise(HUD_TARGET_BANNER, 0);
    assert!(battle_target_plaque(&w).is_some());
}

#[test]
fn an_art_names_itself_once_the_strike_cursor_passes_its_constant() {
    use legaia_engine_battle_vm::battle_action::ActionCategory;
    let mut w = battle_world(1);
    arm_action(&mut w, 0, ActionCategory::Attack.as_byte(), 3);
    // Any Vahn art constant the curated table names.
    let (byte, name) = (0x1Bu8..=0x40)
        .find_map(|b| {
            let c = legaia_art::ActionConstant::from_byte(b)?;
            legaia_art::tables::art_name(legaia_art::Character::Vahn, c).map(|n| (b, n))
        })
        .expect("a Vahn art");
    {
        let a = &mut w.actors[0].battle;
        a.params[0] = 0x0D;
        a.params[1] = byte;
        a.params[2] = 0x27;
        a.strike_index = 1;
    }
    assert_eq!(battle_move_name(&w), None, "the art's byte is still ahead");
    w.actors[0].battle.strike_index = 2;
    assert_eq!(battle_move_name(&w).as_deref(), Some(name));
}

#[test]
fn a_monster_cast_on_a_member_shows_that_member_bar_and_damage_style() {
    use legaia_engine_battle_vm::battle_action::ActionCategory;
    let mut w = battle_world(1);
    arm_action(&mut w, 3, ActionCategory::Magic.as_byte(), 0);
    assert_eq!(battle_active_actor(&w), Some((3, "Gimard".into())));
    assert_eq!(
        battle_readout_bar_slot(&w),
        Some(0),
        "the target's bar rises"
    );
    assert_eq!(
        battle_target_plaque(&w),
        None,
        "monster actions carry no target plaque"
    );
    assert_eq!(battle_combo_style(&w), Some(ComboStyle::Damage));
    assert!(!battle_panels_visible(&w));
}

#[test]
fn a_party_item_shows_the_actor_bar_and_a_party_wide_item_shows_the_panels() {
    use legaia_engine_battle_vm::battle_action::ActionCategory;
    use legaia_engine_battle_vm::battle_cue_group::TARGET_PARTY_WIDE;
    let mut w = battle_world(2);
    arm_action(&mut w, 1, ActionCategory::Item.as_byte(), 0);
    assert_eq!(
        battle_readout_bar_slot(&w),
        Some(1),
        "item: the acting member's bar"
    );
    assert!(!battle_panels_visible(&w));
    arm_action(&mut w, 1, ActionCategory::Item.as_byte(), TARGET_PARTY_WIDE);
    w.battle_ctx.action_state = 0x3C;
    assert!(
        !battle_panels_visible(&w),
        "a party-wide item pre-arms through 0x3C without the panels"
    );
    assert_eq!(battle_readout_bar_slot(&w), Some(1));
    w.battle_ctx.action_state = 0x3E;
    assert!(
        battle_panels_visible(&w),
        "the item band's 0x3E arm raises all the panels"
    );
    w.battle_ctx.action_state = 0x51;
    assert!(battle_panels_visible(&w), "up through the Done hold");
    w.battle_ctx.action_state = 0x52;
    assert!(!battle_panels_visible(&w), "the Done hold closed them");
    arm_action(
        &mut w,
        1,
        ActionCategory::Magic.as_byte(),
        TARGET_PARTY_WIDE,
    );
    assert!(
        !battle_panels_visible(&w),
        "the magic seed opens no panels (orb_summon_mid_cast)"
    );
    assert_eq!(battle_readout_bar_slot(&w), None);
}

/// PROT 0911 (Orb)'s arm 9 raises the panels itself: they glide up from
/// seat A, take the readout bar's seat, hold through the Done band and go
/// with the next action's seed.
#[test]
fn a_module_panel_raise_takes_the_bars_seat() {
    use legaia_engine_battle_vm::battle_action::ActionCategory;
    use legaia_engine_battle_vm::battle_cue_group::HUD_CASTER_BANNER;
    let mut w = battle_world(1);
    arm_action(&mut w, 0, ActionCategory::Magic.as_byte(), 0);
    w.battle_ctx.action_state = 0x36;
    assert_eq!(battle_readout_bar_slot(&w), Some(0), "the seed's bar");
    assert!(!battle_panels_visible(&w));
    w.raise_module_roster_panels();
    assert!(battle_panels_visible(&w), "arm 9's raise");
    assert_eq!(battle_readout_bar_slot(&w), None, "the panel took the seat");
    assert_eq!(battle_round_prompt_panels_dy(&w), 236 - 170, "from seat A");
    for _ in 0..16 {
        w.step_action_plate_glides();
    }
    assert_eq!(battle_round_prompt_panels_dy(&w), 0, "landed");
    w.battle_ctx.action_state = 0x51;
    assert!(battle_panels_visible(&w), "up through the Done hold");
    w.battle_ctx.action_state = 0x52;
    assert!(!battle_panels_visible(&w), "the Done hold closed them");
    w.battle_ctx.action_state = 0x36;
    w.note_action_plate_raise(HUD_CASTER_BANNER, 0);
    assert!(!battle_panels_visible(&w), "the next seed clears the raise");
}

/// `FUN_801E6D84`: a monster's party-wide cast raises the panels at the
/// seed; the three row-arm Seru ids raise no target plaque.
#[test]
fn the_seed_plates_follow_fun_801e6d84() {
    use legaia_engine_battle_vm::battle_action::ActionCategory;
    use legaia_engine_battle_vm::battle_cue_group::TARGET_PARTY_WIDE;
    let mut w = battle_world(1);
    arm_action(
        &mut w,
        3,
        ActionCategory::Magic.as_byte(),
        TARGET_PARTY_WIDE,
    );
    assert!(battle_panels_visible(&w), "monster caster, party-wide");
    // A capture-class special runs its module in the capture band, whose
    // state bytes sit above the Done band's but which hands off into it
    // (`0x70` exits to `0x50`): the seed's panels hold through it.
    for s in 0x6E..=0x71 {
        w.battle_ctx.action_state = s;
        assert!(battle_panels_visible(&w), "capture band {s:#x}");
    }
    arm_action(
        &mut w,
        3,
        ActionCategory::Spirit.as_byte(),
        TARGET_PARTY_WIDE,
    );
    assert!(!battle_panels_visible(&w), "spirit returns before the arm");
    // The plaque keys on the target byte alone: a monster's cast on a
    // monster (Ultra Charge on its own caster) opens it, a monster's cast on
    // a party member does not.
    arm_action(&mut w, 3, ActionCategory::Magic.as_byte(), 3);
    w.actors[3].battle.params[0] = 0xA5;
    assert!(battle_target_plaque(&w).is_some(), "monster on monster");
    arm_action(&mut w, 3, ActionCategory::Magic.as_byte(), 0);
    assert_eq!(battle_target_plaque(&w), None, "monster on the party");
    arm_action(&mut w, 0, ActionCategory::Magic.as_byte(), 3);
    w.actors[0].battle.params[0] = 0x85;
    assert!(battle_target_plaque(&w).is_some(), "single-target Nighto");
    for id in ROW_PLATE_SPELL_IDS {
        w.actors[0].battle.params[0] = id;
        assert_eq!(battle_target_plaque(&w), None, "{id:#x}");
    }
}

#[test]
fn run_shows_no_plaque_and_idle_shows_nothing() {
    use legaia_engine_battle_vm::battle_action::ActionCategory;
    let mut w = battle_world(1);
    arm_action(&mut w, 0, ActionCategory::Run.as_byte(), 3);
    assert_eq!(battle_active_actor(&w), None);
    assert_eq!(battle_combo_style(&w), None);
    w.battle_ctx.action_state = 0x0A;
    assert_eq!(battle_hud_phase(&w), BattleHudPhase::Idle);
    assert_eq!(battle_active_actor(&w), None);
    assert_eq!(battle_readout_bar_slot(&w), None);
    assert!(!battle_panels_visible(&w));
    assert_eq!(battle_target_plaque(&w), None);
    assert_eq!(battle_move_name(&w), None);
}

#[test]
fn action_bands_in_flight_exclude_the_holds() {
    for s in [0x00u8, 0x0A, 0x0B, 0x5A, 0xFF] {
        assert!(!battle_action_in_flight(s), "{s:#x}");
    }
    for s in [0x0C_u8, 0x1E, 0x20, 0x2B, 0x51, 0x52, 0x64, 0x6F] {
        assert!(battle_action_in_flight(s), "{s:#x}");
    }
}
