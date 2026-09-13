//! Summon / cast-module / move-FX scene-graph state: the active summon scene, the cast stager and phase bytes, and the move-effect spawns and trails.
//!
//! Split out of the composite [`World`] so the state one subsystem owns
//! reads as one unit. Fields keep their retail provenance notes.

use super::*;

/// Summon / cast-module / move-FX scene-graph state: the active summon scene, the cast stager and phase bytes, and the move-effect spawns and trails.
pub struct CastFxState {
    /// Active Seru-magic summon scene-graph, while one is playing. Spawned off
    /// the battle-action cast band (or [`World::spawn_summon`] for the debug
    /// path); ticked each frame through the move VM by [`World::tick_summon`]
    /// and drained when every part finishes. Rendered via
    /// [`World::active_summon_part_draws`].
    pub active_summon: Option<crate::summon::SummonScene>,
    /// Production cast-band request: a player Seru-magic cast (spell id
    /// `0x81..=0x8b`) sets `(spell_id, target world pos)` here - the engine
    /// equivalent of the retail cast band resolving the per-summon overlay
    /// (`FUN_8003EC70(id-0x79)`). The host (which has the PROT index) drains it
    /// via [`World::take_pending_summon_spawn`], loads the summon overlay
    /// (extraction `PROT 903 + (id - 0x81)`), and calls [`World::spawn_summon`]. Kept as a
    /// host-fulfilled request because `World` is index-agnostic (same pattern
    /// as the capture-archive load).
    pub pending_summon_spawn: Option<(u8, [i16; 3])>,
    /// The **cast-effect pool**: the DATA half of the slot-B cast-module band
    /// (PROT 0903..0966), keyed by PROT entry. Both of PROT 0898's tick
    /// dispatchers resolve a cast into this band
    /// ([`legaia_engine_vm::battle_cast_dispatch`]), and
    /// [`World::spawn_cast_module_fx`] stages the resolved module's spawn
    /// records. Installed once per host by the scene host (which holds the
    /// PROT index); `None` on a disc-free host, where every cast simply stages
    /// no module records.
    pub effect_pool: Option<Arc<legaia_asset::cast_effect_pool::CastEffectPool>>,
    /// The engine's player-summon stager while a Seru cast is in the action
    /// SM's summon band - the body behind `BattleActionHost::summon_stager_tick`
    /// (see `crate::world::battle::cast_band`).
    pub summon_stager: Option<SummonStager>,
    /// Actor slot a host seated the summon creature at
    /// ([`World::seat_summon_actor`]); `None` while no creature is out.
    pub summon_actor_slot: Option<u8>,
    /// A cast the action SM is carrying whose outcome is still owed - folded
    /// once, at retail's seam ([`World::settle_cast_band`] / the stager's
    /// strike).
    pub pending_cast: Option<PendingCast>,
    /// The resident slot-B module's **phase byte** (`ctx+0x279`) - the second
    /// phase space riding under battle phase `0x70`, driven by the module
    /// code kernels ([`legaia_engine_vm::cast_module_ticks`]) from
    /// [`World::run_cast_module_code`]. Zeroed when a cast is armed, exactly
    /// as retail's `0x801E4B1C` does.
    pub module_phase: u8,
    /// The resident slot-B module's `ctx+0x278` scratch byte, written by
    /// three of the band's stagers.
    pub module_ctx_278: u8,
    /// The action id whose **capture-band** module is resident, i.e. the one
    /// battle phase `0x70` re-enters every frame through `FUN_801F2160`.
    ///
    /// Armed at the pager seam (`BattleActionHost::load_capture_archive`,
    /// retail's `0x6E` arm) and cleared when the band's hold ends. `None`
    /// means no capture module is paged in and
    /// [`World::capture_stager_tick`] reports "not busy" - which is what a
    /// disc-free host, or any cast that is not capture-class, sees.
    pub capture_spell: Option<u8>,
    /// Production battle-FX request for a **non-summon** move: a spell cast or
    /// enemy special whose move-power record carries a spawnable effect list
    /// sets `(move_id, target world pos)` here (see [`World::request_move_fx_spawn`]).
    /// The host drains it via [`World::take_pending_move_fx_spawn`] and calls
    /// [`World::spawn_move_fx`] (which reads the retained PROT 0898 overlay).
    /// The sibling of [`pending_summon_spawn`](Self::pending_summon_spawn) for
    /// the move-FX (rather than summon-creature) render path.
    pub pending_move_fx_spawn: Option<(u8, [i16; 3])>,
    /// Active battle move-power effect-FX scene-graph, while one is playing. A
    /// move's `0x01..=0x63` on-contact / launch effect-list entries spawn the
    /// `0x801f6324` prototype records (summon-format move-VM parts) through the
    /// same machinery as a summon - [`World::spawn_move_fx`] seeds it,
    /// [`World::tick_move_fx`] advances it, [`World::active_move_fx_part_draws`]
    /// renders it. Separate from [`active_summon`](Self::active_summon) so a
    /// move's FX and a summon don't clobber each other.
    pub active_move_fx: Option<crate::summon::SummonScene>,
    /// Live battle **effect-script** table-form scenes - one small
    /// `0x801F6324`-prototype scene-graph per table-form record the per-actor
    /// effect-script walk spawned ([`World::spawn_action_table_effect`],
    /// retail `FUN_801DEA50` -> `FUN_80050ED4`). Retail allocates these from
    /// the same `0x60`-slot effect-actor pool as everything else; the engine
    /// keeps them in their own small list (capped) so concurrent records
    /// don't clobber [`crate::world::CastFxState::active_move_fx`]. Ticked by
    /// [`World::tick_move_fx`]; drawn through
    /// [`World::active_move_fx_part_draws`].
    pub active_action_fx: Vec<crate::summon::SummonScene>,
    /// The trail / afterimage GP0 texpage word (`0x7700 + id`) for the active
    /// move-FX scene, set by [`World::spawn_move_fx`] from the move record's
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
    /// Pending move-FX sound cue id (`+0x0d`), set by [`World::spawn_move_fx`]
    /// when the move carries a non-zero cue. The host drains it via
    /// [`World::take_pending_move_fx_cue`] and routes it through
    /// `legaia_engine_audio::classify_cue` → the SFX ring / voice trigger
    /// (the retail `FUN_8004fcc8` dispatch). Same host-fulfilled-request shape
    /// as [`pending_summon_spawn`](Self::pending_summon_spawn).
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
