//! Disc-gated corpus sweep: does ANY retail Legaia SEQ carry expressive
//! channel events - MIDI pitch-bend (`0xEn`) or aftertouch (`0xDn`/`0xAn`)?
//!
//! Finding: the retail BGM corpus **does** use pitch-bend (hundreds of `0xEn`
//! events, concentrated in a handful of music banks) but uses **no** channel
//! or polyphonic aftertouch. So the sequencer must act on pitch-bend to play
//! the score faithfully, while the aftertouch families remain consumer-free
//! (recognize-but-ignore is correct for those). This sweep is the data the
//! "should we wire it" decision rests on; if a future disc/region changes the
//! aftertouch picture, the relevant assertion fires and the question reopens.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.

use std::collections::BTreeMap;
use std::path::PathBuf;

use legaia_prot::archive::Archive;
use legaia_seq::{ChannelMessage, EventBody, Seq};

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() {
            return Some(d);
        }
    }
    None
}

/// Find every `pQES` SEQ magic offset in `buf` that parses to a structurally
/// valid SEQ. Scene BGM entries wrap the SEQ behind a VAB + chunk header, so
/// the magic rarely sits at offset 0; some entries hold more than one.
fn find_all_seqs(buf: &[u8]) -> Vec<usize> {
    const MAGIC: &[u8; 4] = b"pQES";
    let mut hits = Vec::new();
    if buf.len() < MAGIC.len() + 1 {
        return hits;
    }
    let scan_end = buf.len().saturating_sub(MAGIC.len());
    for i in 0..scan_end {
        if &buf[i..i + MAGIC.len()] == MAGIC && Seq::parse(&buf[i..]).is_ok() {
            hits.push(i);
        }
    }
    hits
}

#[test]
fn retail_seq_uses_pitch_bend_but_no_aftertouch() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    let mut archive = Archive::open(&extracted.join("PROT.DAT")).expect("open PROT");

    let mut seq_files = 0u32;
    let mut total_events = 0u64;
    let mut pitch_bend = 0u32;
    let mut channel_aftertouch = 0u32;
    let mut poly_aftertouch = 0u32;
    // Entries that carry at least one expressive event, for the report.
    let mut flagged: Vec<(usize, usize)> = Vec::new();

    let entries = archive.entries.clone();
    for (idx, entry) in entries.iter().enumerate() {
        let mut bytes = Vec::new();
        if archive.read_entry(entry, &mut bytes).is_err() {
            continue;
        }
        for off in find_all_seqs(&bytes) {
            let Ok(seq) = Seq::parse(&bytes[off..]) else {
                continue;
            };
            let s = seq.event_summary();
            seq_files += 1;
            total_events += seq.events.len() as u64;
            pitch_bend += s.pitch_bend;
            channel_aftertouch += s.channel_aftertouch;
            poly_aftertouch += s.poly_aftertouch;
            let expressive = s.pitch_bend + s.channel_aftertouch + s.poly_aftertouch;
            if expressive > 0 {
                flagged.push((idx, off));
            }
        }
    }

    eprintln!(
        "[seq-sweep] {seq_files} SEQ files, {total_events} events; \
         pitch_bend={pitch_bend} chan_aftertouch={channel_aftertouch} \
         poly_aftertouch={poly_aftertouch}"
    );
    for (idx, off) in &flagged {
        eprintln!("[seq-sweep] expressive events in entry {idx} @ +0x{off:X}");
    }

    assert!(
        seq_files > 0,
        "the corpus must contain at least one parseable SEQ"
    );

    // The decisive finding: retail Legaia's score DOES bend pitch (so the
    // sequencer has to honor `0xEn` to sound right), but never uses channel
    // or polyphonic aftertouch. Pin both halves so a regression in the SEQ
    // parser or a different region's disc is caught.
    assert!(
        pitch_bend > 0,
        "retail SEQ corpus must carry pitch-bend events (the sequencer wires them)"
    );
    assert_eq!(
        channel_aftertouch, 0,
        "unexpected channel-aftertouch events in retail SEQ corpus"
    );
    assert_eq!(
        poly_aftertouch, 0,
        "unexpected poly-aftertouch events in retail SEQ corpus"
    );
}

