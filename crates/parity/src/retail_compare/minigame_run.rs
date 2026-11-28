//! Minigame states (`OTHER MODE`, game mode `0x19`): which minigame a capture
//! runs, how far the engine can be seeded into it, and the channels it is
//! scored on.
//!
//! The minigame is named by the mode-24 door warp's sub-id `_DAT_8007BA34`
//! (field-VM op `0x3E`, `0x801E07B8`), which selects the overlay
//! ([`legaia_engine_mode::minigame_entry::MinigameSubId`]); the scene label
//! stays the departure scene's. The seed lands the save in that scene and
//! takes the same warp (`World::request_minigame_warp`), so the engine enters
//! the session a cabinet installs.
//!
//! Only a session whose captured phase the warp itself reaches is seeded: a
//! phase that needs played input past the entry carries `rand()` history (a
//! slot machine's reel stops) the seed does not replay. That is the slot
//! machine at rest (reel state word `DAT_801D3C84 = 1`, the attract state the
//! init advances to), the dance's count-in (states `3..=5` of `FUN_801CF470`),
//! the contest hub's first visit up to the fight (`FUN_801CF870`), the fishing
//! venue's idle shore (state `0xC` of `FUN_801CF3BC`) and the Baka Fighter's
//! title card (cabinet state `1` of `FUN_801CF388`).

use super::*;
use legaia_engine_mode::minigame_entry::MinigameSubId;

/// The mode-24 door warp's sub-id (`_DAT_8007BA34`, `sh` at `0x801E07B8`).
pub(super) const MINIGAME_SUB_ID: u32 = 0x8007_BA34;
/// The slot machine's reel state word (`DAT_801D3C84`, PROT 0975).
const SLOT_STATE: u32 = 0x801D_3C84;
/// The slot machine's balance (`DAT_801D4114`), assigned from the coin bank
/// at entry.
const SLOT_BALANCE: u32 = 0x801D_4114;
/// The casino coin bank (`_DAT_800845A4`).
const COIN_BANK: u32 = 0x8008_45A4;
/// The battle-return flag the slot init tests (`_DAT_8007B8B8`).
const BATTLE_RETURN_FLAG: u32 = 0x8007_B8B8;
/// The slot machine's attract state.
const SLOT_IDLE: i32 = 1;
/// The slot machine's cash-out picker cursor (`DAT_801D4110`).
const SLOT_CURSOR: u32 = 0x801D_4110;
/// A menu drive's pad edges, `(tick offset, button)` from the first press.
type Presses = Vec<(u64, legaia_engine_system::input::PadButton)>;
/// The cash-out picker (`0x32`) and the two rules pages (`0x35`, `0x36`).
const SLOT_MENU_STATES: [i32; 3] = [0x32, 0x35, 0x36];
/// Ticks a rules page's fade takes before its Cross reads (`0x10` a frame
/// to and from black, `0x33..0x35`).
const SLOT_PAGE_GAP: u64 = 60;
/// The dance's game state (`DAT_801D5334`, PROT 0980) and the READY banner
/// counter its state `3` draws (`DAT_801D5158`, `0x801D015C..0x801D0184`).
const DANCE_STATE: u32 = 0x801D_5334;
const DANCE_COUNTER: u32 = 0x801D_5158;
/// The contest hub's arm byte (`DAT_801D1A78`, PROT 0977) and the intro hold
/// counter arm `1` runs in `0x801D1A84`.
const HUB_ARM: u32 = 0x801D_1A78;
const HUB_HOLD: u32 = 0x801D_1A84;
/// The departure scene's define, backed up by the mode-24 init.
const DEPARTURE_DEFINE: u32 = 0x8007_BAC4;
/// The fishing driver's state word (`DAT_801D926C`, PROT 0972).
const FISHING_STATE: u32 = 0x801D_926C;
/// The prize list's cursor (`DAT_801D90A8`), its quantity (`0x801D90B0`),
/// the confirm's row (`0x801D90B4`, `0` Yes) and the hub menu's cursor
/// (`0x801D912C`).
const FISHING_LIST_CURSOR: u32 = 0x801D_90A8;
const FISHING_QTY: u32 = 0x801D_90B0;
const FISHING_CONFIRM_ROW: u32 = 0x801D_90B4;
const FISHING_HUB_CURSOR: u32 = 0x801D_912C;
/// The tackle list's cursor (`0x801D90DC`) and the equipped lure
/// (`_DAT_80084450`), which the hub copies into it as the list opens.
const FISHING_TACKLE_CURSOR: u32 = 0x801D_90DC;
const FISHING_LURE: u32 = 0x8008_4450;
/// The exchange's three screens: the list, the quantity picker, the confirm.
const FISHING_EXCHANGE_STATES: std::ops::RangeInclusive<u32> = 0x78..=0x7A;
/// Ticks past the warp the exchange drive's first press waits: the shore is
/// idle well before it, and the hub opens only from there.
const FISHING_DRIVE_FIRST: u64 = 200;
/// Ticks between two of its presses: each is a one-tick edge the hub and the
/// exchange read on the tick it lands.
const FISHING_DRIVE_GAP: u64 = 6;
/// Ticks the drive's last screen settles before the frame.
const FISHING_DRIVE_SETTLE: u64 = 30;
/// The dance's `GO!` accumulator (`DAT_801D515C`).
const DANCE_GO: u32 = 0x801D_515C;
/// The dance's song timer (`DAT_801D5820`): `dt * 10` a game frame in the play
/// states (`FUN_801CF470`'s tail).
const DANCE_SONG: u32 = 0x801D_5820;
/// The human dancer's score, press latch and miss counter (`DAT_801D53CC`,
/// `DAT_801D548C`, `DAT_801D568C`): all zero on a floor the pad never reached.
const DANCE_HUMAN_WORDS: [u32; 3] = [0x801D_53CC, 0x801D_548C, 0x801D_568C];
/// The dance's main play state (`DAT_801D5334 = 0xA`).
const DANCE_PLAYING: u32 = 0xA;
/// The hub's arm-`5` hold (`0x801D1A70`).
const HUB_CARD_HOLD: u32 = 0x801D_1A70;
/// The Baka Fighter cabinet's state word (`DAT_801DBF44`, PROT 0976).
const BAKA_STATE: u32 = 0x801D_BF44;
/// The title card's clock (`DAT_801DBE94`), the `t` `FUN_801D59D4` draws from.
const BAKA_CARD_CLOCK: u32 = 0x801D_BE94;
/// The player-select cursor (`DAT_801DBF70`).
const BAKA_SELECT_CURSOR: u32 = 0x801D_BF70;
/// The player-select state (`0x0B`).
const BAKA_SELECT: u32 = 0x0B;
/// Ticks from the attract card's Start to the select screen taking the pad:
/// the fade-out (`0x02`, to blink phase `0x3D`) and the lineup (`0x0A`).
const BAKA_SELECT_GAP: u64 = 150;
/// The tick the seed takes the warp on, after the scene entry settles and
/// ahead of any talk the save's seat would walk into: `koin1`'s sign-up
/// counter opens its refusal line at tick `31` when the player is left there,
/// where retail's warp is the last op of the record that seated him.
const WARP_TICK: u64 = 20;
/// Ticks the entry may take to reach the captured phase.
const ENTRY_DEADLINE: u64 = 900;
/// The displayed frame is the one built a game frame (two vsyncs) before the
/// RAM's, so the replay takes its frame where the lineup stands that much
/// short of the cursors the RAM holds.
const BAKA_DISPLAY_LAG_VSYNCS: i32 = 2;
/// Ticks past the select's settle the replay may wait for its lineup to
/// reach retail's idle frames: two full cycles of the three clips' joint
/// period (`lcm(17, 15)` frames of two vsyncs).
const BAKA_LINEUP_DEADLINE: u64 = 2 * 2 * 17 * 15;
/// Ticks a dance played hands-off may take to reach the captured song frame:
/// the song's `0x64FC` timer at `30` a game frame of three vsyncs, past the
/// count-in.
const DANCE_SONG_DEADLINE: u64 = 4000;
/// The image child's frame, past the warp: the captures sample the attract
/// state 300 vsyncs after the field was left.
const FRAME_TICKS_PAST_WARP: u64 = 300;

