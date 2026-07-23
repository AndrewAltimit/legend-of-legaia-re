//! Disc-gated: does any retail BGM SEQ `ProgramChange` target a VAB program
//! slot the port would silently drop?
//!
//! `vab_bind.rs` documents a deliberate divergence: retail's VAB-open builds a
//! running used-program counter into each `ProgAtr`'s `+8` word (`FUN_80068d94`)
//! and the program-change consumer reads it back as the tone-page index
//! (`FUN_80068b98`). Because the counter is stored *before* the used-slot
//! check, a `ProgramChange` to an **unused** slot aliases onto the next used
//! slot's page (and past the last used slot, reads garbage beyond the tone
//! region). The port instead gives unused slots an empty page, so their notes
//! simply don't resolve - silence.
//!
//! Nobody had measured whether that divergence is ever *exercised*. This test
//! sweeps every in-container `[VAB][SEQ]` pair (the same pQES corpus
//! `real_seq_stream_integrity.rs` sweeps, restricted to entries whose VAB rides
//! along in the same PROT entry) and, for every `ProgramChange`, asks whether
//! the port's own `VabBank` would drop it (`programs[p]` absent or empty).
//!
//! Result (measured, pinned below): **yes, it happens.** A handful of real
//! tracks program-change to an unused slot. They split three ways:
//!   - retail aliases to a *valid different page* and notes follow -> the port
//!     silently loses instruments retail plays (PROT 868 prog 5 -> page of
//!     slot 10; PROT 996 prog 19 -> page of slot 23);
//!   - retail's alias index runs *past* the tone region so retail itself reads
//!     garbage (PROT 994 prog 42) - the port's silence is at least as faithful;
//!   - the channel plays no notes after the change, so it is moot either way
//!     (PROT 988 prog 127).
//!
//! This test MEASURES the divergence; it does not change it (`vab_bind.rs`'s
//! aliasing is out of scope here). The first bucket is a real, exercised loss
//! and is flagged for a follow-up fix.
//!
//! Skips + passes when the extracted corpus / disc is absent.

use std::path::PathBuf;

use legaia_engine_audio::spu::ram::SpuAllocator;
use legaia_engine_audio::{Spu, VabBank};
use legaia_prot::archive::Archive;
use legaia_seq::{ChannelMessage, EventBody, Seq};

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

/// One `ProgramChange` the port would silently drop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct UnusedHit {
    prot: usize,
    channel: u8,
    program: u8,
    /// The used slot retail would alias to (first used slot `>= program`), or
    /// `None` when the alias index runs past the tone region (retail garbage).
    retail_alias: Option<usize>,
    /// NoteOns (velocity > 0) on this channel after the change, before the next
    /// change on the same channel - i.e. whether the divergence is audible.
    notes_after: usize,
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

        // Build the bank exactly as the engine does; a program the port drops
        // is one whose slot is absent or carries an empty tone page.
        let mut spu = Spu::new();
        let mut alloc = SpuAllocator::new(0x1000, 0x10_0000);
        let bank = VabBank::upload(&mut spu, &mut alloc, &report, &bytes);
        let port_drops = |p: usize| bank.programs.get(p).is_none_or(|pr| pr.tones.is_empty());

        // Used slots straight off the file - the rank space retail's +8 counter
        // walks. Retail's page index for program p is the count of used slots
        // below p; that is in-range iff a used slot >= p exists.
        let used_slots: Vec<usize> = report
            .programs
            .iter()
            .enumerate()
            .filter(|(_, pr)| pr.tones != 0)
            .map(|(i, _)| i)
            .collect();

        for (ei, ev) in seq.events.iter().enumerate() {
            let EventBody::Channel {
                channel,
                message: ChannelMessage::ProgramChange { program },
            } = &ev.body
            else {
                continue;
            };
            let p = *program as usize;
            if !port_drops(p) {
                continue;
            }
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
                retail_alias: used_slots.iter().find(|&&s| s >= p).copied(),
                notes_after,
            });
        }
    }
    Some(hits)
}

#[test]
fn some_real_track_program_changes_to_an_unused_slot() {
    let Some(hits) = sweep() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };

    for h in &hits {
        eprintln!(
            "[pc-unused] PROT {} ch{} PC->prog {} retail_alias={:?} notes_after={}",
            h.prot, h.channel, h.program, h.retail_alias, h.notes_after
        );
    }

    // The headline: the documented alias-to-unused-slot divergence is real -
    // retail BGM data does program-change to slots the port drops to silence.
    assert!(
        !hits.is_empty(),
        "expected the unused-slot divergence to be exercised by real data"
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

    // The load-bearing bucket: retail aliases to a valid *different* page AND
    // notes follow, so the port silently drops instruments retail plays. This
    // is the real regression to guard and the follow-up-fix trigger.
    let audible_alias_loss: Vec<(usize, u8)> = hits
        .iter()
        .filter(|h| h.retail_alias.is_some() && h.notes_after > 0)
        .map(|h| (h.prot, h.program))
        .collect();
    assert!(
        audible_alias_loss.contains(&(868, 5)) && audible_alias_loss.contains(&(996, 19)),
        "PROT 868 prog 5 and PROT 996 prog 19 are the audible alias-loss cases: \
         {audible_alias_loss:?}"
    );
}
