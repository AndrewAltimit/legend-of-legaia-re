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
//! The session is booted with `NativeSession::begin_new_game` exactly as
//! `menu_replay` boots it. Two writes are then made to the **party**, never
//! to a menu: one Seru spell prepended to Vahn's record through
//! `magic_xp::learn_spell_prepend` (the record-side commit a capture makes,
//! the same kernel `World::resolve_captures` calls), and one art book put in
//! the bag (what a shop purchase or a chest leaves). Everything after that is
//! a pad edge through `World::set_pad` + `NativeSession::tick`.
//!
//! One host step is replayed as well: the native window's boot installs the
//! menu overlay's data tables (`World::install_menu_overlay_tables` over
//! PROT 0899, `window/run.rs`), and the headless `NativeSession` does not. The
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

use legaia_engine_menu_runtime::bag_rows::WorldBagRows;
use legaia_engine_menu_runtime::field_menu_dispatch::FieldMenuSubsession;
use legaia_engine_menus::field_menu::{FieldMenuPhase, FieldMenuRow};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_engine_system::input::PadButton;

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

fn tap(s: &mut NativeSession, b: PadButton) {
    s.host.world.set_pad(b.mask());
    let _ = s.tick();
    s.host.world.set_pad(0);
    let _ = s.tick();
    // A Start that opened the menu: wait out the field's wipe until the menu
    // exists (`NativeSession::pause_wipe`).
    for _ in 0..64 {
        if s.pause_wipe().menu_spawned() {
            break;
        }
        let _ = s.tick();
    }
}

fn root_cursor(s: &NativeSession) -> Option<u8> {
    match s.field_menu.as_ref()?.phase() {
        FieldMenuPhase::Browsing { cursor } => Some(cursor),
        _ => None,
    }
}

