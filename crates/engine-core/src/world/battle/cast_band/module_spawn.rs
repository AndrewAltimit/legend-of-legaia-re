//! Seating a cast module's actors and effects: the summon actor seat, the cast
//! effect pool, module lookup, and the module FX and arm spawns.

use super::module_seat_arm;
use super::*;
use legaia_engine_mode::mode::SceneMode;

impl World {
    /// A host seated the summon creature at actor `slot`: adopt the seat,
    /// place it where the seat installer `FUN_801F19EC` leaves it, and mark
    /// it active. Hosts call this right after binding the creature's mesh,
    /// idle player and clip set.
    ///
    /// The installer is one battle-overlay routine every creature-seating
    /// module calls (`jal 0x801F19EC`), so what it writes holds band-wide:
    /// the creature stands **on the caster** - the node is built off the
    /// caster's `+0x34` triple (`FUN_80024C88(caster + 0x34, ..)` at
    /// `0x801F1D4C`) and copied into the seat's `+0x3C` / `+0x40` and
    /// `+0x34` / `+0x38` (`0x801F1E04..0x801F1E40`) - wearing the caster's
    /// facing (`+0x26 = caster[+0x46]` at `0x801F1D94`), at animation rate
    /// `2` (`0x801F1DFC`). A module that moves its creature does so in its
    /// own arms afterwards ([`crate::world::battle::cast_band`]'s
    /// `module_cam.creature`). A module the engine paces with its own walk
    /// stand-in (no band-pacing director) keeps the stand-in's spawn point,
    /// which its walk is measured from.
    ///
    /// PORT: FUN_801F19EC (the seat, facing, target and rate stores, and the seat's presentation, shatter and anim-id-ring resets)
    pub fn seat_summon_actor(&mut self, slot: usize) {
        let staged = self.casting.summon_stager.as_ref().map(|st| {
            let caster = self.actors.get(st.caster as usize);
            let spawn = match caster.filter(|_| self.module_seats_on_caster(st.spell_id)) {
                Some(c) => [
                    c.move_state.world_x,
                    c.move_state.world_y,
                    c.move_state.world_z,
                ],
                None => st.spawn,
            };
            (
                spawn,
                caster.map(|a| a.battle.facing_angle).unwrap_or(0),
                caster.map(|a| a.battle.active_target),
            )
        });
        self.casting.summon_actor_slot = Some(slot as u8);
        let placed = self.casting.module_cam.creature;
        let entry = self
            .casting
            .summon_stager
            .as_ref()
            .and_then(|st| self.cast_module_for(st.spell_id));
        let Some(a) = self.actors.get_mut(slot) else {
            return;
        };
        a.active = true;
        // A debug spawn outside a cast keeps the host's own placement.
        let Some((spawn, facing, target)) = staged else {
            return;
        };
        a.move_state.world_x = spawn[0];
        a.move_state.world_y = spawn[1];
        a.move_state.world_z = spawn[2];
        a.battle.facing_angle = facing;
        // `+0x1DD = caster[+0x1DD]` (`0x801F1DB8`): the target the root-motion
        // term's range poll stops a positive-speed clip at.
        if let Some(t) = target {
            a.battle.active_target = t;
        }
        a.battle.queued_anim = 0;
        a.battle.current_anim = 0;
        // The presentation words the installer resets on the seat it reuses:
        // colour `+0x04 = -1` (`sw v1,0x4(a0)` with `v1 = -1` at
        // `0x801F1D5C`), mode word `+0x08 = 0` (`0x801F1D68`), tint state
        // `+0x21C = 0` (`0x801F1E7C`) and impact selector `+0x21F = 0`
        // (`0x801F1E88`). A seat a previous cast's creature left in the
        // defeat fade (`+0x21C = 2`) otherwise fades the new creature out
        // and, at the fade's end, drops the clip a module staged on it.
        a.battle.render_color = u32::MAX;
        a.battle.flag_word = 0;
        a.battle.render_flag = 0;
        a.battle.impact_state = 0;
        // `sh zero,0x1ba` (`0x801F1E94`): the shatter word a previous
        // creature's module raised on this seat (six summons shatter their
        // own creature), which the draw copies to the node's `+0x7A` and
        // turns into the shatter renderer.
        a.battle.shatter = 0;
        // `sb zero,0x1fb(v0)` over 32 bytes (`0x801F1E98..0x801F1EB0`): the
        // committed-anim-id ring `FUN_80047430` shifts each frame and the
        // after-image walk `FUN_80049348` gates on (`0x10 < id`). Only the id
        // ring is zeroed - the position / cursor / entry rings keep their
        // bytes - so the history stays and no frame of it is a ghost.
        for f in a.battle_pose_history.iter_mut() {
            f.ghost_eligible = false;
        }
        a.battle.anim_rate = legaia_engine_battle_vm::battle_anim_rate::AnimRate(
            super::module_creature::SUMMON_SEAT_ANIM_RATE,
        );
        // The installer hangs the record's first clip on the node directly
        // (`sw v1,0x4c(v0)` / `sh zero,0x68(v0)` at `0x801F1DCC..0x801F1DE8`)
        // without the commit `FUN_8004AD80`, so nothing seeds the clip's
        // window: `+0x176` / `+0x21B` keep the zeros the previous cast's
        // `0xFF` arm left on the seat (`sb zero,0x21b` / `sh zero,0x176`,
        // e.g. PROT 0907 `0x801F7F68`). The window first applies at the
        // clip's first commit.
        a.battle.hit_count_bound = 0;
        if let Some(p) = a.battle_animation.as_mut() {
            p.release_loop_window();
        }
        // ...and the walker runs the clip it hung there all the same: the
        // per-frame `FUN_80047430` -> `FUN_801DEA50` call reads the node's
        // record, so clip 0's effect script fires on its frames (PROT 0913's
        // emergence clip lands its dust and floor ring at frame `0x62`). The
        // installer leaves the cursor `+0x1F5` alone; the seat it reuses
        // holds `0` (retail RAM across PROT 0913's arms 3..7).
        a.battle_effect_script = a
            .battle_action_clips
            .as_ref()
            .and_then(|c| c.first().cloned().flatten())
            .map(|c| c.effect_script)
            .filter(|s| !s.is_empty());
        a.battle_effect_cursor = 0;
        a.battle_anim_cue_cursor = 0;
        // The seat arm's own stores, which run after the installer on the
        // same pass (`module_seat_arm`).
        if let Some(e) = entry {
            self.apply_seat_arm_write(e, slot);
        }
        // A seat arm that computes its seat lives in its director (PROT
        // 0907's arm 6 seats Nighto on its grid point at
        // `0x801F76C8..0x801F77A4`, right behind `jal 0x801F19EC`). The host
        // seats the mesh a tick after the director has run that arm, so a
        // placement the director already made wins over the installer's.
        if let Some(c) = placed
            && let Some(a) = self.actors.get_mut(slot)
        {
            a.move_state.world_x = c.x;
            a.move_state.world_z = c.z;
            a.battle.facing_angle = c.facing;
        }
        // An engine walk stand-in ends where the module seated the creature:
        // a module that seats it never walks it there.
        if entry.is_some_and(|e| module_seat_arm::seat_arm_write(e).is_some())
            && let Some(a) = self.actors.get(slot)
        {
            let at = [
                a.move_state.world_x,
                a.move_state.world_y,
                a.move_state.world_z,
            ];
            if let Some(st) = self.casting.summon_stager.as_mut() {
                st.goal = at;
            }
        }
    }

