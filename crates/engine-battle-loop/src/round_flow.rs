//! The live loop's round state: which band of the round the battle is in
//! and the party's committed commands.

/// Which band of retail's round the live battle is in - the coarse split of
/// the command-flow byte `ctx[+0x06]` that the turn cycling keys on.
///
/// Retail runs a round as two bands that never overlap. The **command band**
/// (`0x14..=0x78`, `FUN_801D0748`): `0x14` runs the actor sweep
/// (`FUN_801D88CC`) and the initiative seeder (`FUN_801DA780`, `0x801D0ED8`)
/// and opens `Begin | Run`; every living party member then enters a command
/// while the action SM idles. The **execution band** (`0xFE` / `0xFF`):
/// `0x6E`'s begin arm is the only writer of `0xFE` (`0x801D31AC`), `0xFE` is
/// the only writer of `ctx[+0x07] = 0` (`0x801D3224`), and from there
/// `FUN_801E295C` dispatches every combatant - party and monster - by the
/// max-key pick `FUN_801DABA4`, consuming each key at its `0x0C` dispatch
/// (`sh zero,0x16c` at `0x801E2CDC`). So nothing acts before the last member
/// commits, whoever won initiative.
///
/// REF: FUN_801D0748 (states `0x14`, `0x6E`, `0xFE`)
/// REF: FUN_801E295C (state `0x0C` dispatch, the `0x5A` re-pick, `0xFF` round end)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RoundPhase {
    /// Battle entry, before the first round start (`0x00..=0x0B`).
    #[default]
    Open,
    /// The command band: party members are entering this round's commands.
    Command,
    /// The execution band: the action SM owns the round.
    Execute,
}

/// A party member's committed command, held from its ring commit until the
/// action SM dispatches that member.
///
/// Retail parks the commit in the actor record (`+0x1DE` category, `+0x1DD`
/// target, `+0x1DF..` stream) and the per-member state byte
/// `_DAT_8007BD10[slot] = 4`; the engine's Item / Magic / Arts submenus
/// resolve more than those bytes carry (a spell id, an entered art profile,
/// the healed slots), so the commit is kept typed and the bytes are written at
/// dispatch, where retail's own per-action seeders (`FUN_801EED1C` for the
/// swing stream) run.
#[derive(Debug, Clone)]
pub enum PendingPartyAction {
    /// The plain strike (category `3`) against `target`, an engine actor slot.
    Attack { target: u8 },
    /// A Tactical-Arts entry (category `3`): the entered direction string,
    /// built into retail's action queue at the dispatch.
    Art {
        sequence: Vec<u8>,
        target_row: legaia_engine_battle::target_picker::CursorRow,
        target_slot: u8,
    },
    /// A Ra-Seru spell (category `2`).
    Spell {
        spell_id: u8,
        target_row: legaia_engine_battle::target_picker::CursorRow,
        target_slot: u8,
    },
    /// An item (category `1`). The copy is consumed at the commit, as retail's
    /// item window does (the `0x6E` step-back and the dead-actor sweep refund
    /// it through `FUN_800421D4`); the effect applies at dispatch.
    Item { item_id: u8, used_slots: Vec<u8> },
    /// Spirit (category `4`). The guard stance is up from the commit.
    Spirit,
    /// Run (category `5`). Retail's `0x32` confirm stamps it on every party
    /// actor (`0x801D1174..0x801D1184`) and starts the round at once.
    Run,
    /// No action (category `0`): the dispatch goes straight to the Done band.
    StandBy,
}

/// The live loop's round state: which band it is in, the party's committed
/// commands, and the member cursor.
#[derive(Debug, Clone, Default)]
pub struct RoundFlow {
    /// Which band of the round the battle is in.
    pub phase: RoundPhase,
    /// Per party slot: the command committed this round, `None` until the
    /// member commits (retail `_DAT_8007BD10[slot] != 4`) and again once it
    /// has been dispatched.
    pub pending: [Option<PendingPartyAction>; 3],
    /// `ctx[+0x13]` - the party member currently entering a command; the
    /// member walk (`FUN_801DB81C`) scans forward from it.
    pub cursor: u8,
    /// The no-SPD walk's last pick this round (`None` = start from slot 0).
    /// Only a battle with no SPD anywhere - the synthetic catalog, the
    /// disc-free tests - walks flat turn tokens in slot order; retail has no
    /// such case (every key is at least `1`), so the order is the port's own:
    /// party first, then monsters, once per round.
    pub flat_walk_last: Option<u8>,
    /// The special-battle run arm's `ctx[+0x274] = 0` (`FUN_801D0748`,
    /// `0x801D3284`): the round's first dispatch goes to the leader instead of
    /// the initiative pick. Consumed by that dispatch.
    pub leader_first: bool,
}

impl RoundFlow {
    /// Drop every committed command (the round is over, or a new one starts).
    pub fn clear_pending(&mut self) {
        for p in self.pending.iter_mut() {
            *p = None;
        }
    }

    /// `true` once `slot` has committed this round's command.
    pub fn committed(&self, slot: u8) -> bool {
        self.pending
            .get(usize::from(slot))
            .is_some_and(Option::is_some)
    }
}
