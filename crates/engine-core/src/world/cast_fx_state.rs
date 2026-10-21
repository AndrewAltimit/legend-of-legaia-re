//! Summon / cast-module / move-FX scene-graph state: the active summon scene, the cast stager and phase bytes, and the move-effect spawns and trails.
//!
//! One subsystem's state, held as a unit on the composite [`World`].
//! Fields keep their retail provenance notes.

use super::*;

/// Summon / cast-module / move-FX scene-graph state: the active summon scene, the cast stager and phase bytes, and the move-effect spawns and trails.
pub struct CastFxState {
    /// Active Seru-magic summon scene-graph, while one is playing. Spawned off
    /// the battle-action cast band (or [`crate::world::World::spawn_summon`] for the debug
    /// path); ticked each frame through the move VM by [`crate::world::World::tick_summon`]
    /// and drained when every part finishes. Rendered via
    /// [`crate::world::World::active_summon_part_draws`].
    pub active_summon: Option<legaia_engine_effects::summon::SummonScene>,
    /// Production cast-band request: a player Seru-magic cast (spell id
    /// `0x81..=0x8b`) sets `(spell_id, target world pos)` here - the engine
    /// equivalent of the retail cast band resolving the per-summon overlay
    /// (`FUN_8003EC70(id-0x79)`). The host (which has the PROT index) drains it
    /// via [`crate::world::World::take_pending_summon_spawn`], loads the summon overlay
    /// (extraction `PROT 903 + (id - 0x81)`), and calls [`crate::world::World::spawn_summon`]. Kept as a
    /// host-fulfilled request because `World` is index-agnostic (same pattern
    /// as the capture-archive load).
    pub(crate) pending_summon_spawn: Option<(u8, [i16; 3])>,
    /// A creature seat the stager owes but has not requested yet: `(seat
    /// arm, spell id, spawn point)`. A module whose tick body seats its
    /// creature in a later arm (`cast_module_camera::module_seat_arm`) has
    /// it requested once its phase passes that arm.
    pub(crate) summon_seat_owed: Option<(u8, u8, [i16; 3])>,
    /// The **cast-effect pool**: the DATA half of the slot-B cast-module band
    /// (PROT 0903..0966), keyed by PROT entry. Both of PROT 0898's tick
    /// dispatchers resolve a cast into this band
    /// ([`legaia_engine_battle_vm::battle_cast_dispatch`]), and
    /// [`crate::world::World::spawn_cast_module_fx`] stages the resolved module's spawn
    /// records. Installed once per host by the scene host (which holds the
    /// PROT index); `None` on a disc-free host, where every cast simply stages
    /// no module records.
    pub effect_pool: Option<Arc<legaia_overlay_images::cast_effect_pool::CastEffectPool>>,
    /// The engine's player-summon stager while a Seru cast is in the action
    /// SM's summon band - the body behind `BattleActionHost::summon_stager_tick`
    /// (see `crate::world::battle::cast_band`).
    pub summon_stager: Option<SummonStager>,
    /// Actor slot a host seated the summon creature at
    /// ([`crate::world::World::seat_summon_actor`]); `None` while no creature is out.
    pub summon_actor_slot: Option<u8>,
    /// The seated creature's render-node size `+0x58`: its `summon.dat`
    /// actor record's byte `+0x1F << 5` (`lbu v1,0x1f(v1)` / `sll v1,v1,0x5`
    /// / `sh v1,0x58(v0)` in the seat installer `FUN_801F19EC`,
    /// `0x801F1DE4..0x801F1DF0`). Set by the host's seat; `None` while no
    /// creature record is known.
    pub summon_body_radius: Option<i16>,
    /// A cast the action SM is carrying whose outcome is still owed - folded
    /// once, at retail's seam ([`crate::world::World::settle_cast_band`] / the stager's
    /// strike).
    pub pending_cast: Option<PendingCast>,
    /// The resident slot-B module's **phase byte** (`ctx+0x279`) - the second
    /// phase space riding under battle phase `0x70`, driven by the module
    /// code kernels ([`legaia_engine_cast::cast_module_ticks`]) from
    /// [`crate::world::World::run_cast_module_code`]. Zeroed when a cast is armed, exactly
    /// as retail's `0x801E4B1C` does.
    pub module_phase: u8,
    /// The resident slot-B module's `ctx+0x278` scratch byte, written by
    /// three of the band's stagers.
    pub module_ctx_278: u8,
    /// The capture body took a branch with no damage site (PROT 0953's
    /// charge): the band's fold owes the cast nothing.
    pub module_skips_fold: bool,
    /// PROT 0954's module-resident words while its body runs
    /// ([`legaia_engine_cast::cast_fatal_decision`]).
    pub(crate) fatal_decision: Option<legaia_engine_cast::cast_fatal_decision::FatalDecisionState>,
    /// The outcome name PROT 0954's arm 9 put up in the move-name label
    /// (HUD element `0x4C`), until the band leaves. Read by
    /// `legaia_engine_battle_hud::battle_move_name`.
    pub fatal_banner: Option<String>,
    /// PROT 0904's ring-sweep angle, retail's `ctx+0x6D8`.
    ///
    /// Arm 12 grows it by the frame delta times `8` every tick
    /// (`0x801F7ADC..0x801F7AF0`) and gates each seat on lying inside a
    /// `+-0x30` cone about it, so it is a **rotating ray**, not a radius: the
    /// arm advances once it has passed `0x1000`, a full 12-bit turn. Reset
    /// when a cast is armed.
    pub(crate) module_ring_angle: u16,
    /// The band word `ctx[+0x6D8]` the action SM handed the stager call now
    /// running (`World::summon_band_word_in`), after its own drain; `None`
    /// outside a `0x34` / `0x35` stager call. A director's band-timer gate
    /// reads this rather than the context's pre-step copy.
    pub(crate) band_word_live: Option<i16>,
    /// Vsyncs left before the side-band applier drops its stage byte
    /// `ctx[+0x276]` (`BattleActionCtx::menu_open`) after the summon
    /// invoke `0x32` seeds it; `None` while no seed is being read
    /// ([`legaia_engine_battle_vm::battle_action::sideband_token_vsyncs`]).
    pub(crate) sideband_read_left: Option<u16>,
    /// PROT 0948's beam counter, retail's module word `0x801F8858`: arm 2
    /// zeroes it as it passes and the beam builder `FUN_801F726C` adds
    /// `step * 2` on every call.
    pub(crate) module_beam_counter: i32,
    /// The beam builder ran on this module tick (arm 3 calls it every tick
    /// it holds), so the frame draws its packets
    /// ([`crate::world::World::cross_beam_draw`]).
    pub(crate) module_beam_live: bool,
    /// The Spikefish flute's flee, staged by PROT 0925's outcome arm from
    /// inside the action SM's stager call and applied to the action
    /// context after the step's write-back ([`crate::world::World::step_battle`]):
    /// retail's arm stores `ctx[7] = 0x65` and `ctx[+0x6D8] = 0x3C` itself.
    pub(crate) module_flee: bool,
    /// PROT 0904's draw state: the beam trail ring and the packets its last
    /// tick drew ([`crate::world::World::theeder_draw`]). Reset when a cast
    /// is armed.
    pub module_theeder: legaia_engine_cast::seru_magic_ticks::TheederFx,
    /// PROT 0912's crystal turn word `0x801F92AC`: zeroed on arm 3's pass,
    /// stepped down on each held pass of arm 4
    /// ([`legaia_engine_cast::cast_module_camera::freed_crystal_corners`]).
    pub(crate) module_freed_turn: i32,
    /// The monster poses PROT 0927's arm 0 saves (`0x801F9C9C` / `0x801F9CBC`)
    /// and arms 27 / 28 restore: `(engine slot, position, facing)`.
    pub(crate) module_saved_poses: Vec<(usize, [i16; 3], u16)>,
    /// The `+0x42` the summon creature's render node carries while it draws
    /// clipped (`0` = the floor row, `+0x42 = 1`): PROT 0927 raises `2`, so
    /// its creature is clipped by object-effect row 1.
    pub(crate) module_creature_clip_kind: u16,
    /// PROT 0927's camera-shake word `0x801F9C90`: raised by its stager
    /// arms 3 / 4, counted down by the tick's epilogue.
    pub(crate) module_shake: i32,
    /// The phase-chain arm whose damage site has landed this pass through
    /// it: the band folds a chain body's hit on the tick its arm first runs
    /// (`World::run_cast_module_code`), so a settle wait after it sees the
    /// victim react.
    pub(crate) module_hit_arm: Option<u8>,
    /// Ticks the current arm has held on its settle wait
    /// (`cast_module_ticks::ChainSettle`), bounded by
    /// `cast_fatal_decision::SETTLE_TICK_LIMIT`.
    pub(crate) module_settle_ticks: u16,
    /// PROT 0907 (Nighto)'s kill / confuse / resist verdict for the resident
    /// cast, decided **once**.
    ///
    /// Retail draws both rolls in arm `0` and parks them in the module's own
    /// words `0x801F8534` / `0x801F853C` (`0x801F6B50` / `0x801F6C28`), so the
    /// outcome is settled the frame the cast starts and arm 13 only reads it.
    /// The port keeps that shape: the band rolls on the first tick of a `0x85`
    /// cast and holds the verdict here for every later frame, instead of
    /// re-rolling per frame (which would make a resist flicker into a kill).
    /// `None` while no Nighto cast is resident.
    pub(crate) module_nighto_outcome: Option<legaia_engine_cast::seru_magic_ticks::NightoOutcome>,
    /// The move VM's op-`0x20` calls a resident module's parts made since
    /// the module's last tick - `(*gp[0x714])(actor, op[1], op[2])`, the
    /// module's own `0x801F6734` stager row - as `(arm, what the stager reads
    /// off the calling part)`. The band drains them into the module's stager
    /// (`World::run_cast_module_code`); PROT 0909's phase rendezvous are two
    /// of them.
    pub(crate) module_stager_calls: Vec<(u8, legaia_engine_cast::cast_module_ticks::StagerCaller)>,
    /// PROT 0917's stager counter word `0x801F9364`
    /// ([`legaia_engine_cast::cast_module_ticks::BARRA_STAGER_COUNTER`]),
    /// `0` in the image the band pages in for each cast.
    pub(crate) module_barra_counter: u16,
    /// The cast module whose code ran last (`World::run_cast_module_code`),
    /// whose stager an op-`0x20` call reaches; `None` until a cast's first
    /// tick.
    pub(crate) module_running_entry: Option<u32>,
    /// PROT 0925's stager words `0x801F8014` / `0x801F8018`
    /// ([`legaia_engine_cast::cast_module_ticks::SpikefishStagerWords`]).
    pub(crate) module_spikefish_words: legaia_engine_cast::cast_module_ticks::SpikefishStagerWords,
    /// PROT 0934's stager word `0x801FA410`
    /// ([`legaia_engine_cast::cast_module_ticks::OZMA_STAGER_WORD`]).
    pub(crate) module_ozma_word: i32,
    /// Seats a stager arm run inside its caller's op-`0x20` step decided
    /// (PROT 0925's, which writes into its caller), for the band to seat on
    /// the module's next tick.
    pub(crate) module_stager_seats: Vec<legaia_engine_cast::cast_module_ticks::StagerSpawn>,
    /// The clocks of PROT 0909's stager-calling records the host did not
    /// seat, as `(record, stager arm, frames left)`
    /// ([`legaia_engine_cast::cast_module_camera::VIGURO_STAGER_CALLERS`]).
    /// Armed on the pass whose arm spawns the record and dropped when the
    /// record is seated as a part, which then makes the call itself.
    pub(crate) module_record_clocks: Vec<(u32, u8, u16)>,
    // --- the fourteen trampoline arms ---
    /// The `+0x1DD` value PROT 0940's split arm displaced off the caster,
    /// which the module keeps in its own image word `0x801F8658` until the
    /// `0xFF` arm puts it back
    /// ([`legaia_engine_cast::cast_arm_ticks::glare_divide_split_tick`]).
    /// `None` outside that choreography.
    pub(crate) module_split_saved_target: Option<u8>,
    /// PROT 0910 (Swordie)'s per-slash progress words, hit counter, settle
    /// countdown and `ctx[+0x6D8]` arm timer
    /// ([`legaia_engine_cast::seru_magic_ticks::SwordieSlashes`]) - the module
    /// image's own words in retail, so a fresh cast starts them at zero.
    pub module_swordie: legaia_engine_cast::seru_magic_ticks::SwordieSlashes,
    /// PROT 0920 (Slippery)'s Deadly Rain: its records, budget and colour
    /// words and the streaks its last call drew ([`ModuleRain`]).
    pub module_rain: ModuleRain,
    /// PROT 0911 (Orb)'s module words the port keeps ([`ModuleOrb`]).
    pub module_orb: ModuleOrb,
    /// The settle countdown word PROT 0909 / 0911 / 0912 / 0913 arm after
    /// their hit and drain before they write `0xFF`
    /// (`legaia_engine_cast::seru_magic_ticks::SETTLE_DRAIN_PER_TICK`).
    pub module_settle_countdown: i32,
    /// The resident player-Seru module's camera-arm state: its countdown
    /// word and the creature seat as the module placed it
    /// ([`legaia_engine_cast::cast_module_camera`]). Module-image words in
    /// retail, so a fresh cast starts them at zero.
    pub module_cam: legaia_engine_cast::cast_module_camera::ModuleCamState,
    /// The resident module's presentation-hook state: the colour-wash block
    /// `ctx[+0x27A..]`, the armed wash its last tick armed, and which arm's
    /// once-per-arm calls have fired
    /// (`crate::world::battle::cast_band::presentation`). Reset when a cast
    /// is armed.
    pub module_pres: crate::world::battle::ModulePresentation,
    /// The action id whose **capture-band** module is resident, i.e. the one
    /// battle phase `0x70` re-enters every frame through `FUN_801F2160`.
    ///
    /// Armed at the pager seam (`BattleActionHost::load_capture_archive`,
    /// retail's `0x6E` arm) and cleared when the band's hold ends. `None`
    /// means no capture module is paged in and
    /// [`crate::world::World::capture_stager_tick`] reports "not busy" - which is what a
    /// disc-free host, or any cast that is not capture-class, sees.
    pub(crate) capture_spell: Option<u8>,
    /// The caster's point the pager read for a capture module whose records
    /// go in whole, seated on the band's first `0x70` tick
    /// ([`crate::world::World::capture_stager_tick`]): the module's arms
    /// spawn them, and the first arm runs on the first `0x70` frame, never
    /// through `0x6E` / `0x6F`.
    pub(crate) capture_fx_pending: Option<[i16; 3]>,
    /// A capture body's own pass counter (PROT 0940's `0x801F8650`): bumped
    /// once a pass by the arms that spawn on it
    /// ([`legaia_engine_cast::cast_module_camera::capture_arm_counted`]).
    pub(crate) module_pass_counter: u32,
    /// On the per-vsync cadence: the module's last tick handed its phase
    /// over, and the next arm waits for the tick that opens the next battle
    /// frame ([`crate::world::World::module_pass`]).
    pub(crate) module_handover_waits: bool,
    /// The caster's clip stages the resident capture-class body owes and its
    /// port does not write ([`legaia_engine_cast::cast_module_ticks::CAPTURE_CASTER_STAGES`]),
    /// replayed at the head of the band by
    /// [`crate::world::World::capture_stager_tick`]. `None` once played out,
    /// or for a body that stages nothing on its caster.
    pub(crate) caster_stages: Option<crate::world::battle::CasterStageRun>,
    /// Production battle-FX request for a **non-summon** move: a spell cast or
    /// enemy special whose move-power record carries a spawnable effect list
    /// sets `(move_id, target world pos)` here (see [`crate::world::World::request_move_fx_spawn`]).
    /// The host drains it via [`crate::world::World::take_pending_move_fx_spawn`] and calls
    /// [`crate::world::World::spawn_move_fx`] (which reads the retained PROT 0898 overlay).
    /// The sibling of [`pending_summon_spawn`](crate::world::CastFxState::pending_summon_spawn) for
    /// the move-FX (rather than summon-creature) render path.
    pub(crate) pending_move_fx_spawn: Option<(u8, [i16; 3])>,
    /// Active battle move-power effect-FX scene-graph, while one is playing. A
    /// move's `0x01..=0x63` on-contact / launch effect-list entries spawn the
    /// `0x801f6324` prototype records (summon-format move-VM parts) through the
    /// same machinery as a summon - [`crate::world::World::spawn_move_fx`] seeds it,
    /// [`crate::world::World::tick_move_fx`] advances it, [`crate::world::World::active_move_fx_part_draws`]
    /// renders it. Separate from [`active_summon`](crate::world::CastFxState::active_summon) so a
    /// move's FX and a summon don't clobber each other.
    pub active_move_fx: Option<legaia_engine_effects::summon::SummonScene>,
    /// Live battle **effect-script** table-form scenes - one small
    /// `0x801F6324`-prototype scene-graph per table-form record the per-actor
    /// effect-script walk spawned ([`crate::world::World::spawn_action_table_effect`],
    /// retail `FUN_801DEA50` -> `FUN_80050ED4`). Retail allocates these from
    /// the same `0x60`-slot effect-actor pool as everything else; the engine
    /// keeps them in their own small list (capped) so concurrent records
    /// don't clobber [`crate::world::CastFxState::active_move_fx`]. Ticked by
    /// [`crate::world::World::tick_move_fx`]; drawn through
    /// [`crate::world::World::active_move_fx_part_draws`].
    pub active_action_fx: Vec<legaia_engine_effects::summon::SummonScene>,
    /// Cort's **Mystic Shield** effect: the sphere `0x80078644` (part `0`,
    /// the handle retail keeps in `_DAT_8007BD84`) and its texture scroller
    /// `0x80078720` (part `1`), both SCUS-resident records PROT 0940's arm 4
    /// seats on the first monster seat. It outlives the cast that raised it
    /// (the sphere's program loops), so it has a seat of its own rather than
    /// riding [`Self::active_summon`], which the next action retires. Posed
    /// every frame and broken by [`crate::world::World::tick_mystic_shield`].
    pub mystic_shield_fx: Option<legaia_engine_effects::summon::SummonScene>,
    /// The trail / afterimage GP0 texpage word (`0x7700 + id`) for the active
    /// move-FX scene, set by [`crate::world::World::spawn_move_fx`] from the move record's
    /// `+0x0b` field and cleared when the scene drains. Surfaced via
    /// [`crate::world::CastFxState::move_fx_trail_texpage`] for the render layer's streak
    /// pass - the trail id this carries is what
    /// `legaia_render_kernels::afterimage::build_afterimage_quad` (the ported
    /// `FUN_801e1ab0`) turns into the jittered semi-transparent quad.
    pub move_fx_trail_texpage: Option<u16>,
    /// The battle context's move-FX projection block, installed by the action
    /// effect script's terminator (`ctx[+0x1014]` / `+0x6C6` / `+0x24E` /
    /// `+0x1144`; see [`legaia_engine_effects::action_effect_script::MoveFxStreak`]). Read by
    /// [`crate::world::CastFxState::move_fx_streak`]; the render layer projects the afterimage
    /// streak's billboard from its launch point + half-width.
    pub(crate) move_fx_streak: legaia_engine_effects::action_effect_script::MoveFxStreak,
    /// The four homing slots the same terminator seeds and
    /// `World::tick_homing_slots` flies (`ctx[+0x24E..]` / `+0x252..` /
    /// `+0x1144..`; [`legaia_engine_effects::action_effect_script::HomingSlots`]).
    pub(crate) homing: legaia_engine_effects::action_effect_script::HomingSlots,
    /// Set by a cast's fold when it leaves the move's effect lists to the
    /// homing flight its caster's terminator is about to seed; taken by that
    /// seed (`World::seed_homing_slots`).
    pub(crate) homing_takes_lists: bool,
    /// The caster whose homing flight took the move's effect lists ahead of
    /// its cast's fold - a terminator that seeded the flight before the fold
    /// ran (an enemy Tail Fire, `battle_gimard_tail_fire_b`). The fold reads
    /// it and leaves the lists to the flight; cleared on the next action
    /// seed (`0x0C`).
    pub(crate) homing_holds_lists: Option<u8>,
    /// The text a directed summon module's arm last put up
    /// ([`legaia_engine_cast::cast_module_camera::ModuleCaption`]), cleared at
    /// the band's `0x37` exit. Read by `legaia_engine_battle_hud::battle_move_name`.
    pub module_caption: Option<legaia_engine_cast::cast_module_camera::ModuleCaption>,
    /// Pending move-FX sound cue id (`+0x0d`), set by [`crate::world::World::spawn_move_fx`]
    /// when the move carries a non-zero cue. The host drains it via
    /// [`crate::world::World::take_pending_move_fx_cue`] and routes it through
    /// `legaia_engine_audio::classify_cue` → the SFX ring / voice trigger
    /// (the retail `FUN_8004fcc8` dispatch). Same host-fulfilled-request shape
    /// as [`pending_summon_spawn`](crate::world::CastFxState::pending_summon_spawn).
    pub(crate) pending_move_fx_cue: Option<u8>,
    /// The move-VM actors the effect spawner `FUN_801DFDF0` seats ahead of
    /// effect ids `4` / `0x13` (`0x801DFE60..0x801DFE88`): the trigger record
    /// VA (`0x801F5D90` / `0x801F5CF8`), the spawn position and the angle it
    /// hands `FUN_80050ED4` as `rot[1]`. Queued by the effect host and seated
    /// by [`crate::world::World::flush_battle_bursts`].
    pub(crate) pending_burst_triggers: Vec<(u32, [i16; 3], u16)>,
    /// Move-VM op `0x17` calls a part made this step - the battle-overlay
    /// escape `FUN_801F30C4(actor, mode)`, with the parent's position, rotation
    /// trio and `+0x72` scale - run by [`crate::world::World::flush_battle_bursts`].
    pub(crate) pending_bursts: Vec<PendingBurst>,
}

