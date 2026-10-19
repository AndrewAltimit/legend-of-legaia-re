//! Marks baked off the effect texture page: the cross-out, the Rot stamp and the Curse plate.

use super::*;

/// Bake the command ring's three marks into `atlas` from the battle effect
/// atlas (PROT 870, the entry `legaia_engine_scene_host::scene_host::host::upload_flame_atlas_into_vram`
/// uploads on battle entry): the TIM whose image lands on page `(448, 0)`,
/// each mark decoded through its own 16-colour palette on CLUT row 476.
///
/// - the red cross-out X, texels `(0, 96)`..`(63, 111)` through `(64, 476)` -
///   the quad `legaia_engine_battle_vm::battle_party_panel::cross_out_mark` samples;
/// - the Rot stamp, `(80, 96)`..`(111, 119)` through `(176, 476)` -
///   `rot_stamp_on_chip` / `rot_stamp_on_arts_chip`;
/// - the Curse plate, `(120, 96)`..`(183, 111)` through `(0, 476)` -
///   `curse_plate_on_chip`.
///
/// The marks live on an effect page, not on the system-UI sheet the rest of
/// the atlas comes from, which is why this is a separate step: both play
/// hosts draw the command chips and the arts-entry chips out of this atlas,
/// and a mark has to sit in the same texture to draw over them. Returns
/// whether the X baked; a source without that TIM leaves the atlas
/// untouched.
pub fn add_cross_out_mark(atlas: &mut SaveMenuAtlas, flame_atlas_entry: &[u8]) -> bool {
    for target in legaia_actor_formats::befect_cluster::scan_tims(flame_atlas_entry) {
        let Ok(tim) = legaia_tim::parse(&flame_atlas_entry[target.offset..]) else {
            continue;
        };
        if (tim.image.fb_x, tim.image.fb_y) != CROSS_OUT_PAGE
            || tim.mode != legaia_tim::PixelMode::Bpp4
        {
            continue;
        }
        if !bake_effect_page_cell(
            atlas,
            &tim,
            CROSS_OUT_CLUT,
            CROSS_OUT_TEXELS,
            ATLAS_RECT_CROSS_OUT,
        ) {
            continue;
        }
        atlas.has_cross_out = true;
        atlas.has_rot_stamp = bake_effect_page_cell(
            atlas,
            &tim,
            ROT_STAMP_CLUT,
            ROT_STAMP_TEXELS,
            ATLAS_RECT_ROT_STAMP,
        );
        atlas.has_curse_plate = bake_effect_page_cell(
            atlas,
            &tim,
            CURSE_PLATE_CLUT,
            CURSE_PLATE_TEXELS,
            ATLAS_RECT_CURSE_PLATE,
        );
        return true;
    }
    false
}

/// Decode `texels` of the effect-page `tim` through the 16-colour palette at
/// VRAM `clut` and copy them to `cell`. `false` (atlas untouched) when the
/// TIM's CLUT block does not hold that palette or the page is too small.
fn bake_effect_page_cell(
    atlas: &mut SaveMenuAtlas,
    tim: &legaia_tim::Tim,
    clut: (u16, u16),
    texels: (u32, u32, u32, u32),
    cell: (u32, u32, u32, u32),
) -> bool {
    let Some(block) = tim.clut.as_ref() else {
        return false;
    };
    let (cx, cy) = clut;
    if cy < block.fb_y || cx < block.fb_x || cy >= block.fb_y + block.h {
        return false;
    }
    let entry = usize::from(cy - block.fb_y) * usize::from(block.w) + usize::from(cx - block.fb_x);
    if entry % 16 != 0 {
        return false;
    }
    let Ok(rgba) = legaia_tim::decode_rgba8(tim, entry / 16) else {
        return false;
    };
    let src_w = tim.pixel_width() as u32;
    let (u, v, w, h) = texels;
    if src_w < u + w || (tim.pixel_height() as u32) < v + h {
        return false;
    }
    copy_rect(&mut atlas.rgba, atlas.width, &rgba, src_w, texels, cell);
    true
}
