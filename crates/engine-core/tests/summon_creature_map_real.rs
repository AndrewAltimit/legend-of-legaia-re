//! Disc-gated: every player Seru-magic summon resolves to its namesake
//! `battle_data` creature, so the summon can render through the ordinary battle
//! per-object animation pipeline. Pins `summon::summon_creature_id` against real
//! PROT 867 bytes. The Gimard mapping (`0x81` → id 10) is the one byte-verified
//! against the fingerprinted `gimard_summon_visible` save.
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
}
