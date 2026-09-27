//! Disc-gated: the ending theme's bank takes SPU RAM the way retail's VAB 10
//! does, and the resident SFX banks come back after it.
//!
//! Retail opens the credits bank (extraction `1056`) as VAB `10`, whose SPU
//! base `0x800917B0[10]` is slot 0's (`0x1010`), so it is laid over the
//! resident banks from the bottom of SPU RAM up. Its bodies total `0x631B0`
//! bytes - more than the hosts' BGM region - so the shared placement kernel
//! (`legaia_engine_audio::spu_layout`) lays it across the SFX region and
//! reports the eviction; the next track that fits hands the region back and
//! the host re-stages the boot banks through the same kernel.
//!
//! This is the evidence for that path: nobody listens to it. It asserts SPU
//! addresses and sizes, that every credits body uploads, that the credits
//! upload really overwrote the resident banks' bytes and the re-stage really
//! restored them (compared in memory, never written anywhere), and that a
//! boot SFX cue keys a voice and renders above the noise floor after the
//! re-stage.
//!
//! Skips when `LEGAIA_DISC_BIN` or `extracted/` is missing.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use legaia_asset::sfx_table::{
    SLOT0_SYSTEM_BANK_PROT_INDEX, SLOT6_FIELD_BANK_PROT_INDEX, SfxTable,
};
use legaia_engine_audio::spu_layout::{
    BGM_REGION_BYTES, OwnedBankPlacement, SFX_REGION_BASE, SPU_RAM_BYTES, SPU_RESERVED_BYTES,
    bank_used_end, sfx_region_free, upload_owned_bank, upload_resident_sfx, vab_body_bytes,
};
use legaia_engine_audio::{SPU_INTERNAL_RATE, SfxBank, Spu, VabBank};
use legaia_prot::archive::Archive;
use legaia_prot::cdname;
use legaia_vab::VabReport;

/// Extraction index of the ending theme's VAB-only bank (raw TOC `0x422`,
/// the field initialiser's second read at `0x801D7214`).
const CREDITS_BANK_ENTRY: usize = 1056;
/// Retail's menu cursor cue (`FUN_80032A44`, `li a2,0x21`) - a category-0
/// cue, so it keys the slot-0 system bank.
const MENU_CURSOR_CUE: u8 = 0x21;

fn extracted_dir() -> Option<PathBuf> {
    ["extracted", "../extracted", "../../extracted"]
        .iter()
        .map(PathBuf::from)
        .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists())
}

fn gate() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    }
    let d = extracted_dir();
    if d.is_none() {
        eprintln!("[skip] extracted/ missing");
    }
    d
}

struct Bank {
    bytes: Vec<u8>,
    report: VabReport,
    off: usize,
}

impl Bank {
    fn buf(&self) -> &[u8] {
        &self.bytes[self.off..]
    }
}

fn read_bank(archive: &mut Archive, index: usize) -> Bank {
    let entry = archive.entries[index].clone();
    let mut bytes = Vec::new();
    archive.read_entry(&entry, &mut bytes).expect("read entry");
    let (report, off) = [4usize, 0]
        .into_iter()
        .find_map(|o| legaia_vab::parse(&bytes, o).ok().map(|r| (r, o)))
        .unwrap_or_else(|| panic!("entry {index} carries no VAB at +4 or +0"));
    Bank { bytes, report, off }
}

/// A `music_01` entry whose bank fits the BGM region - an ordinary track.
fn ordinary_track(archive: &mut Archive, extracted: &std::path::Path) -> Bank {
    let map = cdname::parse(&extracted.join("CDNAME.TXT")).expect("CDNAME");
    let (start, end) =
        cdname::block_range_for_name_extraction(&map, "music_01").expect("music_01 block");
    (start..end)
        .find_map(|i| {
            let entry = archive.entries.get(i as usize)?.clone();
            let mut bytes = Vec::new();
            archive.read_entry(&entry, &mut bytes).ok()?;
            let off = bytes.windows(4).position(|w| w == b"pBAV")?;
            let report = legaia_vab::parse(&bytes, off).ok()?;
            (vab_body_bytes(&report) <= BGM_REGION_BYTES).then_some(Bank { bytes, report, off })
        })
        .expect("an ordinary music_01 bank")
}

fn range_hash(spu: &Spu, lo: u32, hi: u32) -> u64 {
    let mut h = DefaultHasher::new();
    spu.ram.slice(lo, hi - lo).hash(&mut h);
    h.finish()
}

fn layout(bank: &VabBank) -> Vec<(u32, u32)> {
    bank.samples
        .iter()
        .flatten()
        .map(|s| (s.addr, s.size))
        .collect()
}

/// Key `id` on a throwaway voice set over `spu`'s RAM image; the peak of a
/// quarter second of output, or `None` when no voice keyed.
fn cue_peak(sfx: &SfxBank, spu: &Spu, vab: &VabBank, id: u8) -> Option<i32> {
    let mut probe = Spu::new();
    probe.ram = spu.ram.clone();
    sfx.play_one_shot(id, &mut probe, vab)?;
    let mut peak = 0i32;
    for _ in 0..(SPU_INTERNAL_RATE / 4) {
        let (l, r) = probe.tick();
        peak = peak.max((l as i32).abs()).max((r as i32).abs());
    }
    Some(peak)
}

