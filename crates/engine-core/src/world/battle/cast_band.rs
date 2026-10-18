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
//! summon, the homing flight's landed hit for a monster's cast whose clip
//! seeds one, the `0x29` exit for everything else - through
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
//! ([`legaia_overlay_images::cast_effect_pool`], installed by the scene host) and stage
//! through [`World::spawn_cast_module_fx`] at the same first tick, because the
//! records are the shape the summon path already runs. What stays open is the
//! module's *code* half - lift, camera, phase machine, damage shape - where a
//! module's tick bodies are not ported
//! (`docs/subsystems/cast-module.md`).
//!
//! Timing is the engine's: the retail durations are the stager's own phase
//! machine and are not dumped, so the frame counts below are chosen to land
//! the strike inside the band's `0x78`-frame sustain, the way the capture
//! shows it.

use super::*;

pub use presentation::ModulePresentation;

mod arm_and_fold;
mod caster_stages;
mod module_creature;
mod module_effects;
mod module_rain;
mod module_seat_arm;
mod module_spawn;
mod module_stager_spawns;
mod nova_motion;
mod orb_stager;
mod presentation;
mod screen_fade;
mod summon_stager;
mod swordie_blades;
mod voice_cues;

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
/// along its heading onto the victim: Gimard's walk clip (archive creature
/// `10`, clip 1, tag `1`) carries root speed `28`, which the anim tick's
/// root-motion term integrates for a creature seated with its clips. A
/// per-vsync log from `shiny_refactor_gimard_plus35` measures the same rate:
/// each game frame at step `4` moves the creature `111` units (`(-16, 110)`)
/// and swings the yaw base `6 * step * scalar + drift = 197`, and the arm's
/// first pass moves the creature before the swing has run - which is what an
/// earlier reading off `gimard_burning_attack`'s yaw (`985`, read as 20.5
/// frames and `32.7` a frame) left out. This constant only walks a creature
/// whose playing clip carries no speed (a headless seat).
const SUMMON_DIRECTED_WALK_STEP: i32 = 28;
/// Frames the creature stands at the strike point after the outcome.
const SUMMON_LINGER_FRAMES: u16 = 40;
/// The longest the band lingers on a module still busy after its outcome
/// before it lets go anyway: twice the longest post-hit tail a ported module
/// runs (PROT 0928's settle arm `0xC8`, `T = scalar * 128` drained a scalar a
/// tick, then its row poll). A module still busy past it is held on a poll
/// the engine cannot answer - a dead seat whose defeat fade never runs, so
/// its colour word `+0x04` never reaches the zero the row polls wait for -
/// and retail's band would not be standing there. Letting go keeps the
/// fight moving.
pub(in crate::world) const SUMMON_LINGER_STALL_FRAMES: u16 = 0x800;
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
fn evil_seru_magic_fade(phase: u8) -> Option<legaia_engine_system::fade::FadeTemplate> {
    let (duration, start_rgb, end_rgb, mode) = match phase {
        2 => (0x10, [0xFF, 0x40, 0x40], [0; 3], [0, 0, 0]),
        4 => (0x40, [0; 3], [0xFF; 3], [0, -1, 0]),
        10 | 27 => (0x80, [0xFF; 3], [0; 3], [0, 0, 0]),
        26 => (0x40, [0; 3], [0xFF; 3], [0xC0, -1, 0]),
        _ => return None,
    };
    Some(legaia_engine_system::fade::FadeTemplate {
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
    /// The fold waits on the move's homing flight: each target is folded on
    /// the pass its effect child lands its hit
    /// ([`World::settle_cast_band`], [`World::fold_pending_cast_target`]).
    pub on_flight: bool,
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
/// write ([`legaia_engine_cast::cast_module_ticks::CAPTURE_CASTER_STAGES`]),
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
    /// The module arm whose pass stages each clip, parallel to
    /// [`Self::clips`] - empty when the clips replay at the head of the band
    /// ([`legaia_engine_cast::cast_module_ticks::capture_caster_stage_arms`]).
    /// A placed clip does not hold the band: the arms run on while it plays.
    pub at_arms: Vec<u8>,
    /// Index of the clip being staged / waited on.
    pub cursor: usize,
    /// The seat the caster walks into reach of before the first stage
    /// ([`legaia_engine_cast::cast_module_ticks::capture_body_approaches`]); `None` for a
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
    pub aoe_hits: Vec<legaia_engine_cast::cast_module_ticks::AoeHit>,
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
    pub voided_accessory: Option<legaia_engine_cast::cast_module_ticks::VoidAccessoriesOutcome>,
    /// `(element, group)` PROT 0964's Element Change committed onto the first
    /// monster seat's record this frame (`+0x1D` / `+0x1C`).
    pub element_change: Option<(u8, u8)>,
    /// The camera shot the module armed this frame
    /// ([`vm::battle_cam_script::ModuleShot`]).
    pub camera_shot: Option<legaia_engine_battle_vm::battle_cam_script::ModuleShot>,
    /// The case-6 follow the module re-armed this frame
    /// ([`legaia_engine_cast::cast_module_camera::ModuleFollow`]).
    pub camera_follow: Option<legaia_engine_cast::cast_module_camera::ModuleFollow>,
    /// The module framed the acting actor through case 8 this frame
    /// ([`legaia_engine_cast::cast_module_camera::ArmDirection::end_frame`]).
    pub camera_end_frame: bool,
    /// The drift the module wrote into the camera globals this frame.
    pub camera_nudge: Option<legaia_engine_cast::cast_module_camera::ModuleNudge>,
    /// A capture-class body's drift this frame
    /// ([`legaia_engine_cast::cast_module_camera::capture_camera_director`]).
    pub capture_drift: Option<legaia_engine_cast::cast_module_camera::CaptureDrift>,
    /// The spawn calls the module's arm made this frame, for a director that
    /// reports them ([`legaia_engine_cast::cast_module_camera::ModuleProfile::stages_spawns`]).
    pub spawns: &'static [legaia_engine_cast::cast_module_camera::ModuleSpawn],
    /// The spawn calls a summon-creature arm makes on every pass, held or
    /// not, beside [`Self::spawns`]
    /// ([`legaia_engine_cast::cast_module_camera::creature_arm_spawns`]).
    pub every_spawns: &'static [legaia_engine_cast::cast_module_camera::ModuleSpawn],
    /// The point the arm built for its [`legaia_engine_cast::cast_module_camera::SpawnAnchor::ArmPoint`]
    /// spawns this frame.
    pub arm_point: Option<([i16; 3], [i16; 3])>,
    /// The `MoveImage` the module's arm issued this frame.
    pub vram_move: Option<legaia_engine_cast::cast_module_camera::ModuleVramMove>,
    /// The text the module's arm put up this frame.
    pub caption: Option<legaia_engine_cast::cast_module_camera::ModuleCaption>,
    /// The fades the module's arm spawned this frame, each `(template, id)`.
    pub fades: &'static [(
        legaia_engine_battle_vm::battle_action::SummonFadeTemplate,
        i16,
    )],
    /// The arm killed the module's earlier fades first.
    pub kills_fades: bool,
}

