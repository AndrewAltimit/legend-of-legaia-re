//! The play page's twins of the native window's launch switches that were
//! native-only: `--retail`, `--set-flag`, `--no-entry-pulse`,
//! `LEGAIA_SMART_MONSTERS`, `LEGAIA_WORLD_MAP_MARKERS`,
//! `LEGAIA_BATTLE_FRAME_CADENCE`, `LEGAIA_ARTS_SAVED_LIST` and the gameplay
//! tweaks (`--no-shiny-seru`, `--flee-exp`, `--exp-rate`,
//! `--no-fortress-encounter-fix`, `--jewel-fix`, `--no-hp-readout-guard`).
//!
//! Each writes the same engine state the native launch writes - the world's
//! `WorldToggles`, the system-flag bank, `OptionsState::pin_retail` - so the
//! two hosts differ only in when the switch is thrown (the page at any time,
//! the window at launch). The world toggles live on the session's `World`,
//! which outlives a scene change; the page also keeps its own copy and
//! re-asserts it on every scene entry, so a switch thrown before a disc is
//! loaded holds once one is.

use crate::runtime::LegaiaRuntime;
use wasm_bindgen::prelude::*;

/// The page-side switches, re-asserted on every scene entry.
#[derive(Debug, Clone, Copy)]
pub struct PageToggles {
    /// `LEGAIA_SMART_MONSTERS`: a monster's single-target attack picks the
    /// lowest-HP living party member. Non-faithful, off by default.
    pub smart_monsters: bool,
    /// `LEGAIA_WORLD_MAP_MARKERS`: the port's per-placement overworld entity
    /// markers. A debug overlay retail does not draw; off by default.
    pub world_map_markers: bool,
    /// `--retail`: the persisted engine-only options pinned to their retail
    /// values for the session, and the scene-entry pulse off.
    pub retail: bool,
    /// The scene-entry VDF pulse enhancement; `false` is the native
    /// `--no-entry-pulse`. On by default, as on the window. Retail mode
    /// holds it off whatever this says.
    pub entry_pulse: bool,
    /// `LEGAIA_BATTLE_FRAME_CADENCE`: the battle pass on retail's
    /// once-a-battle-frame cadence (`WorldToggles::battle_sm_frame_cadence`).
    /// Starts from the engine's one default,
    /// `legaia_engine_core::world::DEFAULT_BATTLE_SM_FRAME_CADENCE`; the page's
    /// box reads it back before its first write.
    pub battle_frame_cadence: bool,
    /// `LEGAIA_ARTS_SAVED_LIST`: the ring's Arts arm opens the legacy
    /// saved-chain list instead of retail's per-press input
    /// (`WorldToggles::arts_saved_list`). Off by default.
    pub arts_saved_list: bool,
    /// Retail's 5-bit texture modulation on the overlay's tinted atlases
    /// (the native `Renderer::set_ui_psx_tint`). On by default: it is retail
    /// behaviour.
    pub ui_psx_tint: bool,
    /// The patcher features graduated into engine switches
    /// (`legaia_engine_session::session_switches::GameplayTweaks`), starting
    /// from the engine's defaults as the window does. Retail mode holds them
    /// at retail's values whatever this says.
    pub tweaks: legaia_engine_session::session_switches::GameplayTweaks,
}

impl Default for PageToggles {
    fn default() -> Self {
        Self {
            smart_monsters: false,
            world_map_markers: false,
            retail: false,
            entry_pulse: true,
            battle_frame_cadence: legaia_engine_core::world::DEFAULT_BATTLE_SM_FRAME_CADENCE,
            arts_saved_list: false,
            ui_psx_tint: true,
            tweaks: legaia_engine_session::session_switches::GameplayTweaks::default(),
        }
    }
}

