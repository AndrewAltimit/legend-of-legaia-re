//! Texas Hold'em played **at a table in the world**: the session's view
//! laid out as cards and chips on a felt table, the other seats' NPCs round
//! it, and the camera's shots.
//!
//! As with the duel table ([`crate::duel_stage`]) the session stays the
//! only authority: [`PokerStage::step`] reads the table's view once a frame
//! and keeps only presentation (card tweens, chip flights, NPC moods, the
//! camera), so a session ticks the same drawn or not, and every peer of a
//! networked table draws its own chair from the view it was sent.
//!
//! Hidden information stays hidden. A hole card shows its face only to its
//! own seat, and to everyone at a showdown where its seat is still in the
//! hand; every other hole card lies face down and has no face on the card
//! page at all ([`legaia_engine_modes::poker::art`]).
//!
//! - **Deal.** Each hand the two hole cards fly from the dealer's deck to
//!   every seat in the hand, one round at a time; the viewer's stand up in
//!   front of them, the rest lie face down before their seats.
//! - **Streets.** The flop, turn and river fly to the board's row and turn
//!   face up.
//! - **Chips.** Every seat's stack stands before it, its street bet nearer
//!   the middle with the amount floating over it; when a street closes the
//!   bets slide into the pot, and a won pot slides to its winner.
//! - **Showdown.** The live hands turn over while the camera sweeps round
//!   the table, then holds on the winner.

use legaia_engine_modes::active::category_label;
use legaia_engine_modes::poker::art::{self, VisibleCards};
use legaia_engine_modes::poker::table::PokerTableFrame;
use legaia_engine_modes::poker::{Phase, PokerView, Street};

use legaia_expansion_format::audio::SoundRef;

use crate::duel_stage::{BurstKind, CardPose, DuelNpcPose, StageBurst, StageCard};
use crate::mode_stage::{LeaderClip, StagePose, StageShot};

/// Frames between two dealt cards.
pub const DEAL_GAP: u32 = 4;
/// A dealt card's flight.
pub const DEAL_FRAMES: u32 = 14;
/// A board card's turn.
pub const TURN_FRAMES: u32 = 14;
/// A chip flight.
pub const CHIP_FRAMES: u32 = 20;
/// A reaction clip's length.
pub const REACT_FRAMES: u32 = 90;
/// The establishing shot.
pub const ESTABLISH_FRAMES: u32 = 110;
/// A banner's life.
pub const BANNER_FRAMES: u32 = 150;
/// A contested showdown's sweep round the table, before the hold on the
/// winner (the session's showdown lasts 120 frames).
pub const SWEEP_FRAMES: f32 = 80.0;
/// How long a contested showdown holds the felt, sweep and winner shot
/// together.
pub const SHOWDOWN_HOLD: u32 = 150;
/// A playing card's size on the felt, world units (half width).
pub const CARD_HALF_W: f32 = 9.5;

/// Chip colours by denomination step, as the stacks draw them.
pub const CHIP_COLOURS: [[u8; 3]; 5] = [
    [236, 236, 228],
    [210, 52, 48],
    [52, 112, 210],
    [40, 150, 72],
    [30, 30, 36],
];

/// A stack of chips on the felt.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChipStack {
    /// World `(X, Z)` and the felt's height.
    pub at: [f32; 2],
    pub height: f32,
    /// Chips drawn (a function of the amount, not the count).
    pub chips: u8,
    /// Index into [`CHIP_COLOURS`] of the top chip.
    pub colour: u8,
}

/// An amount floating over the felt.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StageAmount {
    pub at: [f32; 2],
    /// Height above the floor.
    pub height: f32,
    pub value: u32,
}

/// One frame of the table.
#[derive(Debug, Clone, PartialEq)]
pub struct PokerStageFrame {
    pub table: PokerTableFrame,
    pub local: usize,
    pub clock: u32,
    pub cards: Vec<StageCard>,
    pub stacks: Vec<ChipStack>,
    pub amounts: Vec<StageAmount>,
    /// The dealer button, world `(X, Z)`.
    pub button: Option<[f32; 2]>,
    /// The seat to act, its chair's felt edge (for the turn ring).
    pub to_act: Option<[f32; 2]>,
    /// The dealer's deck, world `(X, Z)`.
    pub deck: [f32; 2],
    pub bursts: Vec<StageBurst>,
    pub npcs: Vec<DuelNpcPose>,
    pub cues: Vec<SoundRef>,
    pub shot: StageShot,
    pub banner: Option<String>,
    /// What the card page holds this frame.
    pub visible: VisibleCards,
}

