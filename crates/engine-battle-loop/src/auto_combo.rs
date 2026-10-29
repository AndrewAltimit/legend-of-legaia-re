//! The Auto attack's per-battle state: the disc inputs the pool arm reads
//! and the queues it builds (`FUN_801F0450`'s pool arm; the arm itself is
//! `engine-core`'s `World::begin_round_execution`).

/// Per-roster-character inputs the pool arm and its tail read off the disc.
#[derive(Debug, Clone, Default)]
pub struct AutoComboInputs {
    /// The first four bytes of each direction command's action entry
    /// (`DAT_801C9360[slot][0xC + i]`, `i` = Left / Right / Down / Up) - the
    /// weight ladder's input. `None` until a disc read fills it, in which case
    /// every command takes the default weight.
    pub command_heads: Option<[[u8; 4]; 4]>,
    /// The art-animation bank's raw `0xD0`-stride records (record`0`
    /// `+0x58`), which the tail walks.
    pub art_records: Vec<[u8; 0xD0]>,
    /// The bank's count byte (`lbu v0,0x0(v0)` at `0x801F0B84`).
    pub art_count: u8,
}

/// The Auto attack's state across one battle.
#[derive(Debug, Clone, Default)]
pub struct AutoComboState {
    /// Disc inputs per roster character (Vahn / Noa / Gala).
    pub inputs: [AutoComboInputs; 3],
    /// The four direction commands' status-guard masks,
    /// `*(i16 *)(0x801F672C + i * 2)` in PROT 0898. `None` without a disc,
    /// which guards nothing.
    pub guards: Option<[u16; 4]>,
    /// `ctx[+0x266 + seat]` - the per-fighter Auto flag.
    pub flags: [bool; 3],
    /// The pick the command session made this frame, before it commits.
    pub pending: bool,
    /// The queue the round-start pool arm built per seat, played by the
    /// seat's dispatch.
    pub queues: [Option<Vec<u8>>; 3],
}
