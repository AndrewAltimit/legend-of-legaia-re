//! Pad-driven pause-menu ladder over a party that has **learned something**.
//!
//! `menu_replay` opens every pause screen by pad, but it opens them on a
//! cold new-game party: Vahn with no Seru magic and a starting bag. Three
//! routines sit behind content that party does not have, so the reach report
//! read them *live but never entered* however deep the menu ladder went
//! (the fourth row is the Magic rung carried one confirm further):
//!
//! | rung | routine | the content it needs |
//! |---|---|---|
//! | Magic | `FUN_8003053C`, the spell-record broadcast the Magic list's build runs per learned spell (`0x80031210`) | a learned spell on the record |
//! | Status -> reorder | `FUN_801DA2A0`, the list-reorder page's browse half | the same learned spell - `ListOrderSession::open` refuses an empty list, which is retail's buzz-and-stay |
//! | Items -> Use | `FUN_801DCD58`, the "learned a new art" notice window's operand patch | a Hyper Art book in the bag |
//! | Magic -> confirm | `FUN_801D9110`'s state-2 confirm dispatch (`0x801D9220..0x801D9260`), target picker vs group flow | a learned field heal, the MP to cast it, a hurt member |
//!
//! ## What is seeded, and why it is player state
//!
//! The session is booted with `BootSession::begin_new_game` exactly as
//! `menu_replay` boots it. Two writes are then made to the **party**, never
//! to a menu: one Seru spell prepended to Vahn's record through
//! `magic_xp::learn_spell_prepend` (the record-side commit a capture makes,
//! the same kernel `World::resolve_captures` calls), and one art book put in
//! the bag (what a shop purchase or a chest leaves). Everything after that is
//! a pad edge through `World::set_pad` + `BootSession::tick`.
//!
//! One host step is replayed as well: the native window's boot installs the
//! menu overlay's data tables (`World::install_menu_overlay_tables` over
//! PROT 0899, `window/run.rs`), and the headless `BootSession` does not. The
//! art notice's template is one of those tables, so without the install the
//! notice is `None` by design ("disc text the engine does not invent"). The
//! bytes are the user's disc; nothing here is synthesised.
//!
//! ## What each rung asserts
//!
//! Each rung scores the routine's **product**, with a contrast that a skipped
//! routine cannot satisfy:
//!
//! - Magic: the relevance probe decided at least one learned spell - with the
//!   disc spell table installed by the boot hook, the answer is `Some`, and
//!   the Magic session carries it (`spell_affects_nobody`) for every spell
//!   the probe refused. The contrast is a spell id the party does not know,
//!   which the session never marks.
//! - Status: Cross on the shown character swaps the sub-session for the
//!   reorder page (the `ListOrder` variant), and a Down + Cross + Cross
//!   exchange on it records a swap - the browse half's latch/exchange arm.
//! - Items: using the book inserts the art into Vahn's displayed-skill list
//!   and the session hands back a notice whose character and art id are the
//!   operands the patch wrote, with non-empty lines.
//!
//! Disc-gated (`LEGAIA_DISC_BIN` plus an extracted tree, found through
//! `LEGAIA_EXTRACTED_DIR` before the repo-relative fallbacks); skips and
//! passes without them, printing why.

use std::path::PathBuf;

use legaia_engine_core::field_menu::{FieldMenuPhase, FieldMenuRow};
use legaia_engine_core::field_menu_dispatch::FieldMenuSubsession;
use legaia_engine_core::input::PadButton;
use legaia_engine_core::world::SceneMode;
use legaia_engine_shell::boot::{BootConfig, BootSession, FieldLiveOpts};

/// A player Seru spell (`0x81..=0x8B`, `docs/formats/spell-table.md`).
const SPELL: u8 = 0x81;
/// A second one, so the reorder page has two rows to exchange.
const SPELL_2: u8 = 0x82;

fn extracted_dir() -> Option<PathBuf> {
    let over = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from);
    over.into_iter()
        .chain(
            ["extracted", "../extracted", "../../extracted"]
                .into_iter()
                .map(PathBuf::from),
        )
        .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists())
}

fn tap(s: &mut BootSession, b: PadButton) {
    s.host.world.set_pad(b.mask());
    let _ = s.tick();
    s.host.world.set_pad(0);
    let _ = s.tick();
}

fn root_cursor(s: &BootSession) -> Option<u8> {
    match s.field_menu.as_ref()?.phase() {
        FieldMenuPhase::Browsing { cursor } => Some(cursor),
        _ => None,
    }
}

