//! The catch celebration and the line sub-state actor.

// --- Catch celebration and line sub-state (FUN_801D4948) -------------------

/// The reeling-line actor's sub-state (`DAT_801D91C8`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinePhase {
    /// `0` - arm: seed the hook cue and step to [`LinePhase::Attach`].
    Arm,
    /// `1` - copy the hooked fish's position out of `actor[+0x48]`, then
    /// step to [`LinePhase::Track`].
    Attach,
    /// `2` - track the hooked fish each frame.
    Track,
    /// `4` - the catch celebration. The published sub-state list omits this
    /// arm; it is the bulk of the routine.
    Celebrate,
    /// Any other value: the routine leaves the actor alone.
    Idle(u32),
}

impl LinePhase {
    /// Decode the raw sub-state word.
    ///
    /// PORT: FUN_801d4948 (sub-state decode)
    // Wired: [`LineActorSim`] owns the engine's `DAT_801D91C8` stand-in and
    // decodes it here every tick; the play window drives the sim across the
    // hook -> fight -> celebration phases (`window/minigames.rs`).
    pub fn from_raw(v: u32) -> LinePhase {
        match v {
            0 => LinePhase::Arm,
            1 => LinePhase::Attach,
            2 => LinePhase::Track,
            4 => LinePhase::Celebrate,
            other => LinePhase::Idle(other),
        }
    }
}

/// SFX cue the arm phase raises (`_DAT_8007B6DA = 0x3A`).
pub const HOOK_CUE: u8 = 0x3A;

/// SFX cue the celebration's first stage raises.
pub const CELEBRATE_CUE: u8 = 0x2B;

/// One firework burst of the catch celebration: the score threshold that
/// unlocks it, its spawn offset and the SFX cue it raises (`None` for the
/// top tier, which is silent).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CelebrationBurst {
    /// Exclusive lower bound on the catch score `DAT_801D91B8`.
    pub above: i32,
    /// Spawn offset `(x, y, z)`.
    pub offset: (i16, i16, i16),
    /// Cue raised alongside the burst.
    pub cue: Option<u8>,
}

impl CelebrationBurst {
    /// The burst's world point for a catch at world `(x, z)` `origin`: the
    /// spawn offset added to the origin's `x` / `z`, its `y` taken as is
    /// (the on-stack trio `FUN_801D4948` hands each `FUN_80021B04`). A host
    /// projects it through the venue camera to seat the part.
    pub fn world_point(&self, origin: (i16, i16)) -> [i32; 3] {
        [
            origin.0 as i32 + self.offset.0 as i32,
            self.offset.1 as i32,
            origin.1 as i32 + self.offset.2 as i32,
        ]
    }
}

/// The four bursts, in the order retail evaluates them. Every tier whose
/// threshold the score clears fires, so a big catch plays all four.
///
/// The address tag lives on [`celebration_bursts`], which reads this table -
/// a tag on the `const` resolves to no code anchor and the audit widens it to
/// the whole module.
///
/// REF: FUN_801d4948 (celebration tiers)
pub const CELEBRATION_BURSTS: [CelebrationBurst; 4] = [
    CelebrationBurst {
        above: 200,
        offset: (0x190, 0x190, 1000),
        cue: Some(0x25),
    },
    CelebrationBurst {
        above: 600,
        offset: (0x190, -0x190, 800),
        cue: Some(0x26),
    },
    CelebrationBurst {
        above: 800,
        offset: (-0x190, 0, 800),
        cue: Some(0x27),
    },
    CelebrationBurst {
        above: 0x4B0,
        offset: (0, 0, 1000),
        cue: None,
    },
];

/// The bursts a catch score unlocks.
///
/// PORT: FUN_801d4948 (celebration gate)
// Wired: [`LineActorSim::tick`]'s celebrate arm resolves the unlocked tiers
// at the first stage frame; `World::tick_fishing` queues each `cue` on the
// world's SFX channel at the session's catch edge (so all three hosts hear
// it), and the play window spawns the bursts into its effect pool (offset
// from the wander actor's catch position) in `window/minigames.rs`.
pub fn celebration_bursts(score: i32) -> impl Iterator<Item = &'static CelebrationBurst> {
    CELEBRATION_BURSTS.iter().filter(move |b| score > b.above)
}

/// Frame counts on the celebration actor's `+0x22` timer at which the
/// celebration advances stage: `(fire the bursts, fire the two flashes,
/// hand the actor back)`. Each test is `stage < timer` on the timer as the
/// frame **starts** (the `+= frame_step` is the arm's last store), so a stage
/// fires on the first frame whose starting timer is past it - frame `0xD4`
/// of the close-up hands back, at step `1`.
pub const CELEBRATION_STAGE_FRAMES: (i16, i16, i16) = (0x72, 0x87, 0xD2);

