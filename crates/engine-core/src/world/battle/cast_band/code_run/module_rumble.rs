//! The slot-B modules' controller rumble stores, applied on the arm a module
//! tick ran (`legaia_engine_cast::cast_module_rumble`).

use super::*;

/// PROT 0922 (Puera): the arm whose exit zeroes the alternation word
/// (`sw zero,-0x5f44(v0)` at `0x801F7988`), the arm before the first
/// alternating one.
const PUERA_ALTERNATE_RESET_ARM: u8 = 13;

impl World {
    /// Apply every rumble store `entry`'s `body` makes on `arm` this tick,
    /// each behind the Battles vibration row as the module's own
    /// `lw ..,0x45c8` gate is. `passed` is whether the arm's gate let it
    /// through, `countdown` the module countdown the tick opened on.
    ///
    /// Ozma's spray draws its two `rand()`s only while the row is on - they
    /// sit inside the gated block (`0x801F6F10` / `0x801F6F40`), so retail's
    /// RNG stream differs between the two settings - while Puera's
    /// alternation word is bumped whatever the row says, ahead of its gate
    /// (`0x801F7A1C..0x801F7A24`).
    pub(in crate::world::battle::cast_band) fn apply_module_rumble(
        &mut self,
        entry: u32,
        body: u32,
        arm: u8,
        passed: bool,
        countdown: i32,
    ) {
        use legaia_engine_cast::cast_module_rumble::{RumbleStore, module_rumble_due, ozma_spray};
        use legaia_engine_system::rumble::BattleRumbleWrite;
        if entry == 922 && arm <= PUERA_ALTERNATE_RESET_ARM {
            self.casting.module_rumble_alternate = 0;
        }
        for m in module_rumble_due(entry, body, arm, passed, countdown) {
            let on = !self.battle_vibration_off();
            match m.store {
                RumbleStore::Write(w) => {
                    if on {
                        self.rumble.battle_write(w);
                    }
                }
                RumbleStore::Rates {
                    pulse_rate,
                    level_rate,
                } => {
                    if on {
                        let d = &mut self.rumble.battle_decay;
                        d.pulse_rate = pulse_rate;
                        d.level_rate = level_rate;
                        d.hold = 0;
                    }
                }
                RumbleStore::OzmaSpray => {
                    if on {
                        let r0 = self.next_rand();
                        let r1 = self.next_rand();
                        self.rumble.battle_write(ozma_spray(r0, r1));
                    }
                }
                RumbleStore::PueraAlternate { odd, even } => {
                    let old = self.casting.module_rumble_alternate;
                    self.casting.module_rumble_alternate = old.wrapping_add(1);
                    if on {
                        let v = if old & 1 != 0 { odd } else { even };
                        self.rumble.battle_write(BattleRumbleWrite::even(v, 4));
                    }
                }
            }
        }
    }
}
