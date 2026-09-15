//! Disc-gated: the live battle loop applies retail's **random-encounter stat
//! profile** and the **Seru-magic side effects** that ride on it.
//!
//! Both halves are one mechanism. The battle loader picks a stat boost
//! profile by the scripted-fight flag `ctx[+0x287]` (`FUN_80054CB0`
//! `0x80055234`), writing each stat into both halfwords of its pair; the
//! Seru side-effect stager (`FUN_801F3D3C`) then compares a target's **base**
//! halfword against the raw record and stages nothing when they differ. So a
//! random encounter's `x7/4` defence is both the enemy the player actually
//! fights and the reason ATK-down lands on it while it is shrugged off by a
//! boss.
//!
//! What is asserted is the output, not the call: a seated monster's live
//! defence halfwords, and the stat pair a cast moved - both against numbers
//! recomputed from the disc record here.
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN` / `extracted/`.

use super::*;
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for base in ["extracted", "../../extracted"] {
        let p = PathBuf::from(base);
        if p.join("PROT.DAT").is_file() && p.join("SCUS_942.54").is_file() {
            return Some(p);
        }
    }
    None
}

fn prot_entry(dir: &std::path::Path, index: usize) -> Vec<u8> {
    let mut archive =
        legaia_prot::archive::Archive::open(&dir.join("PROT.DAT")).expect("open PROT.DAT");
    let entry = archive.entries.get(index).cloned().expect("PROT entry");
    let mut bytes = Vec::new();
    archive.read_entry(&entry, &mut bytes).expect("read entry");
    bytes
}

/// PROT entry the monster archive lives in.
const MONSTER_ARCHIVE_PROT: usize = 867;

/// A monster record whose defence is large enough that the two boost profiles
/// give different answers (`udf * 2 != udf + (udf>>1) + (udf>>2)` needs
/// `udf >= 4`), and whose ATK is boosted by the scripted profile only.
fn pick_record(archive: &[u8]) -> Option<legaia_asset::monster_archive::MonsterRecord> {
    let slots = legaia_asset::monster_archive::slot_count(archive) as u16;
    (1..=slots).find_map(|id| {
        let r = legaia_asset::monster_archive::record(archive, id)
            .ok()
            .flatten()?;
        let s = r.stats;
        // Both profiles must differ on UDF, LDF and ATK, or the assertion
        // below could not tell them apart.
        let scripted = legaia_asset::monster_archive::boost_profile(s, true);
        let random = legaia_asset::monster_archive::boost_profile(s, false);
        (r.hp > 0
            && scripted[1] != random[1]
            && scripted[2] != random[2]
            && scripted[3] != random[3])
            .then_some(r)
    })
}

/// A battle world seating one real archive monster through the real entry
/// path, for a fight of the given class.
fn seat_monster(rec: &legaia_asset::monster_archive::MonsterRecord, scripted: bool) -> World {
    let mut w = World::default();
    w.party.party_count = 1;
    let def = crate::monster_catalog::monster_def_from_record(rec);
    w.tables.monster_catalog.insert(def);
    let formation = crate::monster_catalog::FormationDef::new(
        0,
        vec![crate::monster_catalog::FormationSlot::new(rec.id)],
    )
    // `record[+0]` non-zero is exactly what raises the per-battle `0x80` the
    // scripted flag is derived from (`FUN_801DA51C`).
    .with_header_flags(u8::from(scripted));
    w.enter_battle_from_formation(&formation);
    w
}

#[test]
fn a_random_encounter_seats_the_x7_4_defence_profile() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let archive = prot_entry(&dir, MONSTER_ARCHIVE_PROT);
    let Some(rec) = pick_record(&archive) else {
        eprintln!("[skip] no archive record separates the two boost profiles");
        return;
    };
    let want_random = legaia_asset::monster_archive::boost_profile(rec.stats, false);
    let want_scripted = legaia_asset::monster_archive::boost_profile(rec.stats, true);

    let w = seat_monster(&rec, false);
    assert!(
        !w.battle.scripted_fight,
        "a header-byte-0 row is not scripted"
    );
    let m = w.party.party_count as usize;
    assert_eq!(
        w.battle.defense_split[m],
        Some((want_random[2], want_random[3])),
        "monster {} must seat the random-encounter defence (x7/4), not the boss one",
        rec.id
    );
    assert_eq!(
        w.battle.attack[m], want_random[1],
        "ATK is copied unchanged"
    );
    assert_ne!(
        w.battle.defense_split[m],
        Some((want_scripted[2], want_scripted[3])),
        "the two profiles must actually differ for this record"
    );

    let w = seat_monster(&rec, true);
    assert!(w.battle.scripted_fight, "a header-byte row IS scripted");
    assert_eq!(
        w.battle.defense_split[m],
        Some((want_scripted[2], want_scripted[3])),
        "a boss row must still seat the x2 defence profile"
    );
    assert_eq!(
        w.battle.attack[m], want_scripted[1],
        "ATK x5/4 on a boss row"
    );
    eprintln!(
        "[ok] monster {}: random UDF/LDF {:?} vs scripted {:?}",
        rec.id,
        (want_random[2], want_random[3]),
        (want_scripted[2], want_scripted[3])
    );
}

#[test]
fn the_stat_bases_open_the_fight_equal_to_their_working_halves() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let archive = prot_entry(&dir, MONSTER_ARCHIVE_PROT);
    let Some(rec) = pick_record(&archive) else {
        eprintln!("[skip] no archive record separates the two boost profiles");
        return;
    };
    let w = seat_monster(&rec, false);
    for slot in 0..w.battle.attack.len() {
        assert_eq!(w.battle.attack_base[slot], w.battle.attack[slot]);
        assert_eq!(w.battle.defense_base[slot], w.battle.defense_split[slot]);
        assert_eq!(w.battle.speed_base[slot], w.battle.speed[slot]);
        assert_eq!(w.battle.accuracy_base[slot], w.battle.accuracy[slot]);
    }
    // And the compare the stager makes therefore reads "unchanged" on the
    // stats the random profile leaves alone, and "moved" on the two it does
    // not.
    let m = w.party.party_count;
    let cmp = w
        .enemy_stat_compare(m)
        .expect("an archive-backed enemy seat");
    assert!(
        cmp.spd.unchanged(),
        "SPD is copied unchanged by both profiles"
    );
    assert!(
        cmp.atk.unchanged(),
        "ATK is copied unchanged by the random profile"
    );
    assert!(
        cmp.mp.unchanged(),
        "MP is copied unchanged by both profiles"
    );
    assert!(
        !cmp.udf.unchanged(),
        "the random profile moves UDF off the record"
    );
    eprintln!(
        "[ok] base halves seeded, compare pairs read off monster {}",
        rec.id
    );
}

/// The element -> stat the finisher shaves, for the six damaging summon
/// elements.
fn shaved_stat(w: &World, slot: u8, kind: legaia_asset::seru_side_effect::SideEffectKind) -> u32 {
    use legaia_asset::seru_side_effect::SideEffectKind as K;
    let i = slot as usize;
    match kind {
        K::DefDown => u32::from(w.battle.defense_split[i].unwrap_or_default().0),
        K::AtkDown => u32::from(w.battle.attack[i]),
        K::SpdDown => u32::from(w.battle.speed[i]),
        K::IntDown => u32::from(w.battle.accuracy[i]),
        K::AglDown => u32::from(w.actors[i].battle.agl_base),
        K::MpDown => u32::from(w.actors[i].battle.mp),
        K::Cure | K::None => 0,
    }
}

#[test]
fn a_live_seru_cast_shaves_the_table_percentage_off_the_target() {
    use legaia_asset::seru_side_effect::{SeruSideEffectTable, SideEffectKind};
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let archive = prot_entry(&dir, MONSTER_ARCHIVE_PROT);
    let overlay = prot_entry(
        &dir,
        legaia_asset::move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX,
    );
    let Some(table) = SeruSideEffectTable::parse(&overlay) else {
        eprintln!("[skip] PROT 0898 carries no side-effect table on this image");
        return;
    };
    let Some(rec) = pick_record(&archive) else {
        eprintln!("[skip] no usable archive record");
        return;
    };

    // Fire (element 2) -> ATK down. Any player Seru spell id will do as the
    // carrier; the stager switches on the SUMMON creature's element, which is
    // what `tables.summon_elements` supplies.
    const SPELL_ID: u8 = 0x81;
    const FIRE: u8 = 2;
    // Magic level 9 = the top band, 20%.
    const LEVEL: u8 = 9;

    let mut w = seat_monster(&rec, false);
    w.tables.seru_side_effects = Some(table);
    w.tables.summon_elements.insert(SPELL_ID, FIRE);
    // A roster member carrying the spell at level 9, so the caster's
    // `+0x161` scan finds it.
    let mut member = legaia_save::CharacterRecord::parse(&[0u8; 0x414]).expect("blank record");
    let mut list = member.spell_list();
    list.count = 1;
    list.ids[0] = SPELL_ID;
    list.levels[0] = LEVEL;
    member.set_spell_list(list);
    w.party.roster.members = vec![member];

    let m = w.party.party_count;
    let before = shaved_stat(&w, m, SideEffectKind::AtkDown);
    assert!(before > 0, "the seated monster must carry a non-zero ATK");

    let def = crate::spells::SpellDef {
        id: SPELL_ID,
        name: "probe".into(),
        mp_cost: 0,
        target: crate::spells::SpellTarget::OneEnemy,
        effect: crate::spells::SpellEffect::Damage {
            base_power: 1,
            element: crate::spells::SpellElement::Neutral,
        },
        ..Default::default()
    };
    assert!(w.cast_spell_on_slots(0, &def, &[m]), "the cast folds");

    let after = shaved_stat(&w, m, SideEffectKind::AtkDown);
    let pct = u32::from(
        w.tables
            .seru_side_effects
            .as_ref()
            .unwrap()
            .amount(FIRE, LEVEL),
    );
    let want = before - (before * pct) / 100;
    assert_eq!(
        after, want,
        "a level-{LEVEL} fire Seru cast must shave {pct}% off ATK ({before} -> {want})"
    );
    // The base half moves with it - that is what makes a second cast of the
    // same element land again in a random fight and be refused in a boss one.
    assert_eq!(u32::from(w.battle.attack_base[m as usize]), want);
    eprintln!("[ok] ATK {before} -> {after} at {pct}%");
}

#[test]
fn a_sub_level_three_cast_stages_nothing_and_draws_nothing() {
    use legaia_asset::seru_side_effect::SeruSideEffectTable;
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let archive = prot_entry(&dir, MONSTER_ARCHIVE_PROT);
    let overlay = prot_entry(
        &dir,
        legaia_asset::move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX,
    );
    let Some(table) = SeruSideEffectTable::parse(&overlay) else {
        eprintln!("[skip] PROT 0898 carries no side-effect table");
        return;
    };
    let Some(rec) = pick_record(&archive) else {
        eprintln!("[skip] no usable archive record");
        return;
    };
    let mut w = seat_monster(&rec, false);
    w.tables.seru_side_effects = Some(table);
    w.tables.summon_elements.insert(0x81, 2);
    let mut member = legaia_save::CharacterRecord::parse(&[0u8; 0x414]).expect("blank record");
    let mut list = member.spell_list();
    list.count = 1;
    list.ids[0] = 0x81;
    list.levels[0] = 2; // below MIN_LEVEL
    member.set_spell_list(list);
    w.party.roster.members = vec![member];

    let m = w.party.party_count;
    let atk_before = w.battle.attack[m as usize];
    let rng_before = w.rng_state;
    let def = crate::spells::SpellDef {
        id: 0x81,
        name: "probe".into(),
        mp_cost: 0,
        target: crate::spells::SpellTarget::OneEnemy,
        effect: crate::spells::SpellEffect::Damage {
            base_power: 1,
            element: crate::spells::SpellElement::Neutral,
        },
        ..Default::default()
    };
    assert!(w.cast_spell_on_slots(0, &def, &[m]));
    assert_eq!(
        w.battle.attack[m as usize], atk_before,
        "a level-2 spell carries no side effect at all"
    );
    // The stager's one draw is on the scripted arm, which a random encounter
    // never reaches; a sub-level-3 cast returns before it either way. The
    // fold's own damage roll may draw, so this only asserts the cursor did not
    // move by the stager's extra draw - it is checked against the same cast on
    // a world with no table installed.
    let mut bare = seat_monster(&rec, false);
    bare.party.roster.members = w.party.roster.members.clone();
    bare.rng_state = rng_before;
    assert!(bare.cast_spell_on_slots(0, &def, &[m]));
    assert_eq!(
        bare.rng_state, w.rng_state,
        "installing the table must not perturb the RNG stream for a cast that stages nothing"
    );
    eprintln!("[ok] level-2 cast stages nothing, RNG cursor unperturbed");
}

#[test]
fn a_live_vera_cast_restores_the_module_amount() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let archive = prot_entry(&dir, MONSTER_ARCHIVE_PROT);
    let Some(rec) = pick_record(&archive) else {
        eprintln!("[skip] no usable archive record");
        return;
    };
    // Vera `0x83` and Orb `0x89` - the two ally-side player Seru casts.
    for (spell_id, level, want) in [
        (
            0x83u8,
            7u8,
            u32::from(legaia_engine_vm::cast_seru_ticks_a::vera_heal_amount(
                7, 100, 4000,
            )),
        ),
        (
            0x89u8,
            5u8,
            legaia_engine_vm::cast_seru_ticks_b::orb_heal_amount(5),
        ),
    ] {
        let mut w = seat_monster(&rec, false);
        let mut member = legaia_save::CharacterRecord::parse(&[0u8; 0x414]).expect("blank record");
        let mut list = member.spell_list();
        list.count = 1;
        list.ids[0] = spell_id;
        list.levels[0] = level;
        member.set_spell_list(list);
        w.party.roster.members = vec![member];
        w.actors[0].battle.max_hp = 4000;
        w.actors[0].battle.hp = 100;
        w.actors[0].battle.liveness = 1;

        let def = crate::spells::SpellDef {
            id: spell_id,
            name: "heal probe".into(),
            mp_cost: 0,
            target: crate::spells::SpellTarget::OneAlly,
            // A deliberately wrong placeholder magnitude: the module's own
            // amount has to override it.
            effect: crate::spells::SpellEffect::Heal { amount: 7 },
            ..Default::default()
        };
        assert!(w.cast_spell_on_slots(0, &def, &[0]), "the heal folds");
        let restored = u32::from(w.actors[0].battle.hp) - 100;
        assert_eq!(
            restored, want,
            "spell {spell_id:#04X} at level {level} must restore the module's own amount"
        );
        eprintln!("[ok] {spell_id:#04X} level {level} restored {restored}");
    }
}

/// The discriminator the seed keys on has to actually discriminate: if every
/// MAN formation row carried a non-zero `record[+0]`, every fight would take
/// the boss profile and the change above would be a no-op dressed as a fix.
///
/// So this is a census, not a behaviour test - it walks every CDNAME scene's
/// MAN and counts the rows either way, and requires the scripted rows to be a
/// small minority with both classes present.
#[test]
fn the_scripted_header_byte_is_a_minority_of_the_formation_corpus() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let Ok(mut host) = crate::scene::SceneHost::open_extracted(&dir) else {
        eprintln!("[skip] SceneHost would not open");
        return;
    };
    let Ok(cdname) = legaia_prot::cdname::parse(&dir.join("CDNAME.TXT")) else {
        eprintln!("[skip] CDNAME.TXT unreadable");
        return;
    };
    let mut scenes: Vec<String> = cdname.values().cloned().collect();
    scenes.sort();
    scenes.dedup();

    let (mut scripted, mut random, mut scenes_seen) = (0usize, 0usize, 0usize);
    for scene in &scenes {
        if host.load_scene(scene).is_err() {
            continue;
        }
        let Some(sc) = host.scene.as_ref() else {
            continue;
        };
        let Ok(Some(man)) = sc.field_man_payload(&host.index) else {
            continue;
        };
        let defs = crate::encounter_man::formation_defs_from_man(&man);
        if defs.is_empty() {
            continue;
        }
        scenes_seen += 1;
        for d in &defs {
            if d.per_battle_flags() != 0 {
                scripted += 1;
            } else {
                random += 1;
            }
        }
    }
    let total = scripted + random;
    assert!(
        scenes_seen >= 20 && total >= 100,
        "expected a corpus of MAN formation rows, got {total} rows over {scenes_seen} scenes"
    );
    assert!(scripted > 0, "the disc must carry some scripted rows");
    assert!(
        scripted * 4 < total,
        "scripted rows must be the minority ({scripted} of {total}) - otherwise keying the \
         boost profile on the header byte would make every fight a boss fight"
    );
    eprintln!(
        "[ok] {scripted} scripted / {random} random formation rows over {scenes_seen} scenes"
    );
}
