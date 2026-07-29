//! The PROT 0977 (`other_game` / Muscle Dome arena door-init) overlay's HUD
//! primitive layer: a table-driven textured-Gouraud-quad emitter and the
//! decimal readout built on top of it.
//!
//! Three routines share one descriptor table at overlay VA `0x801D170C`,
//! stride `0x14` ([`HUD_SPRITE_STRIDE`]). Each emits a PSX `POLY_GT4`
//! (Gouraud-shaded, textured four-point polygon, GP0 command `0x3C`, or
//! `0x3E` when the record's semi-transparency byte is set) into the
//! scratchpad primitive pool and links it into the ordering table at the
//! depth held in `DAT_801D1AA8`, which the emitter then resets to `3`.
//!
//! The two emitters differ only in how the quad is placed and scaled:
//!
//! * [`hud_quad_centred`] (`FUN_801D050C`) treats `(x, y)` as the **centre**
//!   and halves the extent (`>> 13` instead of `>> 12`), so the quad spans
//!   `x - half ..= x + half - 1`.
//! * [`hud_quad_corner`] (`FUN_801D08EC`) treats `(x, y)` as the **top-left**
//!   corner and spans `x ..= x + extent`, and clamps its brightness argument
//!   to `0..=0xFF` first - the centred emitter does not clamp.
//!
//! [`decimal_slots`] / [`decimal_quads`] (`FUN_801D1308`) render an unsigned
//! decimal readout of up to eight digits through the centred emitter, using
//! record index [`DIGIT_SPRITE_INDEX`] as the glyph and stepping the glyph's
//! texture-U column per digit.
//!
//! Provenance: `ghidra/scripts/funcs/overlay_0977_other_game_801d050c.txt`,
//! `..._801d08ec.txt`, `..._801d1308.txt`; ported from the disassembly, not
//! the decompiled C.
//!
//! # Wiring
//!
//! The descriptor table is disc data: PROT entry 0977 is a slot-A overlay
//! (base `0x801CE818`), so the table at VA `0x801D170C` sits at file offset
//! [`SPRITE_TABLE_FILE_OFFSET`] of the raw entry, and
//! [`parse_sprite_table`] decodes it straight off a `PROT.DAT` image. The
//! records name the Muscle Dome hub's texture pages - `tpage 0x0005` /
//! `0x0015` are the 4bpp pages at `(320, 0)` / `(320, 256)` the dome's own
//! data file uploads (extraction 1220, `other6.lzs` slot 0: an LZS
//! container whose section 0 carries the two page TIMs + CLUT rows
//! 502/503), capture-verified against a live course-menu VRAM snapshot.
//! Record 3 is the "Welcome to the Muscle Dome!" cursive strip, record 16
//! the INTERVAL heading, records 0/1 the ROUND word + hub digit strip.
//! The site's Muscle Dome page consumes the parsed records as the geometry
//! source for its intro card and interval heading
//! (`legaia-web-viewer::minigames_muscle::muscle_hud_json`).
//!
//! Where each quad goes is disc data too. The entry holds 40 emitter call
//! sites - 9 centred, 19 corner, 12 decimal - all inside PROT 0977's own hub
//! screens (`0x801CF2C0 .. 0x801D04EC`), and their `(sel, x, y, scale)`
//! arguments are `li`/`addiu` immediates that constant-propagation over the
//! entry's own bytes recovers. [`HUB_INTRO_CARD`] and its siblings are those
//! recovered rows, each carrying the VA of the `jal` it came from; a
//! disc-gated test re-reads that word and asserts it is still a `jal` to the
//! emitter the row names. So a page drawing through
//! [`hub_screen_quads`] has both its extent (each record's `size` field) and
//! its placement disc-derived.
//!
//! The screens' *state* is a different matter and stays out of here: which
//! screen is up, and the fade / zoom counters that feed the `brightness` and
//! `scale` arguments, belong to the un-ported hub controllers. A host that
//! draws a screen supplies those.

/// Byte stride of one sprite descriptor in the table at `0x801D170C`.
pub const HUD_SPRITE_STRIDE: usize = 0x14;

/// Load base of the PROT 0977 slot-A overlay image.
pub const OVERLAY_BASE_VA: u32 = 0x801C_E818;

/// Overlay VA of the sprite descriptor table.
pub const SPRITE_TABLE_VA: u32 = 0x801D_170C;

/// File offset of the sprite table inside the raw PROT 0977 entry
/// (`SPRITE_TABLE_VA - OVERLAY_BASE_VA`).
pub const SPRITE_TABLE_FILE_OFFSET: usize = (SPRITE_TABLE_VA - OVERLAY_BASE_VA) as usize;

/// Populated record count. Records `0..=16` carry real sprite descriptors
/// (tpage `0x0005` / `0x0015`, CLUT words in rows 502/503); from record 17
/// on, the bytes are unrelated overlay data, not descriptors.
pub const SPRITE_TABLE_LEN: usize = 17;

/// Low-bit width of the emitter's `sel` argument that selects a table row.
/// The remaining high bits are the *variant* (`sel >> 10`, truncating).
pub const HUD_SEL_INDEX_BITS: u32 = 10;

/// Table row the decimal renderer draws every digit from.
pub const DIGIT_SPRITE_INDEX: usize = 9;

/// Digit slots the decimal renderer walks (`10^7 .. 10^0`).
pub const DECIMAL_SLOTS: usize = 8;

