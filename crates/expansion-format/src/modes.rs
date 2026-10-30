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

use crate::audio::SoundRef;
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
    /// Party nights: playlists of short rounds of the instances above.
    pub party_nights: Vec<PartyNight>,
}

impl ModeSection {
    /// Whether some instance of any mode has id `id`: what a map's `mode`
    /// event, a dialogue choice's `mode` and a card reward name.
    pub fn has_instance(&self, id: &str) -> bool {
        self.poker.iter().any(|m| m.id == id)
            || self.card_duels.iter().any(|m| m.id == id)
            || self.golf.iter().any(|m| m.id == id)
            || self.races.iter().any(|m| m.id == id)
            || self.prop_hunt.iter().any(|m| m.id == id)
            || self.party_nights.iter().any(|m| m.id == id)
    }

    /// The party night `id`.
    pub fn party_night(&self, id: &str) -> Option<&PartyNight> {
        self.party_nights.iter().find(|n| n.id == id)
    }
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
    /// A pack map (`maps/<id>.toml`) the table is played on, at
    /// [`Self::table`]: the cards and chips are laid on a table in the 3D
    /// field scene with the other seats' NPCs around it. Without a map (or a
    /// table) the game is its 2D screen.
    pub map: Option<String>,
    /// The spawn the map is entered at (its first when empty).
    pub spawn: String,
    /// Where the table stands on [`Self::map`] and who sits at it.
    pub table: Option<PokerTableLayout>,
}

/// A poker table on a pack map.
///
/// ```toml
/// [poker.table]
/// tile = [9, 8]
/// facing = 4
/// players = [{ npc = "gambler", think = 3, fold = 5, all_in = 4 }]
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PokerTableLayout {
    /// Map-local tile the table is centred over.
    pub tile: [u16; 2],
    /// World units from the tile centre `[x, y, z]`, y up; `y` raises the
    /// whole table.
    pub offset: [i32; 3],
    /// The compass sector (`0..8`) the player looks across the table in.
    pub facing: u8,
    /// The table top's height above the floor, world units.
    pub height: u16,
    /// The felt's half depth (toward the player), world units; its half
    /// width is 1.35 times that.
    pub radius: u16,
    /// Draw the built-in table; off when a prop of the map is the table.
    pub draw_table: bool,
    /// The map NPCs seated in the other seats, in seat order with the
    /// player's seat skipped. A seat with none sits empty-chaired.
    pub players: Vec<DuelSeatNpc>,
    /// Map NPCs watching from where they stand.
    pub spectators: Vec<DuelSeatNpc>,
    /// The felt sampled from the scene's own VRAM instead of the built-in
    /// green.
    pub surface: Option<DuelSurface>,
    /// Retail sound cue ids: `deal` a card, `place` chips, `flip` a board
    /// card, `win` / `lose` a pot.
    pub sfx: DuelSfx,
}

impl Default for PokerTableLayout {
    fn default() -> Self {
        Self {
            tile: [0, 0],
            offset: [0, 0, 0],
            facing: 0,
            height: 64,
            radius: 130,
            draw_table: true,
            players: Vec::new(),
            spectators: Vec::new(),
            surface: None,
            sfx: DuelSfx::default(),
        }
    }
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
            map: None,
            spawn: String::new(),
            table: None,
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
    /// A pack map (`maps/<id>.toml`) the duel is played on, at
    /// [`Self::table`]: the board and cards are laid on a table in the 3D
    /// field scene, with the opponent seated across it. Without a map (or a
    /// table) the duel is its 2D screen.
    pub map: Option<String>,
    /// The spawn the map is entered at (its first when empty).
    pub spawn: String,
    /// Where the table stands on [`Self::map`] and who sits at it.
    pub table: Option<DuelTable>,
}