/// The minigame a capture runs and its scored words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetailMinigame {
    /// `_DAT_8007BA34`.
    pub sub_id: i16,
    /// The slot machine's state word and balance, when the slot overlay is
    /// the one resident.
    pub slot: Option<(i32, i32)>,
    /// `_DAT_800845A4`.
    pub coin_bank: u32,
    /// The battle-return flag `_DAT_8007B8B8` is up: the overlay was entered
    /// through the door path, which assigns the balance from the bank.
    pub door_entry: bool,
    /// The dance's, the hub's or the fishing driver's state word with its
    /// phase counter (the READY banner counter, the Welcome hold), when that
    /// overlay is the one resident.
    pub phase: Option<(u32, u32)>,
    /// The phase's second counter: the dance's `GO!` accumulator
    /// `DAT_801D515C`, the hub's arm-`5` hold `0x801D1A70`.
    pub phase2: u32,
    /// The departure scene's define the mode-24 init backed up
    /// (`_DAT_8007BAC4`): the venue overlays load their own block over the
    /// scene word, and the warp starts from this one.
    pub departure_define: u16,
    /// The dance's marker tiles as the displayed frame drew them, by script
    /// row: cursor, countdown and mesh ([`displayed_dance_markers`]).
    pub dance_markers: [Option<[i16; 3]>; legaia_engine_field::dance_marker::MARKER_SCRIPT_ROWS],
    /// The hall's animated props' clip cursors as the RAM holds them, one per
    /// clip ([`dance_prop_clips`]).
    pub dance_props: [Option<(u8, i16)>; DANCE_PROP_CLIPS],
    /// The dance's song timer and whether the human dancer's words show a
    /// press (score, latch or miss non-zero), when the dance is the overlay
    /// resident.
    pub dance_song: Option<(u32, bool)>,
    /// The fishing exchange's words, when the fishing overlay is resident.
    pub fishing_exchange: Option<FishingExchangeWords>,
    /// The slot machine's cash-out picker cursor, when its overlay is
    /// resident.
    pub slot_cursor: u32,
    /// The Baka Fighter's player-select cursor, when its overlay is resident.
    pub baka_cursor: u32,
    /// The player select's three lineup fighters as `(idle frame, actor
    /// step)` by roster (Vahn, Noa, Gala): each actor's clip cursor `+0x68`
    /// over sixteen and its step `+0x6A` ([`baka_lineup`]).
    pub baka_lineup: Option<[(usize, i32); 3]>,
}

/// Where the select's state spawns each lineup fighter: the position records
/// at `0x801DBC04` (Vahn `(0, -1000)`, Noa `(350, -600)`, Gala `(-350,
/// -600)`), with clip `1 + 9 * column` at `+0x5C`.
const BAKA_LINEUP_SEATS: [(i16, i16); 3] = [(0, -1000), (350, -600), (-350, -600)];

/// The lineup actors' clip cursors: found by the seat (`+0x14` / `+0x18`) and
/// the idle clip (`+0x5C`) the state wrote, in the actor pool. The cursors are
/// the select's only clock - nothing else counts the lineup's age - so the
/// replay places its frame by them.
fn baka_lineup(ram: &[u8]) -> Option<[(usize, i32); 3]> {
    const POOL: std::ops::Range<u32> = 0x8008_0000..0x8009_0000;
    let mut out = [None; 3];
    for a in POOL.step_by(4) {
        let h = |off: u32| game_anchors::u16_at(ram, a + off) as i16;
        for (r, &(x, z)) in BAKA_LINEUP_SEATS.iter().enumerate() {
            if out[r].is_none() && h(0x14) == x && h(0x18) == z && h(0x5C) == 1 + 9 * r as i16 {
                out[r] = Some((
                    usize::from(game_anchors::u16_at(ram, a + 0x68)) >> 4,
                    i32::from(h(0x6A)),
                ));
            }
        }
    }
    Some([out[0]?, out[1]?, out[2]?])
}

/// The fishing prize exchange's words a capture on one of its screens holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FishingExchangeWords {
    /// `DAT_801D90A8`, the list cursor.
    pub list_cursor: u32,
    /// `0x801D90B0`, the picker's quantity.
    pub qty: i32,
    /// `0x801D90B4`, the confirm's row (`0` Yes, `1` No).
    pub confirm_row: u32,
    /// `0x801D912C`, the hub menu's cursor (row `1` opens the help pages,
    /// row `2` the tackle list, row `3` the prize list).
    pub hub_cursor: u32,
    /// `0x801D90DC`, the tackle list's cursor.
    pub tackle_cursor: u32,
    /// `_DAT_80084450`, the equipped lure the tackle list opens on.
    pub lure: u32,
}

/// The pad edges that walk the engine from the idle shore to the hub screen
/// `state` holds, as `(tick offset, button)` from the first press: Triangle
/// opens the hub (`& 0x110` at `0x801CF9EC`), Down to its row, Cross opens
/// the row's screen. Help page 1 is a Cross past page 0; the tackle list
/// opens on the equipped lure and Down walks it to the cursor. On the prize
/// list: Up from the list's entry row to the cursor (the list opens on the
/// overlay's zeroed cursor, which its entry clamp sends to row `5`); Cross
/// opens the picker on quantity `1`, Right to the quantity; Cross opens the
/// confirm on No, Up to Yes.
fn fishing_exchange_presses(state: u32, w: &FishingExchangeWords) -> Presses {
    use legaia_engine_system::input::PadButton as B;
    let mut seq = vec![B::Triangle];
    seq.extend(std::iter::repeat_n(B::Down, w.hub_cursor as usize));
    seq.push(B::Cross);
    if state == 0x66 {
        seq.push(B::Cross);
    }
    if state == 0x6E {
        seq.extend(std::iter::repeat_n(
            B::Down,
            w.tackle_cursor.saturating_sub(w.lure) as usize,
        ));
    }
    if !FISHING_EXCHANGE_STATES.contains(&state) {
        return (0u64..)
            .zip(seq)
            .map(|(i, b)| (i * FISHING_DRIVE_GAP, b))
            .collect();
    }
    seq.extend(std::iter::repeat_n(
        B::Up,
        5usize.saturating_sub(w.list_cursor as usize),
    ));
    if state >= 0x79 {
        seq.push(B::Cross);
        seq.extend(std::iter::repeat_n(B::Right, w.qty.max(1) as usize - 1));
    }
    if state >= 0x7A {
        seq.push(B::Cross);
        if w.confirm_row == 0 {
            seq.push(B::Up);
        }
    }
    (0u64..)
        .zip(seq)
        .map(|(i, b)| (i * FISHING_DRIVE_GAP, b))
        .collect()
}

/// The hall's animated plain floor actors (tick `FUN_801D4098`) by clip: each
/// clip's `+0x68` cursor. The floor pass spawns them all on one pass at one
/// rate, so the actors of a clip agree; the first found stands for them.
fn dance_prop_clips(ram: &[u8]) -> [Option<(u8, i16)>; DANCE_PROP_CLIPS] {
    let mut out: Vec<(u8, i16)> = Vec::new();
    for n in crate::retail_compare_script::actor_nodes(ram) {
        if game_anchors::u32_at(ram, n + 0x0C) != legaia_engine_minigames::dance::HALL_PROP_TICK {
            continue;
        }
        let Ok(clip) = u8::try_from(game_anchors::i16_at(ram, n + 0x5C)) else {
            continue;
        };
        if clip == 0 || out.iter().any(|&(c, _)| c == clip) {
            continue;
        }
        out.push((clip, game_anchors::i16_at(ram, n + 0x68)));
    }
    out.sort_unstable();
    let mut clips = [None; DANCE_PROP_CLIPS];
    for (slot, c) in clips.iter_mut().zip(out) {
        *slot = Some(c);
    }
    clips
}

