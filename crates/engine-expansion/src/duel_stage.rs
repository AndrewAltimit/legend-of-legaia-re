//! A Grid Duel played **at a table in the world**: the session's view laid
//! out as cards on a board, hands fanned in front of each seat, the
//! opponent and the spectators around it, and the camera's shots.
//!
//! The session stays the only authority. [`DuelStage::step`] reads the
//! duel's view once a frame and returns a [`DuelStageFrame`]; everything it
//! remembers - card tweens, capture flips, bursts, the camera's phase, NPC
//! reactions, its own frame clock - is presentation and never reaches the
//! session, so a session ticks the same with the table drawn or not and two
//! peers drawing their own seats agree on every state hash.
//!
//! The layout is table-local ([`DuelTableFrame`]): `(right, across, up)`
//! from the board's centre, seen from the **local** seat. Each peer sits in
//! the near seat and sees the grid's row 0 on the far side, so the D-pad
//! moves the cursor the way it reads on screen whichever seat it holds.
//!
//! What happens, in order:
//!
//! - **Deal.** The ten cards fly from the dealer's pile into the two hands,
//!   alternating seats, while the camera holds an establishing shot of the
//!   room and the table, then eases in over the board.
//! - **Hands.** Each hand is fanned on its seat's edge of the table, tilted
//!   toward its holder: the player reads their faces, the opponent's show
//!   their backs. The selected card rises; a held card hovers over the
//!   cursor cell.
//! - **Place.** A placed card slides from the hand in an arc and drops onto
//!   its cell, landing with a dust ring.
//! - **Capture.** Each captured card turns over about its long axis, its
//!   owner's colour changing halfway, with a burst of sparks; chain
//!   captures follow a beat later with a larger burst and a "Chain!"
//!   banner.
//! - **End.** The last flips play out, then a reaction shot of the opponent
//!   (cheering or slumping, the spectators the other way) before the
//!   results card.

use legaia_engine_modes::cards::duel::{CELLS, DuelPhase, DuelView, HAND};
use legaia_engine_modes::cards::table::DuelTableFrame;
use legaia_engine_modes::cards::table_art::{self, FACE_H, FACE_W};
use legaia_expansion_format::cards::CardElement;

use legaia_expansion_format::audio::SoundRef;

use crate::mode_stage::{LeaderClip, StagePose, StageShot};

/// Frames the deal takes before the camera eases in.
pub const ESTABLISH_FRAMES: u32 = 120;
/// Frames between two dealt cards.
pub const DEAL_GAP: u32 = 6;
/// A dealt card's flight.
pub const DEAL_FRAMES: u32 = 16;
/// A placed card's slide and drop.
pub const PLACE_FRAMES: u32 = 22;
/// A capture flip.
pub const FLIP_FRAMES: u32 = 20;
/// The beat a chain capture waits after the direct ones.
pub const CHAIN_DELAY: u32 = 12;
/// A burst's life.
pub const BURST_FRAMES: u32 = 32;
/// How long a reaction clip plays.
pub const REACT_FRAMES: u32 = 90;
/// The reaction shot's length before the results card.
pub const OUTRO_FRAMES: u32 = 150;
/// A banner's life.
pub const BANNER_FRAMES: u32 = 90;

/// The close shot over the board: its pitch (12-bit units) and its
/// distance over the field follow camera's eye depth.
pub const CLOSE_PITCH: f32 = 360.0;
pub const CLOSE_DISTANCE: f32 = 0.175;

/// A card's size on the table, as a fraction of a cell's width.
pub const CARD_FILL: f32 = 0.86;

/// Where a card is and how it lies, table-local.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CardPose {
    /// `(right, across, up)` from the board's centre, world units; `up`
    /// is above the table top.
    pub pos: [f32; 3],
    /// Turn about the table's up axis, radians; `0` reads from the local
    /// seat.
    pub yaw: f32,
    /// The top edge raised toward the viewer, radians (`0` lies flat).
    pub tilt: f32,
    /// Turn about the card's long axis, radians (`PI` is face down).
    pub flip: f32,
}

