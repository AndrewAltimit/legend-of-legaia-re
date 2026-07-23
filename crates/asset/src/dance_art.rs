//! Dance-minigame **presentation art** - the PROT 1230 TIM pack, the dance
//! overlay's HUD widget table, and the dancer face-stamp rig.
//!
//! The dance overlay (PROT 0980) issues no texture load of its own - its three
//! PROT loads are all sound (see `docs/subsystems/minigame-dance.md`). The art
//! it draws with is staged by the mode-24 entry path as extraction PROT
//! **1230** (`other7`, a [`legaia_prot::timpack`] of 31 TIMs): the HUD page at
//! VRAM `(512, 0)` with its 16-palette CLUT row at `(0, 500)`, three dancer
//! face strips at `(400..432, 0)`, face-part cells at `(320..384, 0..192)`,
//! and the dance hall's venue texture pages at `(512..832, 0/256)`. 27 of the
//! 31 image blocks (and the HUD CLUT row) are byte-identical to a live retail
//! VRAM capture parked in the minigame; the four that differ are exactly the
//! face strips whose top window the pose blit has been rewriting - positive
//! confirmation of the mechanism, not a mismatch.
//!
//! ## HUD widget table (`DAT_801d46cc`)
//!
//! Every HUD element the overlay draws goes through the textured-quad emitter
//! `FUN_801d2f38`, which indexes a 34-record x 20-byte descriptor table at
//! overlay VA `0x801D46CC`. Record layout (read straight off the emitter):
//!
//! | offset | field |
//! |---|---|
//! | `+0x00` | `i32` scale (12.12 fixed; retail rows are all `0x1000`) |
//! | `+0x04` | `u16` texpage attribute (all HUD rows: `0x0008` = 4bpp page `(512,0)`) |
//! | `+0x06` | `u16` CLUT id (`0x7D00 + n` = palette `n` of the row-500 strip) |
//! | `+0x08` | `u8` u0, `+0x09` v0, `+0x0A` w, `+0x0B` h (cell in the page) |
//! | `+0x0C` | RGB top-edge tint, `+0x0F` semi-transparency code |
//! | `+0x10` | RGB bottom-edge tint, `+0x13` CLUT row offset |
//!
//! The quad is drawn **centred** on the emitter's `(x, y)` with full `w x h`
//! extent. Callers patch records in place: the digit renderers rewrite `u0`,
//! the beat-track flash rewrites the CLUT id (`0x7D0D` on the every-4th-beat
//! combo window, `0x7D0E` on the scrolling notes).
//!
//! ## The dancer face stamp (`FUN_801d03c4`)
//!
//! The dancers on the floor are field-scene actors; the overlay animates their
//! **faces** by `MoveImage` (`FUN_80058490`) blits inside a per-dancer VRAM
//! strip: an eye cell and a mouth cell are copied from the strip's pose bank
//! (rows 64..128) into its live window (the rows the head mesh samples). Rig
//! constants below are read from the traced jumptable bodies; the per-pose
//! source offsets are a small table in the overlay image. Dancer 0's strip is
//! **Noa's own field atlas** (PROT 0874 §2 entry 2 at `(852, 256)` - see
//! [`crate::field_char_textures`]); dancers 1..3 use the pack's strips.

use anyhow::{Context, Result, bail};
use legaia_tim::{PixelMode, Tim, decode_rgba8};

use crate::minigame_art::Sprite;

/// Extraction PROT entry of the dance art pack (raw TOC `0x4D2`).
pub const DANCE_ART_PROT_INDEX: usize = 1230;
/// Extraction PROT entry of the dance's `efect.dat` SFX descriptor bank.
pub const DANCE_SFX_BANK_PROT_INDEX: usize = 1228;
/// Extraction PROT entry of the dance's SFX sample VAB (raw TOC `0x4D1`,
/// loaded by the overlay itself). Sits in the PROT TOC's zeroed tail - needs
/// the footprint fallback in the TOC parsers to resolve at all.
pub const DANCE_SFX_VAB_PROT_INDEX: usize = 1231;
/// The dance's two BGM entries (`music_01` bank; the overlay picks by mode) -
/// extraction 1048 = sound-test #60 `M116` "Sol disco final 1", 1054 = #66
/// `M120` "Sol disco final 2" (the piecewise bank map: extraction = 988+index
/// for indices <= 67).
pub const DANCE_BGM_PROT_INDEX: usize = 1048;
pub const DANCE_BGM_ALT_PROT_INDEX: usize = 1054;

