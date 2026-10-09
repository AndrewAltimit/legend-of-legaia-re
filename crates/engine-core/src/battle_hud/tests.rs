use super::*;
use legaia_engine_vm::status_effects::StatusEffectTracker;

#[test]
fn slot_hud_default_has_no_active_state() {
    let s = BattleSlotHud::default();
    assert!(!s.active);
    assert!(!s.is_party);
    assert!(!s.alive);
    assert_eq!(s.hp, 0);
    assert_eq!(s.hp_max, 0);
    assert_eq!(s.hp_fraction(), 0.0);
}

#[test]
fn slot_hud_fractions_clamp_to_unit_interval() {
    let mut s = BattleSlotHud::new();
    s.hp = 200;
    s.hp_max = 100; // overflow case
    assert_eq!(s.hp_fraction(), 1.0);

    s.mp = 0;
    s.mp_max = 50;
    assert_eq!(s.mp_fraction(), 0.0);
}

#[test]
fn slot_hud_status_icons_sort_by_kind_order() {
    let mut s = BattleSlotHud::new();
    s.set_status_icons([StatusKind::Faint, StatusKind::Toxic, StatusKind::Confuse]);
    assert_eq!(
        s.status_icons,
        vec![StatusKind::Toxic, StatusKind::Confuse, StatusKind::Faint]
    );
}

#[test]
fn slot_hud_status_icons_dedup_repeated_kinds() {
    let mut s = BattleSlotHud::new();
    s.set_status_icons([StatusKind::Toxic, StatusKind::Toxic, StatusKind::Sleep]);
    assert_eq!(s.status_icons, vec![StatusKind::Toxic, StatusKind::Sleep]);
}

#[test]
fn damage_popup_default_is_60_frames_no_crit() {
    let p = DamagePopup::damage(2, 100);
    assert_eq!(p.slot, 2);
    assert_eq!(p.amount, 100);
    assert_eq!(p.frames_remaining, DEFAULT_POPUP_FRAMES);
    assert_eq!(p.frames_total, DEFAULT_POPUP_FRAMES);
    assert!(!p.is_heal);
    assert!(!p.is_crit);
    assert_eq!(p.alpha(), 1.0);
}

#[test]
fn damage_popup_alpha_scales_with_remaining_frames() {
    let mut p = DamagePopup::damage(0, 50).with_lifetime(20);
    p.frames_remaining = 10;
    assert!((p.alpha() - 0.5).abs() < 1e-5);
    p.frames_remaining = 0;
    assert_eq!(p.alpha(), 0.0);
}

#[test]
fn damage_popup_with_status_carries_kind() {
    let p = DamagePopup::damage(0, 0).with_status(StatusKind::Sleep);
    assert_eq!(p.status, Some(StatusKind::Sleep));
}

#[test]
fn hud_push_damage_appends_popup_with_default_lifetime() {
    let mut h = BattleHud::new();
    h.push_damage(3, 250);
    assert_eq!(h.popups.len(), 1);
    assert_eq!(h.popups[0].slot, 3);
    assert_eq!(h.popups[0].amount, 250);
    assert_eq!(h.popups[0].frames_remaining, DEFAULT_POPUP_FRAMES);
}

#[test]
fn a_ninth_simultaneous_popup_overwrites_the_first_and_len_is_ring_bounded() {
    let mut h = BattleHud::new();
    for i in 0..9u16 {
        h.push_popup(DamagePopup::damage(0, 100 + i));
    }
    assert_eq!(
        h.popups.len(),
        POPUP_RING_SLOTS,
        "len never exceeds the 8-slot ring"
    );
    assert!(
        !h.popups.iter().any(|p| p.amount == 100),
        "the ninth push overwrites the first popup"
    );
    assert!(
        h.popups.iter().any(|p| p.amount == 108),
        "the ninth popup is present"
    );
    assert_eq!(h.popup_ring.pushed, 9, "ctx+0x273 counts every push");
    assert_eq!(h.popup_ring.cursor, 1, "ctx+0x262 wrapped past slot 0");
    for i in 0..20u16 {
        h.push_popup(DamagePopup::damage(1, 500 + i));
    }
    assert_eq!(h.popups.len(), POPUP_RING_SLOTS, "still bounded after 29");
}