/// A Grid Duel's table on a pack map.
///
/// ```toml
/// [card_duels.table]
/// tile = [4, 9]                 # the map tile the board sits over
/// offset = [0, 0, 32]           # world units from the tile centre, y up
/// facing = 4                    # compass sector the player looks across
/// height = 64                   # the table top above the floor
/// opponent = { npc = "warden", think = 2, win = 5, lose = 6 }
/// spectators = [{ npc = "gambler", cheer = 3, slump = 4 }]
/// sfx = { place = { kind = "sfx", id = 32 }, flip = { kind = "sound", id = "card_flip" } }
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DuelTable {
    /// Map-local tile the board is centred over.
    pub tile: [u16; 2],
    /// World units from the tile centre `[x, y, z]`, y up; `y` raises the
    /// whole table.
    pub offset: [i32; 3],
    /// The compass sector (`0..8`, a spawn's `facing` table) the player
    /// looks in across the board, toward the opponent's seat.
    pub facing: u8,
    /// The table top's height above the floor, world units.
    pub height: u16,
    /// The board's side, world units.
    pub board: u16,
    /// How far each seat sits from the board's centre, world units.
    pub seat_distance: u16,
    /// Draw a table under the board (a wooden top on four legs). Leave it
    /// off when a prop of the map already is the table.
    pub draw_table: bool,
    /// The map NPC seated across the board.
    pub opponent: Option<DuelSeatNpc>,
    /// Map NPCs watching from where they stand: each turns to the table and
    /// reacts to big moves and the result.
    pub spectators: Vec<DuelSeatNpc>,
    /// The board's playing surface sampled from the scene's own VRAM (a
    /// floor or cloth texture of the base scene), instead of the built-in
    /// felt.
    pub surface: Option<DuelSurface>,
    /// Retail sound cue ids the table plays.
    pub sfx: DuelSfx,
}

impl Default for DuelTable {
    fn default() -> Self {
        Self {
            tile: [0, 0],
            offset: [0, 0, 0],
            facing: 0,
            height: 64,
            board: 132,
            seat_distance: 120,
            draw_table: true,
            opponent: None,
            spectators: Vec::new(),
            surface: None,
            sfx: DuelSfx::default(),
        }
    }
}

/// A map NPC at a duel or poker table and the clips it reacts with (records of the
/// base scene's ANM bundle, the [`crate::map::NpcDef::clip`] numbering).
/// A clip left out keeps the NPC's idle.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DuelSeatNpc {
    /// The map NPC id (`[[npcs]]` of the duel's map).
    pub npc: String,
    /// The name the table's banners and results card give the seat
    /// ("The Warden's Second"); empty keeps "Rival" / "Seat N".
    pub name: String,
    /// Played while the opponent thinks (the opponent only).
    pub think: Option<u8>,
    /// Played on a big capture by this NPC's side and on its win (a
    /// spectator cheers for the player).
    pub cheer: Option<u8>,
    /// Played on a big loss and on its defeat.
    pub slump: Option<u8>,
    /// Poker: played as the seat folds.
    pub fold: Option<u8>,
    /// Poker: played while the seat is all in.
    pub all_in: Option<u8>,
}

/// A duel table's playing surface: one texture rectangle of the map's base
/// scene VRAM, tiled once per board cell.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DuelSurface {
    /// GP0 texpage word (TSB) of the texture.
    pub tpage: u16,
    /// GP0 CLUT word (CBA).
    pub clut: u16,
    /// `[u, v, w, h]` inside the page.
    pub uv: [u8; 4],
}

/// The sounds a duel or poker table plays: retail cues
/// (`{ kind = "sfx", id = N }`, `docs/formats/sfx-table.md` ids) or the
/// pack's own (`{ kind = "sound", id = "..." }`, `audio.toml`). Each
/// defaults to a menu cue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DuelSfx {
    /// A card dealt or lifted from the hand.
    pub deal: SoundRef,
    /// A card laid on a cell; chips pushed out.
    pub place: SoundRef,
    /// A captured card turning over; a board card turned up.
    pub flip: SoundRef,
    /// The player won.
    pub win: SoundRef,
    /// The player lost or drew.
    pub lose: SoundRef,
}

