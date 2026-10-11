//! Battle **facial animation**: per-clip eye/mouth keyframe tracks + the
//! static `SCUS_942.54` face-frame stamp tables.
//!
//! Retail animates the party's battle faces with a per-frame VRAM stamp pass
//! (`FUN_8004C7B4`, called from the render-node update `FUN_80047430` with
//! the node's `+0x68` anim cursor - in integer keyframes - as the frame
//! counter, for party bands 0..2 and every character except Terra). Two
//! fields of the `0xAC`-byte action-entry header are per-clip facial
//! keyframe tracks:
//!
//! - entry `+0x8C`: **eye** track - four 3-byte records
//!   `[frame_id, start, end]`;
//! - entry `+0x98`: **mouth** track - same shape.
//!
//! The mid-battle **art clips** read the same offsets through a different
//! entry: `FUN_8004AD80` installs the art-bank record's embedded entry
//! (record `+0x24`) as the action-table slot `0x10`/`0x11` pointer, so a
//! playing art's tracks are the embedded entry's - record `+0xB0` (eyes) /
//! `+0xBC` (mouth), carried as
//! [`crate::battle_char_assembly::ArtAnimRecord::face`].
//!
//! A record is active while `start <= clip_frame <= end` (`end != 0`); its
//! `frame_id` selects a face frame from the static per-character tables in
//! `SCUS_942.54` (eye source x/y at `DAT_80076824/26` - eight frames per
//! character - mouth at `DAT_80076884/86` - six; rect sizes + destinations
//! at `DAT_800768CC..`, all banded by the per-party-slot origin deltas at
//! `DAT_800768FC/FE`). Each stamp is a libgpu `MoveImage` (wrapper
//! `FUN_80058490`) from the face-frame strip - parked inside the member's
//! texture band by the normal battle texture-pool uploads - onto the live
//! face rows of the band. When no record is active the **neutral frame 0**
//! is re-stamped instead (no active record on disc selects frame 0), and
//! character-record word `+0xF8` flag `0x2000` (ability-bitfield bit 45 -
//! the Rage passive) forces the neutral mouth frame.
//!
//! During the battle-end **victory-celebration window** (`DAT_8007BD71 ==
//! 0xFE` - the battle-end signal - with the victory sequencer
//! `FUN_8004E568` running: its phase halfword `ctx+0x6CE != 0` and the
//! celebration flag `DAT_8007BD60` bit `0x80` set, which the party-wipe
//! path explicitly clears) a member whose last-staged anim id
//! (`actor[+0x1DB]`) is a dynamic-art-slot id `0x11..=0x18` - at victory
//! time, the staged **win pose** - switches its mouth source to a
//! 16-record track from the static SCUS table at `0x80077E80`
//! ([`ArtMouthTables`], indexed `char*0x180 + staged_id*0x30 + i*3` with
//! the *raw* band byte), and the whole animator's frame counter - mouth
//! *and* eye pass - switches to the global victory counter `gp+0x9EA`
//! shifted right by one (the win-quote mouth flap).
//!
//! (The eye/mouth identity is pinned visually: in the catalogued battle
//! captures the `DAT_80076824` strip frames are the wide two-eye band -
//! frame 1 a narrowed blink - and the `DAT_80076884` frames the closed /
//! open mouth shapes.)
//!
//! See `docs/formats/battle-data-pack.md` § Facial animation tracks.
//!
//! This module carries the track parser ([`FaceTracks`]), the SCUS table
//! parsers ([`FaceFrameTables::from_scus`], [`ArtMouthTables::from_scus`])
//! and the per-frame stamp selection ([`FaceFrameTables::stamps`] /
//! [`FaceFrameTables::stamps_with_art_window`]); the engine applies the
//! returned [`FaceStamp`]s as VRAM-to-VRAM copies
//! (`legaia_tim::Vram::move_image`).

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
/// 3 slots - the animator only runs for battle slots `< 3`).
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

