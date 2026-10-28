//! Bring your own game: the generic PS1 crates run on inputs that contain no
//! Legend of Legaia bytes at all, every one of them built here in code.
//!
//! ```text
//! cargo run -p legaia-pcsx-redux --example bring_your_own_game -- [out_dir]
//! ```
//!
//! Each step builds a stock input, hands it to the crate a second PS1 project
//! would depend on, and checks what comes back:
//!
//! 1. a 4-bit TIM with its CLUT -> `legaia_tim::parse_strict`, `decode_rgba8`,
//!    `write_png`;
//! 2. a SEQ -> `legaia_seq::Seq::parse` (see the step's note on which SEQ
//!    shape this is, and the trap it does not cover);
//! 3. a Mode 2/2352 ISO9660 image with a subdirectory ->
//!    `legaia_iso::write::encode_mode2_form1_sector` to master it, then both
//!    the in-memory walker and the file-backed `RawDisc` reader;
//! 4. a PCSX-Redux state and a mednafen state of a made-up game ->
//!    `legaia_pcsx_redux::SaveState` (structural RAM location, no executable)
//!    and `legaia_mednafen::SaveState::main_ram_structured`;
//! 5. the watchpoint-equivalent tooling over those states ->
//!    `legaia_ram_image::diff::diff_ram` and `bisect::bisect_first_bad`, with
//!    `GenericGame` standing in for the per-game profile.
//!
//! The same `run` is executed by `tests/bring_your_own_game.rs`, so CI keeps
//! this example honest without a disc. Outputs (the PNG and the `.bin`) go to
//! `out_dir`, default a fresh directory under the system temp dir.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};

/// What one run produced, for the caller to print or assert on.
#[derive(Debug)]
pub struct Report {
    pub png: PathBuf,
    pub disc: PathBuf,
    pub seq_events: usize,
    pub iso_files: Vec<String>,
    pub ram_diff_regions: usize,
}

pub fn run(out_dir: &Path) -> Result<Report> {
    std::fs::create_dir_all(out_dir).with_context(|| format!("creating {}", out_dir.display()))?;
    let png = tim_step(out_dir)?;
    let seq_events = seq_step()?;
    let (disc, iso_files) = iso_step(out_dir)?;
    let ram_diff_regions = save_state_step()?;
    Ok(Report {
        png,
        disc,
        seq_events,
        iso_files,
        ram_diff_regions,
    })
}

// --- 1. TIM ----------------------------------------------------------------

/// A stock 4bpp TIM: header, CLUT block (one 16-colour palette at VRAM
/// `(0, 480)`), image block (an 8x2 texel image, 2 VRAM halfwords wide).
fn build_tim() -> Vec<u8> {
    let mut t = Vec::new();
    t.extend_from_slice(&0x10u32.to_le_bytes()); // magic
    t.extend_from_slice(&0x08u32.to_le_bytes()); // flags: 4bpp, CLUT present
    // CLUT block: bs_len, x, y, w, h, then 16 BGR555 entries.
    let clut: Vec<u16> = (0..16u16)
        .map(|i| (i << 10) | (31 - i * 2).max(1))
        .collect();
    t.extend_from_slice(&(12u32 + 32).to_le_bytes());
    for v in [0u16, 480, 16, 1] {
        t.extend_from_slice(&v.to_le_bytes());
    }
    for c in &clut {
        t.extend_from_slice(&c.to_le_bytes());
    }
    // Image block: 8 texels per row at 4bpp = 2 halfwords; 2 rows.
    t.extend_from_slice(&(12u32 + 8).to_le_bytes());
    for v in [640u16, 0, 2, 2] {
        t.extend_from_slice(&v.to_le_bytes());
    }
    // Texel nibbles, low nibble first: row 0 = 0..7, row 1 = 8..15.
    t.extend_from_slice(&[0x10, 0x32, 0x54, 0x76, 0x98, 0xBA, 0xDC, 0xFE]);
    t
}

