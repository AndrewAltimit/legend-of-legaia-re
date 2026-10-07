//! Disc-gated: every player Seru-magic summon resolves to its namesake
//! `battle_data` creature, so the summon can render through the ordinary battle
//! per-object animation pipeline. Pins `summon::summon_creature_id` against real
//! PROT 867 bytes. The Gimard mapping (`0x81` → id 10) is the one byte-verified
//! against the fingerprinted `gimard_summon_visible` save; the whole base +
//! evolved block `0x81..=0x95` is disc-pinned by mesh identity (see
//! `legaia_asset::summon_creatures` + the asset-side `summon_creature_tmd_map_real`).
use std::path::PathBuf;

fn battle_data() -> Option<Vec<u8>> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for d in ["extracted/PROT", "../../extracted/PROT"] {
        let p = PathBuf::from(d).join("0867_battle_data.BIN");
        if let Ok(b) = std::fs::read(&p) {
            return Some(b);
        }
    }
    None
}

#[test]
fn player_summons_map_to_their_namesake_battle_data_creatures() {
    let Some(entry) = battle_data() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/PROT/0867 missing");
        return;
    };
    use legaia_engine_core::summon::summon_creature_id;

    // spell id -> (name, expected battle_data creature id).
    let expect: &[(u8, &str, u16)] = &[
        (0x81, "Gimard", 10),
        (0x82, "Theeder", 25),
        (0x83, "Vera", 28),
        (0x84, "Gizam", 55),
        (0x85, "Nighto", 49),
        (0x86, "Zenoir", 64),
        (0x87, "Viguro", 74),
        (0x88, "Swordie", 86),
        (0x89, "Orb", 83),
        (0x8a, "Freed", 92),
        (0x8b, "Nova", 95),
        // Evolved-Seru block (disc-pinned by mesh identity, not name) - the two
        // legs 0x90/0x91 had no mid-cast capture and are pinned from disc bytes.
        (0x8c, "Gola Gola", 98),
        (0x8d, "Mushura", 101),
        (0x8e, "Aluru", 80),
        (0x8f, "Barra", 141),
        (0x90, "Kemaro", 144),
        (0x91, "Spoon", 147),
        (0x92, "Slippery", 150),
        (0x93, "Iota", 153),
        (0x94, "Puera", 156),
        (0x95, "Gilium", 159),
    ];
    for &(spell, name, id) in expect {
        let got = summon_creature_id(spell, &entry)
            .unwrap_or_else(|| panic!("no creature for summon {name} ({spell:#04x})"));
        assert_eq!(got, id, "summon {name} ({spell:#04x}) -> creature id");
        // The resolved creature really carries that name + a decodable idle.
        let rec = legaia_asset::monster_archive::record(&entry, got)
            .expect("decode")
            .expect("populated");
        assert_eq!(rec.name, name);
        let idle = legaia_asset::monster_archive::idle_animation(&entry, got)
            .expect("decode idle")
            .expect("summon creature has an idle clip");
        assert!(
            idle.part_count >= 2 && idle.frame_count >= 2,
            "{name} idle should be a real multi-part clip ({}x{})",
            idle.part_count,
            idle.frame_count
        );
    }

    // Non-summon spell ids resolve to nothing.
    assert!(summon_creature_id(0x80, &entry).is_none());
    assert!(summon_creature_id(0x10, &entry).is_none());

    // The high block 0x99..=0xA0 is a bespoke mesh, not an archive creature, so
    // it is intentionally unresolved here (its body comes from `summon.dat` -
    // `summon_spawn_asset`, pinned below).
    for spell in 0x99u8..=0xA0 {
        assert!(
            summon_creature_id(spell, &entry).is_none(),
            "high-block summon {spell:#04x} should not resolve to an archive creature",
        );
    }
}

/// Every player summon id seats a drawable body through the one kernel both
/// hosts call: the archive twin for `0x81..=0x95`, the cast's own
/// `summon.dat` record (PROT 893) for the high block.
#[test]
fn every_player_summon_resolves_a_drawable_spawn_body() {
    let Some(entry) = battle_data() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/PROT/0867 missing");
        return;
    };
    let summon_dat = ["extracted/PROT", "../../extracted/PROT"]
        .iter()
        .find_map(|d| std::fs::read(PathBuf::from(d).join("0893_monster_se.BIN")).ok())
        .expect("summon.dat (PROT 893) beside 867");
    use legaia_engine_core::summon::{HIGH_SUMMON_IDS, summon_spawn_asset};
    for spell in (0x81u8..=0x95).chain(HIGH_SUMMON_IDS) {
        let a = summon_spawn_asset(spell, &entry, Some(&summon_dat))
            .unwrap_or_else(|| panic!("{spell:#04x}: no spawn body"));
        assert_eq!(a.creature_id.is_some(), spell <= 0x95, "{spell:#04x}");
        let tmd = legaia_tmd::parse(a.mesh.tmd_bytes()).expect("parse body TMD");
        assert!(!tmd.objects.is_empty(), "{spell:#04x}: empty TMD");
        let mut vram = legaia_tim::Vram::new();
        let vmesh = a
            .mesh
            .battle_render_mesh(0, &mut vram)
            .unwrap_or_else(|| panic!("{spell:#04x}: no render mesh"));
        assert!(!vmesh.indices.is_empty(), "{spell:#04x}: no triangles");
        assert!(a.idle.is_some(), "{spell:#04x}: no idle clip");
    }
    // Without summon.dat the high block has no body; the archive twins do.
    assert!(summon_spawn_asset(0x9E, &entry, None).is_none());
    assert!(summon_spawn_asset(0x8C, &entry, None).is_some());
}
