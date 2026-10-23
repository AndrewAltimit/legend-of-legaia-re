//! The session switches both play hosts throw onto a live `World`: the
//! port-only gameplay and overlay toggles that have no persisted option
//! behind them, and how retail-faithful mode masks them.
//!
//! The native window takes them at launch (`--smart-monsters`,
//! `--no-entry-pulse`, `--retail` and the `LEGAIA_*` environment switches,
//! the launcher's rows); the play page takes them at any time (its boxes,
//! `crates/web-play/src/play_toggles.rs`). Both end in
//! [`SessionSwitches::apply`], so "retail-faithful holds this off" has one
//! definition: the switches [`WorldToggles::pin_retail`] lowers are lowered
//! here too while `retail` holds.

use legaia_engine_core::world::WorldToggles;

/// The engine's gameplay tweaks: the patcher features it offers as its own
/// switches - each the engine twin of one `legaia-patcher` flag, so a user
/// without a patched disc gets the same behaviour - plus the port's own
/// battle softlock guard. One value, carried whole by both hosts (the
/// native launch's `LaunchFidelity`, the play page's `PageToggles`), so a new
/// tweak is one field here and nowhere a host can forget. The classification
/// that picked these is in `docs/subsystems/engine.md` ("Patcher features
/// and the engine").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameplayTweaks {
    /// Shiny Seru (`--shiny-seru`): a rare capturable enemy with +35% stats
    /// whose Seru deals +35% damage (`WorldToggles::shiny_seru`). On by
    /// default.
    pub shiny_seru: bool,
    /// Run-away EXP (`--flee-exp`): a successful escape banks a share of the
    /// fled fight's EXP (`WorldToggles::flee_exp`). Off by default.
    pub flee_exp: bool,
    /// EXP multiplier in permille (`--exp-scale`,
    /// `WorldToggles::exp_rate_permille`); `1000` is retail and the default.
    pub exp_rate_permille: u16,
    /// Fortress encounter fix (`--fortress-encounter-fix`,
    /// `WorldToggles::fortress_encounter_fix`). On by default: a healthy save
    /// plays the same either way.
    pub fortress_encounter_fix: bool,
    /// Jewel fix (`--jewel-fix`): the boss cinematic casts respect elemental
    /// guards (`WorldToggles::jewel_fix`). Off by default.
    pub jewel_fix: bool,
    /// The battle HP-readout guard (`WorldToggles::hp_readout_guard`): the
    /// state-`0x51` readout softlock cannot hold. A port fix with no patcher
    /// counterpart. On by default.
    pub hp_readout_guard: bool,
    /// Spirit AP (`--spirit-ap`, `WorldToggles::spirit_ap`): the AP a Spirit
    /// turn charges; retail `32`, negative drains.
    pub spirit_ap: i16,
    /// Enemy-damage AP (`--damage-ap`, `WorldToggles::damage_ap`): AP per
    /// 100% of max HP lost; retail `100`, negative drains.
    pub damage_ap: i16,
    /// Seru catch rate (`--seru-catch-rate`, `WorldToggles::seru_catch_pct`):
    /// one flat percent for every capturable Seru; `None` is retail.
    pub seru_catch_pct: Option<u8>,
    /// Enemy attack count in permille (`--enemy-attack-count`,
    /// `WorldToggles::enemy_attack_permille`); `1000` is retail.
    pub enemy_attack_permille: u16,
    /// Enemy stat scale in permille (a uniform `--enemy-stat-scale`,
    /// `WorldToggles::enemy_stat_permille`); `1000` is retail.
    pub enemy_stat_permille: u16,
    /// Bonus equipment drop (`--equipment-drops`,
    /// `WorldToggles::bonus_drop`). Off by default.
    pub bonus_drop: bool,
}

impl Default for GameplayTweaks {
    /// The engine's own defaults, read off a fresh [`WorldToggles`] so the
    /// two cannot disagree.
    fn default() -> Self {
        Self::from_toggles(&WorldToggles::new())
    }
}

impl GameplayTweaks {
    /// The selectable EXP rates, in permille, in the order a host cycles
    /// them: retail first.
    pub const EXP_RATES: [u16; 6] = [1000, 1500, 2000, 3000, 5000, 500];