fn tim_step(out_dir: &Path) -> Result<PathBuf> {
    let bytes = build_tim();
    let tim = legaia_tim::parse_strict(&bytes).context("parse a stock 4bpp TIM")?;
    ensure!(tim.pixel_width() == 8 && tim.pixel_height() == 2);
    let rgba = legaia_tim::decode_rgba8(&tim, 0)?;
    ensure!(rgba.len() == 8 * 2 * 4);
    // Texel 15 (last pixel) is CLUT entry 15: blue 15, red 1.
    let last = &rgba[rgba.len() - 4..];
    ensure!(last == legaia_tim::bgr555_to_rgba8((15 << 10) | 1));
    let png = out_dir.join("stock_4bpp.png");
    legaia_tim::write_png(&png, 8, 2, &rgba)?;
    Ok(png)
}

// --- 2. SEQ ----------------------------------------------------------------

/// A SEQ in the 13-byte header shape (`u16` version) with the event stream
/// libsnd's SsAPI reader consumes: a program change, a note on/off pair under
/// running status, a tempo meta **without** a length byte, end of track.
///
/// Note what this does and does not show. It is the shape `legaia-seq` parses
/// and the shape the libsnd reader linked into Legaia's executable reads; it
/// is synthetic, and no other title's SEQ file is tested in this repository.
/// The crate does not read an SMF-style meta that carries a length byte
/// (`FF 51 03 tt tt tt`): the last check below demonstrates the misread, so a
/// port can tell which shape its files use before trusting a parse.
fn seq_step() -> Result<usize> {
    let mut s = Vec::new();
    s.extend_from_slice(b"pQES");
    s.extend_from_slice(&1u16.to_be_bytes()); // version (u16 shape)
    s.extend_from_slice(&480u16.to_be_bytes()); // ppqn
    s.extend_from_slice(&[0x07, 0xA1, 0x20]); // 500000 us/qn = 120 BPM
    s.extend_from_slice(&[4, 2]); // 4/4
    let body: &[u8] = &[
        0x00, 0xC0, 0x05, // program change ch0 -> 5
        0x00, 0x90, 0x3C, 0x64, // note on C4
        0x81, 0x70, 0x3C, 0x00, // 240 ticks later, running status, vel 0
        0x00, 0xFF, 0x51, 0x0F, 0x42, 0x40, // tempo 1000000 us/qn, no length byte
        0x00, 0xFF, 0x2F, // end of track
    ];
    s.extend_from_slice(body);

    let seq = legaia_seq::Seq::parse(&s).context("parse the synthetic SEQ")?;
    ensure!(seq.header.ppqn == 480 && seq.header.tempo_us_per_qn == 500_000);
    ensure!(seq.is_complete(), "stream ends on its own FF 2F");
    ensure!(seq.events.len() == 5);
    ensure!(seq.total_ticks() == 240);

    // The trap: an SMF-shaped tempo meta. The `03` length byte is read as the
    // first tempo byte, and the real third tempo byte becomes a stray event.
    let mut smf = s[..13].to_vec();
    smf.extend_from_slice(&[
        0x00, 0xFF, 0x51, 0x03, 0x0F, 0x42, 0x40, 0x00, 0xFF, 0x2F, 0x00,
    ]);
    let misread = legaia_seq::Seq::parse(&smf);
    let tempo_ok = misread.as_ref().is_ok_and(|m| {
        m.events.iter().any(|e| {
            e.body
                == legaia_seq::EventBody::Meta(legaia_seq::MetaMessage::SetTempo {
                    us_per_qn: 1_000_000,
                })
        })
    });
    ensure!(
        !tempo_ok,
        "an SMF-style length byte is not supported, and must not read as the right tempo"
    );
    Ok(seq.events.len())
}

// --- 3. ISO9660 on Mode 2/2352 ---------------------------------------------

const RAW: usize = 2352;
const USER: usize = 2048;

/// Minutes / seconds / frames of an LBA, in BCD, with the 150-sector lead-in.
fn msf(lba: u32) -> [u8; 3] {
    let a = lba + 150;
    let bcd = |v: u32| (((v / 10) << 4) | (v % 10)) as u8;
    [bcd(a / 75 / 60), bcd(a / 75 % 60), bcd(a % 75)]
}

