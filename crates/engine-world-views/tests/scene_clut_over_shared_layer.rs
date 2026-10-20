//! Disc-gated: a field scene's cutout CLUTs land transparent over the shared
//! `init_data` row they share.
//!
//! `init_data` carries a TIM whose 256-wide CLUT covers all of row 484, and the
//! scene build uploads it as a shared block before the scene's own TIMs. The
//! Sky Gardens tower (`tower`) puts its foliage palettes on the same row -
//! `(192, 484)` and `(208, 484)` among them - each with entry 0 `0x0000`, the
//! transparent index the vine and grass quads' backgrounds use. Retail's
//! `LoadImage` replaces every word of a CLUT block, so those words read
//! `0x0000`. The build's merge-zeros CLUT pass used to reach across into the
//! shared layer and keep `init_data`'s opaque word there (`0x8023` at both
//! cells), and every vine drew on a dark-brown card.
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN`.

use std::path::PathBuf;

use legaia_container_formats::tim_scan;
use legaia_engine_scene::scene::{ProtIndex, Scene};
use legaia_engine_scene::vfs::{DiscVfs, Vfs};
use legaia_engine_world_views::scene_staging::assemble_field_scene;

fn disc_index() -> Option<ProtIndex> {
    let path = PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    let vfs = DiscVfs::open(&path).ok()?;
    let prot = vfs.read("prot.dat").ok()?;
    let cdname = vfs
        .read("cdname.txt")
        .or_else(|_| vfs.read("data/cdname.txt"))
        .ok()
        .and_then(|b| String::from_utf8(b).ok());
    ProtIndex::from_bytes(prot, cdname.as_deref()).ok()
}

/// Every parseable TIM in a scene block's entries (raw and LZS sections).
fn block_tims(index: &ProtIndex, name: &str) -> Vec<legaia_tim::Tim> {
    let scene = Scene::load(index, name).expect("load scene block");
    let mut out = Vec::new();
    for e in &scene.entries {
        let scan = tim_scan::scan_entry(&e.bytes);
        for (source, hit) in &scan.hits {
            let src: &[u8] = match source {
                tim_scan::Source::Raw => &e.bytes,
                tim_scan::Source::Lzs(i) => scan.lzs_sections[*i].as_slice(),
            };
            if let Some(b) = src.get(hit.offset..hit.offset + hit.byte_len)
                && let Ok(t) = legaia_tim::parse(b)
            {
                out.push(t);
            }
        }
    }
    out
}

#[test]
fn tower_foliage_clut_entry_zero_is_transparent() {
    let Some(index) = disc_index() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or unreadable");
        return;
    };
    let a = assemble_field_scene(&index, "tower").expect("assemble tower");
    let vram = &a.res.vram;

    // The shared row: which row-484 cells `init_data` writes non-zero.
    let shared: std::collections::BTreeMap<usize, u16> = block_tims(&index, "init_data")
        .into_iter()
        .filter_map(|t| t.clut)
        .filter(|c| c.fb_y == 484)
        .flat_map(|c| {
            let run = usize::from(c.w) * usize::from(c.h);
            let x0 = usize::from(c.fb_x);
            c.entries
                .into_iter()
                .take(run)
                .enumerate()
                .map(move |(k, w)| (x0 + k, w))
        })
        .filter(|&(_, w)| w != 0)
        .collect();

    // The scene's cutout palettes on that row: entry 0 transparent on disc.
    let mut cutouts = Vec::new();
    for t in block_tims(&index, "tower") {
        let Some(c) = t.clut else { continue };
        if c.fb_y == 484 && c.entries.first() == Some(&0) {
            cutouts.push(usize::from(c.fb_x));
        }
    }
    cutouts.sort_unstable();
    cutouts.dedup();
    for x in [192usize, 208] {
        assert!(
            cutouts.contains(&x),
            "tower carries a cutout CLUT at ({x}, 484)"
        );
    }

    let mut over_shared = 0usize;
    for &x in &cutouts {
        assert!(
            vram.is_written(x, 484),
            "tower CLUT at ({x}, 484) was uploaded"
        );
        assert_eq!(
            vram.pixel(x, 484),
            0x0000,
            "tower CLUT at ({x}, 484): entry 0 must stay transparent"
        );
        over_shared += usize::from(shared.contains_key(&x));
    }
    // Non-vacuous: the shared block does put an opaque word under these cells.
    assert!(
        over_shared >= 2,
        "init_data covers the tower cutout cells with opaque words ({over_shared})"
    );
    eprintln!(
        "[ran] tower: {} cutout CLUTs on row 484 transparent, {over_shared} over init_data words",
        cutouts.len()
    );
}