/// Cue ids the overlay fires into the runtime bank (`FUN_801d1af4` sites).
pub const CUE_DANCE_INTRO: u16 = 0x200;
pub const CUE_DANCE_START: u16 = 0x201;
pub const CUE_DANCE_MISS: u16 = 0x210;
/// Combo-tier stings: `Cool!` / `Great!!` / `Fever!!!` (tiers 3/4/5).
pub const CUE_DANCE_COOL: u16 = 0x202;
pub const CUE_DANCE_GREAT: u16 = 0x203;
pub const CUE_DANCE_FEVER: u16 = 0x205;

/// Link base of the dance overlay image (static-overlay map, PROT 0980).
pub const DANCE_OVERLAY_BASE_VA: u32 = 0x801C_E818;
/// VA of the HUD widget descriptor table.
pub const WIDGET_TABLE_VA: u32 = 0x801D_46CC;
/// Records in the widget table (ids `0..=33`; the emitters and the banner
/// spawner never pass a larger id).
pub const WIDGET_COUNT: usize = 34;
/// Bytes per widget record.
pub const WIDGET_STRIDE: usize = 20;

// Widget ids, named from their traced draw sites.
/// `READY...` banner (slide-in, `FUN_801d2d98` cluster).
pub const W_READY: usize = 0;
/// Big blue digit font (16x24; `FUN_801d32f8` patches `u0 = digit * 0x10`).
pub const W_DIGIT: usize = 1;
/// `Lv.` label + level digit (`FUN_801d3e28`; digit `u0 = 0xD0 + lv * 8`).
pub const W_LV_LABEL: usize = 6;
pub const W_LV_DIGIT: usize = 7;
/// Score box (64x40, one per dancer, `FUN_801d231c`).
pub const W_SCORE_BOX: usize = 8;
/// `Miss!` banner (`FUN_801d1af4` -> spawner, at (160, 128)).
pub const W_MISS: usize = 10;
/// `Good!` banner (`FUN_801d40dc`, with two stars).
pub const W_GOOD: usize = 11;
/// `GO!` banner (drawn with the intro fade ramp).
pub const W_GO: usize = 12;
/// Scrolling beat-track notes: chart symbol `s` draws widget `13 + s`
/// (`FUN_801d2524`; symbol 0 = the empty dot).
pub const W_NOTE_BASE: usize = 13;
/// Beat-track end caps + the every-4th-beat flash targets.
pub const W_TRACK_CAP_L: usize = 16;
pub const W_TRACK_CAP_R: usize = 17;
/// The marker arrow over the track.
pub const W_TRACK_ARROW: usize = 18;
/// `Cool!` / `Great!!` / `Fever!!!` combo banners (tiers 3/4/5).
pub const W_COOL: usize = 19;
pub const W_GREAT: usize = 20;
pub const W_FEVER: usize = 21;
/// The star sparkle flanking a banner.
pub const W_STAR: usize = 22;
/// `FINISH!` banner (state 0xB).
pub const W_FINISH: usize = 23;
/// Lead-in countdown digits `1` / `2` / `3`.
pub const W_COUNT_1: usize = 24;
pub const W_COUNT_2: usize = 25;
pub const W_COUNT_3: usize = 26;
/// Beat-track body tile (12 draws of 8px) - a flash target with the caps.
pub const W_TRACK_BODY: usize = 30;
/// Step-stock marker (one per remaining timing-button press).
pub const W_STOCK: usize = 31;
/// `HI SCORE` label (free-play mode) + its small digit font.
pub const W_HISCORE: usize = 32;
pub const W_SMALL_DIGIT: usize = 33;

/// CLUT ids the emitters patch in at runtime (`FUN_801d2524`).
pub const CLUT_TRACK_IDLE: u16 = 0x7D08;
pub const CLUT_TRACK_FLASH: u16 = 0x7D0D;
pub const CLUT_NOTE: u16 = 0x7D0E;

