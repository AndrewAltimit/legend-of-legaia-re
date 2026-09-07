//! The engine half of the action SM's **cast band**: who owes a cast's
//! outcome, when it lands, and the player-summon stager that stands in for
//! the per-summon overlay.
//!
//! ## Where retail folds a cast
//!
//! States `0x28..=0x2E` and `0x32..=0x38` of `FUN_801E295C` apply no damage
//! (the only `jal func_0x800402F4` in the dispatcher is the attack band's).
//! The magic band faces the caster, raises the monster-only spell-name
//! label, debits MP (`0x28`), and after the `0x14`-frame wait (`0x29`) runs
//! the party trigger `FUN_801DBF9C` and stages the cast's first anim byte;
//! the *outcome* is the streamed module's (`FUN_8003EC70` pages it in) and
//! lands while the clips play - for a Seru cast, inside the per-summon
//! stager the summon band ticks through `FUN_801F1ED4`.
//!
//! The engine carries the same shape with one owner, [`PendingCast`]: the
//! menu confirm / monster pick arm the SM and park the resolved targets
//! here; the fold runs once, at retail's seam - the stager's strike for a
//! summon, the `0x29` exit for everything else - through
//! [`World::cast_spell_on_slots_prepaid`], because the band's own `0x28` has
//! already charged the MP. A cast that leaves the band by any other door
//! (a capture branch, a dead caster) is still folded once, at the band's end,
//! so no turn spends MP for nothing.
//!
//! ## The stager
//!
//! Retail's summon band calls `FUN_801F1ED4` at `0x34` entry, every frame
//! of `0x35`, and at `0x36`, where it **holds while the call returns
//! non-zero** (`bne v0,zero` at `0x801E4CB0`). That routine dispatches into
//! the streamed per-summon stager (extraction PROT `903..=934`, slot B
//! `0x801F69D8`): overlay MIPS code the engine cannot run. [`SummonStager`]
//! is the engine's own choreography behind the same seam, and it reports
//! "busy" exactly where retail reads the stager's return.
//!
//! What it stages is capture-pinned on the player Gimard cast
//! (`gimard_summon_start` / `_visible` / `_burning_attack`, read out of the
//! PCSX-Redux states' RAM - `ctx[+7]`, `ctx[+0x279]`, the actor table):
//!
//! * `0x33` - the caster on clip `9`, the summon seat (slot 7) empty.
//! * `0x36`, stager phase 6 - every party seat and living monster hidden
//!   (`+0x21C = 0xFF`, prim word `0`), the creature seated at slot 7 about
//!   [`SUMMON_SPAWN_BEHIND`] units **behind** the caster on the party side
//!   (`x=185, z=-2272` against the caster's `x=82, z=-542`), wearing the
//!   caster's facing (`0xFD9`), on its idle clip.
//! * `0x36`, stager phase 11 - the creature closer in (`z=-1606`,
//!   [`SUMMON_STRIKE_BEHIND`] behind the caster) on clip `1`, the walk, with
//!   the flame part-actors live and the damage numeral up.
//!
//! So the retail creature walks **in from behind the party** toward the
//! target while its effect parts play, and the damage lands mid-walk. The
//! stager here does the same with the pieces the engine has: it requests the
//! namesake creature spawn ([`World::pending_summon_spawn`]) at the spawn
//! point, idles it, stages the walk clip and glides it to the strike point,
//! folds the outcome there, lingers, and despawns it.
//!
//! The per-summon effect parts are staged with it. They used to be the open
//! half here ("the `0x180C` move-VM records are not staged"); the module's
//! spawn records now come out of the **cast-effect pool**
//! ([`legaia_asset::cast_effect_pool`], installed by the scene host) and stage
//! through [`World::spawn_cast_module_fx`] at the same first tick, because the
//! records are the shape the summon path already runs. What stays open is the
//! module's *code* half - lift, camera, phase machine, damage shape - and that
//! function's `NOT WIRED:` names the worklist rows it covers.
//!
//! Timing is the engine's: the retail durations are the stager's own phase
//! machine and are not dumped, so the frame counts below are chosen to land
//! the strike inside the band's `0x78`-frame sustain, the way the capture
//! shows it.