/// How many distinct prop clips the seat carries; the hall runs three.
const DANCE_PROP_CLIPS: usize = 8;

/// The marker tile tick (`FUN_801D0640`, PROT 0980).
const DANCE_MARKER_TICK: u32 = 0x801D_0640;
/// Vsyncs a dance game frame steps (`DAT_1F800393`), which ages a tile's
/// countdown; in every dance state the library holds.
const DANCE_FRAME_STEP: i16 = 3;

/// The dance's marker tiles on the frame the capture displays: the RAM's, taken
/// back the display lag's passes. A tile's flipbook is a clock of the hall's
/// game frames, and a game frame's length varies with what it draws: between
/// `minigame_dance_retail_go_in` and `_go_out` the `GO!` accumulator walks `11`
/// steps of `3` vsyncs while the ambient leader takes `8` rows of one pass each.
/// So the count-in's own counter does not name a tile's phase, and the seat reads
/// it.
fn displayed_dance_markers(
    ram: &[u8],
) -> [Option<[i16; 3]>; legaia_engine_field::dance_marker::MARKER_SCRIPT_ROWS] {
    use legaia_engine_field::dance_marker::{MARKER_SCRIPT_ROW_BYTES, MARKER_SCRIPT_VA};
    let mut out = [None; legaia_engine_field::dance_marker::MARKER_SCRIPT_ROWS];
    for n in crate::retail_compare_script::actor_nodes(ram) {
        if game_anchors::u32_at(ram, n + 0x0C) != DANCE_MARKER_TICK {
            continue;
        }
        let class = game_anchors::u16_at(ram, n + 0x50);
        let Some(slot) = out.get_mut(usize::from(class)) else {
            continue;
        };
        let row = MARKER_SCRIPT_VA + u32::from(class) * MARKER_SCRIPT_ROW_BYTES as u32;
        let pair = |i: i32| {
            let a = row + (i * 4) as u32;
            (
                game_anchors::i16_at(ram, a),
                game_anchors::i16_at(ram, a + 2),
            )
        };
        // The row's length: the sentinel that wraps the cursor.
        let len = (0..32).find(|&i| pair(i).0 < 0).unwrap_or(25);
        let mut cursor = i32::from(game_anchors::u16_at(ram, n + 0x9C));
        let mut timer = game_anchors::i16_at(ram, n + 0x54);
        let bias = legaia_scene_formats::field_objects::FIELD_ACTOR_PACK_BIAS as i16;
        let mut mesh = game_anchors::i16_at(ram, n + 0x64);
        for _ in 0..crate::retail_compare_battle::DISPLAY_LAG_PASSES {
            let current = (cursor / 2 - 1).rem_euclid(len);
            if timer + DANCE_FRAME_STEP <= pair(current).1 {
                timer += DANCE_FRAME_STEP;
            } else {
                // The pass that staged this step: before it, the last step
                // stood at zero with its cursor here.
                cursor = current * 2;
                mesh = pair((current - 1).rem_euclid(len)).0 + bias;
                timer = 0;
            }
        }
        *slot = Some([cursor as i16, timer, mesh]);
    }
    out
}

impl RetailMinigame {
    pub(super) fn from_ram(ram: &[u8]) -> Self {
        let sub_id = game_anchors::i16_at(ram, MINIGAME_SUB_ID);
        let slot = (u8::try_from(sub_id)
            .ok()
            .and_then(MinigameSubId::from_sub_id)
            == Some(MinigameSubId::SlotMachine))
        .then(|| {
            (
                game_anchors::u32_at(ram, SLOT_STATE) as i32,
                game_anchors::u32_at(ram, SLOT_BALANCE) as i32,
            )
        });
        let minigame = u8::try_from(sub_id)
            .ok()
            .and_then(MinigameSubId::from_sub_id);
        let phase = match minigame {
            Some(MinigameSubId::Dance) => Some((
                game_anchors::u32_at(ram, DANCE_STATE),
                game_anchors::u32_at(ram, DANCE_COUNTER),
            )),
            Some(MinigameSubId::MuscleDome) => Some((
                u32::from(game_anchors::u32_at(ram, HUB_ARM) as u8),
                game_anchors::u32_at(ram, HUB_HOLD),
            )),
            Some(MinigameSubId::Fishing) => Some((game_anchors::u32_at(ram, FISHING_STATE), 0)),
            Some(MinigameSubId::BakaFighter) => Some((
                game_anchors::u32_at(ram, BAKA_STATE),
                game_anchors::u32_at(ram, BAKA_CARD_CLOCK),
            )),
            _ => None,
        };
        let phase2 = match minigame {
            Some(MinigameSubId::Dance) => game_anchors::u32_at(ram, DANCE_GO),
            Some(MinigameSubId::MuscleDome) => game_anchors::u32_at(ram, HUB_CARD_HOLD),
            _ => 0,
        };
        Self {
            sub_id,
            slot,
            coin_bank: game_anchors::u32_at(ram, COIN_BANK),
            door_entry: game_anchors::u32_at(ram, BATTLE_RETURN_FLAG) != 0,
            phase,
            phase2,
            departure_define: game_anchors::u32_at(ram, DEPARTURE_DEFINE) as u16,
            dance_markers: if minigame == Some(MinigameSubId::Dance) {
                displayed_dance_markers(ram)
            } else {
                Default::default()
            },
            dance_props: if minigame == Some(MinigameSubId::Dance) {
                dance_prop_clips(ram)
            } else {
                [None; DANCE_PROP_CLIPS]
            },
            baka_cursor: if minigame == Some(MinigameSubId::BakaFighter) {
                game_anchors::u32_at(ram, BAKA_SELECT_CURSOR)
            } else {
                0
            },
            baka_lineup: (minigame == Some(MinigameSubId::BakaFighter)
                && phase.map(|p| p.0) == Some(BAKA_SELECT))
            .then(|| baka_lineup(ram))
            .flatten(),
            slot_cursor: if minigame == Some(MinigameSubId::SlotMachine) {
                game_anchors::u32_at(ram, SLOT_CURSOR)
            } else {
                0
            },
            fishing_exchange: (minigame == Some(MinigameSubId::Fishing)).then(|| {
                FishingExchangeWords {
                    list_cursor: game_anchors::u32_at(ram, FISHING_LIST_CURSOR),
                    qty: game_anchors::u32_at(ram, FISHING_QTY) as i32,
                    confirm_row: game_anchors::u32_at(ram, FISHING_CONFIRM_ROW),
                    hub_cursor: game_anchors::u32_at(ram, FISHING_HUB_CURSOR),
                    tackle_cursor: game_anchors::u32_at(ram, FISHING_TACKLE_CURSOR),
                    lure: game_anchors::u32_at(ram, FISHING_LURE),
                }
            }),
            dance_song: (minigame == Some(MinigameSubId::Dance)).then(|| {
                (
                    game_anchors::u32_at(ram, DANCE_SONG),
                    DANCE_HUMAN_WORDS
                        .iter()
                        .any(|&a| game_anchors::u32_at(ram, a) != 0),
                )
            }),
        }
    }

    /// The exchange screen the capture holds and the presses that reach it,
    /// when it holds one the hub's row 3 opened.
    fn fishing_exchange_drive(&self) -> Option<(u32, Presses)> {
        let state = self.phase.map(|p| p.0)?;
        let w = self.fishing_exchange?;
        let reached = match state {
            0x65 | 0x66 => w.hub_cursor == 1,
            0x6E => w.hub_cursor == 2 && w.tackle_cursor >= w.lure,
            s if FISHING_EXCHANGE_STATES.contains(&s) => w.hub_cursor == 3 && w.list_cursor <= 5,
            _ => false,
        };
        reached.then(|| (state, fishing_exchange_presses(state, &w)))
    }