    /// Whether a cast of `spell_id` seats its creature where the installer
    /// does (on the caster), and its seat arm's own stores after it: every
    /// module whose band a director paces, so that no engine walk stand-in
    /// moves the creature from its seat, and every module with a seat-arm
    /// row (`module_seat_arm::seat_arm_write`).
    pub(in crate::world) fn module_seats_on_caster(&self, spell_id: u8) -> bool {
        self.cast_module_for(spell_id).is_some_and(|e| {
            module_seat_arm::seat_arm_write(e).is_some()
                || legaia_engine_cast::cast_module_camera::module_profile(e)
                    .is_some_and(|p| p.paces_band())
        })
    }

    /// The seat half of a summon spawn request for a host that draws
    /// nothing: take [`crate::world::CastFxState::pending_summon_spawn`] and
    /// seat an unrendered creature at the hosts' slot (`8 + party_count`),
    /// through the same [`Self::seat_summon_actor`] the play hosts call
    /// after binding the mesh.
    ///
    /// The seat is not presentation-only. A directed module's walk arm
    /// (PROT 0903's arm 11) walks the seated creature and lands the hit when
    /// it reaches the victim; with no creature seated the arm passes on its
    /// first tick, so a headless run folded the outcome hundreds of frames
    /// before either play host would. Returns `true` when a creature was
    /// seated.
    pub fn seat_summon_creature_unrendered(&mut self) -> bool {
        if self.mode != SceneMode::Battle {
            return false;
        }
        if self.take_pending_summon_spawn().is_none() {
            return false;
        }
        let slot = self
            .casting
            .summon_actor_slot
            .map_or(8 + usize::from(self.party.party_count), usize::from);
        let Some(a) = self.actors.get_mut(slot) else {
            return false;
        };
        a.active = true;
        self.seat_summon_actor(slot);
        true
    }

