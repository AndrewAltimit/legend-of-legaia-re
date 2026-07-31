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
//! ## Where it runs in the frame
//!
//! **Not** inside `FUN_801D5854` case `6`. The call site is that function's
//! shared tail, *after* case 6 has already built its own pose and handed it to
//! the tween builder (`0x801D7130`, `a3 = 0xC`):
//!
//! ```text
//! 801d7130  jal   0x801d829c        ; case 6's own tween (or case 9's, ...)
//! 801d7138  lui   v0,0x8008
//! 801d713c  lw    v1,0x46c0(v0)     ; _DAT_800846C0
//! 801d7144  beq   v1,0x2,0x801d7188 ; == 2 -> skip the per-art camera
//! 801d714c  lw    v0,-0x42dc(v0)    ; the battle context
//! 801d7154  lbu   v0,0x13(v0)       ; ctx[+0x13], the acting slot
//! 801d7164  lw    v0,0x0(v0)        ; DAT_801C9370[slot], the acting actor
//! 801d716c  lbu   v0,0x1dd(v0)      ; actor[+0x1DD], its target slot
//! 801d7174  sltiu v0,v0,0x8
//! 801d7178  beq   v0,zero,0x801d7188
//! 801d7180  jal   0x801d71b8        ; <- here
//! ```
//!
//! So the per-art camera is an **override**, not a fold: it seeds a fresh pose
//! from the actor ([`CameraPose::seed`]), adds its own arm's offsets, and calls
//! the *same* tween builder again with its own, much shorter duration (`1`, `3`
//! or `6` display frames against case 6's `0xC`). Whichever call ran last owns
//! the tween table that frame, and this one runs last. An art id with no arm
//! leaves case 6's framing standing. [`OUTER_GATE`] carries the two tail
//! conditions.
//!
//! The per-character arms that *are* inside case 6 (`0x801D5DAC` /
//! `0x801D5FC0` / `0x801D61E8`, dispatched at `0x801D5D50` and rejoining at
//! `0x801D645C`) sit between the base pose and the height floor and dispatch on
//! the **same** `actor[+0x1DB]` byte over a different band (`0x11..=0x18`,
//! bias `-0x11`, bound `8`). They are a separate family; this module ports the
//! `0x1A..=0x2D` one.
//!
//! ## What `actor[+0x1DB]` is
//!
//! Not a Tactical-Arts `ActionConstant`: it is the **latched battle-animation
//! id**. `FUN_8004AD80` copies `actor[+0x1DA]` (the staged anim id) into it
//! once per animation tick (`0x8004AEB0..0x8004AEB8`), so the camera arm that
//! runs is chosen by the clip that is playing. Ids `>= 0x10` are art-bank
//! records (`legaia_engine_vm::anim_vm::resolve_staged_anim`), so the arm band
//! `0x1A..=0x2D` is bank records `0x0A..=0x1D`.
//!
//! ## What `ctx[+0x26D]` is
//!
//! A **coin flip**, not a swing-phase counter. Its one writer is
//! `FUN_8004E13C` in `SCUS_942.54` (`0x8004E2DC`), which stores `rand() % 2`
//! beside `ctx[+0x6DA] = (rand() % 2) * 0x800 + 0x280` and `ctx[+0xD] = 0`.
//! Each of the twenty table rows therefore holds **two alternative offsets**
//! and retail picks one per action, which is why the same art frames from two
//! visibly different angles on successive swings. `FUN_801D5854` clears it to
//! `0` when the acting character id is `3` (`0x801D6A5C`).
//!
//! ## The two ramp counters
//!
//! `ctx[+0x26E]` and `ctx[+0x87C]` are advanced by `FUN_801D5854`'s own
//! prologue, on every call, by `8 * frame_step` - the ramp saturating at
//! `0xC8` ([`AttackCamCtx::advance`], `0x801D58F8..0x801D5960`). Several arms
//! re-zero them through the `ctx[+0x26F]` latch when the swing crosses an
//! animation-frame threshold ([`AttackCamCtx::latch_reset`]). Both are ported,
//! so the arms carry their literals and ramps rather than only their table
//! folds.

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

// ---------------------------------------------------------------------------
// The executable arms.
// ---------------------------------------------------------------------------

/// The two conditions `FUN_801D5854`'s tail tests before it calls this
/// routine at all (`0x801D7138..0x801D7180`).
///
/// `render_mode` is `_DAT_800846C0` (the per-art camera is skipped in mode
/// `2`) and `target_slot` is the acting actor's `+0x1DD`; a slot of `8` or
/// more - retail's "no target" encoding - also skips it. Both are outside the
/// gate [`attack_camera_gate`] ports, which lives *inside* the routine.
pub const fn outer_gate(render_mode: u32, target_slot: u8) -> bool {
    render_mode != OUTER_GATE.0 && target_slot < OUTER_GATE.1
}