    /// The tweaks as `toggles` holds them.
    pub fn from_toggles(t: &WorldToggles) -> Self {
        Self {
            shiny_seru: t.shiny_seru,
            flee_exp: t.flee_exp,
            exp_rate_permille: t.exp_rate_permille,
            fortress_encounter_fix: t.fortress_encounter_fix,
            jewel_fix: t.jewel_fix,
            hp_readout_guard: t.hp_readout_guard,
            spirit_ap: t.spirit_ap,
            damage_ap: t.damage_ap,
            seru_catch_pct: t.seru_catch_pct,
            enemy_attack_permille: t.enemy_attack_permille,
            enemy_stat_permille: t.enemy_stat_permille,
            bonus_drop: t.bonus_drop,
        }
    }

    /// Every tweak at retail's value - what [`WorldToggles::pin_retail`]
    /// writes.
    pub fn retail() -> Self {
        let mut t = WorldToggles::new();
        t.pin_retail();
        Self::from_toggles(&t)
    }

    /// Write the set onto `toggles`. The EXP rate is held to
    /// [`legaia_engine_core::world::EXP_RATE_RANGE`].
    pub fn write(&self, toggles: &mut WorldToggles) {
        toggles.shiny_seru = self.shiny_seru;
        toggles.flee_exp = self.flee_exp;
        let range = legaia_engine_core::world::EXP_RATE_RANGE;
        toggles.exp_rate_permille = self.exp_rate_permille.clamp(*range.start(), *range.end());
        toggles.fortress_encounter_fix = self.fortress_encounter_fix;
        toggles.jewel_fix = self.jewel_fix;
        toggles.hp_readout_guard = self.hp_readout_guard;
        use legaia_engine_core::world::{DAMAGE_AP_RANGE, SCALE_RANGE, SPIRIT_AP_RANGE};
        let clamp_i = |v: i16, r: std::ops::RangeInclusive<i16>| v.clamp(*r.start(), *r.end());
        let clamp_u = |v: u16| v.clamp(*SCALE_RANGE.start(), *SCALE_RANGE.end());
        toggles.spirit_ap = clamp_i(self.spirit_ap, SPIRIT_AP_RANGE);
        toggles.damage_ap = clamp_i(self.damage_ap, DAMAGE_AP_RANGE);
        toggles.seru_catch_pct = self.seru_catch_pct.map(|p| p.min(100));
        toggles.enemy_attack_permille = clamp_u(self.enemy_attack_permille);
        toggles.enemy_stat_permille = clamp_u(self.enemy_stat_permille);
        toggles.bonus_drop = self.bonus_drop;
    }

    /// The Spirit AP settings a host cycles, retail first.
    pub const SPIRIT_AP_STEPS: [i16; 8] = [32, 48, 64, 100, 0, 16, -16, -32];
    /// The enemy-damage AP settings a host cycles, retail first.
    pub const DAMAGE_AP_STEPS: [i16; 7] = [100, 150, 200, 50, 0, -50, -100];
    /// The Seru catch rates a host cycles, retail (each Seru's own) first.
    pub const CATCH_RATE_STEPS: [Option<u8>; 6] =
        [None, Some(25), Some(50), Some(75), Some(100), Some(10)];
    /// The enemy attack-count multipliers a host cycles, retail first.
    pub const ENEMY_ATTACK_RATES: [u16; 5] = [1000, 1500, 2000, 3000, 500];
    /// The enemy stat multipliers a host cycles, retail first.
    pub const ENEMY_STAT_RATES: [u16; 8] = [1000, 1250, 1500, 2000, 3000, 5000, 750, 500];

    /// The next (or, `dir < 0`, previous) entry of `steps` after `cur`; an
    /// off-list value steps to the first (retail) entry.
    pub fn step_in<T: PartialEq + Copy>(steps: &[T], cur: T, dir: i8) -> T {
        let n = steps.len();
        match steps.iter().position(|&r| r == cur) {
            Some(i) if dir < 0 => steps[(i + n - 1) % n],
            Some(i) => steps[(i + 1) % n],
            None => steps[0],
        }
    }

    /// A catch-rate setting as a player reads it: `Original` or `50%`.
    pub fn catch_rate_label(pct: Option<u8>) -> String {
        pct.map_or_else(|| "Original".to_string(), |p| format!("{p}%"))
    }

    /// The EXP rate as a player reads it: `1x`, `1.5x`, `0.5x`.
    pub fn exp_rate_label(permille: u16) -> String {
        let whole = permille / 1000;
        let frac = permille % 1000;
        if frac == 0 {
            format!("{whole}x")
        } else {
            let mut s = format!("{whole}.{frac:03}");
            while s.ends_with('0') {
                s.pop();
            }
            format!("{s}x")
        }
    }

