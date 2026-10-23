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
    pub select_attack: legaia_engine_system::option_values::SelectAttackOpt,
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
    /// a vsync. The faithful model and the default
    /// ([`DEFAULT_BATTLE_SM_FRAME_CADENCE`]); clearing it runs the per-vsync
    /// SM (see `docs/subsystems/battle-stage-camera.md`, the frame step).
    /// The engine never reads the process environment for it: the native
    /// session sets it from `LEGAIA_BATTLE_FRAME_CADENCE` (`0` lowers it)
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
    /// Shiny Seru (port gameplay feature, the engine twin of the patcher's
    /// `--shiny-seru`): each battle entry rolls
    /// [`legaia_engine_battle::seru_state::SeruState::shiny_chance_pct`] for one
    /// capturable enemy spawning shiny. On by default; off under
    /// [`Self::pin_retail`], where the roll - and the `rand()` draw it takes
    /// at battle entry, which retail never makes - is skipped entirely.
    pub shiny_seru: bool,
    /// Run-away EXP (the engine twin of the patcher's `--flee-exp`): a
    /// successful escape banks [`FLEE_EXP_PCT`] percent of the fled
    /// formation's listed EXP into every party member's cumulative XP. Off by
    /// default and under [`Self::pin_retail`]: retail awards nothing for
    /// running.
    pub flee_exp: bool,
    /// EXP multiplier, in permille (the engine twin of the patcher's
    /// `--exp-scale`): each monster's base EXP reward is scaled by this
    /// before the victory payout sums it, with the patcher's arithmetic
    /// ([`scale_exp_permille`]). [`EXP_RATE_RETAIL`] (`1000`) is retail and
    /// the default; [`Self::pin_retail`] restores it.
    pub exp_rate_permille: u16,
    /// Fortress encounter fix (the engine twin of the patcher's
    /// `--fortress-encounter-fix`): in Jette's Absolute Fortress
    /// ([`FORTRESS_ENCOUNTER_SCENES`]) the encounter condition walk reads
    /// system flag [`FORTRESS_STALE_FLAG`] as clear, so a save that left the
    /// Floating Castle with under a minute on its clock keeps its random
    /// battles and its Door of Light there. A healthy save never has the flag
    /// set there, so it plays the same either way. On by default; off under
    /// [`Self::pin_retail`].
    pub fortress_encounter_fix: bool,
    /// Jewel fix (the engine twin of the patcher's `--jewel-fix`): the boss
    /// cinematic casts that retail rolls through the resist-bypass wrapper
    /// `FUN_801DD6B4` roll through the respecting one, `FUN_801DD4B0`, so
    /// elemental jewels, guards and All Guard apply to them like every other
    /// special. Off by default (it is a balance change) and under
    /// [`Self::pin_retail`].
    pub jewel_fix: bool,
    /// Battle HP-readout guard (port softlock fix, **on** by default): a
    /// party slot whose displayed HP differs from its live HP with nothing
    /// left in the readout's pending accumulator - the absorbing state that
    /// parks the action SM's state `0x51` on every party-targeted action
    /// (`docs/subsystems/battle-action-exit-gates.md`) - gets the difference
    /// re-seeded, so the readout counts to live HP and the gate opens. Retail
    /// re-syncs only on a poison / regen tick (`FUN_801E752C`). The patcher
    /// has no counterpart. Off under [`Self::pin_retail`]; the invariant holds
    /// in every state retail itself produces cleanly, so the guard is an
    /// identity there.
    pub hp_readout_guard: bool,
    /// Spirit AP (the engine twin of the patcher's `--spirit-ap`): the AP a
    /// Spirit turn charges into the gauge, retail [`SPIRIT_AP_RETAIL`]
    /// (`actor[+0x224] = 0x20` at `0x801E5D84`). Negative drains the gauge,
    /// floored at zero, with the two AP-Boost bonuses inert, as the patch
    /// rewrites it. Held to [`SPIRIT_AP_RANGE`]; retail under
    /// [`Self::pin_retail`].
    pub spirit_ap: i16,
    /// Enemy-damage AP (the engine twin of `--damage-ap`): the AP a defender
    /// gains per 100% of max HP a hit takes, retail [`DAMAGE_AP_RETAIL`].
    /// Negative drains, with the bonuses inert. Held to [`DAMAGE_AP_RANGE`];
    /// retail under [`Self::pin_retail`].
    pub damage_ap: i16,
    /// Seru catch rate (the engine twin of `--seru-catch-rate`): one flat
    /// percent in place of every capturable monster's own catch chance.
    /// `None` (retail, and under [`Self::pin_retail`]) reads each record's.
    pub seru_catch_pct: Option<u8>,
    /// Enemy attack count, in permille (the engine twin of
    /// `--enemy-attack-count`): each affordable attack entry's AGL price is
    /// divided by it ([`scale_attack_cost`]). `1000` is retail.
    pub enemy_attack_permille: u16,
    /// Enemy stat scale, in permille (the engine twin of a uniform
    /// `--enemy-stat-scale`): every monster's HP / MP / ATK / UDF / LDF / INT /
    /// SPD record value is scaled before the battle loader's boost
    /// ([`scale_stat_permille`]). `1000` is retail.
    pub enemy_stat_permille: u16,
    /// Bonus equipment drop (the engine twin of `--equipment-drops`): once
    /// per won battle, after the normal drop, a [`BONUS_DROP_CHANCE_PCT`]
    /// roll grants one random piece of equipment. Off by default and under
    /// [`Self::pin_retail`].
    pub bonus_drop: bool,
    /// The options screen's nine config words as the host last pushed them
    /// (`crate::options::apply_to_world`) or a save last loaded them. The
    /// rows the simulation reads have their own mirrors here and on
    /// `FieldLocomotion::run_default`; this keeps the rest (Sound, the
    /// vibration rows) so a save carries the whole window, as retail's
    /// does. See `WorldSaveState::retail_options` (`legaia-engine-save-state`).
    pub retail_options: legaia_save::card::RetailOptions,
    /// The option words a save load restored that the host has not adopted
    /// yet (`WorldSaveState::take_loaded_options` (`legaia-engine-save-state`)).
    pub loaded_options: Option<legaia_save::card::RetailOptions>,
}

