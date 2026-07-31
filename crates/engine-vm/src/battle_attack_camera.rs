//! Per-art attack-camera framing.
//!
//! PORT: FUN_801D71B8
//!
//! The per-frame routine that frames the camera on a party member's Arts swing.
//! It runs only while an Attack action is executing, resolves a **per-character,
//! per-art** script arm, and hands a rotation / translation / look-at triple to
//! the camera setter. One of the hottest bodies in the attack chain - see
//! [`docs/reference/functions/battle.md`](../../docs/reference/functions/battle.md).
//!
//! Transcribed from the DISASSEMBLY in
//! `ghidra/scripts/funcs/overlay_battle_action_801d71b8.txt` (1081
//! instructions).
//!
//! ## What this module ports
//!
//! * [`attack_camera_gate`] - the four-way entry test.
//! * [`CameraPose`] + [`CameraPose::seed`] - the local triple the arms mutate,
//!   including the negations that make the look-at the *inverse* of the actor's
//!   position and facing.
//! * [`character_arm`] / [`art_arm`] - the two dispatch layers: character id
//!   `1`/`2`/`3`, then art id through **that character's own** jump table.
//! * [`anim_push`] - the animation-frame-driven camera push the arms share,
//!   with its `0x60` bias, `<< 4` scale and `0x100` clamp.
//! * [`ArmTrack`] / [`arm_tracks`] / [`apply_track`] - which rows of the disc
//!   track table each arm folds into the pose, and into which component.
//!
//! ## Three jump tables, not one
//!
//! The earlier reading - "art id `0x1A..=0x2A` through a 17-slot jump table" -
//! was one character's table generalised to all three. Each character branch
//! bounds and indexes its own:
//!
//! | character id | bound | jump table | live arms |
//! |---|---|---|---|
//! | `1` | `0x11` (`0x801D72E0`) | `0x801CEA88` | 4 |
//! | `2` | `0x14` (`0x801D76C4`) | `0x801CEAD0` | 5 |
//! | `3` | `0x11` (`0x801D7B24`) | `0x801CEB20` | 4 |
//!
//! So character `2` reaches art ids up to `0x2D`, three past the range the
//! other two accept, and most slots in every table are the shared epilogue
//! `0x801D828C`. Thirteen arm bodies exist, not seventeen; several art ids
//! share one (`0x1E`/`0x2A` for characters `1` and `3`, `0x1D`/`0x2C` and
//! `0x1E`/`0x2D` for character `2`). See [`ART_JUMP_TABLES`].
//!
//! ## The track table
//!
//! Each arm reads one or more halfwords out of the disc table at
//! `0x801F4E10` - twenty rows of two, addressed `base + row*4 + cursor*2`
//! with `cursor = ctx[+0x26D]` - and adds them into the pose. Parser:
//! [`legaia_asset::battle_attack_camera_table`]. Every row is used and every
//! use is a fold into one pose component; [`ARM_TRACKS`] is the whole map,
//! taken site by site from the dump.
//!
//! ## What it still does not port
//!
//! The per-arm *literals and ramps* around the folds - each arm also adds its
//! own constants and multiples of the context counters `ctx[+0x26E]` (a
//! `0..=0xC8` ramp) and `ctx[+0x87C]`, under its own animation-frame
//! thresholds. Those two counters are battle-context state the engine does
//! not model, so the arms are ported as far as the track folds and the shared
//! [`anim_push`] and no further; the retail address of each remaining block is
//! on its [`ArmTracks`] row.
//!
//! # NOT WIRED
//!
//! The shared battle camera ([`crate::battle_cam_script`]) now has an Action
//! phase and the track table is parsed, but the per-art channel is not
//! connected: that camera holds no `ctx[+0x26D]` phase cursor and no live
//! animation-frame counter for [`anim_push`] to read, and the engine's battle
//! actors do not expose `actor[+0x21B]` (the arm sub-selector) or
//! `actor[+0x22C][+0x68]`. Wiring means those three actor channels, not more
//! camera work.

