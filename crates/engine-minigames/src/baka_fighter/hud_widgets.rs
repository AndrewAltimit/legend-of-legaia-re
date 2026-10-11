//! HUD widget quads and the centre effect spawn.

/// A resolved HUD widget quad - the renderer-agnostic form of the POLY_GT4
/// packet the retail emitter builds (12-word GP0 `0x3C`/`0x3E` shaded
/// textured quad).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HudWidgetQuad {
    /// GP0 polygon code (`(semi << 1) | 0x3C`).
    pub poly_code: u8,
    /// Quad corners, inclusive: `(x0, y0)` top-left, `(x1, y1)` bottom-right.
    pub x0: i16,
    pub y0: i16,
    pub x1: i16,
    pub y1: i16,
    /// Per-corner texture coordinates in vertex order (TL, TR, BL, BR).
    pub uv: [(u8, u8); 4],
    /// Brightness-scaled gouraud colours: verts 0/1 take `rgb_top`, verts
    /// 2/3 take `rgb_bottom`.
    pub rgb_top: [u8; 3],
    pub rgb_bottom: [u8; 3],
    /// CLUT id (packet uv0 hi-half).
    pub clut: u16,
    /// Texpage attribute after the ABR fold (`texpage + abr * 0x20`).
    pub tpage_attr: u16,
}

/// The MIPS `mult`/`sra` scale idiom the emitter applies to every colour
/// channel and half-extent: signed multiply, round toward zero at the given
/// shift (`bgez` skip + `addiu (1 << shift) - 1`).
fn mips_scale(value: i32, factor: i32, shift: u32) -> i32 {
    let p = value * factor;
    let p = if p < 0 { p + ((1 << shift) - 1) } else { p };
    p >> shift
}

// WIRED ON ONE HOST, and the split is deliberate rather than an oversight.
//
// **Browser**: `LegaiaMinigames::baka_hud_quad_json` calls this emitter, and
// the duel page's `_widget` draws the quad it returns - the corners, the
// inclusive UV span and the mirror arm are now this function's, not the page's.
// That is a behavioural change, not a rename: the page computed its half-extent
// as `(cell * scale) / 0x1000 / 2` in floating point with **no `size` term**,
// where retail is `((cell * scale) >> 13) * size >> 12` with both shifts
// rounding toward zero over a span of `x - hw ..= x + hw - 1`.
//
// **Native**: still unwired, and the blocker there is a texel source. The play
// window draws each `ChromeDraw` as font text because no PROT 1203 art page is
// resident in engine VRAM; the quad sink itself is not missing
// (`legaia_render_kernels::screen_prim::ScreenQuad` carries POLY_GT4 corners,
// per-vertex gouraud, CLUT/texpage, ABR and an OT bucket, and both hosts
// consume its `build_geometry` output). The earlier reason on this row said the
// sink was the gap; it is the page.
//
// One half of the emitter is surfaced but not applied on either host: the
// brightness-scaled gouraud pair. PSX modulation is a per-texel
// `texel * c / 128`, which the duel page's 2D canvas cannot express in one
// pass, so the page draws geometry + UVs and the colours ride along in the
// JSON.
//
// (Everything upstream was already present:
// `legaia_minigame_formats::baka_opponents::parse_baka_hud` is called by the play window -
// `window/minigames.rs` stages the 51 records for the duel - and by the browser
// duel page, which also decodes the PROT 1203 art pack the widgets sample; and
// `BakaChrome` emits one
// [`ChromeDraw`](crate::baka_fighter_chrome::ChromeDraw) per call of this
// emitter with the same `(widget, x, y, brightness, size)`, the play window
// resolving its `u` column through
// [`crate::baka_fighter_chrome::glyph_u`].)
/// PORT: FUN_801d5ed0 - the Baka Fighter HUD textured-quad emitter.
///
/// `FUN_801d5ed0(x, y, id, brightness, size)` draws widget `id` of the
/// 51-record descriptor table `DAT_801d7160`
/// ([`legaia_minigame_formats::baka_opponents::parse_baka_hud`]) as a POLY_GT4 centred
/// on `(x, y)`:
///
/// - half-extent per axis = `((cell * scale) >> 13) * size >> 12` (both
///   shifts round toward zero), spanning `x - hw ..= x + hw - 1`;
/// - every colour channel = `channel * brightness >> 8` (round toward
///   zero); verts 0/1 carry `rgb_top`, verts 2/3 `rgb_bottom`;
/// - UVs cover the cell inclusively (`u ..= u + w - 1`); `mirror` swaps the
///   left/right texture columns (retail's one-shot flag `DAT_801dbe98`,
///   consumed - zeroed - by every call);
/// - texpage attribute = `texpage + abr * 0x20` (the ABR blend fold), CLUT
///   passes through.
///
/// Retail then links the packet into the OT bucket `_DAT_801DBEBC` and
/// bumps that slot to 3 - host-side scheduling this kernel leaves to the
/// renderer.
pub fn hud_widget_quad(
    widget: &legaia_minigame_formats::baka_opponents::BakaHudWidget,
    x: i16,
    y: i16,
    brightness: i32,
    size: i32,
    mirror: bool,
) -> HudWidgetQuad {
    let scale8 = |c: u8| mips_scale(c as i32, brightness, 8) as u8;
    let half = |cell: u8| {
        let base = mips_scale(cell as i32, widget.scale, 13);
        mips_scale(size, base, 12)
    };
    let hw = half(widget.w) as i16;
    let hh = half(widget.h) as i16;
    let (u0, v0) = (widget.u, widget.v);
    let (u1, v1) = (
        widget.u.wrapping_add(widget.w).wrapping_sub(1),
        widget.v.wrapping_add(widget.h).wrapping_sub(1),
    );
    let uv = if mirror {
        [(u1, v0), (u0, v0), (u1, v1), (u0, v1)]
    } else {
        [(u0, v0), (u1, v0), (u0, v1), (u1, v1)]
    };
    HudWidgetQuad {
        poly_code: (widget.semi << 1) | 0x3C,
        x0: x - hw,
        y0: y - hh,
        x1: x + hw - 1,
        y1: y + hh - 1,
        uv,
        rgb_top: widget.rgb_top.map(scale8),
        rgb_bottom: widget.rgb_bottom.map(scale8),
        clut: widget.clut,
        tpage_attr: widget.texpage + widget.abr as u16 * 0x20,
    }
}

/// A minigame effect-part spawn spec - the argument set the tiny spawn
/// wrappers pass to the shared part-spawn API (`FUN_80021B04`) plus the
/// fields they stamp on the returned part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectSpawnSpec {
    /// Screen position handed to the spawn API.
    pub x: i16,
    pub y: i16,
    /// Fixed-point scale (`0x1000` = 1.0).
    pub scale: i32,
    /// Sprite/animation id stamped into the spawned part's `+0x50`.
    pub sprite_id: u16,
}

/// PORT: FUN_801d6e04 - the round chrome's screen-centre effect spawn:
/// retail zero-fills a spawn record, plants it at the fixed screen centre
/// `(0xA0, 0x78)` through the shared part-spawn API `FUN_80021B04` at scale
/// `0x1000`, then stamps `sprite_id` into the spawned part's `+0x50`. The
/// dance overlay's cell-placed twin is `FUN_801d3fd0`
/// (`crate::dance::step_mark_effect_spawn`).
pub fn center_effect_spawn(sprite_id: u16) -> EffectSpawnSpec {
    EffectSpawnSpec {
        x: 0xA0,
        y: 0x78,
        scale: 0x1000,
        sprite_id,
    }
}
