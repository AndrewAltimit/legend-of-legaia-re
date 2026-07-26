//! Battle / minigame / menu SFX authenticity oracle - **including which bank
//! each cue sounds out of**.
//!
//! Three headline cues, and deliberately not all from the same bank:
//!
//! * cue `0x1A` - the Tactical-Arts generic strike cue (art-record Hit Effect
//!   Cue kind), descriptor category `0`,
//! * cue `0x21` - the shared pause-menu cursor blip the SCUS list kernel
//!   `FUN_80032A44` writes, also category `0`, and
//! * cue `0x09` - the Baka Fighter exchange-hit cue the duel overlay writes
//!   into the SFX ring (`FUN_801D3B18`), category `2`.
//!
//! All three are decoded from the disc's static SFX descriptor table
//! (`legaia_asset::sfx_table`) and fired against the bank their own **category**
//! names: a category selects a mixer record whose `+8` is a VAB slot id, slot 0
//! is the system bank (extraction PROT 0868) and slot 2 the class-2 bank
//! (PROT 0869) the retail battle scene loader / Baka init load. The cue names
//! its tone by explicit region **index** (`VabBank::play_tone`), so a note that
//! falls outside the tone's key window (which the old key-range `play_note`
//! path silently dropped to silence) still keys a voice.
//!
//! This also pins the SPU arithmetic the native boot depends on: both banks are
//! uploaded out of **one** allocator over the same `SFX_BANK_SPU_BYTES` region
//! the boot reserves, and every sample of both must land inside it.
//!
//! Skip-passes (CLAUDE.md disc-gated convention) when `LEGAIA_DISC_BIN` is unset
//! or the extracted `SCUS_942.54` / `PROT.DAT` aren't on disk.

use std::path::PathBuf;

use legaia_asset::sfx_table::{PINNED_SLOT_BANKS, SLOT2_CLASS2_BANK_ALT_PROT_INDEX, SfxTable};
use legaia_engine_audio::{Spu, VabBank, spu::ram::SPU_RAM_BYTES, spu::ram::SpuAllocator};
use legaia_engine_core::scene::SceneHost;
use legaia_engine_shell::boot::SFX_BANK_SPU_BYTES;

/// Cue ids (descriptor indices) under test.
const CUE_ART_STRIKE: u8 = 0x1A;
const CUE_MENU_CURSOR: u8 = 0x21;
const CUE_BAKA_HIT: u8 = 0x09;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("SCUS_942.54").exists() {
            return Some(d);
        }
    }
    None
}

/// Read one PROT entry's VAB, tolerating the `+4` chunk-header prefix.
fn read_vab(host: &SceneHost, idx: u32) -> Option<(legaia_vab::VabReport, Vec<u8>)> {
    let bytes = host.index.entry_bytes_extended(idx).ok()?;
    let (report, off) = [4usize, 0]
        .into_iter()
        .find_map(|o| legaia_vab::parse(&bytes, o).ok().map(|r| (r, o)))?;
    Some((report, bytes[off..].to_vec()))
}