/// One record of the HUD widget descriptor table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DanceWidget {
    /// 12.12 fixed-point scale (`0x1000` = 1:1 on every retail row).
    pub scale: i32,
    /// PSX texpage attribute (`0x0008` = 4bpp page at `(512, 0)`).
    pub tpage: u16,
    /// PSX CLUT id (`0x7D00 + n` = palette `n` of the `(0, 500)` strip).
    pub clut: u16,
    /// Cell rect inside the (256px-wide 4bpp) page.
    pub u: u8,
    pub v: u8,
    pub w: u8,
    pub h: u8,
    /// Top-edge / bottom-edge vertex tints (the emitter gouraud-blends them).
    pub rgb_top: [u8; 3],
    pub rgb_bottom: [u8; 3],
    /// Semi-transparency code fed into the prim command byte.
    pub semi: u8,
}

impl DanceWidget {
    /// Palette column of the row-500 HUD CLUT strip this widget names.
    pub fn palette_index(&self) -> usize {
        (self.clut & 0x3F) as usize
    }
    /// Texpage base in VRAM halfword coordinates.
    pub fn tpage_xy(&self) -> (u16, u16) {
        (((self.tpage & 0xF) * 64), ((self.tpage >> 4) & 1) * 256)
    }
}

/// Parse the HUD widget table out of the **as-loaded** dance overlay image.
pub fn parse_widgets(overlay: &[u8]) -> Result<Vec<DanceWidget>> {
    let base = (WIDGET_TABLE_VA - DANCE_OVERLAY_BASE_VA) as usize;
    let end = base + WIDGET_COUNT * WIDGET_STRIDE;
    let table = overlay
        .get(base..end)
        .context("dance overlay image too small for the widget table")?;
    let mut out = Vec::with_capacity(WIDGET_COUNT);
    for rec in table.chunks_exact(WIDGET_STRIDE) {
        out.push(DanceWidget {
            scale: i32::from_le_bytes(rec[0..4].try_into().unwrap()),
            tpage: u16::from_le_bytes(rec[4..6].try_into().unwrap()),
            clut: u16::from_le_bytes(rec[6..8].try_into().unwrap()),
            u: rec[8],
            v: rec[9],
            w: rec[10],
            h: rec[11],
            rgb_top: [rec[12], rec[13], rec[14]],
            rgb_bottom: [rec[16], rec[17], rec[18]],
            semi: rec[15],
        });
    }
    // Self-check: the retail rows are uniform 1:1 scale and all name the
    // HUD page (a decode at the wrong offset fails this immediately).
    let sane = out
        .iter()
        .take(27)
        .filter(|w| w.scale == 0x1000 && w.tpage == 0x0008 && (w.clut & 0xFFC0) == 0x7D00)
        .count();
    if sane < 20 {
        bail!("widget table failed the shape check ({sane}/27 sane rows)");
    }
    Ok(out)
}

/// Decode the raw PROT 1230 entry into its TIMs (a `prot::timpack`).
pub fn parse_art_pack(entry: &[u8]) -> Result<Vec<Tim>> {
    if !legaia_prot::timpack::is_tim_pack(entry) {
        bail!("PROT 1230 entry is not a TIM pack");
    }
    let items = legaia_prot::timpack::unpack(entry);
    let tims: Vec<Tim> = items
        .iter()
        .filter_map(|b| legaia_tim::parse(b).ok())
        .collect();
    if tims.len() < 28 {
        bail!("dance art pack decoded only {} TIMs", tims.len());
    }
    // The HUD page must be present at its pinned rect.
    hud_page(&tims).context("dance art pack has no HUD page at (512, 0)")?;
    Ok(tims)
}

/// The HUD page TIM - image `(512, 0)` 64hw x 256, CLUT strip `(0, 500)`.
pub fn hud_page(tims: &[Tim]) -> Result<&Tim> {
    tims.iter()
        .find(|t| t.image.fb_x == 512 && t.image.fb_y == 0 && t.mode == PixelMode::Bpp4)
        .context("no (512,0) 4bpp page in the pack")
}