/// The literals [`outer_gate`] tests: the `_DAT_800846C0` value that skips the
/// per-art camera, and the exclusive bound on `actor[+0x1DD]`.
pub const OUTER_GATE: (u32, u8) = (2, 8);

/// The battle-context counters the arms read - and, through the
/// `ctx[+0x26F]` latch, write.
///
/// Every field is a retail context byte or word. The engine's battle camera
/// owns one of these for the life of a battle, exactly as retail's context
/// does, so the arms' ramps are continuous across an action instead of being
/// recomputed from nothing each frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AttackCamCtx {
    /// `ctx[+0x26D]` - the per-action coin flip that picks a track column.
    /// See the module doc; retail rolls it in `FUN_8004E13C`.
    pub phase_cursor: u8,
    /// `ctx[+0x26E]` - the `0..=`[`AttackCamCtx::RAMP_CAP`] ramp.
    pub ramp: u8,
    /// `ctx[+0x87C]` - the free-running 32-bit accumulator advanced beside
    /// the ramp. Unlike the ramp it does not saturate; the arms read it whole
    /// (`lw` + shift) or truncated (`lhu`).
    pub accum: u32,
    /// `ctx[+0x26F]` - the latch several arms bump when they re-zero the
    /// ramp at an animation-frame threshold, so the reset fires once per
    /// crossing instead of every frame.
    pub latch: u8,
}

impl AttackCamCtx {
    /// Ceiling on [`Self::ramp`] (`sltiu v0,a1,0xc8` / `li v0,0xc8`).
    pub const RAMP_CAP: u8 = 0xC8;
    /// Ramp increment per display frame (`sll v0,v0,0x3` on the frame step).
    pub const RAMP_SCALE: u32 = 8;

    /// `FUN_801D5854`'s prologue ramp advance (`0x801D58F8..0x801D5960`),
    /// which runs on **every** call to that function - so once per display
    /// frame for as long as any framing case is being re-armed.
    ///
    /// The accumulator takes `frame_step * 8` unconditionally; the ramp takes
    /// the same increment only while it is still below the cap, and is then
    /// clamped to it.
    pub fn advance(&mut self, frame_step: u32) {
        let d = frame_step.wrapping_mul(Self::RAMP_SCALE);
        self.accum = self.accum.wrapping_add(d);
        if self.ramp < Self::RAMP_CAP {
            let next = u32::from(self.ramp).wrapping_add(d);
            self.ramp = if next > u32::from(Self::RAMP_CAP) {
                Self::RAMP_CAP
            } else {
                next as u8
            };
        }
    }

    /// The threshold-crossing reset several arms perform: when the latch
    /// still reads `expect`, re-zero the ramp (and, in the two Gala arms that
    /// pass `clear_accum`, the accumulator too) and bump the latch.
    ///
    /// Retail spells it `bne ctx[+0x26F], expect, skip` - the store to
    /// `ctx[+0x26E]` precedes the re-read of the latch, which is why the
    /// order here is reset-then-bump.
    pub fn latch_reset(&mut self, expect: u8, clear_accum: bool) {
        if self.latch != expect {
            return;
        }
        self.ramp = 0;
        if clear_accum {
            self.accum = 0;
        }
        self.latch = self.latch.wrapping_add(1);
    }

    /// Clear the per-action state and roll a fresh [`Self::phase_cursor`].
    /// `coin` is the caller's `rand() % 2` - retail's `FUN_8004E13C` rolls it
    /// at action seed, and the port's battle camera rolls it on entry into
    /// the Action phase.
    pub fn begin_action(&mut self, coin: bool) {
        *self = AttackCamCtx {
            phase_cursor: u8::from(coin),
            ..AttackCamCtx::default()
        };
    }
}

/// The acting actor's per-art camera channels - the three inputs the shared
/// battle camera did not have, plus the position and facing the seed needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttackCamActor {
    /// `DAT_8007BD10[ctx[+0x13]]` resolved through [`character_arm`].
    pub character: CharacterArm,
    /// `actor[+0x1DB]` - the latched battle-animation id (see the module doc).
    pub art_id: u8,
    /// `actor[+0x21B]` - the arm sub-selector two arms branch on.
    pub arm_select: u8,
    /// `actor[+0x22C][+0x68]` - the live animation cursor, in **sixteenths**
    /// of a keyframe (retail's loop bounds are `clip[+0x85] << 4` /
    /// `clip[+0x86] << 4`, `FUN_80047430` `0x800477D4` / `0x8004781C`).
    pub anim_frame: i16,
    /// `actor[+0x34/+0x36/+0x38]`.
    pub pos: [u16; 3],
    /// `actor[+0x46]`.
    pub facing: u16,
}