#[test]
fn the_overwrite_target_is_the_cursor_slot_not_the_oldest_live_popup() {
    // Retail's cursor is independent of expiry: with eight pushed and a
    // mid-ring slot expired, the ninth push still lands on slot 0 - the
    // FIRST popup - even though a dead slot exists elsewhere.
    let mut h = BattleHud::new();
    for i in 0..8u16 {
        let life = if i == 4 { 1 } else { 60 };
        h.push_popup(DamagePopup::damage(0, 100 + i).with_lifetime(life));
    }
    h.tick(); // decrements the short-lived fifth popup to zero
    h.tick(); // drops it - slot 4 is now dead
    assert_eq!(h.popups.len(), 7);
    h.push_popup(DamagePopup::damage(0, 999));
    assert_eq!(h.popups.len(), 7, "slot 0 is replaced, not appended");
    assert!(
        !h.popups.iter().any(|p| p.amount == 100),
        "the first popup (ring slot 0) is the one overwritten"
    );
    assert!(h.popups.iter().any(|p| p.amount == 999));
}

#[test]
fn clear_popups_resets_the_ring_cursor_with_the_display_list() {
    let mut h = BattleHud::new();
    for _ in 0..5 {
        h.push_damage(0, 10);
    }
    h.clear_popups();
    assert!(h.popups.is_empty());
    assert_eq!(h.popup_ring, DamagePopupRing::default());
    h.push_damage(1, 20);
    assert_eq!(h.popup_ring.cursor, 1, "a fresh encounter starts at slot 0");
}

#[test]
fn hud_tick_decrements_and_expires_popups() {
    let mut h = BattleHud::new();
    h.push_popup(DamagePopup::damage(0, 50).with_lifetime(3));
    // Tick 1: 3 -> 2.
    h.tick();
    assert_eq!(h.popups[0].frames_remaining, 2);
    // Tick 2: 2 -> 1.
    h.tick();
    assert_eq!(h.popups[0].frames_remaining, 1);
    // Tick 3: 1 -> 0; still kept (the retain pass on this tick
    // keeps non-zero, then decrements).
    h.tick();
    // Tick 4: filter at 0 drops it.
    h.tick();
    assert!(h.popups.is_empty());
}

#[test]
fn hud_tick_keeps_popup_with_remaining_frames() {
    let mut h = BattleHud::new();
    h.push_popup(DamagePopup::damage(0, 50).with_lifetime(60));
    for _ in 0..30 {
        h.tick();
    }
    assert_eq!(h.popups.len(), 1);
    assert_eq!(h.popups[0].frames_remaining, 30);
}

#[test]
fn hud_log_drops_oldest_at_capacity() {
    let mut h = BattleHud::new();
    h.log_capacity = 3;
    h.push_log("a", LogAccent::Neutral);
    h.push_log("b", LogAccent::Neutral);
    h.push_log("c", LogAccent::Neutral);
    h.push_log("d", LogAccent::Neutral);
    assert_eq!(h.log.len(), 3);
    // Oldest "a" was dropped.
    let texts: Vec<&str> = h.log.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(texts, vec!["b", "c", "d"]);
}

#[test]
fn hud_sync_slot_populates_panel() {
    let mut h = BattleHud::new();
    let mut ap = ApGauge::with_base(8);
    ap.try_spend(3);
    h.sync_slot(
        0,
        SlotSyncInfo {
            name: "Vahn",
            is_party: true,
            alive: true,
            hp: 250,
            hp_max: 300,
            mp: 12,
            mp_max: 30,
            ap: Some(&ap),
        },
    );
    let s = &h.slots[0];
    assert!(s.active);
    assert!(s.is_party);
    assert!(s.alive);
    assert_eq!(s.name, "Vahn");
    assert_eq!(s.hp, 250);
    assert_eq!(s.hp_max, 300);
    assert_eq!(s.ap_filled, 3);
    assert_eq!(s.ap_max, 8);
}

#[test]
fn hud_sync_status_pulls_from_tracker() {
    let mut h = BattleHud::new();
    let mut tracker = StatusEffectTracker::new();
    tracker.apply(2, StatusKind::Toxic);
    tracker.apply(2, StatusKind::Venom);
    h.sync_status(2, &tracker);
    // Sorted order: Toxic (0) before Venom (2).
    assert_eq!(
        h.slots[2].status_icons,
        vec![StatusKind::Toxic, StatusKind::Venom]
    );
}