/// One mastered Mode 2 Form 1 sector carrying `data` (at most 2048 bytes).
fn sector(lba: u32, data: &[u8], end_of_file: bool) -> Result<Vec<u8>> {
    let mut s = vec![0u8; RAW];
    s[0] = 0x00;
    s[1..11].fill(0xFF);
    s[11] = 0x00;
    s[12..15].copy_from_slice(&msf(lba));
    s[15] = 2; // mode 2
    let submode = if end_of_file { 0x89 } else { 0x08 }; // data (+ EOR/EOF)
    for o in [16, 20] {
        s[o..o + 4].copy_from_slice(&[0, 0, submode, 0]);
    }
    s[24..24 + data.len()].copy_from_slice(data);
    legaia_iso::write::encode_mode2_form1_sector(&mut s)?;
    Ok(s)
}

/// An ISO9660 directory record.
fn dir_record(name: &[u8], lba: u32, size: u32, is_dir: bool) -> Vec<u8> {
    let pad = usize::from(name.len().is_multiple_of(2));
    let len = 33 + name.len() + pad;
    let mut r = vec![0u8; len];
    r[0] = len as u8;
    r[2..6].copy_from_slice(&lba.to_le_bytes());
    r[6..10].copy_from_slice(&lba.to_be_bytes());
    r[10..14].copy_from_slice(&size.to_le_bytes());
    r[14..18].copy_from_slice(&size.to_be_bytes());
    r[25] = if is_dir { 0x02 } else { 0x00 };
    r[28..30].copy_from_slice(&1u16.to_le_bytes());
    r[30..32].copy_from_slice(&1u16.to_be_bytes());
    r[32] = name.len() as u8;
    r[33..33 + name.len()].copy_from_slice(name);
    r
}

/// One path-table record (`little` picks the L-type table's byte order).
fn path_record(name: &[u8], lba: u32, parent: u16, little: bool) -> Vec<u8> {
    let mut r = vec![name.len() as u8, 0];
    r.extend(if little {
        lba.to_le_bytes()
    } else {
        lba.to_be_bytes()
    });
    r.extend(if little {
        parent.to_le_bytes()
    } else {
        parent.to_be_bytes()
    });
    r.extend_from_slice(name);
    if name.len() % 2 == 1 {
        r.push(0);
    }
    r
}

/// A 25-sector image: system area, PVD (16), terminator (17), root (18),
/// `DATA` directory (19), `MYGAME.EXE` (20..=21), `DATA/LEVEL1.BIN` (22),
/// the L and M path tables (23, 24). Complete enough that a stock ISO9660
/// reader (libisofs / `xorriso`) lists it from the 2048-byte user data.
fn build_disc() -> Result<Vec<u8>> {
    const ROOT: u32 = 18;
    const SUB: u32 = 19;
    const EXE: u32 = 20;
    const LEVEL: u32 = 22;
    const PATH_L: u32 = 23;
    const PATH_M: u32 = 24;
    const SECTORS: u32 = 25;
    let exe: Vec<u8> = (0..(USER + 100)).map(|i| (i % 251) as u8).collect();
    let level = b"LEVEL ONE".to_vec();

    let mut root = Vec::new();
    root.extend(dir_record(&[0], ROOT, USER as u32, true));
    root.extend(dir_record(&[1], ROOT, USER as u32, true));
    root.extend(dir_record(b"DATA", SUB, USER as u32, true));
    root.extend(dir_record(b"MYGAME.EXE;1", EXE, exe.len() as u32, false));
    let mut sub = Vec::new();
    sub.extend(dir_record(&[0], SUB, USER as u32, true));
    sub.extend(dir_record(&[1], ROOT, USER as u32, true));
    sub.extend(dir_record(
        b"LEVEL1.BIN;1",
        LEVEL,
        level.len() as u32,
        false,
    ));

    let mut pvd = vec![0u8; USER];
    pvd[0] = 1;
    pvd[1..6].copy_from_slice(b"CD001");
    pvd[6] = 1;
    pvd[8..16].copy_from_slice(b"PLAYSTAT");
    pvd[40..72].fill(b' ');
    pvd[40..46].copy_from_slice(b"MYGAME");
    let both32 = |pvd: &mut [u8], at: usize, v: u32| {
        pvd[at..at + 4].copy_from_slice(&v.to_le_bytes());
        pvd[at + 4..at + 8].copy_from_slice(&v.to_be_bytes());
    };
    let both16 = |pvd: &mut [u8], at: usize, v: u16| {
        pvd[at..at + 2].copy_from_slice(&v.to_le_bytes());
        pvd[at + 2..at + 4].copy_from_slice(&v.to_be_bytes());
    };
    let mut path_l = path_record(&[0], ROOT, 1, true);
    path_l.extend(path_record(b"DATA", SUB, 1, true));
    let mut path_m = path_record(&[0], ROOT, 1, false);
    path_m.extend(path_record(b"DATA", SUB, 1, false));
    both32(&mut pvd, 80, SECTORS); // volume space size
    both16(&mut pvd, 120, 1); // volume set size
    both16(&mut pvd, 124, 1); // volume sequence number
    both16(&mut pvd, 128, USER as u16); // logical block size
    both32(&mut pvd, 132, path_l.len() as u32); // path table size
    pvd[140..144].copy_from_slice(&PATH_L.to_le_bytes());
    pvd[148..152].copy_from_slice(&PATH_M.to_be_bytes());
    pvd[156..190].copy_from_slice(&dir_record(&[0], ROOT, USER as u32, true));
    for at in [813, 830, 847, 864] {
        // Creation / modification / expiration / effective dates: "unset".
        pvd[at..at + 16].fill(b'0');
    }
    pvd[881] = 1; // file structure version
    let mut term = vec![0u8; USER];
    term[0] = 0xFF;
    term[1..6].copy_from_slice(b"CD001");
    term[6] = 1;

    let mut disc = Vec::new();
    for lba in 0..16 {
        disc.extend(sector(lba, &[], false)?);
    }
    disc.extend(sector(16, &pvd, false)?);
    disc.extend(sector(17, &term, true)?);
    disc.extend(sector(ROOT, &root, true)?);
    disc.extend(sector(SUB, &sub, true)?);
    disc.extend(sector(EXE, &exe[..USER], false)?);
    disc.extend(sector(EXE + 1, &exe[USER..], true)?);
    disc.extend(sector(LEVEL, &level, true)?);
    disc.extend(sector(PATH_L, &path_l, true)?);
    disc.extend(sector(PATH_M, &path_m, true)?);
    ensure!(disc.len() == SECTORS as usize * RAW);
    Ok(disc)
}

