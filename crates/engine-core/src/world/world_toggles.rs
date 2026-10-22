//! Engine behaviour toggles: the live gameplay loop, VM-driven dialogue, damage finish, monster targeting, select-attack option, flashing reduction and the entry pulse gate.
//!
//! One subsystem's state, held as a unit on the composite [`World`].
//! Fields keep their retail provenance notes.

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
    /// [`legaia_engine_menus::options::OptionsState::reduce_flashing`]). When `true`
    /// (the default - a host that never plumbs options stays safe),
    /// [`crate::world::World::step_ambient_fx`] slew-limits the **applied** luminance
    /// channels (`v_add`, `white`) toward each cell's simulated target
    /// instead of jumping, so full-swing per-tick strobes (koin3's dance
    /// floor) become sub-hazard-rate pulses. Hue / saturation sweeps pass
    /// through untouched. The move-VM state itself always advances
    /// retail-exact - this only shapes the VRAM presentation.
    pub(crate) reduce_flashing: bool,
    /// Retail's **visible-tile crop** on the field ground + decoration layers
    /// (`legaia_engine_world_views::field_view_window::field_view_cells`), from
    /// [`legaia_engine_menus::options::OptionsState::retail_view_window`]. Default on; it
    /// only takes effect at retail framing.
    pub view_window_crop: bool,
    /// The volumetric ground-fog **enhancement**
    /// ([`legaia_render_kernels::fog_volume`], stepped by `World::tick`), from
    /// [`legaia_engine_menus::options::OptionsState::volumetric_fog`]. Default **off** here
    /// so headless hosts (replays, oracles) carry no enhancement; the play
    /// hosts push their option (default on) through
    /// [`crate::options::apply_to_world`]. Off resets the bank,
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
    /// [`legaia_engine_mode::mode::SceneMode::Battle`] - however it got there: this roll, a field
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
    /// finisher ([`legaia_engine_battle_vm::battle_formulas::damage_finish`], the port
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
    /// the pause menu's row ([`legaia_engine_system::option_values::SelectAttackOpt`]): whether
    /// the ring's Attack arm shows the `Auto | Command` prompt (`0x78`), goes
    /// straight to the target cursor (`0x5A`) or straight to the directional
    /// arts entry (`0x50`) - `FUN_801D0748`'s `0x28` Left arm at
    /// `0x801D15E0..0x801D1650`. Hosts mirror their `OptionsState` onto this
    /// the way they mirror [`crate::world::FieldLocomotion::run_default`].
    pub(crate) select_attack: legaia_engine_system::option_values::SelectAttackOpt,
    /// The options screen's **Battle Command** row (config word
    /// `0x800846C8`): directional buttons, or the cross button over a
    /// highlighted arm. Only the second highlights a command chip - the menu
    /// SM's pre-dispatch block (`FUN_801D0748`, `0x801D07FC..0x801D0AC0`)
    /// runs under `_DAT_800846C8 != 0` and stamps the highlighted arm's
    /// frame style. Read by `legaia_engine_battle_hud::battle_chip_highlight`.
    pub battle_command: legaia_engine_system::option_values::BattleCommandOpt,
    /// The options screen's **Battle Camera** row (config word
    /// `0x800846C0`, Close / Normal / Far), which the battle camera's action
    /// shots read (`BattleCamera::set_camera_option`). Hosts mirror their
    /// `OptionsState` onto this through `crate::options::apply_to_world`.
    pub battle_camera: legaia_engine_system::option_values::BattleCameraOpt,
    /// The options screen's **Field HP Display** row (config word
    /// `0x800845C4`, Immediate / Gradual / Display Off). The field party
    /// HUD (`FUN_801D0D38`) reads it as its idle delay - `0x28` frames,
    /// `0xA0` frames, or never - so hosts mirror their `OptionsState` onto
    /// this through `crate::options::apply_to_world` and both ask
    /// `world_map_panel_host::field_hud_view_mode`.
    pub field_hp_display: legaia_engine_system::option_values::HpDisplayOpt,
    /// Draw the overworld's per-placement **entity markers**
    /// (`legaia_engine_world_views::world_map_markers`: a cyan post at each portal, green at
    /// each NPC, red at each encounter zone). A port debug overlay, not a
    /// retail draw - a retail frame at a town entrance shows the town's own
    /// mesh and nothing over it - so it is off by default; the native window
    /// raises it under `LEGAIA_WORLD_MAP_MARKERS=1`. The player's stand-in
    /// marker (drawn only when the leader's mesh is missing) is not gated.
    pub overworld_marker_overlay: bool,
    /// The battle frame driver's discarded `rand()` draw
    /// ([`crate::world::World::tick_battle_pass_draw`]): one draw from the
    /// shared stream on every battle pass, whose result retail never reads.
    /// On by default - it is retail behaviour, and every outcome roll's
    /// position in the stream depends on it. Clearing it is a tooling knob
    /// (an A/B over the stream), not an enhancement.
    pub battle_pass_rand_draw: bool,
    /// Run the battle action SM `FUN_801E295C` once a battle frame, retail's
    /// cadence ([`crate::world::World::battle_sm_pass_due`]), instead of once
    /// a vsync. The faithful model, staged off: the retail-compare drives,
    /// phase gates and displayed-frame retakes were tuned against the
    /// per-vsync SM and measure worse until they are re-tuned (see
    /// `docs/subsystems/battle-stage-camera.md`, the frame step). The engine
    /// never reads the process environment for it: the native session
    /// raises it from `LEGAIA_BATTLE_FRAME_CADENCE=1`
    /// (`legaia_engine_session::native::apply_env_world_toggles`, which a
    /// corpus run's child processes inherit) and the browser play page from
    /// its `set_battle_frame_cadence` switch.
    pub battle_sm_frame_cadence: bool,
    /// Open the legacy **saved-chain list**
    /// ([`legaia_engine_battle::battle_arts::BattleArtsSession`] over
    /// `World::party.saved_chains`) from the command ring's Arts arm instead
    /// of retail's per-press directional command input (`FUN_801D0748`
    /// state `0x50`). Off by default and off under [`Self::pin_retail`]:
    /// retail has no saved-chain list. The native session raises it from
    /// `LEGAIA_ARTS_SAVED_LIST=1`, the browser play page from its
    /// `set_arts_saved_list` switch.
    pub arts_saved_list: bool,
    /// Ledge-hop landing guard (enhancement, **on** by default): refuse a
    /// field ledge hop whose landing point the walk could not leave in any
    /// direction. Retail does not test the landing - the classifier
    /// `FUN_801d1878` checks only the wall bits at `2s` and `3s` and the
    /// actor sweep there, and the arc (`FUN_801d2404` / `FUN_801d5c08`)
    /// writes the Bezier into the player with no collision - so a diagonal
    /// step-up hop can set the player in the corner of a raised pocket whose
    /// walls the leading-edge probes then refuse on all four sides (`jouind`
    /// at `(2880, 2886)`), with no step left to start another hop. Off is
    /// retail's unguarded hop.
    pub hop_landing_guard: bool,
    /// The options screen's nine config words as the host last pushed them
    /// (`crate::options::apply_to_world`) or a save last loaded them. The
    /// rows the simulation reads have their own mirrors here and on
    /// `FieldLocomotion::run_default`; this keeps the rest (Sound, the
    /// vibration rows) so a save carries the whole window, as retail's
    /// does. See [`crate::world::World::retail_options`].
    pub retail_options: legaia_save::card::RetailOptions,
    /// The option words a save load restored that the host has not adopted
    /// yet ([`crate::world::World::take_loaded_options`]).
    pub(crate) loaded_options: Option<legaia_save::card::RetailOptions>,
}