impl LegaiaRuntime {
    /// Write the page's switches onto the live world. Called by each setter
    /// and from every scene entry.
    pub(crate) fn apply_page_toggles(&mut self) {
        let t = self.page_toggles;
        // The native window's launch applies the same set
        // (`legaia_engine_session::session_switches`), so retail mode masks
        // the same switches on both hosts.
        let switches = legaia_engine_session::session_switches::SessionSwitches {
            smart_monsters: t.smart_monsters,
            world_map_markers: t.world_map_markers,
            entry_pulse: t.entry_pulse,
            battle_frame_cadence: t.battle_frame_cadence,
            arts_saved_list: t.arts_saved_list,
            tweaks: t.tweaks,
            retail: t.retail,
        };
        if let Some(host) = self.scene_host.host_mut() {
            switches.apply(&mut host.world.toggles);
        }
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Monsters target the lowest-HP party member (the native window's
    /// `LEGAIA_SMART_MONSTERS=1`). The RNG stream is unchanged.
    pub fn set_smart_monsters(&mut self, on: bool) {
        self.page_toggles.smart_monsters = on;
        self.apply_page_toggles();
    }

    /// Whether [`Self::set_smart_monsters`] is on.
    pub fn smart_monsters(&self) -> bool {
        self.page_toggles.smart_monsters
    }

    /// Draw the overworld's per-placement entity markers (the native
    /// window's `LEGAIA_WORLD_MAP_MARKERS=1`). The page's marker pass already
    /// reads the world toggle.
    pub fn set_world_map_markers(&mut self, on: bool) {
        self.page_toggles.world_map_markers = on;
        self.apply_page_toggles();
    }

    /// Whether [`Self::set_world_map_markers`] is on.
    pub fn world_map_markers(&self) -> bool {
        self.page_toggles.world_map_markers
    }

    /// The scene-entry VDF pulse enhancement (off is the native
    /// `--no-entry-pulse`). Takes effect at the next scene entry, where the
    /// installer runs; retail mode holds it off.
    pub fn set_entry_pulse(&mut self, on: bool) {
        self.page_toggles.entry_pulse = on;
        self.apply_page_toggles();
    }

    /// Whether [`Self::set_entry_pulse`] is on.
    pub fn entry_pulse(&self) -> bool {
        self.page_toggles.entry_pulse
    }

    /// Run the battle pass once a battle frame, retail's cadence (the native
    /// `LEGAIA_BATTLE_FRAME_CADENCE`), or once a vsync. Both hosts start from
    /// `DEFAULT_BATTLE_SM_FRAME_CADENCE`.
    pub fn set_battle_frame_cadence(&mut self, on: bool) {
        self.page_toggles.battle_frame_cadence = on;
        self.apply_page_toggles();
    }

    /// Whether [`Self::set_battle_frame_cadence`] is on.
    pub fn battle_frame_cadence(&self) -> bool {
        self.page_toggles.battle_frame_cadence
    }

    /// Open the legacy saved-chain list from the command ring's Arts arm
    /// instead of retail's per-press input (the native
    /// `LEGAIA_ARTS_SAVED_LIST=1`). Retail mode holds it off.
    pub fn set_arts_saved_list(&mut self, on: bool) {
        self.page_toggles.arts_saved_list = on;
        self.apply_page_toggles();
    }

    /// Whether [`Self::set_arts_saved_list`] is on.
    pub fn arts_saved_list(&self) -> bool {
        self.page_toggles.arts_saved_list
    }

    /// Shiny Seru, the native `--no-shiny-seru` inverted (on by default):
    /// battle entry may roll a rare shiny capturable enemy. Retail mode holds
    /// it off.
    pub fn set_shiny_seru(&mut self, on: bool) {
        self.page_toggles.tweaks.shiny_seru = on;
        self.apply_page_toggles();
    }

    /// Whether [`Self::set_shiny_seru`] is on.
    pub fn shiny_seru(&self) -> bool {
        self.page_toggles.tweaks.shiny_seru
    }

    /// Run-away EXP, the native `--flee-exp`: a successful escape banks a
    /// share of the fled fight's EXP. Retail mode holds it off.
    pub fn set_flee_exp(&mut self, on: bool) {
        self.page_toggles.tweaks.flee_exp = on;
        self.apply_page_toggles();
    }

    /// Whether [`Self::set_flee_exp`] is on.
    pub fn flee_exp(&self) -> bool {
        self.page_toggles.tweaks.flee_exp
    }

    /// The EXP rate in permille, the native `--exp-rate` (`1000` = retail);
    /// clamped to the engine's range when applied. Retail mode holds `1000`.
    pub fn set_exp_rate_permille(&mut self, permille: u16) {
        self.page_toggles.tweaks.exp_rate_permille = permille;
        self.apply_page_toggles();
    }

    /// The rate [`Self::set_exp_rate_permille`] set.
    pub fn exp_rate_permille(&self) -> u16 {
        self.page_toggles.tweaks.exp_rate_permille
    }

    /// The Fortress encounter fix, the native `--no-fortress-encounter-fix`
    /// inverted (on by default). Retail mode holds it off.
    pub fn set_fortress_encounter_fix(&mut self, on: bool) {
        self.page_toggles.tweaks.fortress_encounter_fix = on;
        self.apply_page_toggles();
    }

    /// Whether [`Self::set_fortress_encounter_fix`] is on.
    pub fn fortress_encounter_fix(&self) -> bool {
        self.page_toggles.tweaks.fortress_encounter_fix
    }

    /// The jewel fix, the native `--jewel-fix`: the boss cinematic casts
    /// respect elemental guards. Retail mode holds it off.
    pub fn set_jewel_fix(&mut self, on: bool) {
        self.page_toggles.tweaks.jewel_fix = on;
        self.apply_page_toggles();
    }

    /// Whether [`Self::set_jewel_fix`] is on.
    pub fn jewel_fix(&self) -> bool {
        self.page_toggles.tweaks.jewel_fix
    }

    /// The battle HP-readout guard, the native `--no-hp-readout-guard`
    /// inverted (on by default): the state-`0x51` readout softlock cannot
    /// hold. Retail mode holds it off.
    pub fn set_hp_readout_guard(&mut self, on: bool) {
        self.page_toggles.tweaks.hp_readout_guard = on;
        self.apply_page_toggles();
    }

    /// Whether [`Self::set_hp_readout_guard`] is on.
    pub fn hp_readout_guard(&self) -> bool {
        self.page_toggles.tweaks.hp_readout_guard
    }

    /// The bonus equipment drop, the native `--equipment-drops`. Retail mode
    /// holds it off.
    pub fn set_bonus_drop(&mut self, on: bool) {
        self.page_toggles.tweaks.bonus_drop = on;
        self.apply_page_toggles();
    }

    /// Whether [`Self::set_bonus_drop`] is on.
    pub fn bonus_drop(&self) -> bool {
        self.page_toggles.tweaks.bonus_drop
    }

    /// Spirit AP, the native `--spirit-ap` (retail `32`; clamped to
    /// `-100..=100` when applied). Retail mode holds `32`.
    pub fn set_spirit_ap(&mut self, ap: i16) {
        self.page_toggles.tweaks.spirit_ap = ap;
        self.apply_page_toggles();
    }

    /// The value [`Self::set_spirit_ap`] set.
    pub fn spirit_ap(&self) -> i16 {
        self.page_toggles.tweaks.spirit_ap
    }

    /// Enemy-damage AP, the native `--damage-ap` (retail `100`; clamped to
    /// `-200..=200`). Retail mode holds `100`.
    pub fn set_damage_ap(&mut self, ap: i16) {
        self.page_toggles.tweaks.damage_ap = ap;
        self.apply_page_toggles();
    }

    /// The value [`Self::set_damage_ap`] set.
    pub fn damage_ap(&self) -> i16 {
        self.page_toggles.tweaks.damage_ap
    }

    /// The flat Seru catch rate in percent, the native `--seru-catch-rate`;
    /// a negative value restores each Seru's own chance (retail).
    pub fn set_seru_catch_rate(&mut self, pct: i16) {
        self.page_toggles.tweaks.seru_catch_pct = u8::try_from(pct).ok().map(|p| p.min(100));
        self.apply_page_toggles();
    }

    /// The catch rate [`Self::set_seru_catch_rate`] set, `-1` for retail.
    pub fn seru_catch_rate(&self) -> i16 {
        self.page_toggles
            .tweaks
            .seru_catch_pct
            .map_or(-1, i16::from)
    }

    /// The enemy attack-count multiplier in permille, the native
    /// `--enemy-attack-count` (`1000` = retail).
    pub fn set_enemy_attack_permille(&mut self, permille: u16) {
        self.page_toggles.tweaks.enemy_attack_permille = permille;
        self.apply_page_toggles();
    }

    /// The multiplier [`Self::set_enemy_attack_permille`] set.
    pub fn enemy_attack_permille(&self) -> u16 {
        self.page_toggles.tweaks.enemy_attack_permille
    }

    /// The enemy stat multiplier in permille, the native
    /// `--enemy-stat-scale` (`1000` = retail).
    pub fn set_enemy_stat_permille(&mut self, permille: u16) {
        self.page_toggles.tweaks.enemy_stat_permille = permille;
        self.apply_page_toggles();
    }

    /// The multiplier [`Self::set_enemy_stat_permille`] set.
    pub fn enemy_stat_permille(&self) -> u16 {
        self.page_toggles.tweaks.enemy_stat_permille
    }

    /// The photosensitivity guard (`OptionsState::reduce_flashing`), the
    /// native launcher's Reduce flashing row: persisted with the other
    /// options and applied through the session's option knobs. Retail mode
    /// pins it off for the session; an edit under the pin is the pin's.
    pub fn set_reduce_flashing(&mut self, on: bool) {
        if self.options_state.reduce_flashing != on {
            self.options_state.reduce_flashing = on;
            self.persist_and_apply_options();
        }
    }

    /// Whether [`Self::set_reduce_flashing`] is on.
    pub fn reduce_flashing(&self) -> bool {
        self.options_state.reduce_flashing
    }

    /// The music bus level, `0..=10` (`OptionsState::bgm_volume`, 8 = the
    /// retail mix), the native launcher's Music volume row. Clamped, then
    /// persisted and applied to the mixer.
    pub fn set_bgm_volume(&mut self, level: u8) {
        let level = level.min(legaia_engine_menus::options::VOLUME_MAX);
        if self.options_state.bgm_volume != level {
            self.options_state.bgm_volume = level;
            self.persist_and_apply_options();
        }
    }

    /// The music bus level [`Self::set_bgm_volume`] sets.
    pub fn bgm_volume(&self) -> u8 {
        self.options_state.bgm_volume
    }

    /// The sound-effect bus level, `0..=10` (`OptionsState::sfx_volume`),
    /// the native launcher's Sound effects volume row.
    pub fn set_sfx_volume(&mut self, level: u8) {
        let level = level.min(legaia_engine_menus::options::VOLUME_MAX);
        if self.options_state.sfx_volume != level {
            self.options_state.sfx_volume = level;
            self.persist_and_apply_options();
        }
    }

    /// The sound-effect bus level [`Self::set_sfx_volume`] sets.
    pub fn sfx_volume(&self) -> u8 {
        self.options_state.sfx_volume
    }

    /// The engine-only full-mono downmix (`OptionsState::downmix_mono`), the
    /// native launcher's Mono output row: the finished mix, voice and movie
    /// audio included, averaged into both channels. Separate from the retail
    /// options screen's Monaural row. Persisted and applied to the mixer.
    pub fn set_downmix_mono(&mut self, on: bool) {
        if self.options_state.downmix_mono != on {
            self.options_state.downmix_mono = on;
            self.persist_and_apply_options();
        }
    }

    /// Whether [`Self::set_downmix_mono`] is on.
    pub fn downmix_mono(&self) -> bool {
        self.options_state.downmix_mono
    }

    /// Retail-faithful mode, the native `--retail`: pin every persisted
    /// engine-only option to its retail value (`OptionsState::pin_retail`:
    /// retail camera distance, 8-way movement, no photosensitivity slew,
    /// retail tile windows, no fog, no enhanced lighting or lamp shadows, no
    /// occlusion fade) and turn the
    /// scene-entry pulse off, for this session only. The pinned knobs never
    /// reach the store (`OptionsPins::persistable`: the page's render boxes
    /// re-apply the pinned values on every scene rebuild, and one such write
    /// used to bake the pin into the store); every other row the player
    /// edits while the mode holds is saved, as natively. Off gives the
    /// pinned knobs back their pre-pin values. The page's render boxes
    /// (lighting, lamp shadows, fog, see-through walls) mirror the pinned
    /// options back after this call.
    pub fn set_retail_mode(&mut self, on: bool) {
        if self.page_toggles.retail == on {
            return;
        }
        self.page_toggles.retail = on;
        if on {
            self.options_pins.pin_retail(&mut self.options_state);
        } else {
            self.options_pins.release_all(&mut self.options_state);
        }
        // The pinned knobs persist as they were before the pin; every other
        // row keeps the edits made while the mode held (`OptionsPins`, the
        // native window's rule).
        self.persist_and_apply_options();
        self.apply_page_toggles();
    }

    /// Whether [`Self::set_retail_mode`] is on.
    pub fn retail_mode(&self) -> bool {
        self.page_toggles.retail
    }

    /// The persisted engine-only options still off their retail value, as a
    /// JSON array of names (`OptionsState::retail_deviations`) - empty while
    /// retail mode holds.
    pub fn retail_deviations_json(&self) -> String {
        serde_json::json!(self.options_state.retail_deviations()).to_string()
    }

    /// Raise (`on`) or clear a system / story flag in the shared bank
    /// (`DAT_80085758`), the native `--set-flag`. Returns `false` with no
    /// scene loaded.
    pub fn set_system_flag(&mut self, idx: u16, on: bool) -> bool {
        let Some(host) = self.scene_host.host_mut() else {
            return false;
        };
        if on {
            host.world.system_flag_set(idx);
        } else {
            host.world.system_flag_clear(idx);
        }
        true
    }

    /// Read a system / story flag (`false` with no scene loaded).
    pub fn system_flag(&self, idx: u16) -> bool {
        self.scene_host
            .host()
            .is_some_and(|h| h.world.system_flag_test(idx))
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Tint the overlay the way retail's GPU does (the default) or with the
    /// plain 8-bit multiply - the native window's `Renderer::set_ui_psx_tint`.
    /// The page's atlas blitter reads it through [`Self::psx_tint_atlas`].
    pub fn set_ui_psx_tint(&mut self, on: bool) {
        self.page_toggles.ui_psx_tint = on;
    }

    /// Whether [`Self::set_ui_psx_tint`] is on.
    pub fn ui_psx_tint(&self) -> bool {
        self.page_toggles.ui_psx_tint
    }

    /// One tinted copy of an overlay atlas (RGBA8): every texel through
    /// `legaia_render_kernels::ui_tint::tint_texel` - retail's 5-bit
    /// modulation (unless [`Self::set_ui_psx_tint`] turned it off) and the
    /// font CLUTs' own shadow under each ink. The same kernel the native text
    /// shader's WGSL is generated from; the page caches one copy per tint.
    pub fn psx_tint_atlas(&self, rgba: &[u8], r: f32, g: f32, b: f32) -> Vec<u8> {
        legaia_render_kernels::ui_tint::tint_atlas_rgb(
            rgba,
            [r, g, b],
            self.page_toggles.ui_psx_tint,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The switches hold before a disc is loaded and the retail pin is
    /// session-only: off restores the stored options.
    #[test]
    fn retail_mode_pins_the_options_and_restores_them() {
        let mut rt = LegaiaRuntime::new();
        rt.set_smart_monsters(true);
        rt.set_world_map_markers(true);
        assert!(rt.smart_monsters() && rt.world_map_markers());
        // The entry pulse starts on, as on the window; the cadence starts
        // on the engine's one default and the saved-chain list off, as a
        // native session with an empty environment.
        assert!(rt.entry_pulse());
        assert_eq!(
            rt.battle_frame_cadence(),
            legaia_engine_core::world::DEFAULT_BATTLE_SM_FRAME_CADENCE
        );
        assert!(!rt.arts_saved_list());
        rt.set_entry_pulse(false);
        rt.set_battle_frame_cadence(true);
        rt.set_arts_saved_list(true);
        assert!(!rt.entry_pulse() && rt.battle_frame_cadence() && rt.arts_saved_list());
        let stored = rt.options_state.clone();
        rt.set_retail_mode(true);
        assert!(rt.retail_mode());
        assert_eq!(rt.retail_deviations_json(), "[]");
        rt.set_retail_mode(false);
        assert_eq!(
            rt.options_state.retail_deviations(),
            stored.retail_deviations()
        );
        // No scene: the flag write is refused, not dropped silently.
        assert!(!rt.set_system_flag(0x142, true));
        assert!(!rt.system_flag(0x142));
    }

    /// The gameplay tweaks start at the engine's defaults (the window's too),
    /// each setter reads back, and the page carries a control for each.
    #[test]
    fn gameplay_tweaks_start_at_the_engine_defaults() {
        let mut rt = LegaiaRuntime::new();
        assert_eq!(
            rt.page_toggles.tweaks,
            legaia_engine_session::session_switches::GameplayTweaks::default()
        );
        assert!(rt.shiny_seru() && rt.fortress_encounter_fix() && rt.hp_readout_guard());
        assert!(!rt.flee_exp() && !rt.jewel_fix());
        assert_eq!(rt.exp_rate_permille(), 1000);
        rt.set_shiny_seru(false);
        rt.set_flee_exp(true);
        rt.set_exp_rate_permille(2000);
        rt.set_fortress_encounter_fix(false);
        rt.set_jewel_fix(true);
        assert!(!rt.shiny_seru() && rt.flee_exp() && rt.jewel_fix());
        assert!(!rt.fortress_encounter_fix());
        assert_eq!(rt.exp_rate_permille(), 2000);
        let page = include_str!("../../../site/_content/play.html");
        for call in [
            "set_shiny_seru",
            "set_flee_exp",
            "set_exp_rate_permille",
            "set_fortress_encounter_fix",
            "set_jewel_fix",
            "set_hp_readout_guard",
            "set_bonus_drop",
            "set_spirit_ap",
            "set_damage_ap",
            "set_seru_catch_rate",
            "set_enemy_attack_permille",
            "set_enemy_stat_permille",
        ] {
            assert!(page.contains(call), "play.html never calls {call}");
        }
    }

    /// The page's slider selectors offer exactly the engine's step lists, so
    /// the page and the launcher cycle the same values.
    #[test]
    fn the_page_sliders_offer_the_engine_steps() {
        use legaia_engine_session::session_switches::GameplayTweaks as T;
        let page = include_str!("../../../site/_content/play.html");
        let options = |id: &str| -> Vec<String> {
            let start = page.find(&format!("id=\"{id}\"")).expect(id);
            let end = start + page[start..].find("</select>").expect("select end");
            page[start..end]
                .split("<option value=\"")
                .skip(1)
                .map(|o| o.split('"').next().unwrap_or_default().to_string())
                .collect()
        };
        let s = |v: &[i64]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let i = |v: &[i16]| v.iter().map(|&x| i64::from(x)).collect::<Vec<_>>();
        let u = |v: &[u16]| v.iter().map(|&x| i64::from(x)).collect::<Vec<_>>();
        assert_eq!(options("play-exp-rate"), s(&u(&T::EXP_RATES)));
        assert_eq!(options("play-spirit-ap"), s(&i(&T::SPIRIT_AP_STEPS)));
        assert_eq!(options("play-damage-ap"), s(&i(&T::DAMAGE_AP_STEPS)));
        assert_eq!(options("play-enemy-attacks"), s(&u(&T::ENEMY_ATTACK_RATES)));
        assert_eq!(options("play-enemy-stats"), s(&u(&T::ENEMY_STAT_RATES)));
        let catch: Vec<i64> = T::CATCH_RATE_STEPS
            .iter()
            .map(|p| p.map_or(-1, i64::from))
            .collect();
        assert_eq!(options("play-seru-catch-rate"), s(&catch));
        let mut rt = LegaiaRuntime::new();
        rt.set_seru_catch_rate(-1);
        assert_eq!(rt.seru_catch_rate(), -1);
        rt.set_seru_catch_rate(50);
        assert_eq!(rt.seru_catch_rate(), 50);
    }

    /// The page's cadence starts where `WorldToggles::new` does, and the
    /// play page's box carries no state of its own: it reads the runtime
    /// back before its first write.
    #[test]
    fn the_cadence_default_is_single_sourced() {
        assert_eq!(
            PageToggles::default().battle_frame_cadence,
            legaia_engine_core::world::WorldToggles::new().battle_sm_frame_cadence
        );
        let page = include_str!("../../../site/_content/play.html");
        let tag = page
            .lines()
            .find(|l| l.contains("id=\"play-battle-frame-cadence\""))
            .expect("the cadence box");
        assert!(
            !tag.contains("checked"),
            "the box hardcodes its state: {tag}"
        );
        assert!(
            page.contains("cadenceEl.checked = !!c"),
            "the box no longer reads the runtime's default"
        );
    }

    /// The page tints its overlay atlases through the shared kernel: 5-bit
    /// by default, a font shadow takes its ink's CLUT entry, and the switch
    /// falls back to the plain multiply - the native shader's twin.
    #[test]
    fn the_page_tints_atlases_like_the_native_overlay() {
        use legaia_render_kernels::ui_tint::ink_component;
        let mut rt = LegaiaRuntime::new();
        assert!(rt.ui_psx_tint(), "retail tint is the default");
        let gold = [28u8, 21, 0].map(ink_component);
        let shadow = [41, 41, 41, legaia_font::FONT_SHADOW_ALPHA];
        // A whitewashed fill, a font shadow, a band-grey patch texel.
        let atlas = [[255, 255, 255, 255], shadow, [156, 140, 123, 255]].concat();
        let out = rt.psx_tint_atlas(&atlas, gold[0], gold[1], gold[2]);
        assert_eq!(&out[0..4], &[231, 173, 0, 255]);
        assert_eq!(&out[4..8], &[66, 66, 33, 255]);
        let grey = 97.0 / 128.0;
        let out = rt.psx_tint_atlas(&atlas, grey, grey, grey);
        assert_eq!(&out[8..12], &[115, 99, 90, 255]);
        rt.set_ui_psx_tint(false);
        let out = rt.psx_tint_atlas(&atlas, grey, grey, grey);
        assert_eq!(&out[8..12], &[118, 106, 93, 255]);
    }
}
