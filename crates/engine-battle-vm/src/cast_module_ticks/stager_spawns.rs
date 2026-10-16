//! The spawn arms of the slot-B stagers that a module's own **parts** call.
//!
//! A part's move program reaches its module's stager through op `0x20`
//! (`(*gp[0x714])(part, op[1], op[2])`, the module's `0x801F6734` row), and
//! the stager's arm seats more records off the calling part: its position
//! `+0x14` and rotation `+0x24` (`a0 = part + 0x14`, `a1 = part + 0x24`), or
//! a seat the arm reads. Some arms write into the part they just seated
//! (`FUN_80021B04` returns it in `v0`), after the seater's own move-VM run
//! has set the part up; one rewrites a record's bytecode before seating it.
//!
//! A module's stager arms are pure: each returns what it seats
//! ([`StagerSpawn`]) and the host seats it.

/// What a stager arm reads off the part that called it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StagerCaller {
    /// `+0x14..+0x18`.
    pub pos: [i16; 3],
    /// `+0x24..+0x28`.
    pub rot: [i16; 3],
    /// `+0xB4`, the caller's inner radius word (op `0x13`'s `v10`, ramped by
    /// the part tick).
    pub radius_b4: i16,
    /// Op `0x20`'s second operand, the stager's third argument (`a2`).
    pub arg: i16,
}

/// One `FUN_80021B04(pos, rot, record, 0x1000)` call out of a stager arm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagerSpawn {
    /// The record, by slot-B VA.
    pub record: u32,
    pub pos: [i16; 3],
    pub rot: [i16; 3],
    /// Halfword stores into the seated part after its seat run, as
    /// `(actor offset, value)`.
    pub pokes: Vec<(u16, u16)>,
    /// Halfword stores into the record's own bytes before it is seated, as
    /// `(VA, value)`.
    pub patches: Vec<(u32, u16)>,
    /// Halve the seated part's own speed `+0x98` after its seat run
    /// (`lhu; sll 0x10; sra 0x10; srl 0x1F; addu; sra 1`: a signed halving
    /// rounding toward zero).
    pub halve_speed: bool,
}