/// The default of [`WorldToggles::battle_sm_frame_cadence`] - the one value
/// every host starts from: [`WorldToggles::new`], the native session's
/// environment switch (`legaia_engine_session::native::apply_env_world_toggles`,
/// which raises or lowers from here) and the browser play page's
/// `PageToggles` with its "Retail battle cadence" box, which reads the runtime
/// before its first write.
pub const DEFAULT_BATTLE_SM_FRAME_CADENCE: bool = false;

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
            select_attack: legaia_engine_system::option_values::SelectAttackOpt::default(),
            battle_command: legaia_engine_system::option_values::BattleCommandOpt::default(),
            battle_camera: legaia_engine_system::option_values::BattleCameraOpt::default(),
            field_hp_display: legaia_engine_system::option_values::HpDisplayOpt::Immediate,
            overworld_marker_overlay: false,
            battle_pass_rand_draw: true,
            battle_sm_frame_cadence: DEFAULT_BATTLE_SM_FRAME_CADENCE,
            arts_saved_list: false,
            hop_landing_guard: true,
            retail_options: legaia_save::card::RetailOptions::default(),
            loaded_options: None,
        }
    }
}

impl WorldToggles {
    /// Pin every fidelity toggle to retail's behaviour: no entry pulse, no
    /// photosensitivity slew, the retail tile crop, no volumetric fog, the
    /// uniform monster target, the damage finisher and the battle pass's
    /// discarded draw. The loop and dialogue-routing switches and the option
    /// mirrors are host wiring, not fidelity, and are left alone.
    ///
    /// A bare [`WorldToggles::new`] is not this: it keeps the
    /// photosensitivity slew and the entry pulse on, so a host that never
    /// plumbs options is safe by default. A headless comparison against
    /// retail calls this.
    pub fn pin_retail(&mut self) {
        self.entry_pulse_enabled = false;
        self.reduce_flashing = false;
        self.view_window_crop = true;
        self.volumetric_fog = false;
        self.smarter_monster_targeting = false;
        self.use_damage_finish = true;
        self.overworld_marker_overlay = false;
        self.battle_pass_rand_draw = true;
        self.hop_landing_guard = false;
        self.arts_saved_list = false;
    }