#[test]
fn the_credits_bank_evicts_the_sfx_region_and_the_boot_banks_come_back() {
    let Some(extracted) = gate() else { return };
    let scus = std::fs::read(extracted.join("SCUS_942.54")).expect("SCUS_942.54");
    let table = SfxTable::from_scus(&scus).expect("SFX descriptor table");
    let sfx = SfxBank::from_descriptors(
        table
            .active()
            .map(|(id, d)| (id, d.program, d.tone, d.note, d.voice_count())),
    );
    let cue_slot = table
        .cue_slots()
        .find(|(id, _)| *id == MENU_CURSOR_CUE)
        .map(|(_, s)| s);
    assert_eq!(
        cue_slot,
        Some(0),
        "the menu cursor cue keys the slot-0 bank"
    );

    let mut archive = Archive::open(&extracted.join("PROT.DAT")).expect("open PROT");
    let slot0 = read_bank(&mut archive, SLOT0_SYSTEM_BANK_PROT_INDEX as usize);
    let field = read_bank(&mut archive, SLOT6_FIELD_BANK_PROT_INDEX as usize);
    let credits = read_bank(&mut archive, CREDITS_BANK_ENTRY);
    let track = ordinary_track(&mut archive, &extracted);

    let mut spu = Spu::new();

    // 1. Boot: the slot-0 system bank at the SFX region's floor, the field
    //    bank (the shared region's occupant in the ending's field scenes)
    //    above it.
    let boot = upload_resident_sfx(
        &mut spu,
        (&slot0.report, slot0.buf()),
        Some((&field.report, field.buf())),
    );
    let boot_shared = boot.shared.clone().expect("the field bank fits the region");
    let boot_slot0_layout = layout(&boot.slot0);
    let boot_shared_layout = layout(&boot_shared);
    assert_eq!(
        boot_slot0_layout.first().map(|s| s.0),
        Some(SFX_REGION_BASE)
    );
    let sfx_end = bank_used_end(&boot_shared).expect("shared samples");
    assert!(sfx_end <= SPU_RAM_BYTES);
    let sfx_hash = range_hash(&spu, SFX_REGION_BASE, sfx_end);
    let boot_peak = cue_peak(&sfx, &spu, &boot.slot0, MENU_CURSOR_CUE)
        .expect("the cursor cue keys the boot slot-0 bank");
    assert!(boot_peak > 64, "boot cursor cue peak {boot_peak}");

    // 2. An ordinary track stays under the SFX region.
    let ordinary = upload_owned_bank(&mut spu, &track.report, track.buf());
    assert_eq!(ordinary.placement, OwnedBankPlacement::BgmRegion);
    assert!(!ordinary.evicts_sfx);
    assert!(sfx_region_free(Some(&ordinary.bank)));
    assert_eq!(range_hash(&spu, SFX_REGION_BASE, sfx_end), sfx_hash);

    // 3. The credits bank: every body uploads, from the BGM base across the
    //    SFX region.
    let body = vab_body_bytes(&credits.report);
    assert_eq!(body, 0x631B0, "extraction 1056's VAG bodies");
    let staged = upload_owned_bank(&mut spu, &credits.report, credits.buf());
    assert_eq!(staged.placement, OwnedBankPlacement::AcrossSfxRegion);
    assert!(
        staged.evicts_sfx,
        "the credits bank overwrites the SFX region"
    );
    let nonempty = credits
        .report
        .vag_samples
        .iter()
        .filter(|v| v.size > 0)
        .count();
    let uploaded = staged.bank.samples.iter().flatten().count();
    assert_eq!(uploaded, nonempty, "every credits body is resident");
    let first = staged.bank.samples.iter().flatten().map(|s| s.addr).min();
    assert_eq!(first, Some(SPU_RESERVED_BYTES));
    let end = bank_used_end(&staged.bank).expect("credits samples");
    assert_eq!(end, SPU_RESERVED_BYTES + body);
    assert!(end > SFX_REGION_BASE && end <= SPU_RAM_BYTES);
    assert!(!sfx_region_free(Some(&staged.bank)));
    assert_ne!(
        range_hash(&spu, SFX_REGION_BASE, sfx_end),
        sfx_hash,
        "the resident banks' bytes are gone under the credits bank"
    );
    eprintln!(
        "[ok] credits bank: {uploaded}/{nonempty} bodies, SPU {:#X}..{end:#X} \
         ({body:#X} B) across the SFX region at {SFX_REGION_BASE:#X}",
        SPU_RESERVED_BYTES
    );

    // 4. The next ordinary track hands the region back; the boot banks
    //    re-stage to the same addresses with the same bytes, and the cue
    //    keys and sounds again.
    let after = upload_owned_bank(&mut spu, &track.report, track.buf());
    assert!(!after.evicts_sfx);
    assert!(sfx_region_free(Some(&after.bank)));
    let restaged = upload_resident_sfx(
        &mut spu,
        (&slot0.report, slot0.buf()),
        Some((&field.report, field.buf())),
    );
    assert_eq!(layout(&restaged.slot0), boot_slot0_layout);
    assert_eq!(
        restaged.shared.as_ref().map(layout),
        Some(boot_shared_layout)
    );
    assert_eq!(range_hash(&spu, SFX_REGION_BASE, sfx_end), sfx_hash);
    let peak = cue_peak(&sfx, &spu, &restaged.slot0, MENU_CURSOR_CUE)
        .expect("the cursor cue keys the re-staged slot-0 bank");
    assert_eq!(
        peak, boot_peak,
        "the re-staged cue renders as it did at boot"
    );
    eprintln!(
        "[ok] re-staged slot 0 at {SFX_REGION_BASE:#X}, shared region to {sfx_end:#X}; \
         cue {MENU_CURSOR_CUE:#04X} peak {peak}"
    );
}