/// What one frame of the per-art camera produces: the pose it hands the tween
/// builder and the duration (retail's `a3`, in **display frames**) it hands it
/// alongside.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttackCamFraming {
    /// The pose, with `dist[2]` still in **raw world units** - the tween
    /// builder is what converts it to projection units.
    pub pose: CameraPose,
    /// `FUN_801D829C`'s fourth argument.
    pub duration_frames: u16,
}

/// Run the per-art attack camera for one frame.
///
/// Returns `None` for every path that reaches the epilogue `0x801D828C`
/// without calling the tween builder - an art id with no arm, a character id
/// outside `1..=3`, a monster slot (`0x801D8280` writes the local frame and
/// returns), or Gala's `0x1E` arm past its animation cut-off. In every such
/// case retail leaves whatever `FUN_801D5854` armed standing, which is what
/// the caller must do too.
///
/// `ctx` is `&mut` because seven arms re-zero the ramps through the
/// `ctx[+0x26F]` latch.
///
/// PORT: FUN_801D71B8
pub fn attack_camera_framing(
    actor: AttackCamActor,
    ctx: &mut AttackCamCtx,
    table: &legaia_asset::battle_attack_camera_table::AttackCameraTracks,
) -> Option<AttackCamFraming> {
    let entry = art_arm(actor.character, actor.art_id)?;
    let mut p = CameraPose::seed(actor.pos, actor.facing);
    // Character 3's branch overwrites the seeded TR.y before it dispatches
    // (`li v0,0x600` in the delay slot at `0x801D72C8`, stored at
    // `0x801D7B14`).
    if actor.character == CharacterArm::Three {
        p.dist[1] = 0x600;
    }
    let cursor = ctx.phase_cursor;
    let phase = ArmPhase { cursor };
    let track = |t: usize| phase.track_value(table, t).unwrap_or(0);
    let f = actor.anim_frame;
    let s = actor.arm_select;

    // The ramp reads the arms share. `ramp` is a byte, so every scaling of it
    // fits an i16 without care; the accumulator is a 32-bit word whose low
    // halfword is what lands in the pose.
    let r = i16::from(ctx.ramp);
    let acc = ctx.accum;
    let acc_lo = acc as u16 as i16;
    let acc_shr = |k: u32| (acc >> k) as u16 as i16;
    let acc_shl = |k: u32| (acc << k) as u16 as i16;

    let dur: u16 = match entry {
        // -- character 1 -------------------------------------------------
        // 0x801D7308 (art 0x1A).
        0x801D_7308 => {
            if s == 5 {
                p.dist[2] = p.dist[2].wrapping_sub(0x100);
                p.rot[0] = p.rot[0].wrapping_add(track(0));
                p.rot[1] = p.rot[1].wrapping_add(track(1));
                if let Some(push) = anim_push(f) {
                    apply_anim_push(&mut p, cursor, push);
                }
                6
            } else if s < 2 {
                // `0x801D7418` falls into character 3's late block.
                gala_late_arm(&mut p, ctx, track(4));
                1
            } else {
                threshold_arm(&mut p, ctx, 0, track(2), track(3));
                1
            }
        }
        // 0x801D74A8 (art 0x1E / 0x2A).
        0x801D_74A8 => {
            if f < 0xE0 {
                p.rot[0] = p.rot[0].wrapping_sub(r >> 2);
                p.rot[1] = p.rot[1].wrapping_add(track(5).wrapping_add(acc_lo));
                p.dist[1] = p.dist[1].wrapping_add(r);
                p.dist[2] = p.dist[2].wrapping_add(r >> 2);
                6
            } else {
                p.rot[1] = p.rot[1].wrapping_add(0x600);
                p.dist[2] = p.dist[2].wrapping_add(0x400);
                p.dist[0] = p.dist[0].wrapping_sub(0x200);
                1
            }
        }
        // 0x801D7568 (art 0x1D).
        0x801D_7568 => {
            if f < 0xF0 {
                p.rot[0] = p.rot[0].wrapping_sub(r >> 1);
                p.rot[1] = p.rot[1].wrapping_add(track(6).wrapping_add(acc_lo));
                p.dist[1] = p.dist[1].wrapping_add(r << 1);
                p.dist[2] = p.dist[2].wrapping_add(acc_shr(2));
                6
            } else {
                p.rot[0] = p.rot[0].wrapping_add(0x100);
                p.dist[0] = p.dist[0].wrapping_sub(0x400);
                p.rot[1] = p.rot[1].wrapping_add(0x200);
                p.dist[2] = p.dist[2].wrapping_add(0x400);
                p.dist[1] = p.dist[1].wrapping_add(0x200);
                3
            }
        }
        // 0x801D7650 (art 0x1C).
        0x801D_7650 => {
            p.rot[0] = p.rot[0].wrapping_add(r >> 2);
            p.rot[1] = p.rot[1].wrapping_add(track(7).wrapping_add(acc_lo));
            p.dist[2] = p.dist[2].wrapping_add(acc_shr(1));
            6
        }
        // -- character 2 -------------------------------------------------
        // 0x801D76EC (art 0x1A).
        0x801D_76EC => {
            if s == 2 {
                p.dist[2] = p.dist[2].wrapping_sub(0x100);
                p.dist[0] = p.dist[0].wrapping_sub(0x100);
                p.rot[0] = p.rot[0].wrapping_add(track(0));
                p.rot[1] = p.rot[1].wrapping_add(track(13));
                6
            } else if f < 0x90 {
                threshold_arm(&mut p, ctx, 0, track(2), track(3));
                1
            } else {
                // Rejoins the shared block at `0x801D7D4C` with row 14.
                gala_late_arm(&mut p, ctx, track(14));
                1
            }
        }
        // 0x801D7870 (art 0x20).
        0x801D_7870 => {
            p.rot[0] = p.rot[0].wrapping_add(r >> 2);
            p.rot[1] = p.rot[1].wrapping_add(track(8));
            p.dist[1] = p.dist[1].wrapping_add(r);
            p.dist[2] = p.dist[2].wrapping_sub(acc_shr(1).wrapping_sub(0x400));
            6
        }
        // 0x801D78F0 (art 0x1D / 0x2C).
        0x801D_78F0 => {
            p.rot[0] = p.rot[0].wrapping_add(r >> 1);
            p.dist[0] = p.dist[0].wrapping_add(0x200);
            p.rot[1] = p.rot[1].wrapping_sub(track(9).wrapping_add(acc_shr(1)));
            p.dist[1] = p.dist[1].wrapping_add(r >> 1);
            p.dist[2] = p.dist[2].wrapping_add(acc_shl(1));
            6
        }
        // 0x801D797C (art 0x1E / 0x2D). The pitch fold is `srl` then
        // `ori 0xff80`, i.e. a small ramp OR-ed into a negative constant.
        0x801D_797C => {
            p.rot[0] = p.rot[0].wrapping_add(((r >> 2) as u16 | 0xFF80) as i16);
            p.dist[0] = p.dist[0].wrapping_sub(0x300);
            p.rot[1] = p.rot[1].wrapping_add(track(10).wrapping_add(acc_shr(1)));
            p.dist[2] = p.dist[2].wrapping_add(acc_shr(1));
            1
        }
        // 0x801D79F8 (art 0x1F).
        0x801D_79F8 => {
            if f < 0xE0 {
                p.rot[0] = p.rot[0].wrapping_add(r >> 1);
                p.rot[1] = p.rot[1].wrapping_add(track(11).wrapping_add(acc_shr(1)));
                p.dist[0] = p.dist[0].wrapping_add(track(15));
                p.dist[2] = p.dist[2].wrapping_add(r >> 1);
                6
            } else {
                p.rot[0] = p.rot[0].wrapping_add(r >> 1);
                p.rot[1] = p.rot[1].wrapping_add(track(12));
                p.dist[0] = p.dist[0].wrapping_add(track(16));
                p.dist[1] = p.dist[1].wrapping_sub(r >> 1);
                p.dist[2] = p.dist[2].wrapping_add(r.wrapping_mul(3));
                1
            }
        }
        // -- character 3 -------------------------------------------------
        // 0x801D7B4C (art 0x1A).
        0x801D_7B4C => {
            if f < 0x70 {
                p.dist[2] = p.dist[2].wrapping_sub(track(19).wrapping_add(r));
                p.rot[0] = p.rot[0].wrapping_add(track(0));
                p.rot[1] = p.rot[1].wrapping_add(track(17));
                6
            } else if f < 0xA0 {
                threshold_arm(&mut p, ctx, 0, track(2), track(3));
                1
            } else {
                gala_late_arm(&mut p, ctx, track(4));
                1
            }
        }
        // 0x801D7D7C (art 0x1C) - four literal-only animation windows.
        0x801D_7D7C => {
            if f < 0x40 {
                p.rot[0] = p.rot[0].wrapping_add(0x80);
                p.dist[0] = p.dist[0].wrapping_sub(0x200);
                p.rot[1] = p.rot[1].wrapping_add(0x200);
            } else if f < 0x70 {
                p.rot[0] = p.rot[0].wrapping_sub(0x100);
                p.rot[1] = p.rot[1].wrapping_sub(0x200);
                p.dist[0] = p.dist[0].wrapping_add(0x280);
                p.dist[1] = p.dist[1].wrapping_sub(0x200);
            } else if f < 0xA0 {
                p.rot[0] = p.rot[0].wrapping_sub(0x100);
                p.rot[1] = p.rot[1].wrapping_add(0x200);
                p.dist[0] = p.dist[0].wrapping_sub(0x200);
                p.dist[1] = p.dist[1].wrapping_sub(0x200);
            } else {
                p.rot[0] = p.rot[0].wrapping_add(0x80);
                p.rot[1] = p.rot[1].wrapping_sub(0x200);
                p.dist[0] = p.dist[0].wrapping_add(0x300);
            }
            1
        }
        // 0x801D7EA0 (art 0x1D) - the one arm that drives the roll slot, and
        // the one that reads a track row at a FIXED cursor.
        0x801D_7EA0 => {
            if f < 0xB0 {
                if cursor != 0 {
                    p.rot[0] = p.rot[0].wrapping_sub(r >> 1);
                    p.rot[1] = p.rot[1].wrapping_add(track(18).wrapping_add(acc_shl(1)));
                    p.rot[2] = p.rot[2].wrapping_sub(r >> 1);
                    p.dist[0] = p.dist[0].wrapping_add(acc_lo);
                    p.dist[1] = p.dist[1].wrapping_sub(acc_shr(1));
                    // `multu` by 0xAAAAAAAB then `srl 1` - a divide by three.
                    let third = (((u64::from(acc) * 0xAAAA_AAABu64) >> 32) >> 1) as u16 as i16;
                    p.dist[2] = p.dist[2].wrapping_add(third.wrapping_add(0xFF00u16 as i16));
                } else {
                    p.rot[0] = p.rot[0].wrapping_add(r);
                    // `lhu v1,0x4e58(v0)` - row 18 at cursor 0, hard-coded.
                    let fixed = table.track(18, 0).unwrap_or(0);
                    p.rot[1] = p.rot[1].wrapping_add(fixed.wrapping_add(acc_lo));
                }
                6
            } else if f < 0x110 {
                ctx.latch_reset(0, true);
                let acc = ctx.accum;
                let lo = acc as u16 as i16;
                let shl1 = (acc << 1) as u16 as i16;
                if cursor != 0 {
                    p.rot[0] = p.rot[0].wrapping_add(lo.wrapping_add(0xFFA0u16 as i16));
                    p.rot[1] = p.rot[1]
                        .wrapping_sub((acc.wrapping_mul(3) as u16 as i16).wrapping_sub(0x400));
                    p.dist[0] = p.dist[0].wrapping_add(shl1.wrapping_add(0xFD00u16 as i16));
                    p.dist[1] = p.dist[1].wrapping_add(shl1.wrapping_add(0xFE00u16 as i16));
                    p.dist[2] = p.dist[2].wrapping_sub(shl1.wrapping_sub(0x200));
                } else {
                    p.dist[0] = p.dist[0].wrapping_sub(0x400);
                    p.rot[1] = p.rot[1].wrapping_add(0x280);
                    p.dist[2] = p.dist[2].wrapping_sub(0x100);
                    p.dist[1] = p.dist[1].wrapping_sub(0x300);
                    p.rot[0] = p.rot[0].wrapping_sub((i16::from(ctx.ramp) >> 2) + 0x80);
                }
                1
            } else {
                ctx.latch_reset(1, true);
                let acc = ctx.accum;
                let r = i16::from(ctx.ramp);
                let shl1 = (acc << 1) as u16 as i16;
                p.dist[0] = p.dist[0].wrapping_sub(shl1.wrapping_sub(0x400));
                if cursor != 0 {
                    p.rot[0] = p.rot[0].wrapping_sub(r);
                    p.rot[1] = p.rot[1].wrapping_add(r.wrapping_mul(6).wrapping_sub(0x600));
                    p.dist[1] = p.dist[1].wrapping_sub(r);
                    p.dist[2] = p.dist[2].wrapping_sub(r);
                } else {
                    p.rot[0] = p.rot[0].wrapping_sub(0x80);
                    p.dist[2] = p.dist[2].wrapping_sub(0x100);
                    p.dist[1] = p.dist[1].wrapping_sub(0x100);
                    p.rot[1] =
                        p.rot[1].wrapping_add((acc as u16 as i16).wrapping_add(0xFE00u16 as i16));
                }
                1
            }
        }
        // 0x801D81FC (art 0x1E / 0x2A) - the one arm with a hard cut-off:
        // past `0xC0` it returns without arming anything.
        0x801D_81FC => {
            if f >= 0xC0 {
                return None;
            }
            p.rot[1] = p.rot[1].wrapping_add(r.wrapping_mul(5).wrapping_sub(0x600));
            p.dist[0] = p.dist[0].wrapping_add(r << 2);
            p.dist[2] = p.dist[2].wrapping_add(0x40i16.wrapping_sub(r << 1));
            1
        }
        _ => return None,
    };
    Some(AttackCamFraming {
        pose: p,
        duration_frames: dur,
    })
}

