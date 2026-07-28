//! Disc-gated end-to-end: pull a real SEQ + VAB pair out of a music_01
//! PROT entry, upload them through the engine-audio chain, render PCM,
//! and verify the chain doesn't panic and produces sane state once the
//! sequencer fires its first NoteOn.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.

use std::path::PathBuf;

use legaia_engine_audio::sequencer::Sequencer;
use legaia_engine_audio::spu::ram::SpuAllocator;
use legaia_engine_audio::{Spu, VabBank};
use legaia_prot::archive::Archive;
use legaia_prot::cdname;
use legaia_seq::Seq;
use legaia_vab::parse as parse_vab;

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn real_seq_vab_chain_renders_through_spu_without_panic() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    let mut archive = Archive::open(&extracted.join("PROT.DAT")).expect("open PROT");
    let map = cdname::parse(&extracted.join("CDNAME.TXT")).expect("parse CDNAME");
    // Retail extraction frame: the archive's entry list is extraction-
    // indexed, so the raw #define window would scan two entries past the
    // block (and skip its first two).
    let (start, end) = cdname::block_range_for_name_extraction(&map, "music_01")
        .expect("music_01 block in CDNAME");

    // Walk the music_01 block until we find an entry that holds both pBAV
    // and pQES - that's a single-entry VAB+SEQ pair.
    let mut chosen: Option<(u32, Vec<u8>, usize, usize)> = None;
    for idx in start..end {
        let entry = archive.entries[idx as usize].clone();
        let mut bytes = Vec::new();
        archive.read_entry(&entry, &mut bytes).expect("read entry");
        let vab_at = bytes.windows(4).position(|w| w == b"pBAV");
        let seq_at = bytes.windows(4).position(|w| w == b"pQES");
        if let (Some(v), Some(s)) = (vab_at, seq_at) {
            chosen = Some((idx, bytes, v, s));
            break;
        }
    }
    let (idx, bytes, vab_off, seq_off) =
        chosen.expect("music_01 must contain at least one VAB+SEQ pair");

    let vab_report = parse_vab(&bytes, vab_off).expect("parse real VAB");
    eprintln!(
        "[real-bgm] entry={idx} vab_off=0x{vab_off:X} seq_off=0x{seq_off:X} progs={} tones={} vags={}",
        vab_report.programs.len(),
        vab_report.tones.len(),
        vab_report.vag_samples.len()
    );
    assert!(
        !vab_report.programs.is_empty(),
        "real VAB must declare at least one program"
    );

    let seq_bytes = &bytes[seq_off..];
    let seq = Seq::parse(seq_bytes).expect("parse real SEQ");
    eprintln!(
        "[real-bgm] seq version={} ppqn={} tempo_us={} events={}",
        seq.header.version,
        seq.header.ppqn,
        seq.header.tempo_us_per_qn,
        seq.events.len()
    );
    assert_eq!(seq.header.version, 1, "real SEQ must report version=1");
    assert!(seq.header.ppqn > 0, "real SEQ must have non-zero PPQN");
    assert!(
        seq.header.tempo_us_per_qn > 0,
        "real SEQ must have non-zero initial tempo"
    );
    assert!(
        !seq.events.is_empty(),
        "real SEQ must decode at least one event before EOT"
    );

    // Retail tracks carry an init-placeholder header tempo (commonly 240 BPM)
    // that the first body `0xFF 0x51` event immediately overrides to the real
    // musical tempo. PSX SEQ meta events have NO MIDI length byte; reading a
    // phantom length would drop the override and pin playback at the
    // placeholder rate (the constant-wrong-BPM bug). Confirm the override is
    // decoded and lands in a sane musical range.
    use legaia_seq::{EventBody, MetaMessage};
    let first_tempo = seq.events.iter().find_map(|e| match e.body {
        EventBody::Meta(MetaMessage::SetTempo { us_per_qn }) => Some(us_per_qn),
        _ => None,
    });
    if let Some(us) = first_tempo {
        let bpm = 60_000_000.0 / us as f64;
        eprintln!("[real-bgm] first body SetTempo = {us} us/qn ({bpm:.1} BPM)");
        assert!(
            (40.0..=300.0).contains(&bpm),
            "first body tempo override should be a musical BPM, got {bpm:.1}"
        );
    }

    let mut spu = Spu::new();
    let mut alloc = SpuAllocator::new(0x1000, 0x40_000);
    let bank = VabBank::upload(&mut spu, &mut alloc, &vab_report, &bytes[vab_off..]);
    assert!(
        !bank.programs.is_empty(),
        "VabBank::upload must surface at least one program"
    );

    let mut sequencer = Sequencer::new(seq, bank);
    // Drive four whole notes at the track's OWN tempo, not a fixed
    // wall-clock slice. This used to be a flat 200 ms justified as "real
    // SEQ tempos sit around 120 BPM, enough for a ProgramChange + NoteOn" -
    // but this entry's body tempo is 70 BPM, where a single quarter note is
    // 857 ms, so the window closed before the first note and the chain
    // rendered pure silence on every run. Deriving the span from
    // `first_tempo` is what makes the bar below reachable for any track.
    let us_per_qn = first_tempo.unwrap_or(500_000) as f64;
    let steps = ((us_per_qn * 16.0) / 5_000.0).ceil() as usize;
    // Sample the SPU *across* the drive rather than in one window at the
    // end: a fixed end-window can land in a rest between phrases and read
    // silent even on a healthy chain, which would make the assertion below
    // depend on where the music happens to be rather than on whether it
    // plays at all.
    let mut max_abs: i32 = 0;
    for _ in 0..steps {
        sequencer.tick_us(&mut spu, 5_000.0);
        for _ in 0..64 {
            let (l, r) = spu.tick();
            max_abs = max_abs.max((l as i32).abs()).max((r as i32).abs());
        }
    }
    eprintln!("[real-bgm] peak |sample| over {steps} steps = {max_abs}");
    // Acceptance bar: the REAL bank and the REAL sequence produce audible
    // PCM. The old bar was "renders 1024 samples without panic", which the
    // silent window above made vacuous - a wholly broken synthesis path
    // passed it. Non-silence on the real chain is the property the
    // synthetic known-amplitude test could only assert about itself.
    assert!(
        max_abs > 0,
        "real SEQ + VAB rendered silence after {steps} sequencer steps \
         ({:.1} BPM); the chain parsed but produced no PCM",
        60_000_000.0 / us_per_qn
    );
}
