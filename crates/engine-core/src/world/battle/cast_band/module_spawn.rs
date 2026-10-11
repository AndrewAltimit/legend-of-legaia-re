//! Seating a cast module's actors and effects: the summon actor seat, the cast
//! effect pool, module lookup, and the module FX and arm spawns.

use super::*;
use legaia_engine_mode::mode::SceneMode;

impl World {
    /// A host seated the summon creature at actor `slot`: adopt the seat,
    /// place it at the stager's spawn point wearing the caster's facing (the
    /// capture's slot-7 record), and mark it active. Hosts call this right
    /// after binding the creature's mesh, idle player and clip set.
    pub fn seat_summon_actor(&mut self, slot: usize) {
        let staged = self.casting.summon_stager.as_ref().map(|st| {
            (
                st.spawn,
                self.actors
                    .get(st.caster as usize)
                    .map(|a| a.battle.facing_angle)
                    .unwrap_or(0),
            )
        });
        self.casting.summon_actor_slot = Some(slot as u8);
        let Some(a) = self.actors.get_mut(slot) else {
            return;
        };
        a.active = true;
        // A debug spawn outside a cast keeps the host's own placement.
        let Some((spawn, facing)) = staged else {
            return;
        };
        a.move_state.world_x = spawn[0];
        a.move_state.world_y = spawn[1];
        a.move_state.world_z = spawn[2];
        a.battle.facing_angle = facing;
        a.battle.queued_anim = 0;
        a.battle.current_anim = 0;
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
    /// [`legaia_engine_battle_vm::cast_module_ticks`], driven from
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
        if entry == legaia_engine_battle_vm::cast_fatal_decision::FATAL_DECISION_ENTRY {
            return false;
        }
        self.casting.active_summon = Some(legaia_engine_effects::summon::SummonScene::spawn_parts(
            &module.parts,
            &module.bytes,
            crate::scene::EFFECT_MODEL_LIBRARY_BASE,
            origin,
        ));
        true
    }

    /// Seat the spawn calls one module arm made this pass, each on its own
    /// anchor, into the cast's running scene - the per-arm form of
    /// [`Self::spawn_cast_module_fx`] for a module whose director reports its
    /// spawns ([`vm::cast_module_camera::ModuleProfile::stages_spawns`]).
    ///
    /// A record's program is bounded by the next record start, so each one
    /// is parsed against the whole record set of its image: the module's own
    /// recovered records for a [`SpawnRecord::Module`], the effect-prototype
    /// table's for a [`SpawnRecord::BattleProto`].
    ///
    /// REF: FUN_80021B04 (the spawn calls), FUN_80058490 (the `MoveImage`)
    ///
    /// [`SpawnRecord::Module`]: vm::cast_module_camera::SpawnRecord::Module
    /// [`SpawnRecord::BattleProto`]: vm::cast_module_camera::SpawnRecord::BattleProto
    pub(super) fn stage_module_arm_spawns(
        &mut self,
        prot_entry: u32,
        spawns: &[legaia_engine_battle_vm::cast_module_camera::ModuleSpawn],
        shot: Option<legaia_engine_battle_vm::cast_module_camera::ModuleShot>,
    ) {
        use legaia_engine_battle_vm::cast_module_camera::SpawnRecord;
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
            .map(
                |a| legaia_engine_battle_vm::cast_module_camera::ModuleSeat {
                    x: a.move_state.world_x,
                    y: a.move_state.world_y,
                    z: a.move_state.world_z,
                    facing: a.battle.facing_angle & 0xFFF,
                },
            );
        for spawn in spawns {
            let Some((pos, rot)) = legaia_engine_battle_vm::cast_module_camera::spawn_anchor_point(
                spawn.anchor,
                creature,
                victim,
                shot,
            ) else {
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
            match self.casting.active_summon.as_mut() {
                Some(scene) => scene.push_parts(&picked, &bytes, pos, rot),
                None => {
                    let mut scene = legaia_engine_effects::summon::SummonScene::spawn_parts(
                        &[],
                        &bytes,
                        crate::scene::EFFECT_MODEL_LIBRARY_BASE,
                        pos,
                    );
                    scene.push_parts(&picked, &bytes, pos, rot);
                    self.casting.active_summon = Some(scene);
                }
            }
        }
    }
}