    /// Install the cast-effect pool - the DATA half of the slot-B cast-module
    /// band (PROT 0903..0966), parsed off the disc by the scene host (which
    /// holds the PROT index; `World` is index-agnostic, the same split
    /// [`crate::world::CastFxState::pending_summon_spawn`](crate::world::CastFxState::pending_summon_spawn)
    /// uses). Idempotent; a host that never calls it leaves every cast staging
    /// no module records, which is the disc-free behaviour.
    pub fn install_cast_effect_pool(
        &mut self,
        pool: std::sync::Arc<legaia_overlay_images::cast_effect_pool::CastEffectPool>,
    ) {
        self.casting.effect_pool = Some(pool);
    }

    /// The band entry a cast of `spell_id` pages - the **pool handle** PROT
    /// 0898's two tick dispatchers resolve to.
    ///
    /// Which dispatcher applies is the record's `+0` class byte, exactly as it
    /// is for the damage-kernel pick ([`World::spell_table_class`]):
    ///
    /// * class `'c'` - the capture band. `FUN_801F2160` reads the record's
    ///   `+0x01` byte and jumps through `0x801CF56C`, so the module is
    ///   PROT `935 + sub_id`. Same byte the pager
    ///   `FUN_8003EC70(record[+1] + 0x28)` resolves.
    /// * anything else - `FUN_801F1ED4` keys on the queued action id itself
    ///   and jumps through `0x801CF4EC`, so the module is
    ///   PROT `903 + (id - 0x81)`.
    ///
    /// Without a disc spell table the class byte is unknown and every id is
    /// treated as the action-id band, which is what a disc-free battle's
    /// placeholder catalog means anyway.
    ///
    /// REF: FUN_801F1ED4, FUN_801F2160
    pub fn cast_module_for(&self, spell_id: u8) -> Option<u32> {
        use legaia_engine_battle_vm::battle_cast_dispatch::{
            seru_spell_emitter, spell_class_emitter,
        };
        if self.spell_table_class(spell_id) == Some(legaia_game_tables::spell_names::CAPTURE_CLASS)
        {
            let sub = self.spell_effect_class(spell_id)?;
            return spell_class_emitter(sub, 0).module;
        }
        seru_spell_emitter(spell_id, 0).module
    }

    /// The record's `+0x01` **effect class**: the disc table's byte when one is
    /// installed, else the catalog record's
    /// ([`legaia_engine_battle::spells::SpellDef::effect_class`], which
    /// [`legaia_engine_battle::retail_magic`] fills from `SCUS_942.54`).
    pub fn spell_effect_class(&self, spell_id: u8) -> Option<u8> {
        self.spell_table_sub_class(spell_id).or_else(|| {
            self.tables
                .spell_catalog
                .get(spell_id)
                .map(|d| d.effect_class)
        })
    }

