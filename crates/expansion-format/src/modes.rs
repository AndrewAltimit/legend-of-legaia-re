//! `modes.toml`: instances of the port's own game modes a pack defines - a
//! poker table, a card-duel opponent, a golf course, a race track, a
//! prop-hunt arena. The rules live in `legaia_engine_modes`; a pack only
//! picks the numbers and the layout.
//!
//! ```toml
//! [[poker]]
//! id = "sunken-table"
//! seats = 4
//! buy_in = 1000
//! small_blind = 10
//! big_blind = 20
//! currency = "coins"
//! ai_tier = "regular"
//!
//! [[golf]]
//! id = "rim-elm-links"
//! [[golf.holes]]
//! par = 2
//! rows = ["#####", "#T.O#", "#####"]
//! ```
//!
//! Golf hole tile legend (one character per tile): `.` fairway, `,` rough,
//! `s` sand, `w` water, `#` wall, space out of bounds, `T` tee (fairway),
//! `O` cup (fairway), and `^` `v` `<` `>` fairway sloping downhill that way.
//!
//! A course or a track may instead name a pack map (`map = "<id>"`): the
//! game is then played in the 3D field scene the map builds. A golf hole on
//! a map takes its walls and heights from the map's tiles and its surfaces
//! from [`GolfCourse::surfaces`]; a race on a map takes its centerline from
//! [`RaceTrack::waypoints`], in map tiles.
//!
//! ```toml
//! [[golf]]
//! id = "tower-links"
//! map = "tower_green"
//! surfaces = { "," = "rough", "s" = "sand", "w" = "water" }
//! [[golf.holes]]
//! par = 3
//! tee = [2, 2]
//! cup = [9, 5]
//! ```

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::refs::DiscRef;

/// The `modes.toml` section.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModeSection {
    pub poker: Vec<PokerTable>,
    pub card_duels: Vec<CardDuel>,
    pub golf: Vec<GolfCourse>,
    pub races: Vec<RaceTrack>,
    pub prop_hunt: Vec<PropHuntArena>,
}

/// AI seat strength.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiTier {
    Novice,
    #[default]
    Regular,
    Expert,
}

/// What a table's chips are worth.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Currency {
    #[default]
    Gold,
    Coins,
}

/// A Texas Hold'em table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PokerTable {
    pub id: String,
    pub name: String,
    /// Where the table stands (a scene or pack map), for the host.
    pub location: Option<DiscRef>,
    /// Seats at the table including the player, `2..=6`.
    pub seats: u8,
    /// Chips each seat starts with.
    pub buy_in: u32,
    pub small_blind: u32,
    pub big_blind: u32,
    pub currency: Currency,
    /// Currency units per chip.
    pub chip_value: u32,
    pub ai_tier: AiTier,
    /// Stop after this many hands; `None` plays until one seat holds every
    /// chip or the player busts.
    pub max_hands: Option<u32>,
    /// Double the blinds every this many hands.
    pub blind_double_every: Option<u32>,
}

impl Default for PokerTable {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            location: None,
            seats: 4,
            buy_in: 1000,
            small_blind: 10,
            big_blind: 20,
            currency: Currency::Gold,
            chip_value: 1,
            ai_tier: AiTier::Regular,
            max_hands: None,
            blind_double_every: None,
        }
    }
}

/// A trading-card duel opponent.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CardDuel {
    pub id: String,
    pub name: String,
    /// The opponent's five cards (`cards.toml` ids).
    pub deck: Vec<String>,
    pub ai_tier: AiTier,
    /// Grid cells carry elements that strengthen matching cards.
    pub elemental_cells: bool,
    /// Captured cards capture onward once.
    pub chain: bool,
    /// The winner takes one card from the loser's deck.
    pub stakes: bool,
    /// Booster granted to the player on a win.
    pub win_booster: Option<String>,
}

/// A mini-golf course.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GolfCourse {
    pub id: String,
    pub name: String,
    /// The scene or pack map the host stages the course in.
    pub location: Option<DiscRef>,
    /// A pack map (`maps/<id>.toml`) the course is played on, in 3D. Every
    /// hole then takes its layout from the map's tiles ([`GolfHole::tee`],
    /// [`GolfHole::cup`], [`GolfHole::area`]) instead of `rows`.
    pub map: Option<String>,
    /// The spawn the map is entered at (its first when empty).
    pub spawn: String,
    /// Map legend character -> the surface its tiles play as. A character
    /// not listed falls back on its style's `label` (one naming rough,
    /// sand, bunker, water, pond or lava plays as that), else fairway.
    pub surfaces: BTreeMap<String, GolfSurface>,
    pub holes: Vec<GolfHole>,
}

/// What a map tile plays as on a golf hole.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GolfSurface {
    #[default]
    Fairway,
    Rough,
    Sand,
    Water,
    Wall,
    /// Out of bounds: a penalty stroke, like water.
    Out,
}

/// One hole as a tile grid, or as a region of the course's map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GolfHole {
    pub par: u8,
    /// One string per tile row (legend in the module docs).
    pub rows: Vec<String>,
    /// Optional per-tile height digits `0..=9`, same shape as `rows`; the
    /// ball rolls downhill along the height gradient.
    pub heights: Vec<String>,
    /// Strokes before a player picks up; `0` = `par + 5`.
    pub max_strokes: u8,
    /// On a map course: the tee's map tile `[column, row]`.
    pub tee: Option<[u16; 2]>,
    /// On a map course: the cup's map tile.
    pub cup: Option<[u16; 2]>,
    /// On a map course: the inclusive map-tile rectangle
    /// `[col0, row0, col1, row1]` the hole plays in; the whole map when
    /// left out. Everything outside it is out of bounds.
    pub area: Option<[u16; 4]>,
}