/// The default of [`WorldToggles::battle_sm_frame_cadence`] - the one value
/// every host starts from: [`WorldToggles::new`], the native session's
/// environment switch (`legaia_engine_session::native::apply_env_world_toggles`,
/// which raises or lowers from here) and the browser play page's
/// `PageToggles` with its "Retail battle cadence" box, which reads the runtime
/// before its first write.
pub const DEFAULT_BATTLE_SM_FRAME_CADENCE: bool = true;

/// [`WorldToggles::exp_rate_permille`] at retail: every reward as authored.
pub const EXP_RATE_RETAIL: u16 = 1000;

/// The range [`WorldToggles::exp_rate_permille`] is held to: `0.1x..=5x`, the
/// patcher's `--exp-scale` range.
pub const EXP_RATE_RANGE: std::ops::RangeInclusive<u16> = 100..=5000;

/// The share of a fled formation's EXP [`WorldToggles::flee_exp`] banks, in
/// percent: the patcher's `--flee-exp-pct` default.
pub const FLEE_EXP_PCT: u32 = 5;

/// The scenes [`WorldToggles::fortress_encounter_fix`] applies in: Jette's
/// Absolute Fortress, entrance and interior - the only scenes outside the
/// Floating Castle whose encounter condition list names
/// [`FORTRESS_STALE_FLAG`] (`docs/reference/glitches/field-oddities.md`).
pub const FORTRESS_ENCOUNTER_SCENES: [&str; 2] = ["deroa", "chitei2"];

/// The Floating Castle's last-minute "no battles" system flag, which the
/// escape-success record never lowers.
pub const FORTRESS_STALE_FLAG: u16 = 0x15;