impl Default for DuelSfx {
    fn default() -> Self {
        // The menu cursor, confirm and cancel cues (`menu_cues`).
        let cue = |id| SoundRef::Sfx { id };
        Self {
            deal: cue(0x21),
            place: cue(0x20),
            flip: cue(0x21),
            win: cue(0x20),
            lose: cue(0x37),
        }
    }
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
    /// On a map course: the map NPC ids (`[[npcs]]`) that stand in as the
    /// other seats' golfers, in seat order with the player's seat skipped.
    /// A seat with none draws as a stand-in figure.
    pub bodies: Vec<String>,
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
    /// On a map race: the map NPC ids (`[[npcs]]`) that drive the other
    /// seats' racers, in seat order with the player's seat skipped; each
    /// runs its `walk_clip` while it moves. A seat with none draws as a
    /// stand-in figure.
    pub bodies: Vec<String>,
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
            bodies: Vec::new(),
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

/// A party night: an ordered (or shuffled) playlist of short rounds, each
/// a variant of one of this section's instances, scored by placement into
/// one points table, with prizes for the top places.
///
/// ```toml
/// [[party_nights]]
/// id = "game-night"
/// name = "Game Night"
/// points = [10, 6, 4, 2]
/// double_final = true
/// [[party_nights.rounds]]
/// mode = "windmill-links"
/// holes = [1]
/// [[party_nights.rounds]]
/// mode = "mist-lantern-table"
/// hands = 3
/// stack = 300
/// [[party_nights.prizes]]
/// place = 1
/// gold = 500
/// boosters = ["mistborn"]
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PartyNight {
    pub id: String,
    pub name: String,
    /// Where the night is hosted (a scene or pack map), for the host.
    pub location: Option<DiscRef>,
    /// Play the rounds in file order, or shuffled by the night's seed.
    pub order: PartyOrder,
    pub rounds: Vec<PartyRound>,
    /// Points by placement, first place first; a place past the end scores
    /// nothing. Tied seats share a place and its points.
    pub points: Vec<u32>,
    /// Catch-up: before every round after the first, the player in last
    /// place picks which of the remaining rounds comes next.
    pub last_place_picks: bool,
    /// Catch-up: the last round scores double.
    pub double_final: bool,
    /// Seats in a night played alone (the player and AI rivals), `2..=6`.
    pub players: u8,
    /// The AI rivals' names in a night played alone, in seat order.
    pub rivals: Vec<String>,
    /// How hard the rivals play.
    pub ai_tier: AiTier,
    pub prizes: Vec<PartyPrize>,
}

impl Default for PartyNight {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            location: None,
            order: PartyOrder::Fixed,
            rounds: Vec::new(),
            points: vec![10, 6, 4, 2],
            last_place_picks: false,
            double_final: true,
            players: 4,
            rivals: Vec::new(),
            ai_tier: AiTier::Regular,
            prizes: Vec::new(),
        }
    }
}

/// How a party night orders its rounds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartyOrder {
    #[default]
    Fixed,
    Shuffle,
}

/// One round of a party night: instance `mode`, cut short. A knob that does
/// not fit the instance's kind is a validation error.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PartyRound {
    /// The instance (`[[poker]]`, `[[card_duels]]`, `[[golf]]`, `[[races]]`
    /// or `[[prop_hunt]]` id).
    pub mode: String,
    /// The round's title; the instance's name when empty.
    pub name: String,
    /// Poker: hands dealt.
    pub hands: Option<u32>,
    /// Poker: the stack every seat starts with.
    pub stack: Option<u32>,
    /// Golf: the holes played, 1-based, in order; every hole when empty.
    pub holes: Vec<u8>,
    /// Race: laps.
    pub laps: Option<u8>,
    /// Prop hunt: rounds (each player hunts at most once).
    pub rounds: Option<u8>,
    /// The round scores double.
    pub double: bool,
    /// A wager round: every player stakes this percentage (`0..=50`) of
    /// their points into a pot the round's winners split.
    pub wager: u8,
}

/// What a final place wins. Each player's own game grants its own prize.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PartyPrize {
    /// 1-based final place; tied players share it.
    pub place: u8,
    pub gold: u32,
    pub coins: u32,
    /// `cards.toml` booster ids, one pack each.
    pub boosters: Vec<String>,
    /// `cards.toml` card ids, one copy each.
    pub cards: Vec<String>,
}

/// The kind of instance `id` names in `s`, as the party rules read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstanceKind {
    Poker,
    Duel,
    Golf { holes: usize },
    Race,
    PropHunt,
}

fn instance_kind(s: &ModeSection, id: &str) -> Option<InstanceKind> {
    if s.poker.iter().any(|m| m.id == id) {
        Some(InstanceKind::Poker)
    } else if s.card_duels.iter().any(|m| m.id == id) {
        Some(InstanceKind::Duel)
    } else if let Some(c) = s.golf.iter().find(|m| m.id == id) {
        Some(InstanceKind::Golf {
            holes: c.holes.len(),
        })
    } else if s.races.iter().any(|m| m.id == id) {
        Some(InstanceKind::Race)
    } else if s.prop_hunt.iter().any(|m| m.id == id) {
        Some(InstanceKind::PropHunt)
    } else {
        None
    }
}