/// Controller (`0xBn`) usage census across the whole SEQ corpus. The
/// sequencer acts on a small set of control changes - CC7 (channel volume),
/// CC10 (pan), and CC99 (the NRPN-style loop markers). This sweep pins what
/// the retail score actually emits so the "are we missing a modulation
/// source" question stays answered from data:
///
/// - CC99 only ever carries the two loop-marker values 20 (Loop Start) and
///   30 (Loop Forever); there is no third value the loop handler would drop.
/// - CC6 (Data Entry) is a constant 127 init the score emits ~once per file;
///   it varies nothing, so ignoring it is correct (it is not, e.g., a
///   per-track pitch-bend-range parameter - that would not be constant).
/// - Expression (CC11) and reverb-depth (CC91) never appear, so per-channel
///   volume swells and per-cue reverb sends are NOT carried in the SEQ - the
///   live reverb-enable source for BGM, if any, lives elsewhere.
#[test]
fn retail_seq_controller_census() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    const CC_VOLUME: u8 = 0x07;
    const CC_PAN: u8 = 0x0A;
    const CC_DATA_ENTRY: u8 = 0x06;
    const CC_EXPRESSION: u8 = 0x0B;
    const CC_REVERB_DEPTH: u8 = 0x5B;
    const CC_LOOP_MARKER: u8 = 0x63;

    let mut archive = Archive::open(&extracted.join("PROT.DAT")).expect("open PROT");
    let mut by_controller: BTreeMap<u8, u64> = BTreeMap::new();
    // Values seen for the two NRPN-style controllers we depend on.
    let mut loop_marker_values: BTreeMap<u8, u64> = BTreeMap::new();
    let mut data_entry_values: BTreeMap<u8, u64> = BTreeMap::new();

    let entries = archive.entries.clone();
    for entry in &entries {
        let mut bytes = Vec::new();
        if archive.read_entry(entry, &mut bytes).is_err() {
            continue;
        }
        for off in find_all_seqs(&bytes) {
            let Ok(seq) = Seq::parse(&bytes[off..]) else {
                continue;
            };
            for ev in &seq.events {
                if let EventBody::Channel {
                    message: ChannelMessage::ControlChange { control, value },
                    ..
                } = ev.body
                {
                    *by_controller.entry(control).or_default() += 1;
                    match control {
                        CC_LOOP_MARKER => *loop_marker_values.entry(value).or_default() += 1,
                        CC_DATA_ENTRY => *data_entry_values.entry(value).or_default() += 1,
                        _ => {}
                    }
                }
            }
        }
    }

    eprintln!("[cc-census] controller usage (controller -> count):");
    for (ctrl, count) in &by_controller {
        eprintln!("  CC{ctrl:>3} (0x{ctrl:02X}): {count}");
    }

    // The workhorse controllers must be present (a parser regression that
    // silently dropped them would zero these).
    assert!(
        by_controller.get(&CC_VOLUME).copied().unwrap_or(0) > 0,
        "retail score must use channel volume (CC7)"
    );
    assert!(
        by_controller.get(&CC_PAN).copied().unwrap_or(0) > 0,
        "retail score must use pan (CC10)"
    );

    // Loop markers: only the two handled values ever appear, and both do.
    assert!(
        loop_marker_values.keys().all(|&v| v == 20 || v == 30),
        "CC99 carries a loop-marker value the sequencer does not handle: {loop_marker_values:?}"
    );
    assert!(
        loop_marker_values.contains_key(&20) && loop_marker_values.contains_key(&30),
        "expected both Loop Start (20) and Loop Forever (30) markers in the corpus"
    );

    // Data Entry is a constant init; assert it never varies (so ignoring it
    // can't be hiding a per-track parameter).
    assert!(
        data_entry_values.keys().all(|&v| v == 127),
        "CC6 (Data Entry) carries a non-127 value - reinvestigate: {data_entry_values:?}"
    );

    // No expression or reverb-depth source in the score.
    assert_eq!(
        by_controller.get(&CC_EXPRESSION).copied().unwrap_or(0),
        0,
        "unexpected expression (CC11) events - wire per-channel volume swell"
    );
    assert_eq!(
        by_controller.get(&CC_REVERB_DEPTH).copied().unwrap_or(0),
        0,
        "unexpected reverb-depth (CC91) events - the SEQ carries a reverb send after all"
    );
}