use super::*;

use vm::battle_action::{ActionCategory, ActionState, StepOutcome};

/// How far behind the caster (toward negative Z, the party side) the
/// creature is seated - the capture's `-2272 - (-542)`.
pub const SUMMON_SPAWN_BEHIND: i16 = 1730;
/// Where the walk ends and the outcome lands - the capture's
/// `-1606 - (-542)`.
pub const SUMMON_STRIKE_BEHIND: i16 = 1064;
/// Frames the creature idles at its spawn point before the walk.
const SUMMON_IDLE_FRAMES: u16 = 30;
/// Walk speed, world units per frame.
const SUMMON_WALK_STEP: i16 = 12;
/// Frames the creature stands at the strike point after the outcome.
const SUMMON_LINGER_FRAMES: u16 = 40;
/// A host with no creature to seat (a headless driver) still owes the
/// outcome: fold it after this many frames without a seat.
const SUMMON_UNSEATED_GRACE: u16 = 60;

/// A cast the action SM is carrying whose outcome is still owed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingCast {
    /// Casting actor slot (party or monster).
    pub caster: u8,
    /// The spell id (`actor[+0x1DF]`).
    pub spell_id: u8,
    /// Absolute actor slots the outcome folds onto.
    pub targets: Vec<u8>,
}

/// Where the stager is in its choreography.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummonPhase {
    /// Armed by the cast trigger (`0x29`); the first tick (`0x34` entry)
    /// requests the creature spawn.
    Armed,
    /// Creature requested / seated: idle, then walk in to the strike point.
    Approach,
    /// Outcome folded; the creature holds at the strike point.
    Linger,
    /// Creature despawned; the next tick reports not busy.
    Done,
}

/// One player summon in flight (see the module docs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummonStager {
    /// Casting party slot.
    pub caster: u8,
    /// The spell id (`actor[+0x1DF]`).
    pub spell_id: u8,
    pub phase: SummonPhase,
    frames: u16,
    /// Where the creature is seated (behind the caster).
    pub spawn: [i16; 3],
    /// Where the walk ends and the outcome lands.
    pub goal: [i16; 3],
}

impl World {
    /// Absolute actor slots a player cast lands on, from the submenu's
    /// resolution: a single-target shape takes the picked slot (enemy rows
    /// sit behind the party), a group shape takes the whole band.
    pub(in crate::world) fn spell_targets_for(
        &self,
        def: &crate::spells::SpellDef,
        target_row: crate::target_picker::CursorRow,
        target_slot: u8,
    ) -> Vec<u8> {
        use crate::spells::SpellTarget;
        use crate::target_picker::CursorRow;
        let party_count = self.party_count.clamp(1, 3);
        match def.target {
            SpellTarget::OneEnemy | SpellTarget::OneAlly | SpellTarget::SelfOnly => {
                let abs = match target_row {
                    CursorRow::Enemy => party_count + target_slot,
                    CursorRow::Ally => target_slot,
                };
                vec![abs]
            }
            SpellTarget::AllEnemies => (party_count..self.actors.len() as u8).collect(),
            SpellTarget::AllAllies => (0..party_count).collect(),
        }
    }

    /// The `+0x1DD` target byte a cast of `def` onto `targets` carries: a
    /// group shape is the group code the cast-begin facing store and the
    /// cue expander decode (`8` = the party, `9` = the enemy row), a single
    /// shape is the slot itself.
    fn cast_target_code(&self, def: &crate::spells::SpellDef, targets: &[u8], caster: u8) -> u8 {
        use crate::spells::SpellTarget;
        use vm::battle_target_group::{TARGET_GROUP_ENEMIES, TARGET_GROUP_PARTY};
        let caster_is_party = caster < self.party_count;
        match def.target {
            // Codes are in retail's absolute numbering: 8 = party, 9 = the
            // enemy row - whichever side the caster stands on.
            SpellTarget::AllEnemies => {
                if caster_is_party {
                    TARGET_GROUP_ENEMIES
                } else {
                    TARGET_GROUP_PARTY
                }
            }
            SpellTarget::AllAllies => {
                if caster_is_party {
                    TARGET_GROUP_PARTY
                } else {
                    TARGET_GROUP_ENEMIES
                }
            }
            _ => targets.first().copied().unwrap_or(caster),
        }
    }

