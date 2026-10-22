//! What a directed summon module reads back off its creature seat each tick
//! (the clip it plays, whether it is in reach of the victim), and what the
//! module writes straight onto that seat and the camera (its height, its
//! drift).

use super::*;

/// The depth PROT 0909's stager seats its creature at (`li v0,0xc00` /
/// `sh v0,0x38` at `0x801F7B4C`).
const VIGURO_SEAT_Z: i16 = 0xC00;
/// PROT 0909's arm that holds on its creature's arrival (`0x801F6C7C`).
const VIGURO_ARRIVAL_ARM: u8 = 4;
/// PROT 0908's arm that turns the creature onto the victim (`0x801F7360`).
const ZENOIR_TURN_ARM: u8 = 6;
/// PROT 0908's first arm that writes the creature's height (`0x801F782C`).
const ZENOIR_RISE_ARM: u8 = 8;
/// PROT 0908's arm that holds on the creature reaching the victim
/// (`FUN_8004E2F0(7, victim)` at `0x801F7628`).
const ZENOIR_REACH_ARM: u8 = 8;
/// The closing speed of a creature with no root speed to play while a reach
/// arm holds, world units a tick: the `gimard_burning_attack` capture's walk
/// rate (~30 a vsync).
const CREATURE_CLOSE_STEP: i32 = 30;
/// PROT 0908's arm that moves the creature over the victim (`0x801F79F4`).
const ZENOIR_OVER_VICTIM_ARM: u8 = 10;

/// The modules whose creature's bare `+0x1DA` stores need retail's commit
/// boundary: PROT 0907, 0908 and 0909, whose arms gate on the creature's
/// clip `+0x1D9`, and PROT 0913, whose arm 3 stages clip `1` with no stage
/// bit (`sb v0,0x1da(s8)` at `0x801F6EC8`) over the long emergence clip `0`.
/// Retail RAM through arms 3..7 holds `+0x1D9 = 0`, `+0x1DA = 1` and the
/// installer's rate `2`: the creature climbs out of the vortex on clip `0`
/// until arm 9's stage-bit commit of clip `2`. PROT 0928's arm 6 stages
/// clip `2` bare (`sb v0,0x1da(s4)` at `0x801F7828`) and arm 7 holds until
/// it plays (`lbu v1,0x1d9(s4)` / `bne v1,2` at `0x801F7890..0x801F7898`):
/// in retail that is the end of clip `1`, ~300 vsyncs on.
const CLIP_GATED_MODULES: [u32; 5] = [907, 908, 909, 913, 928];

/// The animation rate `+0x21D` the creature seat installer `FUN_801F19EC`
/// gives the creature it seats (`li s3,0x2` at `0x801F1CFC`, stored at
/// `0x801F1DFC`): a quarter of the battle's normal `8`. The first commit
/// lifts it to half (`battle_anim_rate::commit_rate_decay`, `0x8004B080`), so
/// a creature plays its clips - and walks its root motion - at a quarter of a
/// party member's speed until its first commit and at half after. The
/// installer is the battle overlay's, shared by every creature-seating module
/// in the band, so every seated creature gets it.
pub(in crate::world) const SUMMON_SEAT_ANIM_RATE: u8 = 2;

impl World {
    /// Whether actor `slot` is the summon creature of a resident module that
    /// gates on its clip, whose staged byte then waits for the looping
    /// clip's natural end instead of committing at once.
    ///
    /// Retail commits a byte staged under neither `+0x1DC` stage bit only
    /// from the anim tick's natural-end path (`FUN_80047430` -> `FUN_8004AD80`
    /// at `0x80047B54`): over a loop that is the cycle's end. The engine
    /// commits such a byte at once for every other actor; for these
    /// creatures the difference is the length of an arm - PROT 0908's arm 6
    /// waits on the commit its arm 5 staged.
    pub(in crate::world) fn summon_seat_waits_for_cycle(&self, slot: usize) -> bool {
        self.casting.summon_actor_slot == Some(slot as u8)
            && self
                .casting
                .summon_stager
                .as_ref()
                .and_then(|st| self.cast_module_for(st.spell_id))
                .is_some_and(|e| CLIP_GATED_MODULES.contains(&e))
    }

