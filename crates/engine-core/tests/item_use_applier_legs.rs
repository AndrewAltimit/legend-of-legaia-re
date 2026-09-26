//! The two `FUN_800402F4` arms `World::use_item` now runs: the Hyper-Art
//! **book** insert (selectors `0x0B`..`0x0D`) and the **Point Card** discharge
//! (selector `0x0E`).
//!
//! Disc-free. The item-effect table both legs read is assembled here as a
//! synthetic `PS-X EXE` image, so the tests can put a class-`14` descriptor in
//! front of an item id - something the retail disc never does (decoding all 256
//! static item rows through the effect descriptors finds no class-`14` row, see
//! `docs/formats/item-effect-table.md`).

use legaia_asset::item_effect::{
    HEAL_AMOUNT_TABLE_VA, ItemEffectTable, RECORD_COUNT, RECORD_STRIDE, TABLE_VA,
};
use legaia_engine_core::items::ItemOutcome;
use legaia_engine_core::world::World;

/// `0x80074368` - the static item record table (12-byte stride, `+0` kind,
/// `+1` subtype).
const ITEM_TABLE_VA: u32 = 0x8007_4368;
const ITEM_RECORD_STRIDE: u32 = 0x0C;
/// Enough of the data segment to cover all three tables the parser reads.
const SEG_BASE: u32 = ITEM_TABLE_VA;
const SEG_LEN: u32 = 0x8007_6570 - ITEM_TABLE_VA;

/// One `(item id, kind, class, tier, flags)` row to stamp into the synthetic
/// image. Each id gets its own descriptor subtype, so the rows never collide.
struct Row {
    id: u8,
    kind: u8,
    class: u8,
    tier: u8,
    flags: u8,
}

/// Build a `PS-X EXE`-shaped buffer whose item + effect tables carry `rows`.
fn synth_scus(rows: &[Row]) -> Vec<u8> {
    let mut buf = vec![0u8; 0x800 + SEG_LEN as usize];
    buf[0..8].copy_from_slice(b"PS-X EXE");
    buf[0x18..0x1C].copy_from_slice(&SEG_BASE.to_le_bytes());
    buf[0x1C..0x20].copy_from_slice(&SEG_LEN.to_le_bytes());
    let off = |va: u32| (va - SEG_BASE) as usize + 0x800;

    // Every id defaults to subtype 0; descriptor 0 stays all-zero (class 0,
    // an HP heal of tier 0), which is what an untouched id resolves to.
    for (i, row) in rows.iter().enumerate() {
        let subtype = (i + 1) as u8;
        assert!((subtype as usize) < RECORD_COUNT, "subtype fits the table");
        let rec = off(ITEM_TABLE_VA + u32::from(row.id) * ITEM_RECORD_STRIDE);
        buf[rec] = row.kind;
        buf[rec + 1] = subtype;
        let d = off(TABLE_VA + subtype as u32 * RECORD_STRIDE as u32);
        buf[d] = row.class;
        buf[d + 1] = row.tier;
        buf[d + 2] = row.flags;
    }
    // A plausible heal-amount table so the parser's third read succeeds.
    for tier in 0..3u32 {
        let hp = off(HEAL_AMOUNT_TABLE_VA + tier * 2);
        buf[hp..hp + 2].copy_from_slice(&200u16.to_le_bytes());
    }
    buf
}

fn table(rows: &[Row]) -> ItemEffectTable {
    ItemEffectTable::from_scus(&synth_scus(rows)).expect("synthetic SCUS parses")
}

/// Retail's three book classes, with the field-only flag byte the shipped
/// descriptors carry (`0x80` base | `0x02` field | `0x01` not discardable).
const BOOK_FLAGS: u8 = 0x83;

fn book_rows() -> Vec<Row> {
    vec![
        // Fire Book I / Wind Book I / Thunder Book I, at their retail ids,
        // classes and tiers.
        Row {
            id: 0x8F,
            kind: 2,
            class: 11,
            tier: 3,
            flags: BOOK_FLAGS,
        },
        Row {
            id: 0x92,
            kind: 2,
            class: 12,
            tier: 5,
            flags: BOOK_FLAGS,
        },
        Row {
            id: 0x95,
            kind: 2,
            class: 13,
            tier: 3,
            flags: BOOK_FLAGS,
        },
    ]
}

