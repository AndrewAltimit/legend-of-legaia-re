//! Disc-gated: a meta event is a running status, and PROT entry 1045 is the
//! track that needs it.
//!
//! The retail SEQ decoder `FUN_80063CEC` latches `0xFF` into the channel's
//! running-status byte when it reads a meta (`sb v0,0x16(s3)` at
//! `0x80063EE4`), and a later data byte under that latch is dispatched as the
//! next meta's **kind** (`0x80063F44` -> `0x80064014`). 1045's closing
//! ritardando relies on it: `FF 51 0E C4 3E` (62 BPM), a delta, then
//! `51 0F 42 40` - a second tempo (60 BPM) with no `FF`.
//!
//! A parser that kept the previous *channel* status across the meta read
//! `51 0F 42` as a note, fell one byte out of phase, read the closing volume
//! fade (`B5 07 nn` / `B6 07 nn`) as long deltas and notes, walked through
//! the real `FF 2F 00` and halted on a `0xF4` eleven bytes past it - so the
//! track's last bars were garbage and it never reached its end. That was
//! recorded as a one-byte "desync" in an otherwise valid stream; it was the
//! parser.
//!
//! Skips + passes when the extracted corpus / disc is absent.

use std::path::PathBuf;

use legaia_prot::archive::Archive;
use legaia_seq::{EventBody, MetaMessage, Seq, Termination, parse_header_with_len};

const PROT_1045: usize = 1045;

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

/// The 1045 SEQ, sliced from `pQES`: `(seq_bytes, header_len)`.
fn seq_1045() -> Option<(Vec<u8>, usize)> {
    let extracted = extracted_dir()?;
    let mut archive = Archive::open(&extracted.join("PROT.DAT")).ok()?;
    let entry = archive.entries.get(PROT_1045)?.clone();
    let mut bytes = Vec::new();
    archive.read_entry(&entry, &mut bytes).ok()?;
    let at = bytes.windows(4).position(|w| w == b"pQES")?;
    let seq = bytes[at..].to_vec();
    let (_h, hlen) = parse_header_with_len(&seq).ok()?;
    Some((seq, hlen))
}

#[test]
fn prot_1045_parses_to_its_own_end_of_track() {
    let Some((seq_bytes, hlen)) = seq_1045() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let seq = Seq::parse(&seq_bytes).expect("parse");
    assert_eq!(seq.termination, Termination::EndOfTrack);
    assert!(seq.is_complete());

    // The track ends on its own `FF 2F 00`, the last such marker before the
    // post-track tail, and nothing the parser kept lies past it.
    let stream = &seq_bytes[hlen..];
    let eot = stream
        .windows(3)
        .position(|w| w == [0xFF, 0x2F, 0x00])
        .expect("an FF 2F 00 marker");
    assert_eq!(stream.get(eot + 11), Some(&0xF4), "the old halt byte");

    // The closing ritardando: the last two tempo events are 62 then 60 BPM,
    // the second carried by the meta running status.
    let tempos: Vec<u32> = seq
        .events
        .iter()
        .filter_map(|e| match e.body {
            EventBody::Meta(MetaMessage::SetTempo { us_per_qn }) => Some(us_per_qn),
            _ => None,
        })
        .collect();
    assert_eq!(tempos[tempos.len() - 2..], [0x0E_C43E, 0x0F_4240]);
    eprintln!(
        "[ok] 1045: {} events, clean EOT at stream +{eot}, closing tempos {:?}",
        seq.events.len(),
        &tempos[tempos.len() - 2..]
    );
}

/// The rule in isolation: after a meta, a data byte is the next meta's kind.
#[test]
fn a_data_byte_after_a_meta_is_the_next_meta() {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"pQES");
    buf.extend_from_slice(&1u32.to_be_bytes());
    buf.extend_from_slice(&480u16.to_be_bytes());
    buf.extend_from_slice(&[0x07, 0xA1, 0x20]);
    buf.extend_from_slice(&[4, 2]);
    // delta 0, NoteOn ch0; delta 0, FF 51 tempo; delta 0, 51 tempo (no FF);
    // delta 0, 2F (end of track, no FF).
    buf.extend_from_slice(&[0x00, 0x90, 0x40, 0x64]);
    buf.extend_from_slice(&[0x00, 0xFF, 0x51, 0x0E, 0xC4, 0x3E]);
    buf.extend_from_slice(&[0x00, 0x51, 0x0F, 0x42, 0x40]);
    buf.extend_from_slice(&[0x00, 0x2F, 0x00]);
    let seq = Seq::parse(&buf).expect("parse");
    assert_eq!(seq.termination, Termination::EndOfTrack);
    let tempos: Vec<u32> = seq
        .events
        .iter()
        .filter_map(|e| match e.body {
            EventBody::Meta(MetaMessage::SetTempo { us_per_qn }) => Some(us_per_qn),
            _ => None,
        })
        .collect();
    assert_eq!(tempos, [0x0E_C43E, 0x0F_4240]);
}