impl Default for GolfHole {
    fn default() -> Self {
        Self {
            par: 3,
            rows: Vec::new(),
            heights: Vec::new(),
            max_strokes: 0,
            tee: None,
            cup: None,
            area: None,
        }
    }
}

/// A checkpoint race track: a closed centerline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RaceTrack {
    pub id: String,
    pub name: String,
    pub location: Option<DiscRef>,
    /// A pack map the race runs on, in 3D; the centerline is then
    /// [`Self::waypoints`].
    pub map: Option<String>,
    /// The spawn the map is entered at (its first when empty).
    pub spawn: String,
    pub laps: u8,
    /// Centerline points in world units, in driving order; the last joins
    /// the first. Each is a checkpoint.
    pub points: Vec<[i32; 2]>,
    /// On a map race: the centerline as map tiles `[column, row]` (each
    /// tile's centre), in driving order, closing back on the first.
    pub waypoints: Vec<[u16; 2]>,
    /// Half the track width in world units (in race units on a map race,
    /// where one unit is two field world units).
    pub half_width: u32,
    /// Segment indices (`points[i] -> points[i+1]`) carrying a boost pad.
    pub boost_segments: Vec<u16>,
    pub racers: u8,
    pub ai_tier: AiTier,
}

impl Default for RaceTrack {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            location: None,
            map: None,
            spawn: String::new(),
            laps: 3,
            points: Vec::new(),
            waypoints: Vec::new(),
            half_width: 64,
            boost_segments: Vec::new(),
            racers: 4,
            ai_tier: AiTier::Regular,
        }
    }
}

/// A prop a hider may become.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PropDef {
    pub id: String,
    /// The disc mesh the host draws for it.
    pub mesh: Option<DiscRef>,
    /// Footprint radius in world units.
    pub radius: u32,
}

/// A static decoy prop placed in the arena.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DecoyPlacement {
    pub prop: String,
    pub x: i32,
    pub y: i32,
}

/// A prop-hunt arena and its round rules.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PropHuntArena {
    pub id: String,
    pub name: String,
    /// The scene or pack map the round plays in.
    pub location: Option<DiscRef>,
    pub props: Vec<PropDef>,
    pub decoys: Vec<DecoyPlacement>,
    /// Arena rectangle `[x0, y0, x1, y1]` in world units.
    pub bounds: [i32; 4],
    /// Solid rectangles inside the arena.
    pub blockers: Vec<[i32; 4]>,
    pub rounds: u8,
    pub hunters: u8,
    pub players: u8,
    /// Frames hiders get before hunters are released.
    pub hide_frames: u32,
    /// Frames the hunt lasts.
    pub round_frames: u32,
    pub hunter_health: u32,
    /// Health a hunter loses inspecting a decoy.
    pub miss_penalty: u32,
    pub ai_tier: AiTier,
}

impl Default for PropHuntArena {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            location: None,
            props: Vec::new(),
            decoys: Vec::new(),
            bounds: [0, 0, 2048, 2048],
            blockers: Vec::new(),
            rounds: 3,
            hunters: 1,
            players: 4,
            hide_frames: 30 * 60,
            round_frames: 180 * 60,
            hunter_health: 100,
            miss_penalty: 10,
            ai_tier: AiTier::Regular,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_partial_instances_with_defaults() {
        let text = r#"
[[poker]]
id = "sunken-table"
seats = 4
currency = "coins"

[[golf]]
id = "links"
[[golf.holes]]
par = 2
rows = ['#####', '#T.O#', '#####']

[[races]]
id = "loop"
points = [[0, 0], [1000, 0], [1000, 1000]]

[[prop_hunt]]
id = "yard"
props = [{ id = "barrel", radius = 32 }]
"#;
        let s: ModeSection = toml::from_str(text).unwrap();
        assert_eq!(s.poker[0].big_blind, 20);
        assert_eq!(s.poker[0].currency, Currency::Coins);
        assert_eq!(s.golf[0].holes[0].rows.len(), 3);
        assert_eq!(s.races[0].laps, 3);
        assert_eq!(s.prop_hunt[0].props[0].radius, 32);
        assert_eq!(s.prop_hunt[0].hunter_health, 100);
    }

    #[test]
    fn parses_map_courses_and_tracks() {
        let text = r#"
[[golf]]
id = "green"
map = "tower_green"
surfaces = { "," = "rough", "~" = "water" }
[[golf.holes]]
par = 3
tee = [2, 2]
cup = [9, 5]
area = [1, 1, 10, 6]

[[races]]
id = "ring"
map = "tower_ring"
waypoints = [[2, 2], [12, 2], [12, 10], [2, 10]]
"#;
        let s: ModeSection = toml::from_str(text).unwrap();
        let g = &s.golf[0];
        assert_eq!(g.map.as_deref(), Some("tower_green"));
        assert_eq!(g.surfaces.get("~"), Some(&GolfSurface::Water));
        assert_eq!(g.holes[0].cup, Some([9, 5]));
        assert_eq!(g.holes[0].area, Some([1, 1, 10, 6]));
        assert!(g.holes[0].rows.is_empty());
        assert_eq!(s.races[0].waypoints.len(), 4);
        assert_eq!(s.races[0].map.as_deref(), Some("tower_ring"));
    }
}
