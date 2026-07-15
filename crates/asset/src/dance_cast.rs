//! Dance-minigame **cast + choreography tables** - who dances, with which
//! mesh, playing which ANM clips.
//!
//! The dance overlay (PROT 0980) spawns its dancers in `FUN_801d0190`: a
//! per-mode **spawn table** names each floor slot's dancer *kind* and world
//! position, and a 5-record x `0x80`-byte **kind descriptor table** at overlay
//! VA [`KIND_TABLE_VA`] carries everything else - the dancer's mesh id, its
//! pre-game idle clip, its in-play dance-groove loop, and an 11-pair **move
//! clip array** the judge triggers. Both tables are baked rodata in the
//! overlay's static image, so they decode straight off the disc.
//!
//! ## Who the kinds are
//!
//! The clips resolve against the dance-hall **scene module** - CDNAME block
//! [`DANCE_SCENE_NAME`] (`other7`, raw TOC index `0x4CC` = extraction 1226..;
//! the module whose `efect.dat` is the dance SFX bank PROT 1228 and whose art
//! pack is PROT 1230). Its first-slot MOVE section is a 60-record ANM bundle
//! and its TMD pool holds the dancer NPCs; the kind descriptor's anim ids are
//! placement-space ids into that bundle (`record = id - 1`), pinned by the
//! bone-count partition being exact:
//!
//! | kind | model | rig bones | anim ids (records) | identity |
//! |---|---|---|---|---|
//! | 0 | global pool slot 1 | 10 | 6..18 (recs 5..17) | **Noa** - her resident field mesh (PROT 0874 §0 slot 1) |
//! | 1 | scene TMD 58 | 11 | 47..58 (recs 46..57) | **Mary** (face-strip rig 1, `(400,0)`; koin3's Mary shares her CLUTs) |
//! | 2 | scene TMD 62 | 12 | 33..46 (recs 32..45) | dancer (rig 2, strip `(416,0)`; koin3 twin model 67) |
//! | 3 | scene TMD 61 | 12 | 19..31 (recs 18..30) | dancer recolor (rig 3, strip `(432,0)`; koin3 twin model 66) |
//! | 4 | scene TMD 63 | 10 | 59/60 (recs 58/59) | **Disco King** (koin3 twin model 71; the setumei demo dancer) |
//!
//! Kind 0's model id is written to the spawn descriptor **without** the scene
//! TMD base (`hw(0x8007B6F8)`), so it indexes the resident global pool
//! (`DAT_8007C018`); every other kind's model gets the base added, so it is a
//! scene-pool index in the same space as MAN placement model bytes.
//!
//! ## Which clip plays when (`FUN_801d1358` + `FUN_801d1af4`)
//!
//! The per-dancer actor handler binds `desc+0x10` (idle) before the play
//! states and `desc+0x18` (the dance-groove loop) during them. On a judged
//! event the score routine returns a u32-word index into the pair array at
//! `desc+0x28` (each pair = `[anim | flags, rate]`); in pair units:
//!
//! * pair `0` / `1` - **miss reaction** (Square-lane / Circle-lane wrong press),
//! * pair `lane*2 + 2` / `lane*2 + 3` - **sequence-complete move** (Square /
//!   Circle direction chain closed on difficulty lane `lane`),
//! * pair `8 + lane` - **on-beat step** (the timing-button press).
//!
//! Anim word bit `0x200` sets the actor translucent for that clip; the rate
//! word is the cursor step in the shared clip driver's 1/16-frame units
//! (`FUN_800204F8`; rate 8 = half a frame per tick).
//!
//! Provenance: `overlay_dance_801d0190.txt` (spawner), `_801d1358.txt`
//! (per-dancer handler), `_801d1af4.txt` (judge returns). See
//! `docs/subsystems/minigame-dance.md` § Dancer bodies.

use serde::Serialize;

use crate::dance_chart::DANCE_OVERLAY_BASE_VA;

/// CDNAME label of the dance-hall scene module the clips + dancer meshes live
/// in (`#define other7 1228`; extraction block 1226..). The koin3 town scene
/// places the same NPCs on its field-mode dance floor with sibling bundles.
pub const DANCE_SCENE_NAME: &str = "other7";

/// Overlay VA of the 5-kind x `0x80`-byte dancer descriptor table.
pub const KIND_TABLE_VA: u32 = 0x801D_4E1C;
/// Kind descriptor count (0 = Noa, 1..3 = the competitor dancers, 4 = the
/// Disco King demo dancer).
pub const KIND_COUNT: usize = 5;
/// Kind descriptor stride.
pub const KIND_STRIDE: usize = 0x80;
/// Move-clip pairs per kind descriptor (`desc+0x28..+0x80`).
pub const MOVE_PAIRS: usize = 11;

/// Overlay VA of the mode-0 (yosenn / qualifier) spawn table; also the mode-2
/// (setumei) table, truncated to its first record.
pub const SPAWN_QUALIFIER_VA: u32 = 0x801D_4D5C;
/// Overlay VA of the mode-1 (hosenn / finals) spawn table.
pub const SPAWN_FINALS_VA: u32 = 0x801D_4D8C;
/// Overlay VA of the mode-3 (asobi / free-play) spawn table (6 dancers).
pub const SPAWN_FREEPLAY_VA: u32 = 0x801D_4DBC;

/// Move-pair index of the Square-lane miss reaction.
pub const MOVE_MISS_SQUARE: usize = 0;
/// Move-pair index of the Circle-lane miss reaction.
pub const MOVE_MISS_CIRCLE: usize = 1;

/// Move-pair index of a completed direction sequence on difficulty `lane`
/// (`circle = false` for the Square chain).
pub fn move_sequence_pair(lane: usize, circle: bool) -> usize {
    lane * 2 + 2 + usize::from(circle)
}