/// Action category the gate demands (`actor[+0x1DE] == 3`, Attack).
pub const CATEGORY_ATTACK: u8 = 3;
/// Battle-phase byte the gate demands (`ctx[+6] == 0xFF`).
pub const PHASE_ACTIVE: u8 = 0xFF;
/// Party slots this routine frames (`ctx[+0x13] < 3`).
pub const PARTY_SLOTS: u8 = 3;
/// First art id with a camera arm (`actor[+0x1DB] - 0x1A`, table index `0`).
pub const FIRST_ART: u8 = 0x1A;
/// Base pitch/yaw magnitude the pose seeds two of its slots with (`0x400`).
pub const POSE_SEED_ANGLE: i16 = 0x400;
/// Animation-frame threshold below which the push is skipped (`slti a0, 0x61`).
pub const ANIM_PUSH_FLOOR: i16 = 0x61;
/// Bias subtracted from the animation frame before scaling (`addiu v0, -0x60`).
pub const ANIM_PUSH_BIAS: i16 = 0x60;
/// Ceiling on the scaled push (`sltiu a0, 0x101`, so the cap is `0x100`).
pub const ANIM_PUSH_CAP: i32 = 0x100;

/// The four-way entry test (`0x801D71B8..0x801D722C`).
///
/// Named for its module rather than `gate`: a bare `gate` collides with the
/// local bindings and struct fields of that name all over the tree, and the
/// reachability analysis's free-function edge is by name, so the short name
/// made this whole module read as live from half a dozen unrelated callers.
///
/// All four conditions are `bne`/`beq` straight to the epilogue, so any one
/// failing makes the whole frame a no-op:
///
/// 1. the active actor's target slot holds a live actor (`+0x14C != 0`);
/// 2. the active actor's category is Attack (`+0x1DE == 3`);
/// 3. the battle phase byte `ctx[+6]` is `0xFF`;
/// 4. the active slot is a party slot (`ctx[+0x13] < 3`) whose participant id
///    is `1`, `2` or `3` - any other id, including a monster slot, exits.
pub const fn attack_camera_gate(target_hp: u16, category: u8, phase: u8, active_slot: u8) -> bool {
    target_hp != 0
        && category == CATEGORY_ATTACK
        && phase == PHASE_ACTIVE
        && active_slot < PARTY_SLOTS
}

/// Which character's camera script runs, keyed on the participant-id byte
/// `DAT_8007BD10[active_slot]`.
///
/// The dispatch is a three-way compare with no default arm: `2` first, then
/// `< 3` splitting off `1`, then `== 3`. Everything else falls to the epilogue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CharacterArm {
    /// Participant id `1`.
    One,
    /// Participant id `2`.
    Two,
    /// Participant id `3`.
    Three,
}

/// Resolve [`CharacterArm`], or `None` for an id with no camera script.
pub const fn character_arm(participant_id: u8) -> Option<CharacterArm> {
    match participant_id {
        1 => Some(CharacterArm::One),
        2 => Some(CharacterArm::Two),
        3 => Some(CharacterArm::Three),
        _ => None,
    }
}

/// The three per-character art jump tables, in `CharacterArm` order
/// (`0x801CEA88`, `0x801CEAD0`, `0x801CEB20`), read off the battle-action
/// overlay image. Index = `art_id - `[`FIRST_ART`]; `None` is retail's shared
/// epilogue slot `0x801D828C`, and the entry VA is the arm's own body.
///
/// The table **lengths** are the bounds each character branch tests
/// (`sltiu v0,v1,0x11` / `0x14` / `0x11`), which is why character `2` accepts
/// three art ids the other two reject.
pub const ART_JUMP_TABLES: [&[Option<u32>]; 3] = [
    // Character 1 - `0x801CEA88`, bound 0x11.
    &[
        Some(0x801D_7308), // 0x1A
        None,              // 0x1B
        Some(0x801D_7650), // 0x1C
        Some(0x801D_7568), // 0x1D
        Some(0x801D_74A8), // 0x1E
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(0x801D_74A8), // 0x2A - shares the 0x1E body
    ],
    // Character 2 - `0x801CEAD0`, bound 0x14.
    &[
        Some(0x801D_76EC), // 0x1A
        None,              // 0x1B
        None,              // 0x1C
        Some(0x801D_78F0), // 0x1D
        Some(0x801D_797C), // 0x1E
        Some(0x801D_79F8), // 0x1F
        Some(0x801D_7870), // 0x20
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,              // 0x2A
        None,              // 0x2B
        Some(0x801D_78F0), // 0x2C - shares the 0x1D body
        Some(0x801D_797C), // 0x2D - shares the 0x1E body
    ],
    // Character 3 - `0x801CEB20`, bound 0x11.
    &[
        Some(0x801D_7B4C), // 0x1A
        None,              // 0x1B
        Some(0x801D_7D7C), // 0x1C
        Some(0x801D_7EA0), // 0x1D
        Some(0x801D_81FC), // 0x1E
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(0x801D_81FC), // 0x2A - shares the 0x1E body
    ],
];

