//! Disc-gated end-to-end oracle for the chest randomizer **at runtime**.
//!
//! The randomizer's own disc-gated tests (`crates/rando/tests/chest_patch_real`)
//! prove the patch is *written* faithfully: the field-VM `GIVE_ITEM` (op `0x39`)
//! operand byte changes, the site offsets stay put, sectors stay EDC/ECC-valid.
//! What they do **not** prove is that a runtime actually *reads the patched byte
//! and grants the new item* — the question that matters for "is it truly
//! randomizing, or is something serving a stale value?".
//!
//! Answering that with a savestate is a trap: a mednafen savestate snapshots all
//! of RAM, and a scene's MAN (including each chest's `GIVE_ITEM` bytecode) is
//! resident in RAM the moment you stand in the room (the `keikoku_chest_pre`
//! capture's MAN sits at RAM `0x8012f4c8`). Loading such a state on a patched
//! disc still grants the *original* item, because the chest id is read from the
//! already-loaded RAM copy, never re-fetched from the (patched) disc on open. A
//! patched chest is only observed after a *fresh scene load* re-reads the MAN.
//!
//! The clean-room engine sidesteps the cache entirely: it loads the MAN straight
//! from disc bytes and runs the very op (`0x39`) the randomizer edits. So this
//! test:
//!   1. patches one known chest (keikoku's Phoenix, item `0x80`) to a distinct id
//!      on a scratch copy of the real disc, in memory,
//!   2. re-decodes the patched scene MAN off the patched image (the same bytes a
//!      fresh scene load would stream),
//!   3. drives that chest actor's inline interaction script through the real
//!      field VM (the ported dialog SM `FUN_80039B7C` + `vm::field::step`),
//!   4. asserts the runtime grants the **patched** id and never the original.
//!
//! A baseline pass over the *unpatched* MAN first confirms the engine reaches
//! this chest's give at all (and pins which interaction slot is the Phoenix
//! chest), so the patched assertion can't pass vacuously. Skips without
//! `LEGAIA_DISC_BIN` (CLAUDE.md convention).

use legaia_asset::man_section::parse as parse_man;
use legaia_engine_core::field_events::FieldEvent;
use legaia_engine_core::world::World;
use legaia_rando::chest::SceneChests;
use legaia_rando::disc::DiscPatcher;

/// keikoku (Ravine) scene bundle PROT entry — the chest-randomizer ground-truth
/// scene (see `crates/rando/tests/chest_patch_real.rs`).
const KEIKOKU_ENTRY: usize = 112;
/// keikoku's Phoenix chest gives item id `0x80` (pinned by the
/// `keikoku_chest_pre` / `_open` savestate pair).
const PHOENIX_ITEM: u8 = 0x80;
/// An arbitrary but distinct replacement id. Validity is irrelevant to the
/// runtime-grant proof — the engine `give_item` hook adds whatever id the
/// bytecode carries; the point is that it differs from `PHOENIX_ITEM`.
const REPLACEMENT_ITEM: u8 = 0x42;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// Drive one partition-1 interaction slot's full inline interaction script
/// (prologue + boxes + control ops) through the field VM and return every item
/// id its `GIVE_ITEM` ops grant. A fresh `World` per call keeps the inventory
/// clean; `confirm` is held so each text box auto-dismisses once it finishes
/// typing, advancing the VM to the post-text `0x39`.
fn gives_for_slot(decoded_man: &[u8], slot: u8) -> Vec<u8> {
    let man_file = parse_man(decoded_man).expect("parse patched MAN");
    let mut world = World::new();
    world.install_field_carriers_from_man(&man_file, decoded_man);

    // The chest actor's full interaction record (prologue selects the segment per
    // story flags; with a fresh world all flags are zero = chest unopened).
    let Some(prologue) = world.field_npc_dialog_prologue.get(&slot).cloned() else {
        return Vec::new();
    };
    world.start_inline_dialogue_with_prologue(
        prologue.body,
        prologue.entry_pc,
        prologue.first_segment,
    );

    let mut gives = Vec::new();
    // Bound the drive: a chest record is a handful of boxes; 20k ticks is far more
    // than the typewriter + VM steps need and guards a pathological script.
    for _ in 0..20_000 {
        world.step_inline_dialogue(true, false, false);
        for ev in world.drain_field_events() {
            if let FieldEvent::GiveItem { item_id } = ev {
                gives.push(item_id);
            }
        }
        if world.inline_dialogue.as_ref().is_none_or(|d| d.is_done()) {
            break;
        }
    }
    gives
}