impl CardPose {
    fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        let l = |x: f32, y: f32| x + (y - x) * t;
        Self {
            pos: [
                l(a.pos[0], b.pos[0]),
                l(a.pos[1], b.pos[1]),
                l(a.pos[2], b.pos[2]),
            ],
            yaw: l(a.yaw, b.yaw),
            tilt: l(a.tilt, b.tilt),
            flip: l(a.flip, b.flip),
        }
    }

    fn near(&self, o: &Self) -> bool {
        let d = (0..3).map(|i| (self.pos[i] - o.pos[i]).abs()).sum::<f32>();
        d < 0.5
            && (self.yaw - o.yaw).abs() < 1e-3
            && (self.tilt - o.tilt).abs() < 1e-3
            && (self.flip - o.flip).abs() < 1e-3
    }
}

/// One card to draw this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StageCard {
    /// The seat that dealt it and its hand index: its texture slot
    /// (`table_art::card_slot`).
    pub deck: usize,
    pub card: usize,
    /// The seat whose colour shows under the face.
    pub owner: usize,
    /// Its face's and its back's rectangles on the table's card page,
    /// `[u, v, w, h]` (`table_art` for a duel, `poker::art` for poker).
    pub face_uv: [u8; 4],
    pub back_uv: [u8; 4],
    /// A quad in the owner's colour goes under the face (a duel card's
    /// transparent body); a playing card's face is opaque.
    pub backing: bool,
    /// World centre `(X, Z)` and its height above the floor.
    pub centre: [f32; 2],
    pub height: f32,
    /// The card's right and top edge directions, world `(X, up, Z)`
    /// (up is `+`); their lengths are the card's half width and half
    /// height.
    pub right: [f32; 3],
    pub up: [f32; 3],
    /// Raised in the hand or held: drawn with a rim light.
    pub lit: bool,
    /// Off the table (a slide's arc, a flip's lift): its shadow spreads.
    pub airborne: f32,
}

/// A spark burst: a capture's, a chain's (larger) or a landing's dust.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StageBurst {
    /// World centre `(X, Z)` and height above the floor.
    pub at: [f32; 2],
    pub height: f32,
    /// Frames since it began.
    pub age: u32,
    pub kind: BurstKind,
    /// The seat whose colour it takes.
    pub seat: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BurstKind {
    Dust,
    Capture,
    Chain,
}

/// A map NPC the table drives this frame.
#[derive(Debug, Clone, PartialEq)]
pub struct DuelNpcPose {
    /// The map NPC id.
    pub npc: String,
    /// Where it stands, world `(X, Z)`; `None` keeps its own tile.
    pub pos: Option<[f32; 2]>,
    /// The point it faces, world `(X, Z)`.
    pub look_at: [f32; 2],
    /// The clip it plays; `None` its idle.
    pub clip: Option<u8>,
}

/// One frame of the table.
#[derive(Debug, Clone, PartialEq)]
pub struct DuelStageFrame {
    pub table: DuelTableFrame,
    /// The seat this machine's player holds.
    pub local: usize,
    /// The stage clock (for the drawing's own pulses).
    pub clock: u32,
    /// Every card, farthest-drawn order left to the drawing.
    pub cards: Vec<StageCard>,
    /// Elemental cells, in board order, as world centres with the element.
    pub elements: Vec<([f32; 2], CardElement)>,
    /// The local player's cell cursor, a cell's world centre.
    pub cursor: Option<[f32; 2]>,
    /// Sparks and dust.
    pub bursts: Vec<StageBurst>,
    /// Cell centres in board order (world `(X, Z)`), for the grid lines.
    pub cells: [[f32; 2]; CELLS],
    /// The NPCs at the table.
    pub npcs: Vec<DuelNpcPose>,
    /// Sounds to play this frame (retail cues or pack sounds).
    pub cues: Vec<SoundRef>,
    /// The opponent's seat position, for a reaction shot or a host's use.
    pub opponent_seat: [f32; 2],
    /// The reaction shot is up: the HUD steps aside.
    pub outro: bool,
    /// The camera's shot.
    pub shot: StageShot,
    /// "Chain!", "3 captured!", the result.
    pub banner: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct Tween {
    from: CardPose,
    to: CardPose,
    start: u32,
    dur: u32,
    /// Peak height of the arc above the straight path.
    arc: f32,
}

impl Tween {
    fn at(&self, now: u32) -> CardPose {
        if now <= self.start {
            return self.from;
        }
        let t = ((now - self.start) as f32 / self.dur.max(1) as f32).min(1.0);
        let e = t * t * (3.0 - 2.0 * t);
        let mut p = CardPose::lerp(&self.from, &self.to, e);
        p.pos[2] += self.arc * (std::f32::consts::PI * t).sin();
        p
    }

