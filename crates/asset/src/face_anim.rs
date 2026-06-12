//! Battle **facial animation**: per-clip eye/mouth keyframe tracks + the
//! static `SCUS_942.54` face-frame stamp tables.
//!
//! Retail animates the party's battle faces with a per-frame VRAM stamp pass
//! (`FUN_8004C7B4`, called from the render-node update `FUN_80047430` with
//! the node's `+0x68` anim cursor — in integer keyframes — as the frame
//! counter, for party bands 0..2 and every character except Terra). Two
//! fields of the `0xAC`-byte action-entry header are per-clip facial
//! keyframe tracks:
//!
//! - entry `+0x8C`: **eye** track — four 3-byte records
//!   `[frame_id, start, end]`;
//! - entry `+0x98`: **mouth** track — same shape.
//!
//! A record is active while `start <= clip_frame <= end` (`end != 0`); its
//! `frame_id` selects a face frame from the static per-character tables in
//! `SCUS_942.54` (eye source x/y at `DAT_80076824/26` — eight frames per
//! character — mouth at `DAT_80076884/86` — six; rect sizes + destinations
//! at `DAT_800768CC..`, all banded by the per-party-slot origin deltas at
//! `DAT_800768FC/FE`). Each stamp is a libgpu `MoveImage` (wrapper
//! `FUN_80058490`) from the face-frame strip — parked inside the member's
//! texture band by the normal battle texture-pool uploads — onto the live
//! face rows of the band. When no record is active the **neutral frame 0**
//! is re-stamped instead (no active record on disc selects frame 0), and
//! character-record word `+0xF8` flag `0x2000` (ability-bitfield bit 45 —
//! the Rage passive) forces the neutral mouth frame.
//!
//! (The eye/mouth identity is pinned visually: in the catalogued battle
//! captures the `DAT_80076824` strip frames are the wide two-eye band —
//! frame 1 a narrowed blink — and the `DAT_80076884` frames the closed /
//! open mouth shapes.)
//!
//! See `docs/formats/battle-data-pack.md` § Facial animation tracks.
//!
//! This module carries the track parser ([`FaceTracks`]), the SCUS table
//! parser ([`FaceFrameTables::from_scus`]) and the per-frame stamp selection
//! ([`FaceFrameTables::stamps`]); the engine applies the returned
//! [`FaceStamp`]s as VRAM-to-VRAM copies (`legaia_tim::Vram::move_image`).

use anyhow::{Result, bail};

/// Offset of the eye track inside a `0xAC`-byte action-entry header.
pub const EYE_TRACK_OFFSET: usize = 0x8C;
/// Offset of the mouth track inside a `0xAC`-byte action-entry header.
pub const MOUTH_TRACK_OFFSET: usize = 0x98;
/// Records per track (`FUN_8004C7B4` walks exactly four 3-byte records per
/// pass outside the dynamic-art window).
pub const TRACK_RECORD_COUNT: usize = 4;

/// Characters covered by the face tables: Vahn / Noa / Gala. Terra (char
/// index 3) is skipped by the retail animator (`FUN_80047430` gates the
/// call on `char != 3`) and the tables carry no fourth row.
pub const FACE_CHAR_COUNT: usize = 3;
/// Party bands covered by the per-slot origin deltas (`DAT_800768FC/FE`,
/// 3 slots — the animator only runs for battle slots `< 3`).
pub const FACE_SLOT_COUNT: usize = 3;
/// Eye frames per character (`DAT_80076824` char stride `0x20` = 8 x/y
/// pairs).
pub const EYE_FRAME_COUNT: usize = 8;
/// Mouth frames per character (`DAT_80076884` char stride `0x18` = 6 x/y
/// pairs).
pub const MOUTH_FRAME_COUNT: usize = 6;
/// The animator clamps its frame counter at `0xFE` before testing the
/// track intervals (`if (0xfe < (short)frame) frame = 0xfe`).
pub const FRAME_COUNTER_CLAMP: i16 = 0xFE;

