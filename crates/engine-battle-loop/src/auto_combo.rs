//! The Auto attack's per-battle state: the disc inputs the pool arm reads
//! and the queues it builds (`FUN_801F0450`'s pool arm; the arm itself is
//! `engine-core`'s `World::begin_round_execution`).

use crate::battle_state::BattleState;

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

impl BattleState {
    /// Record the Auto / Command pick a command-session frame implies, from
    /// the phase before and after it.
    pub fn note_auto_attack_pick(
        &mut self,
        before: &legaia_engine_battle_menus::battle_input::CommandPhase,
        after: &legaia_engine_battle_menus::battle_input::CommandPhase,
    ) {
        use legaia_engine_battle_menus::battle_input::{BattleCommand, CommandPhase as P};
        let attack_target = matches!(
            after,
            P::Targeting {
                command: BattleCommand::Attack,
                ..
            } | P::Confirmed {
                command: BattleCommand::Attack,
                ..
            }
        );
        match before {
            // The prompt's `Auto` chip (`0x801D17D0`) or `Command`
            // (`0x801D1760`).
            P::AttackMode { .. } if attack_target => self.auto_combo.pending = true,
            P::AttackMode { .. } if matches!(after, P::OpenArtsMenu) => {
                self.auto_combo.pending = false
            }
            // The ring's Attack arm under the `Automatic` / `Command`
            // options skips the prompt; the option word is the flag
            // (`0x801D164C`).
            P::Menu { .. } if attack_target => self.auto_combo.pending = true,
            P::Menu { .. } => self.auto_combo.pending = false,
            _ => {}
        }
    }

    /// Commit the pending pick as `actor`'s flag - the Attack confirm.
    pub fn commit_auto_attack_flag(&mut self, actor: u8, auto: bool) {
        if let Some(f) = self.auto_combo.flags.get_mut(usize::from(actor)) {
            *f = auto;
        }
        self.auto_combo.pending = false;
    }

    /// The parked Auto queue for `actor`, if the round's start built one.
    pub fn take_auto_attack_queue(&mut self, actor: u8) -> Option<Vec<u8>> {
        self.auto_combo
            .queues
            .get_mut(usize::from(actor))
            .and_then(Option::take)
    }
}