fn world_with(rows: &[Row], members: usize) -> World {
    let mut world = World::new();
    world.load_party(legaia_save::Party::zeroed(members));
    world.set_item_effects(table(rows));
    world.set_item_catalog(legaia_engine_core::items::ItemCatalog::vanilla());
    world
}

fn skills(world: &World, slot: usize) -> Vec<u8> {
    let list = world.party.roster.members[slot].displayed_skills();
    list.ids[..list.count as usize].to_vec()
}

/// The book's **class** picks the record, not the target the player selected:
/// Fire Book I used on party ordinal 2 still writes roster slot 0.
#[test]
fn an_arts_book_writes_the_class_slot_and_ignores_the_picked_target() {
    let mut world = world_with(&book_rows(), 3);
    let out = world.use_item(0x8F, 2);
    assert_eq!(
        out,
        ItemOutcome::ArtLearned {
            character: 0,
            art_id: 3,
            position: 0
        }
    );
    assert_eq!(skills(&world, 0), vec![3]);
    assert!(skills(&world, 1).is_empty());
    assert!(skills(&world, 2).is_empty());
}

/// Class 11 / 12 / 13 map to roster slots 0 / 1 / 2, and each line inserts its
/// own descriptor tier as the art id.
#[test]
fn the_three_book_classes_reach_three_different_records() {
    let mut world = world_with(&book_rows(), 3);
    for (id, slot, art) in [(0x8Fu8, 0usize, 3u8), (0x92, 1, 5), (0x95, 2, 3)] {
        let out = world.use_item(id, 0);
        assert_eq!(
            out,
            ItemOutcome::ArtLearned {
                character: slot as u8,
                art_id: art,
                position: 0
            },
            "item {id:#04x}"
        );
        assert_eq!(skills(&world, slot), vec![art]);
    }
}

/// The insert is ordered ascending, which the capture in
/// `docs/subsystems/level-up.md` (a list holding `0x0C` receiving `0x03`)
/// cannot distinguish from a head insert - but three books in descending tier
/// order can.
#[test]
fn repeated_books_keep_the_list_ascending() {
    let rows = vec![
        Row {
            id: 0x8F,
            kind: 2,
            class: 11,
            tier: 3,
            flags: BOOK_FLAGS,
        },
        Row {
            id: 0x90,
            kind: 2,
            class: 11,
            tier: 2,
            flags: BOOK_FLAGS,
        },
        Row {
            id: 0x91,
            kind: 2,
            class: 11,
            tier: 1,
            flags: BOOK_FLAGS,
        },
    ];
    let mut world = world_with(&rows, 3);
    assert!(matches!(
        world.use_item(0x90, 0),
        ItemOutcome::ArtLearned { art_id: 2, .. }
    ));
    assert!(matches!(
        world.use_item(0x8F, 0),
        ItemOutcome::ArtLearned {
            art_id: 3,
            position: 1,
            ..
        }
    ));
    assert!(matches!(
        world.use_item(0x91, 0),
        ItemOutcome::ArtLearned {
            art_id: 1,
            position: 0,
            ..
        }
    ));
    assert_eq!(skills(&world, 0), vec![1, 2, 3]);
}

/// A book whose class names a roster slot the party does not carry is a
/// no-op rather than a panic.
#[test]
fn a_book_for_a_missing_record_does_nothing() {
    let mut world = world_with(&book_rows(), 1);
    assert_eq!(world.use_item(0x95, 0), ItemOutcome::NoEffect);
}

/// The book's descriptor flags reach the catalog: `0x83` is field-usable and
/// **not** battle-usable, so the battle item list never offers it.
#[test]
fn book_usability_comes_from_the_descriptor_flags() {
    let world = world_with(&book_rows(), 3);
    let entry = world
        .tables
        .item_catalog
        .get(0x8F)
        .expect("the book is seeded into the catalog");
    assert!(entry.usable_in_field);
    assert!(!entry.usable_in_battle);
    assert_eq!(entry.name, "Fire Book I");
}

fn point_card_rows() -> Vec<Row> {
    // Class 14 on an arbitrary id - the shape only an edited effect table
    // produces.
    vec![Row {
        id: 0x40,
        kind: 2,
        class: 14,
        tier: 0,
        flags: 0x86,
    }]
}