/// `SCUS_942.54` virtual addresses of the face-frame tables (one contiguous
/// rodata block; see the module docs for the per-table shape).
pub const EYE_SRC_VA: u32 = 0x8007_6824;
/// Mouth-frame source x/y table VA.
pub const MOUTH_SRC_VA: u32 = 0x8007_6884;
/// Eye destination + rect-size table VA.
pub const EYE_GEO_VA: u32 = 0x8007_68CC;
/// Mouth destination + rect-size table VA.
pub const MOUTH_GEO_VA: u32 = 0x8007_68E4;
/// Per-party-slot origin delta table VA.
pub const SLOT_DELTA_VA: u32 = 0x8007_68FC;

/// One 3-byte facial keyframe record: face frame `frame` is shown while
/// the clip's frame counter sits inside `start..=end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FaceTrackRecord {
    /// Face-frame id (an index into the per-character source-x/y table;
    /// `0` is the neutral face and is never selected by an active record
    /// on disc).
    pub frame: u8,
    /// First clip frame (inclusive) the record is active on.
    pub start: u8,
    /// Last clip frame (inclusive). `0` marks the record unused.
    pub end: u8,
}

impl FaceTrackRecord {
    /// Retail activity predicate: `start <= f && f <= end && end != 0`
    /// (signed compares against the clamped frame counter).
    pub fn active_at(&self, clip_frame: i16) -> bool {
        self.end != 0 && self.start as i16 <= clip_frame && clip_frame <= self.end as i16
    }
}

/// The two facial keyframe tracks of one action entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FaceTracks {
    /// Eye track (entry `+0x8C`).
    pub eyes: [FaceTrackRecord; TRACK_RECORD_COUNT],
    /// Mouth track (entry `+0x98`).
    pub mouth: [FaceTrackRecord; TRACK_RECORD_COUNT],
}

impl FaceTracks {
    /// Read both tracks out of the `0xAC`-byte action-entry header at
    /// `entry_off` in `block` (a decoded record[0] image or equipment
    /// section payload). `None` when the header is out of range.
    pub fn from_entry(block: &[u8], entry_off: usize) -> Option<Self> {
        let read = |base: usize| -> Option<[FaceTrackRecord; TRACK_RECORD_COUNT]> {
            let mut out = [FaceTrackRecord::default(); TRACK_RECORD_COUNT];
            for (i, rec) in out.iter_mut().enumerate() {
                let o = base + i * 3;
                *rec = FaceTrackRecord {
                    frame: *block.get(o)?,
                    start: *block.get(o + 1)?,
                    end: *block.get(o + 2)?,
                };
            }
            Some(out)
        };
        Some(Self {
            eyes: read(entry_off + EYE_TRACK_OFFSET)?,
            mouth: read(entry_off + MOUTH_TRACK_OFFSET)?,
        })
    }

    /// `true` when no record of either track is ever active (all `end`
    /// bytes zero) — the clip shows the neutral face throughout. On disc
    /// that's the case for every **idle** entry: the party's resting faces
    /// are the re-stamped neutral frames, and the eye/mouth records live on
    /// the reaction / defeat / swing clips.
    pub fn is_empty(&self) -> bool {
        self.eyes
            .iter()
            .chain(self.mouth.iter())
            .all(|r| r.end == 0)
    }
}

/// Per-action-slot facial tracks of a player battle file's record[0]
/// (extraction PROT 863..866). Indexed by action slot like
/// [`crate::battle_char_assembly::battle_animations`]'s `action_id`; `None`
/// for unpopulated slots. The runtime swing slots `0xC..0xF` are spliced
/// from the equipment sections instead — see
/// [`crate::battle_char_assembly::SwingAnimation::face`].
pub fn battle_face_tracks(file: &[u8]) -> Result<Vec<Option<FaceTracks>>> {
    let block = crate::battle_char_assembly::decode_record0(file)?;
    let mut out = vec![None; crate::battle_char_assembly::ACTION_SLOT_COUNT];
    for (slot, tracks) in out.iter_mut().enumerate() {
        let o = slot * 4;
        if o + 4 > block.len() {
            break;
        }
        let entry_off = u32::from_le_bytes(block[o..o + 4].try_into().unwrap()) as usize;
        if entry_off == 0 || entry_off >= block.len() {
            continue;
        }
        *tracks = FaceTracks::from_entry(&block, entry_off);
    }
    Ok(out)
}

