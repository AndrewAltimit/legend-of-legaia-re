//! The dance tick's camera keyframe track (`FUN_801CF470`,
//! `0x801CF51C..0x801CF7D8`): the pose records and key tables out of the
//! dance overlay, the entry-built ease table, and the two counters the tick
//! steps. `engine-core`'s `dance_venue` re-exports it and turns a pose into
//! the venue's field camera.

use super::*;

/// Overlay VA of the camera pose records: 8-byte `[i16 x, i16 y, i16 z,
/// i16 pad]` records read in pairs, record `2k` the angle trio and record
/// `2k + 1` the eye-space trio of pose `k` (`addiu t0,v0,0x43a0` at
/// `0x801CF67C`). Pose 0 is the entry's own stores.
pub const DANCE_CAMERA_POSES_VA: u32 = 0x801D_43A0;

/// Overlay VA of the qualifier's key table - `u32` pose indices, one per key
/// (`addiu a0,a0,0x4440` at `0x801CF604`, taken while `DAT_801D514C == 0`).
pub const DANCE_CAMERA_KEYS_QUALIFIER_VA: u32 = 0x801D_4440;

/// Overlay VA of the key table every other mode reads (`addiu a0,a0,0x4488`
/// at `0x801CF60C`).
pub const DANCE_CAMERA_KEYS_VA: u32 = 0x801D_4488;

/// Frames one key segment lasts: the timer `DAT_801D533C` reloads `0x151`
/// when it goes negative (`li v1,0x151` at `0x801CF540`).
pub const DANCE_CAMERA_SEGMENT: i32 = 0x151;

/// The key index wraps back to `1` - never `0` - on reaching this
/// (`slti v0,v0,0xd` at `0x801CF550` and `0x801CF574`), so key 0 plays only
/// on the first pass: the track opens on pose `tbl[0]` and then cycles keys
/// `1..=12`.
pub const DANCE_CAMERA_KEY_WRAP: i32 = 13;

/// Entries of the ease table the entry builds at `DAT_801D583C`
/// (`FUN_801CEF54`, `0x801CEF98..0x801CF054`): a 1024-step half-cosine
/// rise from `0` to `0x1000`, then 32 entries held at `0x1000`.
pub const DANCE_CAMERA_EASE_LEN: usize = 0x420;

/// The ease table `FUN_801CEF54` builds into BSS at `DAT_801D583C` out of
/// the SCUS sine table (`*_DAT_8007B81C`, 4096 steps a turn), stepping it two
/// entries at a time:
///
/// - `ease[i] = (sin[0xC00 + 2i] + 0x1000) / 2` for `i < 0x200`
///   (`lh v0,0x1800(a0)` over a 4-byte stride, `0x801CEFAC..0x801CEFD4`);
/// - `ease[0x200 + i] = sin[2i] / 2 + 0x800` for `i < 0x200`
///   (`0x801CEFF4..0x801CF024`);
/// - `ease[0x400..0x420] = 0x1000` (`0x801CF040..0x801CF050`).
///
/// Both halvings truncate toward zero (`srl 31` / `addu` / `sra 1`).
pub fn dance_camera_ease_table() -> Vec<i16> {
    use legaia_engine_vm::battle_action::motion::sin12;
    let half = |v: i32| ((v + ((v as u32 >> 31) as i32)) >> 1) as i16;
    let mut t = Vec::with_capacity(DANCE_CAMERA_EASE_LEN);
    for i in 0..0x200u16 {
        t.push(half(i32::from(sin12(0xC00 + 2 * i)) + 0x1000));
    }
    for i in 0..0x200u16 {
        t.push(half(i32::from(sin12(2 * i))) + 0x800);
    }
    t.resize(DANCE_CAMERA_EASE_LEN, 0x1000);
    t
}

/// One camera pose: the `_DAT_8007B790` angle trio and the `0x800840B8`
/// eye-space trio the track writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DanceCameraPose {
    pub angles: [i16; 3],
    pub eye: [i32; 3],
}

/// The dance tick's camera keyframe track: the key tables and pose records
/// out of the overlay image, the entry-built ease table, and the two
/// counters the entry seeds (`sw zero,0x533c` / `sw v0(-1),0x5338` at
/// `0x801CF348..0x801CF350`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DanceCameraTrack {
    /// Every pose record the key tables reach, `[x, y, z]`.
    records: Vec<[i16; 3]>,
    /// `[qualifier, other]` key tables, keys `0..13`.
    keys: [[u32; DANCE_CAMERA_KEY_WRAP as usize]; 2],
    ease: Vec<i16>,
    /// `DAT_801D533C`.
    timer: i32,
    /// `DAT_801D5338`.
    key: i32,
}

