//! Audio-extraction helpers for the in-browser asset viewer.
//!
//! Three families:
//!   1. VAB sound banks - scan every PROT entry for the `pBAV` magic, parse
//!      the bank, expose per-sample decode (single-shot VAG → 22 050 Hz mono PCM).
//!   2. BGM pairs - entries that contain both `pBAV` (VAB) and `pQES` (SEQ).
//!      Consumed by the WebAudio BGM player (`LegaiaAudio::start_bgm`).
//!   3. XA streams - ISO9660 walks the disc for `*.STR` / `*.XA` files, demuxes
//!      Form 2 audio sectors into per-channel PCM at 37.8 kHz / 18.9 kHz.
//!
//! Pure data path: every function in this module takes byte slices and
//! returns owned `Vec<i16>` / metadata structs. The wasm-bindgen boundary
//! lives in [`crate::LegaiaAudio`].

use crate::disc::{EntryMeta, FileEntry, parse_prot_toc, read_raw_sector, walk_iso_files};
use legaia_vab::{VabReport, find_vabs, parse as parse_vab};
use legaia_xa::demux::{
    AUDIO_BYTES_PER_SECTOR, SUBHEADER_OFFSET, USER_DATA_OFFSET, parse_subheader,
};
use legaia_xa::{Channels, DecodeOptions, decode};

/// Default playback rate for decoded VAG samples. The Sony VAG header carries
/// no per-sample rate; the engine and the WAV exporter both use 22 050 Hz
/// across the corpus (see `crates/engine-audio/src/vab_bind.rs::VAB_SAMPLE_RATE`).
pub const VAB_SAMPLE_RATE: u32 = 22_050;

/// One VAB bank's high-level metadata. Returned as part of [`enumerate_vabs`].
#[derive(Debug, Clone)]
pub struct VabSummary {
    pub prot_index: u32,
    pub vab_offset: u32,
    pub version: u32,
    pub program_count: u32,
    pub sample_count: u32,
    pub has_seq: bool,
}

/// Enumerate every VAB sound bank in the loaded disc.
///
/// Walks every PROT entry, scans for the `pBAV` magic, and validates each
/// candidate by parsing the full bank header. Reports the SEQ-coexistence
/// flag so the UI can hint at BGM pairs without a second walk.
pub fn enumerate_vabs(disc: &[u8], entries: &[EntryMeta]) -> Vec<VabSummary> {
    let mut out = Vec::new();
    for e in entries {
        let Some(buf) = entry_buf(disc, e) else {
            continue;
        };
        let has_seq = buf.windows(4).any(|w| w == b"pQES");
        for vab_off in find_vabs(buf) {
            let Ok(report) = parse_vab(buf, vab_off) else {
                continue;
            };
            out.push(VabSummary {
                prot_index: e.index,
                vab_offset: vab_off as u32,
                version: report.header.version,
                program_count: report.programs.len() as u32,
                sample_count: report.vag_samples.len() as u32,
                has_seq,
            });
        }
    }
    out
}

/// One BGM pair (VAB + SEQ co-located in a single PROT entry).
#[derive(Debug, Clone)]
pub struct BgmPair {
    pub prot_index: u32,
    pub vab_offset: u32,
    pub seq_offset: u32,
    pub program_count: u32,
    pub sample_count: u32,
    pub ppqn: u32,
    pub bpm: f32,
}

/// Enumerate every PROT entry that contains both a parseable VAB and a SEQ
/// past it. The `ppqn`/`bpm` fields come from the SEQ header for the UI.
pub fn enumerate_bgm_pairs(disc: &[u8], entries: &[EntryMeta]) -> Vec<BgmPair> {
    let mut out = Vec::new();
    for e in entries {
        let Some(buf) = entry_buf(disc, e) else {
            continue;
        };
        let Some(vab_off) = buf.windows(4).position(|w| w == b"pBAV") else {
            continue;
        };
        // Require the SEQ to come *after* the VAB; the scene_vab_stream
        // wrapper places the SEQ inside a trailing streaming chunk past the
        // VAB body.
        let Some(seq_rel) = buf[vab_off..].windows(4).position(|w| w == b"pQES") else {
            continue;
        };
        let seq_off = vab_off + seq_rel;
        let Ok(report) = parse_vab(buf, vab_off) else {
            continue;
        };
        let Ok(hdr) = legaia_seq::parse_header(&buf[seq_off..]) else {
            continue;
        };
        out.push(BgmPair {
            prot_index: e.index,
            vab_offset: vab_off as u32,
            seq_offset: seq_off as u32,
            program_count: report.programs.len() as u32,
            sample_count: report.vag_samples.len() as u32,
            ppqn: hdr.ppqn as u32,
            bpm: hdr.bpm(),
        });
    }
    out
}