    /// Stage the cast module's **spawn records** at `origin` - the engine's
    /// answer to the dispatchers, and the half of a cast that is data.
    ///
    /// Each record is `[i16 model_sel][u16 reserved][move-VM bytecode]`, the shape
    /// the whole spawn stack shares, so the records run through the same
    /// [`legaia_engine_effects::summon::SummonScene`] the summon and move-FX paths already use
    /// and both hosts draw and tick them with no host change
    /// (`active_summon_part_draws` / `tick_summon`).
    ///
    /// Returns `false` - staging nothing - when the spell names no band entry,
    /// no pool is installed (disc-free), or the module carries no record (the
    /// band's null stub PROT 0926, and PROT 0952 whose two spawn sites load
    /// their record pointer out of a saved register).
    ///
    /// This is the **DATA** half of the band's worklist. The **PORT** half -
    /// the six tick bodies and the seven state-touching stagers - is
    /// [`legaia_engine_cast::cast_module_ticks`], driven from
    /// [`Self::run_cast_module_code`] at this same seam. The **SCOPE-IGNORE**
    /// rows are the six null stagers and the one unreferenced routine; there
    /// is nothing to stage for them either.
    ///
    /// REF: FUN_801F1ED4, FUN_801F2160 (the dispatchers that name the module;
    /// their emitter arms are the unported half above)
    /// REF: FUN_80050ED4, FUN_80021B04 (the spawn calls whose `a2` records
    /// these are)
    pub fn spawn_cast_module_fx(&mut self, spell_id: u8, origin: [i16; 3]) -> bool {
        let Some(entry) = self.cast_module_for(spell_id) else {
            return false;
        };
        let Some(pool) = self.casting.effect_pool.clone() else {
            return false;
        };
        let Some(module) = pool.module(entry) else {
            return false;
        };
        if module.parts.is_empty() {
            return false;
        }
        // A module whose body is ported whole seats its own records, each on
        // the pass its arm spawns it.
        if entry == legaia_engine_cast::cast_fatal_decision::FATAL_DECISION_ENTRY {
            return false;
        }
        // PROT 0924's and 0925's stagers are ported: their parts' op-`0x20`
        // calls seat the stager's records (`Self::run_part_stager_calls`).
        let parts: Vec<_> = if matches!(entry, 924 | 925) {
            const LINK_BASE: u32 = legaia_overlay_images::summon_overlay::SUMMON_OVERLAY_LINK_BASE;
            let own = legaia_engine_cast::cast_module_ticks::stager_seated_records(entry);
            module
                .parts
                .iter()
                .filter(|p| !own.contains(&(LINK_BASE + p.record_off as u32)))
                .cloned()
                .collect()
        } else {
            module.parts.clone()
        };
        self.casting.active_summon = Some(legaia_engine_effects::summon::SummonScene::spawn_parts(
            &parts,
            &module.bytes,
            crate::world::EFFECT_MODEL_LIBRARY_BASE,
            origin,
        ));
        true
    }