    /// The fidelity toggles currently off their retail value, by field name.
    /// Empty after [`Self::pin_retail`].
    pub fn retail_deviations(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.entry_pulse_enabled {
            out.push("entry_pulse_enabled");
        }
        if self.reduce_flashing {
            out.push("reduce_flashing");
        }
        if !self.view_window_crop {
            out.push("view_window_crop");
        }
        if self.volumetric_fog {
            out.push("volumetric_fog");
        }
        if self.smarter_monster_targeting {
            out.push("smarter_monster_targeting");
        }
        if !self.use_damage_finish {
            out.push("use_damage_finish");
        }
        if self.overworld_marker_overlay {
            out.push("overworld_marker_overlay");
        }
        if !self.battle_pass_rand_draw {
            out.push("battle_pass_rand_draw");
        }
        if self.hop_landing_guard {
            out.push("hop_landing_guard");
        }
        if self.arts_saved_list {
            out.push("arts_saved_list");
        }
        out
    }
}

impl Default for WorldToggles {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod fidelity_tests {
    use super::WorldToggles;

    #[test]
    fn pinned_toggles_report_no_deviation() {
        let mut t = WorldToggles::new();
        // The bare default keeps three safety / enhancement toggles on.
        assert_eq!(
            t.retail_deviations(),
            [
                "entry_pulse_enabled",
                "reduce_flashing",
                "hop_landing_guard"
            ]
        );
        t.pin_retail();
        assert!(t.retail_deviations().is_empty());
    }

    #[test]
    fn every_deviation_is_one_pin_undoes() {
        let mut t = WorldToggles::new();
        t.view_window_crop = false;
        t.volumetric_fog = true;
        t.smarter_monster_targeting = true;
        t.use_damage_finish = false;
        t.overworld_marker_overlay = true;
        t.battle_pass_rand_draw = false;
        t.arts_saved_list = true;
        assert_eq!(t.retail_deviations().len(), 10);
        t.pin_retail();
        assert!(t.retail_deviations().is_empty());
    }

    /// The engine reads no process environment for a gameplay switch: a
    /// fresh set of toggles is the same whatever the environment holds, so
    /// a host with no environment (the browser) starts where a native one
    /// does and raises a switch only by writing the field.
    #[test]
    fn env_switches_default_off_in_the_engine() {
        let t = WorldToggles::new();
        assert_eq!(
            t.battle_sm_frame_cadence,
            super::DEFAULT_BATTLE_SM_FRAME_CADENCE
        );
        assert!(!t.arts_saved_list);
    }
}