    /// The slot machine's menu screen the capture holds and the presses that
    /// reach it from the machine at rest: Triangle opens the cash-out picker
    /// on row `0` (`& 0x110`, state `1`), Down to its cursor; row `2`'s Cross
    /// fades into the rules' first page, and a Cross once it has faded in
    /// turns to the payout chart. Only a machine whose balance is still the
    /// bank's - never played - is seeded, as at rest.
    fn slot_drive(&self) -> Option<(u32, Presses)> {
        use legaia_engine_system::input::PadButton as B;
        let (state, balance) = self.slot?;
        if !SLOT_MENU_STATES.contains(&state)
            || !self.door_entry
            || i64::from(balance) != i64::from(self.coin_bank)
        {
            return None;
        }
        let mut out = vec![(0, B::Triangle)];
        let rows = if state == 0x32 { self.slot_cursor } else { 2 };
        let mut t = 0;
        for _ in 0..rows.min(2) {
            t += FISHING_DRIVE_GAP;
            out.push((t, B::Down));
        }
        if state != 0x32 {
            t += FISHING_DRIVE_GAP;
            out.push((t, B::Cross));
        }
        if state == 0x36 {
            t += SLOT_PAGE_GAP;
            out.push((t, B::Cross));
        }
        Some((state as u32, out))
    }

    /// The Baka Fighter's player select and the presses that reach it from
    /// the attract card: Start (`& 0x844` in state `1`), then, once the
    /// lineup takes the pad, Right to the cursor. The lineup's one `rand()`
    /// (`0x801CF88C`) lands in a focus id no fighter carries, so the screen
    /// does not depend on it.
    fn baka_drive(&self) -> Option<(u32, Presses)> {
        use legaia_engine_system::input::PadButton as B;
        if self.minigame() != Some(MinigameSubId::BakaFighter)
            || self.phase.map(|p| p.0) != Some(BAKA_SELECT)
            || self.baka_cursor > 2
        {
            return None;
        }
        let mut out = vec![(0, B::Start)];
        for i in 0..u64::from(self.baka_cursor) {
            out.push((BAKA_SELECT_GAP + i * FISHING_DRIVE_GAP, B::Right));
        }
        Some((BAKA_SELECT, out))
    }

    /// The pad drive a menu screen's capture is seeded by
    /// ([`Self::fishing_exchange_drive`], [`Self::slot_drive`]), and the
    /// ticks its last screen settles.
    fn drive(&self) -> Option<(u32, Presses, u64)> {
        if let Some((s, p)) = self.fishing_exchange_drive() {
            return Some((s, p, FISHING_DRIVE_SETTLE));
        }
        if let Some((s, p)) = self.baka_drive() {
            // The last Right, or the Start alone, then the lineup's settle.
            let settle = if p.len() > 1 {
                SLOT_PAGE_GAP
            } else {
                BAKA_SELECT_GAP
            };
            return Some((s, p, settle));
        }
        self.slot_drive().map(|(s, p)| (s, p, SLOT_PAGE_GAP))
    }

    fn minigame(&self) -> Option<MinigameSubId> {
        u8::try_from(self.sub_id)
            .ok()
            .and_then(MinigameSubId::from_sub_id)
    }

    /// The overlay the sub-id names, for the film.
    pub(super) fn minigame_id(&self) -> Option<MinigameSubId> {
        self.minigame()
    }

    /// Why the state is not seeded, or `None` when it is.
    pub fn unseeded_reason(&self) -> Option<String> {
        match (self.minigame(), self.slot) {
            // The door path's init assigns the balance from the coin bank
            // (`FUN_801CEC94`); with the battle-return flag clear it is the
            // 70-coin dev fallback instead, a launch outside the casino door
            // the warp does not take.
            (Some(MinigameSubId::SlotMachine), Some(_)) if !self.door_entry => Some(
                "slot machine launched outside the casino door (battle-return flag \
                 _DAT_8007B8B8 clear, the 70-coin dev balance)"
                    .to_string(),
            ),
            // Only a spin or a payout moves the balance off the bank: a
            // machine at rest off it has been played.
            (Some(MinigameSubId::SlotMachine), Some((SLOT_IDLE, balance)))
                if i64::from(balance) == i64::from(self.coin_bank) =>
            {
                None
            }
            // The cash-out picker and the rules pages of a machine never
            // played: pad history from the rest state, no rand() in it.
            (Some(MinigameSubId::SlotMachine), Some(_)) if self.slot_drive().is_some() => None,
            (Some(MinigameSubId::SlotMachine), Some((SLOT_IDLE, balance))) => Some(format!(
                "slot machine at rest after play (balance {balance}, coin bank {}): the reel \
                 positions and the balance are rand() history the seed does not replay",
                self.coin_bank
            )),
            (Some(MinigameSubId::SlotMachine), Some((state, _))) => Some(format!(
                "slot machine in reel state {state:#X}: past the attract state the reel stops \
                 and feature rolls are rand() history the seed does not replay"
            )),
            // State `0xC`, the idle shore: the entry itself lands there, and
            // no cast has rolled anything. State `0xB` is the fade in front
            // of it, whose frame still holds what stood before.
            (Some(MinigameSubId::Fishing), _) if matches!(self.phase, Some((0xC, _))) => None,
            // The hub's screens - the help pages, the tackle list and the
            // exchange's - reached from the idle shore through the hub's
            // rows: pad history the drive replays, no rand() in it.
            (Some(MinigameSubId::Fishing), _) if self.fishing_exchange_drive().is_some() => None,
            (Some(MinigameSubId::Fishing), _) => Some(
                "fishing past the idle shore: the cast, its drift and the bite rolls are \
                 rand() history the seed does not replay"
                    .to_string(),
            ),
            // States `3..=5`: the count-in the warp plays out on its own.
            (Some(MinigameSubId::Dance), _) if matches!(self.phase, Some((3..=5, _))) => None,
            // The play loop on a floor the pad never reached: the hall, the
            // rivals and the camera are a function of the song's game frames,
            // which a hands-off run from the count-in reaches (the minigame
            // film pairs every rival and camera word through the song).
            (Some(MinigameSubId::Dance), _)
                if matches!(self.phase, Some((DANCE_PLAYING, _)))
                    && matches!(self.dance_song, Some((_, false))) =>
            {
                None
            }
            (Some(MinigameSubId::Dance), _) => Some(
                "dance past the count-in: the chart's presses and the rivals' rolls are \
                 history the seed does not replay"
                    .to_string(),
            ),
            // The first visit's arms, no pad in them.
            (Some(MinigameSubId::MuscleDome), _)
                if matches!(self.phase, Some((0..=5 | 0x15, _))) =>
            {
                None
            }
            (Some(MinigameSubId::MuscleDome), _) => Some(
                "contest hub past the first visit: the legs are played input the seed does \
                 not replay"
                    .to_string(),
            ),
            // State `1`: the title card, before any press.
            (Some(MinigameSubId::BakaFighter), _) if matches!(self.phase, Some((1, _))) => None,
            // The player select: the attract card's Start and the cursor.
            (Some(MinigameSubId::BakaFighter), _) if self.baka_drive().is_some() => None,
            (Some(MinigameSubId::BakaFighter), _) => Some(
                "Baka Fighter past the title card: the rounds are pad input and the AI's \
                 rand() picks"
                    .to_string(),
            ),
            (Some(m), _) => Some(format!(
                "no minigame session seeding path for {m:?} (sub-id {})",
                self.sub_id
            )),
            (None, _) => Some(format!("minigame sub-id {} names no overlay", self.sub_id)),
        }
    }
}

