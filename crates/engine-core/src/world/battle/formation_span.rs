//! The per-round formation squash + recentre - retail's battle-flow case `0`
//! (`FUN_801D388C`, `jal 0x801DB318` at `0x801D3908`), reached from the round
//! start `0x801D0EE4` right after the actor sweep `FUN_801D88CC` and the
//! initiative seeder `FUN_801DA780`, and again from the ring cancelled back to
//! the round prompt (case `2`, `0x801D11E0`).
//!
//! The kernel is `legaia_engine_vm::battle_action::normalize_formation_span`;
//! this module maps the engine's actor table onto the retail pool it walks.
//!
//! ## The two pools differ in layout, not in seating
//!
//! Retail seats party member `i` in pool slot `i` and monster `k` in pool slot
//! `3 + k` whatever the party size (`FUN_800513F0`: the party loop indexes
//! `0x801C9370 + i*4`, the monster loop `0x801C9370 + (k+3)*4` at
//! `0x8005185C`), so a party of one leaves slots `1` and `2` empty. The engine
//! compacts monsters down to `party_count + k`. The seat each combatant gets is
//! the same on both - party member `i` takes row `party_count` entry `i`,
//! monster `k` takes row `monster_count` entry `k` - so the difference is only
//! an index space, and this walk converts it: engine party slot `i` is retail
//! slot `i`, engine monster slot `party_count + k` is retail slot `3 + k`.
//!
//! The kernel includes slots `0..3` **unconditionally** and slots `3..7` only
//! with a live `+0x14C`, so an absent party seat still contributes its
//! position to the extents. Those actors are at the origin in every catalogued
//! capture of a fresh small-party battle (five solo / duo states), and the
//! origin lies inside every authored formation's box - so the engine passes
//! them as `(0, 0)` and the phantom slots cannot move the result. Two
//! later-fight captures show a small party's empty slots carrying non-zero
//! positions left behind by other actors that borrowed the struct; the engine
//! allocates no combatant struct there, so that residue is not modelled.
//!
//! ## What it does to a fight
//!
//! Nothing walks a combatant home after an action
//! ([`World::tick_battle_locomotion`]), so the formation wanders over a fight;
//! this pass pulls it back each round. Its visible effect on an authored
//! formation is the recentre alone: a party of three against one monster
//! spans `z = -825 ..= 800`, whose centroid `(-25 as u32) >> 1` truncates to
//! `-13`, and every combatant moves `+13` in Z. Every catalogued three-on-one
//! capture reads exactly that (party `z = -812 / -762`, monster `z = 813`),
//! which is where an earlier "uniform mid-battle drift" reading came from.
//!
//! ## The seat follows
//!
//! The kernel writes only the live pair `+0x34` / `+0x38`. The engine's seat
//! pair (`BattleActor::seat`, the reference the range law and the separation
//! pass measure against) is shifted by the same per-actor delta. Retail's
//! `+0x3C` / `+0x40` tracks the live pair at a fixed pose offset rather than
//! staying where the formation was: the Tetsu fight reads the solo party at
//! live `z = -800`, `+0x3C` pair `-862`, and a recentred three-on-one reads the
//! lead at live `-812`, pair `-874` - the same `-62` apart, moved `+13`
//! together. Leaving the engine's seat behind would put an attacker's range
//! reference a formation shift away from the body it walks at, and the
//! approach would never arrive.
//!
//! The kernel's other output, the camera-focus shift on `_DAT_80089118` /
//! `_DAT_80089120`, is taken into locals and dropped: the flow case that runs
//! the squash immediately re-arms the far framing `FUN_801D5854(0, 9)`
//! (`0x801D4908`), which re-derives its focus from the live actor table, and
//! the engine's far framing reads the same live table every pass
//! ([`crate::battle_cam_inputs::battle_formation_box`]).

use super::*;
use vm::battle_action::{FormationPos, normalize_formation_span};

/// Retail's pool width the kernel walks (`sltiu v0,v0,0x7` at `0x801DB3D4`):
/// party slots `0..3`, monster slots `3..7`.
const RETAIL_POOL_WALK: usize = 7;

/// The first retail monster slot (`addiu s0,s2,0x3` at `0x8005185C`).
const RETAIL_FIRST_MONSTER_SLOT: usize = 3;