/// Parse a single VAB at a known PROT entry + intra-entry offset. Useful when
/// the JS side already has the (`prot_index`, `vab_offset`) tuple from
/// [`enumerate_vabs`].
pub fn parse_vab_at(
    disc: &[u8],
    entries: &[EntryMeta],
    prot_index: u32,
    vab_offset: u32,
) -> Option<(VabReport, Vec<u8>)> {
    let e = entries.iter().find(|x| x.index == prot_index)?;
    let buf = entry_buf(disc, e)?;
    let report = parse_vab(buf, vab_offset as usize).ok()?;
    // Hand back the bank body sliced from the VAB header onward, so the
    // sample-decode path can resolve `VagSampleSpan::byte_offset` against it.
    let bank_buf = buf[vab_offset as usize..].to_vec();
    Some((report, bank_buf))
}

/// Decode one VAG sample to mono i16 PCM at [`VAB_SAMPLE_RATE`].
pub fn decode_vag_sample(
    disc: &[u8],
    entries: &[EntryMeta],
    prot_index: u32,
    vab_offset: u32,
    sample_idx: u32,
) -> Option<Vec<i16>> {
    let (report, _) = parse_vab_at(disc, entries, prot_index, vab_offset)?;
    let span = report.vag_samples.get(sample_idx as usize)?;
    // span.byte_offset is relative to the original input buffer, with
    // vab_offset added in. Resolve back to the entry buffer.
    let e = entries.iter().find(|x| x.index == prot_index)?;
    let buf = entry_buf(disc, e)?;
    let body = buf.get(span.byte_offset..span.byte_offset + span.size)?;
    if body.is_empty() {
        return None;
    }
    legaia_vab::decode_vag(body).ok()
}

/// One XA audio file on the disc (`*.STR`, `*.XA`). MV*.STR carry video too;
/// the demuxer cleanly separates the audio sectors so the video is ignored.
#[derive(Debug, Clone)]
pub struct XaFile {
    pub path: String,
    pub lba: u32,
    pub size: u32,
}

fn looks_like_xa_filename(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper.ends_with(".STR") || upper.ends_with(".XA")
}

/// Enumerate every disc-resident file whose name looks like an XA / FMV
/// stream (`*.STR` or `*.XA`). Returns an empty vec when `disc` isn't a
/// Mode2/2352 image.
pub fn enumerate_xa_files(disc: &[u8]) -> Vec<XaFile> {
    walk_iso_files(disc)
        .into_iter()
        .filter(|f| looks_like_xa_filename(&f.path))
        .map(|FileEntry { path, lba, size }| XaFile { path, lba, size })
        .collect()
}

/// One demuxed XA channel after decode.
#[derive(Debug, Clone)]
pub struct DecodedXa {
    pub file_no: u8,
    pub ch_no: u8,
    pub sample_rate: u32,
    pub stereo: bool,
    /// Interleaved PCM (L,R,L,R,... for stereo; one stream for mono).
    pub pcm: Vec<i16>,
}

/// In-memory XA demux + decode. Reads `sector_count` raw 2352-byte sectors
/// starting at `start_lba`, filters to Form 2 audio sectors, splits per
/// `(file_no, ch_no)` and decodes each channel to PCM. Returns one
/// [`DecodedXa`] per channel.
///
/// Mirrors the path-based `legaia_xa::demux::demux_disc_range` but reads
/// straight out of the in-memory `disc` slice instead of going through
/// `RawDisc` (which needs `std::fs`).
pub fn decode_xa_in_memory(disc: &[u8], start_lba: u32, byte_size: u32) -> Vec<DecodedXa> {
    // ISO9660 reports file size as if Form 1 (2048-byte user data). Recover
    // the on-disc sector count by rounding up.
    let sector_count = byte_size.div_ceil(2048);

    // Per-channel buffer of concatenated 128-byte sound groups.
    struct Stream {
        sample_rate: u32,
        stereo: bool,
        audio: Vec<u8>,
    }
    let mut by_key: std::collections::BTreeMap<(u8, u8), Stream> = Default::default();

    for s in 0..sector_count {
        let Some(raw) = read_raw_sector(disc, start_lba + s) else {
            break;
        };
        let mut sub_bytes = [0u8; 8];
        sub_bytes.copy_from_slice(&raw[SUBHEADER_OFFSET..SUBHEADER_OFFSET + 8]);
        let (sub, ok) = parse_subheader(&sub_bytes);
        if !ok || !sub.is_audio() || !sub.is_form2() {
            continue;
        }
        let key = (sub.file_no, sub.ch_no);
        let stream = by_key.entry(key).or_insert_with(|| Stream {
            sample_rate: sub.sample_rate(),
            stereo: sub.is_stereo(),
            audio: Vec::new(),
        });
        let audio_off = USER_DATA_OFFSET;
        let audio_end = audio_off + AUDIO_BYTES_PER_SECTOR;
        stream.audio.extend_from_slice(&raw[audio_off..audio_end]);
    }

    let mut out = Vec::with_capacity(by_key.len());
    for ((file_no, ch_no), stream) in by_key {
        let opts = DecodeOptions {
            channels: if stream.stereo {
                Channels::Stereo
            } else {
                Channels::Mono
            },
            sample_rate: stream.sample_rate,
        };
        let Ok((pcm, _)) = decode(&stream.audio, opts) else {
            continue;
        };
        out.push(DecodedXa {
            file_no,
            ch_no,
            sample_rate: stream.sample_rate,
            stereo: stream.stereo,
            pcm,
        });
    }
    out
}

