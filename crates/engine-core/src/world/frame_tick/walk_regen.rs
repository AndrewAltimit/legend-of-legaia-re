//! The field walk regeneration tick.

use super::*;

impl World {
    /// Field walk-regen driver: project the present party onto the
    /// [`legaia_engine_field::walk_regen`] kernel, run one tick against
    /// [`crate::world::FieldLocomotion::walk_regen_steps`], and write the bumped gauges back into the
    /// roster records.
    ///
    /// REF: FUN_801D0B90
    ///
    /// The kernel ([`legaia_engine_field::walk_regen::tick_walk_regen`]) is the retail
    /// body: it only runs while the accumulator exceeds
    /// [`legaia_engine_field::walk_regen::WALK_REGEN_STEP_COST`] (`0x20`), subtracts that
    /// cost, and bumps HP / MP / AP by `8` / `2` / `1` for each member whose
    /// ability-bitfield word 1 carries the walk-passive bit (`0x38` Life
    /// Source, `0x39` Magic Source, `0x3A` Mettle Source), each clamped at
    /// the record's effective maximum. A party with none of those accessories
    /// equipped therefore sees no state change at all, which is why wiring
    /// this moves no existing oracle.
    ///
    /// The **fill** side is retail's as well, and it is inside the locomotion
    /// controller: `FUN_801D01B0`'s tail at `0x801D0910..0x801D0928` adds
    /// `DAT_1F800393` to `_DAT_801F2274` behind the step-delta-non-zero test at
    /// `0x801D08F4..0x801D090C` - one unit per vsync whose step committed.
    /// [`Self::step_field_locomotion`] adds [`legaia_engine_vm::frame_clock::FrameClock::display_frame_step`] once per
    /// sim tick, which is the same rate under the 1:1 denomination.
    ///
    /// The kernel's return value is the edge where the Incense window
    /// [`crate::world::FieldLocomotion::walk_regen_window`] (`_DAT_8007B600`,
    /// armed by the pause Items Incense confirm) runs out. Retail then
    /// installs the entry-context record `0x801F2278` (kind byte `0x0B`) and
    /// spawns the submode driver (`0x801D0CEC..0x801D0D24`), which maps kind
    /// `0x0B` to `FUN_801F1E48` - the one-line wear-off notice. That edge
    /// raises [`Self::raise_incense_notice`].
    ///
    /// The same tick runs on the **overworld**: a kingdom map is a mode-3
    /// field-run scene with the field overlay resident, and the frame driver
    /// `FUN_801D1344` calls this routine (`jal` at `0x801D16EC`) right before
    /// the locomotion controller. The WorldMap arm therefore calls it as
    /// well ([`Self::tick_world_map`]).
    ///
    /// Member order is the present party (retail walks the member-id table
    /// at `0x80084598`), resolved through [`Self::party_roster_slot`].
    pub(crate) fn tick_field_walk_regen(&mut self) {
        use legaia_engine_field::walk_regen::{WalkGauge, WalkRegenMember};
        if self.locomotion.walk_regen_steps <= legaia_engine_field::walk_regen::WALK_REGEN_STEP_COST
        {
            return;
        }
        let count = (self.party.party_count.min(3) as usize).min(self.party.roster.members.len());
        let slots: Vec<usize> = (0..count).map(|i| self.party_roster_slot(i)).collect();
        let mut members: Vec<WalkRegenMember> = Vec::with_capacity(slots.len());
        for &rslot in &slots {
            let Some(rec) = self.party.roster.members.get(rslot) else {
                continue;
            };
            let hms = rec.hp_mp_sp();
            // Word 1 of the `+0xF4` ability bitfield - the word the three
            // walk-passive bits (`0x38..=0x3A`) land in.
            let bits = rec.ability_bits();
            let ability_hi = u32::from_le_bytes([bits[4], bits[5], bits[6], bits[7]]);
            members.push(WalkRegenMember {
                ability_hi,
                hp: WalkGauge {
                    value: hms.hp_cur,
                    cap: hms.hp_max,
                },
                mp: WalkGauge {
                    value: hms.mp_cur,
                    cap: hms.mp_max,
                },
                ap: WalkGauge {
                    value: hms.sp_cur,
                    cap: hms.sp_max,
                },
            });
        }
        let mut counter = self.locomotion.walk_regen_steps;
        let mut window = self.locomotion.walk_regen_window;
        let armed = legaia_engine_field::walk_regen::tick_walk_regen(
            &mut counter,
            &mut members,
            &mut window,
        );
        self.locomotion.walk_regen_steps = counter;
        self.locomotion.walk_regen_window = window;
        if armed {
            self.raise_incense_notice();
        }
        for (&rslot, m) in slots.iter().zip(members.iter()) {
            let Some(rec) = self.party.roster.members.get_mut(rslot) else {
                continue;
            };
            let mut hms = rec.hp_mp_sp();
            if hms.hp_cur == m.hp.value && hms.mp_cur == m.mp.value && hms.sp_cur == m.ap.value {
                continue;
            }
            hms.hp_cur = m.hp.value;
            hms.mp_cur = m.mp.value;
            hms.sp_cur = m.ap.value;
            rec.set_hp_mp_sp(hms);
        }
    }
}