/// [`WorldToggles::spirit_ap`] at retail (`0x20`).
pub const SPIRIT_AP_RETAIL: i16 = legaia_engine_battle_vm::battle_action::SPIRIT_AP_RETAIL;
/// The patcher's `--spirit-ap` range.
pub const SPIRIT_AP_RANGE: std::ops::RangeInclusive<i16> = -100..=100;
/// [`WorldToggles::damage_ap`] at retail: a full-HP hit fills the gauge.
pub const DAMAGE_AP_RETAIL: i16 = legaia_engine_battle_vm::battle_formulas::DAMAGE_AP_RETAIL;
/// The patcher's `--damage-ap` range.
pub const DAMAGE_AP_RANGE: std::ops::RangeInclusive<i16> = -200..=200;
/// A permille scale at retail: the identity.
pub const SCALE_RETAIL: u16 = 1000;
/// The range the permille sliders are held to: `0.1x..=5x`, the patcher's
/// `ScalePermille` range.
pub const SCALE_RANGE: std::ops::RangeInclusive<u16> = 100..=5000;
/// The bonus equipment drop's per-battle chance, in percent: the patcher's
/// `--equipment-drop-chance` default.
pub const BONUS_DROP_CHANCE_PCT: u32 = 5;
/// The monster the enemy sliders never touch: Tetsu, the Rim Elm sparring
/// partner (unwinnable by design), as the patcher's
/// `SCALE_PINNED_MONSTER_IDS` pins him.
pub const SCALE_PINNED_MONSTER_ID: u16 = 79;

/// Scale one monster stat by `permille` with the patcher's `scale_stats`
/// rule: round half up, a zero stays zero, a non-zero clamps to
/// `1..=u16::MAX`. `1000` is the identity.
pub fn scale_stat_permille(v: u16, permille: u16) -> u16 {
    if v == 0 || permille == SCALE_RETAIL {
        return v;
    }
    let scaled = (u32::from(v) * u32::from(permille) + 500) / 1000;
    scaled.clamp(1, u32::from(u16::MAX)) as u16
}

/// Scale one attack entry's AGL price by an attack-count multiplier with the
/// patcher's `attack_count::scale_cost` rule: the `0xFF` sentinel, a zero
/// cost and a retail-unaffordable cost (`> agl`) are left alone; otherwise
/// `round-half-up(cost * 1000 / permille)` clamped to `1..=min(agl, 0xFE)`,
/// so a retail attacker keeps at least one strike a turn.
pub fn scale_attack_cost(cost: u8, agl: u16, permille: u16) -> u8 {
    if permille == SCALE_RETAIL || cost == 0xFF || cost == 0 || u16::from(cost) > agl {
        return cost;
    }
    let p = u32::from(permille);
    let scaled = (u32::from(cost) * 1000 + p / 2) / p;
    let cap = u32::from(agl.min(0xFE));
    scaled.clamp(1, cap) as u8
}

