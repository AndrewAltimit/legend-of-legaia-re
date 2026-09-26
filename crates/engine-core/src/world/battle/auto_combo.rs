//! The player's **Auto** attack - retail's pool arm of `FUN_801F0450` and its
//! art-insertion tail, run for a party member whose Auto flag is set.
//!
//! Retail's command SM writes the per-fighter flag `ctx[+0x266 + seat]` on
//! the attack-mode prompt: `1` on the `Auto` chip (`0x801D17D0`) or under the
//! `Automatic` option (`0x801D164C`, the option word stored as the flag), `0`
//! on `Command` (`0x801D1760`), and `0` every frame the ring is up
//! (`0x801D11A8`). The Begin confirm moves the flow to `0xFE`, whose arm writes
//! `ctx[7] = 0` (`0x801D3224`), and the action SM's state `0x00` opens with
//! `jal 0x801F0450` (`0x801E2AB8`) - so the pool arm runs once per round, for
//! every seat whose flag is set and whose category is Attack
//! (`0x801F0704..0x801F0730`). It **rebuilds** the seat's `+0x1DF` queue:
//! a weighted pool of the four direction commands, spent against the action
//! gauge, then the tail splices learned arts' arrow strings over it. The
//! saved command string the Attack confirm pre-seeded (`FUN_801DA34C`) is
//! what the review screen shows; the round runs the rebuilt one.
//!
//! The engine keeps the same order: the command flow records the flag
//! ([`World::note_auto_attack_pick`]), [`World::begin_round_execution`] runs
//! the pool arm for every flagged Attack commit and parks the result, and the
//! member's dispatch plays the parked queue instead of re-seeding
//! ([`World::take_auto_attack_queue`]). The RNG draws happen at the round's
//! start in seat order, as retail's do.
//!
//! The delegated arm of the same routine (record `+0xF8 & 0x2000`) stays
//! where it was, `battle_action::dispatch::auto_fill_party_queues`; a seat it
//! takes is not a pool-arm seat.
//!
//! REF: FUN_801F0450 (pool arm `0x801F06D8..0x801F0B48`; the kernels carry the `PORT:` tags)

use super::*;

/// Per-roster-character inputs the pool arm and its tail read off the disc.
#[derive(Debug, Clone, Default)]
pub struct AutoComboInputs {
    /// The first four bytes of each direction command's action entry
    /// (`DAT_801C9360[slot][0xC + i]`, `i` = Left / Right / Down / Up) - the
    /// weight ladder's input. `None` until a disc read fills it, in which case
    /// every command takes the default weight.
    pub command_heads: Option<[[u8; 4]; 4]>,
    /// The art-animation bank's raw `0xD0`-stride records (record[0]
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

impl World {
    /// Record the Auto / Command pick a command-session frame implies, from
    /// the phase before and after it.
    pub(in crate::world) fn note_auto_attack_pick(
        &mut self,
        before: &crate::battle_input::CommandPhase,
        after: &crate::battle_input::CommandPhase,
    ) {
        use crate::battle_input::{BattleCommand, CommandPhase as P};
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
            P::AttackMode { .. } if attack_target => self.battle.auto_combo.pending = true,
            P::AttackMode { .. } if matches!(after, P::OpenArtsMenu) => {
                self.battle.auto_combo.pending = false
            }
            // The ring's Attack arm under the `Automatic` / `Command`
            // options skips the prompt; the option word is the flag
            // (`0x801D164C`).
            P::Menu { .. } if attack_target => self.battle.auto_combo.pending = true,
            P::Menu { .. } => self.battle.auto_combo.pending = false,
            _ => {}
        }
    }

    /// Commit the pending pick as `actor`'s flag - the Attack confirm.
    pub(in crate::world) fn commit_auto_attack_flag(&mut self, actor: u8, auto: bool) {
        if let Some(f) = self.battle.auto_combo.flags.get_mut(usize::from(actor)) {
            *f = auto;
        }
        self.battle.auto_combo.pending = false;
    }