/// Whether the engine's session stands at the phase the capture holds.
fn at_captured_phase(world: &legaia_engine_core::world::World, mg: &RetailMinigame) -> bool {
    use legaia_engine_minigames::muscle_ringside::FirstVisitArm;
    let m = &world.minigames;
    match (mg.minigame(), mg.phase) {
        (Some(MinigameSubId::SlotMachine), _) => {
            // A menu screen's drive starts from the machine at rest.
            let want = match mg.slot_drive() {
                Some(_) => Some(SLOT_IDLE),
                None => mg.slot.map(|s| s.0),
            };
            world.mode == SceneMode::SlotMachine
                && m.slot_machine.as_ref().map(|s| s.retail_state()) == want
        }
        (Some(MinigameSubId::Fishing), _) => {
            world.mode == SceneMode::Fishing
                && m.fishing
                    .as_ref()
                    .is_some_and(|f| f.phase() == legaia_engine_fishing::fishing::PondPhase::Idle)
        }
        (Some(MinigameSubId::Dance), Some((DANCE_PLAYING, _))) => {
            // The song timer moves `dt * 10` a game frame; the frame on screen
            // stands the display lag's game frames behind the RAM's.
            let lag = crate::retail_compare_battle::DISPLAY_LAG_PASSES as i64
                * i64::from(DANCE_FRAME_STEP)
                * 10;
            let song = mg.dance_song.map_or(0, |s| i64::from(s.0));
            world.mode == SceneMode::Dance
                && m.dance_countin.is_none()
                && m.dance
                    .as_ref()
                    .is_some_and(|g| i64::from(g.song_timer()) >= song - lag)
        }
        (Some(MinigameSubId::Dance), Some((state, counter))) => {
            use legaia_engine_minigames::dance::CountInStage as S;
            // The frame on screen stands the display lag behind the RAM's
            // counters, each `DANCE_FRAME_STEP` a game frame.
            let lag = i64::from(
                crate::retail_compare_battle::DISPLAY_LAG_PASSES as i16 * DANCE_FRAME_STEP,
            );
            // The `GO!` accumulator moves `dt * 2` a game frame (`0x801CFC08`
            // / `0x801CFC5C`), twice the counter's step; the view carries it
            // doubled again, as the brightness `acc * 2` the emit takes.
            let go = i64::from(mg.phase2);
            let acc = |b: i32| i64::from(b) / 2;
            world.mode == SceneMode::Dance
                && m.dance_countin.as_ref().is_some_and(|c| match state {
                    3 => i64::from(c.frame()) >= i64::from(counter) - lag,
                    4 => {
                        c.stage() == S::GoIn && c.view().go.is_some_and(|b| acc(b) >= go - 2 * lag)
                    }
                    _ => {
                        c.stage() == S::GoOut && c.view().go.is_some_and(|b| acc(b) <= go + 2 * lag)
                    }
                })
        }
        (Some(MinigameSubId::BakaFighter), Some((_, clock))) => {
            // The select's drive starts from the attract card.
            let clock = if mg.baka_drive().is_some() { 0 } else { clock };
            world.mode == SceneMode::BakaFighter
                && m.baka_fighter.as_ref().is_some_and(|f| {
                    f.cabinet().state() == 1
                        && i64::from(f.cabinet().intro_clock()) >= i64::from(clock)
                })
        }
        (Some(MinigameSubId::MuscleDome), Some((arm, count))) => {
            world.mode == SceneMode::MuscleDome
                && m.muscle_hub
                    .first_visit
                    .as_ref()
                    .is_some_and(|h| match arm {
                        0 => h.arm() == FirstVisitArm::IntroIn,
                        1 => h.arm() == FirstVisitArm::IntroHold && h.counter() >= count as i32,
                        2 => h.arm() == FirstVisitArm::IntroOut,
                        3 => h.arm() == FirstVisitArm::TitleZoom,
                        4 => h.arm() == FirstVisitArm::CardIn && h.counter() >= count as i32,
                        5 => h.arm() == FirstVisitArm::CardHold && h.hold() <= mg.phase2 as i32,
                        _ => h.arm() == FirstVisitArm::RoundIn && h.counter() >= count as i32,
                    })
        }
        _ => false,
    }
}

/// Whether the session stands where the capture's first vsync does, for the
/// film: the dance's banner counter at the capture's own value (no display
/// lag, the film pairs RAM words with RAM words); every other minigame at its
/// still's phase.
fn at_film_phase(world: &legaia_engine_core::world::World, mg: &RetailMinigame) -> bool {
    match (mg.minigame(), mg.phase) {
        (Some(MinigameSubId::Dance), Some((3, counter))) => {
            world.mode == SceneMode::Dance
                && world
                    .minigames
                    .dance_countin
                    .as_ref()
                    .is_some_and(|c| i64::from(c.frame()) >= i64::from(counter))
        }
        _ => at_captured_phase(world, mg),
    }
}

/// The scene the warp departs from: the label, or for a venue overlay that
/// loaded its own block over the scene word, the mode-24 init's backup.
fn departure_scene(
    retail: &RetailObs,
    mg: &RetailMinigame,
    cdname: &legaia_prot::cdname::IndexMap,
) -> String {
    match mg.minigame() {
        Some(MinigameSubId::Fishing) => cdname
            .get(&u32::from(mg.departure_define))
            .cloned()
            .unwrap_or_else(|| retail.scene.clone()),
        _ => retail.scene.clone(),
    }
}

/// The casino seats' latches (`koin1` P1[0] `+0xEF..0x10C`, the floor's
/// re-engage of P1[51..=56]): a raised one parks the floor's track.
const CASINO_SEAT_LATCHES: [u16; 3] = [0x4B6, 0x4B7, 0x4B8];

/// The engine side of a seeded minigame state.
struct EngineMinigame {
    mode: SceneMode,
    scene: String,
    slot: Option<(i32, i32)>,
    coin_bank: u32,
    /// The tick the captured phase was reached on, past the warp.
    reached: Option<u64>,
    /// After the exchange drive: the exchange's screen as the retail state
    /// word (`0x78..=0x7A`, `0` closed), its list cursor, quantity and
    /// confirm row.
    exchange: Option<(u32, u32, i32, u32)>,
    /// After a slot menu drive: the cash-out picker's row, while it is up.
    slot_cursor: Option<u32>,
    /// After a Baka Fighter drive: the cabinet state and the select cursor.
    baka: Option<(u32, i32)>,
    /// Ticks past the drive's settle the select's lineup took to reach
    /// retail's idle frames (`baka_lineup`); the image takes its frame there
    /// too.
    baka_lineup_ticks: u64,
}