/// Start edge, then walk the root cursor onto `row` and press Cross.
fn open_row(s: &mut BootSession, row: FieldMenuRow) {
    if !s.field_menu_is_open() {
        tap(s, PadButton::Start);
    }
    assert!(s.field_menu_is_open(), "Start did not open the pause menu");
    for _ in 0..FieldMenuRow::ALL.len() {
        if root_cursor(s) == Some(row.index()) {
            break;
        }
        tap(s, PadButton::Down);
    }
    assert_eq!(
        root_cursor(s),
        Some(row.index()),
        "cursor never landed on {row:?}"
    );
    tap(s, PadButton::Cross);
}

fn close_to_root(s: &mut BootSession) {
    for _ in 0..12 {
        if s.field_menu_sub.is_none() {
            return;
        }
        tap(s, PadButton::Circle);
    }
    panic!("Circle never handed the pad back to the root list");
}

/// town01 with a new-game party, one or two spells learned by Vahn, and
/// `bag` in the bag.
fn booted(spells: &[u8], bag: &[u8]) -> Option<BootSession> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing (set LEGAIA_EXTRACTED_DIR or run legaia-extract)");
        return None;
    };
    let cfg = BootConfig {
        scene: "town01".to_string(),
        enable_audio: false,
    };
    let mut s = BootSession::open(&extracted, &cfg).expect("boot session");
    s.begin_new_game();
    s.enter_field_live("town01", &FieldLiveOpts::default())
        .expect("enter town01");
    // The native host's boot step the headless session skips (see the
    // module docs): the menu overlay's data tables, off the user's disc.
    let overlay = s
        .host
        .index
        .entry_bytes_extended(legaia_asset::menu_windows::MENU_OVERLAY_PROT_INDEX as u32)
        .expect("PROT 0899");
    s.host.world.install_menu_overlay_tables(&overlay);
    // Player state: what a capture and a purchase leave behind.
    let slot = s.host.world.party_roster_slot(0);
    let rec = &mut s.host.world.party.roster.members[slot];
    // Prepend in reverse so the list reads in `spells` order.
    for &id in spells.iter().rev() {
        legaia_engine_core::magic_xp::learn_spell_prepend(rec, id);
    }
    if !spells.is_empty() {
        // A caster with Seru magic carries a Ra-Seru: magic is learned
        // through one. The record screen's list gates on that equip byte
        // (`sub15_list_len`, retail `0x8007B424 + char*2`), so without it
        // the page's exchanges are refused at the apply. The cold new-game
        // record has the slot empty because retail hands Meta over later in
        // the opening.
        let mut equip = rec.equipment();
        if equip.slots[3] == 0 {
            equip.slots[3] = 0x60;
            rec.set_equipment(equip);
        }
    }
    for &id in bag {
        let _ = s.host.world.party.inventory.add(id, 1);
    }
    assert_eq!(
        s.host.world.mode,
        SceneMode::Field,
        "town01 hands control to the field"
    );
    Some(s)
}

#[test]
fn the_magic_list_build_asks_the_broadcast_about_every_learned_spell() {
    let Some(mut s) = booted(&[SPELL], &[]) else {
        return;
    };
    let decided = legaia_engine_core::menu_validator::spell_affects_anyone(&s.host.world, SPELL);
    assert!(
        decided.is_some(),
        "the boot hook did not install the disc spell table - the relevance probe has no data"
    );
    open_row(&mut s, FieldMenuRow::Magic);
    let Some(FieldMenuSubsession::Spells(session)) = s.field_menu_sub.as_ref() else {
        panic!(
            "Magic did not open the spell screen: {:?}",
            s.field_menu_sub.as_ref().map(|x| x.row())
        );
    };
    // The session carries the probe's verdict for the learned spell ...
    assert_eq!(
        session.spell_affects_nobody(SPELL),
        decided == Some(false),
        "the Magic screen's grey state disagrees with the broadcast's answer"
    );
    // ... and marks nothing it was not asked about.
    assert!(
        !session.spell_affects_nobody(0x8B),
        "an unlearned spell was marked"
    );
    eprintln!("[magic] spell {SPELL:#04X} affects anyone on the field: {decided:?}");
    close_to_root(&mut s);
}

