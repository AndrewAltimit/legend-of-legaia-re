//! The battle value readout's **draw** half, pinned at the byte stream both
//! hosts upload.
//!
//! The layout was always shared (`engine-vm::battle_value_readout`) and the
//! two hosts still drew different pixels from it: the native window sampled
//! retail's 24x24 cells out of VRAM through a mesh of its own, and the browser
//! play page restyled the same digits in the dialog font. A shared *layout* is
//! not a shared *draw*, and no host-drift tier can see the difference - both
//! hosts reach one builder either way.
//!
//! What closes it is `battle_numerals`: the quads are `ScreenPrim`s now, and
//! each host pushes them through its own `screen_prim` pass. Since
//! `build_geometry` bakes the ordering-table walk into the index buffer before
//! either host sees the data, the *vertex bytes* are the whole contract - same
//! bytes, same VRAM page, same CLUT decode, same pixels. This test pins those
//! bytes' shape rather than a screenshot, which is what makes it runnable
//! without a GPU on either side.

use legaia_engine_ui::battle_numerals as bn;
use legaia_engine_ui::screen_prim::{
    BlendClass, FLAG_TEXTURED, PSX_DISPLAY_H, PSX_DISPLAY_W, SCREEN_VERTEX_OFF_CBA_TSB,
    SCREEN_VERTEX_OFF_FLAGS, SCREEN_VERTEX_STRIDE, build_geometry,
};
use legaia_engine_vm::battle_value_readout as vr;

/// One frame's worth of readout: a three-hit combo cluster plus a floating
/// numeral over an actor that projected to `(160, 120)`.
fn frame_prims() -> Vec<legaia_engine_ui::screen_prim::ScreenPrim> {
    let cluster = vr::combo_cluster(vr::ComboStyle::HitTotal, 3, 481, 0);
    let mut prims = bn::combo_cluster_prims(&cluster, bn::VALUE_READOUT_OT);
    let cells = vr::value_cells(184, 160, 120, vr::POP_FRAMES);
    prims.extend(bn::digit_run_prims(&cells, bn::VALUE_READOUT_OT));
    prims
}

fn le_u32(bytes: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]])
}

/// Every vertex of the readout carries the textured flag and the battle effect
/// atlas's `(CLUT, texpage)` pair. A quad that lost either samples nothing, or
/// samples the field page - which is the failure mode that reads as "the
/// numerals vanished" rather than as a broken draw.
#[test]
fn every_readout_vertex_names_the_glyph_page() {
    let prims = frame_prims();
    assert!(!prims.is_empty(), "the fixture emitted no primitives");
    let geom = build_geometry(&prims, PSX_DISPLAY_W as u32, PSX_DISPLAY_H as u32);
    let bytes = geom.vertex_bytes();
    let stride = SCREEN_VERTEX_STRIDE as usize;
    assert_eq!(
        bytes.len() % stride,
        0,
        "vertex stream is not stride-aligned"
    );
    assert_eq!(
        bytes.len() / stride,
        prims.len() * 4,
        "four corners per quad"
    );
    for v in 0..bytes.len() / stride {
        let base = v * stride;
        let clut = le_u32(&bytes, base + SCREEN_VERTEX_OFF_CBA_TSB as usize);
        let tpage = le_u32(&bytes, base + SCREEN_VERTEX_OFF_CBA_TSB as usize + 4);
        let flags = le_u32(&bytes, base + SCREEN_VERTEX_OFF_FLAGS as usize);
        assert_eq!(clut, u32::from(bn::GLYPH_CLUT), "vertex {v} CLUT");
        assert_eq!(tpage, u32::from(bn::GLYPH_TPAGE), "vertex {v} texpage");
        assert_eq!(flags & FLAG_TEXTURED, FLAG_TEXTURED, "vertex {v} flags");
    }
}

/// The whole readout is one **opaque** run. Retail's quads are GP0 `0x2C`, so
/// a semi-transparent class here would blend the art against the fight; and a
/// run split would mean two prims disagreed about their blend class, which is
/// the shape that lets half a number draw under a different equation.
#[test]
fn the_readout_is_a_single_opaque_run() {
    let geom = build_geometry(&frame_prims(), PSX_DISPLAY_W as u32, PSX_DISPLAY_H as u32);
    assert_eq!(geom.runs.len(), 1, "{:?}", geom.runs);
    assert_eq!(geom.runs[0].class, BlendClass::Opaque);
    assert_eq!(geom.runs[0].index_start, 0);
    assert_eq!(geom.runs[0].index_count as usize, geom.indices.len());
}

/// The run table's transport encoding is what the page reads out of the WASM
/// boundary (`play_screen_prim_runs`), so `0` has to keep meaning "opaque" -
/// an ABR-0 run encodes as `1`, not as `0`.
#[test]
fn the_run_word_the_page_reads_is_the_opaque_code() {
    let geom = build_geometry(&frame_prims(), PSX_DISPLAY_W as u32, PSX_DISPLAY_H as u32);
    let words = geom.run_words();
    assert_eq!(words.len(), 3, "one `[class, start, count]` triple");
    assert_eq!(words[0], 0, "opaque class code");
    assert_eq!(words[2] as usize, geom.indices.len());
}

/// The geometry is identical whichever host builds it: the same layout input
/// through the same builder produces byte-identical vertices and indices. This
/// is the parity claim, and it holds by construction only because neither host
/// hand-rolls the quads any more.
#[test]
fn two_builds_of_one_frame_are_byte_identical() {
    let a = build_geometry(&frame_prims(), PSX_DISPLAY_W as u32, PSX_DISPLAY_H as u32);
    let b = build_geometry(&frame_prims(), PSX_DISPLAY_W as u32, PSX_DISPLAY_H as u32);
    assert_eq!(a.vertex_bytes(), b.vertex_bytes());
    assert_eq!(a.indices, b.indices);
    assert_eq!(a.run_words(), b.run_words());
}

/// The floating numeral's quads land inside the 320x240 stage for an actor
/// projecting near the middle of it, and the cluster's sit on the pinned
/// right-hand seats. A readout that resolved into NDC off-screen would draw
/// nothing while every count above still passed.
#[test]
fn the_quads_resolve_inside_the_stage() {
    let geom = build_geometry(&frame_prims(), PSX_DISPLAY_W as u32, PSX_DISPLAY_H as u32);
    let bytes = geom.vertex_bytes();
    let stride = SCREEN_VERTEX_STRIDE as usize;
    let mut on_stage = 0usize;
    for v in 0..bytes.len() / stride {
        let base = v * stride;
        let x = f32::from_le_bytes([
            bytes[base],
            bytes[base + 1],
            bytes[base + 2],
            bytes[base + 3],
        ]);
        let y = f32::from_le_bytes([
            bytes[base + 4],
            bytes[base + 5],
            bytes[base + 6],
            bytes[base + 7],
        ]);
        if (-1.0..=1.0).contains(&x) && (-1.0..=1.0).contains(&y) {
            on_stage += 1;
        }
    }
    assert_eq!(
        on_stage,
        bytes.len() / stride,
        "some readout vertices resolved outside the display rect"
    );
}