/// The "mid-swing" block three arms share (`0x801D7420`, `0x801D7788`,
/// `0x801D7C6C`): latch-reset the ramp at the threshold crossing, tilt the
/// pitch a fixed `0xC0`, fold one row into the yaw and another into the depth,
/// and drop the height half a unit.
///
/// The ramp read at the tail (`0x801D749C` and siblings) is **after** the
/// reset, so the frame the threshold is crossed on subtracts nothing - which
/// is why this is a helper rather than an inline `r << 1`.
fn threshold_arm(
    pose: &mut CameraPose,
    ctx: &mut AttackCamCtx,
    expect_latch: u8,
    yaw_row: i16,
    depth_row: i16,
) {
    ctx.latch_reset(expect_latch, false);
    pose.rot[0] = pose.rot[0].wrapping_add(0xC0);
    pose.rot[1] = pose.rot[1].wrapping_add(yaw_row);
    pose.dist[1] = pose.dist[1].wrapping_sub(0x200);
    pose.dist[2] = pose.dist[2]
        .wrapping_add(depth_row)
        .wrapping_sub(i16::from(ctx.ramp) << 1);
}

/// The block at `0x801D7CF4` + `0x801D7D4C`: the late window of Gala's `0x1A`
/// arm, which Vahn's `0x1A` arm branches into when `actor[+0x21B] < 2`
/// (`0x801D7418`) and Noa's `0x1A` arm reaches from `0x801D7810` with its own
/// row. Like [`threshold_arm`], the ramp is read after the latch reset.
fn gala_late_arm(pose: &mut CameraPose, ctx: &mut AttackCamCtx, row: i16) {
    ctx.latch_reset(1, false);
    pose.dist[1] = pose.dist[1].wrapping_sub(0x100);
    pose.dist[2] = pose.dist[2].wrapping_add(0x200);
    pose.rot[1] = pose.rot[1].wrapping_add(row);
    pose.dist[2] = pose.dist[2].wrapping_sub(i16::from(ctx.ramp) << 1);
}

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
        assert_eq!(subtracting, vec![0x801D_7934, 0x801D_7B84]);
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

    // -- the executable arms ------------------------------------------------

    /// A table whose row `r` reads `(r * 16, r * 16 + 1)` - distinguishable
    /// per row AND per cursor, so a test can tell which row an arm folded and
    /// which column it took.
    fn probe_table() -> legaia_asset::battle_attack_camera_table::AttackCameraTracks {
        use legaia_asset::battle_attack_camera_table as tbl;
        let mut buf = vec![0u8; tbl::ATTACK_CAMERA_FILE_OFFSET + tbl::ATTACK_CAMERA_LEN];
        for r in 0..tbl::ATTACK_CAMERA_ROWS {
            for c in 0..tbl::ATTACK_CAMERA_PHASES {
                let v = (r as i16 * 16 + c as i16).to_le_bytes();
                let o = tbl::ATTACK_CAMERA_FILE_OFFSET + r * 4 + c * 2;
                buf[o] = v[0];
                buf[o + 1] = v[1];
            }
        }
        tbl::parse(&buf).expect("probe table parses")
    }

    fn probe_actor(character: CharacterArm, art_id: u8) -> AttackCamActor {
        AttackCamActor {
            character,
            art_id,
            arm_select: 0,
            anim_frame: 0,
            pos: [0, 0, 0],
            facing: 0,
        }
    }

    /// The ramp is a per-display-frame `+8` saturating at `0xC8`; the
    /// accumulator takes the same increment and never saturates.
    #[test]
    fn the_ramps_advance_by_eight_per_frame_and_only_one_saturates() {
        let mut ctx = AttackCamCtx::default();
        ctx.advance(1);
        assert_eq!((ctx.ramp, ctx.accum), (8, 8));
        for _ in 0..100 {
            ctx.advance(1);
        }
        assert_eq!(ctx.ramp, AttackCamCtx::RAMP_CAP, "ramp saturates");
        assert_eq!(ctx.accum, 808, "the accumulator keeps going");
        // A frame step of 2 (a dropped frame) doubles both increments.
        let mut ctx = AttackCamCtx::default();
        ctx.advance(2);
        assert_eq!((ctx.ramp, ctx.accum), (16, 16));
    }

    /// The latch fires once per crossing, and only for its own expected value.
    #[test]
    fn the_latch_reset_fires_once_per_threshold_crossing() {
        let mut ctx = AttackCamCtx {
            ramp: 40,
            accum: 400,
            latch: 0,
            phase_cursor: 0,
        };
        ctx.latch_reset(1, false);
        assert_eq!(ctx.ramp, 40, "a latch of 0 does not answer to `expect = 1`");
        ctx.latch_reset(0, false);
        assert_eq!((ctx.ramp, ctx.latch, ctx.accum), (0, 1, 400));
        ctx.ramp = 40;
        ctx.latch_reset(0, false);
        assert_eq!(ctx.ramp, 40, "the latch has moved on");
        ctx.latch_reset(1, true);
        assert_eq!((ctx.ramp, ctx.latch, ctx.accum), (0, 2, 0), "clears accum");
    }

    /// The cursor is a coin flip, so `begin_action` seeds `0` or `1` and
    /// wipes the ramps the previous action left behind.
    #[test]
    fn begin_action_rolls_the_cursor_and_clears_the_ramps() {
        let mut ctx = AttackCamCtx {
            ramp: 200,
            accum: 9999,
            latch: 2,
            phase_cursor: 1,
        };
        ctx.begin_action(false);
        assert_eq!(ctx, AttackCamCtx::default());
        ctx.begin_action(true);
        assert_eq!(ctx.phase_cursor, 1);
        assert_eq!((ctx.ramp, ctx.accum, ctx.latch), (0, 0, 0));
    }

    /// The outer gate is the CALL SITE's, not the routine's - both halves.
    #[test]
    fn the_outer_gate_rejects_render_mode_two_and_a_targetless_actor() {
        assert!(outer_gate(0, 3));
        assert!(!outer_gate(2, 3), "render mode 2");
        assert!(!outer_gate(0, 8), "no target");
        assert!(!outer_gate(0, 0xFF));
    }

    /// Every one of the thirteen arm bodies is reachable through the
    /// dispatch and produces a framing - the executable counterpart of
    /// `every_dispatched_arm_has_a_track_map`.
    #[test]
    fn every_arm_body_produces_a_framing() {
        let table = probe_table();
        let mut seen = std::collections::BTreeSet::new();
        for (ci, c) in [CharacterArm::One, CharacterArm::Two, CharacterArm::Three]
            .into_iter()
            .enumerate()
        {
            for art in FIRST_ART..=0x2Du8 {
                let Some(va) = art_arm(c, art) else { continue };
                // Walk the animation cursor so every window of a
                // multi-window arm is exercised.
                for anim in [0i16, 0x50, 0x80, 0xB0, 0xF8, 0x120] {
                    let mut ctx = AttackCamCtx {
                        phase_cursor: (ci % 2) as u8,
                        ramp: 24,
                        accum: 240,
                        latch: 0,
                    };
                    let mut a = probe_actor(c, art);
                    a.anim_frame = anim;
                    if let Some(f) = attack_camera_framing(a, &mut ctx, &table) {
                        assert!(f.duration_frames >= 1);
                        seen.insert(va);
                    }
                }
            }
        }
        assert_eq!(seen.len(), 13, "every arm body framed at least once");
    }

    /// The one arm with a hard cut-off returns `None` past it, which is what
    /// leaves case 6's framing standing rather than snapping to a default.
    #[test]
    fn galas_second_arm_stops_framing_past_its_cutoff() {
        let table = probe_table();
        let mut ctx = AttackCamCtx::default();
        let mut a = probe_actor(CharacterArm::Three, 0x1E);
        assert_eq!(art_arm(CharacterArm::Three, 0x1E), Some(0x801D_81FC));
        a.anim_frame = 0xBF;
        assert!(attack_camera_framing(a, &mut ctx, &table).is_some());
        a.anim_frame = 0xC0;
        assert!(attack_camera_framing(a, &mut ctx, &table).is_none());
    }

    /// An art id outside the character's own table frames nothing at all.
    #[test]
    fn an_art_without_an_arm_frames_nothing() {
        let table = probe_table();
        let mut ctx = AttackCamCtx::default();
        for art in [0u8, 0x19, 0x1B, 0x2B, 0xFF] {
            let a = probe_actor(CharacterArm::One, art);
            assert!(
                attack_camera_framing(a, &mut ctx, &table).is_none(),
                "art {art:#x}"
            );
        }
    }

    /// The cursor picks a COLUMN, and the probe table's odd/even split makes
    /// the choice visible in the pose: the two columns of the same row differ
    /// by one, so a cursor flip moves the folded component by one.
    #[test]
    fn the_cursor_selects_which_column_the_arms_fold() {
        let table = probe_table();
        // Character 1, art 0x1C - a single-fold arm (row 7 into the yaw).
        let pose_at = |cursor: u8| {
            let mut ctx = AttackCamCtx {
                phase_cursor: cursor,
                ..Default::default()
            };
            attack_camera_framing(probe_actor(CharacterArm::One, 0x1C), &mut ctx, &table)
                .expect("arm fires")
                .pose
        };
        let a = pose_at(0);
        let b = pose_at(1);
        assert_eq!(b.rot[1] - a.rot[1], 1, "column 1 is column 0 plus one");
        assert_eq!(a.rot[1], 7 * 16, "row 7, column 0");
    }

    /// The arm's sub-selector genuinely selects: Vahn's `0x1A` arm takes
    /// three different bodies for `5`, `0` and `2`.
    #[test]
    fn the_arm_sub_selector_picks_between_three_bodies() {
        let table = probe_table();
        let pose_for = |s: u8| {
            let mut ctx = AttackCamCtx::default();
            let mut a = probe_actor(CharacterArm::One, 0x1A);
            a.arm_select = s;
            attack_camera_framing(a, &mut ctx, &table)
                .expect("arm fires")
                .pose
        };
        let five = pose_for(5);
        let low = pose_for(0);
        let high = pose_for(2);
        assert_ne!(five, low);
        assert_ne!(low, high);
        assert_ne!(five, high);
        // `s == 5` is the only body that folds row 1 into the yaw.
        assert_eq!(five.rot[1], 16, "row 1, column 0");
        // `s >= 2` is the mid-swing block: pitch `+0xC0`, row 2 into the yaw.
        assert_eq!(high.rot[0], 0xC0);
        assert_eq!(high.rot[1], 2 * 16);
    }

    /// The animation cursor selects which window of a multi-window arm runs,
    /// and the windows are visibly different framings.
    #[test]
    fn the_animation_cursor_walks_an_arm_through_its_windows() {
        let table = probe_table();
        let pose_at = |anim: i16| {
            let mut ctx = AttackCamCtx::default();
            let mut a = probe_actor(CharacterArm::Three, 0x1C);
            a.anim_frame = anim;
            attack_camera_framing(a, &mut ctx, &table)
                .expect("arm fires")
                .pose
        };
        // Gala's 0x1C arm has four literal windows at 0x40 / 0x70 / 0xA0.
        let w: Vec<CameraPose> = [0x00, 0x40, 0x70, 0xA0]
            .iter()
            .map(|f| pose_at(*f))
            .collect();
        for (i, a) in w.iter().enumerate() {
            for b in &w[i + 1..] {
                assert_ne!(a, b, "window {i} collides with a later one");
            }
        }
        // ... and the seeded TR.y for character 3 is 0x600, not 0x400.
        assert_eq!(w[0].dist[1], 0x600);
        assert_eq!(w[1].dist[1], 0x600 - 0x200);
    }

    /// The ramp read at the tail of a threshold arm happens AFTER the latch
    /// reset, so the frame the threshold is crossed on subtracts nothing.
    #[test]
    fn a_threshold_crossing_zeroes_the_ramp_before_the_arm_reads_it() {
        let table = probe_table();
        let run = |latch: u8| {
            let mut ctx = AttackCamCtx {
                ramp: 100,
                latch,
                ..Default::default()
            };
            let mut a = probe_actor(CharacterArm::One, 0x1A);
            a.arm_select = 2;
            let p = attack_camera_framing(a, &mut ctx, &table)
                .expect("arm fires")
                .pose;
            (p.dist[2], ctx.ramp)
        };
        // Latch 0: the reset fires, so the `- ramp*2` term is zero.
        assert_eq!(run(0), (POSE_SEED_ANGLE + 3 * 16, 0));
        // Latch 1: no reset, so the ramp is still 100 and the term bites.
        assert_eq!(run(1), (POSE_SEED_ANGLE + 3 * 16 - 200, 100));
    }

    /// The pose the arms hand back keeps the actor's own position as the
    /// (negated) look-at, so the camera orbits whoever is swinging.
    #[test]
    fn the_framing_orbits_the_acting_actor() {
        let table = probe_table();
        let mut ctx = AttackCamCtx::default();
        let mut a = probe_actor(CharacterArm::Two, 0x1F);
        a.pos = [100, 200, 300];
        a.facing = 0x400;
        let f = attack_camera_framing(a, &mut ctx, &table).expect("arm fires");
        assert_eq!(f.pose.look_at, [-100, -200, -300]);
        // Seeded yaw is the negated facing, then the arm's own folds.
        assert_ne!(f.pose.rot[1], 0);
    }

    /// Every arm's duration is one of retail's three `a3` operands.
    #[test]
    fn every_arm_duration_is_one_three_or_six_frames() {
        let table = probe_table();
        for c in [CharacterArm::One, CharacterArm::Two, CharacterArm::Three] {
            for art in FIRST_ART..=0x2Du8 {
                for anim in [0i16, 0x50, 0x80, 0xB0, 0xF8, 0x120] {
                    let mut ctx = AttackCamCtx::default();
                    let mut a = probe_actor(c, art);
                    a.anim_frame = anim;
                    if let Some(f) = attack_camera_framing(a, &mut ctx, &table) {
                        assert!(
                            matches!(f.duration_frames, 1 | 3 | 6),
                            "{c:?} art {art:#x} anim {anim:#x} -> {}",
                            f.duration_frames
                        );
                    }
                }
            }
        }
    }
}