fn run_engine_minigame(
    extracted: &Path,
    retail: &RetailObs,
    departure: &str,
    film: Option<&super::minigame_film::MinigameFilm>,
    mut listen: Option<&mut crate::audio_sweep::EngineAudio>,
) -> Result<EngineMinigame> {
    let mg = retail.minigame.context("no minigame observables")?;
    let sub = mg.minigame().context("sub-id names no overlay")?;
    let cfg = BootConfig {
        scene: departure.to_string(),
        enable_audio: listen.is_some(),
    };
    let mut session = if listen.is_some() {
        NativeSession::open_extracted_detached_audio(extracted, &cfg)
    } else {
        NativeSession::open(extracted, &cfg)
    }
    .context("open boot session")?;
    let listening = listen.is_some();
    let mut director = super::engine_run::RecordingDirector::default();
    // The audio sweep's listener: the hosts' audio frame after every tick,
    // the track words routed as both hosts route them.
    let mut tick = |session: &mut NativeSession| -> Result<()> {
        session.tick()?;
        if let Some(audio) = listen.as_deref_mut() {
            // The minigame arms' own audio steps, which both play hosts run
            // each frame after the tick: the duel's announcer lines and the
            // dome hub's cues (the dance's venue step the film runs).
            session.sound_baka_announcer();
            session.sound_muscle_hub();
            super::engine_run::route_bgm(session, &mut director)?;
            crate::audio_sweep::listen_frame(session, audio);
            if std::env::var_os("LEGAIA_SWEEP_LISTEN_TICKS").is_some() {
                eprintln!("listen tick {}", audio.ticks);
            }
        }
        Ok(())
    };
    crate::retail_compare::pin_seed_retail(&mut session.host.world.toggles, false);
    let mut save = retail
        .seed_save()
        .context("retail state has no liftable save window")?;
    // The casino seat's latch is the seat's own record's, raised after the
    // floor's entry chose the track: lowered for the entry, the floor plays
    // the `2018` it played on retail where its raised arms park the track
    // (`koin1` P1[0] `+0xEF..0x10C`). The listener's seat alone.
    let seat_latches: Vec<u16> = if listening && departure == "koin1" {
        CASINO_SEAT_LATCHES
            .iter()
            .copied()
            .filter(|&f| super::save_flags::system_flag_held(&save, f))
            .collect()
    } else {
        Vec::new()
    };
    super::save_flags::clear_system_flags(&mut save, &seat_latches);
    let _landing = session.resume_save(save, departure, &FieldLiveOpts::default());
    for _ in 0..WARP_TICK {
        tick(&mut session)?;
    }
    for &f in &seat_latches {
        session.host.world.system_flag_set(f);
    }
    session.host.world.request_minigame_warp(sub.sub_id());
    let mut reached = None;
    let deadline = match mg.phase {
        Some((DANCE_PLAYING, _)) if sub == MinigameSubId::Dance => DANCE_SONG_DEADLINE,
        _ => ENTRY_DEADLINE,
    };
    for t in 0..deadline {
        tick(&mut session)?;
        if film.is_some() {
            // The film runs the venue step both play hosts run.
            let _ = session.step_dance_tail();
        }
        let at = match film {
            Some(_) => at_film_phase(&session.host.world, &mg),
            None => at_captured_phase(&session.host.world, &mg),
        };
        if at {
            reached = Some(t);
            if listening && std::env::var_os("LEGAIA_SWEEP_LISTEN_TICKS").is_some() {
                eprintln!(
                    "minigame listen: the phase at listener tick {}",
                    WARP_TICK + t + 1
                );
            }
            break;
        }
    }
    let mut exchange = None;
    let mut slot_cursor = None;
    let mut baka = None;
    let mut baka_lineup_ticks = 0u64;
    if let (Some((_, presses, settle)), Some(at)) = (mg.drive(), reached) {
        // The drive's presses, from the first-press tick past the warp; the
        // hub, the exchange and the cash-out picker read the raw pad edge.
        let first = FISHING_DRIVE_FIRST.max(at + 1);
        let end = first + presses.last().map_or(0, |p| p.0) + settle;
        for t in (at + 1)..end {
            let mask = presses
                .iter()
                .filter(|p| first + p.0 == t)
                .fold(0u16, |m, p| m | p.1.mask());
            session.host.world.input.set_pad(mask);
            tick(&mut session)?;
        }
        session.host.world.input.set_pad(0);
        // The player select's lineup clips are the frame's clock: tick on
        // until the replay's three fighters stand on retail's idle frames
        // (`baka_lineup`), and take the frame there.
        if let Some(lineup) = mg.baka_lineup {
            for _ in 0..BAKA_LINEUP_DEADLINE {
                let on_frame = session
                    .host
                    .world
                    .minigames
                    .baka_fighter
                    .as_ref()
                    .and_then(|f| {
                        let (_, clock) = f.select_lineup()?;
                        Some(lineup.iter().enumerate().all(|(r, &(frame, step))| {
                            f.roster_idle_header(r).is_some_and(|h| {
                                legaia_engine_minigames::baka_fighter::lineup_idle_frame(
                                    &h,
                                    step,
                                    clock + BAKA_DISPLAY_LAG_VSYNCS,
                                ) == frame
                            })
                        }))
                    })
                    .unwrap_or(true);
                if on_frame {
                    break;
                }
                tick(&mut session)?;
                baka_lineup_ticks += 1;
            }
        }
        let m = &session.host.world.minigames;
        baka = m
            .baka_fighter
            .as_ref()
            .map(|f| (f.cabinet().state(), f.cabinet().select_cursor()));
        slot_cursor = m
            .slot_machine
            .as_ref()
            .filter(|s| s.retail_state() == 0x32)
            .map(|s| s.menu_cursor());
        use legaia_engine_fishing::fishing::PrizeScreen;
        use legaia_engine_fishing::fishing_hub::HubScreen;
        let hub = m.fishing.as_ref().and_then(|s| s.hub());
        exchange = Some(match &m.fishing_exchange {
            None => match hub {
                Some(h) => match h.screen {
                    HubScreen::Help(0) => (0x65, 0, 0, 0),
                    HubScreen::Help(_) => (0x66, 0, 0, 0),
                    HubScreen::Tackle => (0x6E, h.tackle.cursor.max(0) as u32, 0, 0),
                    _ => (0x64, 0, 0, 0),
                },
                None => (0, 0, 0, 0),
            },
            Some(ex) => {
                let d = &m.fishing_exchange_dialog;
                let state = match d.screen {
                    PrizeScreen::List => 0x78,
                    PrizeScreen::Quantity => 0x79,
                    PrizeScreen::Confirm => 0x7A,
                };
                (state, ex.cursor as u32, d.qty, d.confirm_row)
            }
        });
    }
    // The listener hears the phase for as long as it holds, up to the span
    // the field sample listens across: a start the phase's own arm queued
    // (the shore's venue track) is routed and sounds, and a phase that moves
    // on (the dance's count-in) is not listened past.
    // `LEGAIA_SWEEP_LISTEN_TICKS=<n>`: listen `n` ticks past the phase
    // whatever it does, for a cue timeline against a retail key-on census
    // run from the same state (harness).
    let listen_on: Option<u32> = std::env::var("LEGAIA_SWEEP_LISTEN_TICKS")
        .ok()
        .and_then(|v| v.parse().ok());
    if listening && (mg.drive().is_none() || listen_on.is_some()) {
        session.host.world.input.set_pad(0);
        let span = listen_on.map_or(crate::audio_sweep::LISTEN_SPAN, |n| n as usize);
        for _ in 0..span {
            tick(&mut session)?;
            if listen_on.is_none() && !at_captured_phase(&session.host.world, &mg) {
                break;
            }
        }
    }
    if let Some(film) = film {
        film.run(
            &mut session,
            &mg,
            WARP_TICK + 1 + reached.unwrap_or(ENTRY_DEADLINE),
            retail.rand_seed,
        )?;
    }
    if let Some(audio) = listen {
        crate::audio_sweep::finish_listen(&session, audio);
        crate::cue_differential::probe(&mut session, audio);
    }
    let w = &session.host.world;
    Ok(EngineMinigame {
        mode: w.mode,
        scene: session
            .host
            .scene
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_default(),
        slot: w
            .minigames
            .slot_machine
            .as_ref()
            .map(|m| (m.retail_state(), m.balance())),
        coin_bank: w.minigames.casino_coins,
        reached,
        exchange,
        slot_cursor,
        baka,
        baka_lineup_ticks,
    })
}

