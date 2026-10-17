//! PROT 0906 (Gizam) and PROT 0912 (Freed)'s effect parts: the spawns their
//! tick bodies and stagers make, and the parts Freed's arm 4 moves and kills.

use super::*;
use legaia_engine_cast::cast_module_camera::{self as cam, ModuleSeat};

impl World {
    /// The summon seat's live trio, or where the module last placed it.
    pub(in crate::world::battle::cast_band) fn module_creature_seat(&self) -> Option<ModuleSeat> {
        self.casting
            .summon_actor_slot
            .and_then(|s| self.actors.get(usize::from(s)))
            .filter(|a| a.active)
            .map(|a| ModuleSeat {
                x: a.move_state.world_x,
                y: a.move_state.world_y,
                z: a.move_state.world_z,
                facing: a.battle.facing_angle & 0xFFF,
            })
            .or(self.casting.module_cam.creature)
    }

    /// What PROT 0912's tick body does to its effect parts, after the body
    /// has run arm `phase_in` and left the phase at `phase_out`: the spawn
    /// calls of the arm it passed ([`cam::freed_arm_spawns`]), arm 3's pass
    /// zeroing the crystal turn word, arm 4's held passes turning the four
    /// crystal triples in round the creature, and arm 4's pass killing them
    /// (`ori 0x8` into each of the twelve handles at `0x801F92B0`,
    /// `0x801F7158..0x801F7174`).
    ///
    /// Arm 2 seats the creature first (`jal 0x801F19EC`), on the caster; its
    /// spawns read that seat, and the arm stores the creature's heading `0`
    /// (`sh zero,0x46(s2)` at `0x801F6C54`).
    pub(crate) fn freed_body_effects(&mut self, phase_in: u8, phase_out: u8, caster_slot: u8) {
        let passed = phase_out != phase_in;
        if passed && phase_in == 2 && self.module_creature_seat().is_none() {
            self.casting.module_cam.creature =
                self.actors
                    .get(usize::from(caster_slot))
                    .map(|a| ModuleSeat {
                        x: a.move_state.world_x,
                        y: a.move_state.world_y,
                        z: a.move_state.world_z,
                        facing: 0,
                    });
        }
        if passed && phase_in == 3 {
            self.casting.module_freed_turn = 0;
        }
        // Arm 7's pass points the creature's node at deformation row 1
        // (`sb s8,0x6d(v0)`, `s8 = 1`) and writes that row of `0x8007BE60`:
        // offset `(0, 0, 0)`, `+6 = -0x400`, flags `0xC`
        // (`0x801F7500..0x801F7528`), so the shatter its `+0x1BA` starts is
        // that row's, not the default spin and push.
        if passed && phase_in == 7 {
            self.npcs.face_rows.insert(
                cam::FREED_SHATTER_ROW,
                crate::world::FieldFaceRow {
                    offset: cam::FREED_SHATTER_ROW_OFFSET,
                    flags: cam::FREED_SHATTER_ROW_FLAGS,
                },
            );
            if let Some(a) = self
                .casting
                .summon_actor_slot
                .and_then(|s| self.actors.get_mut(usize::from(s)))
            {
                a.battle.shatter_row = Some(cam::FREED_SHATTER_ROW);
            }
        }
        if phase_in == 4 {
            let records = cam::FREED_CRYSTAL_RECORDS;
            if passed {
                if let Some(scene) = self.casting.active_summon.as_mut() {
                    for part in &mut scene.parts {
                        if part.tag.is_some_and(|t| records.contains(&t)) {
                            part.finished = true;
                        }
                    }
                }
            } else {
                self.casting.module_freed_turn -= cam::FREED_CRYSTAL_TURN_STEP;
                let corners = cam::freed_crystal_corners(
                    self.casting.module_freed_turn,
                    self.casting.module_settle_countdown,
                );
                let creature = self.module_creature_seat();
                if let (Some(c), Some(scene)) = (creature, self.casting.active_summon.as_mut()) {
                    turn_crystals(&mut scene.parts, &records, c, &corners);
                }
            }
        }
        if passed {
            let spawns = cam::freed_arm_spawns(phase_in);
            if !spawns.is_empty() {
                self.stage_module_arm_spawns(912, spawns, None, None);
            }
        }
    }
}

/// Move the crystal triples onto `corners` off the creature `c`: the
/// `k`-th part tagged `records[0]` is corner `k`'s first handle; it takes
/// the new `+0x14` / `+0x18` and the other two records' `k`-th parts take
/// its whole trio.
fn turn_crystals(
    parts: &mut [legaia_engine_effects::summon::SummonPartRuntime],
    records: &[u32; 3],
    c: ModuleSeat,
    corners: &[(i16, i16); 4],
) {
    let nth = |parts: &[legaia_engine_effects::summon::SummonPartRuntime], record: u32| {
        parts
            .iter()
            .enumerate()
            .filter(|(_, p)| p.tag == Some(record) && !p.finished)
            .map(|(i, _)| i)
            .collect::<Vec<_>>()
    };
    let lead = nth(parts, records[0]);
    let rest: Vec<Vec<usize>> = records[1..].iter().map(|&r| nth(parts, r)).collect();
    for (k, &(dx, dz)) in corners.iter().enumerate() {
        let Some(&i) = lead.get(k) else {
            continue;
        };
        let s = &mut parts[i].state;
        s.world_x = c.x.wrapping_add(dx);
        s.world_z = c.z.wrapping_add(dz);
        let trio = (s.world_x, s.world_y, s.world_z);
        for list in &rest {
            if let Some(&j) = list.get(k) {
                let t = &mut parts[j].state;
                (t.world_x, t.world_y, t.world_z) = trio;
            }
        }
    }
}