    /// Feed the creature seat's clip `+0x1D9` and the range answer
    /// `FUN_8004E2F0(7, victim) == 0` into the module camera state, the two
    /// creature gates PROT 0907 / 0908 / 0909's arms poll. A seat with no
    /// installed clips reports no clip, so a clip gate passes rather than
    /// park on a seat that can never commit one.
    pub(in crate::world::battle::cast_band) fn feed_module_creature(&mut self, caster: u8) {
        let seat = self
            .casting
            .summon_actor_slot
            .filter(|&s| self.actors.get(s as usize).is_some_and(|a| a.active));
        let victim = self.casting.module_cam.victim_slot.unwrap_or_else(|| {
            self.actors
                .get(caster as usize)
                .map_or(0, |a| a.battle.active_target)
        });
        let clip = seat
            .and_then(|s| self.actors.get(s as usize))
            .filter(|a| a.battle_action_clips.is_some())
            .map(|a| a.battle.current_anim);
        // The creature sits outside the engine's eight-slot table, so the
        // metric takes its live pair directly (size class `0`, as PROT
        // 0903's walk arm reads it).
        let in_range = seat
            .and_then(|s| self.actors.get(s as usize))
            .map(|a| (a.move_state.world_x, a.move_state.world_z))
            .map(|pos| self.creature_range_metric(pos, 0, victim) == 0);
        // `FUN_801DCEAC(9, ..)`: the enemy row's live seats, laid out on
        // retail's `3..7`.
        let pc = usize::from(self.party.party_count);
        let mut slots = [legaia_engine_battle_vm::battle_target_group::GroupSlot {
            live: false,
            x: 0,
            z: 0,
        }; 7];
        for (k, a) in self.actors.iter().skip(pc).take(4).enumerate() {
            slots[3 + k] = legaia_engine_battle_vm::battle_target_group::GroupSlot {
                live: a.active && a.battle_monster_id.is_some() && a.battle.hp != 0,
                x: a.move_state.world_x,
                z: a.move_state.world_z,
            };
        }
        self.casting.module_cam.row_aim =
            legaia_engine_battle_vm::battle_target_group::target_group_aim(9, &slots);
        self.casting.module_cam.creature_clip = clip;
        // `+0x21B`, the loop counter the anim tick keeps on the creature
        // (`World::mirrors_loop_counter`).
        self.casting.module_cam.creature_loops = seat
            .and_then(|s| self.actors.get(s as usize))
            .filter(|a| a.battle_action_clips.is_some())
            .map(|a| a.battle.hit_count_bound);
        self.casting.module_cam.creature_in_range = in_range;
        // The node cursor `+0x68` (12.4) a frame wait reads.
        self.casting.module_cam.creature_frame = seat
            .and_then(|s| self.actors.get(s as usize))
            .filter(|a| a.battle_action_clips.is_some())
            .and_then(|a| a.battle_animation.as_ref())
            .map(|p| p.cursor_sixteenths());
    }

    /// The module's direct writes after its pass: the creature seat's
    /// position as a module that moves it by hand left it, and the
    /// camera-global drift the arm returned. PROT 0908 writes its creature's
    /// height `+0x36` every pass from arm 9 on, turns it onto the victim in
    /// arm 6 and moves it over the victim in arm 10; between those the
    /// strike clip's root motion owns `+0x34` / `+0x38`, so the plane pair
    /// is written only once arm 10 has.
    pub(in crate::world::battle::cast_band) fn apply_module_creature_writes(
        &mut self,
        prot_entry: u32,
        phase: u8,
        drift: Option<legaia_engine_cast::cast_module_camera::CaptureDrift>,
    ) {
        if prot_entry == 908
            && phase > ZENOIR_TURN_ARM
            && let Some(c) = self.casting.module_cam.creature
            && let Some(slot) = self.casting.summon_actor_slot
            && let Some(a) = self.actors.get_mut(slot as usize)
        {
            a.battle.facing_angle = c.facing;
            if phase > ZENOIR_RISE_ARM {
                a.move_state.world_y = c.y;
            }
            if phase > ZENOIR_OVER_VICTIM_ARM {
                a.move_state.world_x = c.x;
                a.move_state.world_z = c.z;
            }
        }
        if prot_entry == 908 && phase == ZENOIR_REACH_ARM {
            self.close_creature_on_victim();
        }
        if prot_entry == 909 && phase == VIGURO_ARRIVAL_ARM {
            self.walk_viguro_in();
        }
        if let Some(d) = drift
            && let Some(cam) = self.battle.camera.as_mut()
        {
            cam.drift_module(d.pitch, d.yaw, d.tr_y, d.tr_z);
            if d.tr_x != 0 {
                cam.drift_module_tr_x(d.tr_x);
            }
        }
    }

    /// Close the creature on the victim one step, facing it, while a reach
    /// arm holds and the range poll still fails - for a creature whose
    /// playing clip carries no root speed (a headless seat, or a body with
    /// no record clips). Retail's creature arrives on its record clip's root
    /// motion, which the anim tick's positive-speed term already runs
    /// (`World::drive_playing_root_motion`); stepping it here as well walked
    /// it in at twice retail's rate.
    fn close_creature_on_victim(&mut self) {
        let Some(slot) = self.casting.summon_actor_slot else {
            return;
        };
        if self.creature_root_driven(slot) {
            return;
        }
        let Some(victim) = self.casting.module_cam.victim_slot else {
            return;
        };
        let Some(a) = self.actors.get(usize::from(slot)) else {
            return;
        };
        let pos = (a.move_state.world_x, a.move_state.world_z);
        if self.creature_range_metric(pos, 0, victim) == 0 {
            return;
        }
        let (vx, vz) = self.battle_seat_of(usize::from(victim));
        let bearing =
            legaia_engine_battle_vm::battle_action::bearing_12bit_approx(pos.1, pos.0, vz, vx);
        let (sin, cos) = legaia_engine_system::trig::trig12(bearing & 0xFFF);
        if let Some(a) = self.actors.get_mut(usize::from(slot)) {
            a.battle.facing_angle = bearing & 0xFFF;
            let ms = &mut a.move_state;
            ms.world_x =
                (i32::from(ms.world_x) + i32::from(sin) * CREATURE_CLOSE_STEP / 4096) as i16;
            ms.world_z =
                (i32::from(ms.world_z) + i32::from(cos) * CREATURE_CLOSE_STEP / 4096) as i16;
        }
    }