    /// Run the pool arm for every flagged Attack commit at the round's start
    /// and park each queue for the seat's dispatch.
    pub(in crate::world) fn run_auto_attack_pool_arms(&mut self) {
        use crate::battle_round::PendingPartyAction;
        let party = self.party.party_count.clamp(1, 3);
        self.battle.auto_combo.queues = Default::default();
        for seat in 0..party {
            let flagged = self
                .battle
                .auto_combo
                .flags
                .get(usize::from(seat))
                .copied()
                .unwrap_or(false);
            let target = match self.battle.round_flow.pending.get(usize::from(seat)) {
                Some(Some(PendingPartyAction::Attack { target })) => *target,
                _ => continue,
            };
            if !flagged {
                continue;
            }
            let queue = self.auto_attack_pool_queue(seat, target);
            if let Some(q) = self.battle.auto_combo.queues.get_mut(usize::from(seat)) {
                *q = Some(queue);
            }
        }
    }

    /// The parked Auto queue for `actor`, if the round's start built one.
    pub(in crate::world) fn take_auto_attack_queue(&mut self, actor: u8) -> Option<Vec<u8>> {
        self.battle
            .auto_combo
            .queues
            .get_mut(usize::from(actor))
            .and_then(Option::take)
    }

    /// The pool arm and the tail for one seat: the queue bytes, direction
    /// commands with arts spliced in, unterminated.
    fn auto_attack_pool_queue(&mut self, seat: u8, target: u8) -> Vec<u8> {
        use crate::arts_command_input::{DEFAULT_POOL, FAVORED_COST};
        use legaia_engine_vm::battle_arts_auto_combo as combo;
        let roster = self.party_roster_slot(usize::from(seat));
        let inputs = self
            .battle
            .auto_combo
            .inputs
            .get(roster)
            .cloned()
            .unwrap_or_default();
        let costs = self
            .battle
            .swing_costs
            .get(roster)
            .copied()
            .unwrap_or([FAVORED_COST; 4]);
        let guards = self.battle.auto_combo.guards.unwrap_or([0; 4]);
        let commands: Vec<combo::ArtsCommand> = (0..4)
            .map(|i| combo::ArtsCommand {
                id: combo::FIRST_COMMAND + i as u8,
                cost: costs[i].min(0xFF) as u8,
                bytes: inputs.command_heads.map(|h| h[i]).unwrap_or_default(),
                guard: guards[i],
            })
            .collect();
        let status = self
            .actors
            .get(usize::from(seat))
            .map(|a| a.battle.field_flags)
            .unwrap_or(0);
        let family = combo::weight_family(self.attack_swing_class_of(target));
        let pool = combo::build_candidate_pool(&commands, family, status);
        // The action gauge `+0x154`, read the way the Arts command input
        // reads it, so the two paths price a round identically.
        let gauge = self
            .party
            .roster
            .members
            .get(roster)
            .map(|r| r.live_stats().agl)
            .filter(|&a| a > 0)
            .unwrap_or(DEFAULT_POOL);
        let spent = {
            let mut rng = || self.next_rand() as i32;
            combo::spend_gauge(
                &pool,
                gauge.min(i16::MAX as u16) as i16,
                |id| {
                    commands
                        .iter()
                        .find(|c| c.id == id)
                        .map(|c| c.cost)
                        .unwrap_or(0xFF)
                },
                &mut rng,
            )
        };
        let mut queue = spent.queue;
        let (learned, ability_high) = match self.party.roster.members.get(roster) {
            Some(rec) => {
                let skills = rec.displayed_skills();
                let n = (skills.count as usize).min(skills.ids.len());
                let bits = rec.ability_bits();
                (
                    skills.ids[..n].to_vec(),
                    u32::from_le_bytes([bits[4], bits[5], bits[6], bits[7]]),
                )
            }
            None => (Vec::new(), 0),
        };
        let spirit = self.spirit_gauge(seat);
        let marker = self.miracle_marker_armed_for(roster as u8);
        let input = combo::ArtsTailInput {
            char_id: roster as u8 + 1,
            spirit,
            ability_high,
            learned: &learned,
            miracle_marker: marker,
            records: &inputs.art_records,
            art_count: inputs.art_count,
        };
        let mut rng = || self.next_rand() as i32;
        combo::insert_arts(&mut queue, &input, &mut rng);
        queue
    }
}
