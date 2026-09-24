//! Disc-gated oracle for the **slot-2 / slot-6 shared SPU region** the native
//! director refills per game mode.
//!
//! Retail gives VAB slots 2 and 6 one SPU base (`FUN_800265E8`: `0x33010`)
//! and each mode's initialiser refills it: the field init loads PROT 0876 into
//! slot 6 (`FUN_801D6704`), the battle scene loader PROT 0869 into slot 2
//! (`FUN_800520F0`), the minigame overlays their own banks into slot 2. The
//! engine's residency (`World::sync_sfx_residency`) names the bank; this test
//! walks a world through field -> battle -> fishing -> field and checks that
//! every named bank fits the region above the slot-0 system bank inside the
//! boot's reserved SFX window, and that a field script cue (`0x2E`, category
//! 6) keys a voice out of PROT 0876 - the bank it used to miss.
//!
//! Skip-passes when `LEGAIA_DISC_BIN` is unset or `extracted/` is absent.

use std::path::PathBuf;

use legaia_asset::sfx_table::SfxTable;
use legaia_engine_audio::{Spu, SpuAllocator, VabBank, spu::ram::SPU_RAM_BYTES};
use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::world::{SceneMode, SharedRegionBank, World};
use legaia_engine_shell::boot::SFX_BANK_SPU_BYTES;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("SCUS_942.54").exists() {
            return Some(d);
        }
    }
    None
}

fn read_vab(host: &SceneHost, idx: u32) -> (legaia_vab::VabReport, Vec<u8>) {
    let bytes = host
        .index
        .entry_bytes_extended(idx)
        .unwrap_or_else(|e| panic!("read PROT {idx}: {e}"));
    let (report, off) = [4usize, 0]
        .into_iter()
        .find_map(|o| legaia_vab::parse(&bytes, o).ok().map(|r| (r, o)))
        .unwrap_or_else(|| panic!("PROT {idx} has a VAB"));
    (report, bytes[off..].to_vec())
}

fn body_bytes(report: &legaia_vab::VabReport) -> u32 {
    report.vag_samples.iter().map(|v| v.size as u32).sum()
}

#[test]
fn the_shared_region_follows_the_mode_and_field_cues_key_prot_0876() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/SCUS_942.54 + PROT.DAT not present");
        return;
    };
    let host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let scus = std::fs::read(extracted.join("SCUS_942.54")).expect("read SCUS");
    let table = SfxTable::from_scus(&scus).expect("SFX table");

    // Slot 0 at the bottom of the reserved window, as the boot stages it.
    let region = SPU_RAM_BYTES as u32 - SFX_BANK_SPU_BYTES;
    let mut spu = Spu::new();
    let (r0, b0) = read_vab(&host, 868);
    let mut a0 = SpuAllocator::new(region, SFX_BANK_SPU_BYTES);
    let slot0 = VabBank::upload(&mut spu, &mut a0, &r0, &b0);
    let base = slot0
        .samples
        .iter()
        .flatten()
        .map(|s| s.addr + s.size)
        .max()
        .expect("slot 0 uploaded samples")
        .div_ceil(16)
        * 16;
    let room = SPU_RAM_BYTES as u32 - base;

    // Walk the world through the modes the residency distinguishes.
    let mut w = World::new();
    w.active_scene_label = "town01".into();
    let mut seen = Vec::new();
    for mode in [
        SceneMode::Field,
        SceneMode::Battle,
        SceneMode::Field,
        SceneMode::Fishing,
        SceneMode::SlotMachine,
        SceneMode::BakaFighter,
        SceneMode::WorldMap,
    ] {
        w.mode = mode;
        let bank = w
            .sync_sfx_residency()
            .expect("every walked mode names a bank");
        seen.push((mode, bank));
    }
    assert_eq!(seen[0].1, SharedRegionBank::FIELD);
    assert_eq!(seen[1].1, SharedRegionBank::CLASS2);
    assert_eq!(
        seen[2].1,
        SharedRegionBank::FIELD,
        "the field reloads after battle"
    );
    assert_eq!((seen[3].1.slot, seen[3].1.prot_entry), (2, 1197));
    assert_eq!((seen[4].1.slot, seen[4].1.prot_entry), (2, 1198));
    assert_eq!(seen[5].1, SharedRegionBank::CLASS2);
    assert_eq!(seen[6].1, SharedRegionBank::FIELD);

    // Every bank the walk named fits the region above slot 0.
    for (mode, bank) in &seen {
        let (r, _) = read_vab(&host, bank.prot_entry);
        let need = body_bytes(&r);
        assert!(
            need <= room,
            "{mode:?}: PROT {} needs {need} B, the shared region has {room}",
            bank.prot_entry
        );
    }
    // The dance's bank is the one that does not: retail overruns slot 3's
    // base with it, the port leaves the region closed.
    let (dance, _) = read_vab(&host, 1231);
    assert!(body_bytes(&dance) > room, "PROT 1231 overruns the region");

    // The field bank, uploaded where the director puts it, keys the field
    // script cue 0x2E (category 6 -> slot 6).
    assert_eq!(table.slot_for_cue(0x2E), Some(6));
    let (r6, b6) = read_vab(&host, 876);
    let mut a6 = SpuAllocator::new(base, room);
    let field = VabBank::upload(&mut spu, &mut a6, &r6, &b6);
    for s in field.samples.iter().flatten() {
        assert!(s.addr >= base && s.addr + s.size <= SPU_RAM_BYTES as u32);
    }
    let bank = legaia_engine_audio::SfxBank::from_descriptors(
        table
            .active()
            .map(|(id, d)| (id, d.program, d.tone, d.note, d.voice_count())),
    );
    for id in [0x2Eu8, 0x2F] {
        let voice = bank
            .play_one_shot(id, &mut spu, &field)
            .unwrap_or_else(|| panic!("cue {id:#04x} keys a voice out of PROT 0876"));
        assert!(voice < 24);
    }
    eprintln!(
        "[ok] shared region {room} B above slot 0; field cues 0x2E/0x2F key PROT 0876; \
         walk {:?}",
        seen.iter()
            .map(|(m, b)| (format!("{m:?}"), b.slot, b.prot_entry))
            .collect::<Vec<_>>()
    );
}
