//! Disc-gated: the naming prompt measures a typed name against the disc's own
//! advance table (`0x80073F1C`), as the append arm of `FUN_801F03F0` does -
//! a glyph is kept only while the name holds fewer than nine glyphs
//! (`0x801F05F8`) and measures under `0x39` px (`0x801F0654`). The table
//! rides in through the boot install both play hosts make
//! (`World::install_retail_progression_tables`), so `World::open_name_entry`
//! applies the cap. Skips without `LEGAIA_DISC_BIN`.

use legaia_engine_menus::name_entry::{
    CHAR_CELLS, NAME_MAX, NAME_WIDTH_CAP, NameEntryInput, name_width_px,
};
use legaia_engine_scene::vfs::Vfs;
use std::path::PathBuf;

fn type_glyph(world: &mut legaia_engine_core::world::World, g: char, n: usize) {
    let entry = world.party.name_entry.as_mut().expect("prompt open");
    entry.cursor = (0..CHAR_CELLS)
        .find(|&c| entry.glyph_at(c) == Some(g))
        .expect("glyph on the grid");
    for _ in 0..n {
        world.step_name_entry(NameEntryInput {
            confirm: true,
            ..Default::default()
        });
    }
}

#[test]
fn disc_name_entry_applies_the_width_cap() {
    let Some(path) = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    if !path.is_file() {
        eprintln!("[skip] LEGAIA_DISC_BIN is not a file");
        return;
    }
    let scus = legaia_engine_scene::vfs::DiscVfs::open(&path)
        .expect("open disc")
        .read("SCUS_942.54")
        .expect("SCUS_942.54 present");

    let mut world = legaia_engine_core::world::World::default();
    world.install_retail_progression_tables(&scus);

    // Wide glyphs: the width cap stops the name before nine.
    world.open_name_entry(0);
    let e = world.party.name_entry.as_mut().expect("prompt open");
    let widths = e.glyph_widths.expect("the boot install carries the table");
    // The retail table's own shape: `W` is wide, `i` narrow.
    assert!(widths[usize::from(b'W')] > widths[usize::from(b'i')]);
    e.name.clear();
    type_glyph(&mut world, 'W', NAME_MAX + 3);
    let name = world.party.name_entry.as_ref().unwrap().name.clone();
    let w = name_width_px(&widths, &name);
    assert!(name.len() < NAME_MAX, "wide name {name:?} stopped by width");
    assert!(w < NAME_WIDTH_CAP, "kept name measures {w} px");
    assert!(
        w + u32::from(widths[usize::from(b'W')]) + 1 >= NAME_WIDTH_CAP,
        "one more W would cross the cap"
    );

    // Narrow glyphs: the nine-glyph bound decides.
    if let Some(e) = world.party.name_entry.as_mut() {
        e.name.clear();
    }
    type_glyph(&mut world, 'i', NAME_MAX + 3);
    let name = world.party.name_entry.as_ref().unwrap().name.clone();
    assert_eq!(name.len(), NAME_MAX);
    assert!(name_width_px(&widths, &name) < NAME_WIDTH_CAP);
}
