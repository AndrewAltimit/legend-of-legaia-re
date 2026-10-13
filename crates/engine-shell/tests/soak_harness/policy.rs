//! Policies: the seeded RNG, the pad constants and the game-shaped random
//! input policy (walk segments, talk, menus, fights).

use super::*;

// ---------------------------------------------------------------------------
// RNG
// ---------------------------------------------------------------------------

/// SplitMix64 - tiny, seedable, and stable across platforms.
#[derive(Clone)]
pub(super) struct Rng(u64);

impl Rng {
    pub(super) fn new(seed: u64) -> Self {
        Self(seed ^ 0x9E37_79B9_7F4A_7C15)
    }
    pub(super) fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    pub(super) fn below(&mut self, n: u32) -> u32 {
        (self.next_u64() % u64::from(n.max(1))) as u32
    }
    pub(super) fn range(&mut self, lo: u32, hi: u32) -> u32 {
        lo + self.below(hi - lo + 1)
    }
    pub(super) fn chance(&mut self, pct: u32) -> bool {
        self.below(100) < pct
    }
}

/// Per-(scene, seed) policy seed, so one scene's run does not depend on the
/// order the scene list happens to be in.
pub(super) fn run_seed(scene: &str, seed: u64) -> u64 {
    // The card a `@<card>:<save>` label names is where the save is read
    // from, not part of the run: the stream is the `@<save>` label's.
    let scene = match scene.split_once('@') {
        Some((label, save)) => match save.split_once(':') {
            Some((_, save)) => format!("{label}@{save}"),
            None => scene.to_string(),
        },
        None => scene.to_string(),
    };
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in scene.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01B3);
    }
    h ^ seed.wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

// ---------------------------------------------------------------------------
// Pad
// ---------------------------------------------------------------------------

pub(super) const UP: u16 = 0x0010;
pub(super) const RIGHT: u16 = 0x0020;
pub(super) const DOWN: u16 = 0x0040;
pub(super) const LEFT: u16 = 0x0080;
pub(super) const DIRS: [u16; 4] = [UP, RIGHT, DOWN, LEFT];

pub(super) fn b(p: PadButton) -> u16 {
    p.mask()
}

// ---------------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Segment {
    /// Hold one or two directions; occasionally tap Cross.
    Walk { dirs: u16, confirm_every: u32 },
    /// Alternate Cross / Circle taps.
    Mash,
    /// Press Start once; the menu arm below takes over while it is open.
    OpenMenu,
    /// Neutral pad.
    Idle,
    /// A fresh random mask every few frames, every button but Start.
    Chaos,
}

/// The seeded input source. A pure function of its seed and the observed
/// session, so `(scene, seed, frames)` alone regenerates a run; the recorded
/// pad stream is what the replay file carries.
pub(super) struct Policy {
    pub(super) rng: Rng,
    pub(super) seg: Segment,
    pub(super) seg_left: u32,
    /// Frames the pause menu has been open this visit.
    pub(super) menu_frames: u32,
    /// How long this menu visit browses before it starts backing out.
    pub(super) menu_budget: u32,
    /// Chaos mask currently held and for how long.
    pub(super) chaos_mask: u16,
    pub(super) chaos_left: u32,
    /// Tap duty cycle: the last frame's mask.
    pub(super) last: u16,
}

impl Policy {
    pub(super) fn new(seed: u64) -> Self {
        Self {
            rng: Rng::new(seed),
            seg: Segment::Idle,
            seg_left: 0,
            menu_frames: 0,
            menu_budget: 0,
            chaos_mask: 0,
            chaos_left: 0,
            last: 0,
        }
    }

    /// A tap: the wanted mask on one frame, neutral on the next. Every UI
    /// surface reads `just_pressed`, so a held mask is one event.
    pub(super) fn tap(&mut self, want: u16) -> u16 {
        if self.last != 0 { 0 } else { want }
    }

    pub(super) fn random_face(&mut self) -> u16 {
        match self.rng.below(10) {
            0..=4 => b(PadButton::Cross),
            5..=7 => b(PadButton::Circle),
            8 => b(PadButton::Triangle),
            _ => b(PadButton::Square),
        }
    }

    pub(super) fn random_dir(&mut self) -> u16 {
        DIRS[self.rng.below(4) as usize]
    }

    pub(super) fn next_segment(&mut self) {
        let r = self.rng.below(100);
        let (seg, len) = if r < 42 {
            let d0 = self.random_dir();
            let dirs = if self.rng.chance(25) {
                // A diagonal: two adjacent directions.
                let i = DIRS.iter().position(|&d| d == d0).unwrap_or(0);
                d0 | DIRS[(i + 1) % 4]
            } else {
                d0
            };
            let confirm_every = if self.rng.chance(35) {
                self.rng.range(8, 40)
            } else {
                0
            };
            (
                Segment::Walk {
                    dirs,
                    confirm_every,
                },
                self.rng.range(15, 160),
            )
        } else if r < 67 {
            (Segment::Mash, self.rng.range(20, 140))
        } else if r < 77 {
            (Segment::OpenMenu, 2)
        } else if r < 87 {
            (Segment::Idle, self.rng.range(5, 60))
        } else {
            (Segment::Chaos, self.rng.range(20, 120))
        };
        self.seg = seg;
        self.seg_left = len;
    }