/// The status CLUT recolour reaches VRAM through the one per-slot call
/// every host already makes. Drop the `status_clut.arm(..)` line from
/// [`BattleHud::sync_status`] and this fails at the `armed()` assert -
/// the kernel is intact but nothing ever asks it to run.
#[test]
fn stone_reaches_the_party_clut_row_through_sync_status() {
    use crate::battle_status_clut::{PARTY_CLUT_ENTRIES, PARTY_CLUT_ROW_BASE};

    let mut vram = legaia_tim::Vram::new();
    let row = PARTY_CLUT_ROW_BASE + 1;
    // A resident party palette: STP-set, deliberately not grey.
    let base: Vec<u16> = (0..PARTY_CLUT_ENTRIES)
        .map(|i| 0x8000 | ((i as u16 % 31) + 1) | (0x0A << 5) | (0x1F << 10))
        .collect();
    let bytes: Vec<u8> = base.iter().flat_map(|w| w.to_le_bytes()).collect();
    vram.write_clut_row(0, row, &bytes);

    let mut h = BattleHud::new();
    let mut tracker = StatusEffectTracker::new();

    // Baseline: an ordinary ailment must not touch the palette.
    tracker.apply(1, StatusKind::Venom);
    h.sync_status(1, &tracker);
    assert!(!h.status_clut.armed());
    assert!(!h.status_clut.step(&mut vram));

    tracker.apply(1, StatusKind::Stone);
    h.sync_status(1, &tracker);
    assert!(h.status_clut.armed(), "the Stone edge arms actor +0x220");
    assert!(h.status_clut.step(&mut vram), "the pass writes VRAM");

    for x in 0..PARTY_CLUT_ENTRIES {
        let w = vram.pixel(x, row as usize);
        let (r, g, b) = (w & 0x1F, (w >> 5) & 0x1F, (w >> 10) & 0x1F);
        assert_eq!((r, g, b), (r, r, r), "entry {x} is not grey");
    }
    assert_ne!(
        (0..PARTY_CLUT_ENTRIES)
            .map(|x| vram.pixel(x, row as usize))
            .collect::<Vec<_>>(),
        base,
        "the row actually changed"
    );

    // Held affliction: no re-run, so the row is stable frame to frame.
    h.sync_status(1, &tracker);
    assert!(!h.status_clut.armed());
}

#[test]
fn hud_clear_slot_returns_panel_to_default() {
    let mut h = BattleHud::new();
    h.sync_slot(
        0,
        SlotSyncInfo {
            name: "Vahn",
            is_party: true,
            alive: true,
            hp: 100,
            hp_max: 100,
            mp: 0,
            mp_max: 0,
            ap: None,
        },
    );
    h.clear_slot(0);
    assert!(!h.slots[0].active);
    assert_eq!(h.slots[0].name, "");
}

#[test]
fn hud_iter_active_skips_inactive_slots() {
    let mut h = BattleHud::new();
    h.sync_slot(
        0,
        SlotSyncInfo {
            name: "A",
            is_party: true,
            alive: true,
            hp: 10,
            hp_max: 10,
            mp: 0,
            mp_max: 0,
            ap: None,
        },
    );
    h.sync_slot(
        2,
        SlotSyncInfo {
            name: "C",
            is_party: false,
            alive: true,
            hp: 5,
            hp_max: 5,
            mp: 0,
            mp_max: 0,
            ap: None,
        },
    );
    let visible: Vec<u8> = h.iter_active().map(|(i, _)| i).collect();
    assert_eq!(visible, vec![0, 2]);
    assert_eq!(h.active_slots(), 2);
}

#[test]
fn hud_clear_popups_drains_queue() {
    let mut h = BattleHud::new();
    h.push_damage(0, 10);
    h.push_damage(1, 20);
    h.clear_popups();
    assert!(h.popups.is_empty());
}

#[test]
fn hud_push_status_emits_zero_amount_with_status_set() {
    let mut h = BattleHud::new();
    h.push_status(0, StatusKind::Sleep);
    assert_eq!(h.popups[0].amount, 0);
    assert_eq!(h.popups[0].status, Some(StatusKind::Sleep));
}