    /// Arm the action SM's Magic band for `caster`'s `def` cast onto
    /// `targets`: the committed category-2 action retail's menu confirm
    /// leaves for `FUN_801E295C` to seed (`actor[+0x1DE] = 2`, `+0x1DF` =
    /// the spell id, `+0x1DD` = the target byte), plus the [`PendingCast`]
    /// that owes the outcome.
    ///
    /// The band then does what retail's does: faces the target and debits
    /// the ability-bit-scaled MP at `0x28`, waits `0x14` frames at `0x29`,
    /// runs the trigger (the summon route for a Seru id), and carries the
    /// clips; the outcome folds at the seam the module docs name.
    pub(in crate::world) fn arm_player_cast(
        &mut self,
        caster: u8,
        def: &crate::spells::SpellDef,
        targets: Vec<u8>,
    ) {
        let code = self.cast_target_code(def, &targets, caster);
        self.clear_action_stream(caster);
        if let Some(a) = self.actors.get_mut(caster as usize) {
            a.battle.active_target = code;
            a.battle.action_category = ActionCategory::Magic.as_byte();
            a.battle.params[0] = def.id;
            a.battle.sub_route = 0;
        }
        self.pending_cast = Some(PendingCast {
            caster,
            spell_id: def.id,
            targets,
        });
        self.battle_ctx.active_actor = caster;
        self.battle_ctx.queued_action = ActionCategory::Magic.as_byte();
        self.battle_ctx.action_state = ActionState::Begin.as_byte();
    }

    /// The monster twin of [`Self::arm_player_cast`]. A monster's stream
    /// carries its cast clip behind the spell id (`params[1]`, the entry of
    /// its archive action table whose tag is the spell id - the `+0x1DA`
    /// stage the `0x29` arm makes), terminated at `params[2]`; a monster
    /// with no such clip installed stages the terminator at once and the
    /// band ends the action after the wait.
    pub(in crate::world) fn arm_monster_cast(
        &mut self,
        slot: u8,
        def: &crate::spells::SpellDef,
        targets: Vec<u8>,
    ) {
        let code = self.cast_target_code(def, &targets, slot);
        let clip = self
            .actors
            .get(slot as usize)
            .and_then(|a| a.battle_action_clips.as_ref())
            .and_then(|clips| {
                clips
                    .iter()
                    .position(|c| c.as_ref().is_some_and(|c| c.action_id == def.id))
            })
            .and_then(|i| u8::try_from(i).ok())
            .unwrap_or(0xFF);
        self.clear_action_stream(slot);
        if let Some(a) = self.actors.get_mut(slot as usize) {
            a.battle.active_target = code;
            a.battle.action_category = ActionCategory::Magic.as_byte();
            a.battle.params[0] = def.id;
            a.battle.params[1] = clip;
            a.battle.params[2] = 0xFF;
            a.battle.sub_route = 0;
        }
        self.pending_cast = Some(PendingCast {
            caster: slot,
            spell_id: def.id,
            targets,
        });
        self.battle_ctx.active_actor = slot;
        self.battle_ctx.queued_action = ActionCategory::Magic.as_byte();
        self.battle_ctx.action_state = ActionState::Begin.as_byte();
    }

    /// Fold the owed cast's outcome - exactly once. The MP was the band's
    /// (`0x28`), so the fold is the prepaid one; a monster cast also rolls
    /// its move's impact-status and AGL-status procs onto what it reached.
    pub(in crate::world) fn fold_pending_cast(&mut self) {
        let Some(pc) = self.pending_cast.take() else {
            return;
        };
        let Some(def) = self.spell_catalog.get(pc.spell_id).cloned() else {
            return;
        };
        let hit_fx_start = self.battle_hit_fx.len();
        self.cast_spell_on_slots_prepaid(pc.caster, &def, &pc.targets);
        if pc.caster >= self.party_count {
            self.apply_enemy_move_status(pc.caster, def.id, hit_fx_start);
            self.apply_enemy_agl_status(pc.caster, def.id, &pc.targets);
        }
    }