/// Texture-U column of decimal digit `0`. Each digit steps `+8` from here
/// (`u = digit * 8 - 0x80`, stored as a byte).
pub const DIGIT_U_BASE: i32 = -0x80;

/// Horizontal advance between two digit cells, in screen pixels.
pub const DIGIT_ADVANCE: i32 = 8;

/// CLUT the digit record carries at rest; the renderer offsets it by its
/// palette argument for the duration of the call and restores it after.
pub const DIGIT_CLUT_BASE: u16 = 0x7D86;

/// Quad scale the decimal renderer passes to the centred emitter (1.0 in
/// 12.12 fixed point).
pub const DIGIT_SCALE: i32 = 0x1000;

/// One `0x14`-byte record of the sprite descriptor table at `0x801D170C`.
///
/// Field offsets are the retail ones; the emitters *mutate* two of them
/// (`semi_transparent` and `page`) whenever they are called with a non-zero
/// variant, and that mutation persists in the table for later calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HudSprite {
    /// `+0x00` - texel-to-world size scalar, applied before the caller's
    /// scale.
    pub size: i32,
    /// `+0x04` - base tpage word; the emitter adds `page * 0x20`.
    pub tpage: u16,
    /// `+0x06` - CLUT word.
    pub clut: u16,
    /// `+0x08` - texture U of the top-left texel.
    pub u0: u8,
    /// `+0x09` - texture V of the top-left texel.
    pub v0: u8,
    /// `+0x0A` - texel width.
    pub w: u8,
    /// `+0x0B` - texel height.
    pub h: u8,
    /// `+0x0C..0x0E` - colour of the two **top** vertices.
    pub rgb_top: [u8; 3],
    /// `+0x0F` - non-zero selects the semi-transparent command (`0x3E`).
    pub semi_transparent: u8,
    /// `+0x10..0x12` - colour of the two **bottom** vertices, which is what
    /// makes every quad a vertical two-stop gradient.
    pub rgb_bottom: [u8; 3],
    /// `+0x13` - tpage page offset, multiplied by `0x20` into the tpage word.
    pub page: u8,
}

/// A resolved `POLY_GT4` packet, renderer-agnostic.
///
/// Vertex order is the PSX one: `0` top-left, `1` top-right, `2` bottom-left,
/// `3` bottom-right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HudQuad {
    /// Screen-space vertex positions.
    pub xy: [(i16, i16); 4],
    /// Per-vertex texture coordinates.
    pub uv: [(u8, u8); 4],
    /// Per-vertex colour, already scaled by the brightness argument.
    pub rgb: [[u8; 3]; 4],
    /// Texture-page word (`record.tpage + page * 0x20`).
    pub tpage: u16,
    /// CLUT word (`record.clut`, `+1` when the variant is `2`).
    pub clut: u16,
    /// `true` selects GP0 `0x3E` over `0x3C`.
    pub semi_transparent: bool,
}

/// GP0 command byte of an opaque Gouraud-textured quad.
pub const GP0_POLY_GT4: u8 = 0x3C;

impl HudSprite {
    /// Decode one `0x14`-byte descriptor record (little-endian, retail
    /// field layout - see the struct's per-field offsets).
    pub fn parse(rec: &[u8]) -> Option<HudSprite> {
        if rec.len() < HUD_SPRITE_STRIDE {
            return None;
        }
        Some(HudSprite {
            size: i32::from_le_bytes(rec[0..4].try_into().ok()?),
            tpage: u16::from_le_bytes(rec[4..6].try_into().ok()?),
            clut: u16::from_le_bytes(rec[6..8].try_into().ok()?),
            u0: rec[8],
            v0: rec[9],
            w: rec[10],
            h: rec[11],
            rgb_top: [rec[12], rec[13], rec[14]],
            semi_transparent: rec[15],
            rgb_bottom: [rec[16], rec[17], rec[18]],
            page: rec[19],
        })
    }
}

/// Parse the [`SPRITE_TABLE_LEN`] populated descriptor records out of a raw
/// PROT 0977 entry image (the bytes exactly as they sit in `PROT.DAT`).
///
/// Returns an empty vec when the entry is too short to hold the table.
pub fn parse_sprite_table(overlay_0977: &[u8]) -> Vec<HudSprite> {
    let Some(table) = overlay_0977.get(
        SPRITE_TABLE_FILE_OFFSET..SPRITE_TABLE_FILE_OFFSET + SPRITE_TABLE_LEN * HUD_SPRITE_STRIDE,
    ) else {
        return Vec::new();
    };
    table
        .chunks_exact(HUD_SPRITE_STRIDE)
        .filter_map(HudSprite::parse)
        .collect()
}

/// Split an emitter `sel` argument into `(table index, variant)`.
///
/// The retail code divides by `0x400` truncating toward zero
/// (`if (sel < 0) sel += 0x3FF; sel >>= 10`) and masks the index with
/// `0x3FF`, so a negative `sel` yields a negative variant and a *positive*
/// index.
#[inline]
pub fn hud_sel_split(sel: i32) -> (usize, i32) {
    let biased = if sel < 0 {
        sel.wrapping_add(0x3FF)
    } else {
        sel
    };
    ((sel & 0x3FF) as usize, biased >> HUD_SEL_INDEX_BITS)
}

/// Scale one colour channel by the emitter's brightness argument
/// (`c * brightness / 256`, truncating toward zero, stored as a byte).
#[inline]
fn scale_channel(c: u8, brightness: i32) -> u8 {
    let p = (c as i32).wrapping_mul(brightness);
    let p = if p < 0 { p.wrapping_add(0xFF) } else { p };
    (p >> 8) as u8
}

