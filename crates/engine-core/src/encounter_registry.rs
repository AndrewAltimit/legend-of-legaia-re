//! Per-scene encounter-table registry.
//!
//! Each retail field scene carries its own `battle_data` PROT-entry payload
//! that holds the encounter table - formation ids + weights + trigger rate +
//! per-scene tweaks. The exact on-disc offset of the table inside
//! `0865_battle_data` (a 15.99 MB TIM-pack-shaped bundle) is not yet pinned;
//! the runtime resolver lives in an overlay slice that reads through a
//! pointer populated at scene-load time. Cracking it requires a
//! `mednafen-state diff` over the encounter-state RAM window
//! (`0x801C9300..0x801CA000`) on a save-state pair captured immediately
//! before vs. immediately after a battle trigger - see the
//! `crates/mednafen` toolkit and `scripts/mednafen/scenarios.toml`.
//!
//! Until the disc-side resolver lands, this registry lets engines compose
//! per-scene tables in *clean-room* form: keyed by CDNAME label, with
//! pattern-based fallbacks (substring matches like "outskirts" / "forest" /
//! "town" / "cave" / "world") and a global default. The `World::install_encounter_for_scene`
//! helper consults the registry on every scene transition.
//!
//! ## Design notes
//!
//! - Lookup is **most-specific-first**: exact-label match wins, then
//!   substring matches in registration order, then the global default.
//! - Substring matches are case-insensitive on the scene-label side. The
//!   matcher's pattern is the literal pattern (use `"outskirts"` not
//!   `"OUTSKIRTS"`).
//! - Engines that already have a captured per-scene table from disc data
//!   register it as an exact-label match; this becomes the source of truth
//!   for that label and ignores the substring fallbacks.
//!
//! Pure data - no Vfs / disc / world coupling. Compose from the engine
//! shell once the per-scene tables are known.

use crate::encounter::EncounterTable;
use std::collections::HashMap;

/// Pattern-based fallback rule.
///
/// The pattern is a substring matched against the scene label
/// (case-insensitive). The first registered rule whose pattern occurs in
/// the lower-cased label wins.
#[derive(Debug, Clone)]
pub struct PatternFallback {
    /// Lowercased substring to match against scene labels.
    pub pattern: String,
    /// Table to install when the pattern matches.
    pub table: EncounterTable,
}

/// Per-scene encounter-table resolver.
#[derive(Debug, Clone, Default)]
pub struct EncounterRegistry {
    /// Exact-label tables (highest priority).
    by_label: HashMap<String, EncounterTable>,
    /// Substring-match fallbacks, evaluated in registration order.
    fallbacks: Vec<PatternFallback>,
    /// Global default. Applied if no other rule matches and the resolver
    /// returns `Some(default)`. `None` means "no encounters by default".
    default_table: Option<EncounterTable>,
}

impl EncounterRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a table for an exact CDNAME label (e.g. `"map01"`,
    /// `"dolk"`). Subsequent registrations replace the previous binding.
    pub fn register(&mut self, label: impl Into<String>, table: EncounterTable) {
        self.by_label.insert(label.into(), table);
    }

    /// Register a substring-pattern fallback. Patterns are matched
    /// case-insensitively against the scene label; the first registered
    /// pattern that occurs in the label wins.
    pub fn register_pattern(&mut self, pattern: impl Into<String>, table: EncounterTable) {
        let p = pattern.into().to_lowercase();
        self.fallbacks.push(PatternFallback { pattern: p, table });
    }

    /// Set the global default. `None` clears it.
    pub fn set_default(&mut self, table: Option<EncounterTable>) {
        self.default_table = table;
    }

    /// Resolve `scene_label` to an [`EncounterTable`]. Returns `None` when
    /// no rule matches and no default is set.
    pub fn resolve(&self, scene_label: &str) -> Option<&EncounterTable> {
        // 1. Exact label match.
        if let Some(t) = self.by_label.get(scene_label) {
            return Some(t);
        }
        // 2. Pattern fallbacks.
        let lc = scene_label.to_lowercase();
        for rule in &self.fallbacks {
            if lc.contains(&rule.pattern) {
                return Some(&rule.table);
            }
        }
        // 3. Global default.
        self.default_table.as_ref()
    }

    /// Number of exact-label registrations.
    pub fn label_count(&self) -> usize {
        self.by_label.len()
    }

    /// Number of pattern-fallback registrations.
    pub fn pattern_count(&self) -> usize {
        self.fallbacks.len()
    }

    pub fn has_default(&self) -> bool {
        self.default_table.is_some()
    }
}

