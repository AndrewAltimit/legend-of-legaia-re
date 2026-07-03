//! Headless SEQ playback mode + the music-track label lookup for
//! `music_01` bank files.

use anyhow::{Context, Result};
use legaia_engine_audio::{AudioOut, Sequencer};
use std::path::Path;
use std::time::Instant;

/// If a path's file stem opens with a PROT extraction index that falls in
/// the `music_01` sound-test bank (`0990_music_01.BIN`, an extracted
/// `1006_...` slice, ...), return the curated track label for that slot.
fn music_bank_label_for_path(p: &Path) -> Option<String> {
    let stem = p.file_stem()?.to_str()?;
    let digits: String = stem.chars().take_while(|c| c.is_ascii_digit()).collect();
    legaia_engine_core::music_labels::label_for_prot_entry(digits.parse().ok()?)
}

/// Headless SEQ playback. Opens AudioOut, builds a `Sequencer` over the
/// parsed SEQ + uploaded VAB, attaches it, and prints progress until
/// end-of-track (or forever if `--looped`). Ctrl-C to exit.
pub(crate) fn run_seq_playback(
    seq_path: &Path,
    vab_path: &Path,
    vab_offset: usize,
    looped: bool,
    master_vol: u8,
) -> Result<()> {
    use legaia_engine_audio::spu::ram::SPU_RAM_BYTES;
    use legaia_engine_audio::{Spu, SpuAllocator, VabBank};
    use legaia_seq::Seq;

    let seq_bytes =
        std::fs::read(seq_path).with_context(|| format!("read {}", seq_path.display()))?;
    let seq = Seq::parse(&seq_bytes).context("parse SEQ")?;
    log::info!(
        "seq: {} events, {} ticks, init {:.1} BPM @ {} PPQN",
        seq.events.len(),
        seq.total_ticks(),
        seq.header.bpm(),
        seq.header.ppqn
    );
    // When the file comes from a music_01 bank slot (extraction entries
    // 990..=1071 - the debug sound-test bank), surface the curated track
    // label: the bank slot order is the sound-test order the
    // `legaia_gamedata` music table is keyed on.
    if let Some(label) =
        music_bank_label_for_path(seq_path).or_else(|| music_bank_label_for_path(vab_path))
    {
        log::info!("track: {label}");
    }

    let vab_bytes =
        std::fs::read(vab_path).with_context(|| format!("read {}", vab_path.display()))?;
    let report = legaia_vab::parse(&vab_bytes, vab_offset).context("parse VAB")?;
    log::info!(
        "vab: {} programs, {} samples (offset 0x{:X})",
        report.header.ps,
        report.vag_samples.len(),
        vab_offset
    );

    let audio = AudioOut::new().context("open audio output")?;

    // Build the bank inside the AudioOut's SPU (via with_spu) so the
    // sequencer's SPU references match the playback SPU. Reserve the
    // first 4 KB for voice 0 / scratchpad, allocate from there onward.
    let bank = audio.with_spu(|spu: &mut Spu| {
        let mut alloc = SpuAllocator::new(0x1000, SPU_RAM_BYTES as u32 - 0x1000);
        VabBank::upload(spu, &mut alloc, &report, &vab_bytes)
    });

    let mut sequencer = Sequencer::new(seq, bank);
    sequencer.set_master_vol(master_vol);
    if looped {
        sequencer.set_loop_to(0);
    }
    audio.attach_sequencer(sequencer);

    log::info!(
        "playing SEQ {} (vab {} @ 0x{:X}, looped={}, master_vol={})",
        seq_path.display(),
        vab_path.display(),
        vab_offset,
        looped,
        master_vol
    );

    // Poll the sequencer's progress at ~10 Hz, print one status line per
    // change, and exit when finished (or never, if --looped).
    let start = Instant::now();
    let mut last_tick: u64 = 0;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let Some(p) = audio.sequencer_progress() else {
            break;
        };
        if p.tick != last_tick {
            log::info!(
                "  +{:.1}s tick={} bpm={:.1} active={}",
                start.elapsed().as_secs_f32(),
                p.tick,
                p.bpm,
                p.active_notes
            );
            last_tick = p.tick;
        }
        if p.finished {
            log::info!(
                "end of track ({:.1}s elapsed)",
                start.elapsed().as_secs_f32()
            );
            break;
        }
    }
    audio.detach_sequencer();
    Ok(())
}