    /// The live loop's post-step glue for the cast band: fold the owed cast
    /// at retail's seam.
    ///
    /// * A non-summon cast folds the frame the SM leaves `0x29` for the anim
    ///   chain (the module has been paged in and the first clip staged); the
    ///   party trigger already folded the `< 0x25` arm's cast, so this is
    ///   the monster's fold.
    /// * The summon route (`0x29 -> 0x32`) leaves the fold to the stager's
    ///   strike.
    /// * Any door out of the bands (`0x50` and above: done, the capture
    ///   branch) folds whatever is still owed, so a cast never spends its MP
    ///   for nothing.
    pub(in crate::world) fn settle_cast_band(&mut self, outcome: &StepOutcome) {
        let Some(pc) = &self.pending_cast else {
            return;
        };
        if pc.caster != self.battle_ctx.active_actor {
            return;
        }
        let StepOutcome::Transition { from, to } = *outcome else {
            return;
        };
        let left_precast = from == ActionState::MagicPreCastWait.as_byte()
            && to != ActionState::MagicPreCastWait.as_byte()
            && to != ActionState::SummonInvoke.as_byte();
        let band_over = to >= ActionState::DoneCleanup.as_byte();
        if left_precast || band_over {
            self.fold_pending_cast();
        }
    }

    /// Arm the stager for `caster`'s `spell_id` cast. Called from the cast
    /// trigger (`FUN_801DBF9C`'s `>= 0x25` arm); the band's first stager
    /// tick does the spawn.
    pub(in crate::world) fn arm_summon_stager(&mut self, caster: u8, spell_id: u8) {
        let (cx, cy, cz) = self
            .actors
            .get(caster as usize)
            .map(|a| {
                (
                    a.move_state.world_x,
                    a.move_state.world_y,
                    a.move_state.world_z,
                )
            })
            .unwrap_or((0, 0, -542));
        self.summon_stager = Some(SummonStager {
            caster,
            spell_id,
            phase: SummonPhase::Armed,
            frames: 0,
            spawn: [cx, cy, cz.saturating_sub(SUMMON_SPAWN_BEHIND)],
            goal: [cx, cy, cz.saturating_sub(SUMMON_STRIKE_BEHIND)],
        });
    }