#[test]
fn the_status_confirm_opens_the_reorder_page_and_its_browse_half_exchanges() {
    let Some(mut s) = booted(&[SPELL, SPELL_2], &[]) else {
        return;
    };
    open_row(&mut s, FieldMenuRow::Status);
    assert!(
        matches!(s.field_menu_sub, Some(FieldMenuSubsession::Status(_))),
        "Status did not open the status screen"
    );
    // Retail's confirm arm of sub-screen 0x15's picker: a list with rows
    // opens the page instead of buzzing.
    tap(&mut s, PadButton::Cross);
    let Some(FieldMenuSubsession::ListOrder(page)) = s.field_menu_sub.as_ref() else {
        panic!("Cross on the status screen did not open the reorder page");
    };
    assert_eq!(
        page.rows().len(),
        2,
        "the page lists the two learned spells"
    );
    assert!(
        page.reorderable(),
        "the spell list's page carries the swap arm"
    );
    // Latch row 0, move to row 1, exchange.
    tap(&mut s, PadButton::Cross);
    tap(&mut s, PadButton::Down);
    tap(&mut s, PadButton::Cross);
    let Some(FieldMenuSubsession::ListOrder(page)) = s.field_menu_sub.as_ref() else {
        panic!("the reorder page closed before the exchange");
    };
    assert_eq!(
        page.swaps(),
        &[(0, 1)],
        "the latch/exchange arm recorded no swap"
    );
    close_to_root(&mut s);
    // The finished page is replayed onto the record (`apply_list_order_outcome`).
    let slot = s.host.world.party_roster_slot(0);
    let list = s.host.world.party.roster.members[slot].spell_list();
    assert_eq!(
        &list.ids[..2],
        &[SPELL_2, SPELL],
        "the exchange did not reach Vahn's record"
    );
}

#[test]
fn using_an_art_book_patches_the_learned_art_notice() {
    // Find a book that teaches roster slot 0 (class 11) off the disc's own
    // effect table, which the boot hook installed.
    let Some(probe) = booted(&[], &[]) else {
        return;
    };
    let effects = probe
        .host
        .world
        .tables
        .item_effects
        .clone()
        .expect("the boot hook installs the item-effect table");
    let book = (0u8..=255)
        .find(|&id| {
            effects.effect(id).is_some_and(|e| {
                e.class == legaia_engine_core::items::ARTS_BOOK_CLASS_BASE
                    && probe.host.world.tables.item_catalog.get(id).is_some()
            })
        })
        .expect("the effect table carries a Vahn art book");
    let tier = effects.effect(book).unwrap().tier;
    drop(probe);

    let Some(mut s) = booted(&[], &[book]) else {
        return;
    };
    let slot = s.host.world.party_roster_slot(0);
    let before = s.host.world.party.roster.members[slot].displayed_skills();
    assert!(
        !before.ids[..before.count as usize].contains(&tier),
        "Vahn already knows art {tier:#04X}; the rung would pass without the book"
    );

    open_row(&mut s, FieldMenuRow::Items);
    let Some(FieldMenuSubsession::Items(items)) = s.field_menu_sub.as_ref() else {
        panic!("Items did not open the items screen");
    };
    let row = items
        .rows
        .iter()
        .position(|r| r.id == book)
        .unwrap_or_else(|| panic!("no bag row for book {book:#04X}"));
    // Command row 0 is Use.
    tap(&mut s, PadButton::Cross);
    for _ in 0..row {
        tap(&mut s, PadButton::Down);
    }
    // The row, then whatever target / confirm the route raises.
    for _ in 0..4 {
        tap(&mut s, PadButton::Cross);
        let used = !s.host.world.party.inventory.contains_key(&book)
            || s
                .field_menu_sub
                .as_ref()
                .is_some_and(|sub| matches!(sub, FieldMenuSubsession::Items(i) if i.inner.used_item == Some(book)));
        if used {
            break;
        }
    }
    close_to_root(&mut s);

    let after = s.host.world.party.roster.members[slot].displayed_skills();
    assert!(
        after.ids[..after.count as usize].contains(&tier),
        "the book's art {tier:#04X} never reached Vahn's list"
    );
    let notice = s
        .art_learned_notice
        .clone()
        .expect("the use handed the host no learned-art notice");
    assert_eq!(notice.character, 0, "the notice names the wrong character");
    assert_eq!(notice.art_id, tier, "the notice names the wrong art");
    assert!(
        notice.lines.iter().any(|l| !l.trim().is_empty()),
        "the patched template expanded to nothing"
    );
    eprintln!(
        "[items] book {book:#04X} taught art {tier:#04X}: {:?}",
        notice.lines.len()
    );
}

