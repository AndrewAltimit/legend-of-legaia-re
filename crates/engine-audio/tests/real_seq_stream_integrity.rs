//! Disc-gated corpus sweep: does the SEQ parser decode every retail BGM
//! stream *to its own end-of-track marker*, and is the meta-event grammar
//! it assumes actually the grammar on the disc?
//!
//! This is the parser half of the note-level BGM parity work. A stream the
//! parser gives up on still ends in a synthetic `EndOfTrack`, so a truncated
//! track is indistinguishable from a complete one by its events alone - it
//! just silently stops playing partway through, which is exactly what
//! "missing notes" looks like from the outside. `Seq::termination` is the
//! only way to tell, and this test is what keeps it honest.
//!
//! Two things are measured:
//!
//! 1. **Termination.** Every SEQ-bearing PROT entry is parsed and its
//!    [`Termination`] recorded. The corpus is overwhelmingly clean; the test
//!    pins the count of non-clean streams so a parser change that starts
//!    truncating more tracks fails loudly instead of quietly losing music.
//!
//! 2. **Meta grammar.** The corpus is swept for which meta types occur and
//!    what the tempo payload looks like. Legaia's meta events carry **no**
//!    MIDI variable-length `length` field - `FF 51` is followed directly by
//!    three big-endian tempo bytes. The evidence is that reading it that way
//!    yields exact round BPM values across the corpus; reading a phantom
//!    length byte would shift every tempo into nonsense. The test asserts
//!    that roundness, which is what actually falsifies the phantom-length
//!    reading.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.

use std::path::PathBuf;

use legaia_prot::archive::Archive;
use legaia_seq::{EventBody, MetaMessage, Seq, Termination};

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() {
            return Some(d);
        }
    }
    None
}

/// Every PROT entry that carries a `pQES` stream, parsed.
fn corpus() -> Option<Vec<(u32, Seq)>> {
    let extracted = extracted_dir()?;
    std::env::var_os("LEGAIA_DISC_BIN")?;
    let mut archive = Archive::open(&extracted.join("PROT.DAT")).ok()?;
    let mut out = Vec::new();
    for idx in 0..archive.entries.len() {
        let entry = archive.entries[idx].clone();
        let mut bytes = Vec::new();
        if archive.read_entry(&entry, &mut bytes).is_err() {
            continue;
        }
        let Some(at) = bytes.windows(4).position(|w| w == b"pQES") else {
            continue;
        };
        if let Ok(seq) = Seq::parse(&bytes[at..]) {
            out.push((idx as u32, seq));
        }
    }
    Some(out)
}

#[test]
fn retail_seq_streams_decode_to_their_own_end_of_track() {
    let Some(corpus) = corpus() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    assert!(
        corpus.len() > 50,
        "expected a substantial SEQ corpus, found {}",
        corpus.len()
    );

    let unclean: Vec<_> = corpus
        .iter()
        .filter(|(_, s)| !s.is_complete())
        .map(|(i, s)| (*i, s.termination))
        .collect();

    eprintln!(
        "[seq-integrity] {} SEQ streams, {} clean, {} truncated",
        corpus.len(),
        corpus.len() - unclean.len(),
        unclean.len()
    );
    for (idx, t) in &unclean {
        eprintln!("[seq-integrity]   PROT {idx} terminated {t:?}");
    }

    // The corpus is almost entirely clean. This is a regression pin, not an
    // aspiration: if a parser change starts truncating more streams, that is
    // music silently going missing and the test must fail.
    assert!(
        unclean.len() <= 1,
        "expected at most 1 non-clean SEQ stream, found {}: {:?}",
        unclean.len(),
        unclean
    );
}

#[test]
fn retail_meta_grammar_is_tempo_and_end_of_track_only() {
    let Some(corpus) = corpus() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };

    let mut tempos = Vec::new();
    let mut eots = 0usize;
    for (_, seq) in &corpus {
        for ev in &seq.events {
            if let EventBody::Meta(m) = &ev.body {
                match m {
                    MetaMessage::SetTempo { us_per_qn } => tempos.push(*us_per_qn),
                    MetaMessage::EndOfTrack => eots += 1,
                    other => panic!("unexpected meta in retail corpus: {other:?}"),
                }
            }
        }
    }
    assert!(eots > 0, "corpus must contain end-of-track markers");
    assert!(!tempos.is_empty(), "corpus must contain tempo events");

    // The falsification test for a phantom MIDI length byte. Reading `FF 51`
    // as [3 big-endian tempo bytes] yields exact round BPM; reading it as
    // [length][tempo] would shift every payload by one byte and destroy that.
    let round = tempos
        .iter()
        .filter(|us| **us > 0)
        .filter(|us| {
            let bpm = 60_000_000.0 / **us as f64;
            (bpm - bpm.round()).abs() < 0.01
        })
        .count();
    eprintln!(
        "[seq-integrity] {} tempo events, {} at an exact round BPM",
        tempos.len(),
        round
    );
    let ratio = round as f64 / tempos.len() as f64;
    assert!(
        ratio > 0.9,
        "tempo payload should read as 3 raw big-endian bytes (no length \
         field): only {round}/{} tempos land on a round BPM",
        tempos.len()
    );
}

#[test]
fn truncated_stream_is_reported_not_disguised() {
    // A system-message byte the parser cannot size. It must stop, keep the
    // events it had, and *say* it stopped - the trailing synthetic
    // EndOfTrack must not be mistakable for a real one.
    let mut buf = Vec::new();
    buf.extend_from_slice(b"pQES");
    buf.extend_from_slice(&1u32.to_be_bytes()); // Legaia version
    buf.extend_from_slice(&480u16.to_be_bytes()); // ppqn
    buf.extend_from_slice(&[0x07, 0xA1, 0x20]); // tempo u24
    buf.extend_from_slice(&[4, 2]); // time signature
    // One real NoteOn, then a bare 0xF4 system byte.
    buf.extend_from_slice(&[0x00, 0x90, 0x40, 0x64]);
    buf.extend_from_slice(&[0x00, 0xF4]);

    let seq = Seq::parse(&buf).expect("header + prefix parse");
    assert_eq!(seq.termination, Termination::SystemMessage(0xF4));
    assert!(!seq.is_complete(), "a truncated stream is not complete");
    assert!(
        matches!(
            seq.events.last().map(|e| &e.body),
            Some(EventBody::Meta(MetaMessage::EndOfTrack))
        ),
        "the event list still ends well-formed for consumers"
    );
}

#[test]
fn clean_stream_reports_end_of_track() {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"pQES");
    buf.extend_from_slice(&1u32.to_be_bytes());
    buf.extend_from_slice(&480u16.to_be_bytes());
    buf.extend_from_slice(&[0x07, 0xA1, 0x20]);
    buf.extend_from_slice(&[4, 2]);
    buf.extend_from_slice(&[0x00, 0x90, 0x40, 0x64]);
    buf.extend_from_slice(&[0x00, 0xFF, 0x2F]);

    let seq = Seq::parse(&buf).expect("parse");
    assert_eq!(seq.termination, Termination::EndOfTrack);
    assert!(seq.is_complete());
}