    /// A host seated the summon creature at actor `slot`: adopt the seat,
    /// place it at the stager's spawn point wearing the caster's facing (the
    /// capture's slot-7 record), and mark it active. Hosts call this right
    /// after binding the creature's mesh, idle player and clip set.
    pub fn seat_summon_actor(&mut self, slot: usize) {
        let staged = self.summon_stager.as_ref().map(|st| {
            (
                st.spawn,
                self.actors
                    .get(st.caster as usize)
                    .map(|a| a.battle.facing_angle)
                    .unwrap_or(0),
            )
        });
        self.summon_actor_slot = Some(slot as u8);
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

    /// Install the cast-effect pool - the DATA half of the slot-B cast-module
    /// band (PROT 0903..0966), parsed off the disc by the scene host (which
    /// holds the PROT index; `World` is index-agnostic, the same split
    /// [`Self::pending_summon_spawn`](crate::world::World::pending_summon_spawn)
    /// uses). Idempotent; a host that never calls it leaves every cast staging
    /// no module records, which is the disc-free behaviour.
    pub fn install_cast_effect_pool(
        &mut self,
        pool: std::sync::Arc<legaia_asset::cast_effect_pool::CastEffectPool>,
    ) {
        self.cast_effect_pool = Some(pool);
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
        use vm::battle_cast_dispatch::{seru_spell_emitter, spell_class_emitter};
        if self.spell_table_class(spell_id) == Some(legaia_asset::spell_names::CAPTURE_CLASS) {
            let sub = self.spell_effect_class(spell_id)?;
            return spell_class_emitter(sub, 0).module;
        }
        seru_spell_emitter(spell_id, 0).module
    }

    /// The record's `+0x01` **effect class**: the disc table's byte when one is
    /// installed, else the catalog record's
    /// ([`crate::spells::SpellDef::effect_class`], which
    /// [`crate::retail_magic`] fills from `SCUS_942.54`).
    pub fn spell_effect_class(&self, spell_id: u8) -> Option<u8> {
        self.spell_table_sub_class(spell_id)
            .or_else(|| self.spell_catalog.get(spell_id).map(|d| d.effect_class))
    }

    /// Stage the cast module's **spawn records** at `origin` - the engine's
    /// answer to the dispatchers, and the half of a cast that is data.
    ///
    /// Each record is `[i16 model_sel][u16 flags][move-VM bytecode]`, the shape
    /// the whole spawn stack shares, so the records run through the same
    /// [`crate::summon::SummonScene`] the summon and move-FX paths already use
    /// and both hosts draw and tick them with no host change
    /// (`active_summon_part_draws` / `tick_summon`).
    ///
    /// Returns `false` - staging nothing - when the spell names no band entry,
    /// no pool is installed (disc-free), or the module carries no record (the
    /// band's null stub PROT 0926, and PROT 0952 whose two spawn sites load
    /// their record pointer out of a saved register).
    ///
    /// NOT WIRED: the module's **code** half. Of the per-address verdicts in
    /// `docs/subsystems/cast-module.md`'s worklist this stages the **DATA**
    /// rows and nothing else - every **PORT** row (the six tick bodies
    /// `0x801F6A0C` / `0x801F6A14` / `0x801F6DD8` / `0x801F6EDC` / `0x801F74E4`
    /// / `0x801F798C`, and the seven stagers that also touch state:
    /// `0x801F75BC`, `0x801F7740`, `0x801F7AF4`, `0x801F85A8`, `0x801F8B90`,
    /// `0x801F8D64`, `0x801F90E4`) stays unported, because each writes
    /// simulation state a record cannot express: the staged-clip bytes
    /// `+0x1DA`/`+0x1DC` (the lift), the module phase `ctx+0x279` and
    /// `ctx+0x278` (the camera / phase machine), the HP write `+0x14C` and the
    /// status byte `+0x16E` (the damage shape). The engine's own cast damage
    /// runs through [`World::cast_spell_on_slots_prepaid`] at the band's seam
    /// instead, so no HP outcome is lost - only retail's per-phase timing of
    /// it. The **SCOPE-IGNORE** rows are the six null stagers and the one
    /// unreferenced routine; there is nothing to stage for them either.
    ///
    /// REF: FUN_801F1ED4, FUN_801F2160 (the dispatchers that name the module;
    /// their emitter arms are the unported half above)
    /// REF: FUN_80050ED4, FUN_80021B04 (the spawn calls whose `a2` records
    /// these are)
    pub fn spawn_cast_module_fx(&mut self, spell_id: u8, origin: [i16; 3]) -> bool {
        let Some(entry) = self.cast_module_for(spell_id) else {
            return false;
        };
        let Some(pool) = self.cast_effect_pool.clone() else {
            return false;
        };
        let Some(module) = pool.module(entry) else {
            return false;
        };
        if module.parts.is_empty() {
            return false;
        }
        self.active_summon = Some(crate::summon::SummonScene::spawn_parts(
            &module.parts,
            &module.bytes,
            crate::scene::EFFECT_MODEL_LIBRARY_BASE,
            origin,
        ));
        true
    }

    /// One stager tick - the engine body behind
    /// `BattleActionHost::summon_stager_tick`. Returns `true` while the
    /// choreography is still running (retail: the stager's non-zero return
    /// the `0x36` arm holds on).
    ///
    /// PORT: FUN_801F1ED4 (the dispatch seam; the choreography is the
    /// engine's - see the module docs)
    pub fn summon_stager_tick(&mut self) -> bool {
        let Some(mut st) = self.summon_stager.take() else {
            return false;
        };
        let busy = match st.phase {
            SummonPhase::Armed => {
                // Phase 0: seat the creature (retail: the stager's
                // `FUN_801F19EC` installs the streamed record as slot 7).
                self.pending_summon_spawn = Some((st.spell_id, st.spawn));
                // ...and stage the module's own effect parts. This is the
                // `0x801E4B1C` site's other half: `FUN_801F1ED4` dispatches
                // into the paged module, whose spawn records are the cast's
                // particle layer. Seated where the creature is, since the
                // records carry summon-local offsets.
                self.spawn_cast_module_fx(st.spell_id, st.spawn);
                st.phase = SummonPhase::Approach;
                st.frames = 0;
                true
            }
            SummonPhase::Approach => {
                st.frames = st.frames.saturating_add(1);
                let seat = self
                    .summon_actor_slot
                    .filter(|&s| self.actors.get(s as usize).is_some_and(|a| a.active));
                let strike = match seat {
                    Some(slot) => {
                        st.frames > SUMMON_IDLE_FRAMES
                            && self.summon_walk_step(slot as usize, st.goal)
                    }
                    None => st.frames >= SUMMON_UNSEATED_GRACE,
                };
                if strike {
                    self.fold_pending_cast();
                    st.phase = SummonPhase::Linger;
                    st.frames = 0;
                }
                true
            }
            SummonPhase::Linger => {
                st.frames = st.frames.saturating_add(1);
                if st.frames == 1
                    && let Some(slot) = self.summon_actor_slot
                    && let Some(a) = self.actors.get_mut(slot as usize)
                {
                    // Back to the idle loop for the hold.
                    a.battle.queued_anim = 0;
                }
                if st.frames >= SUMMON_LINGER_FRAMES {
                    self.despawn_summon_actor();
                    st.phase = SummonPhase::Done;
                }
                true
            }
            SummonPhase::Done => false,
        };
        if busy {
            self.summon_stager = Some(st);
        }
        busy
    }

    /// Stage the walk clip (id `1`, the looping approach - the capture's
    /// `+0x1D9 == 1`) and glide the creature one step toward `goal`.
    /// Returns `true` on arrival.
    fn summon_walk_step(&mut self, slot: usize, goal: [i16; 3]) -> bool {
        let Some(a) = self.actors.get_mut(slot) else {
            return true;
        };
        if a.battle.queued_anim != 1 {
            a.battle.queued_anim = 1;
        }
        let ms = &mut a.move_state;
        let dx = i32::from(goal[0]) - i32::from(ms.world_x);
        let dz = i32::from(goal[2]) - i32::from(ms.world_z);
        let step = i32::from(SUMMON_WALK_STEP);
        if dx.abs() <= step && dz.abs() <= step {
            ms.world_x = goal[0];
            ms.world_z = goal[2];
            return true;
        }
        ms.world_x = (i32::from(ms.world_x) + dx.clamp(-step, step)) as i16;
        ms.world_z = (i32::from(ms.world_z) + dz.clamp(-step, step)) as i16;
        false
    }

    /// Retire the summon creature: the seat goes inactive so both hosts
    /// stop drawing it (native: the `active` gate of the battle draw loop;
    /// browser: the transform row's `active` float).
    pub(in crate::world) fn despawn_summon_actor(&mut self) {
        if let Some(slot) = self.summon_actor_slot.take()
            && let Some(a) = self.actors.get_mut(slot as usize)
        {
            a.active = false;
            a.battle_animation = None;
            a.pose_frame = None;
            a.battle.queued_anim = 0;
            a.battle.current_anim = 0;
        }
    }

    /// The full-screen fade quad to composite this frame, as
    /// `(rgb 0xRRGGBB, abr_mode, ot_index)` for
    /// `legaia_engine_ui::screen_prim::fade_prim` - `None` while no fade is
    /// live or its start delay is still running (retail's tick returns `-1`
    /// and draws nothing). The ABR mode is the template's kind word and the
    /// OT index the id `FUN_80024E80` stamped (`AddPrim(ot + id*4, ..)` in
    /// `FUN_80024EE4`).
    pub fn screen_fade_draw(&self) -> Option<(u32, u8, u32)> {
        let f = self.screen_fade.as_ref()?;
        if !f.visible() {
            return None;
        }
        let [r, g, b] = f.rgb();
        Some((
            (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b),
            f.kind.clamp(0, 3) as u8,
            f.mode[2].max(0) as u32,
        ))
    }
}