/// Decode the 256x256 HUD page through palette `n` of its row-500 CLUT strip.
pub fn hud_page_rgba(tims: &[Tim], palette: usize) -> Result<Sprite> {
    let tim = hud_page(tims)?;
    let rgba = decode_rgba8(tim, palette)?;
    Ok(Sprite {
        width: tim.pixel_width(),
        height: tim.pixel_height(),
        rgba,
    })
}

/// Decode any pack page (by its VRAM rect) through one of its own palettes -
/// the venue texture pages (`(576..832, 0/256)`) decode this way too.
pub fn page_rgba(tims: &[Tim], fb_x: u16, fb_y: u16, palette: usize) -> Result<Sprite> {
    let tim = tims
        .iter()
        .find(|t| t.image.fb_x == fb_x && t.image.fb_y == fb_y)
        .with_context(|| format!("no pack TIM at ({fb_x}, {fb_y})"))?;
    let rgba = decode_rgba8(tim, palette)?;
    Ok(Sprite {
        width: tim.pixel_width(),
        height: tim.pixel_height(),
        rgba,
    })
}

// ---------------------------------------------------------------------------
// Dancer face-stamp rig
// ---------------------------------------------------------------------------

/// One `MoveImage` blit of the face stamp, in VRAM halfword coordinates.
#[derive(Debug, Clone, Copy)]
pub struct FaceBlit {
    /// Copy width in halfwords (4bpp: pixels = `w_hw * 4`).
    pub w_hw: u16,
    /// Copy height in rows.
    pub h: u16,
    /// Destination inside the dancer's strip (absolute VRAM halfword coords).
    pub dst: (u16, u16),
}

/// Per-dancer face rig: the strip window and the two blits, as the traced
/// `FUN_801d03c4` jumptable bodies set them up.
#[derive(Debug, Clone, Copy)]
pub struct FaceRig {
    /// The strip's VRAM origin in halfword coordinates (`s4`/`s3`).
    pub base: (u16, u16),
    /// VA of the per-pose source-offset table (`s5`).
    pub table_va: u32,
    /// Poses in the table.
    pub poses: usize,
    /// Eye-cell blit (the first `MoveImage`).
    pub eyes: FaceBlit,
    /// Mouth-cell blit (the second `MoveImage`).
    pub mouth: FaceBlit,
}

/// The four rigs, jumptable-case order. Case 0 is the human dancer - the strip
/// is **Noa's field atlas** at `(852, 256)` (PROT 0874 §2 entry 2), resident
/// from the field scene. Cases 1..3 are the pack strips at `(400..432, 0)`.
/// In mode 0 (yosenn) the overlay remaps dancers `1 -> 2`, `2 -> 3`.
pub const FACE_RIGS: [FaceRig; 4] = [
    FaceRig {
        base: (0x354, 0x100),
        table_va: 0x801D_435C,
        poses: 5,
        eyes: FaceBlit {
            w_hw: 6,
            h: 16,
            dst: (0x354, 0x10C),
        },
        mouth: FaceBlit {
            w_hw: 4,
            h: 8,
            dst: (0x355, 0x11C),
        },
    },
    FaceRig {
        base: (0x190, 0),
        table_va: 0x801D_4370,
        poses: 4,
        eyes: FaceBlit {
            w_hw: 13,
            h: 16,
            dst: (0x190, 8),
        },
        mouth: FaceBlit {
            w_hw: 3,
            h: 8,
            dst: (0x192, 0x20),
        },
    },
    FaceRig {
        base: (0x1A0, 0),
        table_va: 0x801D_4380,
        poses: 4,
        eyes: FaceBlit {
            w_hw: 13,
            h: 16,
            dst: (0x1A0, 8),
        },
        mouth: FaceBlit {
            w_hw: 3,
            h: 8,
            dst: (0x1A2, 0x2F),
        },
    },
    FaceRig {
        base: (0x1B0, 0),
        table_va: 0x801D_4390,
        poses: 4,
        eyes: FaceBlit {
            w_hw: 12,
            h: 16,
            dst: (0x1B2, 0xA),
        },
        mouth: FaceBlit {
            w_hw: 3,
            h: 8,
            dst: (0x1B2, 0x29),
        },
    },
];

