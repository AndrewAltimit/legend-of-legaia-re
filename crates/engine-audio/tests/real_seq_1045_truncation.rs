//! Disc-gated: settle *why* PROT entry 1045's SEQ stream halts on a `0xF4`
//! system-message byte instead of a clean end-of-track.
//!
//! `real_seq_stream_integrity.rs` records that 1045 is the one non-clean
//! stream in the retail corpus; this test pins the cause so it never gets
//! papered over or misdiagnosed as a real, skippable libsnd event.
//!
//! ## What the bytes say
//!
//! The stream is a well-formed track that ends on its own `FF 2F 00`
//! end-of-track marker. A few notes before that marker the parser drifts by
//! exactly **one byte** inside a dense run of running-status note events (near
//! stream offset ~4900-4920, next to an anomalous non-round-BPM `FF 51`
//! tempo). Latched one byte ahead of the true alignment, it reads the volume
//! `Control Change` events of the closing fade (`B5 07 nn` / `B6 07 nn`) as
//! 2-byte VLQ deltas plus running-status note data, sails straight through the
//! real `FF 2F 00`, and finally halts on the first post-track byte it cannot
//! size - a `0xF4`, eleven bytes *past* the true end-of-track.
//!
//! So the `0xF4` is **dead post-track tail**, not a real event the retail
//! driver skips: there is nothing for the parser to resynchronise onto, and
//! "just skip 0xF4 and continue" would decode ~124 KB of trailing bytes (the
//! rest of the container) as bogus music. The parser is right to stop and
//! report `SystemMessage(0xF4)`; the events it kept are a prefix of the track.
//!
//! Two facts prove the diagnosis, both asserted below:
//!  1. A clean `FF 2F 00` marker sits a handful of bytes *before* the `0xF4`
//!     the parser halts on - the track really did end there.
//!  2. Correcting the one-byte drift (drop a single byte anywhere in the
//!     desync window) makes the *whole* stream parse to a clean
//!     `Termination::EndOfTrack` - i.e. the stream is a valid, complete SEQ
//!     modulo one byte, and the `0xF4` is not part of the music.
//!
//! Skips + passes when the extracted corpus / disc is absent.

use std::path::PathBuf;

use legaia_prot::archive::Archive;
use legaia_seq::{Seq, Termination, parse_header_with_len, read_vlq};

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

/// Re-walk the event stream (payload after the header) exactly the way the
/// parser does and return the stream-relative offset of the byte it halts on.
/// `None` if it terminates cleanly.
fn halt_offset(stream: &[u8]) -> Option<usize> {
    let mut pos = 0usize;
    let mut running: Option<u8> = None;
    while pos < stream.len() {
        let (_delta, n) = read_vlq(stream, pos).ok()?;
        pos += n;
        if pos >= stream.len() {
            return None;
        }
        let mut status = stream[pos];
        if status < 0x80 {
            status = running?;
        } else {
            pos += 1;
        }
        if status == 0xFF {
            let kind = stream[pos];
            pos += 1;
            match kind {
                0x51 => pos += 3,
                0x2F => return None, // clean end-of-track
                _ => return Some(pos - 2),
            }
        } else if status & 0xF0 == 0xF0 {
            // System-common / SysEx - the give-up case. `pos` points just past
            // the status byte (explicit) here.
            return Some(pos - 1);
        } else {
            running = Some(status);
            let needs = match status & 0xF0 {
                0x80 | 0x90 | 0xA0 | 0xB0 | 0xE0 => 2,
                0xC0 | 0xD0 => 1,
                _ => return Some(pos),
            };
            pos += needs;
        }
    }
    None
}

#[test]
fn prot_1045_halts_on_a_system_byte() {
    let Some((seq_bytes, _hlen)) = seq_1045() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let seq = Seq::parse(&seq_bytes).expect("header + prefix parse");
    assert_eq!(
        seq.termination,
        Termination::SystemMessage(0xF4),
        "1045 is expected to be the corpus's one non-clean stream, halting on 0xF4"
    );
    assert!(!seq.is_complete());
}

#[test]
fn prot_1045_true_end_of_track_precedes_the_0xf4_halt() {
    let Some((seq_bytes, hlen)) = seq_1045() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let stream = &seq_bytes[hlen..];
    let halt = halt_offset(stream).expect("1045 halts, not clean");
    assert_eq!(
        stream[halt], 0xF4,
        "the halt byte is the 0xF4 system message"
    );

    // A well-formed `FF 2F 00` end-of-track sits a few bytes before the halt:
    // the real track ended there and everything from it to the 0xF4 (and far
    // beyond) is post-track tail the desynced parser walked into.
    let window_start = halt.saturating_sub(32);
    let eot = (window_start..halt)
        .find(|&i| stream[i] == 0xFF && stream.get(i + 1) == Some(&0x2F))
        .expect("a real FF 2F end-of-track marker precedes the 0xF4 halt");
    assert!(
        halt - eot < 32 && halt > eot,
        "true EOT at {eot} is just before the 0xF4 halt at {halt}"
    );
    // Retail writes a trailing 0x00 after FF 2F; confirm this is that marker.
    assert_eq!(stream.get(eot + 2), Some(&0x00), "FF 2F 00 end-of-track");
}

#[test]
fn prot_1045_is_a_complete_track_modulo_a_one_byte_desync() {
    let Some((seq_bytes, hlen)) = seq_1045() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    // The drift is a single byte in the dense running-status note region a
    // little before the track's end. Dropping one byte anywhere in that window
    // re-phases the parse so it stays aligned all the way to the real
    // `FF 2F 00`, giving a clean EndOfTrack. That is only possible if the whole
    // stream up to the marker is a valid track - i.e. the 0xF4 is genuinely not
    // part of the music.
    let desync_window = 4900usize..4922;
    let mut resynced = None;
    for cut in desync_window.clone() {
        let mut m = seq_bytes.clone();
        let at = hlen + cut;
        if at >= m.len() {
            continue;
        }
        m.remove(at);
        let Ok(s) = Seq::parse(&m) else { continue };
        if s.termination == Termination::EndOfTrack {
            resynced = Some((cut, s.events.len()));
            break;
        }
    }
    let (cut, events) = resynced.expect(
        "dropping one byte in the desync window must yield a clean EndOfTrack - \
         proving 1045 is a valid, complete track minus a single desync byte",
    );
    // The clean track is far shorter than the baseline event count, which is
    // inflated by the garbage the desynced parser reads past the real EOT.
    let baseline = Seq::parse(&seq_bytes).unwrap().events.len();
    assert!(
        events < baseline,
        "clean parse ({events} events at cut {cut}) is shorter than the desynced \
         baseline ({baseline}) that over-runs the true end-of-track"
    );
}