/// The arm entry VA this character's art id dispatches to, or `None` for a
/// slot that lands on the shared epilogue (and for an id outside the
/// character's own bound).
///
/// Retail biases by [`FIRST_ART`] and bounds with an **unsigned** compare, so
/// an art id below the bias wraps to a huge index and is rejected by the same
/// test that rejects one above the range.
pub fn art_arm(character: CharacterArm, art_id: u8) -> Option<u32> {
    let table = ART_JUMP_TABLES[character as usize];
    let index = art_id.wrapping_sub(FIRST_ART) as usize;
    if art_id < FIRST_ART || index >= table.len() {
        return None;
    }
    table[index]
}

/// Which pose component an [`ArmTrack`] fold lands in - the stack slot the
/// arm's `sh` writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoseSlot {
    /// `sp+0x10` - [`CameraPose::rot`]`[0]`.
    Pitch,
    /// `sp+0x12` - [`CameraPose::rot`]`[1]`.
    Yaw,
    /// `sp+0x18` - [`CameraPose::dist`]`[0]`.
    Dist0,
    /// `sp+0x1C` - [`CameraPose::dist`]`[2]`.
    Dist2,
}

/// One arm's use of one track-table row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArmTrack {
    /// Row index into [`legaia_asset::battle_attack_camera_table`] (retail's
    /// `lhu` displacement divided by four).
    pub row: usize,
    /// The pose component the row is folded into.
    pub slot: PoseSlot,
    /// Whether the fold subtracts instead of adding (`subu` at the store).
    pub subtract: bool,
    /// The `lhu`'s own address, so a reader can go straight to the block.
    pub site_va: u32,
}

/// The tracks one arm body reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArmTracks {
    /// The arm's entry VA - the value [`art_arm`] returns.
    pub entry_va: u32,
    /// Its row folds, in the order the arm performs them.
    pub tracks: &'static [ArmTrack],
}

/// One additive fold, spelled out so the map below reads as a table.
/// Deliberately not a one-letter name: the reachability analysis matches free
/// functions by name, and a `fn t` picks up an edge from every `t(..)` call in
/// the workspace.
const fn arm_track_fold(row: usize, slot: PoseSlot, site_va: u32) -> ArmTrack {
    ArmTrack {
        row,
        slot,
        subtract: false,
        site_va,
    }
}