/// The discharge spends `min(bank, 0x270F)`, writes the remainder back to the
/// same purse a shop buy credits, and applies the amount as HP damage.
#[test]
fn the_point_card_strike_spends_the_bank_and_damages_the_target() {
    let mut world = world_with(&point_card_rows(), 1);
    world.minigames.point_card = 500;
    world.actors[1].battle.max_hp = 900;
    world.actors[1].battle.hp = 900;

    let out = world.use_item(0x40, 1);
    assert_eq!(
        out,
        ItemOutcome::PointCardSpent {
            spent: 500,
            remaining: 0
        }
    );
    assert_eq!(world.actors[1].battle.hp, 400);
    assert_eq!(world.minigames.point_card, 0);
}

/// The clamp is retail's `sltiu s1, 0x2710`: at most `9999` leaves the bank in
/// one discharge, and the rest stays.
#[test]
fn the_discharge_is_clamped_to_9999_per_use() {
    let mut world = world_with(&point_card_rows(), 1);
    world.minigames.point_card = 30_000;
    world.actors[1].battle.max_hp = 20_000;
    world.actors[1].battle.hp = 20_000;

    assert_eq!(
        world.use_item(0x40, 1),
        ItemOutcome::PointCardSpent {
            spent: 9_999,
            remaining: 20_001
        }
    );
    assert_eq!(world.actors[1].battle.hp, 20_000 - 9_999);
    assert_eq!(world.minigames.point_card, 20_001);
}

/// An empty bank is retail's first test (`beq v1, zero` at `0x800420AC`): the
/// arm returns before it touches the victim.
#[test]
fn an_empty_bank_leaves_the_target_alone() {
    let mut world = world_with(&point_card_rows(), 1);
    world.minigames.point_card = 0;
    world.actors[1].battle.max_hp = 100;
    world.actors[1].battle.hp = 100;
    assert_eq!(world.use_item(0x40, 1), ItemOutcome::NoEffect);
    assert_eq!(world.actors[1].battle.hp, 100);
}

/// The HP clamp is kill-capable (`sltu v0, a0, s1` at `0x80042184`): a spend
/// at or above the victim's HP floors it at zero and downs the actor.
#[test]
fn a_spend_past_the_victims_hp_downs_it() {
    let mut world = world_with(&point_card_rows(), 1);
    world.minigames.point_card = 5_000;
    world.actors[1].battle.max_hp = 300;
    world.actors[1].battle.hp = 300;
    assert_eq!(
        world.use_item(0x40, 1),
        ItemOutcome::PointCardSpent {
            spent: 5_000,
            remaining: 0
        }
    );
    assert_eq!(world.actors[1].battle.hp, 0);
    assert_eq!(world.actors[1].battle.liveness, 0);
}

/// No retail item row resolves to the strike class, so a vanilla catalog seeded
/// from an untouched table installs no such marker - the seeder sweeping the id
/// space is what makes an edited table reach the arm.
#[test]
fn a_table_without_a_class_14_row_seeds_no_strike_item() {
    let world = world_with(&book_rows(), 3);
    let strikes = world
        .tables
        .item_catalog
        .iter()
        .filter(|e| {
            matches!(
                e.effect,
                legaia_engine_core::items::ItemEffect::PointCardStrike
            )
        })
        .count();
    assert_eq!(strikes, 0);
}