#[test]
fn cues_sound_against_the_bank_their_category_names() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/SCUS_942.54 + PROT.DAT not present");
        return;
    };

    // SCUS -> the static SFX descriptor table -> the engine SFX bank, carrying
    // program + tone + note + voice-count (the full descriptor).
    let scus = std::fs::read(extracted.join("SCUS_942.54")).expect("read SCUS_942.54");
    let table = SfxTable::from_scus(&scus).expect("parse SFX descriptor table");
    let bank = legaia_engine_audio::SfxBank::from_descriptors(
        table
            .active()
            .map(|(id, d)| (id, d.program, d.tone, d.note, d.voice_count())),
    );

    // Sanity: the cues carry their disc descriptors (program 3 tone 0 for the
    // strike, program 0 tone 9 for the Baka hit). The strike's note (67) is
    // what the old key-range path resolved against - the tone-index path does
    // not depend on the tone's key window.
    let strike = table.get(CUE_ART_STRIKE).expect("0x1A descriptor");
    assert_eq!((strike.program, strike.tone), (3, 0), "0x1A = p3 t0");
    let baka = table.get(CUE_BAKA_HIT).expect("0x09 descriptor");
    assert_eq!((baka.program, baka.tone), (0, 9), "0x09 = p0 t9");

    // The routing itself: two of these three cues do NOT belong to the class-2
    // bank, which is the whole point of the category byte.
    assert_eq!(table.slot_for_cue(CUE_ART_STRIKE), Some(0));
    assert_eq!(table.slot_for_cue(CUE_MENU_CURSOR), Some(0));
    assert_eq!(table.slot_for_cue(CUE_BAKA_HIT), Some(2));

    // Stage both pinned banks out of ONE allocator over the boot's SFX region -
    // exactly what `boot::stage_sfx_vab` does. Two allocators each starting at
    // the region base would overlay one bank on the other.
    let host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let mut spu = Spu::new();
    let mut alloc = SpuAllocator::new(
        SPU_RAM_BYTES as u32 - SFX_BANK_SPU_BYTES,
        SFX_BANK_SPU_BYTES,
    );
    let mut banks = std::collections::BTreeMap::new();
    for (slot, prot) in PINNED_SLOT_BANKS.iter().copied() {
        let (report, body) = read_vab(&host, prot)
            .or_else(|| (slot == 2).then(|| read_vab(&host, SLOT2_CLASS2_BANK_ALT_PROT_INDEX))?)
            .unwrap_or_else(|| panic!("PROT {prot} (slot {slot}) has a VAB"));
        let uploaded = VabBank::upload(&mut spu, &mut alloc, &report, &body);
        // Every sample landed inside the reserved region - i.e. the allocator
        // never ran out and never handed back an address below it.
        for s in uploaded.samples.iter().flatten() {
            assert!(
                s.addr >= SPU_RAM_BYTES as u32 - SFX_BANK_SPU_BYTES,
                "slot {slot} sample at {:#x} is below the SFX region",
                s.addr
            );
            assert!(
                s.addr + s.size <= SPU_RAM_BYTES as u32,
                "slot {slot} sample at {:#x} runs past SPU RAM",
                s.addr
            );
        }
        assert!(
            uploaded.samples.iter().any(|s| s.is_some()),
            "slot {slot} (PROT {prot}) uploaded at least one sample - a bank \
             that silently allocated nothing is the out-of-room failure"
        );
        banks.insert(slot, uploaded);
    }
    assert_eq!(banks.len(), 2, "both pinned slots stage");

    // Both banks fit: the allocator still has room left, so nothing was
    // dropped for want of SPU RAM.
    assert!(
        alloc.total_free() > 0,
        "the SFX region must not be exactly exhausted - a zero margin means the \
         next disc revision silently drops samples"
    );

    // The two category-0 cues must key a voice in the SLOT-0 bank.
    for id in [CUE_ART_STRIKE, CUE_MENU_CURSOR] {
        let mut s = Spu::new();
        let mut a = SpuAllocator::new(0x1000, 0x60_000);
        let (report, body) = read_vab(&host, 868).expect("PROT 0868 VAB");
        let vab = VabBank::upload(&mut s, &mut a, &report, &body);
        let voice = bank
            .play_one_shot(id, &mut s, &vab)
            .unwrap_or_else(|| panic!("cue {id:#04x} keys a voice in the slot-0 bank"));
        assert!(
            !s.voices[voice as usize].is_off(),
            "cue {id:#04x} voice is playing (not silence)"
        );
    }

    // And the category-2 cue in the SLOT-2 bank.
    //
    // Ground truth (verified against PROT 0869 + the ring drainer FUN_80016B6C,
    // which keys region `tone + i` for i in 0..voice_count):
    //   - 0x09 = program 0, tone 9, voice_count 2.
    //   - Program 0 has 10 populated tone regions (0..=9); tone 9 is the last.
    //   - So the 2nd voice targets region tone+1 = 10, which is an UNUSED slot
    //     (all zero: vag 0, vol 0). Retail programs that voice too, but with a
    //     zero VAG/volume it is silent. `play_one_shot` skips the empty region
    //     (its `vag <= 0` guard), so the cue keys exactly ONE audible voice.
    let mut spu2 = Spu::new();
    let mut alloc2 = SpuAllocator::new(0x1000, 0x60_000);
    let (report2, body2) = read_vab(&host, 869).expect("PROT 0869 VAB");
    let vab2 = VabBank::upload(&mut spu2, &mut alloc2, &report2, &body2);
    let vhit = bank
        .play_one_shot(CUE_BAKA_HIT, &mut spu2, &vab2)
        .expect("Baka hit cue 0x09 keys a voice against the class-2 bank");
    assert!(
        !spu2.voices[vhit as usize].is_off(),
        "Baka hit cue voice is playing (not silence)"
    );

    // Multi-voice fan-out against the real bank: cue 0x4E (program 7, tone 0,
    // voice_count 3) is a cue whose three consecutive regions (0/1/2) are ALL
    // populated in this bank, so all three voices key on - proving the
    // `tone + i` fan-out (not the same tone layered) drives multiple voices.
    const CUE_MULTI: u8 = 0x4E;
    let m = table.get(CUE_MULTI).expect("0x4E descriptor");
    assert_eq!(
        (m.program, m.tone, m.voice_count()),
        (7, 0, 3),
        "0x4E = p7 t0 3-voice"
    );
    let mut spu3 = Spu::new();
    let mut alloc3 = SpuAllocator::new(0x1000, 0x60_000);
    let vab3 = VabBank::upload(&mut spu3, &mut alloc3, &report2, &body2);
    bank.play_one_shot(CUE_MULTI, &mut spu3, &vab3)
        .expect("3-voice cue 0x4E keys a voice");
    assert!(
        spu3.voices.iter().filter(|v| !v.is_off()).count() >= 3,
        "the 3-voice cue 0x4E keyed all three consecutive regions"
    );

    eprintln!(
        "[ok] strike 0x1A + cursor 0x21 out of slot-0 PROT 0868; Baka 0x09 + \
         3-voice 0x4E out of slot-2 PROT 0869; both packed in {SFX_BANK_SPU_BYTES} \
         bytes with {} free",
        alloc.total_free()
    );
}
