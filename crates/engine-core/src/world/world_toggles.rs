//! Engine behaviour toggles: the live gameplay loop, VM-driven dialogue, damage finish, monster targeting, select-attack option, flashing reduction and the entry pulse gate.
//!
//! Split out of the composite [`World`] so the state one subsystem owns
//! reads as one unit. Fields keep their retail provenance notes.

/// Engine behaviour toggles: the live gameplay loop, VM-driven dialogue, damage finish, monster targeting, select-attack option, flashing reduction and the entry pulse gate.
pub struct WorldToggles {
    /// Scene-entry VDF pulse **enhancement** gate
    /// ([`crate::world::World::install_entry_vdf_pulse`]). On by default; clearing it
    /// keeps every never-retail-armed morph pack (jou's flesh ground)
    /// static at plain entry, exactly as retail draws it. Retail-armed
    /// scenes are unaffected either way - the installer stands aside for
    /// them regardless.
    pub entry_pulse_enabled: bool,
    /// Photosensitivity guard over the ambient CLUT-cell cyclers (see
    /// [`crate::options::OptionsState::reduce_flashing`]). When `true`
    /// (the default - a host that never plumbs options stays safe),
    /// [`crate::world::World::step_ambient_fx`] slew-limits the **applied** luminance
    /// channels (`v_add`, `white`) toward each cell's simulated target
    /// instead of jumping, so full-swing per-tick strobes (koin3's dance
    /// floor) become sub-hazard-rate pulses. Hue / saturation sweeps pass
    /// through untouched. The move-VM state itself always advances
    /// retail-exact - this only shapes the VRAM presentation.
    pub(crate) reduce_flashing: bool,
    /// Retail's **visible-tile crop** on the field ground + decoration layers
    /// ([`crate::field_view_window::field_view_cells`]), from
    /// [`crate::options::OptionsState::retail_view_window`]. Default on; it
    /// only takes effect at retail framing.
    pub(crate) view_window_crop: bool,
    /// The volumetric ground-fog **enhancement**
    /// ([`crate::fog_volume`], stepped by `World::tick`), from
    /// [`crate::options::OptionsState::volumetric_fog`]. Default **off** here
    /// so headless hosts (replays, oracles) carry no enhancement; the play
    /// hosts push their option (default on) through
    /// [`crate::options::OptionsState::apply_to_world`]. Off resets the bank,
    /// so every host draws exactly what it drew without the feature.
    pub volumetric_fog: bool,
    // --- live gameplay loop (Field <-> Battle round trip) -----------------
    /// Master opt-in for the **field side** of the in-`tick` Field <-> Battle
    /// round trip: the step-driven random-encounter roll.
    ///
    /// When `false` the Field branch of [`crate::world::World::tick`] runs the field VM +
    /// locomotion but never rolls an encounter. When `true` it also drives
    /// [`crate::world::World::live_field_tick`] - per-step roll, transition countdown, and
    /// the automatic `Field -> Battle` flip resolving a real formation.
    ///
    /// The **battle side is not gated by this flag.** Once the world is in
    /// [`crate::world::SceneMode::Battle`] - however it got there: this roll, a field
    /// carrier's scripted `3E FF` fight, a world-map region encounter, or a
    /// direct [`crate::world::World::enter_battle`] - [`crate::world::World::tick`] always drives
    /// [`crate::world::World::live_battle_tick`], because a battle that cannot resolve is a
    /// soft-lock. Retail has no "loop enabled" concept either
    /// (`FUN_801E295C`). Hosts that want a driven-battle-only slice can
    /// simply leave this flag off and enter battle themselves.
    ///
    // REF: FUN_801E295C (the retail action SM, which has no such gate)
    pub live_gameplay_loop: bool,
    /// Opt-in, NON-FAITHFUL gameplay tweak: when a monster picks a single
    /// living party member to attack, override the (faithful, random) choice
    /// with the lowest-HP living member. Off by default - the retail behaviour
    /// is a uniform random target. The faithful random target is still rolled
    /// in full (identical RNG-call count + stream); only the final single
    /// party slot is replaced, so a replay stays internally deterministic and
    /// all downstream battle RNG is unaffected. All-party / monster-band / self
    /// targets are never touched.
    pub smarter_monster_targeting: bool,
    /// Opt-in: route field NPC dialogue through the inline-script field-VM
    /// runner ([`crate::world::World::drive_inline_dialogue`]) instead of the simplified
    /// `current_dialog` / `OwnedDialogPanel` path, so dialogue branch handlers
    /// actually execute (story-flag tests, `SET`/`CLEAR`, scene changes). Off
    /// by default - when off, behaviour is identical to before.
    pub use_vm_dialogue: bool,
    /// Route the live basic-attack damage through the retail damage
    /// finisher ([`legaia_engine_vm::battle_formulas::damage_finish`], the port
    /// of `FUN_801ddb30`) instead of stopping at the raw roll. The finisher
    /// adds the universal post-stages - the party defender's equipment
    /// elemental-resistance ladder (live, off the character's ability words
    /// via [`crate::world::World::defender_resist`]), the rand-based no-damage floor on a
    /// hit mitigation zeroed, and the 9999 cap. The guard halve is
    /// deliberately not taken here: the melee kernel already charges the
    /// Spirit stance as its guard-roll triple. **On by default** - retail
    /// always runs the finisher after the melee roll; `false` keeps the flat
    /// pre-finisher path (min-floor 1, `0xFFFF` cap) for comparison. The
    /// finisher draws one RNG **only** when the hit zeroes out, matching
    /// retail.
    pub use_damage_finish: bool,
    /// Battle "Select Attack" option - retail config word `0x800846C4`,
    /// the pause menu's row ([`crate::options::SelectAttackOpt`]): whether
    /// the ring's Attack arm shows the `Auto | Command` prompt (`0x78`), goes
    /// straight to the target cursor (`0x5A`) or straight to the directional
    /// arts entry (`0x50`) - `FUN_801D0748`'s `0x28` Left arm at
    /// `0x801D15E0..0x801D1650`. Hosts mirror their `OptionsState` onto this
    /// the way they mirror [`crate::world::FieldLocomotion::run_default`].
    pub(crate) select_attack: crate::options::SelectAttackOpt,
    /// The options screen's **Battle Camera** row (config word
    /// `0x800846C0`, Close / Normal / Far), which the battle camera's action
    /// shots read (`BattleCamera::set_camera_option`). Hosts mirror their
    /// `OptionsState` onto this through `OptionsState::apply_to_world`.
    pub battle_camera: crate::options::BattleCameraOpt,
    /// The options screen's **Field HP Display** row (config word
    /// `0x800845C4`, Immediate / Gradual / Display Off). The field party
    /// HUD (`FUN_801D0D38`) reads it as its idle delay - `0x28` frames,
    /// `0xA0` frames, or never - so hosts mirror their `OptionsState` onto
    /// this through `OptionsState::apply_to_world` and both ask
    /// `world_map_panel_host::field_hud_view_mode`.
    pub(crate) field_hp_display: crate::options::HpDisplayOpt,
    /// Draw the overworld's per-placement **entity markers**
    /// ([`crate::world_map_markers`]: a cyan post at each portal, green at
    /// each NPC, red at each encounter zone). A port debug overlay, not a
    /// retail draw - a retail frame at a town entrance shows the town's own
    /// mesh and nothing over it - so it is off by default; the native window
    /// raises it under `LEGAIA_WORLD_MAP_MARKERS=1`. The player's stand-in
    /// marker (drawn only when the leader's mesh is missing) is not gated.
    pub overworld_marker_overlay: bool,
}

impl WorldToggles {
    pub fn new() -> Self {
        Self {
            entry_pulse_enabled: true,
            reduce_flashing: true,
            view_window_crop: true,
            volumetric_fog: false,
            live_gameplay_loop: false,
            smarter_monster_targeting: false,
            use_vm_dialogue: false,
            use_damage_finish: true,
            select_attack: crate::options::SelectAttackOpt::default(),
            battle_camera: crate::options::BattleCameraOpt::default(),
            field_hp_display: crate::options::HpDisplayOpt::Immediate,
            overworld_marker_overlay: false,
        }
    }
}

impl Default for WorldToggles {
    fn default() -> Self {
        Self::new()
    }
}
