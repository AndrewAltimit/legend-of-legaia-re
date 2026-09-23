//! Disc-gated: the Evil God Icon's steal lands **through the live battle
//! round**, not through a direct call to a grant kernel.
//!
//! `steal_randomizer_runtime_e2e.rs` drives `World::apply_steal` by hand; this
//! is the round-driven sibling. A one-member party whose record carries
//! passive `0x10` (Steal Attack, `+0xF4` bit `0x10000`) fights the real
//! Skeleton (monster id 13) off PROT 867, with the monster's own archive
//! clips installed so its knockdown really plays. The steal fires where
//! retail fires it - the end of the slain monster's knockdown clip
//! (`FUN_8004AD80` `0x8004B29C..0x8004B65C`, ported as
//! `legaia_engine_core::battle_steal`) - and the test asserts the item lands
//! in the bag and the element-`0x5B` caption was up while the battle ran.
//!
//! The steal row's chance is raised to 100 so a single fight decides; the
//! item is the disc's own row. A contrast pass without the passive bit spends
//! the battle's one attempt and grants nothing, so the positive pass cannot be
//! satisfied by some other grant path.
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN` / `extracted/`.

use std::path::PathBuf;

use legaia_asset::steal_table::{StealEntry, StealTable};
use legaia_engine_core::monster_catalog::{
    FormationDef, FormationSlot, FormationTable, catalog_from_monster_archive,
};
use legaia_engine_core::world::{Actor, SceneMode, World};

const SKELETON_ID: u16 = 13;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("SCUS_942.54").exists() {
            return Some(d);
        }
    }
    None
}

struct Inputs {
    archive: Vec<u8>,
    scus: Vec<u8>,
}

fn inputs() -> Option<Inputs> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    let dir = extracted_dir()?;
    let mut prot = legaia_prot::archive::Archive::open(&dir.join("PROT.DAT")).ok()?;
    let entry = prot.entries[867].clone();
    let mut archive = Vec::new();
    prot.read_entry(&entry, &mut archive).ok()?;
    let scus = std::fs::read(dir.join("SCUS_942.54")).ok()?;
    Some(Inputs { archive, scus })
}

/// What one fight produced.
struct Outcome {
    caption: Option<String>,
    item_before: u8,
    item_after: u8,
    item: u8,
    attempted: bool,
}

fn fight(inp: &Inputs, steal_bit: bool) -> Outcome {
    let real = StealTable::from_scus(&inp.scus).expect("steal table");
    let row = real.entry(SKELETON_ID).expect("skeleton row");
    assert!(
        row.is_stealable(),
        "the Skeleton carries a steal on the disc"
    );
    let mut entries: Vec<StealEntry> = (0..256u16)
        .map(|id| {
            real.entry(id).unwrap_or(StealEntry {
                chance_pct: 0,
                item_id: 0,
            })
        })
        .collect();
    entries[SKELETON_ID as usize].chance_pct = 100;

    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party.party_count = 1;
    w.load_party(legaia_save::Party::zeroed(1));
    let mut party = w.party.roster.clone();
    for rec in party.members.iter_mut() {
        let mut hms = rec.hp_mp_sp();
        hms.hp_cur = 400;
        hms.hp_max = 400;
        rec.set_hp_mp_sp(hms);
        if steal_bit {
            let mut bits = rec.ability_bits();
            bits[2] |= 0x01; // +0xF4 bit 0x10000: passive 0x10, Steal Attack
            rec.set_ability_bits(bits);
        }
    }
    w.load_party(party);
    w.actors[0].active = true;
    w.actors[0].battle.hp = 400;
    w.actors[0].battle.max_hp = 400;
    w.actors[0].battle.liveness = 1;
    w.set_battle_attack(0, 120);
    w.set_battle_defense(0, 40);
    w.set_steal_table(StealTable::from_entries(entries));
    w.battle.ui_strings.merge_scus(&inp.scus);
    w.menu.text = Some(legaia_engine_core::pause_screens::MenuTextTables::from_scus(&inp.scus));

    let catalog = catalog_from_monster_archive(&inp.archive, &[SKELETON_ID]);
    let mut table = FormationTable::new();
    table.insert(FormationDef::new(1, vec![FormationSlot::new(SKELETON_ID)]));
    w.set_formation_table(table, catalog);
    w.mode = SceneMode::Field;
    assert!(w.trigger_scripted_battle(1));
    for _ in 0..400 {
        if w.mode == SceneMode::Battle {
            break;
        }
        w.tick();
    }
    assert_eq!(w.mode, SceneMode::Battle);

    // The monster's own archive clips, so its knockdown (tag 4) plays and
    // ends - the frame the death commit runs on.
    let slot = usize::from(w.party.party_count);
    // Positional, holes kept - the install both hosts make.
    let clips = legaia_asset::monster_archive::animations_by_entry(&inp.archive, SKELETON_ID)
        .ok()
        .flatten()
        .expect("skeleton clips");
    assert!(
        clips.iter().flatten().any(|c| c.action_id == 4),
        "the Skeleton carries a knockdown entry"
    );
    w.set_actor_battle_action_clips(slot, std::sync::Arc::new(clips));

    let item = row.item_id;
    let item_before = w.party.inventory.get(&item).copied().unwrap_or(0);
    let mut caption = None;
    for _ in 0..60_000 {
        w.tick();
        if caption.is_none() {
            caption = legaia_engine_core::battle_hud::battle_message_bar(&w);
        }
        if w.mode != SceneMode::Battle {
            break;
        }
    }
    assert_ne!(w.mode, SceneMode::Battle, "the fight must resolve");
    Outcome {
        caption,
        item_before,
        item_after: w.party.inventory.get(&item).copied().unwrap_or(0),
        item,
        attempted: w.battle.steal.attempted,
    }
}

#[test]
fn an_evil_god_icon_kill_steals_through_the_live_round() {
    let Some(inp) = inputs() else {
        eprintln!("[skip] LEGAIA_DISC_BIN / extracted/ missing");
        return;
    };
    let with = fight(&inp, true);
    eprintln!(
        "[ok] steal pass: item {:#04x} {} -> {}, caption {} chars",
        with.item,
        with.item_before,
        with.item_after,
        with.caption.as_deref().map_or(0, str::len)
    );
    assert!(with.attempted, "the killing blow spent the steal attempt");
    assert_eq!(
        with.item_after,
        with.item_before + 1,
        "the stolen item landed in the bag"
    );
    let caption = with.caption.expect("the 0x5B caption was raised");
    // Head + item-name token + tail: the stolen item's own disc name sits
    // inside the line the executable's template frames.
    let scus_name = legaia_engine_core::pause_screens::MenuTextTables::from_scus(&inp.scus)
        .item_name(with.item)
        .map(str::to_string)
        .expect("the stolen item has a name on the disc");
    assert!(
        caption.contains(&scus_name) && !caption.starts_with(&scus_name),
        "caption composed off the executable's template around the item name"
    );

    let without = fight(&inp, false);
    eprintln!(
        "[ok] contrast pass: item {} -> {}, caption raised: {}",
        without.item_before,
        without.item_after,
        without.caption.is_some()
    );
    assert!(
        without.attempted,
        "the kill spends the attempt even without the passive"
    );
    assert_eq!(
        without.item_after, without.item_before,
        "no passive, no steal"
    );
    assert!(without.caption.is_none());
}
