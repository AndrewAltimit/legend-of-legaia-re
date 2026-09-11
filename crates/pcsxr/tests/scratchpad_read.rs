//! Disc-gated: a PCSX-Redux `.sstate` carries the PSX **scratchpad**, and this
//! reader gets at it.
//!
//! The claim this replaces: "a `.sstate` carries main RAM only". It does not -
//! the memory submessage holds a 64 KiB `hardware` blob whose first kilobyte is
//! `0x1F800000`. The `teien_field_run` capture is the fixture because its
//! scratchpad content is independently pinned: `0x1F8003EC` holds the live
//! scene-map pointer the field ground pass dereferences, and `0x1F80035C`
//! holds the sixteen-entry floor LUT `FUN_801CF070` seeds as `-0x20 * n`.
//!
//! Skips when the SCUS binary or the save library is absent.

use std::path::{Path, PathBuf};

use legaia_pcsxr::{SCRATCHPAD_LEN, SaveState};

/// `teien_field_run` in `scripts/scenarios.toml`.
const TEIEN_FIELD_RUN: &str = "811098af2f18e96d7989c03cbc3521976eb913a92af34e244127248c6fbf6fe4";

/// The scene-map pointer the field pass dereferences.
const SCENE_MAP_PTR_VA: u32 = 0x1F80_03EC;

/// Base of the sixteen-entry floor LUT (`FUN_801CF070` writes `-0x20 * n`).
const FLOOR_LUT_VA: u32 = 0x1F80_035C;

fn ensure_scus() -> bool {
    if std::env::var_os("LEGAIA_SCUS").is_some() {
        return true;
    }
    for c in ["extracted", "../extracted", "../../extracted"] {
        let p = PathBuf::from(c).join("SCUS_942.54");
        if p.exists() {
            // SAFETY: single-threaded test setup before any SaveState load.
            unsafe { std::env::set_var("LEGAIA_SCUS", &p) };
            return true;
        }
    }
    false
}

fn library_save(fp: &str) -> Option<PathBuf> {
    for c in ["saves/library", "../saves/library", "../../saves/library"] {
        let p = Path::new(c).join("pcsx-redux").join(format!("{fp}.sstate"));
        if p.exists() {
            return Some(p);
        }
    }
    None
}

#[test]
fn a_pcsx_redux_state_carries_the_scratchpad_not_only_main_ram() {
    if !ensure_scus() {
        eprintln!("[skip] extracted/SCUS_942.54 absent (LEGAIA_SCUS unset)");
        return;
    }
    let Some(path) = library_save(TEIEN_FIELD_RUN) else {
        eprintln!("[skip] saves/library/pcsx-redux/{TEIEN_FIELD_RUN}.sstate absent");
        return;
    };
    let st = SaveState::from_path(&path).expect("load teien_field_run");
    assert_eq!(
        st.scene_name(),
        "teien",
        "the fixture is the teien field run"
    );

    let hw = st.hardware().expect("the state carries a hardware blob");
    assert_eq!(hw.len(), 0x1_0000, "the hardware region is 64 KiB");
    let sp = st.scratchpad().expect("scratchpad");
    assert_eq!(sp.len(), SCRATCHPAD_LEN);
    assert!(
        sp.iter().any(|&b| b != 0),
        "a live field frame's scratchpad is not blank"
    );

    // The scene-map pointer: a plausible KSEG0 main-RAM address, which is what
    // the ground pass dereferences (`*(0x1F8003EC) + 0x8000` is the object
    // grid). A blank or mis-located blob fails this.
    let map_ptr = st.scratchpad_u32_at(SCENE_MAP_PTR_VA).expect("map pointer");
    assert!(
        (0x8000_0000..0x8020_0000).contains(&map_ptr),
        "0x1F8003EC = {map_ptr:#x} is not a KSEG0 main-RAM pointer"
    );

    // The floor LUT: sixteen `i16` at -0x20 * n. Its shape is independent of
    // the pointer above, so the two together pin the blob's base exactly.
    for n in 0..16u32 {
        let off = FLOOR_LUT_VA + n * 2;
        let lo = st.scratchpad_u8_at(off).expect("lut byte");
        let hi = st.scratchpad_u8_at(off + 1).expect("lut byte");
        let v = i16::from_le_bytes([lo, hi]);
        assert_eq!(
            v,
            -0x20i32.wrapping_mul(n as i32) as i16,
            "floor LUT entry {n}"
        );
    }

    // Out-of-window reads are `None`, not a panic or a wrapped read.
    assert!(st.scratchpad_u32_at(0x1F80_1000).is_none());
    assert!(st.scratchpad_u32_at(0x8000_0000).is_none());
}