/// One queued move-VM op-`0x17` call ([`CastFxState::pending_bursts`]).
/// PROT 0920's (Slippery) **Deadly Rain** state: the module's own words in
/// retail, so a fresh cast starts them at zero.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleRain {
    /// The `0x400` rain records at `*(0x801F8448)`.
    pub drops: legaia_render_kernels::cast_rain::DeadlyRain,
    /// `_DAT_8007BDC0` - the spawn odds, and the draw tick's gate on the rain
    /// routine (`0` = not called).
    pub budget: i32,
    /// `0x801F8444` - the streak colour.
    pub colour: u32,
    /// What the routine's last call drew and spawned.
    pub frame: legaia_render_kernels::cast_rain::RainFrame,
}

/// PROT 0911 (Orb)'s words: the opening star swarm's per-star offsets
/// (`0x801F8560`, eight bytes a star) and the creature seat arm 5's pass
/// wrote, held for the tick its cut lands on. Module-image words in retail,
/// so a fresh cast starts them empty.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleOrb {
    /// Star `i`'s `(x, y, z)` offset, in seating order.
    pub star_offsets: Vec<[i16; 3]>,
    /// The creature's `(x, y, z)` after arm 5's step and lift, not yet on
    /// the actor.
    pub deferred_seat: Option<[i16; 3]>,
    /// Arm 2 passed and its stars are owed on the next tick.
    pub stars_owed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingBurst {
    pub mode: u32,
    pub pos: [i16; 3],
    pub rot: [i16; 3],
    pub scale: u16,
}