/// Seed a minigame state with the audio director attached and listen to it
/// (the audio sweep's minigame half): the same seed, warp and drive as the
/// image corpus takes, the hosts' audio frame after every tick.
pub fn run_minigame_listening(
    extracted: &Path,
    retail: &RetailObs,
    cdname: &legaia_prot::cdname::IndexMap,
    probe: Option<crate::cue_differential::CueProbe>,
) -> Result<crate::audio_sweep::EngineAudio> {
    let mg = retail.minigame.context("no minigame observables")?;
    if let Some(why) = mg.unseeded_reason() {
        anyhow::bail!(why);
    }
    let departure = departure_scene(retail, &mg, cdname);
    let mut audio = crate::audio_sweep::EngineAudio::default();
    audio.cue_probe = probe;
    let engine = run_engine_minigame(extracted, retail, &departure, None, Some(&mut audio))?;
    anyhow::ensure!(
        engine.reached.is_some(),
        "the engine never reached the captured phase"
    );
    Ok(audio)
}

/// Seed a minigame state and score it: `mode`, `scene`, `minigame` (the
/// session's own words) and `image`.
pub(super) fn run_minigame(
    opts: &RunOptions<'_>,
    entry: &CorpusEntry,
    retail: &RetailObs,
    cdname: &legaia_prot::cdname::IndexMap,
    report: &mut StateReport,
) {
    let Some(mg) = retail.minigame else {
        report.unseeded = StateClass::Minigame.unseeded_reason().to_string();
        return;
    };
    if let Some(why) = mg.unseeded_reason() {
        report.unseeded = why;
        return;
    }
    let departure = departure_scene(retail, &mg, cdname);
    let film = super::minigame_film::MinigameFilm::from_env();
    let engine = match run_engine_minigame(opts.extracted, retail, &departure, film.as_ref(), None)
    {
        Ok(e) => e,
        Err(e) => {
            report.unseeded = format!("seeding failed: {e:#}");
            return;
        }
    };
    let mut ch = BTreeMap::new();
    let mode_ok = mg
        .minigame()
        .and_then(MinigameSubId::scene_mode)
        .is_some_and(|m| engine.mode == m);
    ch.insert("mode".to_string(), f64::from(u8::from(mode_ok)));
    report.detail.insert(
        "mode".into(),
        format!("retail 0x19 sub-id {}; engine {:?}", mg.sub_id, engine.mode),
    );
    ch.insert(
        "scene".to_string(),
        f64::from(u8::from(engine.scene == departure)),
    );
    let mut words = vec![
        // The phase reached stands for the session's word when there is no
        // balance to compare.
        engine.reached.is_some() && engine.slot.map(|s| s.0) == mg.slot.map(|s| s.0),
        engine.slot.map(|s| s.1) == mg.slot.map(|s| s.1),
        engine.coin_bank == mg.coin_bank,
    ];
    // On the player select, its state and cursor.
    if mg.baka_drive().is_some() {
        words.push(engine.baka == Some((BAKA_SELECT, mg.baka_cursor as i32)));
    }
    // On the cash-out picker, its row.
    if mg.slot_drive().is_some_and(|(s, _)| s == 0x32) {
        words.push(engine.slot_cursor == Some(mg.slot_cursor));
    }
    // On an exchange screen: the screen, the list cursor, and on the dialogs
    // the quantity and the confirm's row.
    let drive = mg.drive();
    if let (Some((state, _, _)), Some(w)) = (&drive, mg.fishing_exchange) {
        let e = engine.exchange.unwrap_or_default();
        words.push(e.0 == *state);
        if *state == 0x6E {
            words.push(e.1 == w.tackle_cursor);
        } else if FISHING_EXCHANGE_STATES.contains(state) {
            words.push(e.1 == w.list_cursor);
        }
        if *state >= 0x79 {
            words.push(e.2 == w.qty);
        }
        if *state >= 0x7A {
            words.push(e.3 == w.confirm_row);
        }
        report.detail.insert(
            "exchange".into(),
            format!(
                "retail state {state:#X} cursor {} qty {} confirm {}; engine (state, cursor, qty, \
                 confirm) {:?}",
                w.list_cursor, w.qty, w.confirm_row, engine.exchange
            ),
        );
    }
    ch.insert(
        "minigame".to_string(),
        round3(words.iter().filter(|&&w| w).count() as f64 / words.len() as f64),
    );
    report.detail.insert(
        "minigame".into(),
        format!(
            "slot state/balance retail {:?} engine {:?}; coin bank retail {} engine {}; retail \
             phase {:?}, reached {:?} ticks past the warp",
            mg.slot, engine.slot, mg.coin_bank, engine.coin_bank, mg.phase, engine.reached
        ),
    );
    if let (Some(exe), Some(rf), Some(save)) =
        (opts.engine_exe, &retail.frame, retail.seed_save().as_ref())
    {
        let warp = WARP_TICK;
        let (key, past_warp) = match mg.minigame() {
            // The capture's own word names the instant; the slot machine's
            // attract state holds still, its frame sampled where the capture
            // stood.
            Some(MinigameSubId::Dance) => ("K", engine.reached.unwrap_or(0) + 1),
            Some(MinigameSubId::MuscleDome) => ("M", engine.reached.unwrap_or(0) + 1),
            // An exchange screen: the drive's last press plus its settle.
            Some(MinigameSubId::Fishing) => (
                "L",
                drive
                    .as_ref()
                    .map_or(FRAME_TICKS_PAST_WARP, |(_, p, settle)| {
                        FISHING_DRIVE_FIRST + p.last().map_or(0, |l| l.0) + settle
                    }),
            ),
            Some(MinigameSubId::SlotMachine) => (
                "O",
                drive
                    .as_ref()
                    .map_or(FRAME_TICKS_PAST_WARP, |(_, p, settle)| {
                        FISHING_DRIVE_FIRST + p.last().map_or(0, |l| l.0) + settle
                    }),
            ),
            Some(MinigameSubId::BakaFighter) => (
                "B",
                drive
                    .as_ref()
                    .map_or(engine.reached.unwrap_or(0) + 1, |(_, p, settle)| {
                        FISHING_DRIVE_FIRST
                            + p.last().map_or(0, |l| l.0)
                            + settle
                            + engine.baka_lineup_ticks
                    }),
            ),
            _ => ("O", FRAME_TICKS_PAST_WARP),
        };
        let mut extra = vec!["--key-script".to_string(), format!("{warp}:{key}")];
        if let Some((_, presses, _)) = &drive {
            let script = presses
                .iter()
                .map(|(t, b)| format!("{}:{b:?}", warp + FISHING_DRIVE_FIRST + t))
                .collect::<Vec<_>>()
                .join(",");
            extra.extend(["--pad-script".to_string(), script]);
        }
        let mut seats: Vec<(&'static str, String)> = mg
            .dance_markers
            .iter()
            .any(Option::is_some)
            .then(|| {
                (
                    "LEGAIA_SEAT_DANCE_MARKERS",
                    legaia_engine_capture::field_seeds::dance_markers_env(&mg.dance_markers),
                )
            })
            .into_iter()
            .collect();
        // The hall's animated props: their clip cursors are a clock of the
        // hall's game frames too.
        let props: Vec<(u8, i16)> = mg.dance_props.iter().flatten().copied().collect();
        if !props.is_empty() {
            seats.push((
                "LEGAIA_SEAT_DANCE_PROPS",
                legaia_engine_capture::field_seeds::dance_props_env(
                    &props,
                    crate::retail_compare_battle::DISPLAY_LAG_PASSES,
                ),
            ));
        }
        // The hall's light show is palette work: its cyclers and scrollers are
        // a clock of game frames, whose lengths the capture measured and the
        // engine's fixed tick does not.
        if mg.minigame() == Some(MinigameSubId::Dance) {
            if !retail.cell_fx.is_empty() {
                seats.push((
                    "LEGAIA_SEAT_CLUT_FX",
                    legaia_engine_capture::field_seeds::cell_fx_env(&retail.cell_fx),
                ));
            }
            seats.extend(retail.vram_rects_env(opts.out_dir, &entry.label));
            seats.extend(retail.cell_sources_env(opts.out_dir, &entry.label));
        }
        match crate::retail_compare_image::engine_frame_with(
            exe,
            opts.extracted,
            &departure,
            None,
            &extra,
            &seats,
            warp + past_warp,
            opts.out_dir,
            &entry.label,
            crate::retail_compare_image::FrameEntry::Resume(save),
        ) {
            Ok(ef) => {
                if let Some(dir) = opts.out_dir {
                    let _ = crate::retail_compare_image::write_side_by_side(
                        &dir.join(format!("{}.png", entry.label)),
                        rf,
                        &ef,
                    );
                }
                let score = crate::retail_compare_image::score(rf, &ef);
                ch.insert("image".into(), round3(score.within));
                report.detail.insert(
                    "image".into(),
                    format!(
                        "mae={:.1} within={:.3} ({})",
                        score.mae, score.within, score.note
                    ),
                );
                report.image = Some(score);
            }
            Err(e) => {
                report
                    .detail
                    .insert("image".into(), format!("engine frame failed: {e:#}"));
            }
        }
    }
    report.score = state_score(&ch);
    report.channels = ch;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(state: i32, balance: i32, bank: u32, door: bool) -> RetailMinigame {
        RetailMinigame {
            sub_id: 3,
            slot: Some((state, balance)),
            coin_bank: bank,
            door_entry: door,
            phase: None,
            phase2: 0,
            departure_define: 0,
            dance_markers: Default::default(),
            dance_props: [None; DANCE_PROP_CLIPS],
            dance_song: None,
            fishing_exchange: None,
            slot_cursor: 0,
            baka_cursor: 0,
            baka_lineup: None,
        }
    }

    #[test]
    fn only_a_fresh_door_entry_slot_machine_is_seeded() {
        assert_eq!(slot(1, 9_999_999, 9_999_999, true).unseeded_reason(), None);
        // A decided spin, a played machine, a dev launch.
        assert!(
            slot(4, 9_999_996, 9_999_999, true)
                .unseeded_reason()
                .is_some()
        );
        assert!(slot(1, 120, 100, true).unseeded_reason().is_some());
        assert!(slot(1, 70, 0, false).unseeded_reason().is_some());
        // Baka Fighter has no path yet.
        let baka = RetailMinigame {
            sub_id: 4,
            slot: None,
            coin_bank: 0,
            door_entry: true,
            phase: Some((0x64, 0)),
            phase2: 0,
            departure_define: 0,
            dance_markers: Default::default(),
            dance_props: [None; DANCE_PROP_CLIPS],
            dance_song: None,
            fishing_exchange: None,
            slot_cursor: 0,
            baka_cursor: 0,
            baka_lineup: None,
        };
        assert!(baka.unseeded_reason().unwrap().contains("Baka Fighter"));
    }

    #[test]
    fn the_entry_phases_are_seeded_and_the_played_ones_are_not() {
        let mg = |sub_id, state| RetailMinigame {
            sub_id,
            slot: None,
            coin_bank: 0,
            door_entry: true,
            phase: Some((state, 0)),
            phase2: 0,
            departure_define: 0,
            dance_markers: Default::default(),
            dance_props: [None; DANCE_PROP_CLIPS],
            dance_song: None,
            fishing_exchange: None,
            slot_cursor: 0,
            baka_cursor: 0,
            baka_lineup: None,
        };
        assert_eq!(mg(6, 3).unseeded_reason(), None);
        // The play loop: seeded on a floor the pad never reached, not on one
        // it did, nor when the words were not read.
        assert!(mg(6, 0xA).unseeded_reason().is_some());
        let played = |pressed| RetailMinigame {
            dance_song: Some((4800, pressed)),
            ..mg(6, 0xA)
        };
        assert_eq!(played(false).unseeded_reason(), None);
        assert!(played(true).unseeded_reason().is_some());
        assert!(mg(6, 0xB).unseeded_reason().is_some());
        assert_eq!(mg(5, 1).unseeded_reason(), None);
        assert_eq!(mg(5, 0x15).unseeded_reason(), None);
        assert!(mg(5, 0xA).unseeded_reason().is_some());
        assert_eq!(mg(4, 1).unseeded_reason(), None);
        assert_eq!(mg(0, 0xC).unseeded_reason(), None);
        assert!(mg(0, 0xB).unseeded_reason().is_some());
        assert!(mg(0, 0x19).unseeded_reason().is_some());
    }

    #[test]
    fn the_slot_menu_screens_of_an_unplayed_machine_are_seeded_by_their_presses() {
        use legaia_engine_system::input::PadButton as B;
        let menu = |state, cursor, balance| RetailMinigame {
            slot_cursor: cursor,
            ..slot(state, balance, 9_999_999, true)
        };
        for state in [0x32, 0x35, 0x36] {
            assert_eq!(
                menu(state, 2, 9_999_999).unseeded_reason(),
                None,
                "{state:#X}"
            );
        }
        // Played: the balance moved off the bank.
        assert!(menu(0x32, 0, 9_999_996).unseeded_reason().is_some());
        let buttons = |state, cursor| -> Vec<B> {
            menu(state, cursor, 9_999_999)
                .slot_drive()
                .unwrap()
                .1
                .into_iter()
                .map(|p| p.1)
                .collect()
        };
        assert_eq!(buttons(0x32, 1), [B::Triangle, B::Down]);
        assert_eq!(
            buttons(0x36, 2),
            [B::Triangle, B::Down, B::Down, B::Cross, B::Cross]
        );
    }

    #[test]
    fn an_exchange_screen_reached_through_the_hub_is_seeded_by_its_presses() {
        use legaia_engine_system::input::PadButton as B;
        let words = FishingExchangeWords {
            list_cursor: 4,
            qty: 3,
            confirm_row: 0,
            hub_cursor: 3,
            tackle_cursor: 0,
            lure: 0,
        };
        let fish = |state| RetailMinigame {
            sub_id: 0,
            slot: None,
            coin_bank: 0,
            door_entry: true,
            phase: Some((state, 0)),
            phase2: 0,
            departure_define: 0,
            dance_markers: Default::default(),
            dance_props: [None; DANCE_PROP_CLIPS],
            dance_song: None,
            fishing_exchange: Some(words),
            slot_cursor: 0,
            baka_cursor: 0,
            baka_lineup: None,
        };
        for state in [0x78, 0x79, 0x7A] {
            assert_eq!(fish(state).unseeded_reason(), None, "{state:#X}");
        }
        assert!(fish(0x6E).unseeded_reason().is_some());
        let buttons: Vec<B> = fishing_exchange_presses(0x7A, &words)
            .into_iter()
            .map(|p| p.1)
            .collect();
        assert_eq!(
            buttons,
            [
                B::Triangle,
                B::Down,
                B::Down,
                B::Down,
                B::Cross,
                B::Up,
                B::Cross,
                B::Right,
                B::Right,
                B::Cross,
                B::Up
            ]
        );
        // The list alone stops at its cursor.
        assert_eq!(fishing_exchange_presses(0x78, &words).len(), 6);
        // The help pages and the tackle list, through rows 1 and 2.
        let hub = |state, hub_cursor, tackle_cursor| RetailMinigame {
            fishing_exchange: Some(FishingExchangeWords {
                hub_cursor,
                tackle_cursor,
                lure: 1,
                ..words
            }),
            ..fish(state)
        };
        assert_eq!(hub(0x66, 1, 0).unseeded_reason(), None);
        assert_eq!(hub(0x6E, 2, 2).unseeded_reason(), None);
        assert!(
            hub(0x6E, 2, 0).unseeded_reason().is_some(),
            "under the lure"
        );
        let tackle: Vec<B> = hub(0x6E, 2, 2)
            .fishing_exchange_drive()
            .unwrap()
            .1
            .into_iter()
            .map(|p| p.1)
            .collect();
        assert_eq!(tackle, [B::Triangle, B::Down, B::Down, B::Cross, B::Down]);
    }
}