    pub(super) fn pad(&mut self, s: &NativeSession, frame: u64) -> u16 {
        let pad = self.pad_inner(s, frame);
        self.last = pad;
        pad
    }

    pub(super) fn pad_inner(&mut self, s: &NativeSession, frame: u64) -> u16 {
        let w = &s.host.world;
        // Pause menu open: browse at random, then back out with Circle.
        if s.field_menu.is_some() {
            if self.menu_frames == 0 {
                self.menu_budget = self.rng.range(30, 420);
            }
            self.menu_frames += 1;
            if self.menu_frames > self.menu_budget {
                return self.tap(b(PadButton::Circle));
            }
            let want = match self.rng.below(10) {
                0..=4 => self.random_dir(),
                5..=6 => b(PadButton::Cross),
                7..=8 => b(PadButton::Circle),
                _ => b(PadButton::Triangle),
            };
            return self.tap(want);
        }
        self.menu_frames = 0;

        match w.mode {
            SceneMode::Battle => {
                if self.rng.chance(75) {
                    let want = fight_pad(s);
                    if want != 0 {
                        return self.tap(want);
                    }
                }
                let want = if self.rng.chance(50) {
                    self.random_dir()
                } else {
                    self.random_face()
                };
                self.tap(want)
            }
            SceneMode::Field | SceneMode::WorldMap => {
                // A dialogue or a picker owns the pad: mostly confirm, some
                // directions (pickers), some cancel.
                // A script that holds the player (a cutscene timeline, an
                // engaging record) pages its own text boxes, which
                // `dialogue_owns_input` does not see: it gets the same
                // presses.
                if (w.dialogue_owns_input() || w.script_context_engages_player())
                    && self.rng.chance(70)
                {
                    let want = match self.rng.below(10) {
                        0..=5 => b(PadButton::Cross),
                        6..=7 => self.random_dir(),
                        _ => b(PadButton::Circle),
                    };
                    return self.tap(want);
                }
                self.field_pad(frame)
            }
            // Minigames, cutscenes, the title: every face button and held
            // directions, and now and then Start - the minigames' escape
            // (`World::poll_minigame_escape`), so the exit paths are soaked
            // too without cutting every session short.
            _ => {
                if self.rng.below(1500) == 0 {
                    return self.tap(b(PadButton::Start));
                }
                if self.chaos_left == 0 {
                    self.chaos_left = self.rng.range(2, 12);
                    self.chaos_mask = match self.rng.below(4) {
                        0 => self.random_dir(),
                        1 => self.random_face(),
                        2 => 0,
                        _ => self.random_dir() | self.random_face(),
                    };
                }
                self.chaos_left -= 1;
                if self.chaos_mask & 0xF000 != 0 {
                    self.tap(self.chaos_mask)
                } else {
                    self.chaos_mask
                }
            }
        }
    }

    pub(super) fn field_pad(&mut self, frame: u64) -> u16 {
        if self.seg_left == 0 {
            self.next_segment();
        }
        self.seg_left -= 1;
        match self.seg {
            Segment::Walk {
                dirs,
                confirm_every,
            } => {
                if confirm_every != 0 && frame.is_multiple_of(u64::from(confirm_every)) {
                    dirs | b(PadButton::Cross)
                } else {
                    dirs
                }
            }
            Segment::Mash => {
                let want = if self.rng.chance(70) {
                    b(PadButton::Cross)
                } else {
                    b(PadButton::Circle)
                };
                self.tap(want)
            }
            Segment::OpenMenu => self.tap(b(PadButton::Start)),
            Segment::Idle => 0,
            Segment::Chaos => {
                if self.chaos_left == 0 {
                    self.chaos_left = self.rng.range(2, 8);
                    let mut m = 0u16;
                    for bit in 0..16u16 {
                        let mask = 1u16 << bit;
                        if mask == b(PadButton::Start) {
                            continue;
                        }
                        if self.rng.chance(12) {
                            m |= mask;
                        }
                    }
                    self.chaos_mask = m;
                }
                self.chaos_left -= 1;
                self.chaos_mask
            }
        }
    }
}

/// The battle fighter's wanted press this frame (see `critical_path_replay`'s
/// `FightPolicy`): Begin, Attack, Auto, confirm the target, Cross any message
/// box. `0` when nothing obvious is wanted.
pub(super) fn fight_pad(s: &NativeSession) -> u16 {
    use legaia_engine_menus::battle_input::CommandPhase;
    use legaia_engine_menus::inventory_use::InventoryUseState;
    let w = &s.host.world;
    if !w.battle.tutorial_boxes.is_empty() {
        return b(PadButton::Cross);
    }
    if let Some(menu) = w.battle.item_menu.as_ref() {
        return match menu.state {
            InventoryUseState::Browsing { .. } => b(PadButton::Circle),
            InventoryUseState::TargetSelect { .. } => b(PadButton::Cross),
            _ => 0,
        };
    }
    if let Some(session) = w.battle.command.as_ref() {
        return match &session.phase {
            CommandPhase::RoundPrompt { .. } => LEFT,
            CommandPhase::Menu { .. } => LEFT,
            CommandPhase::AttackMode { .. } => LEFT,
            CommandPhase::Targeting { .. } => b(PadButton::Cross),
            CommandPhase::CommitConfirm { .. } => LEFT,
            _ => 0,
        };
    }
    b(PadButton::Cross)
}