    /// Seat, on the stager's first tick, the records of a summon-creature
    /// (or per-arm capture-class) module that none of its arms spawns: the
    /// ones only its part stager
    /// (reached through a part's op-`0x20` call, which the engine does not
    /// run for these modules) seats. The arm-spawned records go on their
    /// own arms ([`legaia_engine_cast::cast_module_camera::creature_arm_spawns`]).
    pub(super) fn spawn_cast_module_stager_records(&mut self, spell_id: u8, origin: [i16; 3]) {
        use legaia_engine_cast::cast_module_camera::{
            SpawnRecord, SpawnTiming, creature_arm_spawns,
        };
        const LINK_BASE: u32 = legaia_overlay_images::summon_overlay::SUMMON_OVERLAY_LINK_BASE;
        let Some(entry) = self.cast_module_for(spell_id) else {
            return;
        };
        let Some(module) = self
            .casting
            .effect_pool
            .clone()
            .and_then(|p| p.module(entry).cloned())
        else {
            return;
        };
        let named: std::collections::BTreeSet<u32> = (0..=u8::MAX)
            .flat_map(|phase| {
                [SpawnTiming::Pass, SpawnTiming::Every]
                    .into_iter()
                    .flat_map(move |t| creature_arm_spawns(entry, phase, t).iter())
                    .chain(legaia_engine_cast::cast_module_camera::capture_arm_spawns(
                        entry, phase,
                    ))
            })
            .filter_map(|s| match s.record {
                SpawnRecord::Module(va) => Some(va),
                SpawnRecord::BattleProto(_) => None,
            })
            // A ported stager seats its own records on the pass a part
            // calls it (`Self::run_part_stager_calls`).
            .chain(
                legaia_engine_cast::cast_module_ticks::stager_seated_records(entry)
                    .iter()
                    .copied(),
            )
            .collect();
        let rest: Vec<_> = module
            .parts
            .iter()
            .filter(|p| !named.contains(&(LINK_BASE + p.record_off as u32)))
            .cloned()
            .collect();
        if rest.is_empty() {
            return;
        }
        self.casting.active_summon = Some(legaia_engine_effects::summon::SummonScene::spawn_parts(
            &rest,
            &module.bytes,
            crate::world::EFFECT_MODEL_LIBRARY_BASE,
            origin,
        ));
    }