impl DanceCameraTrack {
    /// Parse the track out of the dance overlay (PROT 0980) in its loaded
    /// form (file offset = VA - `0x801CE818`). `None` when a table or a
    /// record it names falls outside the image.
    pub fn from_overlay(overlay: &[u8]) -> Option<Self> {
        let base = legaia_asset::dance_chart::DANCE_OVERLAY_BASE_VA;
        let word = |va: u32| -> Option<u32> {
            let o = va.checked_sub(base)? as usize;
            Some(u32::from_le_bytes(overlay.get(o..o + 4)?.try_into().ok()?))
        };
        let mut keys = [[0u32; DANCE_CAMERA_KEY_WRAP as usize]; 2];
        for (t, va) in [DANCE_CAMERA_KEYS_QUALIFIER_VA, DANCE_CAMERA_KEYS_VA]
            .into_iter()
            .enumerate()
        {
            for (k, slot) in keys[t].iter_mut().enumerate() {
                *slot = word(va + 4 * k as u32)?;
            }
        }
        let max_pose = keys.iter().flatten().copied().max()?;
        // A pose index past this is not a table the retail track reads.
        if max_pose > 0x100 {
            return None;
        }
        let n = 2 * (max_pose as usize + 1);
        let o = (DANCE_CAMERA_POSES_VA - base) as usize;
        let bytes = overlay.get(o..o + n * 8)?;
        let h = |r: &[u8], i: usize| i16::from_le_bytes([r[i], r[i + 1]]);
        let records = bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|r| [h(r, 0), h(r, 2), h(r, 4)])
            .collect();
        Some(Self {
            records,
            keys,
            ease: dance_camera_ease_table(),
            timer: 0,
            key: -1,
        })
    }

    /// The current key (`DAT_801D5338`) and segment timer (`DAT_801D533C`).
    pub fn counters(&self) -> (i32, i32) {
        (self.key, self.timer)
    }

    /// The pose key `key` of `mode`'s table names, un-interpolated - what the
    /// track sits on exactly at the end of the segment that eases into it.
    pub fn key_pose(&self, mode: DanceMode, key: usize) -> Option<DanceCameraPose> {
        let table = &self.keys[usize::from(mode != DanceMode::Qualifier)];
        let (a, e) = self.record_pose(*table.get(key)?)?;
        Some(DanceCameraPose {
            angles: a,
            eye: e.map(i32::from),
        })
    }

    /// Pose `k`'s angle and eye records, as `lh` reads them.
    fn record_pose(&self, k: u32) -> Option<([i16; 3], [i16; 3])> {
        let a = *self.records.get(2 * k as usize)?;
        let e = *self.records.get(2 * k as usize + 1)?;
        Some((a, e))
    }

    /// The pose the counters stand on, without advancing them. Before the
    /// first tick (the entry's `-1` key) that is pose 0 - the entry's stores.
    pub fn pose(&self, mode: DanceMode) -> Option<DanceCameraPose> {
        let table = &self.keys[usize::from(mode != DanceMode::Qualifier)];
        if self.key < 0 {
            let (a, e) = self.record_pose(table[0])?;
            return Some(DanceCameraPose {
                angles: a,
                eye: e.map(i32::from),
            });
        }
        let s2 = self.key as usize;
        let s1 = if self.key + 1 >= DANCE_CAMERA_KEY_WRAP {
            1
        } else {
            s2 + 1
        };
        let (a_ang, a_eye) = self.record_pose(*table.get(s2)?)?;
        let (b_ang, b_eye) = self.record_pose(*table.get(s1)?)?;
        // `((0x151 - timer) << 10) / 0x151` - the multiply by `0x309E0185`
        // and `mfhi >> 6` at `0x801CF638..0x801CF664` is that division.
        let step = ((DANCE_CAMERA_SEGMENT - self.timer) << 10) / DANCE_CAMERA_SEGMENT;
        let w = i32::from(*self.ease.get(step.clamp(0, 0x41F) as usize)?);
        // `a + (b - a) * w / 0x1000`, the product rounded toward zero
        // (`bgez` / `addiu 0xfff` / `sra 0xc`).
        let lerp = |a: i16, b: i16| i32::from(a) + (i32::from(b) - i32::from(a)) * w / 0x1000;
        Some(DanceCameraPose {
            // `lhu` + delta, `sh`: the angle wraps as a halfword.
            angles: std::array::from_fn(|i| lerp(a_ang[i], b_ang[i]) as i16),
            eye: std::array::from_fn(|i| lerp(a_eye[i], b_eye[i])),
        })
    }

    /// One dance tick of the track: the gate, the segment timer, the key
    /// step, then the interpolated pose `FUN_801CF470` writes into
    /// `_DAT_8007B790..94` and `0x800840B8..C0`.
    ///
    /// The gate is the tick's: the dance state `DAT_801D5334` non-zero (every
    /// state the entry leaves - the port's staged dance), the dev counter
    /// `_DAT_8007B6D0` zero (always, off the debug menu), and the mode not
    /// the how-to demo (`li v0,0x2; beq v1,v0` at `0x801CF510`). `None` when
    /// the gate holds the camera - the how-to demo keeps the entry's pose.
    ///
    /// PORT: FUN_801cf470 (the camera keyframe block, `0x801CF51C..0x801CF7D8`)
    pub fn tick(&mut self, mode: DanceMode, frame_delta: u8) -> Option<DanceCameraPose> {
        if mode == DanceMode::HowTo {
            return None;
        }
        self.timer -= i32::from(frame_delta);
        if self.timer < 0 {
            self.timer = DANCE_CAMERA_SEGMENT;
            self.key += 1;
            if self.key >= DANCE_CAMERA_KEY_WRAP {
                self.key = 1;
            }
        }
        self.pose(mode)
    }
}
