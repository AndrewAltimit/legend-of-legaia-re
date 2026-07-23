//! Disc-gated: retail BGM `ProgramChange`s to UNUSED VAB slots, and the port's
//! alias handling of them.
//!
//! `vab_bind.rs` reproduces retail's VAB-open mapping (`FUN_80068d94`): a
//! running used-program counter is written into each `ProgAtr`'s `+8` word
//! *before* the used-slot check, and the program-change consumer
//! (`FUN_80068b98`) reads it back as the tone-page index. Because the counter
//! is stored before the check, a `ProgramChange` to an **unused** slot aliases
//! onto the SAME page the next used slot gets (and past the last used slot the
//! index runs beyond the tone region, where retail reads garbage).
//!
//! This is exercised by real data: a handful of retail tracks program-change to
//! an unused slot. They split three ways:
//!   - a used slot follows, so retail aliases to a *valid different page* and
//!     the notes play -> the port must resolve to that same page, not silence
//!     (PROT 868 prog 5 -> page of slot 10; PROT 996 prog 19 -> page of slot 23);
//!   - no used slot follows, so retail's alias runs past the tone region and
//!     reads garbage (PROT 994 prog 42) -> the port leaves it empty (silent),
//!     which is at least as faithful as replaying undefined bytes;
//!   - the channel plays no notes after the change, so it is moot (PROT 988).
//!
//! This test both PINS the census (which entries do it) and VERIFIES the port's
//! alias: every in-range unused-slot ProgramChange resolves to exactly the tone
//! page retail's rank rule selects.
//!
//! Skips + passes when the extracted corpus / disc is absent.

use std::path::PathBuf;

use legaia_engine_audio::spu::ram::SpuAllocator;
use legaia_engine_audio::{Spu, VabBank};
use legaia_prot::archive::Archive;
use legaia_seq::{ChannelMessage, EventBody, Seq};
use legaia_vab::VagAtr;

fn extracted_dir() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for p in ["extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() {
            return Some(d);
        }
    }
    None
}

/// A comparable fingerprint of a tone page, so two pages can be checked equal
/// without `VagAtr: PartialEq`.
fn page_sig(tones: &[VagAtr]) -> Vec<(u8, u8, i16, u8, u8)> {
    tones
        .iter()
        .map(|t| (t.min, t.max, t.vag, t.center, t.prior))
        .collect()
}

/// One `ProgramChange` to a slot that is UNUSED in the VAB file.
#[derive(Debug, Clone)]
struct UnusedHit {
    prot: usize,
    channel: u8,
    program: u8,
    /// The used slot retail aliases to (first used slot `>= program`), or
    /// `None` when the alias index runs past the tone region (retail garbage).
    retail_alias: Option<usize>,
    /// NoteOns (velocity > 0) on this channel after the change, before the next
    /// change on the same channel - i.e. whether the divergence is audible.
    notes_after: usize,
    /// Does the port now resolve this ProgramChange the way retail's rank rule
    /// prescribes? In-range: the port's page equals the aliased used slot's
    /// page (non-empty). Past-region: the port leaves it empty (silence).
    port_matches_retail: bool,
}

