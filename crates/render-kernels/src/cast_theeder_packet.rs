//! PROT 0904's (Theeder, spell `0x82`) per-tick draw packet: what the
//! module's tick body (`legaia_engine_cast::seru_magic_ticks`, `TheederFx`)
//! records each tick and the hosts turn into screen primitives through
//! `legaia_engine_ui::cast_theeder`.
//!
//! The packet sits here, below both the tick body and the draw-list builder,
//! so neither crate names the other.

/// One tick's PROT 0904 packets, in retail battle space (Y down). The hosts
/// project the points and build the primitives with
/// `legaia_engine_ui::cast_theeder`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TheederPacket {
    /// Arm 9: two `FUN_801F815C` lightning prongs from the root to `tips`.
    Prongs {
        mouth: [i16; 3],
        tips: [[i16; 3]; 2],
        cells: [u8; 2],
    },
    /// Arm 11's hold: `FUN_801F83A4`'s charge beam at brightness `level`.
    Charge {
        mouth: [i16; 3],
        tip: [i16; 3],
        level: u16,
    },
    /// Arms 11 (exit) and 12: `FUN_801F8634` - the fan over the first
    /// `drawn` trail pairs plus the beam core from the root to `tip`.
    Sweep {
        mouth: [i16; 3],
        tip: [i16; 3],
        drawn: usize,
    },
    /// Arm 13: `FUN_801F8B84` - the fan over the first `drawn` pairs, no core.
    Retract { mouth: [i16; 3], drawn: usize },
}