/// One face feature's destination + rect size for one character (the
/// `DAT_800768CC` / `DAT_800768E4` 8-byte rows): where in the texture band
/// the live face rows sit, and how big a frame is. The destination is
/// slot-relative (the per-slot origin delta is added at stamp time, exactly
/// like the source x/y).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaceGeo {
    /// Slot-relative destination x (VRAM halfwords).
    pub dest_x: i16,
    /// Slot-relative destination y.
    pub dest_y: i16,
    /// Stamp width in VRAM halfwords.
    pub w: u16,
    /// Stamp height in rows.
    pub h: u16,
}

/// One resolved face stamp: a `w x h`-halfword VRAM-to-VRAM `MoveImage`
/// from the face-frame strip onto the band's live face rows. All
/// coordinates are absolute VRAM halfword coordinates (the per-slot origin
/// delta is already applied).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaceStamp {
    /// Source rect origin (inside the member band's face-frame strip).
    pub src_x: u16,
    /// Source rect y.
    pub src_y: u16,
    /// Rect width in VRAM halfwords.
    pub w: u16,
    /// Rect height in rows.
    pub h: u16,
    /// Destination origin (the band's live face rows).
    pub dst_x: u16,
    /// Destination y.
    pub dst_y: u16,
}

/// PSX-EXE `t_addr` -> file-offset resolver (`SCUS_942.54` loads its data
/// segment at `t_addr` from file offset `0x800`; same shape as the resolver
/// in [`crate::steal_table`], kept local so this module stands alone).
struct ExeMap {
    t_addr: u32,
    t_size: u32,
}

impl ExeMap {
    fn parse(scus: &[u8]) -> Option<Self> {
        if scus.len() < 0x800 || &scus[0..8] != b"PS-X EXE" {
            return None;
        }
        let t_addr = u32::from_le_bytes(scus[0x18..0x1C].try_into().ok()?);
        let t_size = u32::from_le_bytes(scus[0x1C..0x20].try_into().ok()?);
        Some(Self { t_addr, t_size })
    }

    fn off(&self, va: u32) -> Option<usize> {
        if va < self.t_addr || va >= self.t_addr.checked_add(self.t_size)? {
            return None;
        }
        Some((va - self.t_addr) as usize + 0x800)
    }
}

fn read_i16(scus: &[u8], off: usize) -> Option<i16> {
    Some(i16::from_le_bytes([*scus.get(off)?, *scus.get(off + 1)?]))
}

/// The static `SCUS_942.54` face-frame tables (see the module docs for the
/// retail VAs). All x/y values are slot-relative; [`Self::stamps`] adds the
/// per-slot origin delta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaceFrameTables {
    /// Eye-frame source x/y per character (`EYE_SRC_VA`, char stride
    /// `0x20`). Unused trailing frames hold `(0, 0)`.
    pub eye_src: [[(i16, i16); EYE_FRAME_COUNT]; FACE_CHAR_COUNT],
    /// Mouth-frame source x/y per character (`MOUTH_SRC_VA`, char stride
    /// `0x18`).
    pub mouth_src: [[(i16, i16); MOUTH_FRAME_COUNT]; FACE_CHAR_COUNT],
    /// Eye destination + rect size per character (`EYE_GEO_VA`).
    pub eye_geo: [FaceGeo; FACE_CHAR_COUNT],
    /// Mouth destination + rect size per character (`MOUTH_GEO_VA`).
    pub mouth_geo: [FaceGeo; FACE_CHAR_COUNT],
    /// Per-party-slot origin delta (`SLOT_DELTA_VA`) — the member band's
    /// VRAM origin, added to both source and destination.
    pub slot_delta: [(i16, i16); FACE_SLOT_COUNT],
}