/// Vanilla scene-pattern registry the engine ships at boot.
///
/// Mirrors the early-game encounter mix observed across the captured area-
/// transition save pairs. Towns and world-map scenes are explicitly
/// suppressed; field
/// scenes (containing `"map"`, `"outskirts"`, `"forest"`, `"cave"`,
/// `"snake"` etc.) get the default early-encounter table.
pub fn vanilla_encounter_registry() -> EncounterRegistry {
    use crate::monster_catalog::default_early_encounter_table;

    let mut r = EncounterRegistry::new();

    // Towns & overworld: no encounters.
    let mut town = EncounterTable::new("(town/world)");
    town.set_trigger_rate(0);
    r.register_pattern("town", town.clone());
    r.register_pattern("world", town.clone());
    r.register_pattern("dolk", town.clone()); // dolk is a town label
    r.register_pattern("hami", town.clone());
    r.register_pattern("rim", town.clone());
    r.register_pattern("vidna", town.clone());
    r.register_pattern("usha", town.clone());
    r.register_pattern("buma", town.clone());
    r.register_pattern("uru", town.clone());
    r.register_pattern("zeto", town.clone());
    // Cutscene labels: never trigger encounters.
    r.register_pattern("op", town.clone()); // op*/edteien etc.
    r.register_pattern("ed", town.clone());

    // Field scenes inherit the default early table.
    r.register_pattern("map", default_early_encounter_table("(field-map)"));
    r.register_pattern("outskirts", default_early_encounter_table("(outskirts)"));
    r.register_pattern("forest", default_early_encounter_table("(forest)"));
    r.register_pattern("cave", default_early_encounter_table("(cave)"));
    r.register_pattern("dome", default_early_encounter_table("(dome)"));
    r.register_pattern("dungeon", default_early_encounter_table("(dungeon)"));

    // Default fallback: no encounters. Engines that want every scene
    // triggerable override via `set_default(Some(default_early_encounter_table(...)))`.
    r.set_default(None);
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encounter::EncounterEntry;

    fn synth_table(name: &str) -> EncounterTable {
        let mut t = EncounterTable::new(name);
        t.set_trigger_rate(8);
        t.push(EncounterEntry::new(1, 50));
        t
    }

    #[test]
    fn exact_label_wins() {
        let mut r = EncounterRegistry::new();
        r.register("map01", synth_table("custom-map01"));
        // pattern match would also fire on "map01" → "map" but exact wins.
        r.register_pattern("map", synth_table("default-field"));
        let t = r.resolve("map01").expect("exact label");
        assert_eq!(t.scene_label, "custom-map01");
    }

    #[test]
    fn pattern_fallback_matches_substring() {
        let mut r = EncounterRegistry::new();
        r.register_pattern("forest", synth_table("forest-default"));
        let t = r.resolve("dark_forest_03").expect("pattern matches");
        assert_eq!(t.scene_label, "forest-default");
    }

    #[test]
    fn pattern_fallback_case_insensitive() {
        let mut r = EncounterRegistry::new();
        r.register_pattern("CAVE", synth_table("cave-default"));
        let t = r.resolve("Lost_Cave").expect("case-insensitive");
        assert_eq!(t.scene_label, "cave-default");
    }

    #[test]
    fn no_match_returns_none_when_no_default() {
        let r = EncounterRegistry::new();
        assert!(r.resolve("anything").is_none());
    }

    #[test]
    fn no_match_returns_default_when_set() {
        let mut r = EncounterRegistry::new();
        r.set_default(Some(synth_table("default")));
        let t = r.resolve("zzz").expect("default");
        assert_eq!(t.scene_label, "default");
    }

    #[test]
    fn first_pattern_wins() {
        let mut r = EncounterRegistry::new();
        r.register_pattern("ma", synth_table("ma-rule"));
        r.register_pattern("map", synth_table("map-rule"));
        // "map01" matches both; first-registered wins.
        let t = r.resolve("map01").expect("first pattern wins");
        assert_eq!(t.scene_label, "ma-rule");
    }

    #[test]
    fn vanilla_registry_disables_encounters_in_towns() {
        let r = vanilla_encounter_registry();
        let t = r.resolve("town01").expect("town pattern hits");
        assert_eq!(t.trigger_rate_q8, 0);
        assert!(t.is_empty());
    }

    #[test]
    fn vanilla_registry_enables_encounters_in_fields() {
        let r = vanilla_encounter_registry();
        let t = r.resolve("map01").expect("map pattern hits");
        assert!(t.trigger_rate_q8 > 0);
        assert!(!t.entries.is_empty());
    }

    #[test]
    fn vanilla_registry_disables_encounters_in_cutscenes() {
        let r = vanilla_encounter_registry();
        let t = r.resolve("opening").expect("op* pattern hits");
        assert_eq!(t.trigger_rate_q8, 0);
        let t2 = r.resolve("edteien").expect("ed* pattern hits");
        assert_eq!(t2.trigger_rate_q8, 0);
    }

    #[test]
    fn vanilla_registry_falls_through_when_no_match() {
        let r = vanilla_encounter_registry();
        // No pattern, no default - None.
        assert!(r.resolve("xxx_unknown_scene").is_none());
    }

    #[test]
    fn registry_label_and_pattern_counts() {
        let mut r = EncounterRegistry::new();
        r.register("a", synth_table("a"));
        r.register("b", synth_table("b"));
        r.register_pattern("p1", synth_table("p1"));
        assert_eq!(r.label_count(), 2);
        assert_eq!(r.pattern_count(), 1);
    }
}
