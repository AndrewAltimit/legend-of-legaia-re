//! The image child's clock for a [`BattleDrive`]: where the drive starts and
//! which tick of its own each press lands on, so a `play-window` capture
//! child presses on the battle ticks the headless seed presses on.
//!
//! The headless seed (`run_engine_battle` in `legaia-parity`) runs the
//! opening to the first prompt - past the battle camera's entry sweep, on the
//! first tick whose command flow has left `Idle` - and starts the drive
//! there: it arms the drive's one-shot world seed, seeds the capture's landed
//! melee hits, and counts its ticks from `0`, reading each tick's press off
//! the world before the steer runs. [`BattleDrive::pad_word_at`] presses on
//! even ticks of that count. A child that counted on its own window tick
//! pressed one tick late whenever the prompt opened on an odd window tick,
//! and the fight dealt its draws onto other ticks from there.

use crate::battle_drive::{BattleDrive, entry_sweep_reached};
use crate::melee_hits::{MeleeHitSeed, seed_melee_hits};
use legaia_engine_battle_menus::battle_flow::BattleFlowState;
use legaia_engine_mode::mode::SceneMode;

/// Where a child's drive stands: whether the entry sweep has been seen over,
/// and the host tick the drive started on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DriveClock {
    swept: bool,
    start: Option<u64>,
}

impl DriveClock {
    /// The host tick the drive started on, once it has.
    pub fn start(&self) -> Option<u64> {
        self.start
    }

    /// This tick's pad word under `drive`, read before the tick runs, with
    /// `tick` the host's own tick counter: `0` until the drive starts (and
    /// outside battle), then the drive's press on its own count, `0` once
    /// the phase is reached. The drive's steer runs after the press is read,
    /// as the headless seed orders them. An opening capture's drive starts
    /// on its first battle tick.
    pub fn pad(
        &mut self,
        drive: &BattleDrive,
        world: &mut legaia_engine_core::world::World,
        melee_hits: &MeleeHitSeed,
        tick: u64,
    ) -> u16 {
        if world.mode != SceneMode::Battle {
            return 0;
        }
        if self.start.is_none() {
            if !self.swept && entry_sweep_reached(world, 0xFF) {
                self.swept = true;
            }
            let opens = matches!(drive, BattleDrive::Opening { .. })
                || self.swept && world.battle.flow != BattleFlowState::Idle;
            if !opens {
                return 0;
            }
            self.start = Some(tick);
            drive.prime(world);
            seed_melee_hits(world, melee_hits);
        }
        let start = self.start.unwrap_or(tick);
        let pad = if drive.reached(world) {
            0
        } else {
            drive.pad_word_at(world, tick.saturating_sub(start))
        };
        drive.steer(world);
        pad
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle_drive::BattleDrive;

    fn menu() -> BattleDrive {
        BattleDrive::Menu {
            flow: BattleFlowState::ArtsCommandEntry,
            seat: 0,
            spirit: 0,
        }
    }

    #[test]
    fn a_drive_does_not_start_outside_battle_or_under_the_entry_sweep() {
        let mut world = legaia_engine_core::world::World::new();
        let mut clock = DriveClock::default();
        assert_eq!(
            clock.pad(&menu(), &mut world, &MeleeHitSeed::default(), 3),
            0
        );
        assert_eq!(clock.start(), None);
        // In battle with no camera the entry sweep never reads over, so a
        // drive into a running fight waits.
        world.mode = SceneMode::Battle;
        world.battle.flow = BattleFlowState::TurnPrompt;
        assert_eq!(
            clock.pad(&menu(), &mut world, &MeleeHitSeed::default(), 4),
            0
        );
        assert_eq!(clock.start(), None);
    }

    #[test]
    fn an_opening_drive_starts_on_its_first_battle_tick_and_keeps_that_start() {
        let mut world = legaia_engine_core::world::World::new();
        world.mode = SceneMode::Battle;
        let drive = BattleDrive::Opening {
            swept: false,
            entry: 0x40,
        };
        let mut clock = DriveClock::default();
        clock.pad(&drive, &mut world, &MeleeHitSeed::default(), 7);
        clock.pad(&drive, &mut world, &MeleeHitSeed::default(), 8);
        // The count is the drive's own: an odd host tick is its tick 0.
        assert_eq!(clock.start(), Some(7));
    }
}