impl FaceFrameTables {
    /// Parse the face-frame tables out of a `SCUS_942.54` image. Returns
    /// `None` when the image isn't a PSX-EXE, the table block is out of
    /// range, or a rect fails the plausibility gate (zero / oversized
    /// stamp, or a frame landing outside the 1024x512 framebuffer for
    /// some slot delta).
    pub fn from_scus(scus: &[u8]) -> Option<Self> {
        let map = ExeMap::parse(scus)?;
        let geo_at = |va: u32| -> Option<FaceGeo> {
            let o = map.off(va)?;
            Some(FaceGeo {
                dest_x: read_i16(scus, o)?,
                dest_y: read_i16(scus, o + 2)?,
                w: read_i16(scus, o + 4)? as u16,
                h: read_i16(scus, o + 6)? as u16,
            })
        };
        let zero_geo = FaceGeo {
            dest_x: 0,
            dest_y: 0,
            w: 0,
            h: 0,
        };
        let mut t = Self {
            eye_src: [[(0, 0); EYE_FRAME_COUNT]; FACE_CHAR_COUNT],
            mouth_src: [[(0, 0); MOUTH_FRAME_COUNT]; FACE_CHAR_COUNT],
            eye_geo: [zero_geo; FACE_CHAR_COUNT],
            mouth_geo: [zero_geo; FACE_CHAR_COUNT],
            slot_delta: [(0, 0); FACE_SLOT_COUNT],
        };
        for c in 0..FACE_CHAR_COUNT {
            for f in 0..EYE_FRAME_COUNT {
                let o = map.off(EYE_SRC_VA + (c * 0x20 + f * 4) as u32)?;
                t.eye_src[c][f] = (read_i16(scus, o)?, read_i16(scus, o + 2)?);
            }
            for f in 0..MOUTH_FRAME_COUNT {
                let o = map.off(MOUTH_SRC_VA + (c * 0x18 + f * 4) as u32)?;
                t.mouth_src[c][f] = (read_i16(scus, o)?, read_i16(scus, o + 2)?);
            }
            t.eye_geo[c] = geo_at(EYE_GEO_VA + (c * 8) as u32)?;
            t.mouth_geo[c] = geo_at(MOUTH_GEO_VA + (c * 8) as u32)?;
        }
        for s in 0..FACE_SLOT_COUNT {
            let o = map.off(SLOT_DELTA_VA + (s * 4) as u32)?;
            t.slot_delta[s] = (read_i16(scus, o)?, read_i16(scus, o + 2)?);
        }
        t.plausible().then_some(t)
    }

    /// Sanity gate: every stamp rect is non-degenerate (`1..=0x40`
    /// halfwords/rows) and every frame source + destination lands inside
    /// the 1024x512 framebuffer for every slot delta.
    fn plausible(&self) -> bool {
        let in_fb = |x: i16, y: i16, w: u16, h: u16| -> bool {
            x >= 0 && y >= 0 && (x as u32 + w as u32) <= 1024 && (y as u32 + h as u32) <= 512
        };
        for c in 0..FACE_CHAR_COUNT {
            for geo in [&self.eye_geo[c], &self.mouth_geo[c]] {
                if !(1..=0x40).contains(&geo.w) || !(1..=0x40).contains(&geo.h) {
                    return false;
                }
            }
            for &(dx, dy) in &self.slot_delta {
                let eg = &self.eye_geo[c];
                let mg = &self.mouth_geo[c];
                if !in_fb(eg.dest_x + dx, eg.dest_y + dy, eg.w, eg.h)
                    || !in_fb(mg.dest_x + dx, mg.dest_y + dy, mg.w, mg.h)
                {
                    return false;
                }
                for &(sx, sy) in &self.eye_src[c] {
                    if !in_fb(sx + dx, sy + dy, eg.w, eg.h) {
                        return false;
                    }
                }
                for &(sx, sy) in &self.mouth_src[c] {
                    if !in_fb(sx + dx, sy + dy, mg.w, mg.h) {
                        return false;
                    }
                }
            }
        }
        true
    }

