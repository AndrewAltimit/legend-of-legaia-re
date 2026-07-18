use crate::*;

/// Retail PSX framebuffer placement of the slot-info panel (bottom
/// of stage), parked / fully-slid-in. Pinned to FUN_801E08D8's
/// `FUN_801e36c4(0xA0, local_34, 0x11c, 0x40)` call through the
/// messagebox model (centre rect inflated by 8 on every side):
/// footprint `(160 - 142 - 10, local_34 - 2, 284 + 16, 64 + 16)` =
/// `(8, 136, 300, 80)` at the parked `local_34 = 138`. GP0-dump-pinned
/// on the live SlotPreview screen (corner tiles at (8, 136) / (304,
/// 212); left/right edges tiled 3 x 24 tall at x = 8 / 304).
pub const SLOT_INFO_PANEL_POS: (i32, i32) = (8, 136);
pub const SLOT_INFO_PANEL_SIZE: (u32, u32) = (300, 80);
/// Panel-y origin when fully slid-in (= `local_34` at anim_t=0x1000).
pub const SLOT_INFO_PANEL_PARKED_Y: i32 = 138;

/// Per-element offsets relative to the panel-y origin (= `local_34`),
/// derived from the Ghidra trace of `FUN_801E08D8`. The renderer adds
/// the live `panel_y` (interpolated through the slide-in animation) to
/// every offset.
///
/// Title-row offsets (`local_34 + 4` in retail = panel_y + 4):
/// - `SLOT_INFO_NO_OFFSET`: "No." badge (retail emits a sprite via
///   `FUN_801E3FF0` modes 2/3 at `(8, local_34 - 8)` with a CLUT row
///   selected by `DAT_801e5062 = slot_index << 4`. The engine renders
///   it as text at the same screen position, glyph-baseline corrected).
/// - `SLOT_INFO_LOCATION_OFFSET`: kingdom name string.
/// - `SLOT_INFO_TIME_LABEL_OFFSET`: "Time " prefix.
/// - `SLOT_INFO_TIME_VALUE_OFFSET`: HH:MM:SS digits. Retail splits the
///   digits across three calls (hours / minutes / seconds, with
///   sprite-colon separators) at x=236/252/260/276/284; the engine
///   renders one packed string at the leftmost x for simplicity.
pub const SLOT_INFO_NO_OFFSET: (i32, i32) = (8, -8);
pub const SLOT_INFO_LOCATION_OFFSET: (i32, i32) = (48, 4);
pub const SLOT_INFO_TIME_LABEL_OFFSET: (i32, i32) = (208, 4);
pub const SLOT_INFO_TIME_VALUE_OFFSET: (i32, i32) = (236, 4);

/// Per-character row offsets (column 0 of the 3-column slot grid;
/// retail loops `iVar4 = 0x10 + i*0x60` for columns 0/1/2 at
/// x = 16/112/208). For slots with one party member (Vahn-only starter
/// state) only column 0 renders. Y offsets relative to the panel y
/// origin via the retail `s3 = local_34 + 0x14` (= 158) per-character
/// base, then `s3 + N` for each row.
/// Centre-x and y-offset of the panel's single-line caption - what fills
/// the panel instead of a preview when the slot holds no loadable save
/// (retail view modes 2/3/4).
///
/// Retail emits it through the centring wrapper
/// `FUN_801E3EE0(text, 0xA0, local_34 + 0x18)`, which measures the string
/// and hands the raw text emitter `x - width/2` at `y + 7`. So the centre
/// is screen x=160 and the drawn y is `panel_y + 24 + 7` (GP0-dump-pinned:
/// the live "No data" caption glyphs sit at y = 169 = 138 + 31). The `+ 7`
/// is folded in here: unlike every other offset on this panel, which retail
/// passes straight to the raw emitter, this one goes through the wrapper.
pub const SLOT_INFO_CAPTION_CENTER_X: i32 = 160;
pub const SLOT_INFO_CAPTION_Y_OFFSET: i32 = 31;

