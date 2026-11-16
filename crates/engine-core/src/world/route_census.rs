//! Field-VM routing census: which ops each script runner steps on a context
//! other than the one retail's dispatcher would hand the op.
//!
//! Retail has one runner shape: `FUN_80039B7C` (and its run-to-text sibling
//! `FUN_8003CF7C`) calls the dispatcher `FUN_801DE840` with the executing
//! actor's own context, and the dispatcher's prologue swaps an extended op's
//! resolved target into `s5` before the arm runs (`0x801DE894..0x801DE8B4`,
//! `FUN_8003C83C`). The port's runners - the system script, the placement and
//! object prologues, the talk runner, the prop runner and the spawned-record
//! slice - route an op onto its target case by case, and any op a runner does
//! not route runs on whatever context the runner stands on: the talk's or the
//! prop's stand-in, the record's own context, or the caller's.
//!
//! With [`crate::world::FieldVmState::route_census`] set, each runner notes
//! every op that reaches its fall-through step. A disc sweep reads the notes
//! back to find the routing gaps a shipped script actually reaches. Off (the
//! default), the hook costs one `Option` test per fall-through op.
//!
//! REF: FUN_801DE840 (0x801DE894..0x801DE8B4), FUN_8003C83C, FUN_80039B7C

/// The runner that stepped an op.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RouteRunner {
    /// The scene system script (`P1[0]`, context `0xFB`).
    System,
    /// A placement's spawn-section pre-run (`FUN_8003A1E4`).
    PlacementPrologue,
    /// An object bind's prologue (`FUN_8003A55C`).
    ObjectPrologue,
    /// A talk on a placement (the dialog SM on the talker's context).
    Talk,
    /// A touched object's record (the prop runner).
    Prop,
    /// A spawned partition-2 record or a touch-resumed interaction timeline.
    Spawned,
}

/// What the op's target byte names, as the port resolves it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RouteTarget {
    /// No target byte: the op acts on the executing context.
    Own,
    /// `0xF8`, the player.
    Player,
    /// `0xFB`, the system script.
    System,
    /// A placement channel (`P1` slot).
    Placement(u8),
    /// A placed-object channel (its object key).
    Object(u16),
    /// An id no channel carries.
    Unresolved(u8),
}

/// One fall-through step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteCensusHit {
    /// The runner.
    pub runner: RouteRunner,
    /// Runner-specific identity of the executing record: the talker's
    /// placement slot, the prop's object key, the spawned record's script id.
    pub who: Option<u16>,
    /// What the op names.
    pub target: RouteTarget,
    /// The op's PC in its record's bytecode.
    pub pc: usize,
    /// The op's first bytes (opcode, then target byte or operands).
    pub bytes: [u8; 4],
}

impl crate::world::World {
    /// Note one fall-through step for the routing census, when it is on.
    pub(crate) fn note_route_fallthrough(
        &mut self,
        runner: RouteRunner,
        who: Option<u16>,
        bytecode: &[u8],
        pc: usize,
    ) {
        if self.field_vm.route_census.is_none() {
            return;
        }
        let Some(&b) = bytecode.get(pc) else {
            return;
        };
        let target = if b & 0x80 == 0 {
            RouteTarget::Own
        } else {
            match legaia_engine_vm::field::peek_extended(bytecode, pc) {
                None => return,
                Some(0xF8) => RouteTarget::Player,
                Some(0xFB) => RouteTarget::System,
                Some(t) => {
                    let view = self.channel_view();
                    match legaia_engine_field::field_channels::resolve_target(view, t) {
                        Some(ci) if view[ci].object_bind => {
                            RouteTarget::Object(view[ci].placement_index as u16)
                        }
                        Some(ci) => RouteTarget::Placement(view[ci].placement_index as u8),
                        None => RouteTarget::Unresolved(t),
                    }
                }
            }
        };
        let mut bytes = [0u8; 4];
        for (i, slot) in bytes.iter_mut().enumerate() {
            *slot = bytecode.get(pc + i).copied().unwrap_or(0);
        }
        if let Some(census) = self.field_vm.route_census.as_mut() {
            census.push(RouteCensusHit {
                runner,
                who,
                target,
                pc,
                bytes,
            });
        }
    }
}