/// The Magic screen's state-2 confirm dispatch (`FUN_801D9110`,
/// `0x801D9220..0x801D9260`): a confirmed field spell opens the per-member
/// target picker or, when the spell's stats `+2` byte carries `0x20`, the
/// no-pick group flow - and the cast that follows lands.
///
/// The rung the Magic test above stops short of. That one opens the list; a
/// confirm needs a spell the field can use, a caster with the MP for it and
/// a member it would affect, so this seeds all three as player state - a
/// learned healing spell, a full MP bar, and a lead hurt to half HP - and
/// then drives caster, spell and commit by pad. It scores the dispatch's
/// product twice: the phase it opened is the one the spell's own flag
/// selects, and the commit raised the hurt member's HP, which a confirm the
/// dispatch refused could not do.
#[test]
fn a_confirmed_field_spell_routes_through_the_state_2_dispatch_and_heals() {
    let Some(probe) = booted(&[], &[]) else {
        return;
    };
    let catalog = probe.host.world.tables.spell_catalog.clone();
    // Prefer the player Seru block; any field-usable heal the list can hold
    // exercises the same dispatch.
    let Some(spell) = (0x81u8..=0x8B).chain(0u8..=0xFF).find(|&id| {
        catalog.get(id).is_some_and(|d| {
            use legaia_engine_core::spells::SpellEffect;
            // A heal, so the commit has an HP delta to score.
            legaia_engine_core::spell_menu::is_field_usable(&d.effect)
                && matches!(
                    d.effect,
                    SpellEffect::Heal { .. } | SpellEffect::HealAll { .. }
                )
        })
    }) else {
        panic!("the disc spell table carries no field-usable heal");
    };
    let group = legaia_engine_core::spell_menu::spell_targets_group(
        catalog.get(spell).unwrap().target.retail_target_flag_bits(),
    );
    drop(probe);

    let Some(mut s) = booted(&[spell], &[]) else {
        return;
    };
    let slot = s.host.world.party_roster_slot(0);
    {
        let rec = &mut s.host.world.party.roster.members[slot];
        let mut hms = rec.hp_mp_sp();
        hms.hp_cur = (hms.hp_max / 2).max(1);
        hms.mp_max = hms.mp_max.max(999);
        hms.mp_cur = hms.mp_max;
        rec.set_hp_mp_sp(hms);
    }
    let hp_before = s.host.world.party.roster.members[slot].hp_mp_sp().hp_cur;

    open_row(&mut s, FieldMenuRow::Magic);
    assert!(
        matches!(s.field_menu_sub, Some(FieldMenuSubsession::Spells(_))),
        "Magic did not open the spell screen"
    );
    // Caster (Vahn, row 0), then the spell (row 0).
    tap(&mut s, PadButton::Cross);
    tap(&mut s, PadButton::Cross);
    let Some(FieldMenuSubsession::Spells(session)) = s.field_menu_sub.as_ref() else {
        panic!("the spell screen closed on the confirm");
    };
    let phase = session.phase().clone();
    use legaia_engine_core::spell_menu::SpellMenuPhase;
    match (&phase, group) {
        (SpellMenuPhase::GroupConfirm { spell_id, .. }, true)
        | (SpellMenuPhase::TargetSelect { spell_id, .. }, false) => {
            assert_eq!(*spell_id, spell, "the dispatch carried the wrong spell")
        }
        _ => panic!(
            "spell {spell:#04X} (group flag {group}) opened {phase:?} - the state-2 dispatch \
             did not route on the spell's own flag"
        ),
    }
    // Commit: the group flow takes one Cross; the picker's cursor starts on
    // row 0, the hurt lead.
    for _ in 0..3 {
        if s.host.world.party.roster.members[slot].hp_mp_sp().hp_cur > hp_before {
            break;
        }
        tap(&mut s, PadButton::Cross);
    }
    let hp_after = s.host.world.party.roster.members[slot].hp_mp_sp().hp_cur;
    assert!(
        hp_after > hp_before,
        "casting {spell:#04X} from the Magic screen left Vahn at {hp_after} HP (was {hp_before})"
    );
    eprintln!("[magic] spell {spell:#04X} group={group}: HP {hp_before} -> {hp_after}");
    close_to_root(&mut s);
}