    /// Run the op-`0x20` calls PROT 0909's parts made since the last tick
    /// through its stager `FUN_801F7AF4`, in call order.
    ///
    /// Arms `0` and `1` are the tick body's two phase rendezvous
    /// ([`legaia_engine_cast::seru_magic_ticks::VIGURO_RENDEZVOUS_PHASES`]):
    /// record `0x801F7E18`, which arm 2 spawns, waits `0x7F` and calls arm
    /// `0` - seating the creature at `(0, 0xC00)` facing `0x800`
    /// (`0x801F7B44..0x801F7B5C`) - and record `0x801F7F20`, which arm 7
    /// spawns, calls arm `1` once its six-pass loop is through. Arms `2`,
    /// `3`, `4` and `6` each seat one record on the calling part's own
    /// position and rotation (`a0 = part + 0x14`, `a1 = part + 0x24`); arm
    /// `5` does nothing.
    ///
    /// REF: FUN_801F7AF4 (PROT 0909's stager, reached through `gp[+0x714]`)
    pub(in crate::world::battle::cast_band) fn run_viguro_stager_calls(
        &mut self,
        ctx: &mut legaia_engine_cast::cast_module_ticks::CastModuleCtx,
        seat: &mut legaia_engine_cast::cast_module_ticks::CastActorState,
    ) {
        use legaia_engine_cast::cast_module_camera::{
            ModuleSeat, ModuleSpawn, SpawnAnchor, SpawnRecord,
        };
        let mut calls = std::mem::take(&mut self.casting.module_stager_calls);
        // A record the host did not seat as a part makes its call on the
        // clock its script keeps (`module_record_clocks`): the rendezvous
        // falls where the record would put it, and a phase whose record was
        // never spawned holds, as retail's does.
        let mut due = Vec::new();
        self.casting
            .module_record_clocks
            .retain_mut(|(_, arm, left)| {
                if *left == 0 {
                    due.push(*arm);
                    false
                } else {
                    *left -= 1;
                    true
                }
            });
        calls.extend(due.into_iter().map(|arm| (arm, Default::default())));
        for (arm, caller) in calls {
            let (pos, rot) = (caller.pos, caller.rot);
            legaia_engine_cast::cast_module_ticks::viguro_stager(ctx, seat, arm);
            if arm == 0 {
                self.casting.module_cam.creature = Some(ModuleSeat {
                    x: 0,
                    y: 0,
                    z: VIGURO_SEAT_Z,
                    facing: 0x800,
                });
            }
            let record = match arm {
                2 => 0x801F_81F4,
                3 => 0x801F_8288,
                4 => 0x801F_82CC,
                6 => 0x801F_8310,
                _ => continue,
            };
            let spawn = [ModuleSpawn {
                record: SpawnRecord::Module(record),
                anchor: SpawnAnchor::Literal { pos, rot },
            }];
            self.stage_module_arm_spawns(909, &spawn, None, None);
        }
    }

    /// Walk PROT 0909's creature in along its facing while arm 4 waits for
    /// it to cross `z = 0x200`, for a creature whose playing clip carries no
    /// root speed. Retail's creature covers the `0xA00` from its seat at
    /// `0xC00` in 87 vsyncs of the `viguro` capture - about 30 units a vsync:
    /// its record's clip 0 carries root speed `120`, which at the seat's
    /// rate `2` is `120 * 2 * 2 >> 3 = 30` a vsync, and which the anim
    /// tick's root-motion term runs once the record clips are bound.
    fn walk_viguro_in(&mut self) {
        let Some(slot) = self.casting.summon_actor_slot else {
            return;
        };
        if self.creature_root_driven(slot) {
            return;
        }
        if let Some(a) = self.actors.get_mut(usize::from(slot)) {
            let (sin, cos) = legaia_engine_system::trig::trig12(a.battle.facing_angle & 0xFFF);
            let ms = &mut a.move_state;
            ms.world_x =
                (i32::from(ms.world_x) + i32::from(sin) * CREATURE_CLOSE_STEP / 4096) as i16;
            ms.world_z =
                (i32::from(ms.world_z) + i32::from(cos) * CREATURE_CLOSE_STEP / 4096) as i16;
        }
    }

    /// Whether the creature at `slot` is playing a clip whose root speed the
    /// anim tick's root-motion term moves it by.
    fn creature_root_driven(&self, slot: u8) -> bool {
        self.battle_playing_root_motion(usize::from(slot))
            .is_some_and(|(speed, _)| speed > 0)
    }
}