    /// The next (or, `dir < 0`, previous) rate in [`Self::EXP_RATES`]
    /// after `permille`; an off-list value steps to retail.
    pub fn step_exp_rate(permille: u16, dir: i8) -> u16 {
        Self::step_in(&Self::EXP_RATES, permille, dir)
    }
}

/// One host's switch set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionSwitches {
    /// A monster's single-target attack picks the lowest-HP living party
    /// member (`WorldToggles::smarter_monster_targeting`). Not retail.
    pub smart_monsters: bool,
    /// The port's per-placement overworld markers, a debug overlay
    /// (`WorldToggles::overworld_marker_overlay`).
    pub world_map_markers: bool,
    /// The scene-entry VDF pulse enhancement
    /// (`WorldToggles::entry_pulse_enabled`).
    pub entry_pulse: bool,
    /// The battle pass on retail's once-a-battle-frame cadence
    /// (`WorldToggles::battle_sm_frame_cadence`). Retail behaviour, so the
    /// retail mask leaves it as set.
    pub battle_frame_cadence: bool,
    /// The Arts arm opens the legacy saved-chain list
    /// (`WorldToggles::arts_saved_list`). Not retail.
    pub arts_saved_list: bool,
    /// The patcher features graduated into engine switches.
    pub tweaks: GameplayTweaks,
    /// Retail-faithful mode: every port-only switch above held off.
    pub retail: bool,
}