/// Apply the variant side effects the retail emitters perform on the shared
/// table before building the packet, and return the CLUT the packet uses.
fn apply_variant(rec: &mut HudSprite, variant: i32) -> u16 {
    if variant != 0 {
        rec.semi_transparent = 1;
        rec.page = variant as u8;
    }
    if variant == 2 {
        rec.clut.wrapping_add(1)
    } else {
        rec.clut
    }
}

/// Fill the parts of a quad that both emitters share: colours, texture
/// coordinates, tpage, CLUT and the transparency flag.
fn shared_quad(rec: &HudSprite, brightness: i32, clut: u16) -> HudQuad {
    let top = [
        scale_channel(rec.rgb_top[0], brightness),
        scale_channel(rec.rgb_top[1], brightness),
        scale_channel(rec.rgb_top[2], brightness),
    ];
    let bottom = [
        scale_channel(rec.rgb_bottom[0], brightness),
        scale_channel(rec.rgb_bottom[1], brightness),
        scale_channel(rec.rgb_bottom[2], brightness),
    ];
    let u1 = rec.u0.wrapping_add(rec.w).wrapping_sub(1);
    let v1 = rec.v0.wrapping_add(rec.h).wrapping_sub(1);
    HudQuad {
        xy: [(0, 0); 4],
        uv: [(rec.u0, rec.v0), (u1, rec.v0), (rec.u0, v1), (u1, v1)],
        rgb: [top, top, bottom, bottom],
        tpage: rec.tpage.wrapping_add((rec.page as u16).wrapping_mul(0x20)),
        clut,
        semi_transparent: rec.semi_transparent != 0,
    }
}

/// Half-extent of the centred emitter: `((texels * size) >> 13) * scale >> 12`,
/// each shift truncating toward zero.
fn centred_half(texels: u8, size: i32, scale: i32) -> i32 {
    let p = (texels as i32).wrapping_mul(size);
    let p = if p < 0 { p.wrapping_add(0x1FFF) } else { p };
    let q = (p >> 13).wrapping_mul(scale);
    let q = if q < 0 { q.wrapping_add(0xFFF) } else { q };
    q >> 12
}

/// Full extent of the corner emitter: the same chain with `>> 12` first.
fn corner_span(texels: u8, size: i32, scale: i32) -> i32 {
    let p = (texels as i32).wrapping_mul(size);
    let p = if p < 0 { p.wrapping_add(0xFFF) } else { p };
    let q = (p >> 12).wrapping_mul(scale);
    let q = if q < 0 { q.wrapping_add(0xFFF) } else { q };
    q >> 12
}

/// Emit the quad **centred** on `(x, y)`.
///
/// `brightness` is applied to every colour channel unclamped (a value above
/// `0x100` overflows the byte exactly as retail does); `scale` is 12.12 fixed
/// point. `variant` is `sel >> 10` and, when non-zero, is written back into
/// the shared record as its transparency flag and tpage page.
///
/// PORT: FUN_801d050c
///
/// The PROT 0977 **hub-screen** quad emitter - the intro card, the INTERVAL
/// heading and the ROUND word / hub digit strip. Not the match strip: the
/// four-turn Turns Left / HP Left readout is drawn from the battle-action
/// overlay 0898 through `func_0x8003541C` (label register + draw) and
/// `func_0x8003563C` (per-actor record-queue append) - a different overlay,
/// a different primitive path, and not the dome's readout at all (see
/// `docs/subsystems/minigame-muscle-dome.md`).
///
/// Wired through [`hub_screen_quads`]: the dome page calls
/// `minigames_muscle::muscle_hub_quads_json`, which runs this emitter over
/// the recovered draw lists and hands the page finished screen rects.
pub fn hud_quad_centred(
    rec: &mut HudSprite,
    x: i16,
    y: i16,
    variant: i32,
    brightness: i32,
    scale: i32,
) -> HudQuad {
    let clut = apply_variant(rec, variant);
    let mut q = shared_quad(rec, brightness, clut);
    let hw = centred_half(rec.w, rec.size, scale);
    let hh = centred_half(rec.h, rec.size, scale);
    let x0 = (x as i32).wrapping_sub(hw) as i16;
    let x1 = (x as i32).wrapping_add(hw).wrapping_sub(1) as i16;
    let y0 = (y as i32).wrapping_sub(hh) as i16;
    let y1 = (y as i32).wrapping_add(hh).wrapping_sub(1) as i16;
    q.xy = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)];
    q
}

/// Emit the quad with `(x, y)` as its **top-left** corner.
///
/// Unlike [`hud_quad_centred`] this clamps `brightness` into `0..=0xFF`
/// before scaling, and its span uses one shift less, so the same record
/// covers twice the pixels for the same `scale`.
///
/// PORT: FUN_801d08ec
///
/// Wired through [`hub_screen_quads`] alongside [`hud_quad_centred`]: the
/// score tally's six label strips ([`HUB_SCORE_TALLY_LABELS`]) are the
/// corner-anchored draws.
pub fn hud_quad_corner(
    rec: &mut HudSprite,
    x: i16,
    y: i16,
    variant: i32,
    brightness: i32,
    scale: i32,
) -> HudQuad {
    let brightness = brightness.clamp(0, 0xFF);
    let clut = apply_variant(rec, variant);
    let mut q = shared_quad(rec, brightness, clut);
    let w = corner_span(rec.w, rec.size, scale);
    let h = corner_span(rec.h, rec.size, scale);
    let x1 = (x as i32).wrapping_add(w) as i16;
    let y1 = (y as i32).wrapping_add(h) as i16;
    q.xy = [(x, y), (x1, y), (x, y1), (x1, y1)];
    q
}