/// `modes.toml`'s rules: the party nights. (Instance ids are the section's
/// own, hyphenated ones included, so they are checked here rather than
/// declared as pack-wide content ids.)
impl crate::validate::SectionRules for ModeSection {
    fn check(&self, cx: &mut crate::validate::Checker<'_>) {
        crate::validate::check_duel_tables(self, cx);
        let mut seen = std::collections::BTreeSet::new();
        for (i, n) in self.party_nights.iter().enumerate() {
            let at = format!("party_nights[{i}]");
            if n.id.is_empty() || !seen.insert(n.id.as_str()) {
                cx.error(
                    &at,
                    format!("party night id {:?} is empty or repeated", n.id),
                );
            }
            if instance_kind(self, &n.id).is_some() {
                cx.error(
                    &format!("{at}.id"),
                    format!("{:?} is already a mode instance's id", n.id),
                );
            }
            if let Some(r) = &n.location {
                cx.disc_ref(&format!("{at}.location"), r);
            }
            if n.rounds.is_empty() {
                cx.error(
                    &format!("{at}.rounds"),
                    "a party night plays at least one round",
                );
            }
            if n.points.iter().all(|&p| p == 0) {
                cx.warning(&format!("{at}.points"), "no place scores any points");
            }
            if !(2..=6).contains(&n.players) {
                cx.error(&format!("{at}.players"), "a night seats 2..=6 players");
            }
            if n.rivals.len() + 1 > usize::from(n.players.max(2)) {
                cx.warning(
                    &format!("{at}.rivals"),
                    "more rival names than a night played alone seats",
                );
            }
            for (j, r) in n.rounds.iter().enumerate() {
                let at = format!("{at}.rounds[{j}]");
                let Some(kind) = instance_kind(self, &r.mode) else {
                    cx.error(
                        &format!("{at}.mode"),
                        format!(
                            "no poker, duel, golf, race or prop hunt instance {:?}",
                            r.mode
                        ),
                    );
                    continue;
                };
                let mut misfit = |field: &str, set: bool, fits: bool| {
                    if set && !fits {
                        cx.error(
                            &format!("{at}.{field}"),
                            format!("`{field}` does not apply to instance {:?}", r.mode),
                        );
                    }
                };
                let poker = kind == InstanceKind::Poker;
                misfit("hands", r.hands.is_some(), poker);
                misfit("stack", r.stack.is_some(), poker);
                misfit(
                    "holes",
                    !r.holes.is_empty(),
                    matches!(kind, InstanceKind::Golf { .. }),
                );
                misfit("laps", r.laps.is_some(), kind == InstanceKind::Race);
                misfit("rounds", r.rounds.is_some(), kind == InstanceKind::PropHunt);
                if let InstanceKind::Golf { holes } = kind {
                    for (k, &h) in r.holes.iter().enumerate() {
                        if h == 0 || usize::from(h) > holes {
                            cx.error(
                                &format!("{at}.holes[{k}]"),
                                format!("hole {h}: {:?} has holes 1..={holes}", r.mode),
                            );
                        }
                    }
                }
                if r.hands == Some(0)
                    || r.stack == Some(0)
                    || r.laps == Some(0)
                    || r.rounds == Some(0)
                {
                    cx.error(
                        &at,
                        "a round's hands, stack, laps and rounds are at least 1",
                    );
                }
                if r.wager > 50 {
                    cx.error(&format!("{at}.wager"), "a wager stakes 0..=50 percent");
                }
            }
            for (j, p) in n.prizes.iter().enumerate() {
                let at = format!("{at}.prizes[{j}]");
                if p.place == 0 {
                    cx.error(&format!("{at}.place"), "places count from 1");
                }
                let cards = &cx.pack().cards;
                let missing_boosters: Vec<String> = p
                    .boosters
                    .iter()
                    .filter(|b| cards.booster(b).is_none())
                    .cloned()
                    .collect();
                let missing_cards: Vec<String> = p
                    .cards
                    .iter()
                    .filter(|c| cards.card(c).is_none())
                    .cloned()
                    .collect();
                for b in missing_boosters {
                    cx.error(&format!("{at}.boosters"), format!("no booster {b:?}"));
                }
                for c in missing_cards {
                    cx.error(&format!("{at}.cards"), format!("no card {c:?}"));
                }
            }
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

    #[test]
    fn parses_a_party_night_with_defaults() {
        let text = r#"
[[party_nights]]
id = "night"
[[party_nights.rounds]]
mode = "links"
holes = [2]
[[party_nights.rounds]]
mode = "table"
hands = 3
stack = 300
wager = 25
[[party_nights.prizes]]
place = 1
gold = 500
boosters = ["b"]
"#;
        let s: ModeSection = toml::from_str(text).unwrap();
        let n = s.party_night("night").unwrap();
        assert_eq!(n.points, vec![10, 6, 4, 2]);
        assert!(n.double_final && !n.last_place_picks);
        assert_eq!(n.order, PartyOrder::Fixed);
        assert_eq!(n.rounds[0].holes, vec![2]);
        assert_eq!(n.rounds[1].wager, 25);
        assert_eq!(n.prizes[0].boosters, vec!["b"]);
        assert!(s.has_instance("night"));
    }
}