/// Every arm body's track folds, taken site by site from the disassembly.
///
/// Twenty rows exist and all twenty are used; the map is dense and the row
/// set is what pins the table's extent (see the parser's module doc). Most
/// rows are yaw offsets - the per-art swing angle - with pitch, eye-space X
/// and eye-space Z taking the rest.
pub const ARM_TRACKS: &[ArmTracks] = &[
    // Character 1.
    ArmTracks {
        entry_va: 0x801D_7308,
        tracks: &[
            arm_track_fold(0, PoseSlot::Pitch, 0x801D_7338),
            arm_track_fold(1, PoseSlot::Yaw, 0x801D_734C),
            // The arm's second branch (`0x801D73F0`, taken when
            // `actor[+0x21B] != 5`).
            arm_track_fold(2, PoseSlot::Yaw, 0x801D_7470),
            arm_track_fold(3, PoseSlot::Dist2, 0x801D_748C),
        ],
    },
    ArmTracks {
        entry_va: 0x801D_74A8,
        tracks: &[arm_track_fold(5, PoseSlot::Yaw, 0x801D_74F8)],
    },
    ArmTracks {
        entry_va: 0x801D_7568,
        tracks: &[arm_track_fold(6, PoseSlot::Yaw, 0x801D_75BC)],
    },
    ArmTracks {
        entry_va: 0x801D_7650,
        tracks: &[arm_track_fold(7, PoseSlot::Yaw, 0x801D_768C)],
    },
    // Character 2.
    ArmTracks {
        entry_va: 0x801D_76EC,
        tracks: &[
            arm_track_fold(0, PoseSlot::Pitch, 0x801D_7730),
            arm_track_fold(13, PoseSlot::Yaw, 0x801D_7744),
            arm_track_fold(2, PoseSlot::Yaw, 0x801D_77D8),
            arm_track_fold(3, PoseSlot::Dist2, 0x801D_77F4),
            arm_track_fold(14, PoseSlot::Yaw, 0x801D_7864),
        ],
    },
    ArmTracks {
        entry_va: 0x801D_7870,
        tracks: &[arm_track_fold(8, PoseSlot::Yaw, 0x801D_78A4)],
    },
    ArmTracks {
        entry_va: 0x801D_78F0,
        tracks: &[arm_track_fold(9, PoseSlot::Yaw, 0x801D_7934)],
    },
    ArmTracks {
        entry_va: 0x801D_797C,
        tracks: &[arm_track_fold(10, PoseSlot::Yaw, 0x801D_79C4)],
    },
    ArmTracks {
        entry_va: 0x801D_79F8,
        tracks: &[
            arm_track_fold(11, PoseSlot::Yaw, 0x801D_7A48),
            arm_track_fold(15, PoseSlot::Dist0, 0x801D_7A68),
            arm_track_fold(12, PoseSlot::Yaw, 0x801D_7AC0),
            arm_track_fold(16, PoseSlot::Dist0, 0x801D_7AD4),
        ],
    },
    // Character 3.
    ArmTracks {
        entry_va: 0x801D_7B4C,
        tracks: &[
            ArmTrack {
                row: 19,
                slot: PoseSlot::Dist2,
                // `subu v1,v1,v0` at `0x801D7B90` - the one fold that
                // subtracts.
                subtract: true,
                site_va: 0x801D_7B84,
            },
            arm_track_fold(0, PoseSlot::Pitch, 0x801D_7BA0),
            arm_track_fold(17, PoseSlot::Yaw, 0x801D_7BB4),
            arm_track_fold(2, PoseSlot::Yaw, 0x801D_7CBC),
            arm_track_fold(3, PoseSlot::Dist2, 0x801D_7CD8),
            arm_track_fold(4, PoseSlot::Yaw, 0x801D_7D48),
        ],
    },
    // `0x801D7D7C` (character 3, art 0x1C) reads no table row - it is built
    // entirely from literals (`0x80`, `-0x200`, `0x280`, `0x300`, `0xFF00`).
    ArmTracks {
        entry_va: 0x801D_7EA0,
        tracks: &[arm_track_fold(18, PoseSlot::Yaw, 0x801D_7EF4)],
    },
    // `0x801D81FC` (character 3, art 0x1E / 0x2A) likewise reads no row - its
    // offsets are multiples of the `ctx[+0x26E]` ramp.
    ArmTracks {
        entry_va: 0x801D_7D7C,
        tracks: &[],
    },
    ArmTracks {
        entry_va: 0x801D_81FC,
        tracks: &[],
    },
];

/// The track folds for an arm entry VA.
pub fn arm_tracks(entry_va: u32) -> Option<&'static [ArmTrack]> {
    ARM_TRACKS
        .iter()
        .find(|a| a.entry_va == entry_va)
        .map(|a| a.tracks)
}

/// Fold one track row into the pose, exactly as the arm's `addu`/`subu` +
/// `sh` pair does: 16-bit wrapping, into the slot the fold names.
pub fn apply_track(pose: &mut CameraPose, track: ArmTrack, value: i16) {
    let slot = match track.slot {
        PoseSlot::Pitch => &mut pose.rot[0],
        PoseSlot::Yaw => &mut pose.rot[1],
        PoseSlot::Dist0 => &mut pose.dist[0],
        PoseSlot::Dist2 => &mut pose.dist[2],
    };
    *slot = if track.subtract {
        slot.wrapping_sub(value)
    } else {
        slot.wrapping_add(value)
    };
}

