//! The measured pose of every clip a borrowed NPC placement can play.
//!
//! A pack NPC wears a retail placement's model and plays a record of its
//! scene's ANM bundle ([`crate::map::NpcDef::clip`]). Nothing in the record
//! says whether it is a standing idle, a walk cycle or a seated pose, and a
//! seated clip on a bare tile draws an NPC sitting on air. The table this
//! module reads (`data/npc_postures.toml`) is that fact, measured: each clip
//! posed frame by frame through the placement's own mesh, its height, how far
//! its lowest point rises off the model's standing floor, and how far the
//! lower body travels. The measurement is
//! `legaia_web_expansion::npc_posture`, and a disc-gated test there checks
//! the committed table against it. The table names disc coordinates and clip
//! ids only - no disc bytes.
//!
//! Format and thresholds: `docs/formats/expansion-pack-maps.md` § NPC clip
//! postures.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// The committed table's text.
pub const TABLE_TOML: &str = include_str!("../data/npc_postures.toml");

/// What a clip's pose is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Posture {
    /// Upright, feet on the floor, the lower body still: an idle or a
    /// gesture.
    Standing,
    /// Upright with the lower body travelling: a walk or run cycle.
    Walking,
    /// Lifted off the floor and folded: sits on a seat.
    Seated,
    /// Folded but not lifted: a crouch, kneel or bow.
    Crouched,
}

impl Posture {
    /// The table key / display word.
    pub fn word(self) -> &'static str {
        match self {
            Posture::Standing => "standing",
            Posture::Walking => "walking",
            Posture::Seated => "seated",
            Posture::Crouched => "crouched",
        }
    }
}

/// One placement's clips, grouped by pose.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PlacementPostures {
    /// The base scene (CDNAME label).
    pub scene: String,
    /// The placement's partition-1 index (`DiscRef::Npc::index`).
    pub index: u16,
    /// The placement's own clip id (ANM record + 1), `0` for none.
    pub clip: u8,
    /// Clip ids by pose. Every clip whose bone count matches the model's
    /// object count is listed in exactly one.
    pub standing: Vec<u8>,
    pub walking: Vec<u8>,
    pub seated: Vec<u8>,
    pub crouched: Vec<u8>,
}

impl PlacementPostures {
    /// The pose of clip `clip` (`None` = the placement's own clip). `None`
    /// when the clip is not one the model can play.
    pub fn posture(&self, clip: Option<u8>) -> Option<Posture> {
        let c = clip.unwrap_or(self.clip);
        [
            (Posture::Standing, &self.standing),
            (Posture::Walking, &self.walking),
            (Posture::Seated, &self.seated),
            (Posture::Crouched, &self.crouched),
        ]
        .into_iter()
        .find(|(_, l)| l.contains(&c))
        .map(|(p, _)| p)
    }

    /// The clip ids listed under `posture`.
    pub fn clips(&self, posture: Posture) -> &[u8] {
        match posture {
            Posture::Standing => &self.standing,
            Posture::Walking => &self.walking,
            Posture::Seated => &self.seated,
            Posture::Crouched => &self.crouched,
        }
    }
}

/// The whole table.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PostureTable {
    /// One row per measured placement.
    pub npc: Vec<PlacementPostures>,
}

impl PostureTable {
    /// Parse a table.
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        Ok(toml::from_str(text)?)
    }

    /// Placement `index` of `scene`.
    pub fn get(&self, scene: &str, index: u16) -> Option<&PlacementPostures> {
        self.npc
            .iter()
            .find(|p| p.scene == scene && p.index == index)
    }

    /// The scenes the table covers, each with its row count.
    pub fn scenes(&self) -> BTreeMap<&str, usize> {
        let mut m = BTreeMap::new();
        for p in &self.npc {
            *m.entry(p.scene.as_str()).or_default() += 1;
        }
        m
    }
}

/// The committed table, parsed once.
pub fn table() -> &'static PostureTable {
    static T: OnceLock<PostureTable> = OnceLock::new();
    T.get_or_init(|| PostureTable::parse(TABLE_TOML).expect("data/npc_postures.toml parses"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_table_parses_and_lists_each_clip_once() {
        let t = table();
        assert!(!t.npc.is_empty());
        for p in &t.npc {
            let mut all: Vec<u8> = [
                &p.standing[..],
                &p.walking[..],
                &p.seated[..],
                &p.crouched[..],
            ]
            .concat();
            let n = all.len();
            all.sort_unstable();
            all.dedup();
            assert_eq!(all.len(), n, "{} {}: a clip listed twice", p.scene, p.index);
            assert!(
                !all.contains(&0),
                "{} {}: clip ids are 1-based",
                p.scene,
                p.index
            );
        }
    }

    #[test]
    fn posture_falls_back_to_the_placement_clip() {
        let p = PlacementPostures {
            scene: "s".into(),
            index: 3,
            clip: 7,
            standing: vec![7],
            seated: vec![9],
            ..Default::default()
        };
        assert_eq!(p.posture(None), Some(Posture::Standing));
        assert_eq!(p.posture(Some(9)), Some(Posture::Seated));
        assert_eq!(p.posture(Some(4)), None);
    }
}
