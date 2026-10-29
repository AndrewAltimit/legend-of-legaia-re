//! The expansion-pack block through the LGSF save round trip
//! (`WorldSaveState::save_full` / `load_full`): a session without a pack
//! writes the bytes it always wrote, a pack's quest progress and pack flags
//! survive a save and load, a save from before the block loads under a pack
//! as fresh progress, and a block for a pack that is not loaded survives a
//! resave.

use std::sync::Arc;

use legaia_engine_core::world::*;
use legaia_engine_expansion::quest::{QuestEvent, QuestStatus};
use legaia_engine_save_state::WorldSaveState;
use legaia_expansion_format::{ExpansionPack, PackFiles};

fn pack(id: &str) -> Arc<ExpansionPack> {
    let mut files = PackFiles::new();
    files.insert(
        "pack.toml".into(),
        format!("id = \"{id}\"\nname = \"P\"\nversion = \"1.0.0\"\n").into_bytes(),
    );
    files.insert(
        "quests.toml".into(),
        br#"
[[quests]]
id = "talks"
auto_start = true
objectives = [{ kind = "talk", npc = "a" }, { kind = "talk", npc = "b" }]
"#
        .to_vec(),
    );
    Arc::new(ExpansionPack::from_files(&files).unwrap())
}

fn world() -> World {
    let mut w = World::new();
    w.load_party(legaia_save::Party::zeroed(2));
    w.party.money = 1234;
    w
}

#[test]
fn a_session_without_a_pack_writes_no_block() {
    let mut w = world();
    let sf = w.save_full();
    assert_eq!(sf.ext_v2.expansion, None);
    let bytes = sf.write();
    assert!(
        !bytes
            .windows(4)
            .any(|x| x == legaia_save::SAVE_FILE_EXTD_MAGIC)
    );
    // Loading it back and saving again is byte-stable.
    let mut w2 = world();
    w2.load_full(legaia_save::SaveFile::parse(&bytes).unwrap());
    assert_eq!(w2.save_full().write(), bytes);
}

#[test]
fn quest_progress_and_pack_flags_survive_a_save_and_load() {
    let mut w = world();
    w.expansion.pack = Some(pack("p"));
    w.expansion_notify(QuestEvent::Refresh);
    w.expansion_talked("a");
    w.expansion_notify(QuestEvent::FlagSet("seen".into()));
    let bytes = w.save_full().write();

    let mut w2 = world();
    w2.expansion.pack = Some(pack("p"));
    w2.load_full(legaia_save::SaveFile::parse(&bytes).unwrap());
    let q = w2.expansion.quests.quest("talks").expect("restored");
    assert_eq!(q.status, QuestStatus::Active);
    assert_eq!(q.objectives, vec![1, 0]);
    assert!(w2.expansion.quests.progress.flags.is_set("seen"));
    w2.expansion_talked("b");
    assert_eq!(
        w2.expansion.quests.quest("talks").unwrap().status,
        QuestStatus::Completed
    );
}

#[test]
fn a_save_from_before_the_block_loads_as_fresh_progress() {
    let mut plain = world();
    let bytes = plain.save_full().write();

    let mut w = world();
    w.expansion.pack = Some(pack("p"));
    w.expansion_notify(QuestEvent::Refresh);
    w.expansion_talked("a");
    w.load_full(legaia_save::SaveFile::parse(&bytes).unwrap());
    assert!(w.expansion.quests.progress.quests.is_empty());
    assert_eq!(w.party.money, 1234);
}

#[test]
fn a_block_for_an_absent_pack_survives_a_resave() {
    let mut w = world();
    w.expansion.pack = Some(pack("p"));
    w.expansion_notify(QuestEvent::Refresh);
    w.expansion_talked("a");
    let sf = w.save_full();
    let block = sf
        .ext_v2
        .expansion
        .clone()
        .expect("a pack session writes a block");
    let bytes = sf.write();

    // No pack: the block is kept and written back verbatim.
    let mut none = world();
    none.load_full(legaia_save::SaveFile::parse(&bytes).unwrap());
    assert_eq!(
        none.save_full().ext_v2.expansion.as_deref(),
        Some(&block[..])
    );

    // Another pack: it starts fresh and the first pack's block waits.
    let mut other = world();
    other.expansion.pack = Some(pack("q"));
    other.load_full(legaia_save::SaveFile::parse(&bytes).unwrap());
    assert!(other.expansion.quests.progress.quests.is_empty());
    assert_eq!(other.expansion.quests.dormant.as_deref(), Some(&block[..]));

    // Activating the first pack later picks the progress up.
    other.expansion.pack = Some(pack("p"));
    other.expansion_notify(QuestEvent::Refresh);
    assert_eq!(
        other.expansion.quests.quest("talks").unwrap().objectives,
        vec![1, 0]
    );
}

fn boss_pack() -> Arc<ExpansionPack> {
    let mut files = PackFiles::new();
    files.insert(
        "pack.toml".into(),
        b"id = \"b\"\nname = \"B\"\nversion = \"1.0.0\"\n".to_vec(),
    );
    files.insert(
        "battle.toml".into(),
        br#"
[formations.den]
monsters = [{ kind = "monster", id = 9 }]
[bosses.alpha]
formation = "den"
once = true
"#
        .to_vec(),
    );
    Arc::new(ExpansionPack::from_files(&files).unwrap())
}

#[test]
fn a_beaten_once_only_boss_survives_a_save_and_load() {
    let mut w = world();
    w.activate_expansion(boss_pack()).unwrap();
    assert!(!w.expansion_boss_cleared("alpha"));
    w.expansion.battle.cleared_bosses.insert("alpha".into());
    let bytes = w.save_full().write();

    // Loaded under the pack.
    let mut w2 = world();
    w2.activate_expansion(boss_pack()).unwrap();
    w2.load_full(legaia_save::SaveFile::parse(&bytes).unwrap());
    assert!(w2.expansion_boss_cleared("alpha"));

    // Loaded first, the pack activated after.
    let mut w3 = world();
    w3.load_full(legaia_save::SaveFile::parse(&bytes).unwrap());
    w3.activate_expansion(boss_pack()).unwrap();
    assert!(w3.expansion_boss_cleared("alpha"));

    // A save with no boss beaten writes no boss record.
    let mut fresh = world();
    fresh.activate_expansion(boss_pack()).unwrap();
    let block = fresh.save_full().ext_v2.expansion.unwrap();
    assert!(!block.windows(5).any(|x| x == b"alpha"));
}