/// Selector `8` - the Antidote's class. Retail masks the target's status word
/// with `0xFFFC` (`0x80041C04` / `0x80041C34`), so ONE Antidote lifts Venom
/// and Toxic together, whatever the catalog names; and it skips a target at
/// zero HP (`beq v0,zero` at `0x80041BCC`) before touching the word.
#[test]
fn the_status_clear_arm_lifts_both_poisons_and_skips_a_dead_target() {
    use legaia_engine_vm::status_effects::StatusKind;
    let rows = [Row {
        id: 0x7E,
        kind: 2,
        class: 8,
        tier: 1,
        flags: 0x86,
    }];
    let mut world = world_with(&rows, 3);
    for (slot, hp) in [(0usize, 50u16), (1, 0)] {
        let rec = &mut world.party.roster.members[slot];
        let mut hms = rec.hp_mp_sp();
        hms.hp_cur = hp;
        hms.hp_max = 100;
        rec.set_hp_mp_sp(hms);
        world
            .battle
            .status_effects
            .apply(slot as u8, StatusKind::Venom);
        world
            .battle
            .status_effects
            .apply(slot as u8, StatusKind::Toxic);
        world
            .battle
            .status_effects
            .apply(slot as u8, StatusKind::Sleep);
    }
    // Living target: both poison bits clear, the Sleep bit survives.
    let out = world.use_item(0x7E, 0);
    assert_eq!(
        out,
        ItemOutcome::Cured {
            kind: StatusKind::Venom
        }
    );
    let left = world.battle.status_effects.display_flags(0);
    assert_eq!(left & 0x0003, 0, "Venom and Toxic both lifted");
    assert_ne!(left & 0x0800, 0, "Sleep is outside the 0xFFFC mask");
    // Dead target: nothing written.
    let before = world.battle.status_effects.display_flags(1);
    assert_eq!(world.use_item(0x7E, 1), ItemOutcome::NoEffect);
    assert_eq!(world.battle.status_effects.display_flags(1), before);
    // Nothing to clear: no effect either.
    assert_eq!(world.use_item(0x7E, 0), ItemOutcome::NoEffect);
}

/// The pause-menu path of a book: the finished Items use composes retail's
/// window-8 notice off the menu overlay's template - patched the way
/// `FUN_801DCD58` patches it (`0xC1` <- the slot, `0xC5` <- `slot * 0x40 +
/// art`) and expanded against the party names and the arts-name table.
///
/// The template here is synthetic (same token shape, no disc text), and the
/// contrast is the same use without a template: no notice, because the
/// engine does not invent the message.
#[test]
fn a_pause_menu_book_use_composes_the_window_8_notice() {
    use legaia_art::arts_table::ArtTableEntry;
    use legaia_art::queue::Character;
    use legaia_engine_core::field_menu_dispatch::apply_inventory_outcome;
    use legaia_engine_core::inventory_use::{
        InventoryContext, InventoryUseSession, InventoryUseState,
    };
    use legaia_engine_core::pause_screens::MenuTextTables;

    let finished = |world: &World| {
        let mut s = InventoryUseSession::new(
            world.tables.item_catalog.clone(),
            vec![0x92],
            Vec::new(),
            InventoryContext::Field,
        );
        s.used_item = Some(0x92);
        s.used_slots = vec![0];
        s.state = InventoryUseState::Done(ItemOutcome::NoEffect);
        s
    };
    let art = |character, index, name: &str| ArtTableEntry {
        character,
        index,
        name: name.into(),
        ap: 0,
        commands: Vec::new(),
        is_miracle: false,
    };

    let mut world = world_with(&book_rows(), 3);
    world.party.party_names = vec!["Vahn".into(), "Noa".into(), "Gala".into()];
    world.menu.text = Some(MenuTextTables {
        arts: Some(vec![
            art(Character::Vahn, 5, "Wrong Row"),
            art(Character::Noa, 5, "Noa Five"),
        ]),
        ..Default::default()
    });
    // `[C1 00] learns|[CF 06][C5 00][CF 07]!` - both operands are
    // placeholders the patch must overwrite.
    let mut template = vec![0xC1, 0x00];
    template.extend_from_slice(b" learns|");
    template.extend_from_slice(&[0xCF, 0x06, 0xC5, 0x00, 0xCF, 0x07]);
    template.push(b'!');
    world.menu.notify_template = Some(template);

    // Wind Book I: class 12 -> roster slot 1, tier 5.
    apply_inventory_outcome(&finished(&world), &mut world);
    let notice = world
        .menu
        .pending_art_notice
        .take()
        .expect("a taught art raises the window-8 notice");
    assert_eq!((notice.character, notice.art_id), (1, 5));
    assert_eq!(notice.lines, vec!["Noa learns", "Noa Five!"]);

    // Contrast: without the overlay template there is no notice at all.
    let mut bare = world_with(&book_rows(), 3);
    apply_inventory_outcome(&finished(&bare), &mut bare);
    assert_eq!(skills(&bare, 1), vec![5], "the art is still taught");
    assert!(bare.menu.pending_art_notice.is_none());
}
