//! Summon / cast-module / move-FX scene-graph state: the active summon scene, the cast stager and phase bytes, and the move-effect spawns and trails.
//!
//! Split out of the composite [`World`] so the state one subsystem owns
//! reads as one unit. Fields keep their retail provenance notes.

use super::*;

/// Summon / cast-module / move-FX scene-graph state: the active summon scene, the cast stager and phase bytes, and the move-effect spawns and trails.
pub struct CastFxState {
    /// Active Seru-magic summon scene-graph, while one is playing. Spawned off
    /// the battle-action cast band (or [`crate::world::World::spawn_summon`] for the debug
    /// path); ticked each frame through the move VM by [`crate::world::World::tick_summon`]
    /// and drained when every part finishes. Rendered via
    /// [`crate::world::World::active_summon_part_draws`].
    pub active_summon: Option<crate::summon::SummonScene>,
    /// Production cast-band request: a player Seru-magic cast (spell id
    /// `0x81..=0x8b`) sets `(spell_id, target world pos)` here - the engine
    /// equivalent of the retail cast band resolving the per-summon overlay
    /// (`FUN_8003EC70(id-0x79)`). The host (which has the PROT index) drains it
    /// via [`crate::world::World::take_pending_summon_spawn`], loads the summon overlay
    /// (extraction `PROT 903 + (id - 0x81)`), and calls [`crate::world::World::spawn_summon`]. Kept as a
    /// host-fulfilled request because `World` is index-agnostic (same pattern
    /// as the capture-archive load).
    pub pending_summon_spawn: Option<(u8, [i16; 3])>,
    /// The **cast-effect pool**: the DATA half of the slot-B cast-module band
    /// (PROT 0903..0966), keyed by PROT entry. Both of PROT 0898's tick
    /// dispatchers resolve a cast into this band
    /// ([`legaia_engine_vm::battle_cast_dispatch`]), and
    /// [`crate::world::World::spawn_cast_module_fx`] stages the resolved module's spawn
    /// records. Installed once per host by the scene host (which holds the
    /// PROT index); `None` on a disc-free host, where every cast simply stages
    /// no module records.
    pub effect_pool: Option<Arc<legaia_asset::cast_effect_pool::CastEffectPool>>,
    /// The engine's player-summon stager while a Seru cast is in the action
    /// SM's summon band - the body behind `BattleActionHost::summon_stager_tick`
    /// (see `crate::world::battle::cast_band`).
    pub summon_stager: Option<SummonStager>,
    /// Actor slot a host seated the summon creature at
    /// ([`crate::world::World::seat_summon_actor`]); `None` while no creature is out.
    pub summon_actor_slot: Option<u8>,
    /// A cast the action SM is carrying whose outcome is still owed - folded
    /// once, at retail's seam ([`crate::world::World::settle_cast_band`] / the stager's
    /// strike).
    pub pending_cast: Option<PendingCast>,
    /// The resident slot-B module's **phase byte** (`ctx+0x279`) - the second
    /// phase space riding under battle phase `0x70`, driven by the module
    /// code kernels ([`legaia_engine_vm::cast_module_ticks`]) from
    /// [`crate::world::World::run_cast_module_code`]. Zeroed when a cast is armed, exactly
    /// as retail's `0x801E4B1C` does.
    pub module_phase: u8,
    /// The resident slot-B module's `ctx+0x278` scratch byte, written by
    /// three of the band's stagers.
    pub module_ctx_278: u8,
    /// PROT 0904's ring-sweep angle, retail's `ctx+0x6D8`.
    ///
    /// Arm 12 grows it by the frame delta times `8` every tick
    /// (`0x801F7ADC..0x801F7AF0`) and gates each seat on lying inside a
    /// `+-0x30` cone about it, so it is a **rotating ray**, not a radius: the
    /// arm advances once it has passed `0x1000`, a full 12-bit turn. Reset
    /// when a cast is armed.
    pub module_ring_angle: u16,
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
    pub module_nighto_outcome: Option<legaia_engine_vm::cast_seru_ticks_a::NightoOutcome>,
    // --- W1-D: the fourteen trampoline arms ---
    /// The `+0x1DD` value PROT 0940's split arm displaced off the caster,
    /// which the module keeps in its own image word `0x801F8658` until the
    /// `0xFF` arm puts it back
    /// ([`legaia_engine_vm::cast_arm_ticks::glare_divide_split_tick`]).
    /// `None` outside that choreography.
    pub module_split_saved_target: Option<u8>,
    // --- end W1-D ---
    /// The action id whose **capture-band** module is resident, i.e. the one
    /// battle phase `0x70` re-enters every frame through `FUN_801F2160`.
    ///
    /// Armed at the pager seam (`BattleActionHost::load_capture_archive`,
    /// retail's `0x6E` arm) and cleared when the band's hold ends. `None`
    /// means no capture module is paged in and
    /// [`crate::world::World::capture_stager_tick`] reports "not busy" - which is what a
    /// disc-free host, or any cast that is not capture-class, sees.
    pub capture_spell: Option<u8>,
    /// Production battle-FX request for a **non-summon** move: a spell cast or
    /// enemy special whose move-power record carries a spawnable effect list
    /// sets `(move_id, target world pos)` here (see [`crate::world::World::request_move_fx_spawn`]).
    /// The host drains it via [`crate::world::World::take_pending_move_fx_spawn`] and calls
    /// [`crate::world::World::spawn_move_fx`] (which reads the retained PROT 0898 overlay).
    /// The sibling of [`pending_summon_spawn`](crate::world::CastFxState::pending_summon_spawn) for
    /// the move-FX (rather than summon-creature) render path.
    pub pending_move_fx_spawn: Option<(u8, [i16; 3])>,
    /// Active battle move-power effect-FX scene-graph, while one is playing. A
    /// move's `0x01..=0x63` on-contact / launch effect-list entries spawn the
    /// `0x801f6324` prototype records (summon-format move-VM parts) through the
    /// same machinery as a summon - [`crate::world::World::spawn_move_fx`] seeds it,
    /// [`crate::world::World::tick_move_fx`] advances it, [`crate::world::World::active_move_fx_part_draws`]
    /// renders it. Separate from [`active_summon`](crate::world::CastFxState::active_summon) so a
    /// move's FX and a summon don't clobber each other.
    pub active_move_fx: Option<crate::summon::SummonScene>,
    /// Live battle **effect-script** table-form scenes - one small
    /// `0x801F6324`-prototype scene-graph per table-form record the per-actor
    /// effect-script walk spawned ([`crate::world::World::spawn_action_table_effect`],
    /// retail `FUN_801DEA50` -> `FUN_80050ED4`). Retail allocates these from
    /// the same `0x60`-slot effect-actor pool as everything else; the engine
    /// keeps them in their own small list (capped) so concurrent records
    /// don't clobber [`crate::world::CastFxState::active_move_fx`]. Ticked by
    /// [`crate::world::World::tick_move_fx`]; drawn through
    /// [`crate::world::World::active_move_fx_part_draws`].
    pub active_action_fx: Vec<crate::summon::SummonScene>,
    /// The trail / afterimage GP0 texpage word (`0x7700 + id`) for the active
    /// move-FX scene, set by [`crate::world::World::spawn_move_fx`] from the move record's
    /// `+0x0b` field and cleared when the scene drains. Surfaced via
    /// [`crate::world::CastFxState::move_fx_trail_texpage`] for the render layer's streak
    /// pass - the trail id this carries is what
    /// `legaia_engine_render::afterimage::build_afterimage_quad` (the ported
    /// `FUN_801e1ab0`) turns into the jittered semi-transparent quad.
    pub move_fx_trail_texpage: Option<u16>,
    /// The battle context's move-FX projection block, installed by the action
    /// effect script's terminator (`ctx[+0x1014]` / `+0x6C6` / `+0x24E` /
    /// `+0x1144`; see [`crate::action_effect_script::MoveFxStreak`]). Read by
    /// [`crate::world::CastFxState::move_fx_streak`]; the render layer projects the afterimage
    /// streak's billboard from its launch point + half-width.
    pub move_fx_streak: crate::action_effect_script::MoveFxStreak,
    /// Pending move-FX sound cue id (`+0x0d`), set by [`crate::world::World::spawn_move_fx`]
    /// when the move carries a non-zero cue. The host drains it via
    /// [`crate::world::World::take_pending_move_fx_cue`] and routes it through
    /// `legaia_engine_audio::classify_cue` → the SFX ring / voice trigger
    /// (the retail `FUN_8004fcc8` dispatch). Same host-fulfilled-request shape
    /// as [`pending_summon_spawn`](crate::world::CastFxState::pending_summon_spawn).
    pub pending_move_fx_cue: Option<u8>,
}

impl CastFxState {
    pub fn new() -> Self {
        Self {
            active_summon: None,
            pending_summon_spawn: None,
            effect_pool: None,
            summon_stager: None,
            summon_actor_slot: None,
            pending_cast: None,
            module_phase: 0,
            module_ctx_278: 0,
            module_ring_angle: 0,
            module_nighto_outcome: None,
            // --- W1-D ---
            module_split_saved_target: None,
            // --- end W1-D ---
            capture_spell: None,
            pending_move_fx_spawn: None,
            active_move_fx: None,
            active_action_fx: Vec::new(),
            move_fx_trail_texpage: None,
            move_fx_streak: Default::default(),
            pending_move_fx_cue: None,
        }
    }
}

impl Default for CastFxState {
    fn default() -> Self {
        Self::new()
    }
}