fn iso_step(out_dir: &Path) -> Result<(PathBuf, Vec<String>)> {
    let disc = build_disc()?;
    ensure!(legaia_iso::image::is_mode2_2352_disc(&disc));
    for lba in 0..disc.len() / RAW {
        ensure!(legaia_iso::write::mode2_form1_sector_is_valid(
            &disc[lba * RAW..(lba + 1) * RAW]
        ));
    }
    let files: Vec<String> = legaia_iso::image::walk_iso_files(&disc)
        .into_iter()
        .map(|f| f.path)
        .collect();
    ensure!(
        files == ["DATA/LEVEL1.BIN", "MYGAME.EXE"],
        "walked {files:?}"
    );
    // Names resolve the way the console resolves them: any case, no `;1`.
    let level = legaia_iso::iso9660::find_path_in_image(&disc, "data/level1.bin")
        .context("find DATA/LEVEL1.BIN")?;
    ensure!(level == (22, 9));
    let exe = legaia_iso::iso9660::read_file_in_image(&disc, "mygame.exe").context("read exe")?;
    ensure!(exe.len() == USER + 100 && exe[USER + 99] == ((USER + 99) % 251) as u8);

    // The file-backed reader a CLI uses, on the same bytes written out.
    let path = out_dir.join("mygame.bin");
    std::fs::write(&path, &disc)?;
    let mut raw = legaia_iso::raw::RawDisc::open(&path)?;
    let vol = legaia_iso::iso9660::read_volume(&mut raw)?;
    ensure!(vol.volume_id == "MYGAME");
    let mut walked: Vec<String> = legaia_iso::iso9660::walk_files(&mut raw, &vol.root)?
        .into_iter()
        .map(|(p, _)| p)
        .collect();
    walked.sort();
    ensure!(walked.len() == 2, "RawDisc walk saw {walked:?}");
    Ok((path, files))
}

// --- 4 + 5. Save states, diff, bisect ---------------------------------------