/// Move-pair index of the on-beat timing-button step on difficulty `lane`.
pub fn move_beat_pair(lane: usize) -> usize {
    8 + lane
}

/// One dancer-clip reference out of a kind descriptor.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct DanceClip {
    /// Placement-space anim id into the scene MOVE bundle (`record = id - 1`;
    /// `0` = none).
    pub anim_id: u16,
    /// Actor drawn translucent while this clip plays (anim word bit `0x200`).
    pub translucent: bool,
    /// Cursor step in 1/16-frame units per tick (`FUN_800204F8`).
    pub rate: u16,
}

impl DanceClip {
    fn from_pair(anim_word: u32, rate_word: u32) -> Self {
        Self {
            anim_id: (anim_word & 0x1FF) as u16,
            translucent: anim_word & 0x200 != 0,
            rate: rate_word as u16,
        }
    }

    /// ANM bundle record index this clip plays (`None` when the id is 0).
    pub fn record_index(&self) -> Option<usize> {
        (self.anim_id > 0).then(|| self.anim_id as usize - 1)
    }
}

/// One dancer kind's full descriptor.
#[derive(Debug, Clone, Serialize)]
pub struct DanceKind {
    /// Mesh id. Kind 0: resident global TMD pool slot (1 = Noa's field mesh).
    /// Kinds 1..: scene TMD-pool index (MAN model-byte space).
    pub model: u16,
    /// Default floor position `(x, y, z)` (`desc+0x00..+0x0C`).
    pub home: [i32; 3],
    /// Pre-game idle clip (`desc+0x10`).
    pub idle: DanceClip,
    /// In-play dance-groove loop (`desc+0x18`).
    pub dance: DanceClip,
    /// Third header clip slot (`desc+0x20`; consumer untraced).
    pub alt: DanceClip,
    /// The judge-triggered move clips (`desc+0x28`, [`MOVE_PAIRS`] pairs) -
    /// see the module docs for the pair-index semantics.
    pub moves: Vec<DanceClip>,
}

/// One spawn-table record: which kind stands where.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct DanceSpawn {
    /// Kind descriptor index (`0` = the human player, Noa).
    pub kind: u32,
    /// World position (y is the floor offset, `-128` on the retail floor).
    pub x: i16,
    pub y: i16,
    pub z: i16,
}

/// The decoded cast tables.
#[derive(Debug, Clone, Serialize)]
pub struct DanceCast {
    /// The five kind descriptors, index = kind id (= face-stamp rig id for
    /// kinds 0..=3).
    pub kinds: Vec<DanceKind>,
    /// Mode-0 (yosenn / qualifier) floor: Noa + kinds 2 and 3. Mode 2
    /// (setumei) spawns only this table's first record.
    pub qualifier: Vec<DanceSpawn>,
    /// Mode-1 (hosenn / finals) floor: Noa + Mary (kind 1) + kind 2.
    pub finals: Vec<DanceSpawn>,
    /// Mode-3 (asobi / free-play) floor: six dancers (kind 3 twice + the
    /// Disco King).
    pub free_play: Vec<DanceSpawn>,
}

fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(off..off + 4)?.try_into().ok()?))
}

fn spawns_at(overlay: &[u8], va: u32, count: usize) -> Option<Vec<DanceSpawn>> {
    let base = (va - DANCE_OVERLAY_BASE_VA) as usize;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let o = base + i * 0x10;
        let kind = u32_at(overlay, o)?;
        if kind as usize >= KIND_COUNT {
            return None;
        }
        out.push(DanceSpawn {
            kind,
            x: u32_at(overlay, o + 4)? as i16,
            y: u32_at(overlay, o + 8)? as i16,
            z: u32_at(overlay, o + 0xC)? as i16,
        });
    }
    Some(out)
}

/// Parse the cast tables out of the as-loaded dance overlay image (PROT 0980).
/// `None` when the buffer is too short or a table fails its sanity bounds.
pub fn parse(overlay: &[u8]) -> Option<DanceCast> {
    let base = (KIND_TABLE_VA - DANCE_OVERLAY_BASE_VA) as usize;
    if overlay.len() < base + KIND_COUNT * KIND_STRIDE {
        return None;
    }
    let mut kinds = Vec::with_capacity(KIND_COUNT);
    for k in 0..KIND_COUNT {
        let d = &overlay[base + k * KIND_STRIDE..base + (k + 1) * KIND_STRIDE];
        let clip_at = |off: usize| -> Option<DanceClip> {
            Some(DanceClip::from_pair(u32_at(d, off)?, u32_at(d, off + 4)?))
        };
        let idle = clip_at(0x10)?;
        let dance = clip_at(0x18)?;
        // The judge only ever triggers real clips; a zero anim id anywhere in
        // the header would mean we are not looking at the descriptor table.
        if idle.anim_id == 0 || dance.anim_id == 0 {
            return None;
        }
        let mut moves = Vec::with_capacity(MOVE_PAIRS);
        for p in 0..MOVE_PAIRS {
            moves.push(clip_at(0x28 + p * 8)?);
        }
        kinds.push(DanceKind {
            model: u32_at(d, 0xC)? as u16,
            home: [
                u32_at(d, 0)? as i32,
                u32_at(d, 4)? as i32,
                u32_at(d, 8)? as i32,
            ],
            idle,
            dance,
            alt: clip_at(0x20)?,
            moves,
        });
    }
    Some(DanceCast {
        kinds,
        qualifier: spawns_at(overlay, SPAWN_QUALIFIER_VA, 3)?,
        finals: spawns_at(overlay, SPAWN_FINALS_VA, 3)?,
        free_play: spawns_at(overlay, SPAWN_FREEPLAY_VA, 6)?,
    })
}