pub const SLOT_INFO_PORTRAIT_OFFSET: (i32, i32) = (16, 16);
pub const SLOT_INFO_NAME_OFFSET: (i32, i32) = (40, 20);
pub const SLOT_INFO_LV_LABEL_OFFSET: (i32, i32) = (16, 33);
pub const SLOT_INFO_LV_VALUE_OFFSET: (i32, i32) = (48, 33);
pub const SLOT_INFO_HP_LABEL_OFFSET: (i32, i32) = (16, 46);
pub const SLOT_INFO_HP_VALUE_OFFSET: (i32, i32) = (32, 46);
/// Retail splits current / max across separate emits with a `/`
/// separator glyph between them (GP0 dump: col-0 HP cur digits from
/// x=32, separator at x=65, max digits from x=77; MP row cur from
/// x=40, separator x=65, max from x=85).
pub const SLOT_INFO_HP_SEP_OFFSET: (i32, i32) = (65, 46);
/// Companion to [`SLOT_INFO_HP_SEP_OFFSET`].
pub const SLOT_INFO_HP_MAX_OFFSET: (i32, i32) = (77, 46);
pub const SLOT_INFO_MP_LABEL_OFFSET: (i32, i32) = (16, 59);
pub const SLOT_INFO_MP_VALUE_OFFSET: (i32, i32) = (40, 59);
/// See [`SLOT_INFO_HP_SEP_OFFSET`].
pub const SLOT_INFO_MP_SEP_OFFSET: (i32, i32) = (65, 59);
/// Companion to [`SLOT_INFO_MP_SEP_OFFSET`].
pub const SLOT_INFO_MP_MAX_OFFSET: (i32, i32) = (85, 59);

/// Plain-data view of the per-slot info passed to the info-panel
/// renderer. Engines build one from the `SlotSnapshot` of the
/// currently-focused slot plus a `Party::from_retail_sc_block` lift
/// for the leader's HP/MP.
#[derive(Debug, Clone, Copy)]
pub struct SlotInfoView<'a> {
    pub slot_no: u8,
    pub location: &'a str,
    pub play_time: &'a str,
    pub leader_name: &'a str,
    pub leader_level: u8,
    pub leader_hp: (u16, u16),
    pub leader_mp: (u16, u16),
    pub leader_char_id: u8,
}

/// Build the chrome [`SpriteDraw`]s for the slot-info panel (9-slice
/// frame + optional leader portrait, no text). Pair with
/// [`slot_info_panel_text_draws_for`] for the labels.
///
/// `panel_y_offset` is the slide-in delta from the parked y
/// (positive = pushed below parked, used while the panel slides up).
/// Engine callers compute it from `interpolate_anim((0, OFFSCREEN),
/// (0, PARKED), session.info_panel_slide_anim_t()).1 - PARKED`.
pub fn slot_info_panel_draws_for(
    rects: &SaveMenuAtlasRects,
    info: Option<&SlotInfoView<'_>>,
    panel_y_offset: i32,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let mut out = Vec::with_capacity(20);
    let (px, py_base) = SLOT_INFO_PANEL_POS;
    let py = py_base + panel_y_offset;
    let (pw, ph) = SLOT_INFO_PANEL_SIZE;
    nine_slice_panel_into(
        &mut out,
        rects,
        (px, py, pw as i32, ph as i32),
        stage_origin,
        stage_scale,
        false,
    );

    // Leader portrait (16x16) inside the info panel - drawn only
    // when a save is present at the current slot. Position pinned
    // from FUN_801E08D8's `FUN_801e3ff0(0, _, iVar4=16, s3-4=154)`
    // with s3 = local_34 + 20: portrait top-left at (16, panel_y+16).
    if let Some(info) = info {
        let scale = stage_scale.max(1) as i32;
        let content_y = SLOT_INFO_PANEL_PARKED_Y + panel_y_offset;
        let mut sprite = |src: (u32, u32, u32, u32), sx: i32, sy: i32| {
            out.push(SpriteDraw {
                dst: (
                    stage_origin.0 + sx * scale,
                    stage_origin.1 + sy * scale,
                    src.2 * scale as u32,
                    src.3 * scale as u32,
                ),
                src,
                color: [1.0, 1.0, 1.0, 1.0],
            });
        };
        if let Some(portrait) = rects
            .load_portrait_by_char
            .get(info.leader_char_id as usize)
            .copied()
            .flatten()
        {
            sprite(
                portrait,
                SLOT_INFO_PORTRAIT_OFFSET.0,
                py_base + SLOT_INFO_PORTRAIT_OFFSET.1 + panel_y_offset,
            );
        }
        // Retail draws the LV / HP / MP row markers as 16x10 label
        // sprites from the system-UI sheet (GP0 dump: 16x10 quads at
        // the column base sampling the label strip with the (16, 511)
        // CLUT), not as font glyphs. The paired text builder skips its
        // ASCII stand-ins when told these sprites are being drawn.
        sprite(
            rects.label_lv,
            SLOT_INFO_LV_LABEL_OFFSET.0,
            content_y + SLOT_INFO_LV_LABEL_OFFSET.1,
        );
        sprite(
            rects.label_hp,
            SLOT_INFO_HP_LABEL_OFFSET.0,
            content_y + SLOT_INFO_HP_LABEL_OFFSET.1,
        );
        sprite(
            rects.label_mp,
            SLOT_INFO_MP_LABEL_OFFSET.0,
            content_y + SLOT_INFO_MP_LABEL_OFFSET.1,
        );
    }
    out
}