impl World {
    /// The retail pool slot an engine battle slot occupies, or `None` for a
    /// slot retail's seven-slot walk does not reach (a fifth monster, or an
    /// actor past the combatant table).
    pub fn retail_battle_pool_slot(&self, engine_slot: usize) -> Option<usize> {
        let pc = usize::from(self.party.party_count.clamp(1, 3));
        let retail = if engine_slot < pc {
            engine_slot
        } else {
            let k = engine_slot - pc;
            let monster = self
                .actors
                .get(engine_slot)
                .is_some_and(|a| a.battle_monster_id.is_some());
            if !monster {
                return None;
            }
            RETAIL_FIRST_MONSTER_SLOT + k
        };
        (retail < RETAIL_POOL_WALK).then_some(retail)
    }

    /// Run the round-start formation squash + recentre over the live battle
    /// positions.
    ///
    /// REF: FUN_801D388C (flow case `0` / `2`: the `jal 0x801DB318` at
    /// `0x801D3908`; the `FUN_801D5854(0, 9)` re-frame that follows is the
    /// engine's per-pass far framing)
    pub(in crate::world) fn normalize_battle_formation(&mut self) {
        let mut pos = [FormationPos::default(); RETAIL_POOL_WALK];
        let mut alive = [false; RETAIL_POOL_WALK];
        let mut owner = [None::<usize>; RETAIL_POOL_WALK];
        for (slot, a) in self.actors.iter().enumerate().take(8) {
            let Some(r) = self.retail_battle_pool_slot(slot) else {
                continue;
            };
            pos[r] = FormationPos {
                x: a.move_state.world_x,
                z: a.move_state.world_z,
            };
            alive[r] = a.battle.liveness != 0;
            owner[r] = Some(slot);
        }
        // The camera-focus pair: see the module docs for why it is local.
        let (mut focus_x, mut focus_z) = (0i32, 0i32);
        normalize_formation_span(&mut pos, &alive, &mut focus_x, &mut focus_z);
        for (r, slot) in owner.iter().enumerate() {
            if let Some(slot) = *slot
                && let Some(a) = self.actors.get_mut(slot)
            {
                let dx = pos[r].x.wrapping_sub(a.move_state.world_x);
                let dz = pos[r].z.wrapping_sub(a.move_state.world_z);
                a.move_state.world_x = pos[r].x;
                a.move_state.world_z = pos[r].z;
                // The seat pair moves with the live pair: see the module
                // docs' "The seat follows".
                if let Some((sx, sz)) = a.battle.seat.as_mut() {
                    *sx = sx.wrapping_add(dx);
                    *sz = sz.wrapping_add(dz);
                }
            }
        }
    }
    /// Re-seat the monster block on the **alternate** stage-seat family.
    ///
    /// `FUN_800513F0` picks the monster row as `ctx[+1] + sp[+0x20] + s4`,
    /// where `sp[+0x20]` is `(DAT_8007BD60 >> 5) & 4` (`0x80051430`, the same
    /// word it stores to the scripted-fight byte `ctx[+0x287]`) - so a
    /// scripted fight reads rows `5..8` of the table at `0x80077608`. Counts
    /// one and two are authored identical in both families; three and four
    /// differ. The second addend `s4 = 4` is the map-gated arm (monster
    /// `0x3D..=0x3F` first, `_DAT_80084540` `0x0C` / `0x15`,
    /// `0x800517A0..0x800517E0`), which needs the numeric map id the engine
    /// does not carry at this layer - the same gap the formation roll's
    /// scripted-ambush arm discloses.
    ///
    /// REF: FUN_800513F0 (the monster row index, `0x80051838..0x8005184C`)
    pub(in crate::world) fn seat_scripted_monster_family(&mut self, monster_count: u8) {
        let pc = usize::from(self.party.party_count.clamp(1, 3));
        for k in 0..usize::from(monster_count) {
            let s = crate::battle_seats::monster_seat(monster_count, k, true);
            let Some(a) = self.actors.get_mut(pc + k) else {
                break;
            };
            a.move_state.world_x = s.x;
            a.move_state.world_y = s.y;
            a.move_state.world_z = s.z;
        }
    }
}
