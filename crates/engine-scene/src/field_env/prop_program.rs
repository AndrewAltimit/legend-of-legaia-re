//! A prop record's animation program: the commands decoded from its field-VM script.

use super::*;

/// One animation command a placed prop's bind script issues against `+0x62` /
/// `+0x6A`. The field VM's whole animation surface, as the retail prop records
/// use it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimCmd {
    /// `0x4C` nibble-4 sub-1 with a zero ramp: `actor+0x6A = max(1, value >> 1)`.
    Rate(i16),
    /// `0x2B <bit>`: `actor+0x62 |= 1 << bit`.
    SetBit(u8),
    /// `0x2C <bit>`: `actor+0x62 &= !(1 << bit)`.
    ClearBit(u8),
    /// `0x4C 0x35`: `actor+0x62 = (+0x62 & !REVERSE) | 0x20A` - restart at
    /// frame 0, one-shot, and hold. The rest pose of every closed door.
    ResetHold,
    /// `0x4C 0x36`: `actor+0x62 |= 0x28A` - restart at the *last* frame,
    /// reverse, one-shot, hold. The "already opened" snap (the shelf whose
    /// story flag says it has been searched).
    EndHold,
}

impl AnimCmd {
    /// Apply the command to a live prop.
    pub fn apply(self, a: &mut PropAnim) {
        match self {
            AnimCmd::Rate(v) => a.rate = (v >> 1).max(1),
            AnimCmd::SetBit(b) => a.flags |= 1u16.wrapping_shl(u32::from(b) & 0x1F),
            AnimCmd::ClearBit(b) => a.flags &= !1u16.wrapping_shl(u32::from(b) & 0x1F),
            AnimCmd::ResetHold => a.flags = (a.flags & !ANIM_REVERSE) | 0x020A,
            AnimCmd::EndHold => a.flags |= 0x028A,
        }
    }
}

/// A run of a prop script's animation commands, ending either at the script's
/// "wait until the clip finishes" spin (`0x2D` on bit 8 = [`ANIM_END`]) or at
/// the end of the pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnimSegment {
    /// Commands applied on entering the segment.
    pub cmds: Vec<AnimCmd>,
    /// True when the segment ends on the end-latch spin: the script parks here
    /// until the clip reaches its end, then runs the next segment.
    pub wait_for_end: bool,
}

/// A placed prop's animation program, decoded from its bind record's script.
///
/// The record is a field-VM script whose passes are delimited by the `0x21`
/// park opcode:
///
/// - **spawn**: `FUN_8003A55C` runs the record from its post-header PC up to
///   the first `0x21` (the retail loop only enters when the leading opcode is
///   `0x24`/`0x25`, which every posed prop record carries). That pass sets the
///   rate and issues `0x4C 0x35`, leaving the prop **held at frame 0** - a
///   closed door, a shut cupboard.
/// - **touch**: the next pass, which `FUN_801D5B5C` resumes when the player's
///   body hits the prop. For a house door that is `[clear REVERSE, clear HOLD,
///   set CLAMP, clear END]` then the end-latch spin: the clip plays forward and
///   clamps open. Rim Elm's cupboard adds a second segment after the spin -
///   `[set REVERSE, clear HOLD, set CLAMP]` - which plays the doors back shut.
///
/// Records with no animation commands in their touch pass (the locked drawer,
/// the clock) decode to an empty program and never move, exactly as in retail.
///
/// Beyond the `+0x62` animation surface, the spawn prologue's own-context
/// `0x31 <bit>` CFLAG_SET ops (writes into the actor flag word `+0x10`) carry
/// the prop's **collision / interaction class**, collected into
/// [`Self::spawn_cflags`]:
///
/// - bits `0`/`1` (`31 00`) - **collision exempt**: the collision candidate
///   list builder `FUN_801CF754` and the interact probe `FUN_801CF9F4` both
///   skip any actor whose `+0x10 & 3 != 0`.
/// - bit `30` (`31 1E`, Rim Elm's cupboards) or bit `17` - the
///   `flags & 0x40020000` class of `FUN_801CFC40`: contact returns result bit
///   `1` instead of `4`, which the locomotion dispatch (`FUN_801D01B0`
///   `0x801d0800`) does NOT auto-post - only the just-pressed-confirm facing
///   probe posts it. This is the **interact gate**: cupboards open on the
///   confirm button, doors on body contact.
/// - bit `17`/`24` - the `flags & 0x1020000` box-source class: the contact box
///   anchors at the live position with the moving-actor extents instead of the
///   record-derived footprint centre.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PropProgram {
    /// Commands the spawn pass issues.
    pub spawn: Vec<AnimCmd>,
    /// The touch pass, split at the end-latch spins.
    pub touch: Vec<AnimSegment>,
    /// Actor `+0x10` bits the spawn prologue sets via own-context `0x31`
    /// CFLAG_SET ops. `0` when the record has no `0x24`/`0x25` prologue marker
    /// (retail then skips the spawn run entirely).
    pub spawn_cflags: u32,
}

