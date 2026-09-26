//! Field entry runs retail `MAIN_INIT`'s grid-mark refresh (`FUN_80017BEC`,
//! `jal` at `0x801D6BF8`) over the scene's `.MAP` before the object cells are
//! decoded. The disc's cells are already stamped almost everywhere; `retona`'s
//! tile `(0x1D, 0x18)` is one of the few that is not, and the
//! `retona_field_card_boot` PCSX-Redux capture holds the stamped value there
//! (`0x306B`, the object index `0x6B` plus the `0x1000` / `0x2000` mirrors of
//! its descriptor's flag bits 0 and 1). This pins the engine's live cell to the
//! refreshed value and the disc's raw cell to the unstamped one, so the test
//! fails if the entry stops running the refresh.
//!
//! Skips (and passes) when `extracted/` or `LEGAIA_DISC_BIN` is missing.

use std::path::PathBuf;

use legaia_engine_core::scene::SceneHost;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn retona_enters_with_the_refreshed_cell_the_capture_holds() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene("retona", 0).expect("enter retona");

    // Tile (x = 0x1D, z = 0x18): cell word at `.MAP` +0x8000 + x*2 + z*0x100.
    let (tx, tz) = (0x1Dusize, 0x18usize);
    let scene = host.scene.as_ref().expect("scene");
    let idx = scene.field_map_index(&host.index).expect(".MAP");
    let raw = host.index.entry_bytes(idx).expect("map bytes");
    let o = 0x8000 + tx * 2 + tz * 0x100;
    let disc = u16::from_le_bytes([raw[o], raw[o + 1]]);
    assert_eq!(disc & 0x3000, 0, "the disc cell is unstamped");

    let live = host.world.terrain.object_cells[tx + tz * 0x80];
    assert_eq!(live & 0x1FF, 0x6B, "object index untouched");
    assert_eq!(
        live & 0x3000,
        0x3000,
        "entry refreshed the cell to the captured value"
    );
    eprintln!("[ok] retona (0x1D, 0x18): disc {disc:#06x} -> live {live:#06x}");
}
