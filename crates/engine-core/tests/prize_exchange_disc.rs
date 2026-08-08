//! Disc-gated: the casino prize counters arm real sessions over the real
//! prize table.
//!
//! Two facts about the disc anchor the sub-op-7 wiring:
//!
//! 1. The prize table at PROT 899 (menu overlay) file `0x15D00` decodes to
//!    non-empty blocks - block 0 (koin1's) and block 1 (balden's) both carry
//!    live rows with non-zero coin prices.
//! 2. The counter ops the scene MANs carry (`49 07 00` at koin1, `49 07 01`
//!    at balden) arm a session over exactly those blocks through
//!    `World::try_arm_prize_exchange`.
//!
//! No Sony bytes are asserted - only counts, id ranges and the arm's
//! observable state.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset (CLAUDE.md disc-gated
//! convention).

use legaia_engine_core::world::World;
use std::path::PathBuf;

const MENU_OVERLAY_PROT_INDEX: u32 = 899;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    None
}

fn menu_overlay() -> Option<Vec<u8>> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let disc = std::env::var_os("LEGAIA_DISC_BIN")?;
    let host = match legaia_engine_core::scene::SceneHost::open_disc(&disc) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[skip] open_disc failed: {e:#}");
            return None;
        }
    };
    match host.index.entry_bytes_extended(MENU_OVERLAY_PROT_INDEX) {
        Ok(b) => Some(b),
        Err(e) => {
            eprintln!("[skip] PROT {MENU_OVERLAY_PROT_INDEX} unreadable: {e:#}");
            None
        }
    }
}

/// The whole player path on the real scene: interact with koin1's prize
/// counter NPC, drive the conversation through the production inline-script
/// runner (the same `drive_inline_dialogue` the play window ticks), and
/// assert the record's `49 07 00` op stages a prize-exchange session and
/// parks the script Armed.
///
/// Before the sub-op-7 route existed this exact drive ran the counter's
/// trigger into the generic close tick: the script resumed, the vendor's
/// outro line played, and no window ever opened - the reported symptom.
#[test]
fn koin1_prize_counter_interact_arms_the_exchange_through_the_runner() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        return;
    };
    let Some(overlay) = menu_overlay() else {
        return;
    };
    let mut host =
        legaia_engine_core::scene::SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene("koin1", 0).expect("enter koin1");
    host.world.install_menu_overlay_tables(&overlay);
    assert!(!host.world.prize_blocks.is_empty(), "prize table installed");
    host.world.use_vm_dialogue = true;

    // The prize counter is the NPC whose interaction record carries the
    // `49 07 00` counter op (P1[5]; keyed by content, not slot order).
    let mut counter_slot = None;
    for (&slot, inline) in &host.world.field_npc_dialog {
        if inline.windows(3).any(|w| w == [0x49, 0x07, 0x00]) {
            counter_slot = Some(slot);
        }
    }
    let slot = counter_slot.expect("koin1 carries the prize-counter record");

    // The prompt itself is the shape that falsified the old `0x2A = resize`
    // classification: a full 3-row box whose dispatch byte opens the
    // Yes/No menu with the box-geometry animation (`Dispatch::Picker(2)`).
    {
        let inline = &host.world.field_npc_dialog[&slot];
        let lead = inline.iter().position(|&b| b == 0x1F).unwrap();
        let bx = legaia_mes::pack_box(inline, lead).unwrap();
        assert_eq!(bx.lines.len(), 3, "the Prize Counter greeting is 3 rows");
        assert_eq!(
            bx.dispatch,
            legaia_mes::Dispatch::Picker(2),
            "the 0x2A after the prompt is the 2-option menu opener"
        );
    }

    host.world.trigger_field_interact(0, slot);
    // Drive the conversation like a player mashing Cross: an edge every few
    // ticks pages the dialogue and confirms any picker's highlighted row.
    let mut armed_at = None;
    for tick in 0..2_000u32 {
        // An edge every few ticks: pages the 3-row greeting, then confirms
        // the Yes/No menu's default row 0 ("Yes").
        let press = tick % 40 == 36;
        host.world
            .set_pad(if press { 0x4000 } else { 0 } /* PSX Cross */);
        let _ = host.world.tick();
        if host.world.prize_exchange_armed {
            armed_at = Some(tick);
            break;
        }
    }
    assert!(
        armed_at.is_some(),
        "the counter conversation never armed the prize exchange \
         (the 49 07 00 op was dropped or the record ended early)"
    );
    let session = host
        .world
        .take_pending_prize_exchange()
        .expect("a session is staged for the host drain");
    assert!(
        session.rows().count() > 0,
        "the armed session walks real visible rows"
    );
    assert!(
        host.world.prize_exchange_open,
        "the op-0x49 park reads Armed while the exchange is up"
    );
}

/// The four exchange windows (43 tab / 44 list / 45 coin counter / 46
/// confirm) carry sane on-stage rects in the disc descriptor table - the
/// rects `engine-ui::ui_prize_exchange` anchors every draw on.
#[test]
fn the_exchange_windows_rects_are_on_stage() {
    let Some(overlay) = menu_overlay() else {
        return;
    };
    let table = legaia_asset::menu_windows::parse(&overlay).expect("window table parses");
    for id in [43usize, 44, 45, 46] {
        let d = table.window(id).unwrap_or_else(|| panic!("window {id}"));
        let (x, y, w, h) = d.rect();
        assert!(
            (0..320).contains(&x) && (0..240).contains(&y),
            "window {id} rect origin off-stage: ({x}, {y})"
        );
        assert!(
            w > 0 && h > 0 && x + w <= 336 && y + h <= 256,
            "window {id} rect degenerate: ({x}, {y}, {w}, {h})"
        );
    }
}

#[test]
fn the_retail_prize_table_arms_both_counters() {
    let Some(overlay) = menu_overlay() else {
        return;
    };
    let mut w = World::new();
    w.install_menu_overlay_tables(&overlay);
    assert_eq!(
        w.prize_blocks.len(),
        legaia_engine_core::prize_exchange::PRIZE_BLOCK_COUNT,
        "the extended PROT 899 read reaches the table at 0x15D00"
    );

    // koin1's counter op arms block 0; balden's arms block 1. Both blocks
    // must be live (non-empty visible rows, coin-priced).
    for (instr, label) in [
        (&[0x49u8, 0x07, 0x00][..], "koin1 (block 0)"),
        (&[0x49u8, 0x07, 0x01][..], "balden (block 1)"),
    ] {
        let mut w2 = World::new();
        w2.prize_blocks = w.prize_blocks.clone();
        assert!(
            w2.try_arm_prize_exchange(instr),
            "{label}: the counter op must arm a session"
        );
        let session = w2.take_pending_prize_exchange().unwrap();
        let rows: Vec<_> = session.rows().collect();
        assert!(
            !rows.is_empty(),
            "{label}: the retail block walks to zero visible rows"
        );
        for r in rows {
            assert!(r.item_id != 0, "{label}: a terminator leaked into the walk");
            assert!(r.price > 0, "{label}: a zero-coin prize is not retail");
        }
    }
}