impl PropProgram {
    /// True when contact plays the clip - the doors, the cupboards, the shop
    /// sign. False for the props whose record only poses them once (the locked
    /// drawer) and for the ones that never stop (the windmill: its spawn pass
    /// issues no `0x4C 0x35`, so it keeps the template's looping
    /// [`ANIM_SPAWN_FLAGS`] and spins forever).
    pub fn animates(&self) -> bool {
        !self.touch.is_empty()
    }

    /// The `FUN_801CFC40` `flags & 0x40020000` class: contact yields result
    /// bit `1`, which the locomotion never auto-posts - the prop fires only
    /// from the button-gated facing probe. Rim Elm's cupboards (`31 1E`).
    pub fn interact_gated(&self) -> bool {
        self.spawn_cflags & 0x4002_0000 != 0
    }

    /// The `flags & 0x1020000` box-source class: the contact box anchors at
    /// the live position with the moving-actor extents (±40) instead of the
    /// static footprint centre (±80).
    pub fn moving_box(&self) -> bool {
        self.spawn_cflags & 0x0102_0000 != 0
    }

    /// Born collision-exempt: the spawn prologue sets `+0x10` bit `0`/`1`
    /// (`31 00`), so `FUN_801CF754`'s `flags & 3` filter never admits the
    /// actor to the collision candidate list.
    pub fn spawn_collision_off(&self) -> bool {
        self.spawn_cflags & 3 != 0
    }
}

/// The field-VM opcodes the prop-script decoder reacts to.
const OP_PARK: u8 = 0x21;
const OP_FLAG_SET: u8 = 0x2B;
const OP_FLAG_CLEAR: u8 = 0x2C;
const OP_FLAG_TEST: u8 = 0x2D;
/// Bit index of [`ANIM_END`] - what the "wait for the clip" spin tests.
const ANIM_END_BIT: u8 = 8;
/// The bits of `actor+0x62` the per-frame anim tick reads: hold, clamp,
/// reverse, end, restart. A `0x2B` / `0x2C` on any other bit is a per-actor
/// flag the animation does not see, so the decoder ignores it - which also
/// keeps a linear walk that drifts into a record's inline dialogue text from
/// synthesising phantom animation commands out of ASCII (a `,` in a message is
/// the byte `0x2C`).
const ANIM_BITS: &[u8] = &[1, 3, 7, 8, 9];

/// Decode a bind record's animation program. `record` is the partition-0
/// record's bytes, `pc0` the offset just past its `[u8 n][n*2 name][u8 anim]`
/// header (where `FUN_8003A55C` parks the actor's PC).
///
/// The **spawn** commands are the ones before the record's first `0x21` park -
/// exactly the run `FUN_8003A55C`'s prologue loop executes (it stops on that
/// opcode). Everything after it is the resumable body, scanned linearly for
/// animation commands and cut at each `0x2D 0x08` end-latch spin.
///
/// A body segment is only kept when it actually **plays** the clip, i.e. it
/// clears the hold bit (`0x2C 0x01`). That drops the flag-gated pose snaps
/// (`0x4C 0x36` on a shelf whose story flag says it has already been searched)
/// that a linear walk sees but a branch-following VM would only reach through a
/// story-flag test - they are alternate *spawn* states, not contact reactions -
/// and it makes the decode robust against the walk drifting through a record's
/// inline dialogue text.
///
/// PORT: FUN_8003A55C (the spawn-prologue run) + the field-VM animation ops
/// (`0x2B` / `0x2C` / `0x2D` on `actor+0x62`, `0x4C` nibble-3 sub-5/6, `0x4C`
/// nibble-4 sub-1)
pub fn decode_prop_program(record: &[u8], pc0: usize) -> PropProgram {
    decode_prop_program_with(record, pc0, None).0
}

