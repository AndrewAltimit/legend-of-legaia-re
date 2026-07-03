//! Field VM result / parameter value types surfaced by the [`FieldHost`]
//! trait methods. Split out of `field.rs`.

/// Tristate result for [`FieldHost::op4c_n_8_sub_d_actor_search`] (op 0x4C
/// n8 sub-D).
///
/// The dispatcher's three-way branch maps:
/// - [`EmptySlot`](Self::EmptySlot): the per-character actor sub-table is
///   empty. The dispatcher takes the standard "advance PC by 6" tail.
/// - [`Found`](Self::Found): a matching marker was located. The dispatcher
///   takes the absolute-jump path (`LE_u16(operand+3..=operand+4)`).
/// - [`NoMatch`](Self::NoMatch): the sub-table is non-empty but no entry
///   matched the marker. The dispatcher halts at the current PC via the
///   `switchD::default` fallthrough.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActorSearchResult {
    /// Per-character sub-table is empty - advance PC.
    #[default]
    EmptySlot,
    /// Marker matched - take the absolute-jump path.
    Found,
    /// Sub-table populated but no match - halt at PC.
    NoMatch,
}

/// Camera parameter for op 0x45 CONFIGURE. The bit index in the mask
/// determines the parameter slot (0..=9, MSB-first per the original); the
/// value is a `u16` read from the operand stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CameraParam {
    pub slot: u8,
    pub value: u16,
}

/// Story-flag-driven dispatch path for op 0x4C outer-nibble-4 sub-9.
///
/// The original VM branches on two bits of `_DAT_1F800394`. See
/// [`FieldHost::op4c_n4_sub9_state`] for the bit table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sub9State {
    /// Bits `0x02000000` clear, `0x01000000` clear - default path. Write or
    /// ramp `_DAT_801C6EA4 + 0x4A` only.
    #[default]
    Default,
    /// Bits `0x02000000` clear, `0x01000000` set - absolute jump. The VM
    /// returns the signed-16 operand as the new PC and bypasses the
    /// per-tick logic entirely.
    AbsJump,
    /// Bit `0x02000000` set - delta path. The host writes/ramps both the
    /// target slot AND a delta-accumulator at `_DAT_8007BCAC`.
    Delta,
}

/// Outcome of [`FieldHost::scene_fade`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneFadeResult {
    /// Fade started or completed - VM advances past the instruction.
    Done,
    /// Scene is busy with a previous transition - hold at current PC, the
    /// runtime will retry next tick.
    Busy,
}

/// Tristate for op 0x49 STATE_RESUME. Mirrors `_DAT_8007B450` in the
/// original (idle = 0, done = 1, anything else = currently armed). Hosts
/// World coordinates copied onto a script context by Op 0x4C sub-3 sub-7
/// ([`FieldHost::fetch_player_coords`]). All values are at-rest player ctx
/// fields - the VM doesn't transform them before assigning.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlayerCoords {
    /// `+0x14` on the player ctx.
    pub world_x: u16,
    /// `+0x16` on the player ctx.
    pub world_y: u16,
    /// `+0x18` on the player ctx.
    pub world_z: u16,
    /// `+0x26` on the player ctx.
    pub field_26: u16,
}

/// implement [`FieldHost::op49_state`] to surface their internal state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Op49State {
    /// `_DAT_8007B450 == 0`. The script can arm a fresh resume.
    #[default]
    Idle,
    /// `_DAT_8007B450` holds a PC pointer. The VM halts at the same PC and
    /// the host advances the underlying state machine.
    Armed,
    /// `_DAT_8007B450 == 1`. The previous arm completed; the script's
    /// 0x49 sub-op fires and the slot is cleared.
    Done,
}
