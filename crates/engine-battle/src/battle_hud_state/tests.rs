use super::*;
use legaia_engine_battle_vm::battle_gauge_rearm::POPUP_RING_SLOTS;

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
/// (FUN_80046A20 via `engine-battle-vm::battle_gauge`): death first, then the
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

#[test]
fn log_views_resolves_color_from_accent() {
    let mut hud = BattleHud::new();
    hud.push_log("hi", LogAccent::Heal);
    let views = hud.log_views();
    assert_eq!(views[0].text, "hi");
    assert_eq!(views[0].color_rgba, log_accent_color(LogAccent::Heal));
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