impl SessionSwitches {
    /// Write the set onto `toggles`.
    ///
    /// While `retail` holds this is the engine's whole retail definition,
    /// [`WorldToggles::pin_retail`]: besides the switches above it lowers
    /// the port's own guards (the hop landing guard) and restores the
    /// retail kernels (the damage finisher, the battle pass's RNG draw), so
    /// [`WorldToggles::retail_deviations`] comes back empty once the options
    /// (pinned by the host's own retail mode) have been applied. Released,
    /// the guard `pin_retail` lowered returns to the engine default; the
    /// kernels it restored are already their defaults.
    pub fn apply(&self, toggles: &mut WorldToggles) {
        let port = !self.retail;
        toggles.smarter_monster_targeting = self.smart_monsters && port;
        toggles.overworld_marker_overlay = self.world_map_markers && port;
        toggles.entry_pulse_enabled = self.entry_pulse && port;
        toggles.battle_sm_frame_cadence = self.battle_frame_cadence;
        toggles.arts_saved_list = self.arts_saved_list && port;
        if port {
            self.tweaks.write(toggles);
        }
        if self.retail {
            toggles.pin_retail();
        } else {
            toggles.hop_landing_guard = WorldToggles::new().hop_landing_guard;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_on(retail: bool) -> SessionSwitches {
        SessionSwitches {
            smart_monsters: true,
            world_map_markers: true,
            entry_pulse: true,
            battle_frame_cadence: true,
            arts_saved_list: true,
            tweaks: GameplayTweaks {
                shiny_seru: true,
                flee_exp: true,
                exp_rate_permille: 2000,
                fortress_encounter_fix: true,
                jewel_fix: true,
                hp_readout_guard: true,
                spirit_ap: -16,
                damage_ap: 200,
                seru_catch_pct: Some(100),
                enemy_attack_permille: 2000,
                enemy_stat_permille: 1500,
                bonus_drop: true,
            },
            retail,
        }
    }

    #[test]
    fn switches_reach_the_world() {
        let mut t = WorldToggles::new();
        all_on(false).apply(&mut t);
        assert!(t.smarter_monster_targeting);
        assert!(t.overworld_marker_overlay);
        assert!(t.entry_pulse_enabled);
        assert!(t.battle_sm_frame_cadence);
        assert!(t.arts_saved_list);
        assert!(t.shiny_seru && t.flee_exp && t.fortress_encounter_fix && t.jewel_fix);
        assert_eq!(t.exp_rate_permille, 2000);
    }

    /// Retail mode holds every tweak at retail's value, whatever the set
    /// says; released, the set lands again.
    #[test]
    fn retail_masks_every_tweak() {
        let mut t = WorldToggles::new();
        all_on(true).apply(&mut t);
        assert_eq!(GameplayTweaks::from_toggles(&t), GameplayTweaks::retail());
        all_on(false).apply(&mut t);
        assert_eq!(GameplayTweaks::from_toggles(&t), all_on(false).tweaks);
    }

    #[test]
    fn tweak_defaults_are_the_engine_defaults_and_rates_cycle() {
        let d = GameplayTweaks::default();
        assert_eq!(d, GameplayTweaks::from_toggles(&WorldToggles::new()));
        assert!(d.shiny_seru && d.fortress_encounter_fix && d.hp_readout_guard);
        assert!(!d.flee_exp && !d.jewel_fix);
        assert_eq!(d.exp_rate_permille, 1000);
        assert_eq!(GameplayTweaks::exp_rate_label(1500), "1.5x");
        assert_eq!(GameplayTweaks::exp_rate_label(500), "0.5x");
        assert_eq!(GameplayTweaks::exp_rate_label(2000), "2x");
        let mut r = 1000;
        for _ in 0..GameplayTweaks::EXP_RATES.len() {
            r = GameplayTweaks::step_exp_rate(r, 1);
        }
        assert_eq!(r, 1000);
        assert_eq!(GameplayTweaks::step_exp_rate(1000, -1), 500);
        assert_eq!(GameplayTweaks::step_exp_rate(1234, 1), 1000);
        // An out-of-range rate is clamped when written.
        let mut t = WorldToggles::new();
        GameplayTweaks {
            exp_rate_permille: 9000,
            ..d
        }
        .write(&mut t);
        assert_eq!(t.exp_rate_permille, 5000);
        // The tuning sliders start at retail and their step lists lead with it.
        assert_eq!(d.spirit_ap, GameplayTweaks::SPIRIT_AP_STEPS[0]);
        assert_eq!(d.damage_ap, GameplayTweaks::DAMAGE_AP_STEPS[0]);
        assert_eq!(d.seru_catch_pct, GameplayTweaks::CATCH_RATE_STEPS[0]);
        assert_eq!(
            d.enemy_attack_permille,
            GameplayTweaks::ENEMY_ATTACK_RATES[0]
        );
        assert_eq!(d.enemy_stat_permille, GameplayTweaks::ENEMY_STAT_RATES[0]);
        assert!(!d.bonus_drop);
        assert_eq!(GameplayTweaks::catch_rate_label(None), "Original");
        assert_eq!(GameplayTweaks::catch_rate_label(Some(50)), "50%");
        GameplayTweaks {
            spirit_ap: 500,
            damage_ap: -900,
            enemy_stat_permille: 1,
            ..d
        }
        .write(&mut t);
        assert_eq!(
            (t.spirit_ap, t.damage_ap, t.enemy_stat_permille),
            (100, -200, 100)
        );
    }

    /// Retail mode lowers exactly what `WorldToggles::pin_retail` lowers
    /// among these switches, and leaves the (retail) cadence alone.
    #[test]
    fn retail_masks_what_pin_retail_lowers() {
        let mut t = WorldToggles::new();
        all_on(true).apply(&mut t);
        let mut pinned = WorldToggles::new();
        all_on(false).apply(&mut pinned);
        pinned.pin_retail();
        assert_eq!(
            t.smarter_monster_targeting,
            pinned.smarter_monster_targeting
        );
        assert_eq!(t.overworld_marker_overlay, pinned.overworld_marker_overlay);
        assert_eq!(t.entry_pulse_enabled, pinned.entry_pulse_enabled);
        assert_eq!(t.arts_saved_list, pinned.arts_saved_list);
        assert!(t.battle_sm_frame_cadence);
    }

    /// Retail mode is the whole `pin_retail`: nothing off its retail value,
    /// whatever the switches and the port's guards said. Released, the hop
    /// guard comes back.
    #[test]
    fn retail_is_the_whole_retail_definition_and_releases() {
        let mut t = WorldToggles::new();
        t.use_damage_finish = false;
        t.battle_pass_rand_draw = false;
        all_on(true).apply(&mut t);
        assert!(
            t.retail_deviations().is_empty(),
            "{:?}",
            t.retail_deviations()
        );
        let mut t = WorldToggles::new();
        all_on(true).apply(&mut t);
        assert!(!t.hop_landing_guard);
        all_on(false).apply(&mut t);
        assert_eq!(t.hop_landing_guard, WorldToggles::new().hop_landing_guard);
    }
}