    fn eye_stamp(&self, c: usize, p: usize, frame: usize) -> Option<FaceStamp> {
        let &(sx, sy) = self.eye_src.get(c)?.get(frame)?;
        let geo = self.eye_geo[c];
        let (dx, dy) = self.slot_delta[p];
        Some(FaceStamp {
            src_x: (sx + dx) as u16,
            src_y: (sy + dy) as u16,
            w: geo.w,
            h: geo.h,
            dst_x: (geo.dest_x + dx) as u16,
            dst_y: (geo.dest_y + dy) as u16,
        })
    }

    fn mouth_stamp(&self, c: usize, p: usize, frame: usize) -> Option<FaceStamp> {
        let &(sx, sy) = self.mouth_src.get(c)?.get(frame)?;
        let geo = self.mouth_geo[c];
        let (dx, dy) = self.slot_delta[p];
        Some(FaceStamp {
            src_x: (sx + dx) as u16,
            src_y: (sy + dy) as u16,
            w: geo.w,
            h: geo.h,
            dst_x: (geo.dest_x + dx) as u16,
            dst_y: (geo.dest_y + dy) as u16,
        })
    }

    /// The per-frame stamp pass for one party member: which face frames the
    /// retail animator copies onto the band's live face rows this frame, in
    /// retail stamp order (the mouth pass runs first, then the eye pass;
    /// later stamps to the same rect win, exactly like the `MoveImage`
    /// sequence).
    ///
    /// `char_index` is the character (0 Vahn / 1 Noa / 2 Gala; Terra and
    /// out-of-range indices return no stamps, mirroring the retail skip),
    /// `party_slot` the present-party band ordinal (`>= 3` returns no
    /// stamps), `tracks` the playing clip's facial tracks (`None` — e.g. a
    /// dynamically-materialized art clip — behaves like a clip whose
    /// records are never active: the neutral face is re-stamped),
    /// `clip_frame` the playing clip's integer keyframe counter and
    /// `force_neutral_mouth` the character-record `0x2000` flag.
    ///
    /// Frame ids out of table range produce no stamp (retail data never
    /// holds one; the disc-gated census asserts that).
    // PORT: FUN_8004C7B4 - the per-frame facial animator: frame-counter
    // clamp at 0xFE, the four-record mouth pass over entry+0x98 (each
    // active record stamps `DAT_80076884` frame + slot delta ->
    // `DAT_800768E4` dest), the char-flag-0x2000 / no-active-record
    // neutral mouth re-stamp, then the four-record eye pass over
    // entry+0x8C (`DAT_80076824` -> `DAT_800768CC`) with its own neutral
    // fallback. The dynamic-art-window mouth-track override (the static
    // table at 0x80077E80 clocked by the global anim counter) is NOT
    // modelled; those clips pass `tracks = None` and show the neutral
    // face.
    pub fn stamps(
        &self,
        char_index: usize,
        party_slot: usize,
        tracks: Option<&FaceTracks>,
        clip_frame: i16,
        force_neutral_mouth: bool,
    ) -> Vec<FaceStamp> {
        if char_index >= FACE_CHAR_COUNT || party_slot >= FACE_SLOT_COUNT {
            return Vec::new();
        }
        let f = clip_frame.min(FRAME_COUNTER_CLAMP);
        let mut out = Vec::new();
        // Mouth pass: every active record stamps; flag 0x2000 (or no
        // active record) re-stamps the neutral frame 0 on top.
        let mut mouth_active = 0usize;
        if let Some(t) = tracks {
            for r in &t.mouth {
                if r.active_at(f)
                    && let Some(s) = self.mouth_stamp(char_index, party_slot, r.frame as usize)
                {
                    out.push(s);
                    mouth_active += 1;
                }
            }
        }
        if mouth_active == 0 || force_neutral_mouth {
            out.extend(self.mouth_stamp(char_index, party_slot, 0));
        }
        // Eye pass: same shape, neutral fallback only when no record is
        // active.
        let mut eye_active = 0usize;
        if let Some(t) = tracks {
            for r in &t.eyes {
                if r.active_at(f)
                    && let Some(s) = self.eye_stamp(char_index, party_slot, r.frame as usize)
                {
                    out.push(s);
                    eye_active += 1;
                }
            }
        }
        if eye_active == 0 {
            out.extend(self.eye_stamp(char_index, party_slot, 0));
        }
        out
    }
}