    /// Seat the spawn calls one module arm made this pass, each on its own
    /// anchor, into the cast's running scene - the per-arm form of
    /// [`Self::spawn_cast_module_fx`] for a module whose director reports its
    /// spawns ([`legaia_engine_cast::cast_module_camera::ModuleProfile::stages_spawns`]).
    ///
    /// A record's program is bounded by the next record start, so each one
    /// is parsed against the whole record set of its image: the module's own
    /// recovered records for a [`SpawnRecord::Module`], the effect-prototype
    /// table's for a [`SpawnRecord::BattleProto`].
    ///
    /// REF: FUN_80021B04 (the spawn calls), FUN_80058490 (the `MoveImage`)
    ///
    /// [`SpawnRecord::Module`]: legaia_engine_cast::cast_module_camera::SpawnRecord::Module
    /// [`SpawnRecord::BattleProto`]: legaia_engine_cast::cast_module_camera::SpawnRecord::BattleProto
    pub(super) fn stage_module_arm_spawns(
        &mut self,
        prot_entry: u32,
        spawns: &[legaia_engine_cast::cast_module_camera::ModuleSpawn],
        shot: Option<legaia_engine_battle_vm::battle_cam_script::ModuleShot>,
        arm_point: Option<([i16; 3], [i16; 3])>,
    ) {
        use legaia_engine_cast::cast_module_camera::SpawnRecord;
        use legaia_game_tables::move_power::{self, BATTLE_OVERLAY_BASE};
        const LINK_BASE: u32 = legaia_overlay_images::summon_overlay::SUMMON_OVERLAY_LINK_BASE;
        let creature = self
            .casting
            .module_cam
            .creature
            .or(self.casting.module_cam.creature_live);
        let victim = self
            .casting
            .module_cam
            .victim_slot
            .and_then(|s| self.actors.get(usize::from(s)))
            .map(|a| legaia_engine_cast::cast_module_camera::ModuleSeat {
                x: a.move_state.world_x,
                y: a.move_state.world_y,
                z: a.move_state.world_z,
                facing: a.battle.facing_angle & 0xFFF,
            });
        let seat_of = |slot: usize| {
            self.actors
                .get(slot)
                .map(|a| legaia_engine_cast::cast_module_camera::ModuleSeat {
                    x: a.move_state.world_x,
                    y: a.move_state.world_y,
                    z: a.move_state.world_z,
                    facing: a.battle.facing_angle & 0xFFF,
                })
        };
        let party_count = usize::from(self.party.party_count);
        let caster_slot = usize::from(self.battle_ctx.active_actor);
        let anchor_seats = legaia_engine_cast::cast_module_camera::AnchorSeats {
            caster: seat_of(caster_slot),
            caster_body: self.battle_display_trio(caster_slot).and_then(|p| {
                Some(legaia_engine_cast::cast_module_camera::ModuleSeat {
                    x: p[0] as i16,
                    y: p[1] as i16,
                    z: p[2] as i16,
                    facing: self.actors.get(caster_slot)?.battle.facing_angle & 0xFFF,
                })
            }),
            party: std::array::from_fn(|n| {
                (n < party_count && self.actors.get(n).is_some_and(|a| a.battle.hp != 0))
                    .then(|| seat_of(n))
                    .flatten()
            }),
            depth: self.battle.camera_frame_height,
        };
        for spawn in spawns {
            let point = match spawn.anchor {
                legaia_engine_cast::cast_module_camera::SpawnAnchor::ArmPoint => arm_point,
                anchor => legaia_engine_cast::cast_module_camera::spawn_anchor_point_with(
                    anchor,
                    creature,
                    victim,
                    shot,
                    anchor_seats,
                ),
            };
            let Some((pos, rot)) = point else {
                continue;
            };
            let (bytes, parts, off): (std::sync::Arc<[u8]>, Vec<_>, usize) = match spawn.record {
                SpawnRecord::Module(va) => {
                    let Some(module) = self
                        .casting
                        .effect_pool
                        .as_ref()
                        .and_then(|p| p.module(prot_entry))
                    else {
                        continue;
                    };
                    let Some(off) = va.checked_sub(LINK_BASE).map(|o| o as usize) else {
                        continue;
                    };
                    let mut offs: Vec<usize> = module.parts.iter().map(|p| p.record_off).collect();
                    offs.push(off);
                    let parts = legaia_overlay_images::summon_overlay::parse_records_at(
                        &module.bytes,
                        &offs,
                    );
                    (module.bytes.clone(), parts, off)
                }
                SpawnRecord::BattleProto(word_va) => {
                    let Some(overlay) = self.tables.move_power_overlay.clone() else {
                        continue;
                    };
                    let Some(ptr) = word_va
                        .checked_sub(BATTLE_OVERLAY_BASE)
                        .and_then(|o| overlay.get(o as usize..o as usize + 4))
                        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                    else {
                        continue;
                    };
                    let Some(off) = ptr.checked_sub(BATTLE_OVERLAY_BASE).map(|o| o as usize) else {
                        continue;
                    };
                    let mut offs: Vec<usize> = move_power::parse_effect_proto_records(&overlay)
                        .unwrap_or_default()
                        .iter()
                        .map(|p| p.record_off)
                        .collect();
                    offs.push(off);
                    let parts =
                        legaia_overlay_images::summon_overlay::parse_records_at(&overlay, &offs);
                    (overlay, parts, off)
                }
            };
            let picked: Vec<_> = parts.into_iter().filter(|p| p.record_off == off).collect();
            if picked.is_empty() {
                continue;
            }
            let scene = self.casting.active_summon.get_or_insert_with(|| {
                legaia_engine_effects::summon::SummonScene::spawn_parts(
                    &[],
                    &bytes,
                    crate::world::EFFECT_MODEL_LIBRARY_BASE,
                    pos,
                )
            });
            let first = scene.parts.len();
            let scale = legaia_engine_cast::cast_module_camera::spawn_anchor_scale(
                spawn.anchor,
                shot,
                anchor_seats,
            );
            scene.push_parts_scaled(&picked, &bytes, pos, rot, scale);
            // Each part keeps the record it was spawned from, the handle a
            // later arm kills it by (`ori 0x8` into the actor's flag word).
            if let SpawnRecord::Module(va) = spawn.record {
                let pokes =
                    legaia_engine_cast::cast_module_camera::capture_spawn_pokes(prot_entry, va);
                for part in &mut scene.parts[first..] {
                    part.tag.get_or_insert(va);
                    part.seat_pokes.extend_from_slice(pokes);
                }
                // A seated stager-calling record makes its own call, so the
                // clock kept in its place stops.
                self.casting
                    .module_record_clocks
                    .retain(|c| !(prot_entry == 909 && c.0 == va));
            }
        }
    }
}