/// The eight decimal slots of a readout, most significant first.
///
/// A slot holds `Some(digit)` when retail would draw it. The rule is retail's
/// own and is not plain leading-zero suppression: the slot array starts at
/// `-1` everywhere, the **units** slot is pre-seeded with `0`, and slot `i`
/// is then written with `value / 10^(7-i)` only when that quotient is
/// non-zero. A slot whose stored quotient is negative is skipped at draw
/// time, so a **negative `value` renders nothing at all**.
///
/// Wired: this is the *shared* fill. `FUN_801d1308` and the fishing
/// overlay's digit field `FUN_801d76e0` open with the identical loop - same
/// `-1` init, same pre-seeded units slot, same `!= 0` store gate, same eight
/// `/10` steps, same negative-slot skip - so
/// [`crate::number_digit_cells`] takes its slots from here, which puts this
/// on the live fishing HUD path (native window and browser page both). The
/// two retail routines diverge only after the fill: this overlay emits one
/// widget id and passes the digit by patching a descriptor's texture column,
/// while the fishing one selects between two emitters and two pen pitches.
///
/// PORT: FUN_801d1308 (slot fill)
pub fn decimal_slots(value: i32) -> [Option<u8>; DECIMAL_SLOTS] {
    let mut raw = [-1i32; DECIMAL_SLOTS];
    raw[DECIMAL_SLOTS - 1] = 0;
    let mut divisor = 10_000_000i32;
    for slot in raw.iter_mut() {
        let q = value / divisor;
        if q != 0 {
            *slot = q;
        }
        divisor /= 10;
    }
    let mut out = [None; DECIMAL_SLOTS];
    for (o, q) in out.iter_mut().zip(raw) {
        if q >= 0 {
            *o = Some((q % 10) as u8);
        }
    }
    out
}

/// Texture-U column of one decimal glyph (`digit * 8 - 0x80`, byte-wrapped).
///
/// PORT: FUN_801d1308 (glyph column)
///
/// Reached through [`decimal_quads`], which the score tally drives.
#[inline]
pub fn digit_column(digit: u8) -> u8 {
    ((digit as i32) * 8 + DIGIT_U_BASE) as u8
}

/// Build the quads of a decimal readout starting at `(x, y)`.
///
/// `digit` is the table's digit record ([`DIGIT_SPRITE_INDEX`]); it is
/// mutated exactly as retail mutates it - the CLUT is offset by `palette` for
/// the duration of the call, each drawn digit rewrites the record's `u0`, and
/// the CLUT is restored to [`DIGIT_CLUT_BASE`] on return. Every glyph goes
/// through the centred emitter at [`DIGIT_SCALE`], and the pen advances
/// [`DIGIT_ADVANCE`] per slot **including** the slots that draw nothing.
///
/// PORT: FUN_801d1308
///
/// The retail decimal-readout emitter, built on [`hud_quad_centred`]. Wired
/// through [`score_tally_quads`] - where retail drives it, as the six tally
/// values, each drawn in two palette passes. A hub readout, not the match
/// strip: see [`hud_quad_centred`].
pub fn decimal_quads(
    digit: &mut HudSprite,
    x: i16,
    y: i16,
    value: i32,
    brightness: i32,
    palette: i16,
) -> Vec<HudQuad> {
    digit.clut = DIGIT_CLUT_BASE.wrapping_add(palette as u16);
    let mut out = Vec::new();
    let mut pen = x;
    for slot in decimal_slots(value) {
        if let Some(d) = slot {
            digit.u0 = digit_column(d);
            out.push(hud_quad_centred(digit, pen, y, 0, brightness, DIGIT_SCALE));
        }
        pen = pen.wrapping_add(DIGIT_ADVANCE as i16);
    }
    digit.clut = DIGIT_CLUT_BASE;
    out
}

// --- Hub screens: the recovered retail draw lists ---------------------------

/// Table row the ROUND banner's digit emitter draws every glyph from
/// (`FUN_801D15C8` patches this record's `u0`, not record
/// [`DIGIT_SPRITE_INDEX`]'s).
pub const ROUND_DIGIT_SPRITE_INDEX: usize = 1;

/// Texture-U pitch of one ROUND-banner glyph: `FUN_801D15C8` computes the
/// column as `digit * 24` (`(d*2 + d) << 3`) and stores it as record
/// [`ROUND_DIGIT_SPRITE_INDEX`]'s `u0` byte.
pub const ROUND_DIGIT_U_PITCH: u8 = 24;

/// How an emitter interprets a draw's `(x, y)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubAnchor {
    /// [`hud_quad_centred`] - `(x, y)` is the quad's centre.
    Centre,
    /// [`hud_quad_corner`] - `(x, y)` is the quad's top-left.
    Corner,
    /// [`hud_quad_centred`] through `FUN_801D15C8`: the `sel` index is
    /// replaced by [`ROUND_DIGIT_SPRITE_INDEX`] and that record's `u0` is
    /// first set to `digit * `[`ROUND_DIGIT_U_PITCH`].
    RoundDigit(u8),
}

