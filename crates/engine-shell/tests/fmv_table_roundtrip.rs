//! End-to-end round-trip for the in-RAM compact STR FMV table.
//!
//! Phase 1 (disc-only, gated on `LEGAIA_DISC_BIN`):
//!   * Walk `\MOV\` on the disc via ISO9660.
//!   * For each `MV*.STR` file, read the first ~2 MiB, push sectors
//!     through `StrFrameAssembler`, decode the first frame via
//!     `MdecDecoder`. This is exactly the pipeline `legaia-engine
//!     play-str` uses, exercised headlessly.
//!   * Confirm decoder reports a non-empty frame and the frame
//!     dimensions are sane (each FMV in retail is 320x240 BS-v2).
//!
//! Phase 2 (gated on `LEGAIA_MEDNAFEN_DIR`):
//!   * Locate a save state with the FMV overlay resident.
//!   * Parse the compact 6-entry table at `0x801CAE40`.
//!   * For each entry, compute the LBA from BCD MSF and verify it
//!     matches the on-disc LBA from the ISO9660 walk.
//!
//! Both phases skip silently when their gate is unset.

use std::path::PathBuf;

use legaia_iso::iso9660::{DirectoryRecord, list_directory, read_volume};
use legaia_iso::raw::RawDisc;
use legaia_mdec::str_sector::StrFrameAssembler;
use legaia_mdec::{MdecDecoder, VideoFrame};

fn disc_bin() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from)
}

fn mednafen_dir() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_MEDNAFEN_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
}

/// Resolve `\MOV\` on the disc and return every directory record
/// inside it that ends in `.STR`.
fn list_mov_str_files(disc: &mut RawDisc) -> anyhow::Result<Vec<DirectoryRecord>> {
    let vol = read_volume(disc)?;
    let root = list_directory(disc, &vol.root)?;
    let mov = root
        .iter()
        .find(|e| e.is_dir && e.name.eq_ignore_ascii_case("MOV"))
        .ok_or_else(|| anyhow::anyhow!("no \\MOV\\ directory on disc"))?;
    let entries = list_directory(disc, mov)?;
    Ok(entries
        .into_iter()
        .filter(|e| !e.is_dir && e.name.to_uppercase().contains(".STR"))
        .collect())
}

/// Decode the first MDEC frame from a raw STR sector stream.
fn decode_first_frame(sectors: &[u8]) -> Option<VideoFrame> {
    let mut asm = StrFrameAssembler::new();
    for sector in sectors.chunks_exact(2048) {
        match asm.push_sector(sector) {
            Ok(Some((hdr, bs))) => {
                let dec = MdecDecoder::new(hdr.width as u32, hdr.height as u32);
                if let Ok(rgba) = dec.decode_frame(&bs) {
                    return Some(VideoFrame {
                        rgba,
                        width: hdr.width as u32,
                        height: hdr.height as u32,
                        frame_number: hdr.frame_number,
                    });
                }
            }
            Ok(None) => {}
            Err(_) => break,
        }
    }
    None
}

#[test]
fn mv_str_files_decode_first_frame_via_engine_pipeline() {
    let Some(disc_path) = disc_bin() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut disc = RawDisc::open(&disc_path).expect("open disc");
    let entries = list_mov_str_files(&mut disc).expect("list /MOV/");
    assert!(
        !entries.is_empty(),
        "/MOV/ on disc has no .STR files - corpus malformed"
    );
    eprintln!(
        "disc {} has {} MV*.STR file(s)",
        disc_path.display(),
        entries.len()
    );

    for entry in &entries {
        // Read up to the smaller of 2 MiB or the file size, in 2048-byte
        // sectors. The first frame fits well inside that.
        let max_bytes = entry.size as usize;
        let cap = max_bytes.min(2 * 1024 * 1024);
        let sector_count = cap.div_ceil(2048);
        let mut buf = Vec::with_capacity(sector_count * 2048);
        disc.read_user_data(entry.lba, sector_count as u32, &mut buf)
            .unwrap_or_else(|e| panic!("read {}: {e}", entry.name));
        // Trim trailing partial sector if the file is smaller.
        buf.truncate(sector_count * 2048);

        let frame = decode_first_frame(&buf).unwrap_or_else(|| {
            panic!(
                "no first frame decoded from {} (LBA {}, size {})",
                entry.name, entry.lba, entry.size
            )
        });
        eprintln!(
            "  {:<14} LBA={:>6} size={:>8}  first frame {}x{} (frame#{})",
            entry.name, entry.lba, entry.size, frame.width, frame.height, frame.frame_number
        );
        // Sanity: retail Legaia FMVs are 320x240 BS-v2; pin that range
        // (allow MDEC blocks-of-16 alignment overshoots).
        assert!(
            frame.width >= 64 && frame.width <= 640,
            "{} frame width {} out of range",
            entry.name,
            frame.width
        );
        assert!(
            frame.height >= 64 && frame.height <= 480,
            "{} frame height {} out of range",
            entry.name,
            frame.height
        );
        assert_eq!(
            frame.rgba.len(),
            (frame.width * frame.height * 4) as usize,
            "{} frame pixel count != width*height*4",
            entry.name
        );
    }
}