/// Scale one monster's base EXP reward by `permille`, with the patcher's
/// `--exp-scale` arithmetic (`legaia_patcher::rewards::scale_exp_value`):
/// round half up, a zero reward stays zero, a non-zero one clamps to
/// `1..=u16::MAX`. `1000` is exactly the identity.
pub fn scale_exp_permille(exp: u16, permille: u16) -> u16 {
    if exp == 0 || permille == EXP_RATE_RETAIL {
        return exp;
    }
    let scaled = (u32::from(exp) * u32::from(permille) + 500) / 1000;
    scaled.clamp(1, u32::from(u16::MAX)) as u16
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
            select_attack: legaia_engine_system::option_values::SelectAttackOpt::default(),
            battle_command: legaia_engine_system::option_values::BattleCommandOpt::default(),
            battle_camera: legaia_engine_system::option_values::BattleCameraOpt::default(),
            field_hp_display: legaia_engine_system::option_values::HpDisplayOpt::Immediate,
            overworld_marker_overlay: false,
            battle_pass_rand_draw: true,
            battle_sm_frame_cadence: DEFAULT_BATTLE_SM_FRAME_CADENCE,
            arts_saved_list: false,
            hop_landing_guard: true,
            shiny_seru: true,
            flee_exp: false,
            exp_rate_permille: EXP_RATE_RETAIL,
            fortress_encounter_fix: true,
            jewel_fix: false,
            hp_readout_guard: true,
            spirit_ap: SPIRIT_AP_RETAIL,
            damage_ap: DAMAGE_AP_RETAIL,
            seru_catch_pct: None,
            enemy_attack_permille: SCALE_RETAIL,
            enemy_stat_permille: SCALE_RETAIL,
            bonus_drop: false,
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
        self.shiny_seru = false;
        self.flee_exp = false;
        self.exp_rate_permille = EXP_RATE_RETAIL;
        self.fortress_encounter_fix = false;
        self.jewel_fix = false;
        self.hp_readout_guard = false;
        self.spirit_ap = SPIRIT_AP_RETAIL;
        self.damage_ap = DAMAGE_AP_RETAIL;
        self.seru_catch_pct = None;
        self.enemy_attack_permille = SCALE_RETAIL;
        self.enemy_stat_permille = SCALE_RETAIL;
        self.bonus_drop = false;
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
        if self.shiny_seru {
            out.push("shiny_seru");
        }
        if self.flee_exp {
            out.push("flee_exp");
        }
        if self.exp_rate_permille != EXP_RATE_RETAIL {
            out.push("exp_rate_permille");
        }
        if self.fortress_encounter_fix {
            out.push("fortress_encounter_fix");
        }
        if self.jewel_fix {
            out.push("jewel_fix");
        }
        if self.hp_readout_guard {
            out.push("hp_readout_guard");
        }
        if self.spirit_ap != SPIRIT_AP_RETAIL {
            out.push("spirit_ap");
        }
        if self.damage_ap != DAMAGE_AP_RETAIL {
            out.push("damage_ap");
        }
        if self.seru_catch_pct.is_some() {
            out.push("seru_catch_pct");
        }
        if self.enemy_attack_permille != SCALE_RETAIL {
            out.push("enemy_attack_permille");
        }
        if self.enemy_stat_permille != SCALE_RETAIL {
            out.push("enemy_stat_permille");
        }
        if self.bonus_drop {
            out.push("bonus_drop");
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
        // The bare default keeps three safety / enhancement toggles, the
        // shiny roll and the Fortress fix on.
        assert_eq!(
            t.retail_deviations(),
            [
                "entry_pulse_enabled",
                "reduce_flashing",
                "hop_landing_guard",
                "shiny_seru",
                "fortress_encounter_fix",
                "hp_readout_guard"
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
        t.flee_exp = true;
        t.exp_rate_permille = 2000;
        t.jewel_fix = true;
        t.spirit_ap = 64;
        t.damage_ap = -50;
        t.seru_catch_pct = Some(100);
        t.enemy_attack_permille = 2000;
        t.enemy_stat_permille = 1500;
        t.bonus_drop = true;
        assert_eq!(t.retail_deviations().len(), 22);
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

    #[test]
    fn exp_scale_matches_the_patcher_arithmetic() {
        use super::scale_exp_permille as s;
        assert_eq!(s(37, 1000), 37);
        assert_eq!(s(0, 3000), 0);
        assert_eq!(s(1, 100), 1); // never scaled away
        assert_eq!(s(15, 1500), 23); // 22.5 rounds half up
        assert_eq!(s(u16::MAX, 5000), u16::MAX);
    }

    #[test]
    fn enemy_scales_match_the_patcher_arithmetic() {
        use super::{scale_attack_cost as c, scale_stat_permille as st};
        assert_eq!(st(250, 1000), 250);
        assert_eq!(st(0, 2000), 0);
        assert_eq!(st(5, 100), 1);
        assert_eq!(st(15, 1500), 23);
        // Two-hit enemy at 2x -> four hits: the price halves.
        assert_eq!(c(44, 88, 2000), 22);
        // Below 1x the price never passes the AGL: one strike minimum.
        assert_eq!(c(88, 88, 500), 88);
        assert_eq!(c(0xFF, 88, 2000), 0xFF);
        assert_eq!(c(200, 99, 2000), 200, "retail lockouts stay locked");
        assert_eq!(c(0, 88, 2000), 0);
    }
}