#[derive(Debug, Clone, Copy)]
struct Tween {
    from: CardPose,
    to: CardPose,
    start: u32,
    dur: u32,
    arc: f32,
}

impl Tween {
    fn at(&self, now: u32) -> CardPose {
        if now <= self.start {
            return self.from;
        }
        let t = ((now - self.start) as f32 / self.dur.max(1) as f32).min(1.0);
        let e = t * t * (3.0 - 2.0 * t);
        let l = |a: f32, b: f32| a + (b - a) * e;
        CardPose {
            pos: [
                l(self.from.pos[0], self.to.pos[0]),
                l(self.from.pos[1], self.to.pos[1]),
                l(self.from.pos[2], self.to.pos[2]) + self.arc * (std::f32::consts::PI * t).sin(),
            ],
            yaw: l(self.from.yaw, self.to.yaw),
            tilt: l(self.from.tilt, self.to.tilt),
            flip: l(self.from.flip, self.to.flip),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Key {
    Hole(usize, usize),
    Board(usize),
}

#[derive(Debug, Clone, Copy)]
struct Flight {
    from: (f32, f32),
    to: (f32, f32),
    start: u32,
    chips: u8,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Mood {
    #[default]
    Idle,
    Fold,
    Cheer,
    Slump,
}

/// The table's memory. Presentation only.
#[derive(Debug, Clone, Default)]
pub struct PokerStage {
    clock: u32,
    last: Option<PokerView>,
    table: Option<PokerTableFrame>,
    local: usize,
    tweens: Vec<(Key, Tween)>,
    flights: Vec<Flight>,
    moods: Vec<(Mood, u32)>,
    bursts: Vec<(f32, f32, u32, usize)>,
    banner: Option<(String, u32)>,
    shot: Option<StageShot>,
    /// The frame the last showdown began, and its winner's seat.
    showdown: Option<(u32, Option<usize>)>,
    pending_cues: Vec<(u32, SoundRef)>,
    /// Whether this machine only watches the table (a spectating peer): no
    /// seat is "You", and no hand shows its faces before the showdown.
    pub watching: bool,
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Chips a stack of `amount` draws, against the big blind.
fn chips_for(amount: u32, unit: u32) -> u8 {
    if amount == 0 {
        return 0;
    }
    let u = unit.max(1);
    (2 + (amount / u).max(1).ilog2().min(4) * 2 + u32::from(amount >= u * 3)).min(12) as u8
}

fn colour_for(amount: u32, unit: u32) -> u8 {
    let r = amount / unit.max(1);
    match r {
        0..=1 => 0,
        2..=4 => 1,
        5..=19 => 2,
        20..=79 => 3,
        _ => 4,
    }
}

impl PokerStage {
    pub fn clock(&self) -> u32 {
        self.clock
    }

    /// The deck's table-local spot: beside the dealer's chair, on the felt.
    fn deck_spot(t: &PokerTableFrame, button_chair: usize) -> (f32, f32) {
        t.on_oval(button_chair, 46.0)
    }

    /// A chair's hole-card spot (the viewer's stand up before them).
    fn hole_pose(t: &PokerTableFrame, chair: usize, i: usize) -> CardPose {
        let (dr, da) = t.chair_dir(chair);
        let (r, a) = t.on_oval(chair, 34.0);
        // Along the edge: the chair's right hand.
        let side = (if i == 0 { -1.0 } else { 1.0 }) * 13.0;
        let (tr, ta) = (-da, dr);
        let yaw = (-dr).atan2(-da);
        if chair == 0 {
            CardPose {
                pos: [r + tr * side, a + ta * side - 6.0, 15.0],
                yaw: yaw + side * 0.012,
                tilt: 0.9,
                flip: 0.0,
            }
        } else {
            CardPose {
                pos: [r + tr * side, a + ta * side, 0.6 + i as f32 * 0.3],
                yaw,
                tilt: 0.0,
                // Face down.
                flip: std::f32::consts::PI,
            }
        }
    }

    fn board_pose(i: usize) -> CardPose {
        CardPose {
            pos: [(i as f32 - 2.0) * (CARD_HALF_W * 2.0 + 5.0), 6.0, 0.6],
            yaw: 0.0,
            tilt: 0.0,
            flip: 0.0,
        }
    }

    fn mood(&self, seat: usize, now: u32) -> Mood {
        match self.moods.get(seat) {
            Some(&(m, until)) if now < until => m,
            _ => Mood::Idle,
        }
    }

    /// Step one frame off `view`, seen from seat `local`.
    pub fn step(
        &mut self,
        table: &PokerTableFrame,
        view: &PokerView,
        local: usize,
    ) -> PokerStageFrame {
        let mut t = table.clone();
        t.seats = view.seats.len().max(2);
        let local = local.min(t.seats - 1);
        if self.table.as_ref() != Some(&t) || self.local != local {
            *self = Self {
                table: Some(t.clone()),
                local,
                watching: self.watching,
                ..Self::default()
            };
        }
        let now = self.clock;
        let n = t.seats;
        self.moods.resize(n, (Mood::Idle, 0));
        let prev = self.last.clone();
        // A contested showdown stays on the felt for its sweep and the hold
        // on the winner even once the session has dealt on: the table shows
        // the shown hands until then, and catches up after.
        let holding = self
            .showdown
            .is_some_and(|(start, _)| now < start + SHOWDOWN_HOLD)
            && prev.as_ref().is_some_and(|p| p.hand != view.hand);
        let held;
        let view = if holding {
            held = prev.clone().expect("held");
            &held
        } else {
            view
        };
        let mut cues = Vec::new();
        let unit = view.big_blind.max(1);
        let chair = |seat: usize| t.chair(seat, local);
        let button_chair = chair(view.button.min(n - 1));
        let deck = Self::deck_spot(&t, button_chair);
        let deck_pose = CardPose {
            pos: [deck.0, deck.1, 2.0],
            yaw: 0.4,
            tilt: 0.0,
            flip: std::f32::consts::PI,
        };

        // A new hand: the old cards are swept, the new ones dealt.
        let new_hand = prev.as_ref().is_none_or(|p| p.hand != view.hand);
        if new_hand {
            self.tweens.clear();
            self.showdown = None;
            let mut k = 0u32;
            for round in 0..2 {
                for off in 1..=n {
                    let seat = (view.button + off) % n;
                    if !view.seats[seat].in_hand {
                        continue;
                    }
                    let to = Self::hole_pose(&t, chair(seat), round);
                    let start = now + k * DEAL_GAP;
                    self.tweens.push((
                        Key::Hole(seat, round),
                        Tween {
                            from: deck_pose,
                            to,
                            start,
                            dur: DEAL_FRAMES,
                            arc: 18.0,
                        },
                    ));
                    self.pending_cues.push((start, t.sfx.deal.clone()));
                    k += 1;
                }
            }
        }
        // The board: each new card flies from the deck and turns up.
        for i in 0..view.board.len() {
            if self.tweens.iter().any(|(k, _)| *k == Key::Board(i)) {
                continue;
            }
            let had = if new_hand {
                0
            } else {
                prev.as_ref().map_or(0, |p| p.board.len())
            };
            let fresh = i >= had && prev.is_some();
            let start = now + (i.saturating_sub(had) as u32) * 6;
            let to = Self::board_pose(i);
            self.tweens.push((
                Key::Board(i),
                Tween {
                    from: if fresh { deck_pose } else { to },
                    to,
                    start,
                    dur: if fresh { DEAL_FRAMES + TURN_FRAMES } else { 1 },
                    arc: if fresh { 22.0 } else { 0.0 },
                },
            ));
            if fresh {
                self.pending_cues
                    .push((start + DEAL_FRAMES, t.sfx.flip.clone()));
            }
        }

        // Folds and showdown flips retarget the hole cards.
        let showdown = matches!(view.phase, Phase::Showdown { .. });
        for seat in 0..n {
            let s = &view.seats[seat];
            for i in 0..2 {
                let Some(pos) = self
                    .tweens
                    .iter()
                    .position(|(k, _)| *k == Key::Hole(seat, i))
                else {
                    continue;
                };
                let tw = self.tweens[pos].1;
                let mut target = Self::hole_pose(&t, chair(seat), i);
                if s.folded {
                    // Mucked: pushed a little way in from the seat, face
                    // down.
                    let (r, a) = t.on_oval(chair(seat), 58.0);
                    target = CardPose {
                        pos: [r, a, 0.6],
                        yaw: target.yaw + 0.5,
                        tilt: 0.0,
                        flip: std::f32::consts::PI,
                    };
                } else if showdown && s.hole.is_some() && chair(seat) != 0 {
                    // Shown: turned face up where it lies.
                    target.flip = std::f32::consts::TAU;
                    target.pos[2] = 0.6;
                }
                let differs = (tw.to.flip - target.flip).abs() > 1e-3
                    || (tw.to.pos[0] - target.pos[0]).abs() > 0.5
                    || (tw.to.pos[1] - target.pos[1]).abs() > 0.5;
                if differs {
                    self.tweens[pos].1 = Tween {
                        from: tw.at(now),
                        to: target,
                        start: now.max(tw.start.min(now + 60)),
                        dur: 16,
                        arc: 10.0,
                    };
                }
            }
        }

        // Chips: bets swept into the pot when a street closes; a pot to its
        // winners.
        let pot_spot = (0.0f32, -34.0f32);
        if let Some(p) = &prev {
            for seat in 0..n.min(p.seats.len()) {
                let (was, is) = (p.seats[seat].street_bet, view.seats[seat].street_bet);
                if is > was {
                    self.pending_cues.push((now, t.sfx.place.clone()));
                    let to = t.on_oval(chair(seat), 62.0);
                    let from = t.on_oval(chair(seat), 30.0);
                    self.flights.push(Flight {
                        from,
                        to,
                        start: now,
                        chips: chips_for(is - was, unit).min(5),
                    });
                }
                if was > 0 && is < was && p.hand == view.hand {
                    self.flights.push(Flight {
                        from: t.on_oval(chair(seat), 62.0),
                        to: pot_spot,
                        start: now,
                        chips: chips_for(was, unit).min(5),
                    });
                }
                let folded_now = view.seats[seat].folded && !p.seats[seat].folded;
                if folded_now {
                    self.moods[seat] = (Mood::Fold, now + REACT_FRAMES);
                }
            }
            if p.last_awards != view.last_awards && !view.last_awards.is_empty() {
                let mut best: Option<(usize, u32)> = None;
                for (winners, amount, _) in &view.last_awards {
                    for &w in winners {
                        if w >= n {
                            continue;
                        }
                        self.flights.push(Flight {
                            from: pot_spot,
                            to: t.on_oval(chair(w), 40.0),
                            start: now + 10,
                            chips: chips_for(*amount, unit).min(6),
                        });
                        self.moods[w] = (Mood::Cheer, now + REACT_FRAMES * 2);
                        if best.is_none_or(|b| *amount > b.1) {
                            best = Some((w, *amount));
                        }
                        let (r, a) = t.on_oval(chair(w), 40.0);
                        self.bursts.push((r, a, now + 10 + CHIP_FRAMES, w));
                    }
                }
                if let Some((w, amount)) = best {
                    let mine = !self.watching && w == local;
                    let who = if mine {
                        "You win".to_string()
                    } else {
                        format!(
                            "{} wins",
                            t.seat_name(w, if self.watching { usize::MAX } else { local })
                        )
                    };
                    let hand = view
                        .last_awards
                        .iter()
                        .find(|a| a.0.contains(&w))
                        .and_then(|a| a.2.as_ref())
                        .map(|h| format!(" - {}", category_label(h.category())))
                        .unwrap_or_default();
                    self.banner = Some((format!("{who} {amount}{hand}"), now + BANNER_FRAMES));
                    self.pending_cues.push((
                        now + 10,
                        if mine || self.watching {
                            t.sfx.win.clone()
                        } else {
                            t.sfx.lose.clone()
                        },
                    ));
                    if let Some(s) = self.showdown.as_mut() {
                        s.1 = Some(w);
                    }
                }
            }
            for seat in 0..n.min(p.seats.len()) {
                let busted = p.seats[seat].chips > 0
                    && view.seats[seat].chips == 0
                    && !view.seats[seat].in_hand;
                if busted {
                    self.moods[seat] = (Mood::Slump, u32::MAX);
                }
            }
        }
        // A contested showdown (two or more hands shown) gets the sweep.
        // A pot decided by comparing hands, not by everyone folding.
        let contested = view.last_awards.iter().any(|a| a.2.is_some())
            || view.seats.iter().filter(|s| s.in_hand && !s.folded).count() >= 2;
        if showdown && contested && self.showdown.is_none() {
            self.showdown = Some((now, None));
        }

        // The cards this frame.
        let (across, right) = t.axes();
        let c = t.centre();
        let top = t.top();
        let world = |r: f32, a: f32| -> [f32; 2] {
            [
                c[0] + right[0] * r + across[0] * a,
                c[1] + right[1] * r + across[1] * a,
            ]
        };
        let dir = |v: [f32; 3]| -> [f32; 3] {
            [
                right[0] * v[0] + across[0] * v[1],
                v[2],
                right[1] * v[0] + across[1] * v[1],
            ]
        };
        let hh = CARD_HALF_W * f32::from(art::CARD_H) / f32::from(art::CARD_W);
        let back = art::slot_rect(art::BACK_SLOT);
        let mut cards = Vec::new();
        for (key, tw) in &self.tweens {
            let pose = tw.at(now);
            let face = match *key {
                Key::Hole(seat, i) => art::slot_rect(art::hole_slot(seat, i)),
                Key::Board(i) => art::slot_rect(art::board_slot(i)),
            };
            // Hidden: a card the viewer may not see has no face to draw,
            // whatever way up it lies.
            let shown = match *key {
                Key::Hole(seat, _) => {
                    seat == local
                        || (showdown
                            && view
                                .seats
                                .get(seat)
                                .is_some_and(|s| !s.folded && s.hole.is_some()))
                }
                Key::Board(_) => true,
            };
            let (sf, cf) = pose.flip.sin_cos();
            let (st, ct) = pose.tilt.sin_cos();
            let (sy, cy) = pose.yaw.sin_cos();
            let r0 = [cf, 0.0, -sf];
            let n0 = [sf, 0.0, cf];
            let u1 = [n0[0] * st, ct + n0[1] * st, n0[2] * st];
            let yaw = |v: [f32; 3]| [v[0] * cy - v[1] * sy, v[0] * sy + v[1] * cy, v[2]];
            let k = |v: [f32; 3], s: f32| [v[0] * s, v[1] * s, v[2] * s];
            cards.push(StageCard {
                deck: match *key {
                    Key::Hole(s, _) => s,
                    Key::Board(_) => usize::MAX,
                },
                card: match *key {
                    Key::Hole(_, i) | Key::Board(i) => i,
                },
                owner: 0,
                face_uv: if shown { face } else { back },
                back_uv: back,
                backing: false,
                centre: world(pose.pos[0], pose.pos[1]),
                height: top + pose.pos[2],
                right: dir(k(yaw(r0), CARD_HALF_W)),
                up: dir(k(yaw(u1), hh)),
                lit: false,
                airborne: (pose.pos[2] / 24.0).clamp(0.0, 1.0),
            });
        }
        // The deck itself while it deals.
        cards.push(StageCard {
            deck: usize::MAX,
            card: usize::MAX,
            owner: 0,
            face_uv: back,
            back_uv: back,
            backing: false,
            centre: world(deck.0, deck.1),
            height: top + 2.5,
            right: dir([-CARD_HALF_W * 0.92, -0.38 * CARD_HALF_W, 0.0]),
            up: dir([-0.38 * hh, 0.92 * hh, 0.0]),
            lit: false,
            airborne: 0.0,
        });

        // Chip stacks: each seat's chips, its bet, the pot.
        let mut stacks = Vec::new();
        let mut amounts = Vec::new();
        for (seat, s) in view.seats.iter().enumerate() {
            let ch = chair(seat);
            if s.chips > 0 {
                let (r, a) = t.on_oval(ch, 22.0);
                let (dr, da) = t.chair_dir(ch);
                let (sr, sa) = (da * 26.0, -dr * 26.0);
                stacks.push(ChipStack {
                    at: world(r + sr, a + sa),
                    height: top,
                    chips: chips_for(s.chips, unit),
                    colour: colour_for(s.chips, unit),
                });
            }
            if s.street_bet > 0 {
                let (r, a) = t.on_oval(ch, 62.0);
                stacks.push(ChipStack {
                    at: world(r, a),
                    height: top,
                    chips: chips_for(s.street_bet, unit),
                    colour: colour_for(s.street_bet, unit),
                });
                amounts.push(StageAmount {
                    at: world(r, a),
                    height: top + 30.0,
                    value: s.street_bet,
                });
            }
        }
        let pot_shown = view
            .pot
            .saturating_sub(view.seats.iter().map(|s| s.street_bet).sum());
        if pot_shown > 0 && !matches!(view.phase, Phase::Showdown { .. }) {
            stacks.push(ChipStack {
                at: world(pot_spot.0, pot_spot.1),
                height: top,
                chips: chips_for(pot_shown, unit) + 2,
                colour: colour_for(pot_shown, unit),
            });
            amounts.push(StageAmount {
                at: world(pot_spot.0, pot_spot.1 - 4.0),
                height: top + 36.0,
                value: pot_shown,
            });
        }
        // Chips in flight.
        self.flights.retain(|f| now < f.start + CHIP_FRAMES);
        for f in &self.flights {
            if now < f.start {
                continue;
            }
            let u = smooth((now - f.start) as f32 / CHIP_FRAMES as f32);
            let (r, a) = (
                f.from.0 + (f.to.0 - f.from.0) * u,
                f.from.1 + (f.to.1 - f.from.1) * u,
            );
            stacks.push(ChipStack {
                at: world(r, a),
                height: top + 6.0 * (std::f32::consts::PI * u).sin(),
                chips: f.chips,
                colour: 1,
            });
        }
        let button = Some({
            let (r, a) = t.on_oval(button_chair, 50.0);
            let (dr, da) = t.chair_dir(button_chair);
            world(r - da * 30.0, a + dr * 30.0)
        });
        let to_act = match (view.phase, view.to_act) {
            (Phase::Betting(_), Some(s)) => {
                let (r, a) = t.on_oval(chair(s), 30.0);
                Some(world(r, a))
            }
            _ => None,
        };
        self.bursts.retain(|b| now < b.2 + 32);
        let bursts = self
            .bursts
            .iter()
            .filter(|b| now >= b.2)
            .map(|b| StageBurst {
                at: world(b.0, b.1),
                height: top + 1.0,
                age: now - b.2,
                kind: BurstKind::Capture,
                seat: if b.3 == local { 0 } else { 1 },
            })
            .collect();

        // The NPCs: the k-th other seat's NPC sits in that seat's chair.
        let mut npcs = Vec::new();
        let centre = world(0.0, 0.0);
        let order: Vec<usize> = (0..n).filter(|&s| s != 0).collect();
        for (k, npc) in t.players.iter().enumerate() {
            // NPC `k` plays session seat `order[k]` (seat 0 is the
            // single-player seat); a networked viewer elsewhere sees it in
            // that seat's chair all the same.
            let Some(&seat) = order.get(k) else {
                break;
            };
            if seat == local {
                // A networked viewer in this seat stands there themselves.
                continue;
            }
            let s = &view.seats[seat];
            let mood = self.mood(seat, now);
            let thinking = matches!(view.phase, Phase::Betting(_)) && view.to_act == Some(seat);
            let clip = match mood {
                Mood::Fold => npc.fold.or(npc.slump),
                Mood::Cheer => npc.cheer,
                Mood::Slump => npc.slump,
                Mood::Idle if s.all_in => npc.all_in,
                Mood::Idle if thinking => npc.think,
                Mood::Idle => None,
            };
            npcs.push(DuelNpcPose {
                npc: npc.npc.clone(),
                pos: Some(t.chair_pos(chair(seat))),
                look_at: centre,
                clip,
            });
        }
        let local_cheer = self.mood(local, now) == Mood::Cheer;
        for s in &t.spectators {
            npcs.push(DuelNpcPose {
                npc: s.npc.clone(),
                pos: None,
                look_at: centre,
                clip: if local_cheer { s.cheer } else { None },
            });
        }

        for (at, cue) in &self.pending_cues {
            if *at == now {
                cues.push(cue.clone());
            }
        }
        self.pending_cues.retain(|(at, _)| *at > now);
        let shot = self.shot(&t, view, now);
        let banner = self
            .banner
            .as_ref()
            .filter(|(_, until)| now < *until)
            .map(|(s, _)| s.clone());
        // What the card page may hold: the viewer's hand, the board, and
        // the hands shown at a showdown.
        let visible = VisibleCards {
            holes: view
                .seats
                .iter()
                .enumerate()
                .map(|(seat, s)| {
                    let open = (!self.watching && seat == local) || (showdown && !s.folded);
                    if open { s.hole } else { None }
                })
                .collect(),
            board: view.board.clone(),
        };
        self.last = Some(view.clone());
        self.clock = now + 1;
        PokerStageFrame {
            table: t,
            local,
            clock: now,
            cards,
            stacks,
            amounts,
            button,
            to_act,
            deck: world(deck.0, deck.1),
            bursts,
            npcs,
            cues,
            shot,
            banner,
            visible,
        }
    }

    /// The leader: standing in the viewer's chair.
    pub fn leader(&self, t: &PokerTableFrame) -> StagePose {
        let p = t.chair_pos(0);
        let c = t.centre();
        let a =
            ((c[1] - p[1]).atan2(c[0] - p[0]) / std::f32::consts::TAU * 4096.0).rem_euclid(4096.0);
        // A step back and to the left, so the camera reads the hand over
        // the leader's shoulder.
        let (across, right) = t.axes();
        StagePose {
            pos: [
                p[0] - right[0] * 96.0 - across[0] * 6.0,
                p[1] - right[1] * 96.0 - across[1] * 6.0,
            ],
            facing: a as u16,
            clip: LeaderClip::Idle,
        }
    }

    fn shot(&mut self, t: &PokerTableFrame, view: &PokerView, now: u32) -> StageShot {
        let (across, right) = t.axes();
        let look = (across[1].atan2(across[0]) / std::f32::consts::TAU * 4096.0).rem_euclid(4096.0);
        let c = t.centre();
        let top = t.top();
        let drift = (now as f32 / 260.0 * std::f32::consts::TAU).sin();
        let mut target = StageShot {
            focus: [c[0] - across[0] * 24.0, c[1] - across[1] * 24.0],
            yaw: look + drift * 26.0,
            pitch: 430.0,
            distance: 0.24,
            lift: top,
        };
        // Lean toward the seat to act.
        if let (Phase::Betting(_), Some(s)) = (view.phase, view.to_act)
            && s != self.local
        {
            let (dr, _) = t.chair_dir(t.chair(s, self.local));
            target.yaw += dr * 70.0;
            target.focus[0] += right[0] * dr * 30.0;
            target.focus[1] += right[1] * dr * 30.0;
        }
        if matches!(view.phase, Phase::Betting(Street::Preflop)) && view.board.is_empty() {
            target.distance = 0.22;
        }
        let mut k = 0.07;
        if now < ESTABLISH_FRAMES {
            let u = smooth(now as f32 / ESTABLISH_FRAMES as f32);
            target = StageShot {
                focus: [c[0], c[1]],
                yaw: look + 900.0 * (1.0 - u),
                pitch: 470.0,
                distance: 0.62 - 0.36 * u,
                lift: top * 0.5,
            };
            k = if now < ESTABLISH_FRAMES / 2 {
                1.0
            } else {
                0.08
            };
        } else if let Some((start, winner)) = self.showdown {
            let age = now.saturating_sub(start) as f32;
            let sweep = (age / SWEEP_FRAMES).min(1.0);
            // A sweep round the table over the shown hands, then a hold on
            // the winner.
            target = StageShot {
                focus: [c[0], c[1]],
                yaw: look - 480.0 * smooth(sweep),
                pitch: 560.0,
                distance: 0.29,
                lift: top,
            };
            if age > SWEEP_FRAMES
                && let Some(w) = winner
                && w != self.local
            {
                let p = t.chair_pos(t.chair(w, self.local));
                let to = [c[0] - p[0], c[1] - p[1]];
                let yaw = (to[1].atan2(to[0]) / std::f32::consts::TAU * 4096.0 + 2048.0)
                    .rem_euclid(4096.0);
                target = StageShot {
                    focus: [p[0] + to[0] * 0.25, p[1] + to[1] * 0.25],
                    yaw,
                    pitch: 300.0,
                    distance: 0.14,
                    lift: top + 30.0,
                };
            }
            k = 0.16;
        }
        let s = match self.shot {
            None => target,
            Some(s) => s.ease_to(&target, k),
        };
        self.shot = Some(s);
        s
    }

    pub fn table(&self) -> Option<&PokerTableFrame> {
        self.table.as_ref()
    }

    pub fn last_view(&self) -> Option<&PokerView> {
        self.last.as_ref()
    }

    pub fn local(&self) -> usize {
        self.local
    }

    /// Note the session's last view as it leaves play.
    pub fn note_final(&mut self, view: PokerView) {
        if self.table.is_some() {
            self.last = Some(view);
        }
    }
}

#[cfg(test)]
mod tests;