/// Build [`TextDraw`]s for the slot-info panel labels (No., kingdom
/// name, time, character stats). Returns empty when `info` is `None`.
/// `panel_y_offset` matches the value passed to
/// [`slot_info_panel_draws_for`].
///
/// `labels_as_sprites = true` skips the ASCII `LV` / `HP` / `MP`
/// stand-ins: when the system-UI atlas is resident the chrome builder
/// emits retail's 16x10 label sprites at the same offsets, and the
/// double-draw would collide with the value columns. Pass `false`
/// only for the no-atlas fallback.
pub fn slot_info_panel_text_draws_for(
    font: &legaia_font::Font,
    info: Option<&SlotInfoView<'_>>,
    panel_y_offset: i32,
    stage_origin: (i32, i32),
    stage_scale: u32,
    labels_as_sprites: bool,
) -> Vec<TextDraw> {
    let Some(info) = info else { return Vec::new() };
    let scale = stage_scale.max(1);
    let color = SAVE_SELECT_TITLE_COLOR;
    let panel_y = SLOT_INFO_PANEL_PARKED_Y + panel_y_offset;
    let mut out = Vec::with_capacity(80);

    let emit_at = |out: &mut Vec<TextDraw>, text: &str, base: (i32, i32)| {
        let layout = font.layout_ascii(text);
        for g in &layout.glyphs {
            let sx = base.0 + g.dst_x;
            let sy = base.1 + g.dst_y;
            out.push(TextDraw {
                dst: (
                    stage_origin.0 + sx * scale as i32,
                    stage_origin.1 + sy * scale as i32,
                    g.width * scale,
                    g.height * scale,
                ),
                src: (g.atlas_x, g.atlas_y, g.width, g.height),
                color,
            });
        }
    };

    // Title row.
    emit_at(
        &mut out,
        &format!("No.{}", info.slot_no),
        (SLOT_INFO_NO_OFFSET.0, panel_y + SLOT_INFO_NO_OFFSET.1),
    );
    emit_at(
        &mut out,
        info.location,
        (
            SLOT_INFO_LOCATION_OFFSET.0,
            panel_y + SLOT_INFO_LOCATION_OFFSET.1,
        ),
    );
    emit_at(
        &mut out,
        "Time",
        (
            SLOT_INFO_TIME_LABEL_OFFSET.0,
            panel_y + SLOT_INFO_TIME_LABEL_OFFSET.1,
        ),
    );
    emit_at(
        &mut out,
        info.play_time,
        (
            SLOT_INFO_TIME_VALUE_OFFSET.0,
            panel_y + SLOT_INFO_TIME_VALUE_OFFSET.1,
        ),
    );

    // Character row (column 0 only - multi-character party expansion
    // would re-iterate at base_x += 96).
    emit_at(
        &mut out,
        info.leader_name,
        (SLOT_INFO_NAME_OFFSET.0, panel_y + SLOT_INFO_NAME_OFFSET.1),
    );
    if !labels_as_sprites {
        emit_at(
            &mut out,
            "LV",
            (
                SLOT_INFO_LV_LABEL_OFFSET.0,
                panel_y + SLOT_INFO_LV_LABEL_OFFSET.1,
            ),
        );
    }
    emit_at(
        &mut out,
        &format!("{}", info.leader_level),
        (
            SLOT_INFO_LV_VALUE_OFFSET.0,
            panel_y + SLOT_INFO_LV_VALUE_OFFSET.1,
        ),
    );
    // Retail emits current, `/` separator, and max at fixed columns
    // rather than one packed string, so the max column stays aligned
    // regardless of the current value's digit count.
    if !labels_as_sprites {
        emit_at(
            &mut out,
            "HP",
            (
                SLOT_INFO_HP_LABEL_OFFSET.0,
                panel_y + SLOT_INFO_HP_LABEL_OFFSET.1,
            ),
        );
    }
    emit_at(
        &mut out,
        &format!("{}", info.leader_hp.0),
        (
            SLOT_INFO_HP_VALUE_OFFSET.0,
            panel_y + SLOT_INFO_HP_VALUE_OFFSET.1,
        ),
    );
    emit_at(
        &mut out,
        "/",
        (
            SLOT_INFO_HP_SEP_OFFSET.0,
            panel_y + SLOT_INFO_HP_SEP_OFFSET.1,
        ),
    );
    emit_at(
        &mut out,
        &format!("{}", info.leader_hp.1),
        (
            SLOT_INFO_HP_MAX_OFFSET.0,
            panel_y + SLOT_INFO_HP_MAX_OFFSET.1,
        ),
    );
    if !labels_as_sprites {
        emit_at(
            &mut out,
            "MP",
            (
                SLOT_INFO_MP_LABEL_OFFSET.0,
                panel_y + SLOT_INFO_MP_LABEL_OFFSET.1,
            ),
        );
    }
    emit_at(
        &mut out,
        &format!("{}", info.leader_mp.0),
        (
            SLOT_INFO_MP_VALUE_OFFSET.0,
            panel_y + SLOT_INFO_MP_VALUE_OFFSET.1,
        ),
    );
    emit_at(
        &mut out,
        "/",
        (
            SLOT_INFO_MP_SEP_OFFSET.0,
            panel_y + SLOT_INFO_MP_SEP_OFFSET.1,
        ),
    );
    emit_at(
        &mut out,
        &format!("{}", info.leader_mp.1),
        (
            SLOT_INFO_MP_MAX_OFFSET.0,
            panel_y + SLOT_INFO_MP_MAX_OFFSET.1,
        ),
    );
    out
}