#[test]
fn log_accent_variants_distinct() {
    // Sanity: Eq lets us use accent in renderer comparisons.
    assert_eq!(LogAccent::Neutral, LogAccent::Neutral);
    assert_ne!(LogAccent::Party, LogAccent::Monster);
}

#[test]
fn slot_hud_ap_fraction_zero_when_max_zero() {
    let s = BattleSlotHud::new();
    assert_eq!(s.ap_fraction(), 0.0);
}

#[test]
fn status_kind_letter_uses_first_char_with_collisions_lowercased() {
    assert_eq!(status_kind_letter(StatusKind::Toxic), b'T');
    assert_eq!(status_kind_letter(StatusKind::Numb), b'N');
    assert_eq!(status_kind_letter(StatusKind::Sleep), b'S');
    assert_eq!(status_kind_letter(StatusKind::Confuse), b'C');
    // Collisions take the lowercase form.
    assert_eq!(status_kind_letter(StatusKind::Curse), b'c');
    assert_eq!(status_kind_letter(StatusKind::Stone), b's');
    assert_eq!(status_kind_letter(StatusKind::Faint), b'F');
}

#[test]
fn slot_hud_status_letters_returns_one_byte_per_icon() {
    let mut s = BattleSlotHud::new();
    s.set_status_icons([StatusKind::Toxic, StatusKind::Sleep]);
    let letters = s.status_letters();
    assert_eq!(letters, vec![b'T', b'S']);
}

#[test]
fn slot_views_filters_inactive_slots() {
    let mut hud = BattleHud::new();
    hud.sync_slot(
        0,
        SlotSyncInfo {
            name: "Vahn",
            is_party: true,
            alive: true,
            hp: 100,
            hp_max: 100,
            mp: 30,
            mp_max: 30,
            ap: None,
        },
    );
    // Slot 1 untouched - should not appear.
    let views = hud.slot_views();
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].slot, 0);
    assert_eq!(views[0].name, "Vahn");
}

#[test]
fn slot_views_carries_the_single_retail_status_element() {
    let mut hud = BattleHud::new();
    hud.sync_slot(
        0,
        SlotSyncInfo {
            name: "Vahn",
            is_party: true,
            alive: true,
            hp: 100,
            hp_max: 100,
            mp: 30,
            mp_max: 30,
            ap: None,
        },
    );
    hud.sync_level(0, 12);
    hud.slots[0].set_status_icons([StatusKind::Toxic, StatusKind::Confuse]);
    let views = hud.slot_views();
    // Two kinds, ONE element: retail's ladder puts the delegation group
    // (`0x0380` -> sprite `0x1C`) above Toxic (`0x0002` -> `0x19`).
    assert_eq!(views[0].status_sprite, 0x1C);
    assert_eq!(views[0].level, 12);
}

#[test]
fn slot_status_element_is_the_packed_word_through_the_retail_ladder() {
    let mut s = BattleSlotHud {
        alive: true,
        level: 7,
        ..Default::default()
    };
    // No ailment: the base marker + the level count, not a sprite.
    assert_eq!(s.status_display_flags(), 0);
    assert_eq!(s.status_element(), StatusIcon::BaseWithCount);
    assert_eq!(s.status_sprite(), 0);

    // Venom alone packs bit 0 and selects sprite 0x18.
    s.set_status_icons([StatusKind::Venom]);
    assert_eq!(s.status_display_flags(), 0x0001);
    assert_eq!(s.status_sprite(), 0x18);

    // Adding Stone changes the *element* without changing the set order:
    // the ladder tests 0x0004 first.
    s.set_status_icons([StatusKind::Venom, StatusKind::Stone]);
    assert_eq!(s.status_display_flags(), 0x0005);
    assert_eq!(s.status_sprite(), 0x1A);

    // A KO'd slot takes the zero-HP arm whatever else is set - retail
    // tests `+0x6CE` before it inspects a bit.
    s.alive = false;
    assert_eq!(s.status_sprite(), 0x20);
}

#[test]
fn popup_views_emits_one_per_popup() {
    let mut hud = BattleHud::new();
    hud.push_damage(0, 50);
    hud.push_heal(1, 25);
    let views = hud.popup_views();
    assert_eq!(views.len(), 2);
    assert_eq!(views[0].slot, 0);
    assert_eq!(views[0].amount, 50);
    assert!(!views[0].is_heal);
    assert_eq!(views[1].slot, 1);
    assert!(views[1].is_heal);
}