/// One recovered draw of a PROT 0977 hub screen.
///
/// `sel` is the raw emitter argument (`index | variant << 10`); `call_site`
/// is the retail VA of the `jal` the row was read from, which is what makes
/// the row checkable against the disc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HubDraw {
    /// Emitter `sel` argument - see [`hud_sel_split`].
    pub sel: i32,
    /// Screen x, in the retail 320x240 frame.
    pub x: i16,
    /// Screen y.
    pub y: i16,
    /// Which emitter, and how it reads `(x, y)`.
    pub anchor: HubAnchor,
    /// 12.12 fixed-point scale argument.
    pub scale: i32,
    /// Retail VA of the `jal` this row was recovered from.
    pub call_site: u32,
}

/// The **intro card**: the "Welcome to the Muscle Dome!" cursive strip
/// (record 3) dead-centre of the frame. `FUN_801CF870`'s opening arm; its
/// brightness argument is that arm's own fade counter (`DAT_801D1A80`).
pub const HUB_INTRO_CARD: &[HubDraw] = &[HubDraw {
    sel: 3,
    x: 0xA0,
    y: 0x78,
    anchor: HubAnchor::Centre,
    scale: 0x1000,
    call_site: 0x801C_FA4C,
}];

/// The **course-title card**: record 4's 218x64 art centred high in the
/// frame with its variant-2 twin offset `(+8, +8)` as a drop shadow. Both
/// are drawn at a fixed brightness `0x80`; the retail arm animates the
/// *scale* instead, ramping `DAT_801D1A88` down from
/// [`TITLE_ART_ZOOM_START`] to [`TITLE_ART_ZOOM_END`].
///
/// The shadow's arguments are set at `0x801CFB04` and the arm then `j`s into
/// the screen's shared emit tail, so its `call_site` is that tail's `jal` -
/// which several draws of this screen share.
pub const HUB_TITLE_ART: &[HubDraw] = &[
    HubDraw {
        sel: 0x804,
        x: 0xA8,
        y: 0x48,
        anchor: HubAnchor::Centre,
        scale: 0x1000,
        call_site: 0x801C_FED0,
    },
    HubDraw {
        sel: 4,
        x: 0xA0,
        y: 0x40,
        anchor: HubAnchor::Centre,
        scale: 0x1000,
        call_site: 0x801C_FAFC,
    },
];

/// Brightness both [`HUB_TITLE_ART`] draws are issued at.
pub const TITLE_ART_BRIGHTNESS: i32 = 0x80;

/// Scale the title-art zoom starts at (`DAT_801D1A88` seed).
pub const TITLE_ART_ZOOM_START: i32 = 0x1640;

/// Scale the title-art zoom clamps to.
pub const TITLE_ART_ZOOM_END: i32 = 0x1000;

/// The **INTERVAL heading** (record 16) centred near the top of the frame.
pub const HUB_INTERVAL_HEADING: &[HubDraw] = &[HubDraw {
    sel: 0x10,
    x: 0xA0,
    y: 0x20,
    anchor: HubAnchor::Centre,
    scale: 0x1000,
    call_site: 0x801C_FED0,
}];

/// Screen y every [`round_banner_draws`] piece sits on.
pub const ROUND_BANNER_Y: i16 = 0x78;

/// Screen x of the ROUND word's centre.
pub const ROUND_WORD_X: i16 = 0x78;

/// Screen x of the round number's first (or only) digit.
pub const ROUND_DIGIT_X: i16 = 0xF0;

/// Screen x of the units digit when the round number has two digits
/// (`ROUND_DIGIT_X + 24`, the same 24 px the glyph pitch uses).
pub const ROUND_DIGIT_X2: i16 = 0x108;

/// The **ROUND banner**: the ROUND word (record 0) plus the round number,
/// each piece drawn twice - variant 1 then variant 2 - which is what gives
/// the word its two-tone edge.
///
/// `round` is the displayed number (`DAT_801D1A94 + 1`); a value below 10
/// draws one digit at [`ROUND_DIGIT_X`], otherwise the tens digit goes there
/// and the units digit at [`ROUND_DIGIT_X2`]. The retail order is
/// **all of variant 1, then all of variant 2**.
///
/// PORT: FUN_801d02f0
pub fn round_banner_draws(round: i32) -> Vec<HubDraw> {
    let mut out = Vec::new();
    for (variant, site_word) in [(0x400, 0x801D_0324u32), (0x800, 0x801D_033C)] {
        out.push(HubDraw {
            sel: variant,
            x: ROUND_WORD_X,
            y: ROUND_BANNER_Y,
            anchor: HubAnchor::Centre,
            scale: 0x1000,
            call_site: site_word,
        });
    }
    // Retail emits the word's two variants back to back, then the digits'.
    let digits: Vec<(i16, u8)> = if round < 10 {
        vec![(ROUND_DIGIT_X, round.clamp(0, 9) as u8)]
    } else {
        let tens = round / 10;
        vec![
            (ROUND_DIGIT_X, (tens % 10) as u8),
            (ROUND_DIGIT_X2, (round - tens * 10) as u8),
        ]
    };
    for (variant, site) in [(0x400, 0x801D_036Cu32), (0x800, 0x801D_03EC)] {
        for &(x, d) in &digits {
            out.push(HubDraw {
                sel: variant,
                x,
                y: ROUND_BANNER_Y,
                anchor: HubAnchor::RoundDigit(d),
                scale: 0x1000,
                call_site: site,
            });
        }
    }
    out
}