    fn done(&self, now: u32) -> bool {
        now >= self.start + self.dur
    }
}

#[derive(Debug, Clone, Copy)]
struct Flip {
    start: u32,
    from_owner: usize,
    to_owner: usize,
    chained: bool,
    burst_sent: bool,
}

#[derive(Debug, Clone, Copy)]
struct Burst {
    at: (f32, f32),
    start: u32,
    kind: BurstKind,
    seat: usize,
}

/// Who reacts how, until a frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Mood {
    #[default]
    Idle,
    Cheer,
    Slump,
}

/// The table's memory. Presentation only.
#[derive(Debug, Clone, Default)]
pub struct DuelStage {
    clock: u32,
    last: Option<DuelView>,
    table: Option<DuelTableFrame>,
    local: usize,
    tweens: Vec<((usize, usize), Tween)>,
    flips: Vec<(usize, Flip)>,
    /// The owner each grid cell shows (lags the view through a flip).
    shown_owner: [Option<usize>; CELLS],
    bursts: Vec<Burst>,
    /// Frame the camera last pulled back for a capture.
    pull_until: u32,
    /// The opponent's / the spectators' mood and until when.
    opponent_mood: (Mood, u32),
    crowd_mood: (Mood, u32),
    /// The banner, shown from the first frame to the second.
    banner: Option<(String, u32, u32)>,
    /// The frame the game ended (the last placement resolved).
    ended: Option<u32>,
    shot: Option<StageShot>,
    pending_cues: Vec<(u32, SoundRef)>,
    /// The session's last view, noted as it left play (the move that ended
    /// it), which the results stage plays out.
    final_view: Option<DuelView>,
    /// Whether this machine only watches the duel (a spectating peer): the
    /// table is laid out from seat 0's side, and the end reads the winner
    /// by name instead of "You win!" / "You lose".
    pub watching: bool,
    /// Each seat's name as a watcher reads it ([`Self::watching`]).
    pub seat_names: [String; 2],
}