/// Read a `SCUS_942.54` image's face tables, bailing with context (the
/// `Option`-returning [`FaceFrameTables::from_scus`] is the primitive).
pub fn face_tables_from_scus(scus: &[u8]) -> Result<FaceFrameTables> {
    match FaceFrameTables::from_scus(scus) {
        Some(t) => Ok(t),
        None => bail!("SCUS image has no plausible face-frame tables"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic tables: band slot deltas at (512,256)/(640,256)/(768,256),
    /// char 0 eye frames in a strip at (32,128), mouth at (32,196).
    fn tables() -> FaceFrameTables {
        let mut t = FaceFrameTables {
            eye_src: [[(0, 0); EYE_FRAME_COUNT]; FACE_CHAR_COUNT],
            mouth_src: [[(0, 0); MOUTH_FRAME_COUNT]; FACE_CHAR_COUNT],
            eye_geo: [FaceGeo {
                dest_x: 0,
                dest_y: 16,
                w: 15,
                h: 17,
            }; FACE_CHAR_COUNT],
            mouth_geo: [FaceGeo {
                dest_x: 4,
                dest_y: 42,
                w: 7,
                h: 16,
            }; FACE_CHAR_COUNT],
            slot_delta: [(512, 256), (640, 256), (768, 256)],
        };
        for c in 0..FACE_CHAR_COUNT {
            for f in 0..EYE_FRAME_COUNT {
                t.eye_src[c][f] = (32, 128 + 17 * f as i16);
            }
            for f in 0..MOUTH_FRAME_COUNT {
                t.mouth_src[c][f] = (32, 196 + 16 * f as i16);
            }
        }
        t
    }

    fn blink_tracks() -> FaceTracks {
        let mut t = FaceTracks::default();
        // Eyes: frame 1 on clip frames 10..=12.
        t.eyes[0] = FaceTrackRecord {
            frame: 1,
            start: 10,
            end: 12,
        };
        // Mouth: frame 2 on clip frames 4..=6.
        t.mouth[0] = FaceTrackRecord {
            frame: 2,
            start: 4,
            end: 6,
        };
        t
    }

    #[test]
    fn neutral_face_when_no_record_is_active() {
        let t = tables();
        let tr = blink_tracks();
        let stamps = t.stamps(0, 0, Some(&tr), 0, false);
        // Neutral mouth + neutral eyes, both frame 0 (mouth pass first).
        assert_eq!(stamps.len(), 2);
        assert_eq!(
            stamps[0],
            FaceStamp {
                src_x: 544,
                src_y: 452,
                w: 7,
                h: 16,
                dst_x: 516,
                dst_y: 298,
            }
        );
        assert_eq!(
            stamps[1],
            FaceStamp {
                src_x: 544,
                src_y: 384,
                w: 15,
                h: 17,
                dst_x: 512,
                dst_y: 272,
            }
        );
    }

    #[test]
    fn active_records_pick_their_frames() {
        let t = tables();
        let tr = blink_tracks();
        // Frame 11: blink active (eye frame 1), mouth inactive -> neutral.
        let stamps = t.stamps(0, 0, Some(&tr), 11, false);
        assert_eq!(stamps.len(), 2);
        assert_eq!(stamps[0].src_y, 452, "mouth neutral");
        assert_eq!(stamps[1].src_y, 384 + 17, "eye frame 1 row");
        // Frame 5: mouth frame 2 active, eyes neutral.
        let stamps = t.stamps(0, 0, Some(&tr), 5, false);
        assert_eq!(stamps[0].src_y, 452 + 32, "mouth frame 2 row");
        assert_eq!(stamps[1].src_y, 384, "eye neutral");
    }

    #[test]
    fn interval_edges_match_retail_inclusive_compare() {
        let t = tables();
        let tr = blink_tracks();
        assert_eq!(t.stamps(0, 0, Some(&tr), 10, false)[1].src_y, 384 + 17);
        assert_eq!(t.stamps(0, 0, Some(&tr), 12, false)[1].src_y, 384 + 17);
        assert_eq!(t.stamps(0, 0, Some(&tr), 13, false)[1].src_y, 384);
        // An end byte of 0 marks the record unused even at frame 0.
        let mut dead = FaceTracks::default();
        dead.eyes[0] = FaceTrackRecord {
            frame: 1,
            start: 0,
            end: 0,
        };
        assert_eq!(t.stamps(0, 0, Some(&dead), 0, false)[1].src_y, 384);
    }

    #[test]
    fn frame_counter_clamps_at_0xfe() {
        let t = tables();
        let mut tr = FaceTracks::default();
        tr.eyes[0] = FaceTrackRecord {
            frame: 1,
            start: 0xF0,
            end: 0xFE,
        };
        // 0x1234 clamps to 0xFE -> the record is active.
        assert_eq!(t.stamps(0, 0, Some(&tr), 0x1234, false)[1].src_y, 384 + 17);
    }

    #[test]
    fn flag_0x2000_restamps_neutral_mouth_over_active_records() {
        let t = tables();
        let tr = blink_tracks();
        let stamps = t.stamps(0, 0, Some(&tr), 5, true);
        // Active mouth stamp, the forced neutral overwrite, then eyes.
        assert_eq!(stamps.len(), 3);
        assert_eq!(stamps[0].src_y, 452 + 32);
        assert_eq!(stamps[1].src_y, 452, "neutral wins (stamped last)");
        assert_eq!(stamps[1].dst_x, stamps[0].dst_x);
        assert_eq!(stamps[2].src_y, 384, "eye neutral");
    }

    #[test]
    fn terra_and_high_slots_get_no_stamps() {
        let t = tables();
        let tr = blink_tracks();
        assert!(t.stamps(3, 0, Some(&tr), 11, false).is_empty());
        assert!(t.stamps(0, 3, Some(&tr), 11, false).is_empty());
    }

    #[test]
    fn missing_tracks_show_the_neutral_face() {
        let t = tables();
        let stamps = t.stamps(1, 2, None, 50, false);
        assert_eq!(stamps.len(), 2);
        // Slot 2 delta applies to both src and dst.
        assert_eq!(stamps[1].src_x, 768 + 32);
        assert_eq!(stamps[1].dst_x, 768);
    }

    #[test]
    fn tracks_parse_from_an_entry_header() {
        let mut block = vec![0u8; 0x100];
        // Entry at 0x10: eye record 0 = [2, 4, 6], mouth record 1 = [1, 10, 12].
        block[0x10 + EYE_TRACK_OFFSET..0x10 + EYE_TRACK_OFFSET + 3].copy_from_slice(&[2, 4, 6]);
        block[0x10 + MOUTH_TRACK_OFFSET + 3..0x10 + MOUTH_TRACK_OFFSET + 6]
            .copy_from_slice(&[1, 10, 12]);
        let t = FaceTracks::from_entry(&block, 0x10).expect("in range");
        assert_eq!(
            t.eyes[0],
            FaceTrackRecord {
                frame: 2,
                start: 4,
                end: 6,
            }
        );
        assert_eq!(
            t.mouth[1],
            FaceTrackRecord {
                frame: 1,
                start: 10,
                end: 12,
            }
        );
        assert!(!t.is_empty());
        assert!(FaceTracks::from_entry(&block, 0x60).is_none(), "truncated");
        assert!(FaceTracks::default().is_empty());
    }
}
