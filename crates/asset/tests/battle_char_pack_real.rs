//! Disc-gated regression for [`legaia_asset::battle_char_pack`].
//!
//! Pins the on-disc layout of the two-entry `other5` battle pack: PROT 1204's
//! five battle-form character TMD chunks at the streaming offsets the doc page
//! lists, and PROT 1205's eight 256x256 4bpp TIM atlases at `0x8224` stride.
//! Entries are read at their **own** sector spans through
//! `legaia_prot::Archive` rather than out of pre-extracted `.BIN`s, so the
//! offsets asserted here are a claim about the entries and not about an
//! extractor's window. Skips when `LEGAIA_DISC_BIN` is unset so CI works
//! without redistributing Sony data.

use legaia_asset::battle_char_pack::{
    ATLAS_CLUT_ROWS, ATLAS_COUNT, ATLAS_PROT_ENTRY_INDEX, ATLAS_STRIDE_BYTES,
    BATTLE_TMD_CHUNK_TYPE, FIRST_ATLAS_OFFSET, PROT_ENTRY_INDEX, SLOT_COUNT, parse, slot_label,
};
use std::path::{Path, PathBuf};

/// Pinned on-disc TMD body byte sizes (5 slots) - matches the streaming
/// chunk sizes `0x82EC`, `0x8364`, `0x60CC`, `0x699C`, `0x823C`.
const EXPECTED_BODY_SIZES: [usize; SLOT_COUNT] = [33516, 33636, 24780, 27036, 33340];

/// Pinned on-disc `nobj` (TMD header `+0x08`) per slot.
const EXPECTED_NOBJ: [u32; SLOT_COUNT] = [15, 16, 15, 20, 15];

/// Pinned absolute file offsets of each TMD body inside PROT 1204.
const EXPECTED_BODY_OFFSETS: [usize; SLOT_COUNT] = [0x4, 0x82F4, 0x1065C, 0x1672C, 0x1D0CC];

fn extracted_root() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let prot = repo.join("extracted").join("PROT");
    prot.is_dir().then_some(prot)
}

/// One PROT entry's own sectors, straight out of `extracted/PROT.DAT`.
fn prot_entry_bytes(index: u32) -> Option<Vec<u8>> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let mut archive =
        legaia_prot::archive::Archive::open(&repo.join("extracted").join("PROT.DAT")).ok()?;
    let entry = archive.entries.iter().find(|e| e.index == index)?.clone();
    let mut out = Vec::new();
    archive.read_entry(&entry, &mut out).ok()?;
    Some(out)
}

fn locate_entry_bin(prot_dir: &Path, index: u32) -> Option<PathBuf> {
    for e in std::fs::read_dir(prot_dir).ok()?.flatten() {
        let name = e.file_name();
        let s = name.to_string_lossy();
        if s.starts_with(&format!("{index:04}_")) && s.ends_with(".BIN") {
            return Some(e.path());
        }
    }
    None
}

#[test]
fn real_pack_layout() {
    let (Some(mesh_bytes), Some(atlas_bytes)) = (
        prot_entry_bytes(PROT_ENTRY_INDEX),
        prot_entry_bytes(ATLAS_PROT_ENTRY_INDEX),
    ) else {
        eprintln!("LEGAIA_DISC_BIN or extracted/PROT.DAT not available; skipping");
        return;
    };
    let pack = parse(&mesh_bytes, &atlas_bytes).expect("parse the two-entry battle character pack");

    // The split is the point: each half must fit in its own entry, and the
    // mesh entry must not be readable as the atlas entry. The pre-correction
    // reading had all fifteen regions inside 1204.
    assert_eq!(
        mesh_bytes.len(),
        0x25800,
        "PROT {PROT_ENTRY_INDEX} is its own 75 sectors"
    );
    assert_eq!(
        atlas_bytes.len(),
        0x41800,
        "PROT {ATLAS_PROT_ENTRY_INDEX} is its own 131 sectors"
    );
    assert!(
        legaia_asset::battle_char_pack::parse_atlases(&mesh_bytes).is_err(),
        "the mesh entry carries no atlases"
    );

    // Slot pin: count + sizes + nobj + file offsets.
    assert_eq!(pack.slots.len(), SLOT_COUNT);
    for (i, slot) in pack.slots.iter().enumerate() {
        assert_eq!(slot.slot, i, "slot index round-trip");
        assert_eq!(
            slot.disc_nobj,
            EXPECTED_NOBJ[i],
            "disc nobj for slot {i} ({})",
            slot_label(i)
        );
        assert_eq!(
            slot.tmd_bytes.len(),
            EXPECTED_BODY_SIZES[i],
            "TMD body size for slot {i} ({})",
            slot_label(i)
        );
        assert_eq!(
            slot.file_offset,
            EXPECTED_BODY_OFFSETS[i],
            "file offset for slot {i} ({})",
            slot_label(i)
        );
        // Each TMD parses cleanly with the canonical Legaia TMD walker.
        let parsed =
            legaia_tmd::parse(&slot.tmd_bytes).expect("Legaia TMD parse for battle character");
        assert_eq!(parsed.objects.len(), EXPECTED_NOBJ[i] as usize);
    }

    // Atlas pin: 8 TIMs at stride 0x8224 starting at offset 4 of PROT 1205,
    // CLUT rows 490..495, 497, 496 in disc order (496 last - the row the
    // over-read window cut off, not a skipped row).
    assert_eq!(pack.atlases.len(), ATLAS_COUNT);
    assert_eq!(ATLAS_COUNT, 8);
    assert_eq!(ATLAS_CLUT_ROWS.last(), Some(&496));
    // The eight chunks plus the terminator fill the entry, with only zero
    // padding left - the invariant that says eight is the whole set.
    let consumed = FIRST_ATLAS_OFFSET + ATLAS_COUNT * ATLAS_STRIDE_BYTES;
    assert!(consumed <= atlas_bytes.len());
    assert!(
        atlas_bytes[consumed - 4..].iter().all(|&b| b == 0),
        "no ninth chunk after atlas {}",
        ATLAS_COUNT - 1
    );
    for (i, atlas) in pack.atlases.iter().enumerate() {
        assert_eq!(atlas.atlas_index, i);
        assert_eq!(
            atlas.file_offset,
            FIRST_ATLAS_OFFSET + i * ATLAS_STRIDE_BYTES,
            "atlas {i} file offset"
        );
        assert_eq!(atlas.clut_fb_y, ATLAS_CLUT_ROWS[i], "atlas {i} CLUT row");
        // TIM magic check (parse byte 0..4 inline; legaia_tim parses too but
        // its full image-block validation isn't needed here).
        assert_eq!(
            &atlas.tim_bytes[..4],
            [0x10, 0, 0, 0],
            "atlas {i} TIM magic"
        );
    }

    // Sanity: streaming chunks are type 0x09 (the dispatcher tag for
    // battle-form character TMDs).
    assert_eq!(BATTLE_TMD_CHUNK_TYPE, 0x09);
}