// --- the fourteen trampoline arms ---
/// The clip PROT 0941's Steal stages on its caster in arm `0`.
///
/// Retail picks it with `FUN_80050E2C(record + 0x4C, 1, record[+0x4A])` - a
/// draw over the monster record's own clip list, which the engine has no
/// equivalent for. `1` is the approach clip every other body in the band
/// stages, and the arm's own contribution - the **OR** restage `+0x1DC |= 1`
/// rather than the band's usual bump - is what the port carries exactly.
const CAST_STEAL_RUN_CLIP: u8 = 1;

mod code_run;
mod seat_map;

#[cfg(test)]
mod capture_hold_tests {
    use super::*;
    use legaia_engine_mode::mode::SceneMode;

    fn band_world() -> World {
        let mut world = World {
            party: legaia_engine_menus::party_state::PartyState {
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
        use legaia_engine_cast::cast_module_ticks::{MYSTIC_CIRCLE_TICK, sweep_damage_shape_for};
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
        // PROT 0926 (id `0x98`) is the band's null stub: no tick arm at all.
        // (This test used to key on PROT 0909 and then 0914, whose chains are
        // now carried - keying on either would make the assertion vacuous.)
        assert_eq!(world.cast_module_for(0x98), Some(926));
        let run = world.run_cast_module_code(0x98, 0).unwrap();
        assert!(run.busy, "the seeded value on its own says 'busy'");
        assert!(!run.tick_ported, "but no tick body ran");

        world.arm_capture_cast_module(0x98);
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
    use legaia_engine_battle::spells::{SpellDef, SpellTarget};
    use legaia_engine_battle_vm::battle_target_group::{TARGET_GROUP_ENEMIES, TARGET_GROUP_PARTY};

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

#[cfg(test)]
mod divide_split_tests {
    use super::*;

    fn split() -> legaia_engine_cast::cast_arm_ticks::GlareDivideSplit {
        legaia_engine_cast::cast_arm_ticks::GlareDivideSplit {
            // Retail pool space: the second monster of any party.
            clone_seat: legaia_engine_cast::cast_module_ticks::FIRST_MONSTER_SEAT + 1,
            clone_hp: 69,
            saved_caster_target: 0,
            weakened: None,
        }
    }

    /// Retail's pool slot `3 + k` is the engine's `party_count + k`; an empty
    /// small-party seat has no engine slot. Element Change lands on the first
    /// monster whatever the party size.
    #[test]
    fn retail_pool_seats_map_onto_the_compacted_row() {
        let mut world = World::default();
        world.enter_battle(1, 2);
        assert_eq!(world.engine_slot_for_retail_pool(0), Some(0));
        assert_eq!(world.engine_slot_for_retail_pool(1), None);
        assert_eq!(world.engine_slot_for_retail_pool(3), Some(1));
        assert_eq!(world.engine_slot_for_retail_pool(4), Some(2));
        world.actors[1].battle_monster_id = Some(7);
        world.apply_cast_element_change(5);
        assert_eq!(world.actors[1].battle_element, Some(5));
        let mut world = World::default();
        world.enter_battle(3, 2);
        for r in 0..8 {
            assert_eq!(
                world.engine_slot_for_retail_pool(r),
                Some(r),
                "identity at 3"
            );
        }
    }

    /// A lone party member's monster row starts at engine slot 1, so a
    /// Divide clone takes slot 2 - inside the five-seat row the target picker
    /// walks - and each later clone the next seat, up to retail's five.
    #[test]
    fn a_small_partys_divide_clone_lands_in_the_engine_row() {
        let mut world = World::default();
        world.enter_battle(1, 1);
        world.actors[1].battle_monster_id = Some(7);
        world.actors[1].battle.max_hp = 69;
        world.actors[1].battle.hp = 69;
        assert!(world.apply_glare_divide_split(1, &split()));
        assert_eq!(world.actors[2].battle_monster_id, Some(7));
        assert_eq!(world.actors[2].battle.hp, 69);
        let (_, monsters) = world.battle_target_rows();
        assert!(monsters[1].alive, "the clone is a targetable enemy seat");
        for seat in 3..6 {
            assert!(world.apply_glare_divide_split(1, &split()));
            assert_eq!(world.actors[seat].battle_monster_id, Some(7));
        }
        assert!(
            !world.apply_glare_divide_split(1, &split()),
            "a sixth monster has no seat"
        );
    }
}