/// A minimal mednafen `MDFNSVST` payload holding only `MAIN.MainRAM.data8`.
fn mednafen_payload(ram: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    let key = b"MainRAM.data8";
    body.push(key.len() as u8);
    body.extend_from_slice(key);
    body.extend_from_slice(&(ram.len() as u32).to_le_bytes());
    body.extend_from_slice(ram);
    let mut out = Vec::new();
    out.extend_from_slice(b"MDFNSVST");
    out.extend_from_slice(&[0u8; 0x10]);
    let mut name = [0u8; 32];
    name[..4].copy_from_slice(b"MAIN");
    out.extend_from_slice(&name);
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    out
}

/// A made-up game's RAM: a frame counter at `0x800A0000` and a "boss HP"
/// word at `0x800A0100` that drops to zero between the second and third
/// capture.
fn game_ram(frame: u32, boss_hp: u32) -> Vec<u8> {
    let mut ram = vec![0u8; legaia_ram_image::PSX_RAM_SIZE];
    ram[0x1_0000..0x1_0010].copy_from_slice(b"OTHER GAME v1.00");
    ram[0xA_0000..0xA_0004].copy_from_slice(&frame.to_le_bytes());
    ram[0xA_0100..0xA_0104].copy_from_slice(&boss_hp.to_le_bytes());
    ram
}

fn save_state_step() -> Result<usize> {
    use legaia_ram_image::{GameProfile, GenericGame, bisect, diff};

    let captures = [game_ram(100, 900), game_ram(160, 350), game_ram(220, 0)];

    // PCSX-Redux: gzipped and bare states, located without any executable.
    let mut from_pcsx = Vec::new();
    for (i, ram) in captures.iter().enumerate() {
        let st = legaia_pcsx_redux::fixture::SyntheticState {
            ram,
            hardware: &[],
            vram: &[],
            gp1: [0; 256],
        };
        let bytes = if i % 2 == 0 {
            st.to_gzip()
        } else {
            st.to_payload()
        };
        let s = legaia_pcsx_redux::SaveState::from_sstate_bytes_structural(&bytes)?;
        ensure!(s.ram_source() == legaia_pcsx_redux::RamSource::Structural);
        // The generic profile claims no identity rather than reading
        // Legaia's globals out of this game's RAM.
        ensure!(s.identity_with(&GenericGame).is_none());
        from_pcsx.push(s.main_ram().to_vec());
    }

    // Mednafen: the same RAM through the section walker.
    for (ram, pcsx) in captures.iter().zip(&from_pcsx) {
        let st = legaia_mednafen::SaveState::from_decompressed(mednafen_payload(ram))?;
        let via_mdfn = st
            .main_ram_structured()
            .context("MAIN.MainRAM.data8 in the synthetic mednafen state")?;
        ensure!(via_mdfn == &pcsx[..], "both emulators' readers agree");
        ensure!(st.main_ram_with_anchors(&[], &GenericGame)? == via_mdfn);
    }

    // Watchpoint-equivalent diff between the first two captures.
    let d = diff::diff_ram(
        &from_pcsx[0],
        &from_pcsx[1],
        "frame100",
        "frame160",
        &diff::DiffOptions::default(),
    );
    ensure!(
        d.regions.len() == 2,
        "frame counter and boss HP: {:?}",
        d.regions
    );
    ensure!(d.regions[0].start_addr == 0x800A_0000 && d.regions[1].start_addr == 0x800A_0100);

    // When did boss HP hit zero?
    let snaps: Vec<(&str, &[u8])> = ["a", "b", "c"]
        .into_iter()
        .zip(from_pcsx.iter().map(Vec::as_slice))
        .collect();
    let when = bisect::bisect_first_bad(&snaps, 0x800A_0100, |hp| hp == 0);
    ensure!(
        when == bisect::BisectOutcome::BracketedAt {
            before_idx: 1,
            after_idx: 2
        }
    );
    ensure!(GenericGame.name() == "generic");
    Ok(d.regions.len())
}

#[allow(dead_code)] // `main` is unused when the test harness includes this file.
fn main() -> Result<()> {
    let out = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("bring-your-own-game-{}", std::process::id()))
        });
    let r = run(&out)?;
    println!("TIM  -> {}", r.png.display());
    println!("SEQ  -> {} events, clean end of track", r.seq_events);
    println!("ISO  -> {} ({})", r.disc.display(), r.iso_files.join(", "));
    println!(
        "RAM  -> PCSX-Redux and mednafen states agree; diff found {} regions; bisect bracketed the write",
        r.ram_diff_regions
    );
    Ok(())
}
