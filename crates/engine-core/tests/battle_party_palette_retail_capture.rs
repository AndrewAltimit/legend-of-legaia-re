//! The battle party's CLUT rows are the **equipped** sections' palettes.
//!
//! `FUN_80052FA0` uploads each block as `[CLUT struct][pixels]` through
//! `FUN_80053B9C`: record[0]'s two blocks, then the flagged pool of each of
//! the five sections the equipment selector picked. The CLUT half lands on
//! row `481 + party ordinal`. The engine used to follow those uploads with a
//! second pass that painted each section's `id = 0` (unequipped) palette over
//! the same row, so any member wearing non-default gear - the late-game
//! Ra-Seru armour sets above all - drew with the default colours.
//!
//! The oracle is two late-game battle states whose whole party wears
//! Ra-Seru armour. For each present member, the row the engine's shared
//! battle-form build writes (`install_party_battle_forms` replays the same
//! `character_texture_uploads`) must match retail's VRAM row at every column
//! the uploads write.
//!
//! Needs the extracted `PROT.DAT` (`LEGAIA_EXTRACTED_DIR`, else
//! `extracted/`) and the mednafen save library (`LEGAIA_SAVES_LIBRARY`, else
//! `saves/library`); skips and passes without either.

use legaia_asset::battle_char_assembly as bca;
use legaia_engine_core::battle_party_form::{
    PartyFormSources, VramWriteLog, build_party_battle_form,
};
use legaia_engine_core::scene::ProtIndex;
use legaia_engine_core::world::World;
use legaia_mednafen::SaveState;
use legaia_prot::archive::Archive;
use std::path::{Path, PathBuf};

/// Character records: `0x414` bytes each from `0x80084708`.
const CHAR_RECORDS: usize = 0x8_4708;
const CHAR_STRIDE: usize = 0x414;

fn prot_dat() -> Option<PathBuf> {
    if let Ok(d) = std::env::var("LEGAIA_EXTRACTED_DIR") {
        let p = PathBuf::from(d).join("PROT.DAT");
        if p.is_file() {
            return Some(p);
        }
    }
    ["extracted", "../extracted", "../../extracted"]
        .iter()
        .map(|b| PathBuf::from(b).join("PROT.DAT"))
        .find(|p| p.is_file())
}

fn library() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("LEGAIA_SAVES_LIBRARY") {
        let p = PathBuf::from(p).join("mednafen");
        if p.is_dir() {
            return Some(p);
        }
    }
    ["", "../", "../../"]
        .iter()
        .map(|p| PathBuf::from(format!("{p}saves/library/mednafen")))
        .find(|p| p.is_dir())
}

fn state(lib: &Path, prefix: &str) -> Option<SaveState> {
    let path = std::fs::read_dir(lib)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| {
            p.file_name()
                .is_some_and(|f| f.to_string_lossy().starts_with(prefix))
        })?;
    SaveState::from_path(&path).ok()
}

#[test]
fn party_clut_rows_are_the_equipped_sections_palettes() {
    let (Some(prot), Some(lib)) = (prot_dat(), library()) else {
        eprintln!(
            "[skip] needs extracted/PROT.DAT and saves/library (LEGAIA_EXTRACTED_DIR / LEGAIA_SAVES_LIBRARY)"
        );
        return;
    };
    let index = ProtIndex::open_extracted(prot.parent().unwrap()).expect("open extracted tree");
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    // (backup-fingerprint prefix, label); both parties are Vahn / Noa / Gala
    // in every late-game Ra-Seru armour set.
    let states = [
        ("6a640a54", "vera_summon_mid_cast"),
        ("6ce34481", "meta_summon_mid_cast"),
    ];
    let mut ran = 0;
    for (prefix, label) in states {
        let Some(st) = state(&lib, prefix) else {
            eprintln!("[skip] {label} not in the library");
            continue;
        };
        let ram = st.main_ram().expect("main RAM");
        let retail = legaia_mednafen::gpu::PsxGpu::new(&st);
        let mut world = World::default();
        world.party.roster.members = (0..4)
            .map(|c| {
                let o = CHAR_RECORDS + c * CHAR_STRIDE;
                legaia_save::CharacterRecord::parse(&ram[o..o + CHAR_STRIDE]).expect("record")
            })
            .collect();
        let mut log = VramWriteLog::default();
        let sources = PartyFormSources::load(&index).expect("PROT 1204 fallback pack");
        for member in 0..3usize {
            let form = build_party_battle_form(&index, &world, &sources, &mut log, member)
                .expect("battle form");
            assert!(form.assembled, "{label}: member {member} assembles");
        }
        let mut vram = legaia_tim::Vram::new();
        log.replay(&mut vram);
        for member in 0..3usize {
            // The columns the equipped uploads own.
            let equipped = world.party.roster.members[member].equipment().slots;
            let equipped: [u8; 5] = equipped[..5].try_into().unwrap();
            let entry = archive.entries[863 + member].clone();
            let mut raw = Vec::new();
            archive.read_entry(&entry, &mut raw).expect("player file");
            let pack = legaia_asset::battle_data_pack::parse(&raw).expect("player-file pack");
            let uploads = bca::character_texture_uploads(&raw, &pack, &equipped, member as u8)
                .expect("texture uploads");
            let row = 481 + member;
            let mut cols: Vec<usize> = uploads
                .iter()
                .flat_map(|u| u.clut_x as usize..u.clut_x as usize + u.clut.len())
                .collect();
            cols.sort_unstable();
            cols.dedup();
            let same = cols
                .iter()
                .filter(|&&x| Some(vram.pixel(x, row)) == retail.vram_pixel(x as u32, row as u32))
                .count();
            println!(
                "{label} member {member} equip {equipped:02X?}: {same}/{} row-{row} CLUT cells match retail",
                cols.len()
            );
            assert!(
                !cols.is_empty(),
                "{label}: member {member} uploads a palette"
            );
            assert_eq!(
                same,
                cols.len(),
                "{label}: member {member}'s battle palette differs from retail's row {row}"
            );
        }
        ran += 1;
    }
    assert!(ran > 0, "no library state was found to compare against");
}