/// The pile the dealer deals from: off the board's right edge.
fn pile_pose(t: &DuelTableFrame) -> CardPose {
    CardPose {
        pos: [t.board * 0.5 + 34.0, 0.0, 1.5],
        yaw: 0.3,
        tilt: 0.0,
        flip: std::f32::consts::PI,
    }
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl DuelStage {
    /// The stage clock.
    pub fn clock(&self) -> u32 {
        self.clock
    }

    /// Whether the reaction shot still plays: the seat holds its results
    /// card until it has.
    pub fn holds_results(&self) -> bool {
        self.ended
            .is_some_and(|e| self.clock < e + OUTRO_FRAMES + self.settle_frames())
    }

    /// Frames the last flips need after the game ends.
    fn settle_frames(&self) -> u32 {
        PLACE_FRAMES + FLIP_FRAMES + CHAIN_DELAY + 10
    }

    /// The card size on the table: half width, half height.
    fn card_half(t: &DuelTableFrame) -> (f32, f32) {
        let w = t.board / 3.0 * CARD_FILL;
        let h = w * f32::from(FACE_H) / f32::from(FACE_W);
        (w * 0.5, h * 0.5)
    }

    /// The physical seat a view seat sits in: the local player near.
    fn phys(&self, seat: usize) -> usize {
        usize::from(seat != self.local)
    }

    /// A cell's table-local centre. The local seat's axes turn with it,
    /// so each peer sees row 0 on the far side.
    fn cell_at(&self, t: &DuelTableFrame, cell: usize) -> (f32, f32) {
        t.cell_local(cell)
    }

    /// Hand card `i` of view seat `seat`'s pose, `n` cards still held and
    /// `k` this card's order among them.
    fn hand_pose(
        &self,
        t: &DuelTableFrame,
        seat: usize,
        k: usize,
        n: usize,
        lit: bool,
    ) -> CardPose {
        let (_, hh) = Self::card_half(t);
        let near = self.phys(seat) == 0;
        let mid = (n.max(1) - 1) as f32 / 2.0;
        let off = k as f32 - mid;
        let edge = t.board_depth() * 0.5 + 26.0;
        let tilt: f32 = if lit { 0.78 } else { 0.95 };
        let lift = if lit { 10.0 } else { 0.0 };
        let reach = if lit { 12.0 } else { 0.0 };
        let r = off * 27.0;
        let a = edge + off.abs() * 3.0 - reach;
        let up = hh * tilt.sin() + 1.5 + lift;
        if near {
            CardPose {
                pos: [r, -a, up],
                yaw: -off * 0.1,
                tilt,
                flip: 0.0,
            }
        } else {
            // The far hand faces its holder: turned half round.
            CardPose {
                pos: [-r, a, up],
                yaw: std::f32::consts::PI - off * 0.1,
                tilt,
                flip: 0.0,
            }
        }
    }

    /// Step one frame off `view` (the session's, or the last one kept once
    /// the session has ended).
    pub fn step(
        &mut self,
        table: &DuelTableFrame,
        view: &DuelView,
        local: usize,
    ) -> DuelStageFrame {
        let now = self.clock;
        let fresh = self.table.as_ref() != Some(table) || self.local != local.min(1);
        if fresh {
            *self = Self {
                table: Some(table.clone()),
                local: local.min(1),
                watching: self.watching,
                seat_names: std::mem::take(&mut self.seat_names),
                ..Self::default()
            };
        }
        let t = table.clone();
        let mut cues: Vec<SoundRef> = Vec::new();
        let first = self.last.is_none();
        let prev = self.last.clone();

        // Placements and captures since last frame.
        let mut placed: Vec<(usize, usize)> = Vec::new();
        let mut captured: Vec<usize> = Vec::new();
        if let Some(p) = &prev {
            for cell in 0..CELLS {
                match (p.grid[cell], view.grid[cell]) {
                    (None, Some(now_p)) => placed.push((cell, now_p.deck)),
                    (Some(a), Some(b)) if a.owner != b.owner => captured.push(cell),
                    _ => {}
                }
            }
        } else {
            for cell in 0..CELLS {
                self.shown_owner[cell] = view.grid[cell].map(|p| p.owner);
            }
        }
        // A capture lands after its card's drop; a chain capture (not a
        // neighbour of the card placed) a beat after that.
        let land = now + PLACE_FRAMES;
        for &cell in &captured {
            let near_placed = placed.iter().any(|&(c, _)| {
                let (r0, c0) = (c / 3, c % 3);
                let (r1, c1) = (cell / 3, cell % 3);
                r0.abs_diff(r1) + c0.abs_diff(c1) == 1
            });
            let chained = !placed.is_empty() && !near_placed;
            let from = prev
                .as_ref()
                .and_then(|p| p.grid[cell])
                .map_or(0, |p| p.owner);
            let to = view.grid[cell].map_or(0, |p| p.owner);
            let start = if placed.is_empty() {
                now
            } else if chained {
                land + CHAIN_DELAY
            } else {
                land
            };
            self.flips.retain(|(c, _)| *c != cell);
            self.flips.push((
                cell,
                Flip {
                    start,
                    from_owner: from,
                    to_owner: to,
                    chained,
                    burst_sent: false,
                },
            ));
        }
        for &(cell, _) in &placed {
            self.shown_owner[cell] = view.grid[cell].map(|p| p.owner);
            self.pending_cues.push((land, t.sfx.place.clone()));
            let (r, a) = self.cell_at(&t, cell);
            self.bursts.push(Burst {
                at: (r, a),
                start: land,
                kind: BurstKind::Dust,
                seat: view.grid[cell].map_or(0, |p| p.owner),
            });
        }
        // A card lifted from the hand.
        if let (Some(p), DuelPhase::PickCell { seat, .. }) = (&prev, view.phase)
            && matches!(p.phase, DuelPhase::PickCard { seat: s } if s == seat)
        {
            cues.push(t.sfx.deal.clone());
        }
        // Moods and banners off the move.
        if !captured.is_empty() {
            let by = view.grid[captured[0]].map_or(0, |p| p.owner);
            let chain = captured.len() >= 2
                && captured
                    .iter()
                    .any(|&c| self.flips.iter().any(|(fc, f)| *fc == c && f.chained));
            let big = captured.len() >= 2;
            if chain {
                let at = land + CHAIN_DELAY;
                self.banner = Some(("Chain!".to_string(), at, at + BANNER_FRAMES));
            } else if big {
                self.banner = Some((
                    format!("{} captured!", captured.len()),
                    land,
                    land + BANNER_FRAMES,
                ));
            }
            self.pull_until = land + FLIP_FRAMES + 40 + if chain { CHAIN_DELAY } else { 0 };
            if big {
                let player = by == self.local;
                let until = land + REACT_FRAMES;
                self.crowd_mood = (if player { Mood::Cheer } else { Mood::Slump }, until);
                self.opponent_mood = (if player { Mood::Slump } else { Mood::Cheer }, until);
            }
        }
        // The end: once, when the session leaves play.
        let over = matches!(view.phase, DuelPhase::Spoils { .. } | DuelPhase::Finished);
        if over && self.ended.is_none() {
            self.ended = Some(now);
            let me = self.local;
            let (mine, theirs) = (view.scores[me], view.scores[1 - me]);
            let at = now + self.settle_frames();
            let (text, cue, crowd, opp) = if self.watching {
                // A watcher has no side: the winner by name, the crowd
                // cheering whoever took it, the opponent's reaction its own.
                let name = |seat: usize| {
                    let n = &self.seat_names[seat];
                    if n.is_empty() {
                        format!("Seat {}", seat + 1)
                    } else {
                        n.clone()
                    }
                };
                let (s0, s1) = (view.scores[0], view.scores[1]);
                if s0 == s1 {
                    (
                        "Draw".to_string(),
                        t.sfx.lose.clone(),
                        Mood::Idle,
                        Mood::Idle,
                    )
                } else {
                    let w = usize::from(s1 > s0);
                    let opp = if w == 1 { Mood::Cheer } else { Mood::Slump };
                    (
                        format!("{} wins!", name(w)),
                        t.sfx.win.clone(),
                        Mood::Cheer,
                        opp,
                    )
                }
            } else if mine > theirs {
                (
                    "You win!".to_string(),
                    t.sfx.win.clone(),
                    Mood::Cheer,
                    Mood::Slump,
                )
            } else if mine < theirs {
                (
                    "You lose".to_string(),
                    t.sfx.lose.clone(),
                    Mood::Slump,
                    Mood::Cheer,
                )
            } else {
                (
                    "Draw".to_string(),
                    t.sfx.lose.clone(),
                    Mood::Idle,
                    Mood::Idle,
                )
            };
            self.pending_cues.push((at, cue));
            self.banner = Some((text, at, at + OUTRO_FRAMES));
            self.crowd_mood = (crowd, u32::MAX);
            self.opponent_mood = (opp, u32::MAX);
        }

        // Every card's target pose.
        let mut targets: Vec<((usize, usize), CardPose, bool, u32)> = Vec::new();
        let held = match view.phase {
            DuelPhase::PickCell { seat, card } => Some((seat, card)),
            _ => None,
        };
        let picking = match view.phase {
            DuelPhase::PickCard { seat } => Some(seat),
            _ => None,
        };
        for seat in 0..2 {
            let in_hand: Vec<usize> = (0..view.hands[seat].len().min(HAND))
                .filter(|&i| !view.used[seat][i])
                .collect();
            let n = in_hand.len();
            for (k, &i) in in_hand.iter().enumerate() {
                let is_held = held == Some((seat, i));
                let lit = is_held || (picking == Some(seat) && view.hand_cursor[seat] == i);
                let pose = if is_held {
                    // Hovering over the cursor cell, face up to the reader.
                    let (r, a) = self.cell_at(&t, view.cell_cursor[seat]);
                    CardPose {
                        pos: [r, a, 26.0],
                        yaw: 0.0,
                        tilt: 0.0,
                        flip: 0.0,
                    }
                } else {
                    self.hand_pose(&t, seat, k, n, lit)
                };
                // Dealt alternately: seat by seat, card by card.
                let order = (i * 2 + self.phys(seat)) as u32;
                targets.push(((seat, i), pose, lit, order));
            }
        }
        for cell in 0..CELLS {
            if let Some(p) = view.grid[cell] {
                let (r, a) = self.cell_at(&t, cell);
                targets.push((
                    (p.deck, p.card),
                    CardPose {
                        pos: [r, a, 0.8],
                        yaw: 0.0,
                        tilt: 0.0,
                        flip: 0.0,
                    },
                    false,
                    u32::MAX,
                ));
            }
        }
        // Retarget the tweens.
        for (key, target, _, order) in &targets {
            let slot = self.tweens.iter().position(|(k, _)| k == key);
            match slot {
                None => {
                    let (from, start, dur, arc) = if first || order != &u32::MAX {
                        (
                            pile_pose(&t),
                            now + order.saturating_mul(DEAL_GAP).min(60),
                            DEAL_FRAMES,
                            24.0,
                        )
                    } else {
                        (*target, now, 1, 0.0)
                    };
                    if first && *order != u32::MAX {
                        self.pending_cues
                            .push((now + (order * DEAL_GAP).min(60), t.sfx.deal.clone()));
                    }
                    self.tweens.push((
                        *key,
                        Tween {
                            from,
                            to: *target,
                            start,
                            dur,
                            arc,
                        },
                    ));
                }
                Some(s) => {
                    let tw = self.tweens[s].1;
                    if !tw.to.near(target) {
                        let to_cell = target.pos[2] < 1.0;
                        let (dur, arc) = if to_cell {
                            (PLACE_FRAMES, 30.0)
                        } else {
                            (8, 0.0)
                        };
                        self.tweens[s].1 = Tween {
                            from: tw.at(now),
                            to: *target,
                            start: now.max(tw.start.min(now + 60)),
                            dur,
                            arc,
                        };
                    }
                }
            }
        }

        // The cards this frame.
        let (hw, hh) = Self::card_half(&t);
        let (across_w, right_w) = t.axes(self.local);
        let centre = t.centre();
        let top = t.top();
        let world = |r: f32, a: f32| -> [f32; 2] {
            [
                centre[0] + right_w[0] * r + across_w[0] * a,
                centre[1] + right_w[1] * r + across_w[1] * a,
            ]
        };
        let dir = |v: [f32; 3]| -> [f32; 3] {
            // Table-local (right, across, up) to world (X, up, Z).
            [
                right_w[0] * v[0] + across_w[0] * v[1],
                v[2],
                right_w[1] * v[0] + across_w[1] * v[1],
            ]
        };
        let mut cards = Vec::new();
        for (key, _target, lit, _) in &targets {
            let Some((_, tw)) = self.tweens.iter().find(|(k, _)| k == key) else {
                continue;
            };
            let mut pose = tw.at(now);
            let mut owner = key.0;
            // A grid card: its owner and its flip.
            if let Some(cell) =
                (0..CELLS).find(|&c| view.grid[c].is_some_and(|p| (p.deck, p.card) == *key))
            {
                owner = self.shown_owner[cell].unwrap_or(owner);
                if let Some((_, f)) = self.flips.iter_mut().find(|(c, _)| *c == cell)
                    && now >= f.start
                {
                    let u = ((now - f.start) as f32 / FLIP_FRAMES as f32).min(1.0);
                    pose.flip = std::f32::consts::TAU * smooth(u);
                    pose.pos[2] += 16.0 * (std::f32::consts::PI * u).sin();
                    owner = if u < 0.5 { f.from_owner } else { f.to_owner };
                    if u >= 0.5 && !f.burst_sent {
                        f.burst_sent = true;
                        cues.push(t.sfx.flip.clone());
                        let (r, a) = t.cell_local(cell);
                        self.bursts.push(Burst {
                            at: (r, a),
                            start: now,
                            kind: if f.chained {
                                BurstKind::Chain
                            } else {
                                BurstKind::Capture
                            },
                            seat: f.to_owner,
                        });
                    }
                    if u >= 1.0 {
                        self.shown_owner[cell] = Some(f.to_owner);
                    }
                }
            }
            // Basis: flip about the card's long axis, tilt the top edge
            // up, then turn about the table's up axis.
            let (sf, cf) = pose.flip.sin_cos();
            let (st, ct) = pose.tilt.sin_cos();
            let (sy, cy) = pose.yaw.sin_cos();
            // Flat: right = r, up(top) = a, normal = u.
            let r0 = [cf, 0.0, -sf];
            let n0 = [sf, 0.0, cf];
            let u0 = [0.0, 1.0, 0.0];
            // Tilt about r0.
            let u1 = [
                u0[0] * ct + n0[0] * st,
                u0[1] * ct + n0[1] * st,
                u0[2] * ct + n0[2] * st,
            ];
            let yaw = |v: [f32; 3]| [v[0] * cy - v[1] * sy, v[0] * sy + v[1] * cy, v[2]];
            let (r1, u2) = (yaw(r0), yaw(u1));
            let scale = |v: [f32; 3], k: f32| [v[0] * k, v[1] * k, v[2] * k];
            let at = world(pose.pos[0], pose.pos[1]);
            let rect = |slot: u16| {
                let (u, v) = table_art::slot_uv(slot);
                [u, v, FACE_W as u8, FACE_H as u8]
            };
            cards.push(StageCard {
                deck: key.0,
                card: key.1,
                owner,
                face_uv: rect(table_art::card_slot(key.0, key.1)),
                back_uv: rect(table_art::BACK_SLOT),
                backing: true,
                centre: at,
                height: top + pose.pos[2],
                right: dir(scale(r1, hw)),
                up: dir(scale(u2, hh)),
                lit: *lit,
                airborne: (pose.pos[2] / 30.0).clamp(0.0, 1.0),
            });
        }
        // Finished tweens are dropped only when their card is gone, so a
        // card keeps its pose; flips are dropped when done.
        self.flips.retain(|(_, f)| now < f.start + FLIP_FRAMES + 1);
        self.tweens
            .retain(|(k, tw)| targets.iter().any(|(tk, ..)| tk == k) || !tw.done(now));
        self.bursts.retain(|b| now < b.start + BURST_FRAMES);
        for (at, cue) in &self.pending_cues {
            if *at == now {
                cues.push(cue.clone());
            }
        }
        self.pending_cues.retain(|(at, _)| *at > now);

        let bursts = self
            .bursts
            .iter()
            .filter(|b| now >= b.start)
            .map(|b| StageBurst {
                at: world(b.at.0, b.at.1),
                height: top + 1.0,
                age: now - b.start,
                kind: b.kind,
                seat: b.seat,
            })
            .collect();

        let cells = std::array::from_fn(|c| {
            let (r, a) = self.cell_at(&t, c);
            world(r, a)
        });
        let elements = (0..CELLS)
            .filter_map(|c| view.cell_elements[c].map(|e| (cells[c], e)))
            .collect();
        let cursor = match view.phase {
            DuelPhase::PickCell { seat, .. } if seat == self.local => {
                Some(cells[view.cell_cursor[seat]])
            }
            _ => None,
        };

        // The NPCs.
        let opp_seat = world(0.0, t.seat_distance);
        let board_centre = world(0.0, 0.0);
        let mood_clip = |npc: &legaia_expansion_format::modes::DuelSeatNpc, m: (Mood, u32)| {
            if now >= m.1 {
                return None;
            }
            match m.0 {
                Mood::Cheer => npc.cheer,
                Mood::Slump => npc.slump,
                Mood::Idle => None,
            }
        };
        let mut npcs = Vec::new();
        let opponent_thinks = match view.phase {
            DuelPhase::PickCard { seat } | DuelPhase::PickCell { seat, .. } => seat != self.local,
            _ => false,
        };
        if let Some(o) = &t.opponent {
            let clip =
                mood_clip(o, self.opponent_mood).or(if opponent_thinks { o.think } else { None });
            npcs.push(DuelNpcPose {
                npc: o.npc.clone(),
                pos: Some(opp_seat),
                look_at: board_centre,
                clip,
            });
        }
        for s in &t.spectators {
            npcs.push(DuelNpcPose {
                npc: s.npc.clone(),
                pos: None,
                look_at: board_centre,
                clip: mood_clip(s, self.crowd_mood),
            });
        }

        let outro = self.ended.is_some_and(|e| now >= e + self.settle_frames());
        let shot = self.shot(&t, view, now);
        let banner = self
            .banner
            .as_ref()
            .filter(|(_, from, until)| (*from..*until).contains(&now))
            .map(|(s, ..)| s.clone());
        self.last = Some(view.clone());
        self.clock = now + 1;
        DuelStageFrame {
            shot,
            banner,
            table: t,
            local: self.local,
            clock: now,
            cards,
            elements,
            cursor,
            bursts,
            cells,
            npcs,
            cues,
            opponent_seat: opp_seat,
            outro,
        }
    }

    /// The leader's pose: standing at the local seat's left, turned in
    /// toward the board.
    pub fn leader(&self, t: &DuelTableFrame) -> StagePose {
        let (across, right) = t.axes(self.local);
        let c = t.centre();
        // Beside the near corner of the table, clear of the hand.
        let (r, a) = (-(t.board * 0.5 + 44.0), -t.board_depth() * 0.5 - 36.0);
        let pos = [
            c[0] + right[0] * r + across[0] * a,
            c[1] + right[1] * r + across[1] * a,
        ];
        let to = [c[0] - pos[0], c[1] - pos[1]];
        let ang = to[1].atan2(to[0]) / std::f32::consts::TAU * 4096.0;
        StagePose {
            pos,
            facing: ang.rem_euclid(4096.0) as u16,
            clip: LeaderClip::Idle,
        }
    }

    /// The camera's shot at stage frame `now` (`view` is its phase).
    fn shot(&mut self, t: &DuelTableFrame, view: &DuelView, now: u32) -> StageShot {
        let (across, _) = t.axes(self.local);
        let look = (across[1].atan2(across[0]) / std::f32::consts::TAU * 4096.0).rem_euclid(4096.0);
        let c = t.centre();
        let toward = |k: f32| [c[0] + across[0] * k, c[1] + across[1] * k];
        let drift = (now as f32 / 240.0 * std::f32::consts::TAU).sin();
        let top = t.top();
        // The close shot over the board, from behind the local seat.
        let mut target = StageShot {
            focus: toward(-34.0),
            yaw: look + drift * 30.0,
            pitch: CLOSE_PITCH,
            distance: CLOSE_DISTANCE + 0.006 * drift,
            lift: top,
        };
        if let DuelPhase::PickCell { seat, .. } = view.phase {
            if seat == self.local {
                // Push in over the cell being chosen.
                let (r, a) = t.cell_local(view.cell_cursor[seat]);
                let (ac, rr) = t.axes(self.local);
                target.focus = [
                    c[0] + rr[0] * r * 0.5 + ac[0] * a * 0.5,
                    c[1] + rr[1] * r * 0.5 + ac[1] * a * 0.5,
                ];
                target.distance = CLOSE_DISTANCE * 0.88;
                target.pitch = CLOSE_PITCH + 50.0;
            } else {
                target.focus = toward(24.0);
            }
        }
        if now < self.pull_until {
            target.distance = CLOSE_DISTANCE * 1.25;
            target.pitch = CLOSE_PITCH + 60.0;
        }
        let ending = self.ended.map(|e| e + self.settle_frames());
        let mut k = 0.08;
        if now < ESTABLISH_FRAMES {
            // The establishing shot: wide and high, turning slowly in.
            let u = smooth(now as f32 / ESTABLISH_FRAMES as f32);
            let wide = StageShot {
                focus: toward(30.0 * (1.0 - u)),
                yaw: look + 700.0 * (1.0 - u),
                pitch: 470.0 + 30.0 * u,
                distance: 0.75 - 0.35 * u,
                lift: top * 0.5,
            };
            if now < ESTABLISH_FRAMES / 2 {
                target = wide;
                k = 1.0;
            } else {
                k = 0.06;
                target = StageShot {
                    focus: [
                        wide.focus[0] + (target.focus[0] - wide.focus[0]) * u,
                        wide.focus[1] + (target.focus[1] - wide.focus[1]) * u,
                    ],
                    ..wide
                };
            }
        } else if let Some(e) = ending
            && now >= e
        {
            // The reaction shot: low across the board on the opponent.
            let o = toward(t.seat_distance);
            target = StageShot {
                focus: [o[0] - across[0] * 20.0, o[1] - across[1] * 20.0],
                yaw: look - 160.0,
                pitch: 260.0,
                distance: 0.13,
                lift: top + 36.0,
            };
            k = 0.05;
        }
        let s = match self.shot {
            None => target,
            Some(s) => s.ease_to(&target, k),
        };
        self.shot = Some(s);
        s
    }

    /// The last view kept, once the session is gone (the results stage):
    /// the final one noted as it left, else the last one stepped.
    pub fn last_view(&self) -> Option<&DuelView> {
        self.final_view.as_ref().or(self.last.as_ref())
    }

    /// Note the session's view as it leaves play.
    pub fn note_final(&mut self, view: DuelView) {
        if self.table.is_some() {
            self.final_view = Some(view);
        }
    }

    /// The table kept with it.
    pub fn table(&self) -> Option<&DuelTableFrame> {
        self.table.as_ref()
    }

    /// The local seat it was laid out for.
    pub fn local(&self) -> usize {
        self.local
    }
}

#[cfg(test)]
mod tests;
