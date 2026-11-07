//! PROT 0942 `0xAA` (Dark Typhoon)'s party-row moves: the party drawn into
//! the typhoon round the caster and lifted, then let down.
//!
//! See `ghidra/scripts/funcs/overlay_cast_power_up_0942_801f69f4.txt`.

use crate::world::World;
use legaia_engine_system::trig::{bearing12, trig12};

/// `(a * b) >> 12` toward zero (`bgez; addiu 0xFFF; sra 12`).
fn mul12(a: i32, b: i32) -> i32 {
    (a * b) / 4096
}

impl World {
    /// The swirl writes of Dark Typhoon's arm `phase_in` for the tick that
    /// just ran (`passed`: the arm's countdown gate opened):
    ///
    /// - arm 2's pass copies each party seat's plane pair into the mark table
    ///   `0x801F88C4` (`0x801F708C..0x801F70F8`);
    /// - arm 3's pass clears both swirl words (`0x801F725C..0x801F726C`);
    /// - arm 4, every pass: the pull grows by `2 * step` and the spin by the
    ///   pull (`0x801F744C..0x801F7474`). Each party seat with HP takes the
    ///   bearing and the sin / cos weighted reach from its mark to the
    ///   caster (`FUN_80019B28`, the SCUS tables `0x8007B81C` / `0x8007B7F8`),
    ///   draws the reach in by half the pull down to `0x200`, turns the
    ///   bearing by the spin, and stands that far from the caster; its facing
    ///   turns by twice the pull and its height is minus half of it
    ///   (`0x801F7488..0x801F76AC`);
    /// - arm 5, every pass: each seat's height climbs back by `step * rate`,
    ///   no higher than the floor (`0x801F7AA8..0x801F7B10`); its pass stands
    ///   every seat on the floor (`0x801F7B6C..0x801F7B94`).
    pub(in crate::world) fn dark_typhoon_party_swirl(
        &mut self,
        caster_slot: u8,
        phase_in: u8,
        passed: bool,
    ) {
        let party = usize::from(self.party.party_count).min(4);
        match phase_in {
            2 if passed => {
                for i in 0..party {
                    let m = &self.actors[i].move_state;
                    self.casting.dark_typhoon_marks[i] = (m.world_x, m.world_z);
                }
            }
            3 if passed => self.casting.dark_typhoon_swirl = (0, 0),
            4 => {
                let Some(c) = self.actors.get(usize::from(caster_slot)) else {
                    return;
                };
                let (cx, cz) = (c.move_state.world_x, c.move_state.world_z);
                let (pull, spin) = &mut self.casting.dark_typhoon_swirl;
                *pull += 2;
                *spin += *pull;
                let (pull, spin) = (*pull, *spin);
                for i in 0..party {
                    if self.actors[i].battle.hp == 0 {
                        continue;
                    }
                    let (mx, mz) = self.casting.dark_typhoon_marks[i];
                    // `FUN_80019B28(mark z, mark x, caster z, caster x)`.
                    let bearing =
                        bearing12(i32::from(cz) - i32::from(mz), i32::from(cx) - i32::from(mx));
                    let (sin, cos) = trig12(bearing);
                    let mut reach = mul12(i32::from(sin), (i32::from(mx) - i32::from(cx)).abs())
                        .abs()
                        + mul12(i32::from(cos), (i32::from(mz) - i32::from(cz)).abs()).abs();
                    if reach > 0x200 {
                        reach = (reach - pull / 2).max(0x200);
                    }
                    let turned = (i32::from(bearing) + spin) & 0xFFF;
                    let (sin, cos) = trig12(turned as u16);
                    let a = &mut self.actors[i];
                    a.move_state.world_x = (i32::from(cx) - mul12(i32::from(sin), reach)) as i16;
                    a.move_state.world_z = (i32::from(cz) - mul12(i32::from(cos), reach)) as i16;
                    a.battle.facing_angle =
                        (i32::from(a.battle.facing_angle) + 2 * pull) as u16 & 0xFFF;
                    a.move_state.world_y = (-pull / 2) as i16;
                }
            }
            5 => {
                let s = i32::from(legaia_engine_battle_vm::battle_anim_rate::RATE_NORMAL);
                for i in 0..party {
                    let a = &mut self.actors[i];
                    a.move_state.world_y = if passed {
                        0
                    } else {
                        (i32::from(a.move_state.world_y) + s).min(0) as i16
                    };
                }
            }
            _ => {}
        }
    }
}