/// The two screen fades the celebration's second stage spawns
/// (`0x801D50C8..0x801D5138`, through `FUN_801D7964`, which unpacks its
/// arguments into a fade template and spawns it with `FUN_80024E80(t, 1)`):
/// an additive (kind `1`) ramp from black to white over `0x3C` frames held
/// `0x50` more, and a white-to-black ramp over `0x1E` frames after a
/// `0x8B`-frame delay - the white-out the close-up climbs into and its
/// release on the showcase. The spawn wrapper stamps id `1` (`sh s2,0x18`
/// at `0x80024EBC`), the quad's ordering-table bucket: in front of the HUD
/// sprites, behind the font text.
// REF: FUN_801d7964 (the template it unpacks), FUN_80024E80 (the spawn and its id stamp)
pub const CATCH_FLASH_FADES: [legaia_engine_system::fade::FadeTemplate; 2] = [
    legaia_engine_system::fade::FadeTemplate {
        kind: 1,
        duration: 0x3C,
        start_rgb: [0, 0, 0],
        end_rgb: [0xFF, 0xFF, 0xFF],
        mode: [0, 0x50, 1],
    },
    legaia_engine_system::fade::FadeTemplate {
        kind: 1,
        duration: 0x1E,
        start_rgb: [0xFF, 0xFF, 0xFF],
        end_rgb: [0, 0, 0],
        mode: [0x8B, 0, 1],
    },
];

/// What one [`LineActorSim::tick`] asks the host to do.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineActorFrame {
    /// SFX cue to raise this frame ([`HOOK_CUE`] on the arm phase,
    /// [`CELEBRATE_CUE`] at the celebration's first stage).
    pub cue: Option<u8>,
    /// Celebration bursts unlocked this frame (the first stage's
    /// [`celebration_bursts`] resolution) - the host spawns each at its
    /// offset from the catch position and fires its own `cue`.
    pub bursts: Vec<CelebrationBurst>,
    /// The celebration's second stage: the two white-out screen fades
    /// ([`CATCH_FLASH_FADES`]) spawn this frame.
    pub flash: bool,
    /// The celebration ran its `+0x22` timer out - the actor hands back.
    pub done: bool,
}

/// The reeling-line actor as one advancing object: the engine's stand-in for
/// the `DAT_801D91C8` sub-state word plus the celebration's `+0x22` timer,
/// decoded through [`LinePhase::from_raw`] every tick exactly as the retail
/// handler switches on the raw word.
///
/// A host arms it on the hook, ticks it while the fight runs, calls
/// [`LineActorSim::land`] with the catch score, and keeps ticking until
/// [`LineActorFrame::done`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineActorSim {
    /// The raw sub-state word (`DAT_801D91C8`).
    pub raw: u32,
    /// The celebration timer (`actor + 0x22`).
    pub timer: i16,
    /// The landed catch score (`DAT_801D91B8`), set by [`LineActorSim::land`].
    pub score: i32,
}

impl LineActorSim {
    /// A line actor at the arm phase (the hook just set).
    pub fn hooked() -> Self {
        LineActorSim::default()
    }

    /// The catch landed for `score` points: enter the celebration arm.
    pub fn land(&mut self, score: i32) {
        self.raw = 4;
        self.timer = 0;
        self.score = score;
    }

    /// One frame of the line actor's handler.
    pub fn tick(&mut self, frame_step: i16) -> LineActorFrame {
        let mut out = LineActorFrame::default();
        match LinePhase::from_raw(self.raw) {
            LinePhase::Arm => {
                out.cue = Some(HOOK_CUE);
                self.raw = 1;
            }
            LinePhase::Attach => {
                // The host copies the hooked fish's position; the sim only
                // steps the sub-state.
                self.raw = 2;
            }
            LinePhase::Track => {}
            LinePhase::Celebrate => {
                // Retail tests the timer as the frame starts and adds the
                // step last (`0x801D4948` sub-state 4).
                let before = self.timer;
                self.timer = self.timer.saturating_add(frame_step);
                let crossed =
                    |at: i16| before > at && before.saturating_sub(frame_step.max(1)) <= at;
                if crossed(CELEBRATION_STAGE_FRAMES.0) {
                    out.cue = Some(CELEBRATE_CUE);
                    out.bursts = celebration_bursts(self.score).copied().collect();
                }
                if crossed(CELEBRATION_STAGE_FRAMES.1) {
                    out.flash = true;
                }
                if crossed(CELEBRATION_STAGE_FRAMES.2) {
                    out.done = true;
                }
            }
            LinePhase::Idle(_) => {}
        }
        out
    }
}
