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
//! * `0x36`, stager phase 11 - the creature closer in (`z=-1606`) on clip
//!   `1`, the walk, with the flame part-actors live and the victim still at
//!   full HP: the walk arm holds on the range poll `FUN_8004E2F0(7, victim)`
//!   and lands the hit only once the creature reaches the victim.
//!
//! So the retail creature walks **in from behind the party** onto the target
//! while its effect parts play, and the damage lands when it arrives. The
//! stager here does the same with the pieces the engine has: it requests the
//! namesake creature spawn ([`crate::world::CastFxState::pending_summon_spawn`]) at the spawn
//! point, idles it, stages the walk clip and walks it onto the victim until
//! the range metric reads in range (a module with no directed walk arm glides
//! it to the fixed strike point [`SUMMON_STRIKE_BEHIND`] instead), folds the
//! outcome there, lingers, and despawns it.
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
/// Where an undirected module's walk ends and the outcome lands. A directed
/// walk arm walks onto the victim instead (`summon_walk_to_victim`); this is
/// the `gimard_burning_attack` capture's mid-walk `-1606 - (-542)`.
pub const SUMMON_STRIKE_BEHIND: i16 = 1064;
/// Frames the creature idles at its spawn point before the walk.
const SUMMON_IDLE_FRAMES: u16 = 30;
/// Walk speed, world units per frame.
const SUMMON_WALK_STEP: i16 = 12;
/// A directed walk arm's creature speed, world units per display frame,
/// along its heading onto the victim. Measured off `gimard_burning_attack`
/// (PROT 0903 arm 11): the yaw base has swung `0x5D9 - 0x200 = 985` at
/// `6 * scalar` (`48`) a display frame - 20.5 frames into the walk - and the
/// creature stands 670 units on from its arm-3 seat (`z -2276 -> -1606`),
/// `32.7` a frame. The walk is the clip's root motion, which the anim tick's
/// root-motion term integrates for the creature like any body; this measured
/// constant only walks a creature whose playing clip carries no speed.
const SUMMON_DIRECTED_WALK_STEP: i32 = 32;
/// Frames the creature stands at the strike point after the outcome.
const SUMMON_LINGER_FRAMES: u16 = 40;
/// A host with no creature to seat (a headless driver) still owes the
/// outcome: fold it after this many frames without a seat.
const SUMMON_UNSEATED_GRACE: u16 = 60;

/// The arm of PROT 0927's and PROT 0966's nine that carries the sweep - the
/// `0x801F6A60` / `0x801F6A50` table's working entry. The other eight are
/// spawn arms the pool already stages.
pub const AOE_STAGER_WORKING_ARM: u8 = 4;

/// The screen fade PROT 0966's arm `phase` spawns on its pass, each a
/// `FUN_80024E80(0x801C9070, 1)` over the template the arm writes:
///
/// | arm | kind | frames | from | to | delay / hold |
/// |---|---|---|---|---|---|
/// | 2 (`0x801F6EE8`) | 1 | `0x10` | `(0xFF, 0x40, 0x40)` | black | - |
/// | 4 (`0x801F6FC0`) | 1 | `0x40` | black | white | held |
/// | 10 (`0x801F70A0`) | 1 | `0x80` | white | black | - |
/// | 26 (`0x801F8574`) | 1 | `0x40` | black | white | `0xC0` delay, held |
/// | 27 (`0x801F879C`) | 1 | `0x80` | white | black | - |
///
/// Arm 26 spawns three fades at once (a white flash, a held blue grade, and
/// the delayed white-in); the engine has one fade seat, so it carries the
/// last, which is the one still on screen when arm 27 fades out of it.
fn evil_seru_magic_fade(phase: u8) -> Option<crate::fade::FadeTemplate> {
    let (duration, start_rgb, end_rgb, mode) = match phase {
        2 => (0x10, [0xFF, 0x40, 0x40], [0; 3], [0, 0, 0]),
        4 => (0x40, [0; 3], [0xFF; 3], [0, -1, 0]),
        10 | 27 => (0x80, [0xFF; 3], [0; 3], [0, 0, 0]),
        26 => (0x40, [0; 3], [0xFF; 3], [0xC0, -1, 0]),
        _ => return None,
    };
    Some(crate::fade::FadeTemplate {
        kind: 1,
        duration,
        start_rgb,
        end_rgb,
        mode,
    })
}

/// Combat seats in retail's actor table `DAT_801C9370` - three party rows and
/// five monster rows. The slot-B AoE sweeps' `ctx[+0]` / `ctx[+1]` bounds are
/// indices into that span, so the engine's own extra seats (the summon
/// creature at 7 and up, host debug actors) are outside both.
pub const BATTLE_TABLE_SLOTS: usize = 8;

/// The actor's own halfword, or the world's per-slot mirror when the actor
/// has not carried one yet. Used to seed the stat block the slot-B kernels
/// debuff: `seed_party_battle_stats` fills the mirrors, not the actor, so a
/// zero here would have Melt Spray compute on nothing.
fn nonzero_or(own: u16, mirror: Option<u16>) -> u16 {
    if own != 0 { own } else { mirror.unwrap_or(0) }
}

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

/// The walk entry a melee body stages while its caster closes in (the
/// literal `1` of every `li v1,0x1; sb v1,0x1da(<caster>)` walk site).
pub const CAPTURE_WALK_ENTRY: u8 = 1;

/// Longest the band waits on the caster's stages before it gives up and lets
/// the module run - a guard against a clip that never commits, not a timing.
pub const CASTER_STAGE_TICK_LIMIT: u16 = 900;

/// The caster's clip stages a capture-class body owes and its port does not
/// write ([`legaia_engine_vm::cast_module_ticks::CAPTURE_CASTER_STAGES`]),
/// in flight at the head of battle phase `0x70`.
///
/// Each stage is written into the caster's `+0x1DA` and the next one is
/// written behind it as soon as it commits, so the commit's boundary rule
/// plays every clip to its natural end; the last is followed by `0`, retail's
/// closing `sb zero,0x1DA(<caster>)`, and the run ends when the caster is
/// back on its idle entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CasterStageRun {
    /// The casting seat.
    pub slot: u8,
    /// The clips to stage, in order.
    pub clips: Vec<u8>,
    /// Index of the clip being staged / waited on.
    pub cursor: usize,
    /// The seat the caster walks into reach of before the first stage
    /// ([`vm::cast_module_ticks::capture_body_approaches`]); `None` for a
    /// body that strikes from where it stands, or a group target.
    pub approach: Option<u8>,
    /// Where the run is.
    pub phase: CasterStagePhase,
    /// Ticks spent holding the band so far ([`CASTER_STAGE_TICK_LIMIT`]).
    pub ticks: u16,
}

/// Whether the clip actor `a` has committed has run as far as it will on its
/// own: played out, or reached its authored loop window (a park, or the start
/// of a looping tail) with cycles still owed.
fn caster_clip_settled(a: &crate::world::Actor) -> bool {
    let Some(p) = a.battle_animation.as_ref() else {
        return true;
    };
    if p.finished() {
        return true;
    }
    let window_start = a
        .battle_action_clips
        .as_ref()
        .and_then(|c| c.get(usize::from(a.battle.current_anim)))
        .and_then(|c| c.as_ref())
        .and_then(|c| c.entry_loop_window())
        .map(|(_, start, _)| i16::from(start));
    matches!(window_start, Some(w) if p.loop_cycles_remaining() > 0 && p.current_frame() >= w)
}

/// The three stretches of a [`CasterStageRun`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CasterStagePhase {
    /// Walking into reach: the walk entry `1` staged and the caster turned
    /// onto its victim every tick (its root motion carries it), until the
    /// range poll `FUN_8004E2F0` reads zero.
    Approach,
    /// Staging the clips: each is held until it has committed and either
    /// played out or reached its authored loop-window park.
    Staging,
    /// The caster sits on the last clip (parked, when it has a window) while
    /// the module's own arms run.
    Module,
    /// The module is done: the park is released (`sh zero,0x176(<caster>)`)
    /// and idle staged behind it (`sb zero,0x1DA(<caster>)`); the band holds
    /// until the caster is back on its idle entry.
    Closing,
}