#[test]
fn popup_views_carries_status_letter_when_set() {
    let mut hud = BattleHud::new();
    hud.push_status(2, StatusKind::Faint);
    let views = hud.popup_views();
    assert_eq!(views[0].status_letter, Some(b'F'));
}

#[test]
fn log_accent_color_distinct_per_variant() {
    assert_ne!(
        log_accent_color(LogAccent::Neutral),
        log_accent_color(LogAccent::Party)
    );
    assert_ne!(
        log_accent_color(LogAccent::Highlight),
        log_accent_color(LogAccent::Heal)
    );
}

/// The drawn-bar fill index follows retail's whole-gauge precedence
/// (FUN_80046A20 via `engine-vm::battle_gauge`): death first, then the
/// status override, then per-bar fill bands.
#[test]
fn gauge_fill_indices_follow_the_retail_precedence() {
    let mut s = BattleSlotHud::new();
    s.alive = true;
    s.hp = 80;
    s.hp_max = 100;
    s.mp = 5;
    s.mp_max = 40;
    // HP high band (7), MP low band (9), coloured independently.
    assert_eq!(s.gauge_fill_indices(), (7, 9));
    // Any active status forces the whole gauge to the override colour.
    s.set_status_icons([StatusKind::Toxic]);
    assert_eq!(s.gauge_fill_indices(), (3, 3));
    // Death (displayed HP zero) wins over everything.
    s.hp = 0;
    assert_eq!(s.gauge_fill_indices(), (2, 2));
    s.status_icons.clear();
    assert_eq!(s.gauge_fill_indices(), (2, 2));
}