impl CastFxState {
    pub fn new() -> Self {
        Self {
            active_summon: None,
            pending_summon_spawn: None,
            summon_seat_owed: None,
            effect_pool: None,
            summon_stager: None,
            summon_actor_slot: None,
            summon_body_radius: None,
            pending_cast: None,
            module_phase: 0,
            module_ctx_278: 0,
            module_skips_fold: false,
            fatal_decision: None,
            fatal_banner: None,
            module_ring_angle: 0,
            band_word_live: None,
            sideband_read_left: None,
            module_beam_counter: 0,
            module_beam_live: false,
            module_flee: false,
            module_theeder: Default::default(),
            module_freed_turn: 0,
            module_saved_poses: Vec::new(),
            module_creature_clip_kind: 0,
            module_shake: 0,
            module_hit_arm: None,
            module_settle_ticks: 0,
            module_nighto_outcome: None,
            module_stager_calls: Vec::new(),
            module_barra_counter: 0,
            module_running_entry: None,
            module_spikefish_words: Default::default(),
            module_ozma_word: 0,
            module_stager_seats: Vec::new(),
            module_record_clocks: Vec::new(),
            module_split_saved_target: None,
            module_swordie: Default::default(),
            module_rain: Default::default(),
            module_orb: Default::default(),
            module_settle_countdown: 0,
            module_cam: Default::default(),
            module_pres: Default::default(),
            capture_spell: None,
            capture_fx_pending: None,
            module_pass_counter: 0,
            module_handover_waits: false,
            caster_stages: None,
            pending_move_fx_spawn: None,
            active_move_fx: None,
            active_action_fx: Vec::new(),
            mystic_shield_fx: None,
            move_fx_trail_texpage: None,
            move_fx_streak: Default::default(),
            homing: Default::default(),
            homing_takes_lists: false,
            homing_holds_lists: None,
            module_caption: None,
            pending_move_fx_cue: None,
            pending_burst_triggers: Vec::new(),
            pending_bursts: Vec::new(),
        }
    }
}

impl Default for CastFxState {
    fn default() -> Self {
        Self::new()
    }
}