/// Read a rig's per-pose source offsets out of the overlay image. Each frame
/// is 4 bytes: `[eye_u, eye_v, mouth_u, mouth_v]`, where the `u` bytes are in
/// **pixels** (the blit shifts them `>> 2` into halfwords) and the `v` bytes
/// are rows, both relative to the rig's `base`.
pub fn parse_face_frames(overlay: &[u8], rig: &FaceRig) -> Result<Vec<[u8; 4]>> {
    let off = (rig.table_va - DANCE_OVERLAY_BASE_VA) as usize;
    let bytes = overlay
        .get(off..off + rig.poses * 4)
        .context("overlay image too small for the face frame table")?;
    Ok(bytes
        .chunks_exact(4)
        .map(|c| [c[0], c[1], c[2], c[3]])
        .collect())
}

/// Compose one dancer's face window with pose `pose` stamped in, decoded
/// through palette `palette` of the strip's own CLUT. `strip` must be the TIM
/// whose image rect contains the rig's window (pack strip or Noa's atlas).
/// Returns the strip's top window (width x `window_h` rows) as RGBA8.
pub fn face_window_rgba(
    strip: &Tim,
    rig: &FaceRig,
    frames: &[[u8; 4]],
    pose: usize,
    palette: usize,
    window_h: usize,
) -> Result<Sprite> {
    if strip.mode != PixelMode::Bpp4 {
        bail!("face strip is not 4bpp");
    }
    let frame = frames
        .get(pose.min(frames.len().saturating_sub(1)))
        .context("empty face frame table")?;
    let mut tim = strip.clone();
    let row_bytes = tim.image.fb_w as usize * 2;

    // One MoveImage: halfword-rect copy inside the strip's image block.
    let mut blit = |src_hw: (u16, u16), dst_hw: (u16, u16), w_hw: usize, h: usize| {
        for r in 0..h {
            let sy = src_hw.1 as i32 - tim.image.fb_y as i32 + r as i32;
            let dy = dst_hw.1 as i32 - tim.image.fb_y as i32 + r as i32;
            let sx = (src_hw.0 as i32 - tim.image.fb_x as i32) * 2;
            let dx = (dst_hw.0 as i32 - tim.image.fb_x as i32) * 2;
            if sy < 0 || dy < 0 || sx < 0 || dx < 0 {
                continue;
            }
            let (sy, dy, sx, dx) = (sy as usize, dy as usize, sx as usize, dx as usize);
            let so = sy * row_bytes + sx;
            let d_o = dy * row_bytes + dx;
            let n = w_hw * 2;
            if so + n > tim.image.data.len() || d_o + n > tim.image.data.len() {
                continue;
            }
            let src: Vec<u8> = tim.image.data[so..so + n].to_vec();
            tim.image.data[d_o..d_o + n].copy_from_slice(&src);
        }
    };

    // Eye cell: src = base + (u >> 2, v).
    blit(
        (
            rig.base.0 + (frame[0] >> 2) as u16,
            rig.base.1 + frame[1] as u16,
        ),
        rig.eyes.dst,
        rig.eyes.w_hw as usize,
        rig.eyes.h as usize,
    );
    // Mouth cell.
    blit(
        (
            rig.base.0 + (frame[2] >> 2) as u16,
            rig.base.1 + frame[3] as u16,
        ),
        rig.mouth.dst,
        rig.mouth.w_hw as usize,
        rig.mouth.h as usize,
    );

    let rgba = decode_rgba8(&tim, palette)?;
    let w = tim.pixel_width();
    let h = window_h.min(tim.pixel_height());
    Ok(Sprite {
        width: w,
        height: h,
        rgba: rgba[..w * h * 4].to_vec(),
    })
}

/// Find the pack strip TIM that hosts a rig's window (cases 1..3).
pub fn pack_strip<'a>(tims: &'a [Tim], rig: &FaceRig) -> Option<&'a Tim> {
    tims.iter().find(|t| {
        t.mode == PixelMode::Bpp4
            && t.image.fb_x == rig.base.0
            && t.image.fb_y == rig.base.1
            && t.image.h as usize > rig.poses * 16 // the full 128-row strip, not the 64-row alternate
    })
}