/// What [`World::capture_stager_tick`] does with the module this tick.
enum CasterStageGate {
    /// The caster's stages hold the band; the module does not run.
    Hold,
    /// Run the module.
    RunModule,
    /// The run closed out: leave `0x70`.
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
    /// The creature's walk has reached [`Self::goal`] (the answer a directed
    /// module's walk arm polls).
    pub walked: bool,
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
        let party_count = self.party.party_count.clamp(1, 3);
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
        let caster_is_party = caster < self.party.party_count;
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
        self.casting.pending_cast = Some(PendingCast {
            caster,
            spell_id: def.id,
            targets,
        });
        self.battle_ctx.active_actor = caster;
        self.battle_ctx.queued_action = ActionCategory::Magic.as_byte();
        self.battle_ctx.action_state = ActionState::Begin.as_byte();
    }

    /// The monster twin of [`Self::arm_player_cast`]. A monster's stream
    /// carries its cast clip behind the spell id (`params[1]`, the `+0x1E0`
    /// byte the `0x29` arm stages into `+0x1DA` - see
    /// [`Self::monster_cast_clip`]), then its opening camera shot
    /// (`params[2]`, `+0x1E1`, a case of the cast-effect driver - see
    /// `SpellAnimPairs::opening_shot`), terminated at `params[3]`
    /// (`FUN_801E9FD4`, `0x801EA53C..0x801EA588`); a monster with no such
    /// clip installed stages the terminator at once and the band ends the
    /// action after the wait.
    pub(in crate::world) fn arm_monster_cast(
        &mut self,
        slot: u8,
        def: &crate::spells::SpellDef,
        targets: Vec<u8>,
    ) {
        let code = self.cast_target_code(def, &targets, slot);
        let clip = self.monster_cast_clip(slot, def.id).unwrap_or(0xFF);
        // A table that was not read stages the terminator in the shot's
        // place, the stream the port carried before the shot existed.
        let shot = self
            .battle
            .spell_anim_pairs
            .opening_shot(def.id)
            .unwrap_or(0xFF);
        self.clear_action_stream(slot);
        if let Some(a) = self.actors.get_mut(slot as usize) {
            a.battle.active_target = code;
            a.battle.action_category = ActionCategory::Magic.as_byte();
            a.battle.params[0] = def.id;
            a.battle.params[1] = clip;
            a.battle.params[2] = shot;
            a.battle.params[3] = 0xFF;
            a.battle.sub_route = 0;
            // The picker stores the same entry into `+0x1E7` one instruction
            // ahead of `+0x1E0` (`sb s2,0x1e7(s4)` at `0x801EA53C`, and the
            // `OneAlly` re-pick's `sb s5,0x1e7(s4)` at `0x801EA6C4`). That is
            // the byte the action seed's **Spirit band** stages instead of
            // `+0x1E0` (`lbu v0,0x1e7(s3); sb v0,0x1da(s3)` at
            // `0x801E3B4C..0x801E3B54`), which every monster cast of a
            // class-`< 0x14` record below id `0x65` - the heals and buffs -
            // runs through. A walk that found no entry writes neither byte.
            if clip != 0xFF {
                a.battle.queued_anim_b = clip;
            }
        }
        self.casting.pending_cast = Some(PendingCast {
            caster: slot,
            spell_id: def.id,
            targets,
        });
        self.battle_ctx.active_actor = slot;
        self.battle_ctx.queued_action = ActionCategory::Magic.as_byte();
        self.battle_ctx.action_state = ActionState::Begin.as_byte();
    }

    /// The archive entry a monster's cast plays - the `+0x1E0` byte the
    /// picker `FUN_801E9FD4` stores beside the spell id.
    ///
    /// Retail's generic magic pick does not look the clip up by the spell id.
    /// It counts the record's live magic slots (`+0x21..+0x23` at `>= 2`,
    /// `0x801EA3E0..0x801EA408`), rolls `k = rand() % count`, then walks the
    /// entry table from index 2 counting the entries tagged `0x23`
    /// (`0x801EA4C8..0x801EA4D0`): the `k`-th one is the clip, and the spell
    /// is magic slot `count - 1 - k` (`+0x21 + (s1 - 1)` with `s1` counted
    /// down beside `k`, `0x801EA4E0..0x801EA544`). So the clip is keyed on the
    /// spell's **slot**, reversed, and on no tag the spell id names - Gimard's
    /// Tail Fire `0x27` plays his lone tag-`0x23` entry 8, which a search for
    /// tag `0x27` never finds (`battle_gimard_tail_fire_a` reads `+0x1E0 = 8`).
    ///
    /// A spell outside the record's magic slots (a per-monster scripted
    /// cast) falls back to the entry whose tag is the spell id.
    ///
    /// PORT: FUN_801E9FD4 (the tag-`0x23` clip walk, `0x801EA4A4..0x801EA548`)
    pub(in crate::world) fn monster_cast_clip(&self, slot: u8, spell_id: u8) -> Option<u8> {
        /// The tag the picker walks for a castable's clip.
        const CAST_CLIP_TAG: u8 = 0x23;
        /// The walk starts past the idle and walk entries (`li s2,0x2`).
        const FIRST_CAST_ENTRY: usize = 2;
        let actor = self.actors.get(slot as usize)?;
        let clips = actor.battle_action_clips.as_ref()?;
        let magic = actor
            .battle_monster_id
            .and_then(|id| self.tables.monster_catalog.get(id))
            .map(|d| d.magic_attacks.as_slice())
            .unwrap_or(&[]);
        if let Some(m) = magic.iter().position(|&id| id == spell_id) {
            let k = magic.len() - 1 - m;
            return clips
                .iter()
                .enumerate()
                .skip(FIRST_CAST_ENTRY)
                .filter(|(_, c)| c.as_ref().is_some_and(|c| c.action_id == CAST_CLIP_TAG))
                .nth(k)
                .and_then(|(i, _)| u8::try_from(i).ok());
        }
        clips
            .iter()
            .position(|c| c.as_ref().is_some_and(|c| c.action_id == spell_id))
            .and_then(|i| u8::try_from(i).ok())
    }

    /// Fold the owed cast's outcome - exactly once. The MP was the band's
    /// (`0x28`), so the fold is the prepaid one; a monster cast also rolls
    /// its move's impact-status and AGL-status procs onto what it reached.
    pub(in crate::world) fn fold_pending_cast(&mut self) {
        let Some(pc) = self.casting.pending_cast.take() else {
            return;
        };
        // A capture body that took a branch with no damage site (PROT 0953's
        // charge, `0x801F6CEC`) owes nothing: retail never reaches its
        // `FUN_801DD6B4` on that path.
        if std::mem::take(&mut self.casting.module_skips_fold) {
            return;
        }
        // Two casts in the band do not fold through the catalog at all:
        // PROT 0927 and PROT 0966 apply their damage inside the module's own
        // `0x801F6734` stager, over a seat range the spell record cannot
        // express and with a clamp that cannot kill. Where the module owns
        // the outcome, run the module - and do not also run the generic fold,
        // which would apply the hit twice.
        if self
            .run_cast_module_aoe_for(pc.caster, pc.spell_id)
            .is_some()
        {
            return;
        }
        // ...and neither do the three whole-row **tick** sweeps, once they
        // have run. PROT 0938's two bodies and PROT 0965's each roll their
        // own baked power per hittable seat and store the clamped net into
        // `+0x14C` themselves, over `actor_table[0 .. ctx[+0]]` - a seat
        // range the spell record cannot express, exactly like the two
        // stagers above. The one difference is the clamp: these kill, the
        // stagers cannot.
        //
        // The `phase > sweep_arm` test is what keeps this from *losing* an
        // outcome: a host that never drove the band past the sweep arm has
        // had no module damage applied, and skipping the generic fold there
        // would leave the cast owed forever instead of double-applied.
        if let Some(entry) = self.cast_module_for(pc.spell_id)
            && let Some(body) = vm::cast_module_ticks::capture_tick_body(entry, pc.spell_id)
            && vm::cast_module_ticks::tick_body_owns_the_fold(entry, body)
            && let Some(arm) = vm::cast_module_ticks::sweep_arm_for(entry, body)
            && self.casting.module_phase > arm
        {
            return;
        }
        // --- W1-D: the fourteen trampoline arms ---
        // The same property for the three whole-row sweeps among them (PROT
        // 0941's `0xB9`, 0950's `0xAB`, 0956's `0x71`): each rolls its own
        // baked power per hittable seat and stores the clamped net into
        // `+0x14C` itself, over a seat range the spell record cannot express.
        // The `phase > arm` test is the same guard - a host that never drove
        // the band past the sweep arm is still owed the generic fold.
        if let Some(entry) = self.cast_module_for(pc.spell_id)
            && let Some(body) = vm::cast_module_ticks::capture_tick_body(entry, pc.spell_id)
            && let Some(arm) = vm::cast_arm_ticks::arm_sweep_arm(entry, body)
            && self.casting.module_phase > arm
        {
            return;
        }
        // --- end W1-D ---
        // A monster's capture-class special is not in the catalog (its record
        // is the module's), so the fold resolves it the way the arm did -
        // and only when the module has a damage site to seed it with; a
        // status-only body (Glare) folds nothing here.
        let def = self
            .tables
            .spell_catalog
            .get(pc.spell_id)
            .cloned()
            .or_else(|| {
                self.monster_capture_power(pc.caster, pc.spell_id)
                    .and_then(|_| self.monster_cast_def(pc.spell_id))
            });
        let Some(def) = def else {
            return;
        };
        let hit_fx_start = self.battle.hit_fx.len();
        self.cast_spell_on_slots_prepaid(pc.caster, &def, &pc.targets);
        if pc.caster >= self.party.party_count {
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
        let Some(pc) = &self.casting.pending_cast else {
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
        // The capture band (`0x6E..=0x71`) sits above `0x50` in the state
        // space but is not a door out: its module has not run yet, so its
        // fold waits for the band's own exit into `0x50`, after the caster's
        // stages and the module's arms - where retail's module lands its hit.
        let capture_band = (ActionState::MagicCaptureBranch.as_byte()
            ..=ActionState::MagicCaptureFinalize.as_byte())
            .contains(&to);
        let band_over = to >= ActionState::DoneCleanup.as_byte() && !capture_band;
        if left_precast || band_over {
            self.fold_pending_cast();
        }
    }

    /// Arm the stager for `caster`'s `spell_id` cast. Called from the cast
    /// trigger (`FUN_801DBF9C`'s `>= 0x25` arm); the band's first stager
    /// tick does the spawn.
    ///
    /// Public because it is the band's arming *seam*, not an internal step:
    /// the trigger runs it for a host's cast, and a parity oracle that drives
    /// the band a frame at a time has to reach the same door rather than a
    /// second one of its own.
    pub fn arm_summon_stager(&mut self, caster: u8, spell_id: u8) {
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
        self.casting.summon_seat_owed = None;
        // Retail's cast-start site `0x801E4B1C` zeroes `ctx+0x278` and the
        // module phase `ctx+0x279` before the first tick.
        self.casting.module_phase = 0;
        self.casting.module_ctx_278 = 0;
        // A new cast re-arms the Nighto verdict; the roll happens on the first
        // tick of the module, the way retail's arm 0 draws it. The ring sweep
        // starts from zero for the same reason.
        self.casting.module_nighto_outcome = None;
        self.casting.module_ring_angle = 0;
        self.casting.module_theeder = Default::default();
        self.casting.module_swordie = Default::default();
        self.casting.module_cam = Default::default();
        self.casting.summon_stager = Some(SummonStager {
            caster,
            spell_id,
            phase: SummonPhase::Armed,
            frames: 0,
            spawn: [cx, cy, cz.saturating_sub(SUMMON_SPAWN_BEHIND)],
            goal: [cx, cy, cz.saturating_sub(SUMMON_STRIKE_BEHIND)],
            walked: false,
        });
        self.emit_cast_module_voice(spell_id);
    }

    /// Raise the paged module's **own** CD-XA voice - the head cue every
    /// slot-B image calls the dispatcher `FUN_8004FCC8` with near its head
    /// (`docs/subsystems/cast-module.md`, "The cast's own CD-XA voice"),
    /// scanned off the module's bytes at cast time
    /// ([`legaia_engine_vm::battle_cast_cue::module_head_cue`]) and run
    /// through the dispatcher's CD-XA arm
    /// ([`legaia_engine_vm::battle_cast_cue::admit_voice_cue`]).
    ///
    /// Retail raises it from inside the module's first tick, after the
    /// module's own poll of the side-band stage byte; the port raises it at
    /// the arming seam, the frame the module becomes resident - which is
    /// the same frame, since the port's side-band is resident rather than
    /// streamed (`VoiceCueGates::side_band_stage` is `0` here for that
    /// reason). The other gate is live: a cast inside the previous clip's
    /// read span (`battle_xa_busy_frames`, retail's `gp+0x91C`) plays no
    /// voice, exactly as `FUN_8003DE7C(1)` declines it.
    ///
    /// The request lands on [`crate::world::AudioState::battle_xa_cues`],
    /// the `(clip_slot, channel, dur)` channel both hosts already play
    /// through their XA lane, and arms the busy span the way the melee
    /// kernel's clip start does. Nothing here touches gameplay state; a
    /// disc-free build (no pool, no span table) emits nothing.
    ///
    /// REF: FUN_8004FCC8 (the CD-XA arm), FUN_8003D53C (the starter the
    /// request stands for)
    fn emit_cast_module_voice(&mut self, spell_id: u8) {
        let Some(entry) = self.cast_module_for(spell_id) else {
            return;
        };
        let Some(pool) = self.casting.effect_pool.clone() else {
            return;
        };
        let Some(module) = pool.module(entry) else {
            return;
        };
        let Some(head) = vm::battle_cast_cue::module_head_cue(&module.bytes) else {
            return;
        };
        let id = head.resolve(|| self.next_rand());
        self.emit_battle_xa_cue(id);
    }

    /// Run one CD-XA cue id through the dispatcher's CD-XA arm
    /// ([`legaia_engine_vm::battle_cast_cue::admit_voice_cue`]) onto
    /// [`crate::world::AudioState::battle_xa_cues`] - the half of
    /// [`Self::emit_cast_module_voice`] after the module's head cue is
    /// resolved, and what a module arm's own `FUN_8004FCC8(id >= 0x100)` call
    /// raises.
    ///
    /// REF: FUN_8004FCC8 (the CD-XA arm)
    pub(in crate::world) fn emit_battle_xa_cue(&mut self, id: u16) {
        let raw = self
            .audio
            .xa_cue_durations
            .as_deref()
            .and_then(|t| t.get(usize::from(id).wrapping_sub(0x100)).copied());
        let gates = vm::battle_cast_cue::VoiceCueGates {
            side_band_stage: 0,
            clip_span_left: self.audio.battle_xa_busy_frames,
        };
        if let vm::battle_cast_cue::VoiceCueVerdict::Play(req) =
            vm::battle_cast_cue::admit_voice_cue(id, gates, raw)
        {
            // The starter holds the drive for the clip's read span
            // (`FUN_8003D53C`; the same latch `push_battle_xa_cue` arms for
            // the melee kernel's clips).
            self.audio.battle_xa_busy_frames = req.duration_sectors.min(u16::MAX as u32) as u16;
            self.audio.battle_xa_cues.push(crate::sfx_cue::XaVoiceClip {
                clip: req.clip_slot,
                channel: req.channel,
                duration_sectors: req.duration_sectors,
            });
        }
    }

    /// Every cast-voice clip `spell_id`'s module can raise - the
    /// [`Self::emit_cast_module_voice`] resolution without its gates or its
    /// coin flip: a literal head cue gives one request, a random one
    /// (`base + rand() % span`, PROT 0936 / 0937) one per candidate. Empty
    /// when the module, its head cue or the span table is missing.
    pub(in crate::world) fn cast_module_voice_candidates(
        &self,
        spell_id: u8,
    ) -> Vec<crate::sfx_cue::XaVoiceClip> {
        let Some(entry) = self.cast_module_for(spell_id) else {
            return Vec::new();
        };
        let Some(module) = self
            .casting
            .effect_pool
            .as_ref()
            .and_then(|pool| pool.module(entry))
        else {
            return Vec::new();
        };
        let ids: Vec<u16> = match vm::battle_cast_cue::module_head_cue(&module.bytes) {
            Some(vm::battle_cast_cue::ModuleHeadCue::Literal(id)) => vec![id],
            Some(vm::battle_cast_cue::ModuleHeadCue::Random { base, span }) => {
                (0..u16::from(span.max(1)))
                    .map(|k| base.wrapping_add(k))
                    .collect()
            }
            None => Vec::new(),
        };
        let open = vm::battle_cast_cue::VoiceCueGates {
            side_band_stage: 0,
            clip_span_left: 0,
        };
        ids.into_iter()
            .filter_map(|id| {
                let raw = self
                    .audio
                    .xa_cue_durations
                    .as_deref()
                    .and_then(|t| t.get(usize::from(id).wrapping_sub(0x100)).copied());
                match vm::battle_cast_cue::admit_voice_cue(id, open, raw) {
                    vm::battle_cast_cue::VoiceCueVerdict::Play(req) => {
                        Some(crate::sfx_cue::XaVoiceClip {
                            clip: req.clip_slot,
                            channel: req.channel,
                            duration_sectors: req.duration_sectors,
                        })
                    }
                    _ => None,
                }
            })
            .collect()
    }

    /// List the cast voices the round's committed party spells may raise onto
    /// [`crate::world::AudioState::battle_xa_prestage`] - called at the
    /// round's start, before the first action dispatches.
    pub fn list_round_cast_voices(&mut self) {
        let spells: Vec<u8> = self
            .battle
            .round_flow
            .pending
            .iter()
            .filter_map(|p| match p {
                Some(crate::battle_round::PendingPartyAction::Spell { spell_id, .. }) => {
                    Some(*spell_id)
                }
                _ => None,
            })
            .collect();
        for spell in spells {
            for clip in self.cast_module_voice_candidates(spell) {
                if !self.audio.battle_xa_prestage.contains(&clip) {
                    self.audio.battle_xa_prestage.push(clip);
                }
            }
        }
    }

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
        pool: std::sync::Arc<legaia_asset::cast_effect_pool::CastEffectPool>,
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
    /// [`crate::summon::SummonScene`] the summon and move-FX paths already use
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
    /// [`legaia_engine_vm::cast_module_ticks`], driven from
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
        if entry == vm::cast_fatal_decision::FATAL_DECISION_ENTRY {
            return false;
        }
        self.casting.active_summon = Some(crate::summon::SummonScene::spawn_parts(
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
    fn stage_module_arm_spawns(
        &mut self,
        prot_entry: u32,
        spawns: &[vm::cast_module_camera::ModuleSpawn],
        shot: Option<vm::cast_module_camera::ModuleShot>,
    ) {
        use legaia_asset::move_power::{self, BATTLE_OVERLAY_BASE};
        use vm::cast_module_camera::SpawnRecord;
        const LINK_BASE: u32 = legaia_asset::summon_overlay::SUMMON_OVERLAY_LINK_BASE;
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
            .map(|a| vm::cast_module_camera::ModuleSeat {
                x: a.move_state.world_x,
                y: a.move_state.world_y,
                z: a.move_state.world_z,
                facing: a.battle.facing_angle & 0xFFF,
            });
        for spawn in spawns {
            let Some((pos, rot)) =
                vm::cast_module_camera::spawn_anchor_point(spawn.anchor, creature, victim, shot)
            else {
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
                    let parts =
                        legaia_asset::summon_overlay::parse_records_at(&module.bytes, &offs);
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
                    let parts = legaia_asset::summon_overlay::parse_records_at(&overlay, &offs);
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
                    let mut scene = crate::summon::SummonScene::spawn_parts(
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

    /// One stager tick - the engine body behind
    /// `BattleActionHost::summon_stager_tick`. Returns `true` while the
    /// choreography is still running (retail: the stager's non-zero return
    /// the `0x36` arm holds on).
    ///
    /// PORT: FUN_801F1ED4 (the dispatch seam; the choreography is the
    /// engine's - see the module docs)
    pub fn summon_stager_tick(&mut self) -> bool {
        let Some(mut st) = self.casting.summon_stager.take() else {
            return false;
        };
        // Retail re-enters the paged module every frame from this seam; the
        // band's PORT rows are the code that runs there.
        let module_arm = self.casting.module_phase;
        // The creature's live seat and walk state, as a directed module's
        // walk arm reads them.
        let seat_live = self
            .casting
            .summon_actor_slot
            .filter(|&s| self.actors.get(s as usize).is_some_and(|a| a.active))
            .and_then(|s| self.actors.get(s as usize))
            .map(|a| vm::cast_module_camera::ModuleSeat {
                x: a.move_state.world_x,
                y: a.move_state.world_y,
                z: a.move_state.world_z,
                facing: a.battle.facing_angle & 0xFFF,
            });
        self.casting.module_cam.creature_live = seat_live;
        self.casting.module_cam.creature_arrived = st.walked;
        let placed_before = self.casting.module_cam.creature.is_some();
        let run = self.run_cast_module_code(st.spell_id, module_arm);
        // The module seats its creature itself (Gimard's arm 3); put the
        // engine's creature where it did.
        if !placed_before
            && let Some(c) = self.casting.module_cam.creature
            && let Some(slot) = self.casting.summon_actor_slot
            && let Some(a) = self.actors.get_mut(slot as usize)
        {
            a.move_state.world_x = c.x;
            a.move_state.world_z = c.z;
            a.battle.facing_angle = c.facing;
        }
        // A module whose arms are paced by their own countdown
        // (`vm::cast_module_camera`) owns the band's length the way retail's
        // does: `0x36` holds on the module's return, and the outcome lands on
        // the module's hit arm rather than on the engine's walk-in.
        let profile = run
            .as_ref()
            .and_then(|r| vm::cast_module_camera::module_profile(r.prot_entry))
            .filter(|p| p.paces_band());
        // A module whose director reports its spawn calls seats each record
        // on the pass its arm makes the call, after the arm's own creature
        // placement above (Gimard's arm 3 spawns on the seat it just set).
        let stages_spawns = run
            .as_ref()
            .and_then(|r| vm::cast_module_camera::module_profile(r.prot_entry))
            .is_some_and(|p| p.stages_spawns);
        if let Some(r) = run.as_ref().filter(|_| stages_spawns) {
            if let Some(m) = r.vram_move {
                let i = |v: u16| v as i16;
                self.battle.vram_moves.push(crate::world::ScriptVramMove {
                    src: (i(m.src.0), i(m.src.1)),
                    size: (i(m.size.0), i(m.size.1)),
                    dst: (i(m.dst.0), i(m.dst.1)),
                });
            }
            if !r.spawns.is_empty() {
                self.stage_module_arm_spawns(r.prot_entry, r.spawns, r.camera_shot);
            }
            if let Some(c) = r.caption {
                self.casting.module_caption = Some(c);
            }
            // The arm's own fades run beside the band's flash: retail's
            // spawner takes a fresh pool actor per call.
            if r.kills_fades {
                self.presentation.module_fades.clear();
            }
            for (t, id) in r.fades {
                let mut seat = None;
                crate::fade::spawn_fade(&mut seat, &crate::fade::summon_template(t), *id);
                self.presentation.module_fades.extend(seat);
            }
        }
        let directed_hit = profile.and_then(|p| p.hit_arm);
        let module_busy =
            profile.is_some() && run.as_ref().is_some_and(|r| r.tick_ported && r.busy);
        let module_phase = run.as_ref().map_or(0, |r| r.phase);
        if let Some(shot) = run.as_ref().and_then(|r| r.camera_shot)
            && let Some(cam) = self.battle.camera.as_mut()
        {
            let (pose, raw_z) = shot.pose();
            cam.arm_module_shot(pose, raw_z, u32::from(shot.frames));
        }
        if let Some(n) = run.as_ref().and_then(|r| r.camera_nudge)
            && let Some(cam) = self.battle.camera.as_mut()
        {
            cam.nudge_module(n.pitch, n.tr_y, n.tr_z);
        }
        if let Some(f) = run.as_ref().and_then(|r| r.camera_follow)
            && let Some(cam) = self.battle.camera.as_mut()
        {
            let actor = legaia_engine_vm::battle_cam_script::BattleCamActor {
                facing: i32::from(f.seat.facing),
                world: [
                    f32::from(f.seat.x),
                    f32::from(f.seat.y),
                    f32::from(f.seat.z),
                ],
                height: None,
            };
            cam.arm_module_follow(actor, f.yaw_base, f.depth_raw);
        }
        // The module's seat arm (`FUN_801F19EC`): a seat owed since the
        // stager armed is requested as the module reaches it.
        let seat_arm = run
            .as_ref()
            .and_then(|r| vm::cast_module_camera::module_profile(r.prot_entry))
            .and_then(|p| p.seat_arm);
        if let Some((arm, spell, at)) = self.casting.summon_seat_owed
            && (module_phase >= arm || run.is_none())
        {
            self.casting.summon_seat_owed = None;
            self.casting.pending_summon_spawn = Some((spell, at));
        }
        let busy = match st.phase {
            SummonPhase::Armed => {
                // Phase 0: seat the creature (retail: the stager's
                // `FUN_801F19EC` installs the streamed record as slot 7) -
                // here, unless the module seats it in a later arm.
                match seat_arm.filter(|&a| module_phase < a) {
                    Some(arm) => {
                        self.casting.summon_seat_owed = Some((arm, st.spell_id, st.spawn));
                    }
                    None => {
                        self.casting.pending_summon_spawn = Some((st.spell_id, st.spawn));
                    }
                }
                // ...and stage the module's own effect parts. This is the
                // `0x801E4B1C` site's other half: `FUN_801F1ED4` dispatches
                // into the paged module, whose spawn records are the cast's
                // particle layer. Seated where the creature is, since the
                // records carry summon-local offsets.
                if !stages_spawns {
                    self.spawn_cast_module_fx(st.spell_id, st.spawn);
                }
                st.phase = SummonPhase::Approach;
                st.frames = 0;
                true
            }
            SummonPhase::Approach => {
                st.frames = st.frames.saturating_add(1);
                let seat = self
                    .casting
                    .summon_actor_slot
                    .filter(|&s| self.actors.get(s as usize).is_some_and(|a| a.active));
                // A directed module walks its creature in its own walk arm,
                // not after the engine's idle count.
                let may_walk = match profile {
                    Some(p) => p.walk_arm.is_some_and(|w| module_phase >= w),
                    None => st.frames > SUMMON_IDLE_FRAMES,
                };
                // A directed walk arm walks the creature onto the **victim**
                // and holds on the range poll `FUN_8004E2F0(7, victim)`
                // (PROT 0903's arm 11, `bne v0,zero` at `0x801F7418`); the
                // engine's fixed goal stands in only where no module walks.
                let victim = profile
                    .filter(|p| p.walk_arm.is_some())
                    .map(|_| self.actors.get(st.caster as usize))
                    .and_then(|c| c.map(|c| c.battle.active_target))
                    .filter(|&v| self.actors.get(usize::from(v)).is_some_and(|a| a.active));
                let walked = match (seat, victim) {
                    (Some(slot), Some(v)) => may_walk && self.summon_walk_to_victim(slot, v),
                    (Some(slot), None) => may_walk && self.summon_walk_step(slot as usize, st.goal),
                    (None, _) => {
                        self.casting.summon_seat_owed.is_none()
                            && st.frames >= SUMMON_UNSEATED_GRACE
                    }
                };
                st.walked = walked;
                let strike = match directed_hit {
                    Some(hit) => module_phase > hit || !module_busy,
                    None => walked,
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
                    && let Some(slot) = self.casting.summon_actor_slot
                    && let Some(a) = self.actors.get_mut(slot as usize)
                {
                    // Back to the idle loop for the hold.
                    a.battle.queued_anim = 0;
                }
                if st.frames >= SUMMON_LINGER_FRAMES && !module_busy {
                    self.despawn_summon_actor();
                    st.phase = SummonPhase::Done;
                }
                true
            }
            SummonPhase::Done => false,
        };
        if busy {
            self.casting.summon_stager = Some(st);
        }
        busy
    }

    /// One tick of the **capture band's** resident module - the engine body
    /// behind `BattleActionHost::capture_stager_tick`, and the twin of
    /// [`Self::summon_stager_tick`] on the other dispatcher.
    ///
    /// Retail's battle phase `0x70` re-enters `FUN_801F2160` every frame
    /// (`jal` at `0x801E50C8`) and holds while it returns non-zero
    /// (`bne v0,zero` at `0x801E50D0`); the return is the selected slot-B
    /// module tick's own return. So this runs the resident module's code half
    /// ([`Self::run_cast_module_code`]) and reports what it reported.
    ///
    /// The one deviation, and it is deliberate: a module whose tick body is
    /// **not** ported reports "not busy" rather than
    /// [`CastModuleCodeRun::busy`]'s seeded `true`. Holding on an unported
    /// row would park the band forever, which is a softlock and not a
    /// fidelity gain; [`CastModuleCodeRun::tick_ported`] is what distinguishes
    /// the two.
    ///
    /// PORT: FUN_801F2160 (the dispatch seam; the per-module tick bodies are
    /// [`legaia_engine_vm::cast_module_ticks`])
    pub fn capture_stager_tick(&mut self) -> bool {
        let Some(spell_id) = self.casting.capture_spell else {
            return false;
        };
        // The caster's own stages the body's port does not write: the
        // wind-up plays first, the module's arms run with the caster on its
        // park, and the close releases the park once they are done.
        match self.step_caster_stages() {
            CasterStageGate::Hold => return true,
            CasterStageGate::Done => {
                self.casting.capture_spell = None;
                return false;
            }
            CasterStageGate::RunModule => {}
        }
        let arm = self.casting.module_phase;
        let Some(run) = self.run_cast_module_code(spell_id, arm) else {
            if self.close_caster_stages() {
                return true;
            }
            self.casting.capture_spell = None;
            return false;
        };
        // `0x70` frames nothing of its own: the camera is the module's.
        if let Some(cam) = self.battle.camera.as_mut() {
            if let Some(shot) = run.camera_shot {
                let (pose, raw_z) = shot.pose();
                cam.arm_module_shot(pose, raw_z, u32::from(shot.frames));
            }
            if let Some(d) = run.capture_drift {
                cam.drift_module(d.pitch, d.yaw, d.tr_y, d.tr_z);
                if d.tr_x != 0 {
                    cam.drift_module_tr_x(d.tr_x);
                }
            }
        }
        if run.tick_ported && run.busy {
            return true;
        }
        if self.close_caster_stages() {
            return true;
        }
        // The band is leaving `0x70`; the module stops being re-entered.
        self.casting.capture_spell = None;
        self.casting.fatal_banner = None;
        false
    }

    /// Arm [`Self::capture_stager_tick`] for `spell_id`'s module - the pager
    /// seam retail runs at its `0x6F` exit, where `sb zero,0x279(v0)`
    /// (`0x801E5048`) also zeroes the module phase before phase `0x70` starts
    /// ticking it.
    pub(in crate::world) fn arm_capture_cast_module(&mut self, spell_id: u8) {
        if self.cast_module_for(spell_id).is_none() {
            return;
        }
        self.casting.module_phase = 0;
        self.casting.module_ctx_278 = 0;
        self.casting.module_skips_fold = false;
        self.casting.fatal_decision = None;
        self.casting.fatal_banner = None;
        self.casting.module_swordie = Default::default();
        self.casting.module_cam = Default::default();
        self.casting.module_beam_counter = 0;
        self.casting.module_beam_live = false;
        self.casting.module_hit_arm = None;
        self.casting.module_settle_ticks = 0;
        self.casting.capture_spell = Some(spell_id);
        self.casting.caster_stages = self.caster_stage_run_for(spell_id);
        self.emit_cast_module_voice(spell_id);
    }

    /// The [`CasterStageRun`] a capture-class cast of `spell_id` by the
    /// acting seat owes, when its body's port stages nothing on the caster
    /// ([`vm::cast_module_ticks::capture_caster_stages`]). Only a monster seat
    /// with installed clips plays one, and a stage naming an entry the
    /// caster's record does not carry is dropped - retail would index past
    /// the record's offset array there (the Gobu Gobu Curse fault on
    /// `docs/subsystems/cast-module.md`).
    fn caster_stage_run_for(&self, spell_id: u8) -> Option<CasterStageRun> {
        let entry = self.cast_module_for(spell_id)?;
        let slot = self.battle_ctx.active_actor;
        let actor = self.actors.get(usize::from(slot))?;
        let seat_monster = (actor.battle_monster_id? & 0xFF) as u8;
        let installed = actor.battle_action_clips.as_ref()?;
        let approach = vm::cast_module_ticks::capture_body_approaches(entry, spell_id)
            .then_some(actor.battle.active_target)
            .filter(|&t| t < 8 && usize::from(t) < self.actors.len() && t != slot);
        let clips: Vec<u8> = vm::cast_module_ticks::capture_caster_stages(
            entry,
            spell_id,
            seat_monster,
            self.battle_first_monster_byte(),
        )
        .unwrap_or_default()
        .into_iter()
        .filter(|&c| {
            installed
                .get(usize::from(c))
                .is_some_and(|c| c.as_ref().is_some_and(|c| c.frame_count > 0))
        })
        .collect();
        (!clips.is_empty() || approach.is_some()).then_some(CasterStageRun {
            slot,
            clips,
            cursor: 0,
            approach,
            phase: if approach.is_some() {
                CasterStagePhase::Approach
            } else {
                CasterStagePhase::Staging
            },
            ticks: 0,
        })
    }

    /// Advance the [`CasterStageRun`] by one band tick.
    fn step_caster_stages(&mut self) -> CasterStageGate {
        let Some(mut run) = self.casting.caster_stages.take() else {
            return CasterStageGate::RunModule;
        };
        let Some(a) = self.actors.get_mut(usize::from(run.slot)) else {
            return CasterStageGate::RunModule;
        };
        if run.phase != CasterStagePhase::Module {
            run.ticks = run.ticks.saturating_add(1);
            if run.ticks > CASTER_STAGE_TICK_LIMIT {
                // A clip that never committed: hand the band back rather
                // than hold it.
                if let Some(p) = a.battle_animation.as_mut() {
                    p.release_loop_window();
                }
                a.battle.queued_anim = 0;
                return if run.phase == CasterStagePhase::Closing {
                    CasterStageGate::Done
                } else {
                    CasterStageGate::RunModule
                };
            }
        }
        let gate = match run.phase {
            CasterStagePhase::Approach => {
                let victim = run.approach.unwrap_or(run.slot);
                if self.battle_range_metric(run.slot, victim) == 0 {
                    // Arrived. A body whose port stages its own strike takes
                    // the band from here; the rest go on to their stages.
                    run.ticks = 0;
                    let a = &mut self.actors[usize::from(run.slot)];
                    if a.battle.queued_anim == CAPTURE_WALK_ENTRY {
                        a.battle.queued_anim = 0;
                    }
                    if run.clips.is_empty() {
                        run.phase = CasterStagePhase::Module;
                        self.casting.caster_stages = Some(run);
                        return CasterStageGate::RunModule;
                    }
                    run.phase = CasterStagePhase::Staging;
                    self.casting.caster_stages = Some(run);
                    return CasterStageGate::Hold;
                }
                // `FUN_80019B28(victim, caster) + 0x800` onto the caster's
                // facing each tick, then the walk stage (`li v1,0x1;
                // sb v1,0x1da(<caster>)`, e.g. `0x801F72C8` in PROT 0952).
                let (vx, vz) = self.battle_seat_of(usize::from(victim));
                let a = &mut self.actors[usize::from(run.slot)];
                let bearing = vm::battle_action::bearing_12bit_approx(
                    vz,
                    vx,
                    a.move_state.world_z,
                    a.move_state.world_x,
                );
                a.battle.facing_angle = bearing.wrapping_add(0x800) & 0xFFF;
                a.battle.queued_anim = CAPTURE_WALK_ENTRY;
                CasterStageGate::Hold
            }
            CasterStagePhase::Staging => {
                let want = run.clips[run.cursor];
                if run.ticks == 1 {
                    a.battle.queued_anim = want;
                    CasterStageGate::Hold
                } else if a.battle.current_anim == want && caster_clip_settled(a) {
                    if run.cursor + 1 < run.clips.len() {
                        // A wind-up's park is released for the stage that
                        // follows it.
                        if let Some(p) = a.battle_animation.as_mut() {
                            p.release_loop_window();
                        }
                        run.cursor += 1;
                        a.battle.queued_anim = run.clips[run.cursor];
                        CasterStageGate::Hold
                    } else {
                        run.phase = CasterStagePhase::Module;
                        CasterStageGate::RunModule
                    }
                } else {
                    CasterStageGate::Hold
                }
            }
            CasterStagePhase::Module => CasterStageGate::RunModule,
            CasterStagePhase::Closing if run.clips.is_empty() => CasterStageGate::Done,
            CasterStagePhase::Closing => {
                if a.battle.current_anim == 0 {
                    return CasterStageGate::Done;
                }
                CasterStageGate::Hold
            }
        };
        self.casting.caster_stages = Some(run);
        gate
    }

    /// The module reported done: release the caster's park and stage idle
    /// behind it. `true` when a [`CasterStageRun`] is now closing (the band
    /// holds until the caster is back on idle).
    fn close_caster_stages(&mut self) -> bool {
        let Some(run) = self.casting.caster_stages.as_mut() else {
            return false;
        };
        if run.phase != CasterStagePhase::Module || run.clips.is_empty() {
            return false;
        }
        run.phase = CasterStagePhase::Closing;
        if let Some(a) = self.actors.get_mut(usize::from(run.slot)) {
            if let Some(p) = a.battle_animation.as_mut() {
                p.release_loop_window();
            }
            a.battle.queued_anim = 0;
            if a.battle.current_anim == 0 {
                self.casting.caster_stages = None;
                return false;
            }
        }
        true
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

    /// A directed walk arm's step: stage the walk clip, glide the creature
    /// one step toward `victim`'s seat, and report arrival on the range poll
    /// retail's arm holds on - `FUN_8004E2F0(7, victim)` reading `0`. Retail
    /// turns the creature onto the victim every pass (`sh v0,0x46(s3)` at
    /// `0x801F73C4`); the creature's own `+0x1F` reads `0` in the
    /// `gimard_burning_attack` capture, so the size class is the victim's.
    fn summon_walk_to_victim(&mut self, slot: u8, victim: u8) -> bool {
        let Some(a) = self.actors.get(usize::from(slot)) else {
            return true;
        };
        let pos = (a.move_state.world_x, a.move_state.world_z);
        if self.creature_range_metric(pos, 0, victim) == 0 {
            return true;
        }
        let (vx, vz) = self.battle_seat_of(usize::from(victim));
        // `FUN_80019B28(victim, creature) + 0x800` (`0x801F73A4..0x801F73C4`).
        let bearing = vm::battle_action::bearing_12bit_approx(vz, vx, pos.1, pos.0);
        let facing = bearing.wrapping_add(0x800) & 0xFFF;
        // The walk is the creature clip's own root motion: the anim tick's
        // positive-speed term steps it along this facing while the range
        // poll against its target `+0x1DD` (the victim -
        // `gimard_burning_attack` reads `3`) still fails
        // (`World::drive_playing_root_motion`). Stepping it here as well
        // walked it twice - 58 units a tick against retail's ~30 (the
        // capture's creature `+0x21D = 4` halves the clip's speed). The
        // measured constant stays for a creature with no root speed to
        // play (a headless seat with no clip).
        let root_driven = self
            .battle_playing_root_motion(usize::from(slot))
            .is_some_and(|(speed, _)| speed > 0);
        if let Some(a) = self.actors.get_mut(usize::from(slot)) {
            a.battle.facing_angle = facing;
            a.battle.active_target = victim;
            if a.battle.queued_anim != 1 {
                a.battle.queued_anim = 1;
            }
            if root_driven {
                return false;
            }
            let ms = &mut a.move_state;
            let (dx, dz) = (
                i32::from(vx) - i32::from(ms.world_x),
                i32::from(vz) - i32::from(ms.world_z),
            );
            if dx.abs() + dz.abs() <= SUMMON_DIRECTED_WALK_STEP {
                ms.world_x = vx;
                ms.world_z = vz;
            } else {
                let (sin, cos) = vm::battle_action::motion::trig12(facing);
                ms.world_x = (i32::from(ms.world_x)
                    + ((i32::from(sin) * SUMMON_DIRECTED_WALK_STEP) >> 12))
                    as i16;
                ms.world_z = (i32::from(ms.world_z)
                    + ((i32::from(cos) * SUMMON_DIRECTED_WALK_STEP) >> 12))
                    as i16;
            }
        }
        let Some(a) = self.actors.get(usize::from(slot)) else {
            return true;
        };
        let pos = (a.move_state.world_x, a.move_state.world_z);
        self.creature_range_metric(pos, 0, victim) == 0 || pos == (vx, vz)
    }

    /// Retire the summon creature: the seat goes inactive so both hosts
    /// stop drawing it (native: the `active` gate of the battle draw loop;
    /// browser: the transform row's `active` float).
    pub(in crate::world) fn despawn_summon_actor(&mut self) {
        if let Some(slot) = self.casting.summon_actor_slot.take()
            && let Some(a) = self.actors.get_mut(slot as usize)
        {
            a.active = false;
            a.battle_animation = None;
            a.pose_frame = None;
            a.battle.queued_anim = 0;
            a.battle.current_anim = 0;
        }
    }

    /// PROT 0948's beam counter while its builder ran on this module tick -
    /// the one input `legaia_engine_ui::cast_beam::cross_beam_prims` builds
    /// the frame's beam packets from, on both hosts. `None` outside arm 3.
    pub fn cross_beam_draw(&self) -> Option<i32> {
        (self.mode == SceneMode::Battle && self.casting.module_beam_live)
            .then_some(self.casting.module_beam_counter)
    }

    /// The full-screen fade quad to composite this frame, as
    /// `(rgb 0xRRGGBB, abr_mode, ot_index)` for
    /// `legaia_engine_ui::screen_prim::fade_prim` - `None` while no fade is
    /// live or its start delay is still running (retail's tick returns `-1`
    /// and draws nothing). The ABR mode is the template's kind word and the
    /// OT index the id `FUN_80024E80` stamped (`AddPrim(ot + id*4, ..)` in
    /// `FUN_80024EE4`).
    pub fn screen_fade_draw(&self) -> Option<(u32, u8, u32)> {
        fade_draw(self.presentation.fade.as_ref()?)
    }

    /// Every live full-screen fade quad this frame, the world's fade first
    /// and then the module fades running beside it
    /// ([`crate::world::ScreenFxState::module_fades`]), each as
    /// [`Self::screen_fade_draw`] gives it. Both hosts composite the list.
    pub fn screen_fade_draws(&self) -> Vec<(u32, u8, u32)> {
        self.presentation
            .fade
            .iter()
            .chain(self.presentation.module_fades.iter())
            .filter_map(fade_draw)
            .collect()
    }
}

/// One fade's quad, `None` while its start delay runs.
fn fade_draw(f: &crate::fade::FadeState) -> Option<(u32, u8, u32)> {
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

// ---------------------------------------------------------------------------
// The band's PORT half: the slot-B module code kernels
// ---------------------------------------------------------------------------

/// The equipment-slot index the first accessory occupies: record `+0x196` is
/// slot 0 and `+0x19B` - the byte PROT 0955's Void Accessories arm forms as
/// `0x80084140 + (char - 1) * 0x414 + 0x75E + 5 + slot` - is slot 5.
const ACCESSORY_EQUIP_SLOT_0: usize = 5;

/// Where a module's code half wrote, so a caller can see the kernel ran
/// without re-reading every actor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CastModuleCodeRun {
    /// The band entry that was resident (`903 + row` / `935 + sub_id`).
    pub prot_entry: u32,
    /// The module phase after the tick (`ctx+0x279`).
    pub phase: u8,
    /// `ctx+0x278` after the tick.
    pub ctx_278: u8,
    /// Seats an AoE stager swept, with the amount applied to each.
    pub aoe_hits: Vec<vm::cast_module_ticks::AoeHit>,
    /// `true` while the module's phase machine still reports busy.
    ///
    /// Seeded `true` for a resident module and only *lowered* by a ported
    /// tick body, so it is meaningless on its own - read it together with
    /// [`Self::tick_ported`].
    pub busy: bool,
    /// Whether a ported tick body actually ran this frame. `false` means the
    /// entry is resident but its code half is one of the band's unported
    /// rows, and then [`Self::busy`] carries no information: a caller that
    /// held on it would hold forever.
    pub tick_ported: bool,
    /// The item PROT 0955's turn-steal arms handed back to the bag
    /// (`FUN_800421D4(victim[+0x1DF], 1)`), when the victim had an Item
    /// action queued.
    pub item_refund: Option<u8>,
    /// What PROT 0955's Void Accessories arm decided this frame.
    pub voided_accessory: Option<vm::cast_module_ticks::VoidAccessoriesOutcome>,
    /// `(element, group)` PROT 0964's Element Change committed onto the first
    /// monster seat's record this frame (`+0x1D` / `+0x1C`).
    pub element_change: Option<(u8, u8)>,
    /// The camera shot the module armed this frame
    /// ([`vm::cast_module_camera::ModuleShot`]).
    pub camera_shot: Option<vm::cast_module_camera::ModuleShot>,
    /// The case-6 follow the module re-armed this frame
    /// ([`vm::cast_module_camera::ModuleFollow`]).
    pub camera_follow: Option<vm::cast_module_camera::ModuleFollow>,
    /// The drift the module wrote into the camera globals this frame.
    pub camera_nudge: Option<vm::cast_module_camera::ModuleNudge>,
    /// A capture-class body's drift this frame
    /// ([`vm::cast_module_camera::capture_camera_director`]).
    pub capture_drift: Option<vm::cast_module_camera::CaptureDrift>,
    /// The spawn calls the module's arm made this frame, for a director that
    /// reports them ([`vm::cast_module_camera::ModuleProfile::stages_spawns`]).
    pub spawns: &'static [vm::cast_module_camera::ModuleSpawn],
    /// The `MoveImage` the module's arm issued this frame.
    pub vram_move: Option<vm::cast_module_camera::ModuleVramMove>,
    /// The text the module's arm put up this frame.
    pub caption: Option<vm::cast_module_camera::ModuleCaption>,
    /// The fades the module's arm spawned this frame, each `(template, id)`.
    pub fades: &'static [(vm::battle_action::SummonFadeTemplate, i16)],
    /// The arm killed the module's earlier fades first.
    pub kills_fades: bool,
}

// --- W1-D: the fourteen trampoline arms ---
/// The clip PROT 0941's Steal stages on its caster in arm `0`.
///
/// Retail picks it with `FUN_80050E2C(record + 0x4C, 1, record[+0x4A])` - a
/// draw over the monster record's own clip list, which the engine has no
/// equivalent for. `1` is the approach clip every other body in the band
/// stages, and the arm's own contribution - the **OR** restage `+0x1DC |= 1`
/// rather than the band's usual bump - is what the port carries exactly.
const CAST_STEAL_RUN_CLIP: u8 = 1;
// --- end W1-D ---

impl World {
    /// Lift one actor slot into the state view the slot-B kernels take.
    ///
    /// The engine carries every field these routines touch except two: the
    /// `+0x0C` root-speed word and the `+0x1DC` restage counter, which live
    /// in the run's own scratch because the engine's `flag_bits` byte at that
    /// offset is a flag set, not a counter (`docs/subsystems/battle-action.md`
    /// and `docs/subsystems/cast-module.md` read `+0x1DC` differently, and the
    /// bytes here only ever increment it).
    fn cast_actor_state(&self, slot: u8) -> vm::cast_module_ticks::CastActorState {
        use vm::cast_module_ticks::{ANIM_RATE_NORMAL, CastActorState};
        let Some(a) = self.actors.get(slot as usize) else {
            return CastActorState {
                anim_rate: ANIM_RATE_NORMAL,
                ..Default::default()
            };
        };
        CastActorState {
            root_speed: 0,
            hp_bar_delta: a.battle.hp_bar_pending,
            hp: a.battle.hp,
            flags: a.battle.field_flags,
            playing_anim: a.battle.current_anim,
            staged_anim: a.battle.queued_anim,
            restage: 0,
            target_code: a.battle.active_target,
            // `+0x1F1` sits inside the per-action parameter stream that starts
            // at `+0x1DF`.
            knockdown_anim: a.battle.params.get(0x1F1 - 0x1DF).copied().unwrap_or(0),
            render_flag: a.battle.render_flag,
            // The three mirrors the band's hit arms stamp beside the HP write
            // all have a home on the battle actor already, so they are seeded
            // and written back like every other view field rather than being
            // dropped at the seam.
            present_04: a.battle.render_color,
            render_21f: a.battle.impact_state,
            render_225: a.battle.capture_state,
            anim_rate: a.battle.anim_rate.get(),
            // The whole `+0x158..+0x16A` stat block, five `(working, base)`
            // pairs, now has a home on the battle actor - so every one of
            // Melt Spray's ten halfword stores and both of Power Charge's
            // `+0x15A` stores land instead of stopping at the view. The
            // defence pair is still kept in the world's per-slot split,
            // which is where the physical-defence facet reads it.
            agl: a.battle.agl,
            agl_base: a.battle.agl_base,
            atk: a.battle.atk_working,
            atk_base: a.battle.atk_base,
            udf: self.cast_defence_split(slot).0,
            udf_base: self.cast_defence_split(slot).0,
            ldf: self.cast_defence_split(slot).1,
            ldf_base: self.cast_defence_split(slot).1,
            // SPD and INT live in two places: the actor's own halfwords (new,
            // so the band's writers have somewhere to land) and the world's
            // per-slot mirrors, which are what turn order / the escape roll
            // (`battle_speed`) and the accuracy seed (`battle_accuracy`)
            // actually read. Seed from the mirror whenever the actor's own
            // halfword is still zero - `seed_party_battle_stats` fills the
            // mirrors, not the actor - so a debuff computes on a real number
            // instead of underflowing zero.
            spd: nonzero_or(a.battle.spd, self.battle.speed.get(slot as usize).copied()),
            spd_base: nonzero_or(
                a.battle.spd_base,
                self.battle.speed.get(slot as usize).copied(),
            ),
            intel: nonzero_or(
                a.battle.intel,
                self.battle.accuracy.get(slot as usize).copied(),
            ),
            intel_base: nonzero_or(
                a.battle.intel_base,
                self.battle.accuracy.get(slot as usize).copied(),
            ),
            spirit_gauge: a.battle.spirit_gauge,
            init_key: a.battle.init_key,
            action_category: a.battle.action_category,
            queued_action: a.battle.params.first().copied().unwrap_or(0),
            reaction_alt: a.battle.params.get(0x1EF - 0x1DF).copied().unwrap_or(0),
            reaction_alt2: a.battle.params.get(0x1F0 - 0x1DF).copied().unwrap_or(0),
            reaction_gate: a.battle.params.get(0x1F2 - 0x1DF).copied().unwrap_or(0),
            combo_total: a.battle.damage_accum,
            hits_pending: a.battle_staged_anim.is_some()
                && a.battle_animation
                    .as_ref()
                    .and_then(|p| p.hit_source())
                    .is_some_and(|src| {
                        let in_band = src.power_run[0]
                            .wrapping_sub(vm::battle_action::HIT_POWER_BASE)
                            < vm::battle_action::HIT_POWER_SPAN;
                        let next = src.event_frames.get(usize::from(a.battle.input_cursor));
                        in_band && next.is_some_and(|&f| f != 0)
                    }),
        }
    }

    /// The live UDF / LDF pair for one battle slot, out of the world's own
    /// per-slot defence split (the same store the physical-defence facet
    /// reads).
    fn cast_defence_split(&self, slot: u8) -> (u16, u16) {
        self.battle
            .defense_split
            .get(slot as usize)
            .copied()
            .flatten()
            .unwrap_or((0, 0))
    }

    /// The monster **record**'s AGL (`+0x0E`) for one battle seat - what
    /// PROT 0942's Power Up arm reads through `0x801C9348[seat - 3]` before
    /// it writes `caster[+0x156]`.
    ///
    /// Retail's table only covers the monster seats; a party caster indexes
    /// out of it, so the engine falls back to the actor's own AGL base rather
    /// than reading whatever sits below the table.
    fn cast_record_agl(&self, slot: u8) -> u16 {
        self.actors
            .get(slot as usize)
            .and_then(|a| a.battle_monster_id)
            .and_then(|id| self.tables.monster_catalog.get(id))
            .map(|d| d.agl)
            .unwrap_or_else(|| {
                self.actors
                    .get(slot as usize)
                    .map(|a| a.battle.agl_base)
                    .unwrap_or(0)
            })
    }

    /// Retail's `0x801C8FE4`, the word PROT 0964's re-roll compares its draw
    /// against (`lw v1,4(a1)` with `a1 = 0x801C8FE0` at `0x801F8A20`, again
    /// at `0x801F8A4C`) and then overwrites with the accepted one (`sw a3,
    /// 4(v1)` at `0x801F8A90`). It is the AI's phase counter
    /// ([`crate::monster_ai::MonsterAiState::counter`]): Rogue's picker reads
    /// the same byte back as its next attack, `counter - 0x50` (`0xB0` Wind /
    /// `0xB1` Thunder / `0xB2` Flame), so the roll that recolours the record
    /// is also what makes the attacks cycle. Battle init zeroes it, so the
    /// first Element Change cannot roll `0`.
    fn cast_element_change_last_roll(&self) -> u8 {
        self.battle.monster_ai_state.counter() as u8
    }

    /// Commit PROT 0964's new element onto the first monster seat.
    ///
    /// Retail writes the record copy the battle loader made for that seat
    /// (`0x801C9348[0]`, record `+0x1D`) - per seat and per fight. The engine
    /// keeps that copy as the seat's [`crate::world::Actor::battle_element`],
    /// which `World::battle_slot_element` and the plaque badge read ahead of
    /// the catalog; the shared catalog entry is never touched, so the next
    /// battle with the same monster starts on its disc element.
    pub(in crate::world) fn apply_cast_element_change(&mut self, element: u8) {
        use vm::cast_module_ticks::FIRST_MONSTER_SEAT;
        if let Some(a) = self
            .actors
            .get_mut(FIRST_MONSTER_SEAT as usize)
            .filter(|a| a.battle_monster_id.is_some())
        {
            a.battle_element = Some(element);
        }
    }

    /// Write a kernel's state view back onto an actor slot.
    fn write_cast_actor_state(&mut self, slot: u8, st: &vm::cast_module_ticks::CastActorState) {
        use vm::battle_anim_rate::AnimRate;
        let Some(a) = self.actors.get_mut(slot as usize) else {
            return;
        };
        a.battle.hp_bar_pending = st.hp_bar_delta;
        a.battle.hp = st.hp;
        a.battle.field_flags = st.flags;
        a.battle.queued_anim = st.staged_anim;
        a.battle.active_target = st.target_code;
        a.battle.render_flag = st.render_flag;
        a.battle.render_color = st.present_04;
        a.battle.impact_state = st.render_21f;
        a.battle.capture_state = st.render_225;
        a.battle.anim_rate = AnimRate(st.anim_rate);
        a.battle.agl = st.agl;
        a.battle.agl_base = st.agl_base;
        a.battle.atk_working = st.atk;
        a.battle.atk_base = st.atk_base;
        a.battle.spd = st.spd;
        a.battle.spd_base = st.spd_base;
        a.battle.intel = st.intel;
        a.battle.intel_base = st.intel_base;
        a.battle.init_key = st.init_key;
        a.battle.spirit_gauge = st.spirit_gauge;
        a.battle.action_category = st.action_category;
        a.battle.damage_accum = st.combo_total;
        // ...and back into the mirrors the rest of the engine reads, so a
        // five-stat debuff is visible to turn order and the accuracy seed
        // rather than only to the next module tick.
        if let Some(s) = self.battle.speed.get_mut(slot as usize) {
            *s = st.spd;
        }
        if let Some(s) = self.battle.accuracy.get_mut(slot as usize) {
            *s = st.intel;
        }
        if let Some(s) = self.battle.defense_split.get_mut(slot as usize)
            && s.is_some()
        {
            *s = Some((st.udf, st.ldf));
        }
    }

    /// The `(x, z)` pair a battle seat occupies - its anchor when the battle
    /// loader seeded one, else its live position.
    fn cast_seat_xz(&self, slot: u8) -> (i16, i16) {
        self.actors
            .get(slot as usize)
            .map(|a| {
                a.battle
                    .seat
                    .unwrap_or((a.move_state.world_x, a.move_state.world_z))
            })
            .unwrap_or((0, 0))
    }

    /// PROT 0904's beam root and arm-12 ray tip for sweep word `ctx_6d8`
    /// (before the arm's ramp - the tip is built from the ramped word, as the
    /// tick builds it), from the summon seat's live position and facing
    /// (retail reads slot 7's `+0x34` / `+0x38` / `+0x46`).
    ///
    /// REF: FUN_801F69D8 (PROT 0904 arm 12, `0x801F7AF4..0x801F7C80`)
    pub(in crate::world) fn theeder_ray(
        &self,
        summon_slot: u8,
        ctx_6d8: u16,
    ) -> ([i16; 3], [i16; 3]) {
        use vm::cast_seru_ticks_a as ta;
        let g = self.theeder_geom(summon_slot);
        let mouth = ta::theeder_mouth(g.x, g.z, g.facing);
        let tip = ta::theeder_ray_tip(mouth, g.facing, ta::theeder_sweep_phase(ctx_6d8));
        (mouth, tip)
    }

    /// The summon seat's live `(x, z)` and facing, as PROT 0904 reads slot 7.
    fn theeder_geom(&self, summon_slot: u8) -> vm::cast_seru_ticks_a::TheederGeom {
        self.actors
            .get(summon_slot as usize)
            .map(|a| vm::cast_seru_ticks_a::TheederGeom {
                x: a.move_state.world_x,
                z: a.move_state.world_z,
                facing: a.battle.facing_angle & 0x0FFF,
            })
            .unwrap_or_default()
    }

    /// PROT 0904's packets for this frame - what the Theeder module's last
    /// tick drew (the arm-9 prongs, the arm-11 charge beam, the arm-11/12
    /// sweeping beam and its trail, the arm-13 retract) - while its cast is
    /// in the band. Both hosts project the points with their battle camera
    /// and build the primitives with `legaia_engine_ui::cast_theeder`.
    ///
    /// REF: FUN_801F815C, FUN_801F83A4, FUN_801F8634, FUN_801F8B84
    pub fn theeder_draw(&self) -> Option<vm::cast_seru_ticks_a::TheederPacket> {
        if self.mode != SceneMode::Battle || self.casting.summon_stager.is_none() {
            return None;
        }
        self.casting.module_theeder.packet
    }

    /// The trail ring [`Self::theeder_draw`]'s fan packets index
    /// (`hist[0]` newest).
    pub fn theeder_trail(&self) -> &[[i16; 3]] {
        &self.casting.module_theeder.trail.hist
    }

    /// Which of `seats` lie inside a `+-half_width` cone about `bearing`, as
    /// seen from `centre` - the geometry PROT 0904's swinging-ray sweep
    /// gates each hit on, and the one thing the module's arm 12 needs from its
    /// host.
    ///
    /// Retail's shape, read off `0x801F7CA0..0x801F7D14`: two calls to the
    /// 12-bit atan2 `FUN_80019B28` against the **same** second point (the
    /// beam root) - one from the ray's tip, one from
    /// the seat - each `+0x800` and masked to `0xFFF`, then
    /// `|ref - seat| - 0x30` compared **unsigned** against `0xFB1`. That
    /// comparison is the wrap: a difference below `0x30` underflows past
    /// `0xFB1` and a difference at or above `0xFE1` exceeds it, so both ends
    /// of the cone are in and everything between is out. The `+0x800` cancels
    /// in the difference and the bearings are measured toward the centre in
    /// both calls, so measuring outward from the centre gives the same
    /// difference.
    ///
    /// The atan2 is the ported `FUN_80019B28`
    /// ([`vm::battle_action::bearing_12bit_approx`]) over the
    /// approximated arctan LUT, the same one the enemy target cursor uses.
    ///
    /// REF: FUN_801F69D8 (PROT 0904 arm 12 cone gate), FUN_80019B28
    pub fn seats_in_cone(
        &self,
        centre: (i16, i16),
        bearing: u16,
        half_width: u16,
        seats: std::ops::Range<u8>,
    ) -> Vec<u8> {
        use vm::battle_action::bearing_12bit_approx;
        const FULL_TURN: u16 = 0x1000;
        seats
            .filter(|&seat| {
                let (sx, sz) = self.cast_seat_xz(seat);
                let to_seat = bearing_12bit_approx(centre.1, centre.0, sz, sx);
                let d = (to_seat.wrapping_sub(bearing)) & (FULL_TURN - 1);
                d <= half_width || d >= FULL_TURN - half_width
            })
            .collect()
    }

    /// PROT 0907 (Nighto)'s kill / confuse / resist verdict for the resident
    /// cast - drawn once and held on [`crate::world::CastFxState::module_nighto_outcome`].
    ///
    /// Retail's arm 0 draws both rolls off the SCUS RNG `FUN_80056798` and
    /// parks them in the module's own words, so the outcome is settled the
    /// frame the cast starts (`0x801F6B50` kill roll, `0x801F6C28` resist
    /// throw, `0x801F6CF0` the third-character extra throw). This is the same
    /// draw against this world's RNG cursor, and the arithmetic is the ported
    /// kernel's ([`vm::cast_seru_ticks_a::nighto_outcome`]).
    ///
    /// The three inputs the roll needs beyond the dice:
    ///
    /// * the caster's **magic level** for this spell, the record byte both
    ///   heal modules scan for ([`Self::caster_magic_power_byte`]);
    /// * the victim's immunity - retail's `ctx[+0x287] != 0 && record[+0x20]
    ///   != 0`, i.e. the scripted-fight flag AND the monster record's
    ///   double-width texture-page byte read as a "big model" proxy
    ///   ([`crate::monster_catalog::MonsterDef::wide_texture_page`]);
    /// * whether the caster is character index `3`, the only one that takes
    ///   the extra forced-resist throw. Retail reads `0x8007BD10[ctx+0x13]`,
    ///   the **1-based** present-party character id; the engine's mirror of
    ///   that list is [`crate::world::World::party_roster_slot`], so the id is
    ///   its roster slot plus one.
    ///
    /// The **cure selector** three cast modules read out of `0x801F6960`.
    ///
    /// That word is not a module constant and not a per-spell field: it is the
    /// Seru side-effect stager's own output latch. `FUN_801F3D3C` picks an
    /// 8-byte record out of the `[element][level band]` table at `0x801F6870`
    /// (`0x801F4420..0x801F4480`: `0x801F6870 + ((level - 3) >> 1) * 8 +
    /// element * 0x20`) and stores the record's **first byte** to `0x801F6960`.
    /// On the six damaging rows that byte is a percent (`5 / 10 / 15 / 20`);
    /// on the **light** row it is a cure class `1..=4`, and `1..=4` is exactly
    /// the switch PROT 0905 (`0x801F7D68`), 0911 (`0x801F7BE4`) and 0919
    /// (`0x801F8168`) compare against. So a non-light summon's cast leaves a
    /// percent in the latch, matches none of the four arms and cures nothing -
    /// the element gate is the latch's own value, not a second test.
    ///
    /// The port's copy of that latch is
    /// [`legaia_engine_vm::battle_action::BattleActionCtx::follow_up_pending`],
    /// written on every player Seru cast by
    /// [`Self::stage_seru_side_effect`]. `min_level` is the module's own
    /// `sltiu v0,v0,0x3` gate, below which its ladder is skipped entirely.
    ///
    /// REF: FUN_801F3D3C (the stager), FUN_801F69D8 (the three readers)
    fn cure_selector(&self, caster_slot: u8, spell_id: u8, min_level: u8) -> Option<u8> {
        if self.caster_magic_power_byte(caster_slot, spell_id) < min_level {
            return None;
        }
        Some(self.battle_ctx.follow_up_pending)
    }

    /// What PROT 0905's restore arm needs: the caster's per-spell magic level,
    /// the ally target's max HP and the cure selector above.
    ///
    /// `apply_hp` is **clear**. The arm's cure sweep and its phase machine are
    /// this body's, but its HP store is not: the engine folds a cast's HP
    /// outcome exactly once at [`Self::cast_spell_on_slots_prepaid`], and that
    /// fold already routes this module's own magnitude in through
    /// `seru_tick_heal_amount`. Since [`Self::summon_stager_tick`] re-enters
    /// the module every frame, leaving the store here would restore twice -
    /// once in the arm, once in the fold. This is the same neutral-magnitude
    /// posture the rest of the band takes.
    ///
    /// REF: FUN_801F69D8 (PROT 0905 arm 9, `0x801F7C28..0x801F7F4C`)
    fn vera_restore(
        &self,
        caster_slot: u8,
        victim_slot: u8,
        spell_id: u8,
    ) -> Option<vm::cast_seru_ticks_a::VeraRestore> {
        let magic_level = self.caster_magic_power_byte(caster_slot, spell_id);
        let max_hp = self.actors.get(victim_slot as usize)?.battle.max_hp;
        Some(vm::cast_seru_ticks_a::VeraRestore {
            magic_level,
            max_hp,
            cure_tier: self
                .cure_selector(
                    caster_slot,
                    spell_id,
                    vm::cast_seru_ticks_a::VERA_CURE_MIN_LEVEL,
                )
                .unwrap_or(0),
            apply_hp: false,
        })
    }

    /// REF: FUN_801F69E8 (`0x801F6B50..0x801F6D28`, PROT 0907 arm 0)
    fn nighto_verdict(
        &mut self,
        caster_slot: u8,
        victim_slot: u8,
        spell_id: u8,
    ) -> vm::cast_seru_ticks_a::NightoOutcome {
        if let Some(held) = self.casting.module_nighto_outcome {
            return held;
        }
        use vm::cast_seru_ticks_a as ticks_a;
        let magic_level = self.caster_magic_power_byte(caster_slot, spell_id);
        let target_immune = self.battle_ctx.scripted_fight != 0
            && self
                .actors
                .get(victim_slot as usize)
                .and_then(|a| a.battle_monster_id)
                .and_then(|id| self.tables.monster_catalog.get(id))
                .is_some_and(|def| def.wide_texture_page != 0);
        // Retail's character id is 1-based over the present-party list.
        let caster_character = self.party_roster_slot(caster_slot as usize) as u8 + 1;
        let kill_roll = self.next_rand();
        let resist_roll = self.next_rand();
        let extra_roll =
            (caster_character == ticks_a::NIGHTO_EXTRA_ROLL_CHARACTER).then(|| self.next_rand());
        let outcome = ticks_a::nighto_outcome(&ticks_a::NightoRoll {
            kill_roll,
            resist_roll,
            magic_level,
            target_immune,
            extra_roll,
        });
        self.casting.module_nighto_outcome = Some(outcome);
        outcome
    }

    /// The module's camera state with its `yaw_base` loaded from the battle
    /// camera's live `ctx[+0x6DA]`. Retail has one word: the module's
    /// `sh 0x200, 0x6DA(ctx)` and per-pass swing land in the counter the
    /// action SM's prologue keeps drifting, and the Done band's case 6 reads
    /// what they left (`shiny_refactor_gimard_levelup`).
    fn module_cam_with_yaw_base(&self) -> vm::cast_module_camera::ModuleCamState {
        let mut st = self.casting.module_cam;
        if let Some(cam) = self.battle.camera.as_ref() {
            st.yaw_base = cam.action_yaw_base();
        }
        st
    }

    /// Store the module's camera state back and its `yaw_base` into the
    /// battle camera's `ctx[+0x6DA]` ([`Self::module_cam_with_yaw_base`]).
    fn store_module_cam(&mut self, st: vm::cast_module_camera::ModuleCamState) {
        if let Some(cam) = self.battle.camera.as_mut() {
            cam.set_action_yaw_base(st.yaw_base);
        }
        self.casting.module_cam = st;
    }

    /// The caster and victim as the module camera arms read them: world
    /// position (`+0x34..+0x38`) and battle heading (`+0x46`).
    pub(in crate::world) fn module_cam_seats(
        &self,
        caster_slot: u8,
        victim_slot: u8,
    ) -> vm::cast_module_camera::ModuleCamSeats {
        let seat = |slot: u8| {
            self.actors
                .get(slot as usize)
                .map(|a| vm::cast_module_camera::ModuleSeat {
                    x: a.move_state.world_x,
                    y: a.move_state.world_y,
                    z: a.move_state.world_z,
                    facing: a.battle.facing_angle & 0xFFF,
                })
                .unwrap_or_default()
        };
        vm::cast_module_camera::ModuleCamSeats {
            caster: seat(caster_slot),
            victim: seat(victim_slot),
            band_timer: i32::from(self.battle_ctx.frame_timer),
            first_monster: self.battle_first_monster_byte(),
            caster_monster: self
                .actors
                .get(caster_slot as usize)
                .and_then(|a| a.battle_monster_id)
                .map_or(0, |id| id as u8),
            action: self
                .actors
                .get(caster_slot as usize)
                .map_or(0, |a| a.battle.params[0]),
            depth_raw: self.battle.camera_frame_height as i32,
            caster_latch: self.caster_latch(caster_slot).unwrap_or(0),
        }
    }

    /// The monster caster's battle-scoped latch word `0x801C8FE0 +
    /// (ctx[+0x13] + 1) * 4` - the monster AI's ability cooldown
    /// `dat[m + 4]`. `None` for a party caster.
    fn caster_latch_index(&self, caster_slot: u8) -> Option<usize> {
        let m = usize::from(caster_slot).checked_sub(self.party.party_count as usize)?;
        let i = m + 4;
        (i < self.battle.monster_ai_state.dat.len()).then_some(i)
    }

    fn caster_latch(&self, caster_slot: u8) -> Option<i32> {
        self.caster_latch_index(caster_slot)
            .map(|i| self.battle.monster_ai_state.dat[i])
    }

    /// The context bytes the kernels read (`ctx+0`, `+1`, `+0x13`, `+0x278`,
    /// `+0x279`).
    ///
    /// Both counts are bounded by [`BATTLE_TABLE_SLOTS`]: retail's `ctx[+0]`
    /// and `ctx[+1]` index `DAT_801C9370`, whose battle span is the eight
    /// combat seats - the summon seat and any host-side extras above them are
    /// not part of either sweep's range. The monster count starts at
    /// `cast_module_ticks::FIRST_MONSTER_SEAT`, the fixed base the Juggernaut
    /// sweep's `addiu s4, zero, 0xc` encodes.
    pub(in crate::world) fn cast_module_ctx(&self) -> vm::cast_module_ticks::CastModuleCtx {
        use vm::cast_module_ticks::FIRST_MONSTER_SEAT;
        let table = self.actors.len().min(BATTLE_TABLE_SLOTS);
        vm::cast_module_ticks::CastModuleCtx {
            // Retail's `ctx[+0]` is the **party** count, not the actor count:
            // `0x8004B3F0` loads it as the bound of a loop that turns
            // `DAT_8007BD10[i]` - the present-party char-id list - into a
            // `0x414`-byte record (`0x8004B420..0x8004B484`, id-1 scaled by
            // `0x414` onto `0x80084140`). So the seat range it names is the
            // party row `0..FIRST_MONSTER_SEAT`, and nothing above it.
            //
            // The engine's mirror of that list is `PartyState::party_count`
            // (the same ordinal space `World::party_roster_slot` resolves).
            // Seeding this from the whole actor table instead made every
            // `ctx[+0]` sweep - Evil Seru Magic's whole-row hit, the Orb heal,
            // the Element Change hide - run over the monster rows as well,
            // which retail's separate `ctx[+1]` sweep is what covers.
            party_count: self
                .party
                .party_count
                .min(FIRST_MONSTER_SEAT)
                .min(table as u8),
            monster_count: (FIRST_MONSTER_SEAT as usize..table)
                .filter(|&s| self.actors[s].active)
                .count() as u8,
            caster_seat: self.battle_ctx.active_actor,
            ctx_278: self.casting.module_ctx_278,
            phase: self.casting.module_phase,
            ctx_0d: 0,
            turn_cursor: self.battle_ctx.turn_cursor,
            ctx_27a: 0,
            ctx_6d8: self.casting.module_ring_angle,
        }
    }

    /// Run the resident module's **code** half for one frame - the band's
    /// **PORT** worklist rows, ported in
    /// [`legaia_engine_vm::cast_module_ticks`].
    ///
    /// Retail re-enters the paged module every frame through
    /// `FUN_801F1ED4` / `FUN_801F2160` and holds battle phase `0x70` while
    /// the tick reports busy; the engine calls this from
    /// [`Self::summon_stager_tick`], which the action SM already drives at
    /// states `0x34` / `0x35` / `0x36` (`crate::world::vm_hosts` ->
    /// `legaia_engine_vm::battle_action::summon`). So the chain from a host
    /// root is: `play-window` / the browser play page -> `World::tick` ->
    /// the action SM's summon band -> here.
    ///
    /// What runs is the module's **presentation and phase** half: the
    /// `ctx+0x279` walk, `ctx+0x278`, the staged-clip and animation-rate
    /// writes, the summon-seat pose and the `+0x1DD` retarget. The damage
    /// half is deliberately *not* re-applied here - the engine folds a cast's
    /// HP outcome once, at [`Self::cast_spell_on_slots_prepaid`], and the
    /// module's own numbers reach that fold as the baked per-hit power
    /// ([`vm::cast_module_ticks::baked_power_for`], read by
    /// `capture_bypass_predamage` / `capture_respect_predamage`) rather than
    /// as a second application.
    ///
    /// Returns `None` when no band entry is resident (a disc-free host, or a
    /// spell that names no module).
    /// Whether one seat has settled the way every settle loop in the band
    /// tests it ([`vm::cast_module_ticks::ChainSettle`]): a live seat once its playing clip
    /// is back to idle, a dead one once it plays the down clip - or, where
    /// `faded` counts, once its defeat fade has run its colour word out
    /// (retail's prim word `+0x04` at `0`).
    ///
    /// The engine's playing clip is the reaction channel's entry while a
    /// reaction plays (retail commits a reaction into `+0x1D9` like any
    /// other clip), else the committed `current_anim`. A downed party seat
    /// holds the engine's defeat pose rather than clip `8`, so a finished
    /// defeat pose reads as settled too.
    fn chain_seat_settled(&self, slot: usize, faded: bool) -> bool {
        let Some(a) = self.actors.get(slot) else {
            return true;
        };
        let playing = match a.battle_reaction {
            Some(tag) => a.battle_reaction_entry.unwrap_or(tag.max(1)),
            None => a.battle.current_anim,
        };
        if a.battle.hp != 0 {
            return playing == 0;
        }
        let downed = playing == vm::cast_module_ticks::SETTLE_DOWN_CLIP
            || (a.battle_pose == Some(vm::battle_action::Pose::Defeat as u8)
                && a.battle_animation.as_ref().is_none_or(|p| p.finished()));
        downed || (faded && (a.battle.render_color == 0 || !a.active))
    }

    /// [`Self::chain_seat_settled`] over the seats a [`vm::cast_module_ticks::ChainSettle`]
    /// walks.
    fn chain_settled(
        &self,
        settle: vm::cast_module_ticks::ChainSettle,
        caster: u8,
        victim: u8,
        ctx: &vm::cast_module_ticks::CastModuleCtx,
    ) -> bool {
        use vm::cast_module_ticks::ChainSettle as S;
        let party = 0..usize::from(ctx.party_count);
        let pc = usize::from(self.party.party_count);
        let monsters = pc..pc + usize::from(ctx.monster_count);
        match settle {
            S::Victim => self.chain_seat_settled(usize::from(victim), false),
            S::VictimOrFaded => self.chain_seat_settled(usize::from(victim), true),
            S::PartyRow => party.into_iter().all(|s| self.chain_seat_settled(s, false)),
            S::TargetRow => {
                let t = self
                    .actors
                    .get(usize::from(caster))
                    .map_or(0, |a| a.battle.active_target);
                if t == legaia_engine_vm::battle_cue_group::TARGET_PARTY_WIDE || usize::from(t) < pc
                {
                    party.into_iter().all(|s| self.chain_seat_settled(s, false))
                } else {
                    monsters.into_iter().all(|s| {
                        let a = self.actors.get(s);
                        match a {
                            Some(a) if a.battle.hp == 0 => a.battle.render_color == 0 || !a.active,
                            _ => self.chain_seat_settled(s, false),
                        }
                    })
                }
            }
        }
    }

    pub fn run_cast_module_code(&mut self, spell_id: u8, arm: u8) -> Option<CastModuleCodeRun> {
        // --- W1-D ---
        use vm::cast_arm_ticks as arms;
        // --- end W1-D ---
        use vm::cast_module_ticks as ticks;
        // --- W1-B ---
        use vm::cast_seru_ticks_a as ticks_a;
        // --- end W1-B ---

        let entry = self.cast_module_for(spell_id)?;
        // PROT 0954's body is ported whole - camera, records, wheel and
        // outcomes - and drives the band on its own
        // (`world::battle::fatal_decision`).
        if entry == vm::cast_fatal_decision::FATAL_DECISION_ENTRY {
            return Some(self.run_fatal_decision(entry));
        }
        let mut ctx = self.cast_module_ctx();
        let caster_slot = self.battle_ctx.active_actor;
        let victim_slot = self
            .actors
            .get(caster_slot as usize)
            .map(|a| a.battle.active_target)
            .unwrap_or(0);
        let seat_slot = self.casting.summon_actor_slot.unwrap_or(ticks::SUMMON_SEAT);

        let mut caster = self.cast_actor_state(caster_slot);
        let mut victim = self.cast_actor_state(victim_slot);
        let mut seat = self.cast_actor_state(seat_slot);
        let (mut caster_orig, mut victim_orig, mut seat_orig) = (caster, victim, seat);
        let mut run = CastModuleCodeRun {
            prot_entry: entry,
            busy: true,
            ..Default::default()
        };

        // The seven state-touching spawn stagers, by owning entry.
        match entry {
            906 => ticks::gizam_stager(&mut ctx, &mut seat, arm),
            909 => {
                ticks::viguro_stager(&mut ctx, &mut seat, arm);
            }
            922 => ticks::puera_stager(&mut ctx, arm),
            923 => ticks::gilium_stager(&mut ctx, arm),
            949 => ticks::water_crystals_stager(&mut victim, arm),
            _ => {}
        }

        // The tick bodies, by owning entry. `hit` is `None` because the fold
        // is the band seam's, not the tick's (see the note above).
        //
        // A capture-class module reaches its body through a **trampoline**
        // (`ticks::capture_tick_body`), which switches on the caster's queued
        // action id, so a multi-spell cell picks a different choreography for
        // each of its ids. Where the module has one, the trampoline decides
        // whether anything ticks at all: an id it does not name returns zero
        // and the drive loop proceeds.
        //
        // The arm has to be keyed on `(entry, body)`, never on the body VA
        // alone: six modules in the band put a tick body at `0x801F69D8` -
        // the load base itself - so PROT 0960's Neo Star Slash and PROT
        // 0965's Doomsday wear the same address in different images.
        let body = ticks::capture_tick_body(entry, spell_id);
        let has_trampoline = ticks::capture_trampoline_for(entry).is_some();
        // A player-Seru module whose camera arms are ported paces its own
        // arms: the director runs first and, while an arm's countdown holds,
        // the phase-chain body does not run at all (retail's arm returns
        // before any of its writes). Its shot is the camera the summon band's
        // `0x35` / `0x36` hand the battle camera.
        let profile = if has_trampoline {
            None
        } else {
            vm::cast_module_camera::module_profile(entry)
        };
        let direction = if has_trampoline {
            None
        } else {
            vm::cast_module_camera::module_director(entry).map(|direct| {
                let latched = *self
                    .casting
                    .module_cam
                    .victim_slot
                    .get_or_insert(victim_slot);
                let seats = self.module_cam_seats(caster_slot, latched);
                let mut st = self.module_cam_with_yaw_base();
                let d = direct(&mut st, ctx.phase, seats);
                self.store_module_cam(st);
                d
            })
        };
        run.camera_shot = direction.and_then(|d| d.shot);
        let mut capture_held = false;
        let mut capture_arm = None;
        let phase_in = ctx.phase;
        // A capture-class body's camera arms, on the phase it is about to
        // run (they make no gate of their own: the body's port does).
        if let Some(direct) = vm::cast_module_camera::capture_camera_director(
            entry,
            body.unwrap_or(vm::cast_module_camera::SINGLE_BODY),
        ) {
            let seats = self.module_cam_seats(caster_slot, victim_slot);
            let mut st = self.module_cam_with_yaw_base();
            let arm = direct(&mut st, ctx.phase, seats);
            self.store_module_cam(st);
            if let Some(v) = arm.latch
                && let Some(i) = self.caster_latch_index(caster_slot)
            {
                self.battle.monster_ai_state.dat[i] = v;
            }
            if arm.skips_fold {
                self.casting.module_skips_fold = true;
            }
            run.camera_shot = arm.shot;
            run.capture_drift = arm.drift;
            capture_held = arm.hold;
            capture_arm = Some(arm);
        }
        // PROT 0966 (Evil Seru Magic) has no other port: its seat half rides
        // the director's gate, on the phase the director is answering for.
        if entry == 966
            && let Some(arm) = capture_arm
        {
            let (phase, passed) = (ctx.phase, !arm.hold);
            if passed && let Some(t) = evil_seru_magic_fade(phase) {
                crate::fade::spawn_fade(&mut self.presentation.fade, &t, 1);
            }
            // The module's own move-VM stager hit lands mid-cast, not at the
            // fold: arm 10 spawns record `0x801F937C`, whose script waits
            // `0x7F` (`<< 3`, drained `scalar * delta` - 127 vsyncs) and then
            // runs op `0x20` with arm 4, the `0x100` never-kill sweep. That
            // is arm 11's gate (`0x80` vsyncs), so it lands before arm 26's
            // kill-capable hit, and the band's fold owes nothing more.
            let mut stager_hits = Vec::new();
            if passed && phase == vm::cast_module_camera::EVIL_SERU_MAGIC_STAGER_HIT_ARM {
                if let Some(r) =
                    self.run_cast_module_aoe_as(caster_slot, spell_id, AOE_STAGER_WORKING_ARM)
                {
                    stager_hits = r.aoe_hits;
                }
                self.casting.module_skips_fold = true;
            }
            let mut seats: Vec<ticks::CastActorState> = (0..self.actors.len() as u8)
                .map(|s| self.cast_actor_state(s))
                .collect();
            let mut rolls: Vec<(u8, i32)> = Vec::new();
            if passed && phase == vm::cast_module_camera::EVIL_SERU_MAGIC_SWEEP_ARM {
                // Rolled only for the seats the hit visits, in seat order,
                // so the shared RNG cursor moves as retail's does.
                for s in 0..ctx.party_count {
                    if seats
                        .get(s as usize)
                        .is_some_and(ticks::aoe_seat_is_hittable)
                    {
                        let r = self
                            .capture_module_roll(
                                &ticks::EVIL_SERU_MAGIC_SWEEP_SHAPE,
                                caster_slot,
                                s,
                            )
                            .unwrap_or(0);
                        rolls.push((s, r));
                    }
                }
                self.refresh_seat_spirit(&mut seats);
            }
            let take = |s: u8| {
                rolls
                    .iter()
                    .find(|(seat, _)| *seat == s)
                    .map_or(0, |(_, r)| *r)
            };
            let hits = ticks::evil_seru_magic_seat_writes(
                phase,
                passed,
                ctx.party_count,
                caster_slot,
                &mut seats,
                take,
            );
            for (slot, st) in seats.iter().enumerate() {
                self.write_cast_actor_state(slot as u8, st);
            }
            run.aoe_hits = stager_hits;
            run.aoe_hits.extend(hits.iter().map(|h| ticks::AoeHit {
                seat: h.seat,
                applied: h.applied as i32,
            }));
        }
        // A capture body with no camera director still gates its arms on
        // the module countdown: the arm's phase chain runs only on the tick
        // the gate lets through (`cast_module_camera::capture_countdown`).
        let arm_countdown = if capture_arm.is_none() {
            vm::cast_module_camera::capture_arm_countdowns(
                entry,
                body.unwrap_or(vm::cast_module_camera::SINGLE_BODY),
            )
            .and_then(|t| vm::cast_module_camera::arm_countdown(t, phase_in))
        } else {
            None
        };
        if let Some(a) = arm_countdown
            && a.holds(&mut self.casting.module_cam.countdown)
        {
            capture_held = true;
        }
        // A phase-chain body's hit lands on the tick its arm first runs -
        // retail calls the damage wrapper inside the arm - and an arm that
        // then waits on the hit seats' clips holds until they settle. The
        // runner itself holds no clip state, so both happen here.
        let chain = if has_trampoline {
            body.and_then(|b| ticks::chain_body_for(entry, b))
        } else {
            ticks::direct_chain_body(entry)
        };
        if let Some(arm) = chain.and_then(|c| c.arm(phase_in))
            && !capture_held
        {
            if arm.wrapper_site.is_some() && self.casting.module_hit_arm != Some(phase_in) {
                self.casting.module_hit_arm = Some(phase_in);
                self.fold_pending_cast();
                // The arm runs on the seats the hit left (a dead victim
                // takes PROT 0956's reaction branch, not its turn-steal).
                caster = self.cast_actor_state(caster_slot);
                victim = self.cast_actor_state(victim_slot);
                seat = self.cast_actor_state(seat_slot);
                (caster_orig, victim_orig, seat_orig) = (caster, victim, seat);
            }
            if let Some(settle) = arm.settle {
                let settled = self.chain_settled(settle, caster_slot, victim_slot, &ctx);
                self.casting.module_settle_ticks =
                    self.casting.module_settle_ticks.saturating_add(1);
                if settled
                    || self.casting.module_settle_ticks > vm::cast_fatal_decision::SETTLE_TICK_LIMIT
                {
                    self.casting.module_settle_ticks = 0;
                } else {
                    capture_held = true;
                }
            }
        }
        run.camera_follow = direction.and_then(|d| d.follow);
        run.camera_nudge = direction.and_then(|d| d.nudge);
        run.spawns = direction.map_or(&[], |d| d.spawns);
        run.vram_move = direction.and_then(|d| d.vram_move);
        run.caption = direction.and_then(|d| d.caption);
        run.fades = direction.map_or(&[], |d| d.fades);
        run.kills_fades = direction.is_some_and(|d| d.kills_fades);
        let held = direction.is_some_and(|d| d.hold) || capture_held;
        // A camera-only director owns the phase of a module whose tick body
        // is unported: its pass advances it, and it claims no tick.
        let camera_only = profile.is_some_and(|p| !p.paces_band() && p.owns_phase);
        if camera_only && !held {
            ctx.phase = ctx.phase.wrapping_add(1);
        }
        let step = if held && !camera_only {
            Some(ticks::CastTickStep::Busy)
        } else if held || camera_only {
            None
        } else if has_trampoline {
            match (entry, body) {
                (958, Some(ticks::BLAZING_SLASH_TICK)) => {
                    Some(ticks::blazing_slash_tick(&mut ctx, &mut victim, None))
                }
                (952, Some(ticks::ASTRAL_SLASH_TICK)) => {
                    Some(ticks::astral_slash_tick(&mut ctx, &mut caster, &mut victim))
                }
                (945, Some(ticks::WATER_COLUMN_TICK)) => {
                    Some(ticks::water_column_tick(&mut ctx, &mut victim, None, 0))
                }
                // PROT 0945's other choreography: the band's widest stat
                // write, on the caster.
                (945, Some(ticks::ALL_STATS_SURGE_TICK)) => {
                    let agl_record = self.cast_record_agl(caster_slot);
                    Some(ticks::all_stats_surge_tick(
                        &mut ctx,
                        &mut caster,
                        agl_record,
                    ))
                }
                (960, Some(ticks::PLASMA_STRIKE_TICK)) => {
                    // The burst arm's `0x1C0` roll. Retail aims it at
                    // `0x801C9370[0]` - seat 0, not the derived victim - so
                    // it rides the victim view only when the two coincide;
                    // the arm's other writes (the close, the knockdown) are
                    // the victim's either way.
                    let hit = if ctx.phase == ticks::PLASMA_STRIKE_BURST_ARM && victim_slot == 0 {
                        ticks::damage_shape_for(960)
                            .and_then(|sh| self.capture_module_roll(sh, caster_slot, 0))
                    } else {
                        None
                    };
                    // The burst is the module's one wrapper call (`jal
                    // 0x801DD6B4` at `0x801F8168`), and its HP writes are the
                    // cast's whole outcome beside arm `0x0C`'s flurry
                    // landing: once it has landed here, the band's generic
                    // fold owes nothing. Folding as well rolled the
                    // `0x1C0` a second time through the catalog def.
                    if hit.is_some() {
                        self.casting.module_skips_fold = true;
                    }
                    Some(ticks::plasma_strike_tick(
                        &mut ctx,
                        &mut caster,
                        &mut victim,
                        hit,
                    ))
                }
                (957, Some(ticks::SUMMON_EFFECT_TICK_B)) => {
                    Some(ticks::summon_effect_tick_b(&mut ctx, &mut victim))
                }
                (957, Some(ticks::SUMMON_EFFECT_TICK_A)) => {
                    Some(ticks::summon_effect_tick_a(&mut ctx, &mut victim, None))
                }
                // PROT 0942's Power Up reads the caster's own monster record
                // `+0x0E` and writes the AGL **base** half only.
                (942, Some(ticks::POWER_UP_TICK)) => {
                    let agl_record = self.cast_record_agl(caster_slot);
                    Some(ticks::power_up_tick(&mut ctx, &mut caster, agl_record))
                }
                // The three whole-row sweeps. Each rolls the module's own
                // baked power per hittable seat off this world's RNG cursor,
                // in seat order, so the draw order stays retail's; the two
                // 1-in-8 status draws Chaos Breath makes per seat come off
                // the same cursor.
                (938, Some(ticks::CHAOS_BREATH_TICK))
                | (938, Some(ticks::MYSTIC_CIRCLE_TICK))
                | (965, Some(ticks::DOOMSDAY_TICK)) => {
                    let body = body.unwrap_or_default();
                    let mut seats: Vec<ticks::CastActorState> = (0..self.actors.len() as u8)
                        .map(|s| self.cast_actor_state(s))
                        .collect();
                    let sweep_arm = ctx.phase;
                    let rolls = self.sweep_status_rolls(&ctx, &seats, body, caster_slot);
                    self.refresh_seat_spirit(&mut seats);
                    // The module's own baked power, rolled per seat: these
                    // three write `+0x14C` themselves, so they own the
                    // outcome and `fold_pending_cast` skips the generic fold
                    // for them (see `sweep_status_rolls`).
                    let take = |seat: u8| {
                        rolls
                            .iter()
                            .find(|(s, _, _)| *s == seat)
                            .map(|(_, d, _)| *d)
                            .unwrap_or(0)
                    };
                    let status = |seat: u8| {
                        rolls
                            .iter()
                            .find(|(s, _, _)| *s == seat)
                            .map(|(_, _, st)| *st)
                            .unwrap_or((1, 1))
                    };
                    let (step, hits) = match body {
                        ticks::CHAOS_BREATH_TICK => ticks::chaos_breath_tick(
                            &mut ctx,
                            &mut caster,
                            &mut seats,
                            take,
                            status,
                        ),
                        ticks::MYSTIC_CIRCLE_TICK => ticks::mystic_circle_tick(
                            &mut ctx,
                            &mut seats,
                            sweep_arm == ticks::MYSTIC_CIRCLE_SWEEP_ARM,
                            take,
                        ),
                        _ => ticks::doomsday_tick(
                            &mut ctx,
                            &mut seats,
                            sweep_arm == ticks::DOOMSDAY_SWEEP_ARM,
                            take,
                        ),
                    };
                    for (slot, st) in seats.iter().enumerate() {
                        self.write_cast_actor_state(slot as u8, st);
                    }
                    run.aoe_hits = hits
                        .iter()
                        .map(|h| ticks::AoeHit {
                            seat: h.seat,
                            applied: h.applied as i32,
                        })
                        .collect();
                    Some(step)
                }
                (951, Some(ticks::CHAOS_FLARE_TICK)) => {
                    Some(ticks::chaos_flare_tick(&mut ctx, &mut victim, None))
                }
                (951, Some(ticks::SCYTHE_WIND_TICK)) => {
                    Some(ticks::scythe_wind_tick(&mut ctx, &mut victim, None))
                }
                (952, Some(ticks::BLOODY_HORNS_TICK)) => Some(ticks::bloody_horns_tick(
                    &mut ctx,
                    &mut caster,
                    &mut victim,
                    None,
                )),
                // PROT 0955's six-spell cell. Its four status / buff bodies
                // write simulation state no damage fold can express, so they
                // run here and their outcome is reported back on the run.
                (955, Some(ticks::WHITE_SHIELD_TICK)) => {
                    // Retail reads the caster's own monster **record** at
                    // `0x801C9348[seat - 3]` rather than the live actor, so
                    // the buff is idempotent; the engine's nearest equivalent
                    // is the un-buffed defence split it seeded at battle load.
                    let record = (caster.udf_base, caster.ldf_base);
                    Some(ticks::white_shield_tick(&mut ctx, &mut caster, record))
                }
                (955, Some(ticks::KISS_OF_DEATH_TICK)) => {
                    let roll = Some(self.next_rand());
                    let (step, refund) = ticks::kiss_of_death_tick(&mut ctx, &mut victim, roll);
                    run.item_refund = refund;
                    Some(step)
                }
                (955, Some(ticks::MELT_SPRAY_TICK)) => {
                    let debuff = ctx.phase == ticks::MELT_SPRAY_DEBUFF_ARM;
                    Some(ticks::melt_spray_tick(&mut ctx, &mut victim, debuff))
                }
                (955, Some(ticks::TERROR_SCREAM_TICK)) => {
                    let (step, refund) = ticks::terror_scream_tick(&mut ctx, &mut victim);
                    run.item_refund = refund;
                    Some(step)
                }
                (955, Some(ticks::POWER_CHARGE_TICK)) => {
                    Some(ticks::power_charge_tick(&mut ctx, &mut caster))
                }
                (955, Some(ticks::VOID_ACCESSORIES_TICK)) => {
                    let rolls = Some((self.next_rand(), self.next_rand()));
                    let accessories = self.cast_victim_accessories(victim_slot);
                    let (step, outcome) =
                        ticks::void_accessories_tick(&mut ctx, &mut victim, accessories, rolls);
                    run.voided_accessory = outcome;
                    Some(step)
                }
                // PROT 0964's Element Change re-rolls the first monster
                // seat's element. `last_roll` is derived from what the record
                // holds now, which is what the previous commit wrote.
                (964, Some(ticks::ELEMENT_CHANGE_TICK)) => {
                    let mut seats: Vec<ticks::CastActorState> = (0..self.actors.len() as u8)
                        .map(|s| self.cast_actor_state(s))
                        .collect();
                    let last_roll = self.cast_element_change_last_roll();
                    let mut rolls: Vec<u32> = Vec::new();
                    if ctx.phase == 0 {
                        for _ in 0..=ticks::ELEMENT_CHANGE_MAX_REROLLS {
                            rolls.push(self.next_rand());
                        }
                    }
                    let mut cursor = rolls.into_iter();
                    let (step, outcome) =
                        ticks::element_change_tick(&mut ctx, &mut seats, last_roll, || {
                            cursor.next().unwrap_or(0)
                        });
                    for (slot, st) in seats.iter().enumerate() {
                        self.write_cast_actor_state(slot as u8, st);
                    }
                    if let Some(out) = outcome {
                        self.battle
                            .monster_ai_state
                            .set_counter(i32::from(out.roll));
                        self.apply_cast_element_change(out.element);
                        run.element_change = Some((out.element, out.group));
                    }
                    Some(step)
                }
                // --- W1-D: fourteen arms ---
                // PROT 0940 / 0941 / 0943 / 0944 / 0950 / 0956 / 0962, ported
                // in `legaia_engine_vm::cast_arm_ticks`. Three of these bodies
                // wear the VA `0x801F6A04` in three different images, which is
                // why every arm below names its entry.
                (940, Some(arms::GLARE_DIVIDE_BLIND_TICK)) => {
                    use vm::cast_module_ticks::FIRST_MONSTER_SEAT;
                    let mut seat = self.cast_actor_state(FIRST_MONSTER_SEAT);
                    let mut ext = self.cast_arm_ext_state(FIRST_MONSTER_SEAT);
                    let shield_arm = ctx.phase == arms::MYSTIC_SHIELD_ARM;
                    let step = arms::glare_divide_blind_tick(&mut ctx, &mut seat, &mut ext);
                    self.write_cast_actor_state(FIRST_MONSTER_SEAT, &seat);
                    self.write_cast_arm_ext_state(FIRST_MONSTER_SEAT, &ext);
                    if shield_arm {
                        // `_DAT_8007BD84 = FUN_80021B04(..)` - the shield's
                        // effect handle; the engine carries it as a flag.
                        self.battle.monster_ai_state.flag_bd84 = 1;
                    }
                    Some(step)
                }
                (940, Some(arms::GLARE_DIVIDE_SPLIT_TICK)) => {
                    let ext = self.cast_arm_ext_state(caster_slot);
                    let saved = self.casting.module_split_saved_target.unwrap_or(0);
                    let roll = (ctx.phase == 2).then(|| self.next_rand());
                    let (step, split) = arms::glare_divide_split_tick(
                        &mut ctx,
                        &mut caster,
                        &ext,
                        spell_id,
                        saved,
                        roll,
                    );
                    if let Some(sp) = split {
                        self.casting.module_split_saved_target = Some(sp.saved_caster_target);
                        self.apply_glare_divide_split(caster_slot, &sp);
                    }
                    Some(step)
                }
                (941, Some(arms::STEAL_TICK)) => {
                    let outcome = (ctx.phase == 1)
                        .then(|| self.roll_cast_steal(victim_slot))
                        .flatten();
                    let (step, taken) =
                        arms::steal_tick(&mut ctx, &mut caster, CAST_STEAL_RUN_CLIP, outcome);
                    if let Some(arms::StealOutcome::FromBag { item }) = taken {
                        let _removed = self.take_one_from_bag(item);
                    }
                    if let Some(outcome) = taken {
                        self.stash_cast_steal(caster_slot, outcome);
                    }
                    Some(step)
                }
                (941, Some(arms::STEAL_SWEEP_TICK)) => {
                    let (step, hits) = self.run_cast_arm_sweep(
                        &mut ctx,
                        941,
                        arms::STEAL_SWEEP_TICK,
                        caster_slot,
                        &mut caster,
                    );
                    run.aoe_hits = hits;
                    Some(step)
                }
                (943, Some(arms::CURSE_SINGLE_TICK)) => {
                    Some(arms::curse_single_tick(&mut ctx, &mut caster, &mut victim))
                }
                (943, Some(arms::CURSE_MP_DRAIN_TICK)) => {
                    let mut seats: Vec<ticks::CastActorState> = (0..self.actors.len() as u8)
                        .map(|s| self.cast_actor_state(s))
                        .collect();
                    let mut exts: Vec<arms::CastArmExtState> = (0..self.actors.len() as u8)
                        .map(|s| self.cast_arm_ext_state(s))
                        .collect();
                    let (step, _drained) =
                        arms::curse_mp_drain_tick(&mut ctx, &mut seats, &mut exts);
                    for (slot, st) in seats.iter().enumerate() {
                        self.write_cast_actor_state(slot as u8, st);
                    }
                    for (slot, st) in exts.iter().enumerate() {
                        self.write_cast_arm_ext_state(slot as u8, st);
                    }
                    Some(step)
                }
                (944, Some(arms::GUILTY_CROSS_TICK)) => Some(arms::guilty_cross_tick(
                    &mut ctx,
                    &mut caster,
                    &mut victim,
                    None,
                )),
                (944, Some(arms::GUILTY_CROSS_CURSE_TICK)) => {
                    let code = caster.target_code;
                    let mut seats: Vec<ticks::CastActorState> = (0..self.actors.len() as u8)
                        .map(|s| self.cast_actor_state(s))
                        .collect();
                    let (step, _marked) =
                        arms::guilty_cross_curse_tick(&mut ctx, code, &mut caster, &mut seats);
                    for (slot, st) in seats.iter().enumerate() {
                        self.write_cast_actor_state(slot as u8, st);
                    }
                    Some(step)
                }
                (950, Some(arms::ROLLING_FLARE_TICK)) => Some(arms::rolling_flare_tick(
                    &mut ctx,
                    &mut caster,
                    &mut victim,
                    None,
                )),
                (950, Some(arms::ROLLING_FLARE_SWEEP_TICK)) => {
                    let sweep = ctx.phase
                        == arms::arm_sweep_arm(950, arms::ROLLING_FLARE_SWEEP_TICK).unwrap_or(0xFF);
                    let rolls = self.cast_arm_sweep_rolls(
                        &ctx,
                        950,
                        arms::ROLLING_FLARE_SWEEP_TICK,
                        caster_slot,
                    );
                    let mut seats: Vec<ticks::CastActorState> = (0..self.actors.len() as u8)
                        .map(|s| self.cast_actor_state(s))
                        .collect();
                    let take = |seat: u8| {
                        rolls
                            .iter()
                            .find(|(s, _)| *s == seat)
                            .map(|(_, d)| *d)
                            .unwrap_or(0)
                    };
                    let (step, hits) = arms::rolling_flare_sweep_tick(
                        &mut ctx,
                        &mut caster,
                        &mut seats,
                        sweep,
                        take,
                    );
                    for (slot, st) in seats.iter().enumerate() {
                        self.write_cast_actor_state(slot as u8, st);
                    }
                    run.aoe_hits = hits
                        .iter()
                        .map(|h| ticks::AoeHit {
                            seat: h.seat,
                            applied: h.applied as i32,
                        })
                        .collect();
                    Some(step)
                }
                (956, Some(arms::WATER_HAZARD_TICK)) => {
                    let code = caster.target_code;
                    let rolls =
                        self.cast_arm_sweep_rolls(&ctx, 956, arms::WATER_HAZARD_TICK, caster_slot);
                    let status: Vec<(u8, u32)> = if ctx.phase == 2 {
                        rolls.iter().map(|(s, _)| (*s, self.next_rand())).collect()
                    } else {
                        Vec::new()
                    };
                    let mut seats: Vec<ticks::CastActorState> = (0..self.actors.len() as u8)
                        .map(|s| self.cast_actor_state(s))
                        .collect();
                    let take = |seat: u8| {
                        rolls
                            .iter()
                            .find(|(s, _)| *s == seat)
                            .map(|(_, d)| *d)
                            .unwrap_or(0)
                    };
                    let st = |seat: u8| {
                        status
                            .iter()
                            .find(|(s, _)| *s == seat)
                            .map(|(_, r)| *r)
                            .unwrap_or(1)
                    };
                    let (step, hits) = arms::water_hazard_tick(
                        &mut ctx,
                        caster_slot,
                        code,
                        &mut caster,
                        &mut seats,
                        take,
                        st,
                    );
                    for (slot, s) in seats.iter().enumerate() {
                        self.write_cast_actor_state(slot as u8, s);
                    }
                    run.aoe_hits = hits
                        .iter()
                        .map(|h| ticks::AoeHit {
                            seat: h.seat,
                            applied: h.applied as i32,
                        })
                        .collect();
                    Some(step)
                }
                // `arrived = true` on all three: the band's approach leg
                // (`CasterStagePhase::Approach`, gated on
                // `capture_body_approaches`) walks the caster in and holds
                // the module until the metric reads zero, so by the time
                // this body runs its poll would read arrival.
                //
                // Retail's predicate is `FUN_8004E2F0(ctx[+0x13],
                // caster[+0x1DD]) == 0` - the **zero** side, not the non-zero
                // one: `sll v0,v0,0x10; bne v0,zero,<hold>` at
                // `0x801F7C0C` / `0x801F7D08` sends a non-zero metric to the
                // "stage the run clip and hold" arm, so the metric reads
                // "still out of reach" and zero is arrival. The engine has
                // that metric (`Self::battle_range_metric`, the port of the
                // same routine) and the 0x20 `FUN_80050BB8` calls the arrived
                // path makes are ported too
                // (`legaia_engine_vm::battle_separation`, a pairwise
                // separation nudge - NOT the approach itself).
                //
                // The walk itself is the band's approach leg: the body's arm
                // `0` stages the walk entry and turns the caster onto the
                // victim, and the walk clip's own root motion carries it.
                (962, Some(arms::BLADE_BREATH_A_TICK)) => Some(arms::blade_breath_a_tick(
                    &mut ctx,
                    &mut caster,
                    &mut victim,
                    true,
                    None,
                )),
                (962, Some(arms::BLADE_BREATH_B_TICK)) => Some(arms::blade_breath_b_tick(
                    &mut ctx,
                    &mut caster,
                    &mut victim,
                    true,
                    None,
                )),
                (962, Some(arms::BLADE_BREATH_C_TICK)) => Some(arms::blade_breath_c_tick(
                    &mut ctx,
                    &mut caster,
                    &mut victim,
                    true,
                    None,
                )),
                // --- end W1-D ---
                // The phase-chain bodies (`cast_module_ticks::chain_bodies`):
                // PROT 0942 `0xAA`, 0956 `0x75`, 0959, 0960 `0xA6`, 0961,
                // 0963 and 0964 `0xB0..=0xB2`. Their hits are the fold's.
                (e, Some(b)) if ticks::chain_body_for(e, b).is_some() => {
                    let chain = ticks::chain_body_for(e, b).expect("guarded");
                    let selector = if e == 964 {
                        self.cast_element_change_last_roll()
                    } else {
                        0
                    };
                    let t = ticks::run_chain_body(
                        chain,
                        &mut ctx,
                        &mut caster,
                        &mut victim,
                        &mut seat,
                        selector,
                    );
                    run.item_refund = t.refund;
                    Some(t.step)
                }
                // Every ported trampoline arm the band names. An arm whose
                // body has no port ticks nothing, which is exactly what
                // retail's fall-through does for an id the trampoline does
                // not name.
                _ => None,
            }
        } else {
            match entry {
                // --- W1-B: player Seru 0903..0908 ---
                // The first six `0x801CF4EC` arms - the player Seru-magic tick
                // bodies (`legaia_engine_vm::cast_seru_ticks_a`). Unlike the
                // summon-creature ticks below, five of the six sweep or
                // retarget seats other than the caster's own three, so they
                // take the whole seat row and the run writes it back before
                // the caster / victim / summon views are refreshed from it.
                //
                // No damage roll is fed in: the engine folds a cast's HP
                // outcome once at `cast_spell_on_slots_prepaid`, so the bodies
                // below take `None`.
                //
                // PROT 0907's kill / confuse fork is the exception, because it
                // is not a damage roll at all - it writes the victim's HP to
                // zero or `+0x16E |= 0x380` and there is no magnitude for the
                // fold to carry. Its verdict is drawn here, once per cast
                // (`Self::nighto_verdict`), and held on the cast state for
                // every later frame, mirroring retail's arm-0 draw into the
                // module words `0x801F8534` / `0x801F853C`.
                903..=908 => {
                    // PROT 0904's ring sweep: advance the ray, then resolve
                    // the cone once for this tick (both borrow `self`, which
                    // the seat row below does not allow).
                    let cone_seats: Vec<u8> =
                        if entry == 904 && ctx.phase == ticks_a::THEEDER_SWEEP_ARM {
                            use vm::cast_seru_ticks_a::{MONSTER_ROW_END, THEEDER_CONE_HALF_WIDTH};
                            // The ray the sweep arm tests this tick: from the
                            // beam root ahead of the summon seat to the tip the
                            // word `ctx+0x6D8` (after its own ramp) swings about
                            // the summon's facing.
                            let (mouth, tip) = self.theeder_ray(seat_slot, ctx.ctx_6d8);
                            let bearing = vm::battle_action::bearing_12bit_approx(
                                mouth[2], mouth[0], tip[2], tip[0],
                            );
                            self.seats_in_cone(
                                (mouth[0], mouth[2]),
                                bearing,
                                THEEDER_CONE_HALF_WIDTH,
                                ticks::FIRST_MONSTER_SEAT..MONSTER_ROW_END,
                            )
                        } else {
                            Vec::new()
                        };
                    let nighto_outcome = if entry == 907 {
                        self.nighto_verdict(caster_slot, victim_slot, spell_id)
                    } else {
                        ticks_a::NightoOutcome::ConfuseResisted
                    };
                    let who = ticks_a::SeruSeats {
                        caster: caster_slot,
                        victim: victim_slot,
                        summon: seat_slot,
                    };
                    let mut seats: Vec<ticks::CastActorState> = (0..self.actors.len() as u8)
                        .map(|s| self.cast_actor_state(s))
                        .collect();
                    // Carry the three views into the row so a stager that ran
                    // above this match is not thrown away.
                    for (slot, view) in [
                        (caster_slot, caster),
                        (victim_slot, victim),
                        (seat_slot, seat),
                    ] {
                        if let Some(s) = seats.get_mut(slot as usize) {
                            *s = view;
                        }
                    }
                    let (step, hits) = match entry {
                        903 => ticks_a::gimard_tick(&mut ctx, &mut seats, who, None),
                        904 => {
                            // Arm 12's cone gate is the host's: retail reads
                            // the ring's rim bearing and every seat's, and the
                            // module only ever sees "in" or "out". The damage
                            // magnitude stays the fold's, so an in-cone seat
                            // gets a zero roll and the arm's presentation half
                            // (render flag, reaction bits) runs.
                            let in_cone = cone_seats.clone();
                            let geom = self.theeder_geom(seat_slot);
                            let mut fx = self.casting.module_theeder;
                            let run = ticks_a::theeder_tick(
                                &mut ctx,
                                &mut seats,
                                who,
                                geom,
                                &mut fx,
                                |seat| in_cone.contains(&seat).then_some(0),
                            );
                            self.casting.module_theeder = fx;
                            run
                        }
                        905 => {
                            let restore = self.vera_restore(caster_slot, victim_slot, spell_id);
                            let (step, _) = ticks_a::vera_tick(&mut ctx, &mut seats, who, restore);
                            (step, Vec::new())
                        }
                        906 => ticks_a::gizam_tick(&mut ctx, &mut seats, who, |_| None),
                        907 => (
                            ticks_a::nighto_tick(&mut ctx, &mut seats, who, nighto_outcome),
                            Vec::new(),
                        ),
                        _ => ticks_a::zenoir_tick(&mut ctx, &mut seats, who, |_| None),
                    };
                    for (slot, st) in seats.iter().enumerate() {
                        self.write_cast_actor_state(slot as u8, st);
                    }
                    caster = seats.get(caster_slot as usize).copied().unwrap_or(caster);
                    victim = seats.get(victim_slot as usize).copied().unwrap_or(victim);
                    seat = seats.get(seat_slot as usize).copied().unwrap_or(seat);
                    run.aoe_hits = hits
                        .iter()
                        .map(|h| ticks::AoeHit {
                            seat: h.seat,
                            applied: h.applied as i32,
                        })
                        .collect();
                    Some(step)
                }
                // --- end W1-B ---
                // The summon band's own tick bodies - no trampoline, the
                // `0x801CF4EC` arm calls them directly.
                918 => ticks::kemaro_tick(&mut ctx, &mut victim, None),
                922 => Some(ticks::puera_tick(&mut ctx, &mut victim, None)),
                924 => Some(ticks::ultimate_rave_tick(
                    &mut ctx,
                    &mut caster,
                    &mut victim,
                )),
                925 => Some(ticks::spikefish_tick(&mut ctx, &mut caster)),
                927 => Some(ticks::juggernaut_tick(&mut ctx, &mut victim, None)),
                949 => Some(ticks::water_crystals_tick(&mut ctx, &mut victim, None)),
                // --- W1-C: player Seru 0909..0913 ---
                // These five bodies read and write the whole actor table -
                // the summon seat, the caster and the enemy row - so they
                // take the table rather than the three locals above, and the
                // locals are refreshed from it afterwards.
                //
                // PROT 0909 is the one entry in this band whose move-VM
                // stager also advances `ctx[+0x279]` (its arms `0` and `1`).
                // Retail reaches the stager from the effect script and the
                // tick from `FUN_801F1ED4` - two call sites - while this seam
                // runs both in one call, so the tick is skipped on a frame
                // the stager already stepped the phase. `casting.module_phase`
                // is not written back until the end of this function, so it
                // still holds the phase this call started on.
                909..=913 => {
                    let stager_stepped = ctx.phase != self.casting.module_phase;
                    let mut seats: Vec<ticks::CastActorState> = (0..self.actors.len() as u8)
                        .map(|s| self.cast_actor_state(s))
                        .collect();
                    let summon = seat_slot;
                    let step = if entry == 909 && stager_stepped {
                        None
                    } else {
                        let (step, hits) = self.run_seru_b_tick(
                            entry,
                            &mut ctx,
                            &mut seats,
                            caster_slot,
                            summon,
                            victim_slot,
                        );
                        run.aoe_hits.extend(hits);
                        Some(step)
                    };
                    if step.is_some() {
                        for (slot, st) in seats.iter().enumerate() {
                            self.write_cast_actor_state(slot as u8, st);
                        }
                        // The three locals are written back below; take them
                        // from the table so this arm's writes survive.
                        caster = seats.get(caster_slot as usize).copied().unwrap_or(caster);
                        victim = seats.get(victim_slot as usize).copied().unwrap_or(victim);
                        seat = seats.get(summon as usize).copied().unwrap_or(seat);
                    }
                    step
                }
                // --- end W1-C ---
                // The phase-chain bodies whose tick arm calls them directly
                // (`cast_module_ticks::chain_bodies`): PROT 0919, 0935, 0936,
                // 0937, 0939, 0947 and 0948. Their hits and heals are the
                // fold's.
                e if ticks::direct_chain_body(e).is_some() => {
                    let chain = ticks::direct_chain_body(e).expect("guarded");
                    let phase_before = ctx.phase;
                    let t = ticks::run_chain_body(
                        chain,
                        &mut ctx,
                        &mut caster,
                        &mut victim,
                        &mut seat,
                        0,
                    );
                    run.item_refund = t.refund;
                    // PROT 0919 (Spoon) arm 7 also runs the party cure ladder
                    // and its tier-4 AP doubling - the half of the arm the
                    // fold does not own (its heal is the fold's). Once, on
                    // the frame the arm lets the phase through.
                    if e == 919
                        && phase_before == vm::cast_seru_ticks_b::SPOON_HEAL_ARM
                        && ctx.phase != phase_before
                    {
                        // The views carry this frame's chain writes; seat them
                        // in the row before the sweep reads it.
                        let mut seats: Vec<ticks::CastActorState> = (0..self.actors.len() as u8)
                            .map(|s| self.cast_actor_state(s))
                            .collect();
                        for (slot, view) in [
                            (caster_slot, caster),
                            (victim_slot, victim),
                            (seat_slot, seat),
                        ] {
                            if let Some(s) = seats.get_mut(slot as usize) {
                                *s = view;
                            }
                        }
                        let cleanse = self.cure_selector(
                            caster_slot,
                            spell_id,
                            vm::cast_seru_ticks_b::ORB_CLEANSE_MIN_LEVEL,
                        );
                        vm::cast_seru_ticks_b::spoon_cure_sweep(&mut seats, cleanse);
                        for (slot, st) in seats.iter().enumerate() {
                            self.write_cast_actor_state(slot as u8, st);
                        }
                        caster = seats.get(caster_slot as usize).copied().unwrap_or(caster);
                        victim = seats.get(victim_slot as usize).copied().unwrap_or(victim);
                        seat = seats.get(seat_slot as usize).copied().unwrap_or(seat);
                    }
                    Some(t.step)
                }
                _ => None,
            }
        };
        if let Some(step) = step {
            run.busy = step == ticks::CastTickStep::Busy;
            run.tick_ported = true;
        } else if let Some(arm) = capture_arm {
            // A capture director over a body with no port (a ported body, or
            // a holding arm, has already produced a step) owns its phase.
            run.tick_ported = true;
            run.busy = arm.next.is_some();
            if let Some(next) = arm.next {
                ctx.phase = next;
            }
        }

        // Each view writes back only what the tick changed in it, folded onto
        // the slot's live state: the three views can name one actor (a
        // self-targeted cast's victim is its caster), and a whole-view write
        // of the second would undo the first's stores - PROT 0960's phase-5
        // stage of clip `0x0D` on Lu Delilas, read back as never staged,
        // held the band in `0x70` for good.
        for (slot, view, orig) in [
            (caster_slot, &caster, &caster_orig),
            (victim_slot, &victim, &victim_orig),
            (seat_slot, &seat, &seat_orig),
        ] {
            let mut live = self.cast_actor_state(slot);
            live.fold_writes(orig, view);
            self.write_cast_actor_state(slot, &live);
        }
        // PROT 0948's beam: arm 2 zeroes the builder's counter as it passes,
        // and arm 3 calls the builder (`jal 0x801F726C` at `0x801F6EF4`)
        // ahead of its own gate, so it draws on every tick the arm runs.
        self.casting.module_beam_live = false;
        if entry == vm::cast_module_ticks::CROSS_BEAM_ENTRY {
            if phase_in == 2 && ctx.phase != phase_in {
                self.casting.module_beam_counter = 0;
            }
            if phase_in == 3 {
                self.casting.module_beam_counter +=
                    vm::cast_module_ticks::CROSS_BEAM_COUNTER_PER_TICK;
                self.casting.module_beam_live = true;
            }
        }
        // The arm passed: its re-arm of the countdown word.
        if let Some(a) = arm_countdown
            && !capture_held
            && ctx.phase != phase_in
        {
            a.pass(&mut self.casting.module_cam.countdown);
        }
        self.casting.module_ctx_278 = ctx.ctx_278;
        self.casting.module_ring_angle = ctx.ctx_6d8;
        self.casting.module_phase = ctx.phase;
        // The turn-steal arms bump `ctx[+0x1A]`; it is a context byte, so it
        // has to travel back out of the view.
        self.battle_ctx.turn_cursor = ctx.turn_cursor;
        run.phase = ctx.phase;
        run.ctx_278 = ctx.ctx_278;
        // PROT 0955's two arms that reach outside the battle actor: the
        // refunded item goes back in the bag (retail's `FUN_800421D4`), and
        // the voided accessory is cleared out of the character record and
        // handed back (retail's record write plus `FUN_80042558`).
        if let Some(item) = run.item_refund {
            *self.party.inventory.entry(item).or_insert(0) += 1;
        }
        if let Some(out) = run.voided_accessory
            && let Some(id) = out.voided
        {
            let rslot = self.party_roster_slot(victim_slot as usize);
            if let Some(rec) = self.party.roster.members.get_mut(rslot) {
                let mut eq = rec.equipment();
                if let Some(slot) = eq.slots.get_mut(ACCESSORY_EQUIP_SLOT_0 + out.slot as usize) {
                    *slot = 0;
                }
                rec.set_equipment(eq);
            }
            *self.party.inventory.entry(id).or_insert(0) += 1;
            self.refresh_party_ability_bits();
        }
        Some(run)
    }

    // --- W1-C: player Seru 0909..0913 ---
    /// Dispatch one of the five player-Seru tick bodies for this frame
    /// ([`legaia_engine_vm::cast_seru_ticks_b`], PROT 0909..0913, action ids
    /// `0x87..=0x8B`).
    ///
    /// The damage and heal inputs are deliberately neutral. The engine folds
    /// a cast's HP outcome exactly once, at
    /// [`Self::cast_spell_on_slots_prepaid`], so what runs here is each
    /// module's phase machine, its `ctx+0x278` discipline and its staging /
    /// render writes; the damage step itself stays a tested kernel rather
    /// than becoming a second application. That is the same posture every
    /// other non-sweep tick body in the band takes, and it is why PROT 0911's
    /// heal amount is passed as `0`.
    ///
    /// The magnitude is not lost by that: `seru_tick_heal_amount` computes
    /// `cast_seru_ticks_b::orb_heal_amount` of the caster's per-spell magic
    /// level (the character record's `+0x161` byte, found by scanning the
    /// learned-id list at `+0x13D`) and overrides the spell catalog's
    /// placeholder inside the fold, so the amount a live Orb restores is
    /// retail's `(level << 6) + 0x1C0` clamped to the seat's missing HP.
    /// Passing it here as well would restore twice, once per owner - which is
    /// exactly what PROT 0905 used to do.
    fn run_seru_b_tick(
        &mut self,
        entry: u32,
        ctx: &mut vm::cast_module_ticks::CastModuleCtx,
        seats: &mut [vm::cast_module_ticks::CastActorState],
        caster_slot: u8,
        summon_slot: u8,
        victim_slot: u8,
    ) -> (
        vm::cast_module_ticks::CastTickStep,
        Vec<vm::cast_module_ticks::AoeHit>,
    ) {
        use legaia_engine_vm::cast_seru_ticks_b as seru;
        use vm::cast_module_ticks::{AoeHit, SweepHit};

        fn lift(hits: &[SweepHit]) -> Vec<AoeHit> {
            hits.iter()
                .map(|h| AoeHit {
                    seat: h.seat,
                    applied: h.applied as i32,
                })
                .collect()
        }
        match entry {
            909 => {
                let (step, sweep) = seru::viguro_tick(ctx, seats, caster_slot, summon_slot, |_| 0);
                (step, lift(&sweep.hits))
            }
            // PROT 0910 paces its strike on two timers - arm 7's wind-up and
            // arm 9's staggered slashes - built from the frame step
            // (`0x1F800393`) and the speed scalar (`0x1F80037D`), so the slash
            // reactions land on retail's cadence. Each of the four landings
            // runs `swordie_slash_step` with a neutral wrapper return: the
            // clamp, the hit counter and the per-slash reaction clip are
            // live, the HP outcome stays the fold's.
            //
            // Retail runs the body once a battle pass, which spans the frame
            // step's vsyncs, and drains by `rate * speed` there; the engine
            // runs it once a vsync, so its `rate` is `1` (passing the frame
            // step ran both timers at twice retail's speed). `speed` is the
            // scalar itself - the battle seating's normal rate `8` - not `1`:
            // the thresholds scale with it, and arm 7 stores it as the
            // summon's animation rate `+0x21D`, which at `1` played the
            // strike at an eighth of normal speed.
            910 => {
                let clock = seru::SwordieClock {
                    rate: 1,
                    speed: vm::battle_anim_rate::RATE_NORMAL,
                };
                let mut slashes = self.casting.module_swordie;
                let (step, hits) = seru::swordie_tick(
                    ctx,
                    seats,
                    summon_slot,
                    victim_slot,
                    &mut slashes,
                    clock,
                    |_| 0,
                );
                self.casting.module_swordie = slashes;
                (step, lift(&hits))
            }
            911 => {
                let maxes: Vec<u16> = self.actors.iter().map(|a| a.battle.max_hp).collect();
                // Spell id for a `cast_seru_ticks_b` entry: the player
                // Seru-magic block is linear, `entry = 903 + (id - 0x81)`.
                let spell_id = (entry - 903 + 0x81) as u8;
                let cleanse =
                    self.cure_selector(caster_slot, spell_id, seru::ORB_CLEANSE_MIN_LEVEL);
                let (step, _healed) = seru::orb_tick(ctx, seats, summon_slot, 0, cleanse, |s| {
                    maxes.get(s as usize).copied().unwrap_or(0)
                });
                (step, Vec::new())
            }
            912 => {
                let (step, hits) = seru::freed_tick(ctx, seats, summon_slot, |_| 0);
                (step, lift(&hits))
            }
            _ => {
                let (step, hit) = seru::nova_tick(ctx, seats, summon_slot, victim_slot, 0);
                let hits: Vec<SweepHit> = hit.into_iter().collect();
                (step, lift(&hits))
            }
        }
    }
    // --- end W1-C ---

    /// Run the band's two whole-row AoE stagers - PROT 0927 (Juggernaut,
    /// `FUN_801F85A8`) and PROT 0966 (Evil Seru Magic, `FUN_801F8D64`) -
    /// which is where those two casts' damage actually lands in retail.
    ///
    /// Both sweep a seat range with the module's own guards (skip dead, skip
    /// `+0x16E & 4`) and both clamp to `HP - 1`, so neither **sweep** can
    /// kill. That is a property of these two stagers only - PROT 0927's own
    /// tick (`0x801F6A84`) uses the unsigned `sltu` clamp instead and is
    /// kill-capable (`legaia_engine_vm::cast_module_ticks`,
    /// `docs/subsystems/cast-module.md`). ESM also
    /// stages each victim's own `+0x1F1` reaction and drops its animation rate
    /// to `2`. The roll per seat is the module's **baked** power through the
    /// module's own wrapper, drawn off this world's RNG cursor so the draw
    /// order stays retail's.
    ///
    /// Returns `None` when the spell names neither module, so the ordinary
    /// [`Self::fold_pending_cast`] path is unaffected.
    pub fn run_cast_module_aoe(&mut self, spell_id: u8, arm: u8) -> Option<CastModuleCodeRun> {
        self.run_cast_module_aoe_as(self.battle_ctx.active_actor, spell_id, arm)
    }

    /// [`Self::run_cast_module_aoe`] at the cast band's fold seam: the caster
    /// is the [`PendingCast`]'s, not whoever the context happens to point at,
    /// and the arm is the working one of the module's nine.
    fn run_cast_module_aoe_for(&mut self, caster: u8, spell_id: u8) -> Option<CastModuleCodeRun> {
        self.run_cast_module_aoe_as(caster, spell_id, AOE_STAGER_WORKING_ARM)
    }

    fn run_cast_module_aoe_as(
        &mut self,
        caster: u8,
        spell_id: u8,
        arm: u8,
    ) -> Option<CastModuleCodeRun> {
        use vm::cast_module_ticks as ticks;

        let entry = self.cast_module_for(spell_id)?;
        let shape = ticks::damage_shape_for(entry).filter(|s| s.never_kills)?;
        let ctx = self.cast_module_ctx();

        let mut seats: Vec<ticks::CastActorState> = (0..self.actors.len() as u8)
            .map(|s| self.cast_actor_state(s))
            .collect();

        // Pre-roll so the sweep borrows nothing from `self`, but roll only
        // for the seats the sweep will actually hit and in the order it visits
        // them: retail's loop calls the wrapper *after* its two skip guards,
        // so a dead or non-targetable seat draws nothing and the shared RNG
        // cursor must not advance for it either.
        let order: Vec<u8> = if entry == 966 {
            (0..ctx.party_count).collect()
        } else {
            (0..ctx.monster_count)
                .map(|i| ticks::FIRST_MONSTER_SEAT.saturating_add(i))
                .collect()
        };
        let mut rolls: Vec<(u8, i32)> = Vec::with_capacity(order.len());
        for seat in order {
            let hittable = seats
                .get(seat as usize)
                .is_some_and(ticks::aoe_seat_is_hittable);
            if !hittable {
                continue;
            }
            let roll = self
                .capture_module_roll(shape, caster, seat)
                .unwrap_or_default();
            rolls.push((seat, roll));
        }
        self.refresh_seat_spirit(&mut seats);
        let take = |seat: u8| {
            rolls
                .iter()
                .find(|(s, _)| *s == seat)
                .map(|(_, r)| *r)
                .unwrap_or_default()
        };

        let hits = if entry == 966 {
            ticks::evil_seru_magic_stager(&ctx, &mut seats, arm, take)
        } else {
            ticks::juggernaut_stager(&ctx, &mut seats, arm, take)
        };
        for (slot, st) in seats.iter().enumerate() {
            self.write_cast_actor_state(slot as u8, st);
        }
        Some(CastModuleCodeRun {
            prot_entry: entry,
            phase: ctx.phase,
            ctx_278: ctx.ctx_278,
            busy: false,
            tick_ported: true,
            aoe_hits: hits,
            item_refund: None,
            voided_accessory: None,
            element_change: None,
            camera_shot: None,
            camera_follow: None,
            camera_nudge: None,
            capture_drift: None,
            spawns: &[],
            vram_move: None,
            caption: None,
            fades: &[],
            kills_fades: false,
        })
    }

    /// One hit off a module's own damage shape: its baked power, its wrapper,
    /// this world's RNG cursor. The raw signed wrapper return - the caller
    /// applies the module's clamp.
    fn capture_module_roll(
        &mut self,
        shape: &vm::cast_module_ticks::CastDamageShape,
        attacker: u8,
        target: u8,
    ) -> Option<i32> {
        use legaia_engine_vm::battle_damage_wrappers::{WrapperAttacker, WrapperDefender};
        use vm::cast_module_ticks::roll_module_hit;

        let element_affinity_pct = self.enemy_affinity_pct(attacker, target);
        let attacker_hp = self.actors.get(attacker as usize)?.battle.hp;
        let defender = self.summon_roll_defender(target)?;
        let a = WrapperAttacker {
            hp: attacker_hp,
            agl: self
                .battle
                .accuracy
                .get(attacker as usize)
                .copied()
                .unwrap_or(0),
            spell_power: self
                .battle
                .attack
                .get(attacker as usize)
                .copied()
                .unwrap_or(0),
            status: 0,
        };
        let d = WrapperDefender {
            hp: defender.hp,
            agl: defender.agl,
            stat_a: defender.stat_a,
            stat_b: defender.stat_b,
            status: 0,
            guard: 0,
        };
        let rng = [
            self.next_rand() as u16,
            self.next_rand() as u16,
            self.next_rand() as u16,
        ];
        let net = roll_module_hit(shape, 0, &a, &d, element_affinity_pct, rng, || {
            self.next_rand() as u16
        });
        Some(self.finish_module_hit(shape, attacker, target, net))
    }

    /// The finisher half of a module wrapper hit. Both per-move wrappers end
    /// in the shared finisher: `FUN_801DD4B0` calls `jal 0x801DDB30` at
    /// `0x801DD678` on its own attacker / defender roll words and returns
    /// their difference afterwards (`subu v0,v1,v0` at `0x801DD6A8`), so the
    /// net a module stores into `+0x14C` is **post**-finisher: the party
    /// resist ladder (skipped by `FUN_801DD6B4`'s `param_5 = 1`), Mystic
    /// Shield's enemy-defender halve, the guard halve (`+0x1DE == 4`), the
    /// no-damage floor and the `9999` cap - and the finisher's spirit stage
    /// fills the defender's gauge from the same hit. The earlier port
    /// returned the raw wrapper net, so a guarding member took Cort's Mystic
    /// Circle whole and gained no Spirit from it.
    ///
    /// `SharedSummon` (PROT 0927, `FUN_801DD0AC`'s summon branch) keeps the
    /// raw net: its finisher arguments (attacker slot `7`, the power-percent
    /// scale) are the shared kernel's own and are not modelled here.
    ///
    /// PORT: FUN_801DD4B0 (`0x801DD66C..0x801DD6AC`, the finisher call and return)
    fn finish_module_hit(
        &mut self,
        shape: &vm::cast_module_ticks::CastDamageShape,
        attacker: u8,
        target: u8,
        net: i32,
    ) -> i32 {
        use legaia_engine_vm::battle_damage_wrappers::{
            ATK_WRAPPER_BYPASSES_PARTY_RESIST, INT_WRAPPER_BYPASSES_PARTY_RESIST,
        };
        use vm::battle_formulas::{DamageFinish, damage_finish_lazy};
        use vm::cast_module_ticks::CastWrapper;

        let bypass_party_resist = match shape.wrapper {
            CastWrapper::Respect => INT_WRAPPER_BYPASSES_PARTY_RESIST,
            CastWrapper::Bypass => ATK_WRAPPER_BYPASSES_PARTY_RESIST,
            CastWrapper::SharedSummon => return net,
        };
        let party_count = self.party.party_count;
        let attacker_element = self.monster_seat_element(attacker as usize).unwrap_or(7);
        let finish = DamageFinish {
            predamage: net.max(0) as u32,
            attacker_slot: if attacker < party_count { 0 } else { 3 },
            defender_slot: if target < party_count { 0 } else { 3 },
            attacker_element,
            defender_resist: self.defender_resist(target),
            defender_guarding: self
                .battle
                .guarding
                .get(target as usize)
                .copied()
                .unwrap_or(false),
            enemy_defender_halve: self.mystic_shield_up(),
            bypass_party_resist,
            summon_power_pct: 100,
            floor_rand: 0,
        };
        let over = damage_finish_lazy(&finish, || self.next_rand() as u16).min(9999);
        self.accrue_spirit_gauge(target, over as u16);
        over as i32
    }

    /// Carry the spirit gauges [`Self::finish_module_hit`] filled into seat
    /// snapshots taken before the rolls, so the tick's write-back does not
    /// restore the pre-hit gauge.
    fn refresh_seat_spirit(&self, seats: &mut [vm::cast_module_ticks::CastActorState]) {
        for (slot, st) in seats.iter_mut().enumerate() {
            if let Some(a) = self.actors.get(slot) {
                st.spirit_gauge = a.battle.spirit_gauge;
            }
        }
    }

    /// Pre-roll the status draws one whole-row sweep makes, in the order
    /// retail visits its seats: the two `FUN_80056798` calls PROT 0938's
    /// `0x4E` body makes per hittable seat (`0x801F7888` / `0x801F78B0`),
    /// which decide Venom then Toxic.
    ///
    /// The **damage** roll comes first, per seat, because retail's loop calls
    /// the wrapper before the status draws: each of the three sweeps opens
    /// its seat body with `jal 0x801DD4B0` on its own baked power
    /// ([`vm::cast_module_ticks::sweep_damage_shape_for`]) and stores the
    /// clamped net into `+0x14C` itself. So these bodies **own** the cast's
    /// HP outcome the way the PROT 0927 / 0966 stagers do, and
    /// [`Self::fold_pending_cast`] must not also run the generic fold - which
    /// is the double-application trap. The clamp is the kill-capable one
    /// (`sltu` at `0x801F77EC` / `0x801F7118` / `0x801F77E0`), unlike the two
    /// stagers' `HP - 1`.
    ///
    /// Returns `(seat, damage, (status_a, status_b))` in visit order.
    fn sweep_status_rolls(
        &mut self,
        ctx: &vm::cast_module_ticks::CastModuleCtx,
        seats: &[vm::cast_module_ticks::CastActorState],
        body: u32,
        caster: u8,
    ) -> Vec<(u8, i32, (u32, u32))> {
        use vm::cast_module_ticks as ticks;
        // Only the body's own sweep arm draws anything: retail reaches the
        // wrapper and the two status calls inside that one arm, so rolling on
        // every re-entry would run the shared `rand()` cursor forward on
        // frames retail never draws.
        let sweep_arm = match body {
            ticks::CHAOS_BREATH_TICK => ticks::CHAOS_BREATH_SWEEP_ARM,
            ticks::MYSTIC_CIRCLE_TICK => ticks::MYSTIC_CIRCLE_SWEEP_ARM,
            _ => ticks::DOOMSDAY_SWEEP_ARM,
        };
        if ctx.phase != sweep_arm {
            return Vec::new();
        }
        let shape = ticks::sweep_damage_shape_for(body);
        let mut out = Vec::new();
        for seat in 0..ctx.party_count {
            let Some(s) = seats.get(seat as usize) else {
                continue;
            };
            // PROT 0938's `0xB7` body and PROT 0965's skip only a dead seat;
            // the `0x4E` body also skips `+0x16E & 4`.
            let hittable = if body == ticks::CHAOS_BREATH_TICK {
                ticks::aoe_seat_is_hittable(s)
            } else {
                s.hp != 0
            };
            if !hittable {
                continue;
            }
            let damage = match shape {
                Some(sh) => self.capture_module_roll(sh, caster, seat).unwrap_or(0),
                None => 0,
            };
            out.push((seat, damage, (self.next_rand(), self.next_rand())));
        }
        out
    }

    // --- W1-D: the fourteen trampoline arms ---

    /// The five extra record fields
    /// [`legaia_engine_vm::cast_arm_ticks::CastArmExtState`] carries, lifted
    /// off one actor slot.
    ///
    /// Two of them have an engine home: `+0x150` is the actor's live MP and
    /// `+0x172` its max HP. The other three do not, and the reason is that no
    /// routine in PROT 0903..0966 reads them back - `+0x152` (the MP base) and
    /// `+0x178` (where PROT 0943's drain stashes the old working MP) are
    /// write-only in the band, and `+0x1F3` sits one byte past the end of the
    /// engine's `+0x1DF..+0x1F2` action-parameter window. They round-trip
    /// through the view for the tick's own arithmetic and are dropped here.
    fn cast_arm_ext_state(&self, slot: u8) -> vm::cast_arm_ticks::CastArmExtState {
        use vm::cast_arm_ticks::CastArmExtState;
        let Some(a) = self.actors.get(slot as usize) else {
            return CastArmExtState::default();
        };
        CastArmExtState {
            mp: a.battle.mp,
            mp_base: a.battle.mp,
            mp_stash: 0,
            max_hp: a.battle.max_hp,
            reaction_extra: 0,
        }
    }

    /// Write back the two halves of [`Self::cast_arm_ext_state`] the engine
    /// actually carries.
    fn write_cast_arm_ext_state(&mut self, slot: u8, st: &vm::cast_arm_ticks::CastArmExtState) {
        let Some(a) = self.actors.get_mut(slot as usize) else {
            return;
        };
        a.battle.mp = st.mp;
        a.battle.max_hp = st.max_hp;
    }

    /// Pre-roll one whole-row sweep arm's per-seat damage, in the order retail
    /// visits its seats.
    ///
    /// Only the body's own sweep arm draws anything
    /// ([`legaia_engine_vm::cast_arm_ticks::arm_sweep_arm`]); rolling on every
    /// re-entry would run the shared RNG cursor forward on frames retail never
    /// draws. The sibling of [`Self::sweep_status_rolls`] for the arms
    /// `cast_arm_ticks` carries.
    fn cast_arm_sweep_rolls(
        &mut self,
        ctx: &vm::cast_module_ticks::CastModuleCtx,
        entry: u32,
        body: u32,
        caster: u8,
    ) -> Vec<(u8, i32)> {
        let Some(arm) = vm::cast_arm_ticks::arm_sweep_arm(entry, body) else {
            return Vec::new();
        };
        if ctx.phase != arm {
            return Vec::new();
        }
        let Some(shape) = vm::cast_arm_ticks::arm_damage_shape_for(entry, body) else {
            return Vec::new();
        };
        let seats: Vec<vm::cast_module_ticks::CastActorState> = (0..ctx.party_count)
            .map(|s| self.cast_actor_state(s))
            .collect();
        let mut out = Vec::new();
        for (seat, s) in seats.iter().enumerate() {
            if !vm::cast_module_ticks::aoe_seat_is_hittable(s) {
                continue;
            }
            let seat = seat as u8;
            out.push((
                seat,
                self.capture_module_roll(shape, caster, seat).unwrap_or(0),
            ));
        }
        out
    }

    /// Drive one of the two table-dispatched sweep arms end to end: roll,
    /// tick, write the seats back.
    fn run_cast_arm_sweep(
        &mut self,
        ctx: &mut vm::cast_module_ticks::CastModuleCtx,
        entry: u32,
        body: u32,
        caster_slot: u8,
        _caster: &mut vm::cast_module_ticks::CastActorState,
    ) -> (
        vm::cast_module_ticks::CastTickStep,
        Vec<vm::cast_module_ticks::AoeHit>,
    ) {
        let sweep = Some(ctx.phase) == vm::cast_arm_ticks::arm_sweep_arm(entry, body);
        let rolls = self.cast_arm_sweep_rolls(ctx, entry, body, caster_slot);
        let mut seats: Vec<vm::cast_module_ticks::CastActorState> = (0..self.actors.len() as u8)
            .map(|s| self.cast_actor_state(s))
            .collect();
        let take = |seat: u8| {
            rolls
                .iter()
                .find(|(s, _)| *s == seat)
                .map(|(_, d)| *d)
                .unwrap_or(0)
        };
        let (step, hits) = vm::cast_arm_ticks::steal_sweep_tick(ctx, &mut seats, sweep, take);
        for (slot, st) in seats.iter().enumerate() {
            self.write_cast_actor_state(slot as u8, st);
        }
        (
            step,
            hits.iter()
                .map(|h| vm::cast_module_ticks::AoeHit {
                    seat: h.seat,
                    applied: h.applied as i32,
                })
                .collect(),
        )
    }

    /// PROT 0941's Steal resolution, both legs.
    ///
    /// **Monster seat** (`victim_slot >= FIRST_MONSTER_SEAT`): the static
    /// `SCUS_942.54` steal table `0x80077828 + monster_id * 2`, fields
    /// `[chance, item]` - the same table the player-side steal reads, and NOT
    /// a field of the PROT 867 monster record (`docs/formats/steal-table.md`).
    /// `rand() % 100 < chance` decides. `None` when no table is installed (a
    /// disc-free host) or the victim carries no monster id, which keeps a
    /// synthetic battle from inventing a steal and from drawing the roll.
    ///
    /// **Party seat**: the bag draw plus the consume. The rejection rule
    /// (`id != 0 && count != 0 && the item table knows the id`, up to `0x400`
    /// draws, one RNG draw per rejection so the shared cursor advances the way
    /// retail's does) is the module's, byte for byte.
    ///
    /// The **array** is retail's own: [`crate::world::ItemBag`] holds the
    /// physical 256 slots, so the draw rejects its way past a played-through
    /// bag's holes exactly as retail's does, and the third acceptance leg is
    /// the item record's **shop price** halfword (`0x80074368 + id*0xC + 2`,
    /// `0x801F789C`) rather than a tautology over the ids the bag already
    /// holds - the quest and found-only items carry a zero price and are
    /// unstealable.
    ///
    /// The re-roll floor is applied too. `0x801F77E8` arms it on
    /// `DAT_8007BD10[1] == 4` (the second present-party member's character id)
    /// and re-draws while `slot < *(i16*)0x8007B5EA`, which is `gp[+0x2D2]` -
    /// the active window's **start** (`gp = 0x8007B318`), so the arm confines
    /// the draw to the window's own half.
    ///
    /// The removal is asymmetric with the draw, and deliberately so: the draw
    /// is over the whole array while `FUN_80042310` scans only
    /// `[gp[+0x2D2], gp[+0x2D4])` and returns `0x100` for an id outside it,
    /// touching nothing. A steal that lands on the other half's slot therefore
    /// announces an item the party keeps.
    pub(in crate::world) fn roll_cast_steal(
        &mut self,
        victim_slot: u8,
    ) -> Option<vm::cast_arm_ticks::StealOutcome> {
        use vm::cast_arm_ticks::StealOutcome;
        use vm::cast_module_ticks::FIRST_MONSTER_SEAT;
        if victim_slot >= FIRST_MONSTER_SEAT {
            let entry = self
                .actors
                .get(victim_slot as usize)
                .and_then(|a| a.battle_monster_id)
                .and_then(|id| self.tables.steal_table.as_ref()?.entry(id))?;
            let roll = (self.next_rand() % 100) as u8;
            return Some(StealOutcome::FromMonster {
                chance: entry.chance_pct,
                item: entry.item_id,
                roll,
                hit: roll < entry.chance_pct,
            });
        }
        let bag: Vec<(u8, u8)> = self.party.inventory.slots().to_vec();
        // The price leg, precomputed so the draw closure can borrow `self`
        // for the RNG. Without a disc image there is no item table to read a
        // price from, so the leg cannot be evaluated and every id passes -
        // a disc-free host still spends retail's draws and rejects on the
        // two legs it can see.
        let mut priced = [true; 256];
        if let Some(data) = self.shops.item_shop_data.as_ref() {
            for (id, cell) in priced.iter_mut().enumerate() {
                *cell = data.price(id as u8) != 0;
            }
        }
        // `DAT_8007BD10[1] == 4`: the second present-party member's character
        // id, which the engine mirrors as `party_roster_slot(1) + 1`. A party
        // of one has no second member, and the port's identity mapping would
        // fabricate one, so the arm needs both.
        let floor = (self.party.party_count > 1 && self.party_roster_slot(1) as u8 + 1 == 4)
            .then(|| self.party.inventory.window_bounds().0.min(0xFF) as u8);
        // One `next_rand` per rejected slot, not a pre-drawn batch: retail
        // advances the shared cursor once per draw, so over-drawing would
        // desynchronise every later roll in the battle.
        let slot = vm::cast_arm_ticks::steal_pick_bag_slot(
            &bag,
            floor,
            || self.next_rand(),
            |id| priced[id as usize],
        );
        match slot.and_then(|s| bag.get(s as usize).copied()) {
            Some((item, _)) => Some(StealOutcome::FromBag { item }),
            None => Some(StealOutcome::BagEmpty),
        }
    }

    /// The inventory consume PROT 0941's Steal performs (`FUN_80042310`),
    /// through the window-bounded helper.
    ///
    /// Returns whether a slot was actually emptied: the helper's `0x100`
    /// sentinel says the id is outside the active window, and retail's own
    /// steal ignores the return, so the message has already been staged by the
    /// time the removal declines. The engine keeps the same order and reports
    /// the difference instead of hiding it.
    pub(in crate::world) fn take_one_from_bag(&mut self, item: u8) -> bool {
        self.party.inventory.consume_returning_slot(item, 1)
            != legaia_save::retail_inventory::NOT_IN_WINDOW
    }

    /// Materialise the seat PROT 0940's split allocated.
    ///
    /// Retail builds a whole actor: it copies the caster's monster-record
    /// pointer into `0x801C9348[seat]`, allocates a display object through
    /// `FUN_80054CB0` / `FUN_80024C88`, and unaligned-copies the caster's
    /// pose. The engine's equivalent is the caster's own actor record cloned
    /// into the seat, and the five simulation writes the arm makes on top of
    /// that (`+0x16C`, `+0x1DE`, `+0x1DF`, `+0x1DD`, `+0x14C` / `+0x172`) are
    /// what this applies.
    ///
    /// Returns `false` when the table has no seat there, which is retail's own
    /// bound: `ctx[+1]` indexes `actor_table` and the engine caps both counts
    /// at [`BATTLE_TABLE_SLOTS`].
    fn apply_glare_divide_split(
        &mut self,
        caster_slot: u8,
        split: &vm::cast_arm_ticks::GlareDivideSplit,
    ) -> bool {
        use vm::cast_arm_ticks::{
            SPLIT_CLONE_ACTION, SPLIT_CLONE_CATEGORY, SPLIT_WEAK_AGL, SplitWeakened,
        };
        let seat = split.clone_seat as usize;
        if seat >= self.actors.len() || seat >= BATTLE_TABLE_SLOTS {
            return false;
        }
        let Some(src) = self.actors.get(caster_slot as usize).cloned() else {
            return false;
        };
        let mut clone = src;
        clone.active = true;
        clone.battle.init_key = 0;
        clone.battle.action_category = SPLIT_CLONE_CATEGORY;
        if let Some(p) = clone.battle.params.first_mut() {
            *p = SPLIT_CLONE_ACTION;
        }
        clone.battle.active_target = vm::cast_module_ticks::TARGET_CODE_ENEMY_ROW;
        clone.battle.hp = split.clone_hp;
        clone.battle.max_hp = split.clone_hp;
        if split.weakened == Some(SplitWeakened::Clone) {
            clone.battle.hp = 1;
            clone.battle.mp = 0;
            clone.battle.atk_working = 1;
            clone.battle.agl = SPLIT_WEAK_AGL;
            clone.battle.agl_base = SPLIT_WEAK_AGL;
        }
        self.actors[seat] = clone;
        true
    }

    /// The three accessory ids PROT 0955's Void Accessories rolls between -
    /// `record[+0x19B + slot]` for the character seated at `slot`.
    fn cast_victim_accessories(&self, slot: u8) -> [u8; 3] {
        let mut out = [0u8; 3];
        let Some(rec) = self
            .party
            .roster
            .members
            .get(self.party_roster_slot(slot as usize))
        else {
            return out;
        };
        // `+0x196` is equipment slot 0, so `+0x19B` - the module's
        // `+ 0x75E + 5` - is index 5, and the three accessory slots are
        // 5, 6, 7 (`legaia_save::character::EquipmentSlots`).
        let eq = rec.equipment();
        for (i, o) in out.iter_mut().enumerate() {
            *o = eq
                .slots
                .get(ACCESSORY_EQUIP_SLOT_0 + i)
                .copied()
                .unwrap_or(0);
        }
        out
    }

    /// Apply one enemy-cast hit through retail's **safe** applier - the hit
    /// arm of `FUN_801E09F8`'s per-slot effect-child driver
    /// (`legaia_engine_vm::battle_cast_census::effect_child_hit`).
    ///
    /// This is the path a monster's cast takes in retail, and it differs from
    /// the action band's accumulating seed in the one way that matters: the
    /// roll is clamped against live HP **once** and that single value reaches
    /// both the readout accumulator `+0x10` and live HP `+0x14C`, so the bar
    /// can never be asked to travel further than HP moved. The action band's
    /// seed can, which is the `0x51` settle park
    /// `legaia_engine_vm::battle_hp_bar` documents.
    ///
    /// Also carried: the reaction-clip pick (`+0x1F2` gates `+0x1F1` against
    /// `+0x1EF` / `+0x1F0`, and a dead victim always takes `+0x1F1`), the
    /// `+0x1DC` **bit** ORs - retail ORs here, it does not bump - and the
    /// readout cursor `ctx[+0x262]`.
    ///
    /// Returns the damage actually applied.
    pub(in crate::world) fn apply_effect_child_hit(&mut self, slot: usize, damage: i32) -> i32 {
        use vm::battle_cast_census::{EffectChildVictim, effect_child_hit};
        let mut cursor = self.battle_ctx.cast_readout_cursor;
        let Some(a) = self.actors.get_mut(slot) else {
            return 0;
        };
        a.battle.arm_hp_bar();
        let p = |i: usize| a.battle.params.get(i - 0x1DF).copied().unwrap_or(0);
        let mut victim = EffectChildVictim {
            hp: a.battle.hp,
            hp_bar_delta: a.battle.hp_bar_pending,
            flags: a.battle.field_flags,
            staged_anim: a.battle.queued_anim,
            restage: 0,
            reaction_alt: p(0x1EF),
            reaction_alt2: p(0x1F0),
            knockdown_anim: p(0x1F1),
            reaction_gate: p(0x1F2),
        };
        let hit = effect_child_hit(&mut victim, damage, &mut cursor);
        a.battle.hp = victim.hp;
        a.battle.hp_bar_pending = victim.hp_bar_delta;
        a.battle.queued_anim = victim.staged_anim;
        // `hp == 0 -> liveness = 0` holds for **present** actors only, the
        // same `max_hp > 0` guard `apply_battle_hp_delta` applies.
        if a.battle.max_hp > 0 && a.battle.hp == 0 {
            a.battle.liveness = 0;
        }
        self.battle_ctx.cast_readout_cursor = cursor;
        hit.applied
    }
}

#[cfg(test)]
mod capture_hold_tests {
    use super::*;

    fn band_world() -> World {
        let mut world = World {
            party: crate::world::PartyState {
                party_count: 3,
                ..Default::default()
            },
            ..World::default()
        };
        while world.actors.len() < 8 {
            world.actors.push(crate::world::Actor::default());
        }
        for a in world.actors.iter_mut() {
            a.active = true;
            a.battle.hp = 100;
            a.battle.max_hp = 100;
            a.battle.liveness = 1;
        }
        world.mode = SceneMode::Battle;
        world
    }

    /// A module wrapper hit runs the shared finisher (`FUN_801DD4B0` calls
    /// `0x801DDB30` at `0x801DD678`): Mystic Circle's sweep is halved on a
    /// guarding seat, and the hit fills the defender's Spirit gauge.
    #[test]
    fn a_module_hit_is_finished_guard_halve_and_spirit() {
        use vm::cast_module_ticks::{MYSTIC_CIRCLE_TICK, sweep_damage_shape_for};
        let shape = sweep_damage_shape_for(MYSTIC_CIRCLE_TICK).expect("Mystic Circle shape");
        let mut world = band_world();
        world.actors[0].battle.max_hp = 2000;
        world.actors[0].battle.spirit_gauge = 0;
        assert_eq!(world.finish_module_hit(shape, 3, 0, 1200), 1200);
        assert_eq!(world.actors[0].battle.spirit_gauge, 60, "1200 of 2000 HP");

        world.actors[0].battle.spirit_gauge = 0;
        world.battle.guarding[0] = true;
        assert_eq!(world.finish_module_hit(shape, 3, 0, 1200), 600);
        assert_eq!(world.actors[0].battle.spirit_gauge, 30);
    }

    /// Nothing paged in: the seam is inert, so a host that never reaches the
    /// capture band behaves exactly as it did before the hold existed.
    #[test]
    fn an_unarmed_band_is_never_busy() {
        let mut world = band_world();
        assert!(world.casting.capture_spell.is_none());
        assert!(!world.capture_stager_tick());
    }

    /// Arming resets the module phase pair - retail's `sb zero,0x279(v0)` at
    /// the `0x6F` exit (`0x801E5048`), just before `0x70` starts ticking.
    #[test]
    fn arming_resets_the_module_phase_pair() {
        let mut world = band_world();
        world.casting.module_phase = 9;
        world.casting.module_ctx_278 = 7;
        world.arm_capture_cast_module(0x87);
        assert_eq!(world.casting.capture_spell, Some(0x87));
        assert_eq!(world.casting.module_phase, 0);
        assert_eq!(world.casting.module_ctx_278, 0);
    }

    /// A spell that names no band entry arms nothing, so no host can be
    /// parked on a module that is not there.
    #[test]
    fn a_spell_with_no_module_arms_nothing() {
        let mut world = band_world();
        world.arm_capture_cast_module(0x00);
        assert!(world.casting.capture_spell.is_none());
        assert!(!world.capture_stager_tick());
    }

    /// The no-softlock rule. `CastModuleCodeRun::busy` seeds `true` for any
    /// resident entry, so a module whose tick body is unported would hold
    /// phase `0x70` forever; the hold reads `tick_ported` first and lets the
    /// band through instead, disarming as it goes.
    #[test]
    fn an_unported_tick_body_never_holds_the_phase() {
        let mut world = band_world();
        // --- W1-C ---
        // PROT 0914's tick body is one of the band's unported rows. (This
        // test used to key on PROT 0909, whose tick body is now ported -
        // `legaia_engine_vm::cast_seru_ticks_b::viguro_tick` - so keying on
        // it would have made the assertion vacuous.)
        assert_eq!(world.cast_module_for(0x8C), Some(914));
        let run = world.run_cast_module_code(0x8C, 0).unwrap();
        assert!(run.busy, "the seeded value on its own says 'busy'");
        assert!(!run.tick_ported, "but no tick body ran");

        world.arm_capture_cast_module(0x8C);
        // --- end W1-C ---
        assert!(
            !world.capture_stager_tick(),
            "so the band is not held on it"
        );
        assert!(
            world.casting.capture_spell.is_none(),
            "and the module stops being re-entered"
        );
    }
}

/// The live command flow's sweep encoding: a group-shaped cast reaches the SM
/// as retail's group code (`8` party / `9` enemy row, absolute numbering),
/// never a sentinel, and a self-target stays the caster's own slot - the
/// value `FUN_801E295C`'s cast-begin split (`sltiu v0,t2,0x8` at
/// `0x801E433C`) and its self-skip (`beq v0,t2` at `0x801E4350`) expect.
#[cfg(test)]
mod sweep_code_tests {
    use super::*;
    use crate::spells::{SpellDef, SpellTarget};
    use vm::battle_target_group::{TARGET_GROUP_ENEMIES, TARGET_GROUP_PARTY};

    fn staged_target(caster: u8, target: SpellTarget, targets: Vec<u8>) -> u8 {
        let mut world = World::default();
        world.enter_battle(3, 2);
        let def = SpellDef {
            id: 0x81,
            target,
            ..SpellDef::default()
        };
        world.arm_player_cast(caster, &def, targets);
        world.actors[caster as usize].battle.active_target
    }

    #[test]
    fn a_party_casters_sweeps_carry_retail_group_codes() {
        assert_eq!(
            staged_target(0, SpellTarget::AllAllies, vec![0, 1, 2]),
            TARGET_GROUP_PARTY
        );
        assert_eq!(
            staged_target(0, SpellTarget::AllEnemies, vec![3, 4]),
            TARGET_GROUP_ENEMIES
        );
    }

    #[test]
    fn a_monster_casters_sweeps_are_mirrored() {
        assert_eq!(
            staged_target(3, SpellTarget::AllAllies, vec![3, 4]),
            TARGET_GROUP_ENEMIES
        );
        assert_eq!(
            staged_target(3, SpellTarget::AllEnemies, vec![0, 1, 2]),
            TARGET_GROUP_PARTY
        );
    }

    #[test]
    fn a_self_target_is_the_casters_own_slot() {
        assert_eq!(staged_target(1, SpellTarget::SelfOnly, vec![1]), 1);
    }
}
