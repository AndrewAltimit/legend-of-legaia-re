//! The entry gate, the per-character arm pick, the per-art jump tables and the per-arm camera tracks.

use super::*;

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
    /// Row index into [`legaia_game_tables::battle_attack_camera_table`] (retail's
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
        tracks: &[ArmTrack {
            row: 9,
            slot: PoseSlot::Yaw,
            // `subu v0,v0,v1` at `0x801D794C` - the row and the accumulator
            // are summed first and the SUM is subtracted from the yaw.
            subtract: true,
            site_va: 0x801D_7934,
        }],
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