/// [`decode_prop_program`] over the spawn pass [`run_spawn_prologue`] runs
/// against the live story flags (`flags`), plus the parked cursor that pass
/// leaves. `None` walks the prologue linearly (both arms of every branch).
pub fn decode_prop_program_with(
    record: &[u8],
    pc0: usize,
    flags: Option<&mut SpawnFlags<'_>>,
) -> (PropProgram, usize) {
    use legaia_scene_formats::field_disasm::{FlagKind, InsnInfo, LinearWalker, MenuCtrlKind};

    let anim_cmd = |insn: &legaia_scene_formats::field_disasm::Insn| -> Option<AnimCmd> {
        // Cross-context writes target a different actor's record, not this
        // prop's - retail's `0x2B`/`0x2C` write `iVar18 + 0x62`, the executing
        // context's own actor.
        if insn.extended.is_some() {
            return None;
        }
        match (insn.opcode, &insn.info) {
            (OP_FLAG_SET, InsnInfo::LFlag { bit, .. }) if ANIM_BITS.contains(bit) => {
                Some(AnimCmd::SetBit(*bit))
            }
            (OP_FLAG_CLEAR, InsnInfo::LFlag { bit, .. }) if ANIM_BITS.contains(bit) => {
                Some(AnimCmd::ClearBit(*bit))
            }
            (
                _,
                InsnInfo::MenuCtrl {
                    kind: MenuCtrlKind::Nibble3 { sub: 5 },
                    ..
                },
            ) => Some(AnimCmd::ResetHold),
            (
                _,
                InsnInfo::MenuCtrl {
                    kind: MenuCtrlKind::Nibble3 { sub: 6 },
                    ..
                },
            ) => Some(AnimCmd::EndHold),
            (
                _,
                InsnInfo::MenuCtrl {
                    kind:
                        MenuCtrlKind::Nibble4 {
                            sub: 1,
                            target,
                            ticks: 0,
                        },
                    ..
                },
            ) => Some(AnimCmd::Rate(*target)),
            _ => None,
        }
    };

    let mut prog = PropProgram::default();
    // The cursor the first touch resumes from, and the pass's flag writes,
    // are the flagged run's. The clip words and class bits are still read
    // off the linear walk, both arms of a branch together: a door whose
    // "already opened" arm sets `31 00` and `4C 36` therefore spawns
    // exempt and drawn open whatever its flag says, as it always has here.
    // Retail spawns a locked door solid (`dolk` P0[15..18], the Sunrise
    // and sister gates, test `0x270..0x273`), and the ladder's routes still
    // pass through those doors unopened, so that half stays as it was.
    let (_, parked) = run_spawn_prologue(record, pc0, flags);
    let (prologue, _) = run_spawn_prologue(record, pc0, None);
    for insn in &prologue {
        if let Some(c) = anim_cmd(insn) {
            prog.spawn.push(c);
        } else if insn.extended.is_none()
            && insn.opcode == 0x31
            && let InsnInfo::CFlag {
                kind: FlagKind::Set,
                bit,
            } = insn.info
        {
            // Actor `+0x10` class bits (see [`PropProgram::spawn_cflags`]).
            prog.spawn_cflags |= 1u32 << (bit & 0x1F);
        }
    }
    let mut seg = AnimSegment::default();
    let mut body: Vec<AnimSegment> = Vec::new();
    for insn in LinearWalker::new(record, parked).flatten() {
        // The end-latch spin closes the current segment.
        if insn.opcode == OP_FLAG_TEST
            && insn.extended.is_none()
            && matches!(
                insn.info,
                InsnInfo::LFlag {
                    kind: FlagKind::Test,
                    bit: ANIM_END_BIT
                }
            )
        {
            seg.wait_for_end = true;
            body.push(std::mem::take(&mut seg));
            continue;
        }
        if let Some(c) = anim_cmd(&insn) {
            seg.cmds.push(c);
        }
    }
    body.push(seg);
    /// `0x2C 0x01` - the command that un-holds the clip. A segment without it
    /// does not play anything.
    const PLAY: AnimCmd = AnimCmd::ClearBit(1);
    prog.touch = body
        .into_iter()
        .filter(|s| s.cmds.contains(&PLAY))
        .collect();
    (prog, parked)
}