fn sweep() -> Option<Vec<UnusedHit>> {
    let extracted = extracted_dir()?;
    let mut archive = Archive::open(&extracted.join("PROT.DAT")).ok()?;
    let mut hits = Vec::new();
    for idx in 0..archive.entries.len() {
        let entry = archive.entries[idx].clone();
        let mut bytes = Vec::new();
        if archive.read_entry(&entry, &mut bytes).is_err() {
            continue;
        }
        // The bank that rides in the same container: wrapped at +4, or bare.
        let Some(report) = [4usize, 0]
            .iter()
            .find_map(|&off| legaia_vab::parse(&bytes, off).ok())
        else {
            continue;
        };
        let Some(at) = bytes.windows(4).position(|w| w == b"pQES") else {
            continue;
        };
        let Ok(seq) = Seq::parse(&bytes[at..]) else {
            continue;
        };

        // Build the bank exactly as the engine does.
        let mut spu = Spu::new();
        let mut alloc = SpuAllocator::new(0x1000, 0x10_0000);
        let bank = VabBank::upload(&mut spu, &mut alloc, &report, &bytes);

        // A slot is "unused" when the file marks it so (tones == 0). Retail's
        // page index for such a slot is the count of used slots below it; the
        // aliased page is the next used slot's page (first used slot >= p).
        let used_slots: Vec<usize> = report
            .programs
            .iter()
            .enumerate()
            .filter(|(_, pr)| pr.tones != 0)
            .map(|(i, _)| i)
            .collect();
        let file_unused = |p: usize| report.programs.get(p).is_none_or(|pr| pr.tones == 0);

        for (ei, ev) in seq.events.iter().enumerate() {
            let EventBody::Channel {
                channel,
                message: ChannelMessage::ProgramChange { program },
            } = &ev.body
            else {
                continue;
            };
            let p = *program as usize;
            if !file_unused(p) {
                continue;
            }
            let retail_alias = used_slots.iter().find(|&&s| s >= p).copied();
            let expected_page = used_slots.iter().filter(|&&s| s < p).count();

            // What the port produced for this slot.
            let port_page = bank.programs.get(p).map(|pr| pr.tones.as_slice());
            let port_matches_retail = match retail_alias {
                // In range: the port must resolve to exactly the aliased page.
                Some(_) => port_page
                    .zip(report.tones.get(expected_page))
                    .is_some_and(|(got, want)| !got.is_empty() && page_sig(got) == page_sig(want)),
                // Past the tone region: the port must be empty/absent (silence),
                // deliberately NOT replaying retail's out-of-region garbage.
                None => port_page.is_none_or(|t| t.is_empty()),
            };

            let notes_after = seq.events[ei + 1..]
                .iter()
                .take_while(|e| {
                    !matches!(
                        &e.body,
                        EventBody::Channel { channel: c2, message: ChannelMessage::ProgramChange { .. } }
                            if *c2 == *channel
                    )
                })
                .filter(|e| {
                    matches!(
                        &e.body,
                        EventBody::Channel { channel: c2, message: ChannelMessage::NoteOn { velocity, .. } }
                            if *c2 == *channel && *velocity > 0
                    )
                })
                .count();
            hits.push(UnusedHit {
                prot: idx,
                channel: *channel,
                program: *program,
                retail_alias,
                notes_after,
                port_matches_retail,
            });
        }
    }
    Some(hits)
}

#[test]
fn unused_slot_program_changes_alias_like_retail() {
    let Some(hits) = sweep() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };

    for h in &hits {
        eprintln!(
            "[pc-unused] PROT {} ch{} PC->prog {} retail_alias={:?} notes_after={} port_ok={}",
            h.prot, h.channel, h.program, h.retail_alias, h.notes_after, h.port_matches_retail
        );
    }

    // The census is exercised: real BGM data program-changes to unused slots.
    assert!(
        !hits.is_empty(),
        "expected the unused-slot case to be exercised by real data"
    );

    // Pinned disc census: eight such ProgramChanges across four entries. A
    // change here means the corpus, the VAB parser, or the used-slot rule moved.
    assert_eq!(hits.len(), 8, "unused-slot ProgramChange count: {hits:?}");
    let entries: std::collections::BTreeSet<usize> = hits.iter().map(|h| h.prot).collect();
    assert_eq!(
        entries,
        [868usize, 988, 994, 996].into_iter().collect(),
        "offending PROT entries"
    );

    // THE FIX: every unused-slot ProgramChange now resolves the way retail's
    // rank rule prescribes - in-range ones alias to the next used slot's page,
    // past-region ones stay silent.
    let mismatched: Vec<_> = hits.iter().filter(|h| !h.port_matches_retail).collect();
    assert!(
        mismatched.is_empty(),
        "these unused-slot ProgramChanges do not match retail's alias: {mismatched:?}"
    );

    // The load-bearing bucket: retail aliases to a valid *different* page AND
    // notes follow. Before the fix the port dropped these to silence; now they
    // must resolve (non-empty aliased page), so the instruments actually play.
    let audible_alias: Vec<(usize, u8)> = hits
        .iter()
        .filter(|h| h.retail_alias.is_some() && h.notes_after > 0 && h.port_matches_retail)
        .map(|h| (h.prot, h.program))
        .collect();
    assert!(
        audible_alias.contains(&(868, 5)) && audible_alias.contains(&(996, 19)),
        "PROT 868 prog 5 and PROT 996 prog 19 must now resolve to their aliased page: \
         {audible_alias:?}"
    );
}