/// Rows of the score-tally readout ([`HUB_SCORE_TALLY_LABELS`]).
pub const SCORE_TALLY_ROWS: usize = 6;

/// Screen x of every tally label's top-left corner.
pub const SCORE_TALLY_LABEL_X: i16 = 0x40;

/// Screen y of the first tally label; rows step [`SCORE_TALLY_ROW_PITCH`].
pub const SCORE_TALLY_LABEL_Y: i16 = 0x50;

/// Screen x of every tally value's first digit.
pub const SCORE_TALLY_VALUE_X: i16 = 0xC0;

/// Screen y of the first tally value - the label row's `y + 5`.
pub const SCORE_TALLY_VALUE_Y: i16 = 0x55;

/// Vertical pitch between two tally rows.
pub const SCORE_TALLY_ROW_PITCH: i16 = 0x10;

/// First sprite-table record of the six tally label strips (records
/// `10 ..= 15`, each a 96x8 texel strip on the hub's page).
pub const SCORE_TALLY_LABEL_INDEX: usize = 10;

/// The **score tally's** six label strips, in retail draw order: all six in
/// variant 1, then all six in variant 2.
///
/// The tally's *values* are not constants - they come off the overlay's
/// counter block, so they are built by [`score_tally_quads`] instead.
pub const HUB_SCORE_TALLY_LABELS: &[HubDraw] = &[
    HubDraw {
        sel: 0x40A,
        x: 0x40,
        y: 0x50,
        anchor: HubAnchor::Corner,
        scale: 0x1000,
        call_site: 0x801C_F2C0,
    },
    HubDraw {
        sel: 0x40B,
        x: 0x40,
        y: 0x60,
        anchor: HubAnchor::Corner,
        scale: 0x1000,
        call_site: 0x801C_F300,
    },
    HubDraw {
        sel: 0x40C,
        x: 0x40,
        y: 0x70,
        anchor: HubAnchor::Corner,
        scale: 0x1000,
        call_site: 0x801C_F340,
    },
    HubDraw {
        sel: 0x40D,
        x: 0x40,
        y: 0x80,
        anchor: HubAnchor::Corner,
        scale: 0x1000,
        call_site: 0x801C_F378,
    },
    HubDraw {
        sel: 0x40E,
        x: 0x40,
        y: 0x90,
        anchor: HubAnchor::Corner,
        scale: 0x1000,
        call_site: 0x801C_F3B4,
    },
    HubDraw {
        sel: 0x40F,
        x: 0x40,
        y: 0xA0,
        anchor: HubAnchor::Corner,
        scale: 0x1000,
        call_site: 0x801C_F3E8,
    },
    HubDraw {
        sel: 0x80A,
        x: 0x40,
        y: 0x50,
        anchor: HubAnchor::Corner,
        scale: 0x1000,
        call_site: 0x801C_F418,
    },
    HubDraw {
        sel: 0x80B,
        x: 0x40,
        y: 0x60,
        anchor: HubAnchor::Corner,
        scale: 0x1000,
        call_site: 0x801C_F448,
    },
    HubDraw {
        sel: 0x80C,
        x: 0x40,
        y: 0x70,
        anchor: HubAnchor::Corner,
        scale: 0x1000,
        call_site: 0x801C_F478,
    },
    HubDraw {
        sel: 0x80D,
        x: 0x40,
        y: 0x80,
        anchor: HubAnchor::Corner,
        scale: 0x1000,
        call_site: 0x801C_F4A8,
    },
    HubDraw {
        sel: 0x80E,
        x: 0x40,
        y: 0x90,
        anchor: HubAnchor::Corner,
        scale: 0x1000,
        call_site: 0x801C_F4D8,
    },
    HubDraw {
        sel: 0x80F,
        x: 0x40,
        y: 0xA0,
        anchor: HubAnchor::Corner,
        scale: 0x1000,
        call_site: 0x801C_F508,
    },
];

/// Per-row `(palette, palette)` pair of the tally's two decimal passes.
///
/// Retail draws all six values once with the first palette of the pair, then
/// all six again with the second - rows `0..=3` use `(0, 1)` and rows `4`/`5`
/// the brighter `(2, 3)`. Between the passes it also stamps the digit
/// record's `semi_transparent` byte to `1` and its `page` byte to the pass
/// number, which is what [`hud_quad_centred`]'s variant side effect does.
pub const SCORE_TALLY_VALUE_PALETTES: [(i16, i16); SCORE_TALLY_ROWS] =
    [(0, 1), (0, 1), (0, 1), (0, 1), (2, 3), (2, 3)];

/// Build one hub screen's quads from a parsed sprite table.
///
/// `table` is [`parse_sprite_table`]'s output; it is taken by `&mut` because
/// the retail emitters write their variant back into the shared record and
/// the next call sees it - reproducing that is the point. A draw naming a
/// record the table does not hold is skipped.
///
/// PORT: FUN_801d15c8 (the [`HubAnchor::RoundDigit`] arm)
pub fn hub_screen_quads(
    table: &mut [HudSprite],
    draws: &[HubDraw],
    brightness: i32,
) -> Vec<HudQuad> {
    let mut out = Vec::new();
    for d in draws {
        let (idx, variant) = hud_sel_split(d.sel);
        let idx = match d.anchor {
            HubAnchor::RoundDigit(_) => ROUND_DIGIT_SPRITE_INDEX,
            _ => idx,
        };
        let Some(rec) = table.get_mut(idx) else {
            continue;
        };
        if let HubAnchor::RoundDigit(digit) = d.anchor {
            rec.u0 = digit.wrapping_mul(ROUND_DIGIT_U_PITCH);
        }
        out.push(match d.anchor {
            HubAnchor::Corner => hud_quad_corner(rec, d.x, d.y, variant, brightness, d.scale),
            _ => hud_quad_centred(rec, d.x, d.y, variant, brightness, d.scale),
        });
    }
    out
}