/// Build the [`TextDraw`]s for the info panel's centred caption - the body
/// of the panel when the focused slot holds no loadable save.
///
/// The counterpart to [`slot_info_panel_text_draws_for`]: exactly one of the
/// two fills the panel, mirroring retail, where a single `view_mode` picks
/// between the stats preview and one centred line. Callers get the string
/// from `SlotInfoMode::caption`; pass the same `panel_y_offset` as the
/// chrome.
///
/// PORT: FUN_801E08D8's `FUN_801E3EE0(caption, 0xA0, local_34 + 0x18)` tail.
pub fn slot_info_caption_draws_for(
    font: &legaia_font::Font,
    caption: &str,
    panel_y_offset: i32,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<TextDraw> {
    if caption.is_empty() {
        return Vec::new();
    }
    let scale = stage_scale.max(1);
    let layout = font.layout_ascii(caption);
    // Retail measures the string and draws from `x - width/2`, truncating
    // toward zero exactly as this does.
    let base_x = SLOT_INFO_CAPTION_CENTER_X - layout.advance_x as i32 / 2;
    let base_y = SLOT_INFO_PANEL_PARKED_Y + panel_y_offset + SLOT_INFO_CAPTION_Y_OFFSET;

    layout
        .glyphs
        .iter()
        .map(|g| TextDraw {
            dst: (
                stage_origin.0 + (base_x + g.dst_x) * scale as i32,
                stage_origin.1 + (base_y + g.dst_y) * scale as i32,
                g.width * scale,
                g.height * scale,
            ),
            src: (g.atlas_x, g.atlas_y, g.width, g.height),
            color: SAVE_SELECT_TITLE_COLOR,
        })
        .collect()
}