/// The camera triple the arms build on the stack before the setter call.
///
/// Retail's frame is three halfword triples at `sp+0x10`, `sp+0x18` and
/// `sp+0x20`, passed as `a0`/`a1`/`a2` with a mode selector in `a3`. Field
/// names follow what the seeds fix; the `look_at` triple is the actor's
/// position **negated**, which is how a camera translation is expressed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CameraPose {
    /// `sp+0x10`, `sp+0x12`, `sp+0x14` - rotation triple. The middle slot seeds
    /// to the actor's negated facing angle (`-actor[+0x46]`).
    pub rot: [i16; 3],
    /// `sp+0x18`, `sp+0x1A`, `sp+0x1C` - distance triple. The second and third
    /// slots seed to [`POSE_SEED_ANGLE`].
    pub dist: [i16; 3],
    /// `sp+0x20`, `sp+0x22`, `sp+0x24` - look-at, the negated actor position
    /// (`-actor[+0x34]`, `-actor[+0x36]`, `-actor[+0x38]`).
    pub look_at: [i16; 3],
}

impl CameraPose {
    /// Seed the triple exactly as `0x801D7230..0x801D729C` does.
    pub fn seed(actor_pos: [u16; 3], actor_facing: u16) -> Self {
        Self {
            rot: [0, (actor_facing as i16).wrapping_neg(), 0],
            dist: [0, POSE_SEED_ANGLE, POSE_SEED_ANGLE],
            look_at: [
                (actor_pos[0] as i16).wrapping_neg(),
                (actor_pos[1] as i16).wrapping_neg(),
                (actor_pos[2] as i16).wrapping_neg(),
            ],
        }
    }
}

/// The animation-frame-driven push the arms share
/// (`0x801D735C..0x801D73DC` and its siblings at the `0xE0` / `0xF0`
/// thresholds).
///
/// `anim_frame` is `actor[+0x22C][+0x68]`, the live animation counter. Below
/// [`ANIM_PUSH_FLOOR`] the arm skips the push entirely; at or above it the push
/// is `(frame - 0x60) << 4`, clamped to [`ANIM_PUSH_CAP`]. Returns `None` for
/// the skip so a caller cannot confuse "no push" with "a push of zero" - retail
/// distinguishes them by branching around the pose writes.
pub const fn anim_push(anim_frame: i16) -> Option<i32> {
    if anim_frame < ANIM_PUSH_FLOOR {
        return None;
    }
    let scaled = ((anim_frame - ANIM_PUSH_BIAS) as i32) << 4;
    Some(if (scaled as u32) < ANIM_PUSH_CAP as u32 + 1 {
        scaled
    } else {
        ANIM_PUSH_CAP
    })
}

/// How the push splits across the pose in the first arm's two sub-branches
/// (`0x801D7398..0x801D73D8`).
///
/// The split is selected by whether the phase cursor `ctx[+0x26D]` is zero:
///
/// * non-zero: `rot[1] += push >> 1`, `dist[2] -= push >> 2`, `dist[0] += push`;
/// * zero: `rot[1] -= push`, `dist[2] -= push >> 1`.
///
/// `dist[1]` takes `-push` either way (the store at `0x801D7394` precedes the
/// branch). Retail's halvings are `srl`, not `sra`; the push is always
/// non-negative here (it is `(frame - 0x60) << 4` past a `frame >= 0x61` gate,
/// clamped at `0x100`), so the two agree.
pub fn apply_anim_push(pose: &mut CameraPose, phase_cursor: u8, push: i32) {
    pose.dist[1] = pose.dist[1].wrapping_sub(push as i16);
    if phase_cursor != 0 {
        pose.rot[1] = pose.rot[1].wrapping_add((push >> 1) as i16);
        pose.dist[2] = pose.dist[2].wrapping_sub((push >> 2) as i16);
        pose.dist[0] = pose.dist[0].wrapping_add(push as i16);
    } else {
        pose.rot[1] = pose.rot[1].wrapping_sub(push as i16);
        pose.dist[2] = pose.dist[2].wrapping_sub((push >> 1) as i16);
    }
}

/// How an arm addresses the per-phase halfword tracks at `0x801F4E10`.
///
/// Each arm computes one base, `0x801F4E10 + phase_cursor * 2`, and then reads
/// fixed byte offsets off it. The offsets the arms use are `0x00, 0x04, …,
/// 0x4C` - dense over twenty rows - so the table is twenty parallel halfword
/// tracks four bytes apart, each holding one value per phase.
/// [`legaia_asset::battle_attack_camera_table`] parses it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArmPhase {
    /// `ctx[+0x26D]` - the phase cursor.
    pub cursor: u8,
}