#[test]
fn fmv_table_lbas_match_iso_walk_when_overlay_resident() {
    let Some(disc_path) = disc_bin() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let Some(mc_dir) = mednafen_dir() else {
        eprintln!("[skip] LEGAIA_MEDNAFEN_DIR unset");
        return;
    };
    use legaia_engine_core::capture_observations::str_fmv_overlay;
    use legaia_mednafen::SaveState;

    // Probe every .mc{0..9} for the FMV-overlay residency signature.
    // Skip-pass if no save in the user's directory has it.
    let mut resident_save: Option<PathBuf> = None;
    let canon_name = |slot: u8| {
        // Mednafen's default-pattern name used by the project's saves.
        format!("Legend of Legaia (USA).788de08f9c7e652c51d8d08ee374d055.mc{slot}")
    };
    for slot in 0..=9u8 {
        let p = mc_dir.join(canon_name(slot));
        if !p.exists() {
            continue;
        }
        if let Ok(s) = SaveState::from_path(&p)
            && let Ok(r) = s.main_ram()
            && str_fmv_overlay::is_resident(r)
        {
            resident_save = Some(p);
            break;
        }
    }
    let Some(save_path) = resident_save else {
        eprintln!(
            "[skip] no FMV-overlay-resident save found in {}",
            mc_dir.display()
        );
        return;
    };

    let s = SaveState::from_path(&save_path).expect("parse save state");
    let r = s.main_ram().expect("main ram slice");
    let off = (str_fmv_overlay::COMPACT_TABLE_ADDR - 0x80000000) as usize;
    let entries = legaia_asset::str_fmv_table::parse_entries(
        &r[off..off
            + str_fmv_overlay::MV_BASENAMES.len() * legaia_asset::str_fmv_table::ENTRY_STRIDE],
        str_fmv_overlay::MV_BASENAMES.len(),
    )
    .expect("compact table parses");

    let mut disc = RawDisc::open(&disc_path).expect("open disc");
    let iso_files = list_mov_str_files(&mut disc).expect("list /MOV/");
    eprintln!(
        "table {} entries vs disc {} MV*.STR file(s)",
        entries.len(),
        iso_files.len()
    );

    for (i, entry) in entries.iter().enumerate() {
        let basename = entry.name.split(';').next().unwrap_or(&entry.name);
        let iso = iso_files
            .iter()
            .find(|e| e.name.eq_ignore_ascii_case(basename))
            .unwrap_or_else(|| panic!("table entry {i} '{basename}' has no matching disc file"));
        let table_lba = entry.lba();
        eprintln!(
            "  [{i}] {:<14}  table LBA={:>6}  iso LBA={:>6}  size={:>8}",
            basename, table_lba, iso.lba, iso.size
        );
        assert_eq!(
            table_lba, iso.lba,
            "table LBA for {basename} disagrees with disc LBA"
        );
        // Size match isn't strict (table size includes sector padding;
        // ISO size is the user-data length). Bound it loosely.
        let diff = (entry.size as i64 - iso.size as i64).abs();
        assert!(
            diff < (4 * 2336),
            "table size {} for {basename} diverges from iso size {}",
            entry.size,
            iso.size
        );
    }
}