/// Identical adjacent monsters must collapse into one dedup-labelled
/// retail row (FUN_801D9D3C via `target_picker::enemy_menu_rows`), and a
/// dead slot must contribute nothing (retail's zero id).
#[test]
fn enemy_target_rows_collapse_runs_and_skip_dead_slots() {
    use crate::monster_catalog::MonsterDef;
    use crate::world::{Actor, World};
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
    use crate::world::{Actor, World};
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

#[test]
fn log_views_resolves_color_from_accent() {
    let mut hud = BattleHud::new();
    hud.push_log("hi", LogAccent::Heal);
    let views = hud.log_views();
    assert_eq!(views[0].text, "hi");
    assert_eq!(views[0].color_rgba, log_accent_color(LogAccent::Heal));
}

// ------------------------------------------------------------------
// The per-phase rule (retail's sub-draw script + action-SM seed arms)
// ------------------------------------------------------------------

fn battle_world(party: u8) -> crate::world::World {
    use crate::monster_catalog::MonsterDef;
    use crate::world::{Actor, SceneMode, World};
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
    use crate::battle_input::BattleCommandSession;
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

/// A no-escape fight still labels the prompt's right chip `Run`: the
/// flow SM draws the static SCUS word and never reads `ctx[+0x287]`.
#[test]
fn a_no_escape_round_prompt_still_reads_run() {
    use crate::battle_input::BattleCommandSession;
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
    use crate::battle_input::BattleCommandSession;
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
    use crate::arts_command_input::{ArtsCommandInputSession, chip_icon};
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
    use crate::battle_input::BattleCommandSession;
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
    use crate::battle_input::BattleCommandSession;
    let mut w = battle_world(1);
    w.battle.command = Some(BattleCommandSession::new(0, 0));
    let mut eq = w.party.roster.members[0].equipment();
    eq.slots[RASERU_EQUIP_SLOT] = 1;
    w.party.roster.members[0].set_equipment(eq);
    assert!(battle_command_chips(&w).expect("ring chips").chips[2].1);
    assert_eq!(battle_magic_chip_mark(&w), None);
    // The Rim Elm ambush / monster 0xAF: the name stays, the chip greys
    // and wears the red cross-out.
    w.battle.special_word = legaia_engine_vm::battle_formulas::SPECIAL_RASERU_FORBIDDEN;
    let (label, enabled) = battle_command_chips(&w).expect("ring chips").chips[2].clone();
    assert_ne!(label, "-");
    assert!(!enabled);
    assert_eq!(
        battle_magic_chip_mark(&w),
        Some(crate::muscle_dome::ChipMark::Forbidden)
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
    use crate::battle_input::BattleCommandSession;
    let mut w = battle_world(1);
    w.battle.command = Some(BattleCommandSession::new(0, 0));
    assert_eq!(
        battle_ring_marks(&w),
        legaia_engine_vm::battle_party_panel::RingMarks::default()
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
        legaia_engine_vm::battle_party_panel::RingMarks {
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
        legaia_engine_vm::battle_party_panel::RingMarks::default()
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
fn attack_mode_and_targeting_park_the_bar_but_keep_tab_and_plaque() {
    use crate::battle_input::{BattleCommandSession, CommandPhase};
    let mut w = battle_world(1);
    let mut cmd = BattleCommandSession::new(0, 0);
    cmd.phase = CommandPhase::AttackMode { cursor: 0 };
    w.battle.command = Some(cmd);
    assert_eq!(battle_command_surface(&w), Some(CommandSurface::AttackMode));
    assert_eq!(battle_readout_bar_slot(&w), None);
    assert!(!battle_panels_visible(&w));
    assert!(battle_begin_tab_visible(&w));
    assert!(battle_active_actor(&w).is_some());
    assert_eq!(battle_breadcrumb_third_tab(&w).as_deref(), Some("Attack"));
    assert_eq!(battle_ring_ap_plate_value(&w), None);
    assert_eq!(
        battle_command_chips(&w).map(|c| c.phase),
        Some(CommandChipPhase::AttackMode)
    );
}

#[test]
fn item_window_shows_panels_and_its_target_step_shows_the_pointed_bar() {
    use crate::inventory_use::{
        InventoryContext, InventoryUseSession, InventoryUseState, TargetRow,
    };
    let mut w = battle_world(2);
    w.battle_ctx.active_actor = 1;
    let mut menu = InventoryUseSession::new(
        crate::items::ItemCatalog::default(),
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

fn arm_action(w: &mut crate::world::World, actor: u8, category: u8, target: u8) {
    w.battle.command = None;
    w.battle_ctx.active_actor = actor;
    w.battle_ctx.action_state = 0x20;
    let a = &mut w.actors[usize::from(actor)].battle;
    a.action_category = category;
    a.active_target = target;
    // The action's record-7 openers, as the seed and the Item pre-arm run
    // them: the seed for an Attack / Magic on a party seat, `0x3C` for a
    // party member's item.
    use legaia_engine_vm::battle_action::ActionCategory;
    let pc = w.party.party_count;
    let opened = ((category == ActionCategory::Attack.as_byte()
        || category == ActionCategory::Magic.as_byte())
        && target < pc)
        || (category == ActionCategory::Item.as_byte() && actor < pc);
    w.battle.readout_bar_glide =
        opened.then(|| legaia_engine_vm::battle_commit_log::LogLaunch::new(false));
}

#[test]
fn a_group_cast_rewritten_to_a_slot_raises_no_bar() {
    // Holy Eyes from a lone Vahn: the seed reads the party code `8` and opens
    // nothing; the band then rewrites the byte to slot `0`
    // (`evolved_0x91_midcast`), which must not raise the bar mid-cast.
    use legaia_engine_vm::battle_action::ActionCategory;
    use legaia_engine_vm::battle_cue_group::TARGET_PARTY_WIDE;
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
    use legaia_engine_vm::battle_action::ActionCategory;
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

#[test]
fn an_art_names_itself_once_the_strike_cursor_passes_its_constant() {
    use legaia_engine_vm::battle_action::ActionCategory;
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
    use legaia_engine_vm::battle_action::ActionCategory;
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
    use legaia_engine_vm::battle_action::ActionCategory;
    use legaia_engine_vm::battle_cue_group::TARGET_PARTY_WIDE;
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

/// `FUN_801E6D84`: a monster's party-wide cast raises the panels at the
/// seed; the three row-arm Seru ids raise no target plaque.
#[test]
fn the_seed_plates_follow_fun_801e6d84() {
    use legaia_engine_vm::battle_action::ActionCategory;
    use legaia_engine_vm::battle_cue_group::TARGET_PARTY_WIDE;
    let mut w = battle_world(1);
    arm_action(
        &mut w,
        3,
        ActionCategory::Magic.as_byte(),
        TARGET_PARTY_WIDE,
    );
    assert!(battle_panels_visible(&w), "monster caster, party-wide");
    arm_action(
        &mut w,
        3,
        ActionCategory::Spirit.as_byte(),
        TARGET_PARTY_WIDE,
    );
    assert!(!battle_panels_visible(&w), "spirit returns before the arm");
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
    use legaia_engine_vm::battle_action::ActionCategory;
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

#[test]
fn the_combo_cluster_counts_landed_damage_of_one_action_and_drops_with_it() {
    let mut h = BattleHud::new();
    h.arm_combo(Some(ComboStyle::HitTotal), 0);
    h.push_damage(3, 15);
    h.push_damage(3, 14);
    h.push_heal(0, 20);
    let c = h.combo.expect("cluster armed by the first hit");
    assert_eq!((c.style, c.hits, c.total), (ComboStyle::HitTotal, 2, 29));
    assert_eq!(c.age, 0);
    h.tick();
    assert_eq!(h.combo.unwrap().age, 1);
    // The next landed hit re-opens the slide from the off-screen seat
    // (`FUN_801E805C`'s `FUN_801D8DE8(0x50, 0)` on the melee kernel's
    // flag), keeping the count.
    for _ in 0..20 {
        h.tick();
    }
    assert!(h.combo.unwrap().settled());
    h.push_damage(3, 6);
    let c = h.combo.unwrap();
    assert_eq!((c.hits, c.total, c.age), (3, 35, 0));
    assert!(!c.settled());
    // The `0x51` teardown slides it back out over sixteen frames, then
    // drops it; `0x52` lies past the teardown.
    let mut closed = h.clone();
    closed.close_combo_on_fade_down(0x51, false);
    assert!(!closed.combo.unwrap().closing, "before the teardown latch");
    closed.close_combo_on_fade_down(0x51, true);
    assert_eq!(closed.combo.unwrap().slide(), 0);
    for _ in 0..8 {
        closed.tick();
    }
    assert_eq!(closed.combo.unwrap().slide(), 80);
    for _ in 0..8 {
        closed.tick();
    }
    assert!(closed.combo.is_none());
    let mut past = h.clone();
    past.close_combo_on_fade_down(0x52, false);
    assert!(past.combo.unwrap().closing);
    // Same actor, same style: the cluster keeps counting.
    h.arm_combo(Some(ComboStyle::HitTotal), 0);
    assert!(h.combo.is_some());
    // A new actor is a new cluster; no action at all tears it down.
    h.arm_combo(Some(ComboStyle::Damage), 3);
    assert!(h.combo.is_none());
    h.push_damage(0, 16);
    assert_eq!(
        h.combo.map(|c| (c.style, c.total)),
        Some((ComboStyle::Damage, 16))
    );
    h.arm_combo(None, 3);
    assert!(h.combo.is_none());
    // Without an action in flight a stray popup raises nothing.
    h.push_damage(0, 5);
    assert!(h.combo.is_none());
}

#[test]
fn subdraw_decoder_reads_a_synthetic_table() {
    let base = 0x801F_0000u32;
    let mut image = vec![0u8; 0x6000];
    // Step 1 -> record at base + 0x5000: [3][1][4] (9,0) (6,1) (7,0).
    let slot = (SUBDRAW_PTR_TABLE_VA - base) as usize + 4;
    image[slot..slot + 4].copy_from_slice(&(base + 0x5000).to_le_bytes());
    image[0x5000..0x5009].copy_from_slice(&[3, 1, 4, 9, 0, 6, 1, 7, 0]);
    let s = subdraw_step(&image, base, 1).expect("decodes");
    assert_eq!((s.anim, s.panel), (1, 4));
    assert_eq!(s.pairs, vec![(9, 0), (6, 1), (7, 0)]);
    assert!(s.shows(7) && s.shows(9));
    assert!(!s.shows(6));
    assert_eq!(s.mode_of(6), Some(1));
    assert_eq!(s.mode_of(0x52), None);
    // A null pointer / out-of-range step decodes to nothing.
    assert_eq!(subdraw_step(&image, base, 0), None);
    assert_eq!(subdraw_step(&image, base, SUBDRAW_STEP_COUNT), None);
}