/// Records per **victory-window mouth override** track (`FUN_8004C7B4`'s
/// override loop runs `0xC + 4` = sixteen 3-byte records).
pub const ART_MOUTH_RECORD_COUNT: usize = 16;
/// First staged-anim band id the override table covers (`actor[+0x1DB]`
/// gate `0x10 < id < 0x19` - the dynamic-art-slot ids `0x11..=0x18`).
pub const ART_BAND_FIRST: u8 = 0x11;
/// Last staged-anim band id the override table covers (inclusive).
pub const ART_BAND_LAST: u8 = 0x18;
/// Bands per character (`ART_BAND_FIRST..=ART_BAND_LAST`; the char stride
/// `0x180` = exactly 8 bands x `0x30`, so consecutive characters' addressed
/// rows tile contiguously).
pub const ART_BAND_COUNT: usize = 8;
/// Base VA of the override-track table. Retail indexes it
/// `0x80077E80 + char*0x180 + staged_id*0x30 + i*3` with the **raw** band
/// byte (`0x11..=0x18`), so the lowest addressed row sits at `+0x330`
/// (`0x800781B0`) and the table's data block spans 24 rows x `0x30` bytes.
pub const ART_MOUTH_VA: u32 = 0x8007_7E80;

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
    /// bytes zero) - the clip shows the neutral face throughout. On disc
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
/// from the equipment sections instead - see
/// [`crate::battle_char_assembly::SwingAnimation::face`] - and the
/// dynamically-materialized art clips (staged ids `>= 0x10`) carry their
/// tracks in the art-bank records' embedded entries - see
/// [`crate::battle_char_assembly::ArtAnimRecord::face`].
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
/// in `legaia_game_tables::steal_table`, kept local so this module stands alone).
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
    /// Per-party-slot origin delta (`SLOT_DELTA_VA`) - the member band's
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
    /// stamps), `tracks` the playing clip's facial tracks (`None` - a clip
    /// with no registered tracks - behaves like a clip whose
    /// records are never active: the neutral face is re-stamped),
    /// `clip_frame` the playing clip's integer keyframe counter and
    /// `force_neutral_mouth` the character-record `0x2000` flag.
    ///
    /// Equivalent to [`Self::stamps_with_art_window`] with no override -
    /// the every-frame path outside the victory-celebration window.
    pub fn stamps(
        &self,
        char_index: usize,
        party_slot: usize,
        tracks: Option<&FaceTracks>,
        clip_frame: i16,
        force_neutral_mouth: bool,
    ) -> Vec<FaceStamp> {
        self.stamps_with_art_window(
            char_index,
            party_slot,
            tracks,
            clip_frame,
            None,
            force_neutral_mouth,
        )
    }

    /// [`Self::stamps`] with the **victory-window mouth override**: when
    /// `art_mouth` is `Some`, the mouth pass walks the override's sixteen
    /// records instead of the entry's four, and the frame counter for
    /// **both** passes (the eye records still come from the entry track)
    /// becomes the override's global counter shifted right by one - retail
    /// replaces the single counter local before the clamp, so the eye pass
    /// clocks on it too. Neutral fallbacks and the `force_neutral_mouth`
    /// flag behave exactly as in the base pass.
    ///
    /// Frame ids out of table range produce no stamp (retail data never
    /// holds one; the disc-gated census asserts that).
    // PORT: FUN_8004C7B4 - the per-frame facial animator: the
    // victory-window override branch (DAT_8007BD71 == 0xFE + victory
    // sequencer ctx+0x6CE != 0 + DAT_8007BD60 bit 0x80 + actor[+0x1DB] in
    // 0x11..=0x18 swaps the mouth source to the 0x80077E80 sixteen-record
    // track and the frame counter to gp[+0x9EA] >> 1), the frame-counter
    // clamp at 0xFE, the mouth pass (each active record stamps
    // `DAT_80076884` frame + slot delta -> `DAT_800768E4` dest), the
    // char-flag-0x2000 / no-active-record neutral mouth re-stamp, then the
    // four-record eye pass over entry+0x8C (`DAT_80076824` ->
    // `DAT_800768CC`) with its own neutral fallback. The gate conditions
    // themselves live with the caller (engine: battle ended in victory +
    // playing clip's staged id in the art-band range).
    pub fn stamps_with_art_window(
        &self,
        char_index: usize,
        party_slot: usize,
        tracks: Option<&FaceTracks>,
        clip_frame: i16,
        art_mouth: Option<ArtMouthOverride<'_>>,
        force_neutral_mouth: bool,
    ) -> Vec<FaceStamp> {
        if char_index >= FACE_CHAR_COUNT || party_slot >= FACE_SLOT_COUNT {
            return Vec::new();
        }
        // The override replaces the frame counter *before* the clamp
        // (retail: `local_40 = gp[0x9EA] >> 1` then `if (0xfe < local_40)
        // local_40 = 0xfe`).
        let f = match &art_mouth {
            Some(o) => ((o.counter >> 1) as i16).min(FRAME_COUNTER_CLAMP),
            None => clip_frame.min(FRAME_COUNTER_CLAMP),
        };
        let mut out = Vec::new();
        // Mouth pass: every active record stamps; flag 0x2000 (or no
        // active record) re-stamps the neutral frame 0 on top. Record
        // source: the override track in the victory window, the entry
        // track otherwise.
        let mouth_records: &[FaceTrackRecord] = match &art_mouth {
            Some(o) => o.track,
            None => tracks.map(|t| t.mouth.as_slice()).unwrap_or(&[]),
        };
        let mut mouth_active = 0usize;
        for r in mouth_records {
            if r.active_at(f)
                && let Some(s) = self.mouth_stamp(char_index, party_slot, r.frame as usize)
            {
                out.push(s);
                mouth_active += 1;
            }
        }
        if mouth_active == 0 || force_neutral_mouth {
            out.extend(self.mouth_stamp(char_index, party_slot, 0));
        }
        // Eye pass: same shape, neutral fallback only when no record is
        // active. Always the entry track (the override only swaps the
        // mouth source), but clocked on the same (possibly overridden)
        // counter.
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

/// One member's active victory-window mouth override: the per-(character,
/// staged-id) sixteen-record track ([`ArtMouthTables::track`]) plus the
/// raw global victory counter (`gp+0x9EA`; the stamp pass applies the
/// retail `>> 1` and the `0xFE` clamp).
#[derive(Debug, Clone, Copy)]
pub struct ArtMouthOverride<'a> {
    /// The override mouth track for the playing staged anim id.
    pub track: &'a [FaceTrackRecord; ART_MOUTH_RECORD_COUNT],
    /// Raw victory-sequence frame counter (retail resets it to 0 when the
    /// win pose is staged and advances it per frame).
    pub counter: u16,
}