/// Build the whole score-tally readout: the six label strips followed by the
/// six decimal values, in retail's two-pass order.
///
/// `brightness` is per row - each row fades in on its own lane counter (see
/// `legaia_engine_core::other_game_overlay`, whose `step_scale` is the ramp
/// each lane counts up by).
///
/// `values` are the contest's own rows: the four lanes
/// `legaia_engine_core::muscle_dome::LegScoreRows` carries, then the running
/// tally and the coin bank they settle into.
pub fn score_tally_quads(
    table: &mut [HudSprite],
    values: [i32; SCORE_TALLY_ROWS],
    brightness: [i32; SCORE_TALLY_ROWS],
) -> Vec<HudQuad> {
    let mut out = Vec::new();
    for (i, d) in HUB_SCORE_TALLY_LABELS.iter().enumerate() {
        let mut one = *d;
        one.sel = d.sel;
        out.extend(hub_screen_quads(
            table,
            std::slice::from_ref(&one),
            brightness[i % SCORE_TALLY_ROWS],
        ));
    }
    for pass in 0..2 {
        for row in 0..SCORE_TALLY_ROWS {
            let Some(digit) = table.get_mut(DIGIT_SPRITE_INDEX) else {
                continue;
            };
            // The `sb` pair retail issues between the passes: the digit
            // record's transparency flag and its tpage page.
            digit.semi_transparent = 1;
            digit.page = pass as u8 + 1;
            let (pal_a, pal_b) = SCORE_TALLY_VALUE_PALETTES[row];
            out.extend(decimal_quads(
                digit,
                SCORE_TALLY_VALUE_X,
                SCORE_TALLY_VALUE_Y + SCORE_TALLY_ROW_PITCH * row as i16,
                values[row],
                brightness[row],
                if pass == 0 { pal_a } else { pal_b },
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sprite() -> HudSprite {
        HudSprite {
            size: 0x1000,
            tpage: 0x0040,
            clut: 0x7D86,
            u0: 0x10,
            v0: 0x20,
            w: 8,
            h: 16,
            rgb_top: [0x80, 0x40, 0x20],
            semi_transparent: 0,
            rgb_bottom: [0x40, 0x20, 0x10],
            page: 0,
        }
    }

    #[test]
    fn sprite_record_parses_the_retail_field_layout() {
        let mut rec = [0u8; HUD_SPRITE_STRIDE];
        rec[0..4].copy_from_slice(&0x1000i32.to_le_bytes());
        rec[4..6].copy_from_slice(&0x0005u16.to_le_bytes());
        rec[6..8].copy_from_slice(&0x7D86u16.to_le_bytes());
        rec[8] = 0; // u0
        rec[9] = 224; // v0
        rec[10] = 240; // w
        rec[11] = 18; // h
        rec[12..15].copy_from_slice(&[0xFF, 0xFF, 0xFF]);
        rec[15] = 1;
        rec[16..19].copy_from_slice(&[0xFF, 0xFF, 0xFF]);
        rec[19] = 1;
        let s = HudSprite::parse(&rec).unwrap();
        assert_eq!(s.tpage, 0x0005);
        assert_eq!(s.clut, 0x7D86);
        assert_eq!((s.u0, s.v0, s.w, s.h), (0, 224, 240, 18));
        assert_eq!(s.semi_transparent, 1);
        assert_eq!(s.page, 1);
    }

    #[test]
    fn sprite_table_parses_from_a_raw_overlay_image() {
        // Synthetic overlay: zeros with one recognisable record at the
        // table offset.
        let mut img = vec![0u8; SPRITE_TABLE_FILE_OFFSET + SPRITE_TABLE_LEN * HUD_SPRITE_STRIDE];
        img[SPRITE_TABLE_FILE_OFFSET + 4..SPRITE_TABLE_FILE_OFFSET + 6]
            .copy_from_slice(&0x0015u16.to_le_bytes());
        let t = parse_sprite_table(&img);
        assert_eq!(t.len(), SPRITE_TABLE_LEN);
        assert_eq!(t[0].tpage, 0x0015);
        assert!(parse_sprite_table(&[0u8; 16]).is_empty(), "short image");
    }

    #[test]
    fn sel_splits_index_and_variant() {
        assert_eq!(hud_sel_split(9), (9, 0));
        assert_eq!(hud_sel_split(0x409), (9, 1));
        assert_eq!(hud_sel_split(0x809), (9, 2));
        // The index mask keeps the low ten bits even for a negative sel.
        assert_eq!(hud_sel_split(-1).0, 0x3FF);
    }

    #[test]
    fn full_brightness_passes_the_record_colours_through() {
        let mut r = sprite();
        let q = hud_quad_centred(&mut r, 100, 50, 0, 0x100, 0x1000);
        assert_eq!(q.rgb[0], [0x80, 0x40, 0x20]);
        assert_eq!(q.rgb[1], q.rgb[0], "both top vertices share a colour");
        assert_eq!(q.rgb[2], [0x40, 0x20, 0x10]);
        assert_eq!(q.rgb[3], q.rgb[2], "both bottom vertices share a colour");
    }

    #[test]
    fn half_brightness_halves_every_channel() {
        let mut r = sprite();
        let q = hud_quad_centred(&mut r, 0, 0, 0, 0x80, 0x1000);
        assert_eq!(q.rgb[0], [0x40, 0x20, 0x10]);
    }

    #[test]
    fn the_centred_emitter_brackets_the_anchor() {
        let mut r = sprite();
        // size 0x1000 and scale 0x1000 make the half-extent w/2 and h/2.
        let q = hud_quad_centred(&mut r, 100, 50, 0, 0x100, 0x1000);
        assert_eq!(q.xy[0], (100 - 4, 50 - 8));
        assert_eq!(q.xy[3], (100 + 4 - 1, 50 + 8 - 1));
    }

    #[test]
    fn the_corner_emitter_spans_the_full_extent_from_the_anchor() {
        let mut r = sprite();
        let q = hud_quad_corner(&mut r, 100, 50, 0, 0x100, 0x1000);
        assert_eq!(q.xy[0], (100, 50));
        assert_eq!(q.xy[3], (100 + 8, 50 + 16));
    }

    #[test]
    fn only_the_corner_emitter_clamps_brightness() {
        let mut a = sprite();
        let mut b = sprite();
        // 0x200 doubles: 0x80 * 0x200 >> 8 = 0x100, which truncates to 0.
        assert_eq!(
            hud_quad_centred(&mut a, 0, 0, 0, 0x200, 0x1000).rgb[0][0],
            0
        );
        // The corner emitter clamps to 0xFF first, so nothing overflows.
        assert_eq!(
            hud_quad_corner(&mut b, 0, 0, 0, 0x200, 0x1000).rgb[0][0],
            0x7F
        );
    }

    #[test]
    fn texture_coordinates_span_the_record_rect() {
        let mut r = sprite();
        let q = hud_quad_centred(&mut r, 0, 0, 0, 0x100, 0x1000);
        assert_eq!(q.uv[0], (0x10, 0x20));
        assert_eq!(q.uv[3], (0x10 + 8 - 1, 0x20 + 16 - 1));
    }

    #[test]
    fn a_non_zero_variant_writes_back_into_the_shared_record() {
        let mut r = sprite();
        let q = hud_quad_centred(&mut r, 0, 0, 1, 0x100, 0x1000);
        assert!(q.semi_transparent);
        assert_eq!(q.tpage, 0x0040 + 0x20);
        assert_eq!(r.page, 1, "the mutation persists in the table");
        assert_eq!(r.semi_transparent, 1);
        // A later variant-0 call still sees the mutated record.
        let q2 = hud_quad_centred(&mut r, 0, 0, 0, 0x100, 0x1000);
        assert!(q2.semi_transparent);
        assert_eq!(q2.tpage, 0x0040 + 0x20);
    }

    #[test]
    fn variant_two_also_bumps_the_clut() {
        let mut r = sprite();
        let q = hud_quad_centred(&mut r, 0, 0, 2, 0x100, 0x1000);
        assert_eq!(q.clut, 0x7D86 + 1);
        assert_eq!(r.page, 2);
        // The record's own CLUT is not mutated - only the packet's.
        assert_eq!(r.clut, 0x7D86);
    }

    #[test]
    fn zero_renders_a_single_units_digit() {
        let s = decimal_slots(0);
        assert_eq!(s[..7].iter().filter(|d| d.is_some()).count(), 0);
        assert_eq!(s[7], Some(0));
    }

    #[test]
    fn leading_zeros_are_suppressed() {
        let s = decimal_slots(1000);
        assert_eq!(
            s,
            [None, None, None, None, Some(1), Some(0), Some(0), Some(0)]
        );
    }

    #[test]
    fn eight_digits_fill_every_slot() {
        let s = decimal_slots(12_345_678);
        let got: Vec<u8> = s.iter().map(|d| d.unwrap()).collect();
        assert_eq!(got, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn a_negative_value_draws_nothing() {
        // Every stored quotient is negative, including the pre-seeded units
        // slot, so retail's `bltz` skip drops all eight.
        assert!(decimal_slots(-42).iter().all(|d| d.is_none()));
    }

    #[test]
    fn digit_columns_step_eight_from_the_base() {
        assert_eq!(digit_column(0), 0x80);
        assert_eq!(digit_column(1), 0x88);
        assert_eq!(digit_column(9), 0xC8);
    }

    #[test]
    fn the_readout_advances_over_suppressed_slots_too() {
        let mut d = sprite();
        let quads = decimal_quads(&mut d, 100, 60, 42, 0x100, 0);
        assert_eq!(quads.len(), 2);
        // Slots 6 and 7 draw; the pen has already stepped six cells.
        assert_eq!(quads[0].xy[0].0, 100 + 6 * 8 - 4);
        assert_eq!(quads[1].xy[0].0, 100 + 7 * 8 - 4);
    }

    #[test]
    fn the_readout_restores_the_digit_clut() {
        let mut d = sprite();
        let quads = decimal_quads(&mut d, 0, 0, 7, 0x100, 3);
        assert_eq!(quads[0].clut, DIGIT_CLUT_BASE + 3);
        assert_eq!(d.clut, DIGIT_CLUT_BASE, "restored on return");
    }
}
