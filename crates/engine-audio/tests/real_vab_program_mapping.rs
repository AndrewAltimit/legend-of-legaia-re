//! Disc-gated: the program-number -> packed-tone-page mapping over every VAB
//! in the extracted PROT corpus.
//!
//! Retail resolves a SEQ ProgramChange / SFX-descriptor program number to a
//! tone page by its rank among the used `ProgAtr` slots (`tones != 0`) - the
//! table `FUN_80068d94` builds into each ProgAtr's +8 reserved word at VAB
//! open and `FUN_80068b98` consumes at program change. This test pins the
//! engine's `VabBank::upload` expansion to that law against every real bank,
//! and pins the corpus facts that make the law load-bearing: the used-slot
//! count equals the packed page count on every bank, and a large share of
//! banks author *sparse* (non-contiguous) program sets - the shape on which
//! raw packed indexing collapses the score onto a few low pages.
//!
//! Skips + passes when the extracted corpus is absent (CI runs without disc
//! data).

use std::path::PathBuf;

use legaia_engine_audio::spu::ram::SpuAllocator;
use legaia_engine_audio::{Spu, VabBank};

fn prot_dir() -> Option<PathBuf> {
    ["extracted/PROT", "../../extracted/PROT"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_dir())
}

#[test]
fn every_real_bank_maps_programs_by_used_slot_rank() {
    let Some(dir) = prot_dir() else {
        eprintln!("[skip] extracted PROT corpus not present");
        return;
    };
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("read extracted/PROT")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    entries.sort();

    let mut banks = 0usize;
    let mut sparse = 0usize;
    let mut rescued = 0usize;
    for path in entries {
        let bytes = std::fs::read(&path).expect("read PROT entry");
        // Scene/music VABs sit behind the `[u32 chunk header][VAB]` wrapper;
        // accept a bare VAB at 0 too.
        let Some(report) = [4usize, 0]
            .iter()
            .find_map(|&off| legaia_vab::parse(&bytes, off).ok().map(|r| (off, r)))
            .map(|(_, r)| r)
        else {
            continue;
        };
        banks += 1;
        let name = path.file_name().unwrap().to_string_lossy().into_owned();

        let used: Vec<usize> = report
            .programs
            .iter()
            .enumerate()
            .filter(|(_, p)| p.tones != 0)
            .map(|(i, _)| i)
            .collect();
        // The file's packed page count always equals the used-slot count -
        // the invariant that makes rank resolution well-defined.
        assert_eq!(
            used.len(),
            report.tones.len(),
            "{name}: used-slot count vs packed page count"
        );
        let is_sparse = used != (0..used.len()).collect::<Vec<_>>();
        if is_sparse {
            sparse += 1;
        }

        let mut spu = Spu::new();
        let mut alloc = SpuAllocator::new(0x1000, 0x10_0000);
        let bank = VabBank::upload(&mut spu, &mut alloc, &report, &bytes);

        // The law itself: used slot -> its rank's packed page, unused -> empty.
        let mut page = 0usize;
        for (slot, prog) in report.programs.iter().enumerate() {
            if prog.tones != 0 {
                let got = &bank.programs[slot];
                assert_eq!(got.mvol, prog.mvol, "{name} slot {slot}: mvol");
                assert_eq!(got.mpan, prog.mpan, "{name} slot {slot}: mpan");
                assert_eq!(
                    got.tones.len(),
                    report.tones[page].len(),
                    "{name} slot {slot}: page {page} row count"
                );
                for (a, b) in got.tones.iter().zip(&report.tones[page]) {
                    assert_eq!((a.vag, a.vol, a.min, a.max), (b.vag, b.vol, b.min, b.max));
                }
                page += 1;
            } else if let Some(p) = bank.programs.get(slot) {
                assert!(p.tones.is_empty(), "{name} slot {slot}: unused but toned");
            }
        }

        // Behavioral leg: every used program number the OLD packed indexing
        // dropped outright (slot >= page count) resolves a tone now.
        for &slot in used.iter().filter(|&&s| s >= report.tones.len()) {
            if let Some(t) = bank.programs[slot]
                .tones
                .iter()
                .find(|t| t.min <= t.max && t.vag > 0)
            {
                assert!(
                    bank.tone_prior(slot, t.min).is_some(),
                    "{name}: program {slot} must resolve (old indexing dropped it)"
                );
                rescued += 1;
            }
        }
    }

    assert!(banks > 100, "corpus should carry many VABs, found {banks}");
    // Non-vacuity: the collapse-triggering shape really exists on disc.
    assert!(
        sparse > 20,
        "expected a large sparse-bank population, found {sparse}"
    );
    assert!(
        rescued > 50,
        "expected many previously-dropped program numbers, found {rescued}"
    );
    eprintln!(
        "[ok] {banks} banks obey the used-slot-rank law ({sparse} sparse, \
         {rescued} program numbers the packed indexing dropped)"
    );
}