/// Find the partition-1 interaction slot whose runtime give-set contains `item`.
fn slot_granting(decoded_man: &[u8], item: u8) -> Option<u8> {
    let man_file = parse_man(decoded_man).ok()?;
    let mut world = World::new();
    world.install_field_carriers_from_man(&man_file, decoded_man);
    let slots: Vec<u8> = world.field_npc_dialog_prologue.keys().copied().collect();
    slots
        .into_iter()
        .find(|&slot| gives_for_slot(decoded_man, slot).contains(&item))
}

#[test]
fn patched_chest_grants_new_item_at_runtime() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };

    // Resolve item names from the disc's SCUS table purely for legible output
    // (no Sony bytes are asserted) — so the run reads "Phoenix -> Ra-Seru Helmet"
    // rather than raw ids.
    let names = legaia_iso::iso9660::read_file_in_image(&disc, "SCUS_942.54")
        .and_then(|scus| legaia_asset::item_names::ItemNameTable::from_scus(&scus));
    let name_of = |id: u8| {
        names
            .as_ref()
            .and_then(|t| t.name(id))
            .unwrap_or("?")
            .to_string()
    };

    // --- Baseline: the engine reaches the Phoenix chest's give on the UNPATCHED
    //     disc, and we learn which interaction slot it is. ---
    let patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let entry = patcher.read_entry(KEIKOKU_ENTRY).expect("read keikoku");
    let sc = SceneChests::locate(&entry, KEIKOKU_ENTRY).expect("keikoku has chest sites");
    let items = sc.current_items();
    let site_k = items
        .iter()
        .position(|&b| b == PHOENIX_ITEM)
        .expect("keikoku has the Phoenix (0x80) chest");

    let phoenix_slot = slot_granting(&sc.decoded, PHOENIX_ITEM).unwrap_or_else(|| {
        panic!(
            "engine inline-script runner must reach keikoku's Phoenix give at runtime \
             (drove every interaction slot, none granted 0x{PHOENIX_ITEM:02x})"
        )
    });
    let baseline = gives_for_slot(&sc.decoded, phoenix_slot);
    assert!(
        baseline.contains(&PHOENIX_ITEM),
        "baseline: Phoenix slot {phoenix_slot} must grant 0x{PHOENIX_ITEM:02x}, got {baseline:02x?}"
    );
    assert!(
        !baseline.contains(&REPLACEMENT_ITEM),
        "baseline must NOT already grant the replacement id (test would be vacuous)"
    );

    // --- Patch only the Phoenix chest's give operand on a scratch disc, then
    //     re-decode the MAN off the PATCHED image (what a fresh scene load reads). ---
    let mut patcher = DiscPatcher::open(disc).expect("reopen disc");
    let mut sc =
        SceneChests::locate(&patcher.read_entry(KEIKOKU_ENTRY).unwrap(), KEIKOKU_ENTRY).unwrap();
    let site = sc.sites[site_k];
    assert_eq!(sc.decoded[site], PHOENIX_ITEM);
    sc.decoded[site] = REPLACEMENT_ITEM;
    let stream = sc
        .repack()
        .expect("keikoku MAN re-packs within its footprint");
    patcher
        .patch_prot_entry(KEIKOKU_ENTRY, sc.man_offset as u64, &stream)
        .expect("write patched keikoku MAN");

    // Re-decode off the patched image — this is the disc-truth, not the in-memory
    // copy we mutated above.
    let patched_entry = patcher.read_entry(KEIKOKU_ENTRY).unwrap();
    let patched = SceneChests::locate(&patched_entry, KEIKOKU_ENTRY).unwrap();
    assert_eq!(
        patched.decoded[patched.sites[site_k]], REPLACEMENT_ITEM,
        "patched disc bytes must carry the new chest id at the same site"
    );

    // --- Runtime: drive the SAME chest slot through the field VM on the patched
    //     MAN. It must grant the replacement and never the original. ---
    let runtime = gives_for_slot(&patched.decoded, phoenix_slot);
    assert!(
        runtime.contains(&REPLACEMENT_ITEM),
        "runtime grants the patched id 0x{REPLACEMENT_ITEM:02x} (slot {phoenix_slot}), got {runtime:02x?}"
    );
    assert!(
        !runtime.contains(&PHOENIX_ITEM),
        "runtime must NOT grant the original Phoenix 0x{PHOENIX_ITEM:02x} after the patch \
         (got {runtime:02x?}) — a stale value here is the caching failure this test guards"
    );

    eprintln!(
        "chest runtime E2E: keikoku slot {phoenix_slot} baseline grants {baseline:02x?} \
         ({}), patched grants {runtime:02x?} ({}) — 0x{PHOENIX_ITEM:02x} {} -> 0x{REPLACEMENT_ITEM:02x} {}",
        name_of(PHOENIX_ITEM),
        name_of(REPLACEMENT_ITEM),
        name_of(PHOENIX_ITEM),
        name_of(REPLACEMENT_ITEM),
    );
}