/// Start edge, then walk the root cursor onto `row` and press Cross.
fn open_row(s: &mut NativeSession, row: FieldMenuRow) {
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

fn close_to_root(s: &mut NativeSession) {
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
fn booted(spells: &[u8], bag: &[u8]) -> Option<NativeSession> {
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
    let mut s = NativeSession::open(&extracted, &cfg).expect("boot session");
    s.begin_new_game();
    s.enter_field_live("town01", &FieldLiveOpts::default())
        .expect("enter town01");
    // The native host's boot step the headless session skips (see the
    // module docs): the menu overlay's data tables, off the user's disc.
    let overlay = s
        .host
        .index
        .entry_bytes_extended(legaia_game_tables::menu_windows::MENU_OVERLAY_PROT_INDEX as u32)
        .expect("PROT 0899");
    s.host.world.install_menu_overlay_tables(&overlay);
    // Player state: what a capture and a purchase leave behind.
    let slot = s.host.world.party_roster_slot(0);
    let rec = &mut s.host.world.party.roster.members[slot];
    // Prepend in reverse so the list reads in `spells` order.
    for &id in spells.iter().rev() {
        legaia_engine_battle::magic_xp::learn_spell_prepend(rec, id);
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
    let decided =
        legaia_engine_menu_runtime::menu_validator::spell_affects_anyone(&s.host.world, SPELL);
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

/// Meta, the winged-plate Ra-Seru spell (`0xCE 0x1D`, 28 px plate).
const RA_SERU_SPELL: u8 = 0x9E;

/// The Magic screen draws each spell's element plate in front of its name,
/// the way retail's list kernel does by drawing the raw `name_ptr` string
/// (`FUN_8002FF8C` -> `FUN_80036888` at `0x80033738`).
///
/// The screen is opened by pad over a party that learned Gimard (fire
/// plate, escape `0x14`, 20 px) and Meta (winged plate, escape `0x1D`,
/// 28 px), then composed through the shared `engine-screens` builder both
/// play hosts call, with the session's own disc font (escape sprites
/// attached at boot). Each row must carry an untinted plate cell at the
/// row pen (window 18's content x `174` + `0xC`) and its name one plate
/// width plus a space further on - `+25` and `+33`, the offsets the
/// `sol_to_karisto_worldmap` retail capture measures.
#[test]
fn the_magic_list_draws_each_spells_element_plate() {
    let Some(mut s) = booted(&[SPELL, RA_SERU_SPELL], &[]) else {
        return;
    };
    open_row(&mut s, FieldMenuRow::Magic);
    // Into the list: the info window stages the hovered spell too.
    tap(&mut s, PadButton::Cross);
    let Some(sub) = s.field_menu_sub.as_ref() else {
        panic!("Magic did not open a sub-session");
    };
    let font = s
        .dialog_font
        .clone()
        .expect("the boot source decodes the dialog font");
    assert!(
        font.escape_cell(0x14).is_some() && font.escape_cell(0x1D).is_some(),
        "the boot font carries no element-plate escape sprites"
    );
    // The menu sprite cells both play hosts attach at boot (the native
    // window's `attach_menu_font_cells`, the play page's `play_menu`
    // assets), off the same PROT.DAT head bytes.
    let system_ui = s
        .host
        .index
        .prot_dat_raw_bytes(
            legaia_engine_menus::save_menu_atlas::SYSTEM_UI_CLUT_EXT_TIM_OFFSET as u64,
            legaia_ui_formats::title_pak::OVERLAY_LOAD_EMPTY_FRAME_TIM_OFFSET
                + legaia_ui_formats::title_pak::OVERLAY_LOAD_EMPTY_FRAME_TIM_SIZE
                - legaia_engine_menus::save_menu_atlas::SYSTEM_UI_CLUT_EXT_TIM_OFFSET,
        )
        .expect("system-UI sheet bytes");
    let font = font.with_sprite_cells(&legaia_engine_menus::save_menu_atlas::menu_font_cells(
        &system_ui, None,
    ));
    let ctx = legaia_engine_ui::pause_menu::PauseMenuCtx {
        font: &font,
        rects: legaia_engine_ui::pause_menu::MenuRects::new(None),
        chrome: None,
        origin: (0, 0),
        scale: 1,
    };
    let draws = legaia_engine_screens::pause_screens::field_menu_sub_draws(
        &ctx,
        sub,
        Some(&s.host.world),
        None,
    );
    let pen_x = 174 + 0x0C;
    let plate = |w: u32| {
        draws
            .texts
            .iter()
            .find(|d| d.dst.0 == pen_x && d.dst.2 == w && d.dst.3 == 12)
            .map(|d| (d.dst.1, d.color))
    };
    let (fire_y, fire_ink) = plate(20).expect("Gimard's fire plate draws at the row pen");
    let (wing_y, _) = plate(28).expect("Meta's winged plate draws at the row pen");
    assert_eq!(
        fire_ink[..3],
        [1.0, 1.0, 1.0],
        "a plate keeps its own colours"
    );
    assert_ne!(fire_y, wing_y, "one plate per row");
    let glyph_at = |x: i32, y: i32| draws.texts.iter().any(|d| d.dst.0 == x && d.dst.1 == y);
    assert!(
        glyph_at(pen_x + 25, fire_y),
        "Gimard's name follows its plate"
    );
    assert!(
        glyph_at(pen_x + 33, wing_y),
        "Meta's name follows its plate"
    );
    // The PAGE header is the kernel's sprite header: the 24x8 tag at
    // `(174 + 132 - 0x38, 22 - 2)` and the page digit at
    // `174 + 132 - 0x20 + 6`, the columns the retail capture of this screen
    // shows.
    let cell = |x: i32, w: u32| {
        draws
            .texts
            .iter()
            .any(|d| d.dst.0 == x && d.dst.1 == 20 && d.dst.2 == w && d.dst.3 == 8)
    };
    assert!(cell(250, 24), "the PAGE tag sprite");
    assert!(cell(280, 6), "the page digit sprite");
    eprintln!("[magic] plates at rows y={fire_y} / y={wing_y}");
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
                e.class == legaia_engine_menus::items::ARTS_BOOK_CLASS_BASE
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
            use legaia_engine_battle::spells::SpellEffect;
            // A heal, so the commit has an HP delta to score.
            legaia_engine_menus::spell_menu::is_field_usable(&d.effect)
                && matches!(
                    d.effect,
                    SpellEffect::Heal { .. } | SpellEffect::HealAll { .. }
                )
        })
    }) else {
        panic!("the disc spell table carries no field-usable heal");
    };
    let group = legaia_engine_menus::spell_menu::spell_targets_group(
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
    use legaia_engine_menus::spell_menu::SpellMenuPhase;
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

/// The Items and Magic lists ink each row the way the kind-4 list kernel
/// does (`FUN_80032A44`, `0x8003312C..` / `0x8003351C..`): every row white
/// while the list is parked behind the command / caster window, and once
/// the hand is in it a row grey only when its build-time word carries the
/// dim bit - not every row.
///
/// The party is seeded so each list holds both kinds: a hurt lead with full
/// MP, a learned field heal (castable, so white) beside Gimard (an attack,
/// greyed in the field), and a Healing Leaf (applies to the hurt lead) beside
/// a bag item the Use build dims. The screens are composed through the
/// shared `engine-screens` builder both play hosts call.
#[test]
fn list_rows_take_their_build_time_ink_once_the_hand_is_in_the_list() {
    let Some(mut probe) = booted(&[], &[]) else {
        return;
    };
    let catalog = probe.host.world.tables.spell_catalog.clone();
    let heal = (0x81u8..=0xA0)
        .find(|&id| {
            catalog.get(id).is_some_and(|d| {
                use legaia_engine_battle::spells::SpellEffect;
                matches!(
                    d.effect,
                    SpellEffect::Heal { .. } | SpellEffect::HealAll { .. }
                )
            })
        })
        .expect("the disc spell table carries a field heal");
    // A bag id the Use list build dims: not field-usable (or equipment).
    const LEAF: u8 = 0x77;
    let dimmed = (0x1Au8..=0xFF)
        .filter(|&id| id != LEAF)
        .find(|&id| {
            let w = &mut probe.host.world;
            w.party.inventory.clear();
            let _ = w.party.inventory.add(id, 1);
            w.bag_use_rows(false)
                .is_some_and(|rows| rows.len() == 1 && rows[0].dim && !rows[0].alt_ink)
        })
        .expect("some bag item dims in the field Use list");
    drop(probe);

    let Some(mut s) = booted(&[heal, SPELL], &[LEAF, dimmed]) else {
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
    let font = s.dialog_font.clone().expect("disc dialog font");
    // Row `i`'s name ink in the list window (content pen `(174, 22)`, rows
    // from `+0xC` at the `0xE` pitch, name at `+0xC`).
    let row_inks = |s: &NativeSession, n: usize| -> Vec<[f32; 4]> {
        let ctx = legaia_engine_ui::pause_menu::PauseMenuCtx {
            font: &font,
            rects: legaia_engine_ui::pause_menu::MenuRects::new(None),
            chrome: None,
            origin: (0, 0),
            scale: 1,
        };
        let sub = s.field_menu_sub.as_ref().expect("a sub-screen is open");
        let draws = legaia_engine_screens::pause_screens::field_menu_sub_draws(
            &ctx,
            sub,
            Some(&s.host.world),
            None,
        );
        (0..n)
            .map(|i| {
                let y = 22 + 0x0C + 0x0E * i as i32;
                draws
                    .texts
                    .iter()
                    .filter(|d| d.dst.1 == y && d.dst.0 >= 174 + 0x0C && d.dst.0 < 174 + 0x70)
                    .find(|d| d.color[..3] != [1.0, 1.0, 1.0])
                    .map(|d| d.color)
                    .unwrap_or_else(|| panic!("no name ink on list row {i}"))
            })
            .collect()
    };
    let white = legaia_engine_ui::MENU_TEXT_WHITE;
    let grey = legaia_engine_ui::MENU_TEXT_GREY;

    // Magic: caster focus parks the list - both rows white.
    open_row(&mut s, FieldMenuRow::Magic);
    assert_eq!(
        row_inks(&s, 2),
        [white, white],
        "a parked spell list is all white"
    );
    // Into the list: the heal stays white, the attack greys.
    tap(&mut s, PadButton::Cross);
    assert_eq!(
        row_inks(&s, 2),
        [white, grey],
        "list focus inks heal {heal:#04X} / attack {SPELL:#04X} by their build-time words"
    );
    close_to_root(&mut s);

    // Items: the command window parks the list; Use opens it.
    open_row(&mut s, FieldMenuRow::Items);
    let names: Vec<u8> = match s.field_menu_sub.as_ref() {
        Some(FieldMenuSubsession::Items(items)) => items.rows.iter().map(|r| r.id).collect(),
        _ => panic!("Items did not open the item screen"),
    };
    assert_eq!(names.len(), 2);
    assert_eq!(
        row_inks(&s, 2),
        [white, white],
        "a parked bag list is all white"
    );
    tap(&mut s, PadButton::Cross);
    let want: Vec<[f32; 4]> = names
        .iter()
        .map(|&id| if id == LEAF { white } else { grey })
        .collect();
    assert_eq!(
        row_inks(&s, 2),
        want,
        "Use-list inks for Healing Leaf / item {dimmed:#04X} (rows {names:02X?})"
    );
    eprintln!("[ink] heal {heal:#04X} white, attack grey; leaf white, item {dimmed:#04X} grey");
    close_to_root(&mut s);
}

/// The Incense confirm (window 12, `FUN_801D1F10`) draws the route's own
/// two prompt lines out of the menu overlay's string pool - the first at
/// the window pen with its `0xC2` token spliced to the item's disc name,
/// the second `0xC` right and one row down - not port-composed text.
#[test]
fn the_incense_confirm_draws_the_overlay_prompt_lines() {
    const INCENSE: u8 = 0x8A;
    let Some(mut s) = booted(&[], &[INCENSE]) else {
        return;
    };
    assert!(
        s.host.world.menu.use_route_prompts.is_installed(),
        "the menu overlay install read no Use-route prompts"
    );
    open_row(&mut s, FieldMenuRow::Items);
    // Use -> the list -> the hand onto the Incense row (the new-game bag
    // already holds other items) -> its confirm window.
    tap(&mut s, PadButton::Cross);
    let row = match s.field_menu_sub.as_ref() {
        Some(FieldMenuSubsession::Items(items)) => items
            .rows
            .iter()
            .position(|r| r.id == INCENSE)
            .expect("Incense is in the bag"),
        _ => panic!("Items did not open the item screen"),
    };
    for _ in 0..row {
        tap(&mut s, PadButton::Down);
    }
    tap(&mut s, PadButton::Cross);
    let Some(FieldMenuSubsession::Items(items)) = s.field_menu_sub.as_ref() else {
        panic!("Items did not open the item screen");
    };
    assert!(
        items.special_use().is_some(),
        "the Incense row did not open its confirm route: focus {:?}, rows {:?}, blocked {}",
        items.focus,
        items
            .rows
            .iter()
            .map(|r| (r.id, r.use_bits))
            .collect::<Vec<_>>(),
        items.incense_blocked()
    );
    let font = s.dialog_font.clone().expect("disc dialog font");
    let ctx = legaia_engine_ui::pause_menu::PauseMenuCtx {
        font: &font,
        rects: legaia_engine_ui::pause_menu::MenuRects::new(None),
        chrome: None,
        origin: (0, 0),
        scale: 1,
    };
    let draws = legaia_engine_screens::pause_screens::field_menu_sub_draws(
        &ctx,
        s.field_menu_sub.as_ref().unwrap(),
        Some(&s.host.world),
        None,
    );
    let w = &s.host.world;
    let name = legaia_engine_menu_runtime::field_menu_dispatch::item_display_text(w, INCENSE).name;
    let lines = w.menu.use_route_prompts.lines(
        legaia_engine_menus::pause_screens::UseRoute::Incense,
        |t, op| {
            (t == 0xC2).then(|| {
                legaia_engine_menu_runtime::field_menu_dispatch::item_display_text(w, op).name
            })
        },
    );
    assert_eq!(lines.len(), 2, "window 12 carries two prompt lines");
    assert!(
        lines[0].contains(&name),
        "the first line splices the Incense name ({name:?}): {:?}",
        lines[0]
    );
    let (px, py) = ctx.rects.pen(12);
    let glyph_at = |x: i32, y: i32| draws.texts.iter().any(|d| d.dst.0 == x && d.dst.1 == y);
    // Each line starts with the pool's blank `@` (width 0, a one-pixel
    // advance), at the pen and at `+0xC`; the first visible glyph follows
    // one pixel on.
    assert!(
        glyph_at(px, py) && glyph_at(px + 1, py),
        "line 1 at the window pen"
    );
    assert!(
        glyph_at(px + 0x0C, py + 0x0E) && glyph_at(px + 0x0D, py + 0x0E),
        "line 2 at (+0xC, +0xE)"
    );
    eprintln!(
        "[incense] prompt lines: {} / {} chars",
        lines[0].len(),
        lines[1].len()
    );
}

/// A party holding casino coins opens the pause menu with the money box's
/// coin row (`FUN_801DC6B4` state 0 raises system flag `8`; `FUN_801D0148`
/// tests it): the box moves up one row to `y = 166` and draws gold, coins
/// and the clock at the 14-px pitch - the rows a retail capture of a coin
/// holder's root menu shows (value ink from y 168 / 182 / 196). Without
/// coins the box sits at `y = 180` with two rows.
#[test]
fn a_coin_holder_s_root_menu_shows_the_coin_row() {
    let Some(mut s) = booted(&[], &[]) else {
        return;
    };
    let flag = legaia_engine_menus::field_menu::SYSTEM_FLAG_CASINO_COIN_ROW;
    let font = s.dialog_font.clone().expect("disc dialog font");
    let value_rows = |s: &NativeSession| -> Vec<i32> {
        let ctx = legaia_engine_ui::pause_menu::PauseMenuCtx {
            font: &font,
            rects: legaia_engine_ui::pause_menu::MenuRects::new(None),
            chrome: None,
            origin: (0, 0),
            scale: 1,
        };
        let root = s.field_menu.as_ref().expect("the pause menu is open");
        let draws = legaia_engine_screens::pause_screens::field_menu_root_draws(
            &ctx,
            root,
            &s.host.world,
            None,
        );
        // The box's value columns: right of the pictograms, left of the
        // party panel.
        let ys: std::collections::BTreeSet<i32> = draws
            .texts
            .iter()
            .filter(|d| (24 + 0x20..128).contains(&d.dst.0) && d.dst.1 >= 150)
            .map(|d| d.dst.1)
            .collect();
        ys.into_iter().collect()
    };

    tap(&mut s, PadButton::Start);
    assert!(s.field_menu_is_open(), "Start did not open the pause menu");
    assert!(!s.host.world.system_flag_test(flag), "no coins, no flag");
    assert_eq!(
        value_rows(&s),
        [180, 194],
        "gold + clock at the retail y = 180 box"
    );
    tap(&mut s, PadButton::Triangle);
    for _ in 0..8 {
        if !s.field_menu_is_open() {
            break;
        }
        tap(&mut s, PadButton::Circle);
    }
    assert!(!s.field_menu_is_open(), "the menu did not close");

    s.host.world.minigames.casino_coins = 42;
    tap(&mut s, PadButton::Start);
    assert!(
        s.field_menu_is_open(),
        "Start did not reopen the pause menu"
    );
    assert!(
        s.host.world.system_flag_test(flag),
        "the menu open raised the coin-row flag"
    );
    assert_eq!(value_rows(&s), [166, 180, 194], "gold, coins, clock");
}

/// The Throw Out confirm (window 9, `FUN_801D1B20`) draws the overlay's own
/// labels: the question line at `(WX + 6, WY + 0xE)` and Yes / No at
/// `WX + 0x3C`. Each pool string keeps its `@` lead, a blank width-0 glyph,
/// so the first visible glyph sits one pixel past each pen, as retail
/// draws it.
#[test]
fn the_throw_out_confirm_draws_the_overlay_labels() {
    let Some(mut s) = booted(&[], &[]) else {
        return;
    };
    assert!(
        s.host.world.menu.throw_confirm_labels.is_installed(),
        "the menu overlay install read no Throw Out labels"
    );
    open_row(&mut s, FieldMenuRow::Items);
    // Command window: Down to Throw Out, Cross into its list, Cross on the
    // first stack (the new-game bag holds one).
    tap(&mut s, PadButton::Down);
    tap(&mut s, PadButton::Cross);
    tap(&mut s, PadButton::Cross);
    let Some(FieldMenuSubsession::Items(items)) = s.field_menu_sub.as_ref() else {
        panic!("Items did not open the item screen");
    };
    assert_eq!(
        items.focus,
        legaia_engine_menus::pause_screens::PauseItemsFocus::ThrowOutConfirm,
        "the Throw Out pick did not open its confirm"
    );
    let font = s.dialog_font.clone().expect("disc dialog font");
    let ctx = legaia_engine_ui::pause_menu::PauseMenuCtx {
        font: &font,
        rects: legaia_engine_ui::pause_menu::MenuRects::new(None),
        chrome: None,
        origin: (0, 0),
        scale: 1,
    };
    let draws = legaia_engine_screens::pause_screens::field_menu_sub_draws(
        &ctx,
        s.field_menu_sub.as_ref().unwrap(),
        Some(&s.host.world),
        None,
    );
    let (wx, wy) = ctx.rects.pen(9);
    let glyph_at = |x: i32, y: i32| draws.texts.iter().any(|d| d.dst.0 == x && d.dst.1 == y);
    for (x, y, what) in [
        (wx + 6, wy + 0x0E, "the question"),
        (wx + 0x3C, wy + 0x1C, "Yes"),
        (wx + 0x3C, wy + 0x2A, "No"),
    ] {
        assert!(
            glyph_at(x, y) && glyph_at(x + 1, y),
            "{what}: the `@` at the pen, the first visible glyph one pixel on"
        );
    }
}

/// A menu heal that crosses its magic-XP threshold raises window 7
/// (`FUN_801DCCB4`) with the menu overlay's own sentence: its `0xC3`
/// spell-name token spliced to the spell's raw display name, element plate
/// escape first. Composed through the shared notice builder over the disc
/// window table, the plate draws untinted at the window pen.
#[test]
fn a_menu_heal_level_up_draws_the_overlay_sentence_with_the_plate() {
    let Some(probe) = booted(&[], &[]) else {
        return;
    };
    let catalog = probe.host.world.tables.spell_catalog.clone();
    let spell = (0x81u8..=0x8B)
        .find(|&id| {
            catalog.get(id).is_some_and(|d| {
                use legaia_engine_battle::spells::SpellEffect;
                legaia_engine_menus::spell_menu::is_field_usable(&d.effect)
                    && matches!(d.effect, SpellEffect::Heal { .. })
            })
        })
        .expect("a single-target field heal");
    drop(probe);
    let Some(mut s) = booted(&[spell], &[]) else {
        return;
    };
    let thresholds = s
        .host
        .world
        .tables
        .magic_xp_thresholds
        .expect("the boot installs the disc magic-XP table");
    let template = s
        .host
        .world
        .menu
        .spell_level_template
        .clone()
        .expect("the overlay install read window 7's sentence");
    let slot = s.host.world.party_roster_slot(0);
    {
        let rec = &mut s.host.world.party.roster.members[slot];
        let mut hms = rec.hp_mp_sp();
        hms.hp_cur = (hms.hp_max / 2).max(1);
        hms.mp_max = hms.mp_max.max(999);
        hms.mp_cur = hms.mp_max;
        rec.set_hp_mp_sp(hms);
        // One cast short of level 2: the accumulator sits on the level-1
        // entry, which the test passes only once exceeded.
        let at = legaia_engine_battle::magic_xp::spell_slot(rec, spell).expect("learned");
        legaia_engine_battle::magic_xp::add_spell_xp(rec, at, u32::from(thresholds[0]));
    }
    open_row(&mut s, FieldMenuRow::Magic);
    tap(&mut s, PadButton::Cross); // caster
    tap(&mut s, PadButton::Cross); // spell -> target picker
    for _ in 0..4 {
        if s.spell_level_notice.is_some() {
            break;
        }
        tap(&mut s, PadButton::Cross);
    }
    let notice = s
        .spell_level_notice
        .clone()
        .expect("the cast leveled the spell and raised window 7");
    let names = s
        .host
        .world
        .menu
        .text
        .as_ref()
        .unwrap()
        .spell_names
        .as_ref()
        .unwrap();
    let op = names
        .icon(spell)
        .expect("a Seru spell name opens with a plate");
    let mut head = vec![0xCE, op, b' '];
    head.extend(names.name(spell).unwrap().bytes());
    assert!(
        notice.line.starts_with(&head),
        "the line opens with the raw spell name, plate first"
    );
    assert_eq!(
        &notice.line[head.len()..],
        &template[2..],
        "the rest is the overlay's own sentence"
    );

    let overlay = s
        .host
        .index
        .entry_bytes_extended(legaia_game_tables::menu_windows::MENU_OVERLAY_PROT_INDEX as u32)
        .expect("PROT 0899");
    let table = legaia_game_tables::menu_windows::parse(&overlay).expect("menu window table");
    let font = s.dialog_font.clone().expect("disc dialog font");
    let ctx = legaia_engine_ui::pause_menu::PauseMenuCtx {
        font: &font,
        rects: legaia_engine_ui::pause_menu::MenuRects::new(Some(&table)),
        chrome: None,
        origin: (0, 0),
        scale: 1,
    };
    let draws = legaia_engine_ui::pause_menu::spell_level_notice_draws(&ctx, &notice.line);
    let (px, py) = ctx.rects.pen(7);
    let plate = draws
        .iter()
        .find(|d| d.dst.0 == px && d.dst.2 == 20 && d.dst.3 == 12)
        .expect("the plate draws at window 7's pen");
    assert_eq!(plate.dst.1, py, "on the line's row");
    assert_eq!(plate.color[..3], [1.0, 1.0, 1.0]);
}

/// The Equip screen's weapon candidate list is retail's armament build
/// (`FUN_80030628` cases `7`..`10` / `0x15`..`0x18`): the Remove verb first,
/// the worn weapon second when there is one, then the bag's accepted slots
/// in slot order - not id order. The bag is seeded with two weapons Vahn can
/// wield, the higher id first, and the list is opened by pad.
#[test]
fn the_weapon_candidate_list_is_remove_worn_then_bag_order() {
    let Some(probe) = booted(&[], &[]) else {
        return;
    };
    let stats = probe
        .host
        .world
        .tables
        .equip_stats
        .clone()
        .expect("the boot parses the equipment stat table");
    let info = legaia_engine_menus::equipment::DiscEquipInfo::from_disc(&stats);
    let mut weapons: Vec<u8> = (1u8..=0xFF)
        .filter(|&id| {
            info.category(id) == Some(legaia_game_tables::equip_stats::EquipSlot::Weapon)
                && info.can_equip(id, 0)
        })
        .collect();
    drop(probe);
    weapons.sort_unstable();
    let (low, high) = (weapons[0], weapons[weapons.len() - 1]);
    assert!(low < high, "two distinct Vahn weapons on the disc");
    // Bag order: the higher id first.
    let Some(mut s) = booted(&[], &[high, low]) else {
        return;
    };
    open_row(&mut s, FieldMenuRow::Equip);
    tap(&mut s, PadButton::Cross); // Vahn
    tap(&mut s, PadButton::Down); // row 1: the weapon slot
    tap(&mut s, PadButton::Cross); // its candidate list
    let Some(FieldMenuSubsession::Equip { session, .. }) = s.field_menu_sub.as_ref() else {
        panic!("Equip did not open the equip screen");
    };
    let legaia_engine_menus::equip_session::EquipState::ItemPicker { slot, .. } = session.state()
    else {
        panic!(
            "the weapon row did not open its candidate list: {:?}",
            session.state()
        );
    };
    let rows = session.items_for_slot(slot);
    assert_eq!(
        rows[0].id,
        legaia_engine_menus::equip_session::REMOVE_ROW_ID,
        "Remove leads"
    );
    let worn = session.record().equip[slot as usize];
    let mut at = 1;
    if worn != 0 {
        assert!(
            rows[1].equipped && rows[1].id == worn,
            "the worn weapon second"
        );
        at = 2;
    }
    let bag: Vec<u8> = rows[at..].iter().map(|r| r.id).collect();
    let hi = bag
        .iter()
        .position(|&id| id == high)
        .expect("the high-id weapon is listed");
    let lo = bag
        .iter()
        .position(|&id| id == low)
        .expect("the low-id weapon is listed");
    assert!(hi < lo, "bag order, not id order: {bag:02X?}");
}

/// The pause menu's root rows, the Items tab and its command window draw the
/// menu overlay's own pool strings (`0x801CE9D0` and siblings), `@` lead
/// included: the blank lead at each pen, the first visible glyph one pixel
/// on - where a retail capture of the root menu inks its rows (`WX + 0x15`).
/// Composed over the disc window table, which carries the pool labels.
#[test]
fn the_root_rows_tab_and_item_commands_are_the_overlay_pool_strings() {
    use legaia_game_tables::menu_pool::MenuLabel;
    use legaia_game_tables::menu_windows::window_ids;
    let Some(mut s) = booted(&[], &[]) else {
        return;
    };
    let overlay = s
        .host
        .index
        .entry_bytes_extended(legaia_game_tables::menu_windows::MENU_OVERLAY_PROT_INDEX as u32)
        .expect("PROT 0899");
    let table = legaia_game_tables::menu_windows::parse(&overlay).expect("menu window table");
    assert!(
        table.labels.is_installed(),
        "the table read the pool labels"
    );
    for l in MenuLabel::ROOT_ROWS {
        assert!(
            table.labels.get(l).is_some_and(|t| t.starts_with('@')),
            "{l:?} keeps its @ lead"
        );
    }
    let font = s.dialog_font.clone().expect("disc dialog font");
    let ctx = legaia_engine_ui::pause_menu::PauseMenuCtx {
        font: &font,
        rects: legaia_engine_ui::pause_menu::MenuRects::new(Some(&table)),
        chrome: None,
        origin: (0, 0),
        scale: 1,
    };
    let at = |draws: &[legaia_engine_ui::TextDraw], x: i32, y: i32| {
        draws.iter().any(|d| d.dst.0 == x && d.dst.1 == y)
    };

    tap(&mut s, PadButton::Start);
    let root = legaia_engine_screens::pause_screens::field_menu_root_draws(
        &ctx,
        s.field_menu.as_ref().expect("the pause menu is open"),
        &s.host.world,
        None,
    );
    let (lx, ly) = ctx.rects.pen(window_ids::TOP_COMMAND_LIST);
    for row in 0..7 {
        let y = ly + row * 14;
        assert!(
            at(&root.texts, lx + 0x14, y) && at(&root.texts, lx + 0x15, y),
            "root row {row}: the @ at the pen, its first glyph one pixel on"
        );
    }

    open_row(&mut s, FieldMenuRow::Items);
    let items = legaia_engine_screens::pause_screens::field_menu_sub_draws(
        &ctx,
        s.field_menu_sub.as_ref().expect("the Items screen is open"),
        Some(&s.host.world),
        None,
    );
    let (tx, ty) = ctx.rects.pen(window_ids::TAB_ITEMS);
    assert!(
        at(&items.texts, tx, ty) && at(&items.texts, tx + 1, ty),
        "the Items tab draws the pool string with its lead"
    );
    let (cx, cy) = ctx.rects.pen(window_ids::ITEMS_COMMAND);
    for row in 0..3 {
        let y = cy + row * 14;
        assert!(
            at(&items.texts, cx + 0x14, y) && at(&items.texts, cx + 0x15, y),
            "command row {row} is the pool's @Use / @Throw Out / @Arrange"
        );
    }
}
