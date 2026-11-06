//! Disc-gated: move-VM op `0x21` writes a row of the deformation table
//! `0x8007BE60`, and the part that wrote it shatters under that row.
//!
//! The op's retail arm (`0x8002377C..0x800237D4` in `FUN_80023070`) stores the
//! four halfwords `op[2..=5]` at `row + 0..+6` and the sign-extended `op[6]` at
//! `row + 8`, with `row = 0x8007BE60 + id * 0xC`, and points the part's `+0x6D`
//! at it. The model draw `FUN_8001ADA4` hands a part whose `+0x7A` is non-zero
//! to the shatter renderer with `+0x7A | row[+8] << 16`
//! (`0x8001B538..0x8001B564`), the renderer adding `row[+0..+4]` to every
//! vertex.
//!
//! Six stager records on the disc issue the op. `rikuroa` / `rikuroa2` records
//! `56` / `57` are the pair the parent records `2` / `3` spawn through op
//! `0x25`, and the streamed `rikuroa` MAN spawns those parents with
//! `34 30 01` / `34 30 02` (the field VM's op-`0x34` sub-3 arm,
//! `spawn_ambient_record_at(arg + 1)`). Each child waits `0x3F` vsyncs, writes
//! its row - record `56` row `2`, burst offset `(0, 0xA8, 0)`; record `57` row
//! `1`, `(0, 0x140, 0)`; both flag word `4` - then ramps `+0x7A` by `0x30` a
//! tick. Flag word `4` lands at `0x40000` in the draw word, the shrink flag,
//! so each triangle collapses as the word climbs instead of flying apart at
//! full size about the part's origin.
//!
//! The test enters the scene through `SceneHost`, spawns the parent the way
//! the op-`0x34` arm does, ticks the real ambient pass, and requires the row
//! in the shared table, on the part's draw and in the mesh both play hosts
//! upload (the scene-pack mesh through `SummonPartDraw::apply_render_state`).
//! The row-less draw is the same part with the row the port used to pass.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / extracted assets are missing.

use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_render_kernels::shatter::ShatterRow;
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    None
}

/// The shrink flag the rows carry, as `FUN_80029888` reads it off the draw
/// word's high half (`row[+8] = 4`, shifted by 16).
const ROW_FLAGS_SHRINK: u32 = 4;

/// Mean triangle edge length of a mesh: the shrink flag scales every
/// triangle about its own centre, so this falls as the shatter word climbs.
fn mean_edge(mesh: &legaia_tmd::mesh::VramMesh) -> f32 {
    let p = &mesh.positions;
    let (mut sum, mut n) = (0.0f32, 0usize);
    for t in mesh.indices.as_chunks::<3>().0 {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let (a, b) = (p[a as usize], p[b as usize]);
            sum += ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
            n += 1;
        }
    }
    sum / n.max(1) as f32
}

#[test]
fn rikuroa_op21_rows_reach_the_part_shatter() {
    let Some(ex) = extracted_dir() else { return };
    let mut host = SceneHost::open_extracted(&ex).expect("open");
    host.enter_field_scene("rikuroa", 0).expect("enter rikuroa");
    let w = &host.world;
    assert_eq!(
        w.shatter_row(1),
        ShatterRow::default(),
        "row 1 unwritten on entry"
    );
    assert_eq!(
        w.shatter_row(2),
        ShatterRow::default(),
        "row 2 unwritten on entry"
    );

    // The scene-pack TMD list a field stager's `model_sel` indexes (retail
    // `DAT_8007C018[5..]`), built the way the native window's
    // `install_field_pack_tables` builds it.
    // The scene-pack TMD list a field stager's `model_sel` indexes (retail
    // `DAT_8007C018[5..]`): the environment pack both hosts resolve.
    let res = host.resources.as_ref().expect("resources");
    let pack: Vec<(legaia_tmd::Tmd, Vec<u8>)> =
        legaia_engine_scene::field_env::env_pack_tmd_indices(
            host.scene.as_ref().expect("scene"),
            res,
        )
        .into_iter()
        .map(|i| (res.tmds[i].tmd.clone(), res.tmds[i].raw.clone()))
        .collect();
    let mesh_of = |d: &legaia_engine_effects::summon::SummonPartDraw| {
        let (tmd, raw) = pack.get(d.model_index).expect("the part's pack slot");
        let mut m = legaia_tmd::mesh::tmd_to_vram_mesh(tmd, raw);
        d.apply_render_state(&mut m);
        m
    };

    // `34 30 01`: record 2, which spawns records 56 and 57 through op 0x25.
    assert!(
        host.world.spawn_ambient_record(2, [0, 0, 0]),
        "spawn record 2"
    );

    let mut checked = 0usize;
    for _ in 0..200u32 {
        host.tick().expect("tick");
        let _ = host.step_headless_frame_tail();
        let draws = host.world.ambient_mesh_part_draws();
        let shattering: Vec<_> = draws.iter().filter(|d| d.shatter > 0x400).collect();
        if shattering.len() < 2 {
            continue;
        }
        for d in shattering {
            assert_eq!(d.shatter_row.flags, ROW_FLAGS_SHRINK, "the row's flag word");
            assert!(
                d.shatter_row.off == [0, 0xA8, 0] || d.shatter_row.off == [0, 0x140, 0],
                "a record-56 / record-57 burst offset, got {:?}",
                d.shatter_row.off
            );
            let live = mesh_of(d);
            let mut rowless = *d;
            rowless.shatter_row = ShatterRow::default();
            let old = mesh_of(&rowless);
            assert_eq!(live.indices.len(), old.indices.len());
            let (e_live, e_old) = (mean_edge(&live), mean_edge(&old));
            eprintln!(
                "[shatter] pack slot {} word {:#x}: mean edge {e_live:.1} under the row, \
                 {e_old:.1} without",
                d.model_index, d.shatter
            );
            assert!(
                e_live < e_old * 0.9,
                "the shrink flag must collapse the triangles ({e_live} vs {e_old})"
            );
            checked += 1;
        }
        break;
    }
    assert!(
        checked >= 2,
        "records 56 and 57 never reached their shatter ramp"
    );

    let w = &host.world;
    assert_eq!(
        w.shatter_row(2),
        ShatterRow {
            off: [0, 0xA8, 0],
            h3: 0,
            flags: ROW_FLAGS_SHRINK
        },
        "record 56's op 0x21: `0021 0002 0000 00A8 0000 0000 0004`"
    );
    assert_eq!(
        w.shatter_row(1),
        ShatterRow {
            off: [0, 0x140, 0],
            h3: 0,
            flags: ROW_FLAGS_SHRINK
        },
        "record 57's op 0x21: `0021 0001 0000 0140 0000 0000 0004`"
    );
}
