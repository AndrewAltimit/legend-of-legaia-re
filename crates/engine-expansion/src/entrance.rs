//! Map entrances: the tile rectangles in retail field scenes that lead into
//! a pack's maps (`[[entrances]]` of `maps/<id>.toml`).
//!
//! The scene host feeds an [`EntranceWatch`] the scene the player is in and
//! the field-grid tile under them once per field tick ([`EntranceWatch::step`]).
//! An entrance fires on **entry**, as a map zone and a retail walk-on trigger
//! do: stepping onto one of its tiles from outside its rectangle. The first
//! tick in a scene only records the tile, so arriving on an entrance (the
//! map's own exit lands beside it) does not bounce the player straight back.
//!
//! An entrance is shut while its `requires` does not hold; the gate is the
//! quest prerequisite test ([`crate::quest::QuestRuntime::prerequisites_hold`]),
//! handed in as a closure so this module stays free of the quest state.

use legaia_expansion_format::ExpansionPack;
use legaia_expansion_format::map::{EntranceDef, field_rect_contains};

/// An entrance the player stepped onto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntranceHit {
    /// The pack map it enters.
    pub map: String,
    /// The spawn the player arrives at (empty: the map's first).
    pub spawn: String,
    /// The entrance's id, for logs.
    pub id: String,
}

/// The per-scene entrance tracker, seated on `ExpansionState::entrances`.
#[derive(Debug, Clone, Default)]
pub struct EntranceWatch {
    /// The scene the last step ran in.
    scene: String,
    /// The tile the last step saw the player on.
    last_tile: Option<(u8, u8)>,
}

impl EntranceWatch {
    /// Forget the last scene, so the next step only records the tile.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// One field tick in `scene` with the player on field-grid tile `tile`.
    /// Returns the entrance whose rectangle the player just stepped into, if
    /// it is open (`open(entrance)`).
    pub fn step(
        &mut self,
        pack: &ExpansionPack,
        scene: &str,
        tile: Option<(u8, u8)>,
        open: impl Fn(&EntranceDef) -> bool,
    ) -> Option<EntranceHit> {
        if self.scene != scene {
            self.scene = scene.to_string();
            self.last_tile = tile;
            return None;
        }
        let last = std::mem::replace(&mut self.last_tile, tile);
        let (x, z) = tile?;
        if last == Some((x, z)) {
            return None;
        }
        for (map_id, def) in &pack.maps {
            for e in &def.entrances {
                if e.scene_name() != Some(scene) || !field_rect_contains(&e.rect, x, z) {
                    continue;
                }
                let from_outside =
                    last.is_none_or(|(lx, lz)| !field_rect_contains(&e.rect, lx, lz));
                if from_outside && open(e) {
                    return Some(EntranceHit {
                        map: map_id.clone(),
                        spawn: e.spawn.clone(),
                        id: e.id.clone(),
                    });
                }
            }
        }
        None
    }
}

/// Every entrance of `pack` in `scene`, with the map it leads to.
pub fn entrances_in<'a>(
    pack: &'a ExpansionPack,
    scene: &'a str,
) -> impl Iterator<Item = (&'a str, &'a EntranceDef)> + 'a {
    pack.maps.iter().flat_map(move |(id, def)| {
        def.entrances
            .iter()
            .filter(move |e| e.scene_name() == Some(scene))
            .map(move |e| (id.as_str(), e))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_expansion_format::map::MapDef;

    fn pack() -> ExpansionPack {
        let map: MapDef = toml::from_str(
            r#"
name = "Hall"
base = { scene = { kind = "scene", name = "tower" } }
[tiles]
rows = ["..."]
[[spawns]]
id = "door"
tile = [1, 0]
[[entrances]]
id = "well"
scene = { kind = "scene", name = "town01" }
rect = [10, 20, 11, 20]
spawn = "door"
[[entrances]]
id = "gated"
scene = { kind = "scene", name = "town01" }
rect = [40, 40, 40, 40]
requires = { flags = ["open"] }
"#,
        )
        .expect("map parses");
        let mut p = ExpansionPack::default();
        p.maps.insert("hall".into(), map);
        p
    }

    #[test]
    fn stepping_onto_the_rect_from_outside_fires_once() {
        let p = pack();
        let mut w = EntranceWatch::default();
        let open = |_: &EntranceDef| true;
        // The first tick in the scene records the tile only.
        assert_eq!(w.step(&p, "town01", Some((9, 20)), open), None);
        let hit = w.step(&p, "town01", Some((10, 20)), open).expect("fires");
        assert_eq!((hit.map.as_str(), hit.spawn.as_str()), ("hall", "door"));
        // Standing still, and walking within the rect, fire nothing.
        assert_eq!(w.step(&p, "town01", Some((10, 20)), open), None);
        assert_eq!(w.step(&p, "town01", Some((11, 20)), open), None);
        // Leaving and coming back fires again.
        assert_eq!(w.step(&p, "town01", Some((12, 20)), open), None);
        assert!(w.step(&p, "town01", Some((11, 20)), open).is_some());
    }

    #[test]
    fn arriving_on_the_rect_does_not_fire() {
        let p = pack();
        let mut w = EntranceWatch::default();
        assert_eq!(w.step(&p, "town01", Some((10, 20)), |_| true), None);
        assert_eq!(w.step(&p, "town01", Some((10, 20)), |_| true), None);
    }

    #[test]
    fn other_scenes_and_shut_entrances_stay_quiet() {
        let p = pack();
        let mut w = EntranceWatch::default();
        assert_eq!(w.step(&p, "town02", Some((9, 20)), |_| true), None);
        assert_eq!(w.step(&p, "town02", Some((10, 20)), |_| true), None);
        let mut w = EntranceWatch::default();
        let shut = |e: &EntranceDef| e.requires.flags.is_empty();
        assert_eq!(w.step(&p, "town01", Some((39, 40)), shut), None);
        assert_eq!(w.step(&p, "town01", Some((40, 40)), shut), None);
        assert_eq!(entrances_in(&p, "town01").count(), 2);
    }
}