/// One assembled-but-not-yet-decoded STR video frame: its dimensions and the
/// concatenated Iki bitstream. Decoding to RGBA is deferred to
/// [`decode_str_frame_rgba`] so the front-end pays MDEC cost one frame at a
/// time (a whole movie's RGBA would be hundreds of MB).
#[derive(Debug, Clone)]
pub struct StrVideoFrame {
    pub width: u32,
    pub height: u32,
    pub bitstream: Vec<u8>,
}

/// Demuxed STR video: every frame's bitstream plus the recovered playback rate.
#[derive(Debug, Clone, Default)]
pub struct StrVideo {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub frames: Vec<StrVideoFrame>,
}

/// Walk an `MV*.STR` file's raw sectors and assemble every MDEC video frame's
/// bitstream (skipping the interleaved Form-2 audio sectors). Mirrors the
/// native `cutscene_av::decode_str_av_from_disc` video path but reads from the
/// in-memory disc slice and defers the per-frame MDEC decode. The frame rate is
/// recovered from the mean sectors-per-frame at the 2x CD rate, exactly like
/// the native path.
pub fn demux_str_video(disc: &[u8], start_lba: u32, byte_size: u32) -> StrVideo {
    use legaia_mdec::str_sector::{CD_SECTORS_PER_SEC_2X, StrFrameAssembler};

    const VIDEO_USER_DATA: usize = 2048;
    let sector_count = byte_size.div_ceil(2048);
    let mut asm = StrFrameAssembler::new();
    let mut frames: Vec<StrVideoFrame> = Vec::new();

    for s in 0..sector_count {
        let Some(raw) = read_raw_sector(disc, start_lba + s) else {
            break;
        };
        let mut sub_bytes = [0u8; 8];
        sub_bytes.copy_from_slice(&raw[SUBHEADER_OFFSET..SUBHEADER_OFFSET + 8]);
        let (sub, ok) = parse_subheader(&sub_bytes);
        if ok && sub.is_audio() && sub.is_form2() {
            continue; // audio sector - handled by decode_xa_in_memory
        }
        let video_user = &raw[USER_DATA_OFFSET..USER_DATA_OFFSET + VIDEO_USER_DATA];
        // The assembler magic-checks each sector and skips non-video data.
        if let Ok(Some((hdr, bs))) = asm.push_sector(video_user) {
            frames.push(StrVideoFrame {
                width: hdr.width as u32,
                height: hdr.height as u32,
                bitstream: bs,
            });
        }
    }

    let (width, height) = frames
        .first()
        .map(|f| (f.width, f.height))
        .unwrap_or((0, 0));
    let fps = if frames.is_empty() {
        0.0
    } else {
        CD_SECTORS_PER_SEC_2X / (sector_count as f64 / frames.len() as f64)
    };
    StrVideo {
        width,
        height,
        fps,
        frames,
    }
}

/// Decode one assembled STR frame bitstream to a row-major RGBA8 buffer.
/// Returns an empty vec on a decode error (a single bad frame shouldn't abort
/// playback).
pub fn decode_str_frame_rgba(frame: &StrVideoFrame) -> Vec<u8> {
    legaia_mdec::MdecDecoder::new(frame.width, frame.height)
        .decode_frame(&frame.bitstream)
        .unwrap_or_default()
}

fn entry_buf<'a>(disc: &'a [u8], e: &EntryMeta) -> Option<&'a [u8]> {
    let off = e.byte_offset as usize;
    let end = (e.byte_offset + e.size_bytes) as usize;
    disc.get(off..end)
}

/// Parse the loaded buffer's PROT TOC and return the entry list. Mirrors the
/// detection in [`crate::LegaiaViewer::load_disc`] so callers don't need
/// access to the private viewer state.
pub fn entries_from_disc(disc: &[u8], prot_bytes: &[u8]) -> Vec<EntryMeta> {
    // PROT TOC parses against the PROT.DAT bytes; for the in-memory walker
    // we always extract PROT.DAT first (see [`crate::disc::extract_prot_dat`]).
    let _ = disc;
    parse_prot_toc(prot_bytes).unwrap_or_default()
}