/// The battle pack (1204) and the field pack (0874 §0) are **distinct** mesh
/// sets, not two views of the same geometry. This is the disc-only half of the
/// "battle does not reuse the field pack" finding: the empirical (save-state)
/// half lives outside the committed tree (it reads `DAT_8007C018[0..=2]` out of
/// real-battle RAM and byte-matches the party vertex data to this pack and not
/// to 0874 - see the module-level provenance note). Here we assert the two
/// packs share no character geometry: the battle pack's Vahn vertex pool does
/// not appear anywhere in the field-pack entry.
#[test]
fn battle_pack_is_distinct_from_field_pack() {
    let Some(prot_dir) = extracted_root() else {
        eprintln!("LEGAIA_DISC_BIN or extracted/PROT not available; skipping");
        return;
    };
    let Some(battle_bytes) = prot_entry_bytes(PROT_ENTRY_INDEX) else {
        return;
    };
    // Locate the field-form character pack entry (PROT 0874).
    let field_idx = legaia_asset::character_pack::PROT_ENTRY_INDEX;
    let Some(field_path) = locate_entry_bin(&prot_dir, field_idx) else {
        eprintln!("PROT 0874 entry missing; skipping");
        return;
    };

    let battle = legaia_asset::battle_char_pack::parse_slots(&battle_bytes)
        .expect("parse the battle pack's meshes");
    let field_bytes = std::fs::read(&field_path).expect("read PROT 0874");

    // Battle Vahn (slot 0) is nobj 15; the field Vahn is nobj 12 - different
    // object counts to begin with.
    assert_eq!(battle[0].disc_nobj, 15);

    // Take a window of battle-Vahn's first object's vertex pool and assert it
    // does NOT occur in the field-pack entry bytes. (Vertex SVECTORs are
    // pose-independent geometry: if the two packs were the same mesh, this
    // window would be present in both.)
    let vahn = legaia_tmd::parse(&battle[0].tmd_bytes).expect("battle Vahn TMD");
    let obj0 = &vahn.objects[0];
    // Rebuild the raw 8-byte SVECTOR stream (x,y,z,pad LE) from the parsed
    // vertices - pack-form-independent geometry bytes.
    let mut vbytes = Vec::with_capacity(obj0.vertices.len() * 8);
    for v in &obj0.vertices {
        vbytes.extend_from_slice(&v.x.to_le_bytes());
        vbytes.extend_from_slice(&v.y.to_le_bytes());
        vbytes.extend_from_slice(&v.z.to_le_bytes());
        vbytes.extend_from_slice(&v._pad.to_le_bytes());
    }
    assert!(vbytes.len() >= 104, "battle Vahn obj0 has enough vertices");
    let needle = &vbytes[8..8 + 96];
    assert!(
        find_subslice(&field_bytes, needle).is_none(),
        "battle-Vahn vertex geometry must not appear in the field pack (the packs are distinct)"
    );
    // ...and it *does* appear in its own pack, as a control.
    assert!(
        find_subslice(&battle_bytes, needle).is_some(),
        "control: battle-Vahn vertex geometry is present in its own pack entry"
    );
}

fn find_subslice(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}
