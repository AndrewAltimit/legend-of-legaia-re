//! Disc-gated regression test for the bytes **past** the runtime SFX
//! descriptor bank in extraction entry 888, and their twin in entry 1062.
//!
//! Skips silently when `extracted/PROT/` or `LEGAIA_DISC_BIN` is missing.
//!
//! What this catches:
//! - The tail being read as a second `bse.dat` record family again. It is a
//!   run of PsyQ `VagAtr` tone rows - the 32-byte per-tone records of a
//!   [VAB](https://andrewaltimit.github.io/legend-of-legaia-re/formats/vab.html) -
//!   left in the sector by the file that previously occupied the slot, and it
//!   is byte-identical to a neighbouring entry at the *same* file offsets.
//! - The `VagAtr` grid phase drifting. A VAB laid out from file offset 4 puts
//!   its tone table at `0x824` (`4 + 0x20 header + 0x800 program table`), so
//!   every row starts at a file offset `= 4 (mod 0x20)` and the builder's
//!   `+0x18` fill lands at `0x1C (mod 0x20)`. That holds in every entry on the
//!   disc that carries the fill, which is what identifies the two orphan tails.
//! - Entry 888's walk stop being mistaken for an authored terminator: the four
//!   zero bytes `bse_bank::detect` stops on are the residue row's
//!   `vibW/vibT/porW/porT` field, which is zero on every retail tone.
//!
//! Format: `docs/formats/bse-dat.md` § "The bytes past the table are not this
//! bank's".

use std::path::{Path, PathBuf};

/// `bse.dat`.
const BSE_ENTRY: u32 = 888;
/// The entry whose tone table 888's tail is a copy of (and its twin).
const BSE_DONORS: [u32; 2] = [886, 1063];
/// The SEQ-only `music_01` entry carrying the same shape.
const SEQ_ENTRY: u32 = 1062;
/// The entry whose tone table 1062's tail is a copy of.
const SEQ_DONOR: u32 = 1056;

/// `VagAtr` stride.
const TONE_BYTES: usize = 0x20;
/// Byte offset of the builder's `reserved[0..3]` fill inside a `VagAtr`.
const RESERVED_OFF: usize = 0x18;
/// That fill, as four little-endian `u16`s.
const RESERVED_FILL: [u8; 8] = [0xC0, 0, 0xC1, 0, 0xC2, 0, 0xC3, 0];
/// First tone row of a VAB laid out from file offset 4.
const FIRST_TONE_OFF: usize = 0x824;

fn extracted_root() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    ["extracted", "../../extracted"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.join("PROT").is_dir())
}

fn entry_path(root: &Path, index: u32) -> Option<PathBuf> {
    let dir = root.join("PROT");
    let prefix = format!("{index:04}_");
    let name = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .find(|n| n.starts_with(&prefix))?;
    Some(dir.join(name))
}

fn entry_bytes(root: &Path, index: u32) -> Option<Vec<u8>> {
    std::fs::read(entry_path(root, index)?).ok()
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

#[test]
fn bse_tail_is_a_foreign_tone_table_or_skips() {
    let Some(root) = extracted_root() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let Some(bse) = entry_bytes(&root, BSE_ENTRY) else {
        eprintln!("[skip] extraction entry 0888 missing");
        return;
    };

    let bank = legaia_asset::bse_bank::detect(&bse).expect("entry 888 is a runtime SFX bank");
    let table_end =
        legaia_asset::bse_bank::HEADER_BYTES + bank.records * legaia_asset::bse_bank::RECORD_BYTES;
    assert_eq!(table_end, 0x94C, "the walk stops where the bank's rows end");
    assert_eq!(
        &bse[table_end..table_end + 4],
        &[0u8; 4],
        "the stop word is four zero bytes"
    );
    // ... and those four bytes are a VagAtr row's vibW/vibT/porW/porT, not a
    // record the bank's author wrote: they sit at +0x08 of the row that
    // started at 0x944.
    assert_eq!(
        (table_end - FIRST_TONE_OFF) % TONE_BYTES,
        8,
        "the stop lands 8 bytes into a tone row"
    );

    for donor in BSE_DONORS {
        let Some(d) = entry_bytes(&root, donor) else {
            eprintln!("[skip] donor entry {donor:04} missing");
            return;
        };
        assert!(d.len() >= bse.len(), "donor covers the tail offsets");
        assert_eq!(
            &bse[table_end..],
            &d[table_end..bse.len()],
            "888's tail is entry {donor:04}'s tone table at the same offsets"
        );
    }

    let Some(seq) = entry_bytes(&root, SEQ_ENTRY) else {
        eprintln!("[skip] extraction entry 1062 missing");
        return;
    };
    let Some(seq_donor) = entry_bytes(&root, SEQ_DONOR) else {
        eprintln!("[skip] extraction entry 1056 missing");
        return;
    };
    // 1062 is `[u24 length][u8 type = 2]` then a `pQES` SEQ; its tail starts
    // where the declared SEQ ends.
    let hdr = u32::from_le_bytes(seq[..4].try_into().expect("hdr"));
    assert_eq!(hdr >> 24, 2, "chunk type 2 = SEQ");
    assert_eq!(&seq[4..8], b"pQES", "SEQ magic");
    assert!(
        find(&seq, b"pBAV").is_none(),
        "1062 ships no VAB of its own"
    );
    let tail = 4 + (hdr & 0x00FF_FFFF) as usize;
    let differing = seq[tail..]
        .iter()
        .zip(&seq_donor[tail..seq.len()])
        .filter(|(a, b)| a != b)
        .count();
    assert!(
        differing <= 2,
        "1062's tail is entry {SEQ_DONOR:04}'s tone table at the same offsets \
         (only the bytes the SEQ's own padding clipped may differ), got {differing}"
    );
}

#[test]
fn tone_fill_phase_holds_across_the_corpus_or_skips() {
    let Some(root) = extracted_root() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let Ok(dir) = std::fs::read_dir(root.join("PROT")) else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    let mut carriers = 0usize;
    let mut orphans = Vec::new();
    for e in dir.filter_map(|e| e.ok()) {
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()) != Some("BIN") {
            continue;
        }
        let Ok(buf) = std::fs::read(&path) else {
            continue;
        };
        let Some(at) = find(&buf, &RESERVED_FILL) else {
            continue;
        };
        carriers += 1;
        assert_eq!(
            at % TONE_BYTES,
            (FIRST_TONE_OFF + RESERVED_OFF) % TONE_BYTES,
            "{}: the tone fill sits at VagAtr+0x18 on the 0x824 grid",
            path.display()
        );
        if find(&buf, b"pBAV").is_none() {
            orphans.push(path.file_name().unwrap().to_string_lossy().into_owned());
        }
    }
    assert!(
        carriers > 200,
        "the fill is a per-tone builder sentinel, not rare"
    );
    orphans.sort();
    let ids: Vec<u32> = orphans.iter().filter_map(|n| n[..4].parse().ok()).collect();
    assert_eq!(
        ids,
        vec![BSE_ENTRY, SEQ_ENTRY],
        "exactly two entries carry tone rows without a VAB of their own"
    );
}