/// The static `SCUS_942.54` victory-window mouth-override table at
/// [`ART_MOUTH_VA`]: per (character, staged-anim band `0x11..=0x18`) one
/// sixteen-record mouth track, same 3-byte `[frame, start, end]` shape as
/// the entry tracks. The `frame` ids select from the same per-character
/// mouth-frame table the entry tracks use ([`MOUTH_FRAME_COUNT`] frames).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtMouthTables {
    /// `tracks[char][band - ART_BAND_FIRST]` - the override mouth track
    /// for that character's staged id.
    pub tracks: [[[FaceTrackRecord; ART_MOUTH_RECORD_COUNT]; ART_BAND_COUNT]; FACE_CHAR_COUNT],
}

impl ArtMouthTables {
    /// Parse the override table out of a `SCUS_942.54` image. Returns
    /// `None` when the image isn't a PSX-EXE, the table block is out of
    /// range, or the data fails the plausibility gate.
    pub fn from_scus(scus: &[u8]) -> Option<Self> {
        let map = ExeMap::parse(scus)?;
        let mut t = Self {
            tracks: [[[FaceTrackRecord::default(); ART_MOUTH_RECORD_COUNT]; ART_BAND_COUNT];
                FACE_CHAR_COUNT],
        };
        for c in 0..FACE_CHAR_COUNT {
            for b in 0..ART_BAND_COUNT {
                let band = ART_BAND_FIRST as usize + b;
                for (i, rec) in t.tracks[c][b].iter_mut().enumerate() {
                    let o = map.off(ART_MOUTH_VA + (c * 0x180 + band * 0x30 + i * 3) as u32)?;
                    *rec = FaceTrackRecord {
                        frame: *scus.get(o)?,
                        start: *scus.get(o + 1)?,
                        end: *scus.get(o + 2)?,
                    };
                }
            }
        }
        t.plausible().then_some(t)
    }

    /// Sanity gate: every live record (`end != 0`) selects a non-neutral
    /// in-range mouth frame (`1..MOUTH_FRAME_COUNT`) with `start <= end`,
    /// every unused record is fully zero, and the table carries at least
    /// one live record. All three hold for the retail table and reject
    /// arbitrary data.
    fn plausible(&self) -> bool {
        let mut live = 0usize;
        for row in self.tracks.iter().flatten() {
            for r in row {
                if r.end == 0 {
                    if r.frame != 0 || r.start != 0 {
                        return false;
                    }
                } else {
                    if r.frame == 0 || r.frame as usize >= MOUTH_FRAME_COUNT || r.start > r.end {
                        return false;
                    }
                    live += 1;
                }
            }
        }
        live > 0
    }

    /// The override mouth track for `staged_id` (`actor[+0x1DB]`), or
    /// `None` when the id is outside the art-band window `0x11..=0x18`
    /// (the retail gate) or the character is out of range.
    pub fn track(
        &self,
        char_index: usize,
        staged_id: u8,
    ) -> Option<&[FaceTrackRecord; ART_MOUTH_RECORD_COUNT]> {
        if !(ART_BAND_FIRST..=ART_BAND_LAST).contains(&staged_id) {
            return None;
        }
        self.tracks
            .get(char_index)?
            .get((staged_id - ART_BAND_FIRST) as usize)
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
mod tests;