impl ArmPhase {
    /// Byte offset from the table base for track `track` at this phase.
    pub const fn track_offset(self, track: usize) -> usize {
        self.cursor as usize * 2 + track * 4
    }

    /// Read one track out of a parsed table, at this phase.
    ///
    /// Named `track_value` rather than `read`: the reachability analysis
    /// matches free functions and methods by NAME, so a method called `read`
    /// here picks up an edge from every `read` call in the workspace and
    /// reports this whole module as live.
    pub fn track_value(
        self,
        table: &legaia_asset::battle_attack_camera_table::AttackCameraTracks,
        track: usize,
    ) -> Option<i16> {
        table.track(track, self.cursor as usize)
    }
}

/// Retail's per-arm animation-frame thresholds, in the order the arms test them.
///
/// The first arm gates at `0x61`, later arms at `0xE0` and `0xF0`; each is a
/// `slti` against the same `actor[+0x22C][+0x68]` counter, so an arm's framing
/// changes only once the swing clip has run that far.
pub const ANIM_THRESHOLDS: [i16; 3] = [0x61, 0xE0, 0xF0];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_gate_condition_is_independently_fatal() {
        assert!(attack_camera_gate(100, CATEGORY_ATTACK, PHASE_ACTIVE, 0));
        assert!(
            !attack_camera_gate(0, CATEGORY_ATTACK, PHASE_ACTIVE, 0),
            "dead target"
        );
        assert!(
            !attack_camera_gate(100, 2, PHASE_ACTIVE, 0),
            "magic, not attack"
        );
        assert!(
            !attack_camera_gate(100, CATEGORY_ATTACK, 0x14, 0),
            "wrong phase"
        );
        assert!(
            !attack_camera_gate(100, CATEGORY_ATTACK, PHASE_ACTIVE, 3),
            "monster slot"
        );
        assert!(!attack_camera_gate(100, CATEGORY_ATTACK, PHASE_ACTIVE, 7));
    }

    #[test]
    fn only_three_participant_ids_have_camera_scripts() {
        assert_eq!(character_arm(1), Some(CharacterArm::One));
        assert_eq!(character_arm(2), Some(CharacterArm::Two));
        assert_eq!(character_arm(3), Some(CharacterArm::Three));
        for id in [0u8, 4, 5, 0xFF] {
            assert_eq!(character_arm(id), None, "id {id}");
        }
    }

    /// The three characters have three different tables, and the bound is
    /// per character: `2` accepts `0x2C`/`0x2D`, which `1` and `3` reject.
    #[test]
    fn each_character_dispatches_through_its_own_art_table() {
        use CharacterArm::*;
        assert_eq!(ART_JUMP_TABLES[One as usize].len(), 0x11);
        assert_eq!(ART_JUMP_TABLES[Two as usize].len(), 0x14);
        assert_eq!(ART_JUMP_TABLES[Three as usize].len(), 0x11);

        assert_eq!(art_arm(One, 0x1A), Some(0x801D_7308));
        assert_eq!(art_arm(Two, 0x1A), Some(0x801D_76EC));
        assert_eq!(art_arm(Three, 0x1A), Some(0x801D_7B4C));
        // Art 0x1C exists for 1 and 3 but is an epilogue slot for 2.
        assert!(art_arm(One, 0x1C).is_some());
        assert_eq!(art_arm(Two, 0x1C), None);
        assert!(art_arm(Three, 0x1C).is_some());
        // The three ids only character 2 can reach.
        for id in [0x2B, 0x2C, 0x2D] {
            assert_eq!(art_arm(One, id), None, "id {id:#x} past char 1's bound");
            assert_eq!(art_arm(Three, id), None, "id {id:#x} past char 3's bound");
        }
        assert_eq!(art_arm(Two, 0x2C), art_arm(Two, 0x1D), "0x2C shares 0x1D");
        assert_eq!(art_arm(Two, 0x2D), art_arm(Two, 0x1E), "0x2D shares 0x1E");
        assert_eq!(art_arm(Two, 0x2B), None, "an epilogue slot inside range");
    }

    /// The bias is unsigned, so an id below `0x1A` wraps past every table.
    #[test]
    fn art_ids_below_the_bias_are_rejected() {
        for c in [CharacterArm::One, CharacterArm::Two, CharacterArm::Three] {
            for id in [0u8, 1, 0x19] {
                assert_eq!(art_arm(c, id), None, "{c:?} id {id:#x}");
            }
        }
    }

    /// Thirteen distinct arm bodies exist across the three tables, and every
    /// one has a track-fold row (possibly empty).
    #[test]
    fn every_dispatched_arm_has_a_track_map() {
        let mut bodies = std::collections::BTreeSet::new();
        for table in ART_JUMP_TABLES {
            bodies.extend(table.iter().flatten().copied());
        }
        assert_eq!(bodies.len(), 13, "distinct arm bodies");
        for va in &bodies {
            assert!(arm_tracks(*va).is_some(), "arm {va:#010x} has no track map");
        }
        // And the map has no rows for bodies that are not dispatched.
        for arm in ARM_TRACKS {
            assert!(bodies.contains(&arm.entry_va), "{:#010x}", arm.entry_va);
        }
    }

    /// The track map covers the whole table: every row `0..20` is read by at
    /// least one arm. That density is what pins the parser's extent.
    #[test]
    fn the_track_map_uses_every_row_of_the_table() {
        use legaia_asset::battle_attack_camera_table::ATTACK_CAMERA_ROWS;
        let mut used = std::collections::BTreeSet::new();
        for arm in ARM_TRACKS {
            for t in arm.tracks {
                assert!(t.row < ATTACK_CAMERA_ROWS, "row {} out of table", t.row);
                used.insert(t.row);
            }
        }
        assert_eq!(
            used.len(),
            ATTACK_CAMERA_ROWS,
            "unused rows: {:?}",
            (0..ATTACK_CAMERA_ROWS)
                .filter(|r| !used.contains(r))
                .collect::<Vec<_>>()
        );
    }

    /// A fold lands in the component the arm's `sh` names and wraps at 16
    /// bits, and exactly one fold in the whole map subtracts.
    #[test]
    fn track_folds_hit_the_named_component() {
        let seed = CameraPose::seed([0, 0, 0], 0);
        let fold = |slot, subtract, v| {
            let mut p = seed;
            apply_track(
                &mut p,
                ArmTrack {
                    row: 0,
                    slot,
                    subtract,
                    site_va: 0,
                },
                v,
            );
            p
        };
        assert_eq!(fold(PoseSlot::Pitch, false, 0x100).rot[0], 0x100);
        assert_eq!(fold(PoseSlot::Yaw, false, 0x100).rot[1], 0x100);
        assert_eq!(fold(PoseSlot::Dist0, false, 0x100).dist[0], 0x100);
        assert_eq!(
            fold(PoseSlot::Dist2, false, 0x100).dist[2],
            POSE_SEED_ANGLE + 0x100
        );
        assert_eq!(
            fold(PoseSlot::Dist2, true, 0x100).dist[2],
            POSE_SEED_ANGLE - 0x100
        );
        // Halfword wrap, not saturation.
        assert_eq!(fold(PoseSlot::Yaw, false, i16::MAX).rot[1], i16::MAX);
        let mut p = fold(PoseSlot::Yaw, false, i16::MAX);
        apply_track(
            &mut p,
            ArmTrack {
                row: 0,
                slot: PoseSlot::Yaw,
                subtract: false,
                site_va: 0,
            },
            1,
        );
        assert_eq!(p.rot[1], i16::MIN);

        let subtracting: Vec<u32> = ARM_TRACKS
            .iter()
            .flat_map(|a| a.tracks.iter())
            .filter(|t| t.subtract)
            .map(|t| t.site_va)
            .collect();
        assert_eq!(subtracting, vec![0x801D_7B84]);
    }

    /// The phase cursor selects the row's column, so the two swing phases
    /// pull different offsets out of one row.
    #[test]
    fn arm_phase_reads_the_cursor_column() {
        use legaia_asset::battle_attack_camera_table::{
            self as tbl, ATTACK_CAMERA_FILE_OFFSET, ATTACK_CAMERA_LEN,
        };
        let mut buf = vec![0u8; ATTACK_CAMERA_FILE_OFFSET + ATTACK_CAMERA_LEN];
        // Row 3, phase 0 = -256; row 3, phase 1 = 512.
        let o = ATTACK_CAMERA_FILE_OFFSET + 3 * 4;
        buf[o..o + 2].copy_from_slice(&(-256i16).to_le_bytes());
        buf[o + 2..o + 4].copy_from_slice(&512i16.to_le_bytes());
        let table = tbl::parse(&buf).expect("parses");
        assert_eq!(ArmPhase { cursor: 0 }.track_value(&table, 3), Some(-256));
        assert_eq!(ArmPhase { cursor: 1 }.track_value(&table, 3), Some(512));
        assert_eq!(ArmPhase { cursor: 2 }.track_value(&table, 3), None);
    }

    #[test]
    fn pose_seed_negates_the_position_and_the_facing() {
        let p = CameraPose::seed([100, 200, 300], 0x800);
        assert_eq!(p.look_at, [-100, -200, -300]);
        assert_eq!(p.rot, [0, -0x800, 0]);
        assert_eq!(p.dist, [0, POSE_SEED_ANGLE, POSE_SEED_ANGLE]);
    }

    #[test]
    fn pose_seed_negation_wraps_at_the_halfword_boundary() {
        // The actor position is read with `lhu` and negated with `subu`, so a
        // coordinate above 0x7FFF comes back as a positive i16.
        let p = CameraPose::seed([0x8000, 0, 0], 0);
        assert_eq!(p.look_at[0], -0x8000i32 as i16);
        let p = CameraPose::seed([0xFFFF, 0, 0], 0);
        assert_eq!(p.look_at[0], 1);
    }

    #[test]
    fn anim_push_skips_below_the_floor_and_clamps_above_the_cap() {
        assert_eq!(anim_push(0), None);
        assert_eq!(anim_push(ANIM_PUSH_FLOOR - 1), None);
        // At the floor the push is (0x61 - 0x60) << 4 = 0x10.
        assert_eq!(anim_push(ANIM_PUSH_FLOOR), Some(0x10));
        // 0x70 - 0x60 = 0x10, << 4 = 0x100, exactly at the cap and kept.
        assert_eq!(anim_push(0x70), Some(0x100));
        // One frame later the scale overshoots and the clamp bites.
        assert_eq!(anim_push(0x71), Some(ANIM_PUSH_CAP));
        assert_eq!(anim_push(0x7FF), Some(ANIM_PUSH_CAP));
    }

    #[test]
    fn push_split_differs_between_the_two_phase_arms() {
        let base = CameraPose::seed([0, 0, 0], 0);

        let mut a = base;
        apply_anim_push(&mut a, 1, 0x100);
        assert_eq!(a.dist[1], POSE_SEED_ANGLE - 0x100);
        assert_eq!(a.rot[1], 0x80);
        assert_eq!(a.dist[2], POSE_SEED_ANGLE - 0x40);
        assert_eq!(a.dist[0], 0x100);

        let mut b = base;
        apply_anim_push(&mut b, 0, 0x100);
        assert_eq!(b.dist[1], POSE_SEED_ANGLE - 0x100);
        assert_eq!(b.rot[1], -0x100);
        assert_eq!(b.dist[2], POSE_SEED_ANGLE - 0x80);
        assert_eq!(b.dist[0], 0, "the zero-phase arm leaves dist[0] alone");
    }

    #[test]
    fn push_of_zero_still_writes_the_shared_slot() {
        // The store at 0x801D7394 precedes the branch, so it happens even for a
        // push the arms would otherwise ignore.
        let mut p = CameraPose::seed([0, 0, 0], 0);
        apply_anim_push(&mut p, 1, 0);
        assert_eq!(p.dist[1], POSE_SEED_ANGLE);
    }

    #[test]
    fn track_offsets_step_by_two_per_phase_and_four_per_track() {
        let p = ArmPhase { cursor: 0 };
        assert_eq!(p.track_offset(0), 0);
        assert_eq!(p.track_offset(1), 4);
        assert_eq!(p.track_offset(2), 8);
        assert_eq!(p.track_offset(3), 0xC);
        assert_eq!(p.track_offset(5), 0x14);
        let q = ArmPhase { cursor: 3 };
        assert_eq!(q.track_offset(0), 6);
        assert_eq!(q.track_offset(1), 10);
    }

    #[test]
    fn thresholds_are_ordered() {
        assert!(ANIM_THRESHOLDS.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(ANIM_THRESHOLDS[0], ANIM_PUSH_FLOOR);
    }
}