/// Does this bind record's spawn pass leave the clip **free-running** - i.e.
/// looping forever with no script driving it?
///
/// This is the retail split between the windmill and the doors: every actor is
/// born with the looping [`ANIM_SPAWN_FLAGS`], and the spawn pass either parks
/// the clip (a door's `0x4C 0x35` reset-hold, a searched shelf's `0x4C 0x36`)
/// or leaves the template flags alone (the windmill, whose sails then spin
/// under the unconditional per-frame anim tick `FUN_800204F8` for as long as
/// the scene is loaded). Free-running means [`ANIM_HOLD`] and [`ANIM_CLAMP`]
/// are both still clear after the spawn commands run.
///
/// A record index that does not decode is reported free-running: an actor with
/// no runnable spawn prologue keeps the template flags, same as an unbound
/// placement.
pub fn prop_spawn_free_runs(man_file: &ManFile, man: &[u8], record: usize) -> bool {
    let Some((rec, pc0)) = partition0_record(man_file, man, record) else {
        return true;
    };
    let program = decode_prop_program(rec, pc0);
    let mut anim = PropAnim::spawned(1, 2, false, 0);
    for c in &program.spawn {
        c.apply(&mut anim);
    }
    anim.flags & (ANIM_HOLD | ANIM_CLAMP) == 0
}

/// What `FUN_8003A55C`'s spawn pass asks of the story flags: test, set or
/// clear system flag `idx` (the bitfield at `DAT_80085758`). The answer is the
/// test's; a write's is ignored.
pub type SpawnFlags<'a> = dyn FnMut(u16, legaia_scene_formats::field_disasm::FlagKind) -> bool + 'a;

/// The spawn pass `FUN_8003A55C` runs on a freshly placed object: the
/// instructions it executes, in order, and the cursor it parks at
/// (`actor+0x9E`, where the first touch resumes).
///
/// Retail enters only when the record's first opcode is the `0x24` / `0x25`
/// marker, then hands one instruction at a time to the field-VM dispatcher
/// `FUN_801DE840` (`0x8003A914..0x8003A95C`) and stops after it has executed
/// a `0x21`, when the dispatcher returns the cursor unchanged (a wait), or
/// when the next opcode byte is below `0x20` (`& 0x7F`). The pass is a real
/// run of the VM, so a story-flag test branches on the live flags and a flag
/// write lands: `korb3` P0[3], the gate stone in front of the east corridor,
/// clears `0x43E`, tests `0x403` and either holds itself shut and parks on
/// the `0x21` at `+0x2A` (the touch then opens the gate) or, with `0x403` up,
/// sets `0x43E` and parks in its idle loop. A linear walk took the second
/// arm's `0x21` on every visit, so the stone's touch resumed in the idle loop
/// and the gate never opened.
///
/// With `flags` the pass follows `0x26` jumps and the system-flag tests and
/// performs the system-flag writes; every other op is stepped over in place
/// (the prologues carry class bits, clip words and rates, which the caller
/// reads off the returned instructions). Without it, the prologue is walked
/// linearly to its first own-context `0x21`.
///
/// PORT: FUN_8003A55C (the spawn-prologue loop)
pub fn run_spawn_prologue(
    record: &[u8],
    pc0: usize,
    flags: Option<&mut SpawnFlags<'_>>,
) -> (Vec<legaia_scene_formats::field_disasm::Insn>, usize) {
    use legaia_scene_formats::field_disasm::{FlagKind, InsnInfo, LinearWalker, decode};
    if !matches!(record.get(pc0), Some(0x24) | Some(0x25)) {
        return (Vec::new(), pc0);
    }
    let mut ran = Vec::new();
    let Some(flags) = flags else {
        for insn in LinearWalker::new(record, pc0).flatten() {
            let park = insn.opcode == OP_PARK && insn.extended.is_none();
            let next = insn.pc + insn.size;
            ran.push(insn);
            if park {
                return (ran, next);
            }
        }
        return (ran, pc0);
    };
    let mut pc = pc0;
    // A prologue is a handful of ops; the bound only stops a jump cycle.
    for _ in 0..512 {
        let Ok(insn) = decode(record, pc) else {
            return (ran, pc);
        };
        let raw = record[pc];
        let mut next = pc + insn.size.max(1);
        if insn.extended.is_none() {
            match insn.info {
                InsnInfo::JmpRel { target, .. } => next = target,
                InsnInfo::SystemFlag {
                    kind: FlagKind::Test,
                    idx,
                    target: Some(target),
                    ..
                } => {
                    if flags(idx, FlagKind::Test) {
                        next = target;
                    }
                }
                InsnInfo::SystemFlag { kind, idx, .. } => {
                    flags(idx, kind);
                }
                _ => {}
            }
        }
        ran.push(insn);
        if raw == OP_PARK || next == pc {
            return (ran, next);
        }
        match record.get(next) {
            Some(&b) if b & 0x7F >= 0x20 => pc = next,
            _ => return (ran, next),
        }
    }
    (ran, pc)
}
