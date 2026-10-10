use super::*;

/// Ticks the pad path may take to reach a menu capture's surface.
pub(super) const MENU_DRIVE_TICKS: u32 = 600;
/// Ticks a menu capture's surface is held, with no input, before it is
/// sampled. A retail menu capture is a surface the player was sitting on,
/// so its camera has finished whatever transition opened it: the case-`0`
/// glide onto a member (`FUN_801D829C`, `a3 = 0xC`: 12 display frames), or
/// the submenu-exit swing and return to the far framing that the commit
/// confirm opens on (6 + 7 camera steps, 26 frames). The drive reaches the
/// surface on the tick it opens, so sampling then reads the transition's
/// first step - a clock reading, not the framing. Once the camera is in the
/// hold changes nothing: the surface waits for input.
pub const MENU_HOLD_TICKS: u32 = 32;
/// Ticks the pad path may take to reach an in-flight capture's action-SM
/// state: long enough for several rounds, since the seat's turn comes up in
/// initiative order.
pub(super) const ACTION_DRIVE_TICKS: u32 = 4000;
/// The extra capture deadline a driven `play-window` child gets.
pub const DRIVE_DEADLINE: u64 = ACTION_DRIVE_TICKS as u64;

/// How a capture that sits past the round prompt is reached through the
/// engine's own pad path - the headless seed and the `play-window` image
/// child run the same driver (`LEGAIA_BATTLE_DRIVE`), so the frame is taken
/// at the phase the state channels were scored at.
///
/// Seats are **retail** pool slots (`ctx[+0x13]`); [`engine_seat`] maps
/// them onto the engine's seating.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattleDrive {
    /// An opening capture ([`SeedPlan::Opening`]): nothing is pressed, and
    /// the frame is the first one in battle mode whose entry sweep has run
    /// as far as retail's - the instant the headless seed samples. Retail's opening holds the fight on its intro (the
    /// tutorial's first speech, the formation reveal) while the engine's
    /// round prompt opens with it, so a frame taken past the prompt shows a
    /// surface retail had not reached.
    ///
    /// `swept` is set for a capture whose flow byte is past the intro timer
    /// (`0x0C` / `0x14`): the enemy-name labels `0x0B` sweeps are gone there,
    /// so the frame waits for the engine's own intro names to clear
    /// (`World::battle.intro_names_frames`) - and, in the sparring fight, for
    /// the round start they were holding back, which is what raises the
    /// side-band's caption a `0x14` capture shows.
    ///
    /// `entry` is retail's frame-driver counter `gp+0x330`: the frame also
    /// waits for the engine's battle-entry sweep to run as far
    /// ([`entry_sweep_reached`]).
    Opening { swept: bool, entry: u8 },
    /// A command-selection surface on party seat `seat`: members ahead of
    /// it commit a plain Attack, the seat itself takes the arm that leads
    /// to `flow`.
    Menu { flow: BattleFlowState, seat: u8 },
    /// An action in flight: rounds are committed (a plain Attack each, the
    /// capture's own seat Spirit when that is what it had committed) until
    /// the action SM holds `state` on `seat`. A monster seat that was
    /// casting (`category == 2`) casts the capture's spell `queued` on its
    /// next turn ([`legaia_engine_core::world::BattleState::forced_monster_cast`]);
    /// the party commits Spirit throughout, so the caster is still standing
    /// when that turn comes (Zeto's captures sit in a party able to kill him
    /// in two swings).
    ///
    /// `spare` is set on a killing-blow capture
    /// ([`RetailBattle::action_victims`]): the victim is replayed at `1` HP,
    /// so any member's swing would kill it, and the members other than the
    /// capture's seat commit Spirit instead of Attack - the kill is then the
    /// seat's own, whatever order initiative put them in.
    ///
    /// `absorbed` is the Seru a capture on the Done band already staged
    /// (`ctx[+0x269]`, [`RetailBattle::absorbed_seru`]). Retail's grant ran
    /// before the capture, so the lifted save's spell list already holds
    /// spell `absorbed + 0x80` - and the replayed kill's absorb lookup would
    /// answer "known" and stage nothing. [`Self::prime`] takes it back off
    /// the seat's list, the twin of crediting a cast's MP back.
    ///
    /// `style` is the capture's framing style `ctx[+0xD]`, a draw the action
    /// seed rolls per action (`rand() % 2 * 2`, `rand() % 4`, ...). A
    /// replay on another stream rolls its own, and the post-strike cases
    /// fork on it (pitch `0x80` / `TR.y 0x400` against level, a half-turn),
    /// so [`Self::steer`] sets the acting seat's style to retail's while it
    /// holds the capture's state (outside the capture band, [`CAPTURE_BAND`])
    /// - the camera twin of the orbit-yaw alignment.
    Action {
        seat: u8,
        state: u8,
        category: u8,
        queued: u8,
        spare: bool,
        absorbed: u8,
        end: SpanGate,
        style: Option<u8>,
        steer: ActionSteer,
    },
}

/// The two further capture facts an action drive steers by
/// ([`BattleDrive::Action`]).
///
/// `target` is the capture's target seat `actor[+0x1DD]`, set only on a
/// killing-blow capture whose target is one of its victims
/// ([`RetailBattle::action_victims`]). The phase is then reached only with
/// the acting seat on that target: with the victim seeded at its read HP of
/// `0` the auto-target picks the next standing monster, and a run that held
/// the right state against the wrong body scored the camera of a different
/// shot (`player_steal_skeleton_banner` framed the live skeleton where
/// retail's case 8 frames the falling one), so the search moves on to the
/// `1`-HP re-run that makes the kill. On a monster's cast it is the party
/// seat the cast aimed at, which the replayed cast is steered onto
/// (`BattleState::forced_monster_target`): the caster turns to face its
/// target as the cast begins, and the module's shots frame from that facing.
///
/// `yaw` is the capture's yaw counter `ctx[+0x6DA]`. A party attacker's
/// first swing-clip commit re-seeds it to `(rand() % 2) * 0x800 + 0x280`
/// (`FUN_8004E13C`, `0x8004E288..0x8004E2B4`) - a coin on a stream the replay
/// does not share, and the half-turn it picks is the side cases 6, 7 and 8
/// film from. [`BattleDrive::steer`] keeps the engine's counter on retail's
/// half while the seat's action runs - the twin of the style alignment.
///
/// `message` is the timed message (HUD element `0x66`) the capture holds -
/// its battle-overlay string pointer and remaining hold - and `plate_cleared`
/// the counter swap's cleared target plaque. Both are the HUD's half of a
/// counterattack the replay's own monster turn does not roll (the drive
/// reaches the counterer's strike loop through its own turn), raised on the
/// engine when it holds the capture's state.
///
/// `arts` marks a party capture whose committed queue holds an art starter
/// ([`RetailBattle::arts_queue`]): the player entered that turn through
/// `Command`, so the drive opens the arts entry and confirms the string it
/// preseeds from the record (`FUN_801DA34C`) instead of taking `Auto`, which
/// builds a different queue under the same state byte. `gauge` is that
/// seat's live gauge over its base ([`RetailBattle::acting_gauge`]), set when
/// a Spirit turn the replay does not play had extended it: the extension is
/// what selects the saved string's band and pays for its arrows, so
/// [`BattleDrive::prime`] restores it.
///
/// `clip` is the acting party seat's committed clip `+0x1D9` on an
/// [`SpanGate::Age`] capture, when that is a swing clip. The age is frames
/// since the seat's **last** clip commit, and the strike loop `0x1E` spans
/// one commit per queued swing, so an age alone matches the first swing
/// that runs as long; the clip names which one retail sits in
/// (`rim_elm_gimard_victory`: `0x1E` at `16` on `0x0E`, the second swing -
/// the age alone took the first, `0x0F`). Art clips are compared on the
/// dynamic slot (`0x10` / `0x11`) the anim commit remaps them onto, which
/// both sides store. An idle `0` is not gated: a party seat's idle between
/// the approach and its first strike has no engine twin (the engine holds
/// the walk clip there).
///
/// `aim` is the monster row a party attack capture's target byte `+0x1DD`
/// names: the player picked that monster, so the drive walks the target
/// cursor onto it rather than confirming the picker's default. Which body
/// the swing lands on moves the framing (the strike shots look at the
/// target), and a default pick lands on whichever monster the earlier turns
/// left first in the ring (`battle_vahn_tri_somersault_super`: retail row
/// `1`, the default row `0`, which Noa's and Gala's swings had knocked far
/// back).
///
/// `queue` is an arts capture's committed queue `+0x1DF..`. The string a
/// bare confirm replays is the record's saved band, and that band is not
/// what the player entered on the captured turn: `battle_melee_hit_spark`'s
/// Vahn holds `0F 0E 0F 0E` (Up Down Up Down) in band A while his committed
/// queue `0D 0F 0E 19 27` is Right Up Down Up, tokenized with Somersault -
/// so the replay struck Up, Down, Somersault, Down and sat in `0x20` on a
/// Down swing where retail plays the Somersault. [`BattleDrive::prime`]
/// recovers the entered arrows from the queue ([`entered_arrows`]) and,
/// when neither saved band holds them, writes them into both, so the replay
/// tokenizes the captured turn.
///
/// `cursor` is that queue's strike cursor `ctx[+0x15]` on an
/// [`SpanGate::Age`] capture inside the strike loop. With the queue
/// replayed byte for byte, the age alone matches the first clip of the turn
/// that runs as long - `battle_vahn_tri_somersault_super` sits at age `160`
/// on the Somersault at cursor `5`, and the drive took the turn's first Down
/// swing at cursor `3` - so the cursor names the clip the art slot's `clip`
/// gate cannot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ActionSteer {
    pub target: Option<u8>,
    pub yaw: Option<u16>,
    /// The track coin `ctx[+0x26D]` the capture holds - a `rand() % 2`
    /// draw like the yaw counter's half-turn, aligned the same way
    /// (`BattleCamera::align_phase_cursor`).
    pub coin: Option<u8>,
    pub message: Option<(u32, i16)>,
    pub plate_cleared: bool,
    pub arts: bool,
    pub gauge: Option<u16>,
    pub clip: Option<u8>,
    pub aim: Option<u8>,
    pub queue: Option<[u8; 16]>,
    pub cursor: Option<u8>,
    /// Retail pool seat of a victim the capture shows in its defeat fade, and
    /// the fade lane it reads: an [`SpanGate::Age`] phase is met only once
    /// the engine's victim has faded at least as far.
    pub fading: Option<(u8, u16)>,
    /// The capture shows the death-spoils caption (HUD element `0x5B`): the
    /// phase is met only on a stream whose kill rolled the steal too.
    pub spoils: bool,
}

/// Where inside a state that spans many frames a capture sits.
///
/// **The Done band's continuation `0x52`** holds for its own countdown
/// `ctx[+0x6D8]` (`0xB4` frames when a Seru absorb stages it), and the engine
/// enters it at the top: the capture's countdown places it, the drive holding
/// until the engine's has run down to it. **The fade-down `0x51`** ticks the
/// same word - the `0x3C` tail timer `0x50` seeds - and is placed by it too,
/// not by the close-up accumulator: in the Done band that word counts frames
/// since the acting actor's last clip commit, which there is whichever idle or
/// return commit the actor's clip lengths put last, so the engine's
/// accumulator in `0x51` may not have been reset since its cast clip
/// (`zora_glare_petrify_post`: retail `24`, engine past `4000`) and the
/// accumulator gate held the band's first tick.
///
/// **The battle-end sequence**, for a capture taken after the `0x5A` gate
/// raised the end signal (`DAT_8007BD71 == 0xFE`):
/// Retail stops stepping the action SM on the signal and runs the results
/// sequencer `FUN_8004E568` instead, so `ctx[+0x07]` stays `0x5A` (and
/// `ctx[+0x13]` on the pose actor) through the whole load hold, results
/// hold and exit fade. The action-SM state then names a span of several
/// hundred vsyncs; the sequencer's own words place the capture in it
/// ([`legaia_engine_core::world::VictoryPhase`] is the engine's twin):
/// the phase word `_DAT_8007BD2C`, the phase halfword `ctx[+0x6CE]` and the
/// results hold `gp+0xA54` (`0x8007BD6C`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpanGate {
    /// No end signal: the action SM's own state places the capture.
    #[default]
    None,
    /// Phases `0..=4`, the CD loads (`ctx[+0x6CE] == 0`).
    Loading,
    /// Phase 5 with `ctx[+0x6CE] == 1`: the results frame onward, `hold`
    /// vsyncs into the hold.
    Results { hold: u16 },
    /// `ctx[+0x6CE] >= 2`: the exit fade, at that phase halfword.
    Exit { phase: u16 },
    /// Action-SM state `0x51` or `0x52`, its countdown `ctx[+0x6D8]` down to
    /// `timer`.
    DoneHold { timer: i16 },
    /// The capture band's CD holds `0x6E` / `0x6F` (or the module tick
    /// `0x70` they lead to), with retail's framing depth `ctx[+0x6D0]` at
    /// `height` and its close-up accumulator `ctx[+0x87C]` at `accum`.
    ///
    /// Both holds wait on the disc, not on a counter: `0x6E` on the CD-ready
    /// poll `FUN_8003DE7C(1)` (`0x801E4F08`), `0x6F` on `FUN_8003F2B8(1)`
    /// (`0x801E5024`). Every frame of either first calls
    /// `FUN_801D5854(ctx[+0x13], 6)`, whose prologue adds
    /// `8 * frame_step` to `ctx[+0x87C]` (`0x801D5900..0x801D5920`), and
    /// `0x6F` also ramps `ctx[+0x6D0]` down by `16 * frame_step`
    /// (`0x801E4FFC..0x801E5014`); `0x70` does not write the depth, and the
    /// `0x71` store re-seeds it. So the depth counts `0x6F`'s frames and
    /// the accumulator, less half the depth's ramp, counts `0x6E`'s since
    /// the caster's last clip commit zeroed it - clocks of how long
    /// retail's reads took, a disc-timing property the engine, whose polls
    /// are always ready, does not have. Through them the case-6 glide onto
    /// the caster has landed in every such capture. The drive holds the
    /// engine's polls busy ([`BattleDrive::steer`]) until its own words
    /// have run as far.
    ///
    /// `arm` is `(ctx[+0x279], countdown)` for a `0x70` capture of a module
    /// whose countdown the engine directs
    /// ([`legaia_engine_vm::cast_module_camera::capture_countdown_va`]): the
    /// phase is held until the engine's module sits in that arm with its
    /// countdown run down at least as far - the module's own clock, which
    /// places a camera shot or drift in flight.
    ///
    /// `yaw` is retail's yaw counter `ctx[+0x6DA]` (`None` under the Far
    /// option, which stops it): the action seed `0x0C` stores `0x800` and
    /// the SM's prologue adds about one a display frame, so it counts the
    /// frames since the seed - including the ones `0x6E` spent waiting on
    /// the drive (`FUN_8003DE7C(1)` at `0x801E4F08`) before the caster's
    /// clip commit zeroed the accumulator. The accumulator alone cannot see
    /// those; the three Delilas special captures sit `34` frames past the
    /// accumulator's reading of them.
    CaptureFade {
        height: u16,
        accum: u16,
        arm: Option<(u8, i32)>,
        yaw: Option<u16>,
    },
    /// Any other action-SM state, `accum` (`ctx[+0x87C]`) into it.
    ///
    /// The action SM's states span frames, and the drive reaches each on
    /// its first tick, while a retail capture sits wherever the save was
    /// made. The close-up accumulator places it: the acting actor's clip
    /// commit zeroes it (`FUN_8004AD80`) and every framing call adds
    /// `8 * frame_step` (`FUN_801D5854`, `0x801D5900..0x801D5920`), so it is
    /// the frames since the actor's last clip commit. The drive takes the
    /// first tick in the state whose engine accumulator has run as far; a
    /// state the engine leaves sooner is re-run to its last tick
    /// ([`EngineBattle::age_short`]).
    Age { accum: u16 },
    /// A capture ahead of the seed pass ([`PRE_SEED_STATES`]) whose camera
    /// has landed on its framing ([`camera_landed`]). `0x0A` waits on the CD
    /// (`FUN_8003F2B8(1)` at `0x801E2B78`) for as long as the next actor's
    /// data takes, which the engine's always-ready poll does not; the
    /// `evil_medallion_rage_battle` capture sits there with the commit
    /// confirm's case-9 glide finished. The drive holds the engine's wait
    /// until its own glide lands ([`BattleDrive::steer`]).
    Landed,
}

/// The capture band `0x6E..=0x71`. `0x70` pins the style to `1`
/// (`sb v0,0xd(v1)` at `0x801E50CC`) without re-arming a framing, so a
/// capture there reads `1` over a camera the rolled style placed; the style
/// is not aligned in the band.
pub(super) const CAPTURE_BAND: std::ops::RangeInclusive<u8> = 0x6E..=0x71;

/// The capture band's states placed by its CD holds
/// ([`SpanGate::CaptureFade`]).
pub const CAPTURE_FADE_STATES: [u8; 3] = [0x6E, 0x6F, 0x70];

/// How far the engine's `0x6F` depth ramp still has to come down to reach
/// retail's `height` (`ctx[+0x6D0]`, an unsigned halfword the ramp wraps),
/// `0` once it is there.
pub(super) fn capture_ramp_left(world: &legaia_engine_core::world::World, height: u16) -> u16 {
    ((world.battle.camera_frame_height as u16).wrapping_sub(height) as i16).max(0) as u16
}

/// How old the active actor's clip is, in the close-up accumulator's units
/// ([`BattleCamera::clip_age`](legaia_engine_vm::battle_cam_script::BattleCamera::clip_age)):
/// the engine's `ctx[+0x87C]` with the natural-end re-commits of a looping
/// clip left out. Retail's
/// accumulator restarts on those (`player_steal_skeleton_banner` reads `176`
/// 22 vsyncs after Vahn's idle wrapped), so the age is an upper bound on it
/// whenever the engine plays retail's clip - the gates read it with `>=` -
/// and it keeps placing a phase whose engine clip loops where retail's did
/// not (a parked Gaza on idle against the engine's Gaza still walking).
pub(super) fn active_clip_age(world: &legaia_engine_core::world::World) -> u32 {
    world
        .battle
        .camera
        .as_ref()
        .map_or(u32::MAX, |c| c.clip_age())
}

/// Whether the engine's close-up accumulator has run as far through `0x6E`
/// as retail's `accum` (`ctx[+0x87C]`) says retail's did: the capture's
/// value less what the `0x6F` frames still to come will add (half the depth
/// ramp left, `8` against `16` per frame step).
pub(super) fn capture_accum_done(
    world: &legaia_engine_core::world::World,
    height: u16,
    accum: u16,
) -> bool {
    let want = accum.saturating_sub(capture_ramp_left(world, height) / 2);
    world
        .battle
        .camera
        .as_ref()
        .is_none_or(|c| c.close_up_accum() >= u32::from(want))
}

/// How far apart the engine's yaw counter `ctx[+0x6DA]` and a capture's may
/// stand and still be the same rung of the per-action ladder read at another
/// instant (`BattleDrive::steer`): under half the `0x80` between the nearest
/// rungs (`0x200` / `0x280`), about a second of drift.
const YAW_CLOCK_SLACK: i32 = 0x3F;

/// Whether the engine's yaw counter has run as far through `0x6E` as
/// retail's `yaw` (`ctx[+0x6DA]`) says retail's did: the capture's value
/// less the `0x6F` frames still to come (the depth ramps `16` a display
/// frame against the counter's one). `None` - the Far option - holds
/// nothing.
pub(super) fn capture_yaw_done(
    world: &legaia_engine_core::world::World,
    height: u16,
    yaw: Option<u16>,
) -> bool {
    let Some(yaw) = yaw else {
        return true;
    };
    let want = i32::from(yaw) - i32::from(capture_ramp_left(world, height) / 16);
    world
        .battle
        .camera
        .as_ref()
        .is_none_or(|c| c.action_yaw_base() >= want)
}

/// Whether the engine's `0x6F` pull-in has come down at least as far as
/// retail's `height` (`ctx[+0x6D0]`, an unsigned halfword the ramp wraps).
pub(super) fn capture_ramp_done(world: &legaia_engine_core::world::World, height: u16) -> bool {
    capture_ramp_left(world, height) == 0
}

impl SpanGate {
    pub(super) fn to_env(self) -> (u8, u16) {
        match self {
            Self::None => (0, 0),
            Self::Loading => (1, 0),
            Self::Results { hold } => (2, hold),
            Self::Exit { phase } => (3, phase),
            Self::DoneHold { timer } => (4, timer as u16),
            Self::CaptureFade { .. } => (5, 0),
            Self::Age { accum } => (6, accum),
            Self::Landed => (7, 0),
        }
    }

    /// The env value field: one number, or `height/accum` for a capture
    /// hold.
    pub(super) fn env_value(self) -> String {
        match self {
            Self::CaptureFade {
                height,
                accum,
                arm,
                yaw,
            } => {
                let mut s = format!("{height}/{accum}");
                if let Some((phase, countdown)) = arm {
                    s.push_str(&format!("/{phase}/{countdown}"));
                }
                if let Some(y) = yaw {
                    s.push_str(&format!("/y{y}"));
                }
                s
            }
            _ => self.to_env().1.to_string(),
        }
    }

    pub(super) fn from_env(kind: &str, value: &str) -> Option<Self> {
        if kind.trim() == "5" {
            let mut parts: Vec<&str> = value.trim().split('/').collect();
            let yaw = match parts.last() {
                Some(t) if t.starts_with('y') => {
                    let y = t[1..].parse().ok()?;
                    parts.pop();
                    Some(y)
                }
                _ => None,
            };
            let arm = match parts.as_slice() {
                [_, _] => None,
                [_, _, p, c] => Some((p.parse().ok()?, c.parse().ok()?)),
                _ => return None,
            };
            return Some(Self::CaptureFade {
                height: parts[0].parse().ok()?,
                accum: parts[1].parse().ok()?,
                arm,
                yaw,
            });
        }
        let value: u16 = value.trim().parse().ok()?;
        Some(match kind.trim() {
            "0" => Self::None,
            "1" => Self::Loading,
            "2" => Self::Results { hold: value },
            "3" => Self::Exit { phase: value },
            "4" => Self::DoneHold {
                timer: value as i16,
            },
            "6" => Self::Age { accum: value },
            "7" => Self::Landed,
            _ => return None,
        })
    }

    /// Whether a gate past the end signal.
    pub fn is_end(self) -> bool {
        matches!(
            self,
            Self::Loading | Self::Results { .. } | Self::Exit { .. }
        )
    }

    /// Whether the engine's battle-end sequence holds this gate.
    pub(super) fn met(self, world: &legaia_engine_core::world::World) -> bool {
        use legaia_engine_core::world::VictoryPhase;
        let Some(seq) = world.battle.victory else {
            return false;
        };
        match (self, seq.phase) {
            (Self::Loading, VictoryPhase::Loading { .. }) => true,
            (Self::Results { hold }, VictoryPhase::Results { hold: h }) => h == hold,
            (Self::Exit { phase }, VictoryPhase::Exit { phase: p }) => p == phase,
            _ => false,
        }
    }
}

impl BattleDrive {
    /// `menu,<flow>,<seat>` or
    /// `action,<seat>,<state>,<category>,<queued>[,<spare>,<absorbed>[,<end kind>,<end value>]]`.
    pub fn to_env(&self) -> String {
        match *self {
            Self::Opening { swept, entry } => format!("opening,{},{entry}", u8::from(swept)),
            Self::Menu { flow, seat } => format!("menu,{},{seat}", flow.raw()),
            Self::Action {
                seat,
                state,
                category,
                queued,
                spare,
                absorbed,
                end,
                style,
                steer,
            } => {
                let (kind, _) = end.to_env();
                let value = end.env_value();
                let mut s = format!(
                    "action,{seat},{state},{category},{queued},{},{absorbed},{kind},{value}",
                    u8::from(spare)
                );
                if let Some(style) = style {
                    s.push_str(&format!(",{style}"));
                }
                if let Some(t) = steer.target {
                    s.push_str(&format!(",t{t}"));
                }
                if let Some(y) = steer.yaw {
                    s.push_str(&format!(",y{y}"));
                }
                if let Some(o) = steer.coin {
                    s.push_str(&format!(",o{o}"));
                }
                if let Some((va, hold)) = steer.message {
                    s.push_str(&format!(",m{va:x}/{hold}"));
                }
                if steer.plate_cleared {
                    s.push_str(",c");
                }
                if let Some(g) = steer.gauge {
                    s.push_str(&format!(",g{g}"));
                }
                if steer.arts {
                    s.push_str(",a");
                }
                if let Some(k) = steer.clip {
                    s.push_str(&format!(",k{k}"));
                }
                if let Some(p) = steer.aim {
                    s.push_str(&format!(",p{p}"));
                }
                if let Some(u) = steer.cursor {
                    s.push_str(&format!(",u{u}"));
                }
                if let Some((f, lane)) = steer.fading {
                    s.push_str(&format!(",f{f}/{lane}"));
                }
                if steer.spoils {
                    s.push_str(",s");
                }
                if let Some(q) = steer.queue {
                    s.push_str(",q");
                    for b in q {
                        s.push_str(&format!("{b:02x}"));
                    }
                }
                s
            }
        }
    }

    pub fn from_env(s: &str) -> Option<Self> {
        match s.trim() {
            "opening" | "opening,0" => {
                return Some(Self::Opening {
                    swept: false,
                    entry: 0,
                });
            }
            "opening,1" => {
                return Some(Self::Opening {
                    swept: true,
                    entry: 0,
                });
            }
            t if t.starts_with("opening,") => {
                let mut it = t.split(',').skip(1);
                let swept = it.next()? == "1";
                let entry = it.next()?.parse().ok()?;
                return Some(Self::Opening { swept, entry });
            }
            _ => {}
        }
        // The tagged steering tokens ride at the end (`t<seat>`, `y<yaw>`).
        let mut steer = ActionSteer::default();
        let mut fields: Vec<&str> = s.split(',').collect();
        while fields.len() > 1 {
            let last = fields[fields.len() - 1].trim();
            if let Some(t) = last.strip_prefix('t') {
                steer.target = Some(t.parse().ok()?);
            } else if let Some(y) = last.strip_prefix('y') {
                steer.yaw = Some(y.parse().ok()?);
            } else if let Some(o) = last.strip_prefix('o') {
                steer.coin = Some(o.parse().ok()?);
            } else if let Some(m) = last.strip_prefix('m') {
                let (va, hold) = m.split_once('/')?;
                steer.message = Some((u32::from_str_radix(va, 16).ok()?, hold.parse().ok()?));
            } else if last == "c" {
                steer.plate_cleared = true;
            } else if last == "a" {
                steer.arts = true;
            } else if last == "s" {
                steer.spoils = true;
            } else if let Some(g) = last.strip_prefix('g') {
                steer.gauge = Some(g.parse().ok()?);
            } else if let Some(k) = last.strip_prefix('k') {
                steer.clip = Some(k.parse().ok()?);
            } else if let Some(p) = last.strip_prefix('p') {
                steer.aim = Some(p.parse().ok()?);
            } else if let Some(u) = last.strip_prefix('u') {
                steer.cursor = Some(u.parse().ok()?);
            } else if let Some(f) = last.strip_prefix('f') {
                let (seat, lane) = f.split_once('/')?;
                steer.fading = Some((seat.parse().ok()?, lane.parse().ok()?));
            } else if let Some(q) = last.strip_prefix('q') {
                if q.len() != 32 {
                    return None;
                }
                let mut queue = [0u8; 16];
                for (i, b) in queue.iter_mut().enumerate() {
                    *b = u8::from_str_radix(q.get(i * 2..i * 2 + 2)?, 16).ok()?;
                }
                steer.queue = Some(queue);
            } else {
                break;
            }
            fields.pop();
        }
        if steer != ActionSteer::default() {
            let mut d = Self::from_env(&fields.join(","))?;
            if let Self::Action { steer: st, .. } = &mut d {
                *st = steer;
            }
            return Some(d);
        }
        if fields.len() == 10 && fields[0].trim() == "action" {
            let mut d = Self::from_env(&fields[..9].join(","))?;
            if let Self::Action { style, .. } = &mut d {
                *style = Some(fields[9].trim().parse().ok()?);
            }
            return Some(d);
        }
        if fields.len() == 9 && fields[0].trim() == "action" {
            let mut d = Self::from_env(&fields[..7].join(","))?;
            if let Self::Action { end, .. } = &mut d {
                *end = SpanGate::from_env(fields[7], fields[8])?;
            }
            return Some(d);
        }
        let parts: Vec<u8> = s
            .split(',')
            .skip(1)
            .map(|p| p.trim().parse().ok())
            .collect::<Option<_>>()?;
        match (s.split(',').next()?.trim(), parts.as_slice()) {
            ("menu", &[flow, seat]) => Some(Self::Menu {
                flow: BattleFlowState::from_raw(flow),
                seat,
            }),
            ("action", &[seat, state, category, queued]) => Some(Self::Action {
                seat,
                state,
                category,
                queued,
                spare: false,
                absorbed: 0,
                end: SpanGate::None,
                style: None,
                steer: ActionSteer::default(),
            }),
            ("action", &[seat, state, category, queued, spare, absorbed]) => Some(Self::Action {
                seat,
                state,
                category,
                queued,
                spare: spare != 0,
                absorbed,
                end: SpanGate::None,
                style: None,
                steer: ActionSteer::default(),
            }),
            _ => None,
        }
    }

    /// An action drive's steering facts.
    pub(super) fn steering(&self) -> Option<ActionSteer> {
        match *self {
            Self::Action { steer, .. } => Some(steer),
            _ => None,
        }
    }

    /// The tick budget the drive gets past the first prompt.
    pub fn budget(&self) -> u32 {
        match self {
            Self::Opening { .. } => 0,
            Self::Menu { .. } => MENU_DRIVE_TICKS,
            Self::Action { .. } => ACTION_DRIVE_TICKS,
        }
    }

    /// Ticks the reached phase is held before it is sampled
    /// ([`MENU_HOLD_TICKS`]); an action phase moves on by itself, so it is
    /// sampled the tick it is reached.
    pub fn hold_ticks(&self) -> u32 {
        match self {
            Self::Menu { .. } => MENU_HOLD_TICKS,
            Self::Action { .. } | Self::Opening { .. } => 0,
        }
    }

    /// Arm the drive's one-shot world seed (a monster cast to replay, an
    /// absorbed Seru to take back off the seat's spell list). Called once,
    /// on the first battle tick.
    pub fn prime(&self, world: &mut legaia_engine_core::world::World) {
        if let Self::Action { seat, steer, .. } = *self
            && let Some(live) = steer.gauge
            && seat < 3
        {
            let pc = world.party.party_count.clamp(1, 3);
            if let Some(a) = world.actors.get_mut(usize::from(engine_seat(seat, pc))) {
                a.battle.agl = live;
            }
        }
        if let Self::Action { seat, steer, .. } = *self
            && seat < 3
            && let Some(queue) = steer.queue
        {
            seed_entered_arrows(world, seat, &queue);
        }
        if let Self::Action { seat, absorbed, .. } = *self
            && absorbed != 0
            && seat < 3
        {
            let roster = world.party_roster_slot(usize::from(seat));
            if let Some(rec) = world.party.roster.members.get_mut(roster) {
                unlearn_spell(rec, absorbed.wrapping_add(0x80));
            }
        }
        if let Self::Action {
            seat,
            category: 2,
            queued,
            ..
        } = *self
        {
            let pc = world.party.party_count.clamp(1, 3);
            let seat = engine_seat(seat, pc);
            if seat >= pc {
                world.battle.forced_monster_cast = Some((seat, queued));
                // A party seat keeps its number; the caster's own retail
                // seat is re-keyed to its engine seat.
                world.battle.forced_monster_target = self
                    .steering()
                    .and_then(|s| s.target)
                    .and_then(|t| match t {
                        0..3 => Some(t),
                        t if engine_seat(t, pc) == seat => Some(seat),
                        _ => None,
                    });
            }
        }
    }

    /// Whether the engine holds the capture's phase.
    pub fn reached(&self, world: &legaia_engine_core::world::World) -> bool {
        if world.mode != SceneMode::Battle {
            return false;
        }
        let pc = world.party.party_count.clamp(1, 3);
        match *self {
            // The first frame whose monsters are drawable: the bodies bind
            // a few ticks after the mode flip.
            Self::Opening { swept, entry } => {
                // A sparring capture past the intro timer also holds the
                // side-band's caption, which the engine raises on the round
                // start the open was holding back
                // (`World::sparring_open_held`).
                let ok = (!swept
                    || world.battle.intro_names_frames == 0
                        && !world.battle.sparring_round_pending)
                    && entry_sweep_reached(world, entry)
                    && world.actors.iter().enumerate().all(|(i, a)| {
                        a.battle_monster_id.is_none()
                            || !a.active
                            || a.tmd_binding.is_some()
                                && world
                                    .battle_actor_draw_plan(i, None, 4.0, false)
                                    .is_none_or(|p| p.drawn)
                    });
                if std::env::var_os("LEGAIA_RC_DRIVE_TRACE").is_some() {
                    for (i, a) in world.actors.iter().enumerate().take(8) {
                        eprintln!(
                            "[op] {i} mon={:?} act={} bind={:?} rf={} rc={:#x} plan={:?} anim={} pose={} cam={:?} pos={:?}",
                            a.battle_monster_id,
                            a.active,
                            a.tmd_binding,
                            a.battle.render_flag,
                            a.battle.render_color,
                            world
                                .battle_actor_draw_plan(i, None, 4.0, false)
                                .map(|p| p.drawn),
                            a.battle_animation.is_some(),
                            a.battle_pose.is_some(),
                            world
                                .battle
                                .camera
                                .as_ref()
                                .map(|_| world.battle_cam_pose().tr),
                            (
                                a.move_state.world_x,
                                a.move_state.world_y,
                                a.move_state.world_z
                            )
                        );
                    }
                }
                ok
            }
            // A menu capture is a surface the player sat on: the camera has
            // arrived at its framing, so the frame waits out the glide.
            Self::Menu { flow, seat } => {
                world.battle.flow == flow
                    && (!menu_seat_matters(flow) || world.battle_ctx.active_actor == seat)
                    && !world.battle.camera.as_ref().is_some_and(|c| c.is_gliding())
            }
            Self::Action { seat, end, .. } if end.is_end() => {
                // The sequencer frames its pose actor `ctx[+0x13]`.
                end.met(world)
                    && world
                        .battle
                        .victory
                        .is_some_and(|v| v.pose_actor == usize::from(engine_seat(seat, pc)))
            }
            Self::Action {
                seat,
                state,
                end,
                steer,
                ..
            } => {
                world.battle.flow == BattleFlowState::Idle
                    && world.battle.command.is_none()
                    && world.battle_ctx.active_actor == engine_seat(seat, pc)
                    && world.battle_ctx.action_state == state
                    && steer.target.is_none_or(|t| {
                        world
                            .actors
                            .get(usize::from(engine_seat(seat, pc)))
                            .is_some_and(|a| a.battle.active_target == engine_seat(t, pc))
                    })
                    && match end {
                        SpanGate::DoneHold { timer } => world.battle_ctx.frame_timer <= timer,
                        SpanGate::CaptureFade {
                            arm: Some((phase, countdown)),
                            ..
                        } if state == 0x70 => {
                            world.casting.module_phase > phase
                                || world.casting.module_phase == phase
                                    && world.casting.module_cam.countdown.0 <= countdown
                        }
                        SpanGate::CaptureFade {
                            height, accum, yaw, ..
                        } if state == 0x6E => {
                            capture_accum_done(world, height, accum)
                                && capture_yaw_done(world, height, yaw)
                        }
                        SpanGate::CaptureFade { height, .. } if state == 0x6F => {
                            capture_ramp_done(world, height)
                        }
                        SpanGate::Landed => {
                            !world.battle.camera.as_ref().is_some_and(|c| c.is_gliding())
                        }
                        SpanGate::Age { accum } => {
                            active_clip_age(world) >= u32::from(accum)
                                && steer.clip.is_none_or(|k| {
                                    world.battle_current_anim(usize::from(engine_seat(seat, pc)))
                                        == k
                                })
                                && steer.cursor.is_none_or(|u| {
                                    world
                                        .actors
                                        .get(usize::from(engine_seat(seat, pc)))
                                        .is_some_and(|a| a.battle.strike_index == u)
                                })
                                && (!steer.spoils
                                    || world.battle_ctx.message_id
                                        == legaia_engine_core::battle_steal::STEAL_CAPTION_ELEMENT)
                                && steer.fading.is_none_or(|(v, lane)| {
                                    world
                                        .actors
                                        .get(usize::from(engine_seat(v, pc)))
                                        .is_some_and(|a| {
                                            a.battle.render_flag
                                            == legaia_engine_vm::battle_formulas::STATE_DEFEAT_FADE
                                            && (a.battle.render_color & 0x3FF) as u16 <= lane
                                        })
                                })
                        }
                        _ => true,
                    }
            }
        }
    }

    /// Whether the engine sits in an [`SpanGate::Age`] action phase's state
    /// at all, whatever its age.
    pub(super) fn in_aged_state(&self, world: &legaia_engine_core::world::World) -> bool {
        let mut base = *self;
        match &mut base {
            Self::Action { end, .. } if matches!(end, SpanGate::Age { .. }) => {
                *end = SpanGate::None;
                base.reached(world)
            }
            _ => false,
        }
    }

    /// Per-tick world steering the drive owns besides the pad: a
    /// [`SpanGate::CaptureFade`] capture holds the engine's capture-band CD
    /// polls busy while its acting seat sits in `0x6E` with the close-up
    /// accumulator short of retail's, or in `0x6F` with the pull-in not yet
    /// down to retail's depth, and releases them otherwise.
    pub fn steer(&self, world: &mut legaia_engine_core::world::World) {
        let Self::Action {
            seat,
            state: want,
            end,
            style,
            steer,
            ..
        } = *self
        else {
            return;
        };
        // The yaw counter's half-turn coin ([`ActionSteer::yaw`]): from the
        // seat's seed pass to the capture's state.
        {
            let pc = world.party.party_count.clamp(1, 3);
            let state = world.battle_ctx.action_state;
            if let Some(yaw) = steer.yaw
                && world.mode == SceneMode::Battle
                && world.battle_ctx.active_actor == engine_seat(seat, pc)
                && (0x0C..=want).contains(&state)
                && let Some(cam) = world.battle.camera.as_mut()
            {
                cam.align_action_yaw_half(i32::from(yaw));
                // The counter's drift is a clock from the seat's seed pass
                // (`max(1, 4 * frame_step / 3)` an SM pass, `0x801E29E4`),
                // so how far it has run by the capture is the frame-step
                // and CD-wait history of retail's passes. In the capture's
                // own state, a counter within the clock's slack of retail's
                // reads retail's; a wrong rung of the ladder (they stand
                // `0x80` apart at the nearest) stays the engine's. The
                // capture band gates on the counter instead.
                if state == want && !matches!(end, SpanGate::CaptureFade { .. }) {
                    let d = (cam.action_yaw_base() - i32::from(yaw)).rem_euclid(0x1000);
                    if d.min(0x1000 - d) <= YAW_CLOCK_SLACK {
                        cam.set_action_yaw_base(i32::from(yaw));
                    }
                }
                if let Some(coin) = steer.coin {
                    cam.align_phase_cursor(coin);
                }
            }
        }
        let pc = world.party.party_count.clamp(1, 3);
        let ours = world.mode == SceneMode::Battle
            && world.battle_ctx.active_actor == engine_seat(seat, pc);
        let state = world.battle_ctx.action_state;
        // The capture's counterattack HUD, on the frame the engine holds its
        // state ([`ActionSteer::message`] / [`ActionSteer::plate_cleared`]).
        if ours && state == want {
            if let Some((va, hold)) = steer.message
                && world.battle.message_banner.is_none()
            {
                world.raise_timed_message(va, i32::from(hold));
            }
            if steer.plate_cleared {
                world.battle.target_plate_cleared = true;
                // The swap's HUD: the monster seed's bar for the counterer,
                // and no combo cluster.
                world.battle.counter_hud = Some(engine_seat(seat, pc));
            }
        }
        if let Some(style) = style
            && ours
            && (state == want || seat < 3 && (0x0C..=want).contains(&state))
            && !CAPTURE_BAND.contains(&state)
        {
            world.battle_ctx.camera_variant = style;
        }
        // The stamp above lands on the tick after it is made, and the seed
        // pass `0x0C` rolls its own variant on the very tick that hands the
        // action to the state that arms the framing - so a capture one or
        // two passes into its action would be filmed on the engine's coin.
        // Pin it through the seed instead
        // (`BattleActionCtx::camera_variant_pin`), for the driven seat only.
        world.battle_ctx.camera_variant_pin = style.filter(|_| {
            ours && seat < 3
                && (PRE_SEED_STATES.contains(&state) || state == 0x0C)
                && !CAPTURE_BAND.contains(&want)
        });
        if end == SpanGate::Landed {
            let gliding = world.battle.camera.as_ref().is_some_and(|c| c.is_gliding());
            world.battle.prev_action_cleared = !(ours && state == want && state == 0x0A && gliding);
            return;
        }
        let SpanGate::CaptureFade {
            height, accum, yaw, ..
        } = end
        else {
            return;
        };
        world.audio.sound_bank_ready = !(ours
            && state == 0x6E
            && !(capture_accum_done(world, height, accum) && capture_yaw_done(world, height, yaw)));
        world.battle.prev_action_cleared =
            !(ours && state == 0x6F && !capture_ramp_done(world, height));
    }

    /// The press that walks the engine one step toward the phase, or `None`
    /// when nothing on screen wants one (the action SM owns the frame).
    pub fn press(
        &self,
        world: &legaia_engine_core::world::World,
    ) -> Option<legaia_engine_core::input::PadButton> {
        use legaia_engine_core::battle_input::CommandPhase;
        use legaia_engine_core::input::PadButton;
        if world.mode != SceneMode::Battle || matches!(self, Self::Opening { .. }) {
            return None;
        }
        // A battle message box parks the whole battle until it is dismissed.
        if !world.battle.tutorial_boxes.is_empty() {
            return Some(PadButton::Cross);
        }
        // The arts entry the capture seat opened to replay its saved
        // command string: a bare confirm takes the string, and the
        // confirms after it begin the turn and pick the target.
        if let Self::Action { seat, steer, .. } = *self
            && let Some(want) = steer.aim
        {
            let picker = world
                .battle
                .arts_input
                .as_ref()
                .filter(|a| a.party_slot == seat)
                .and_then(|a| a.picker())
                .or_else(|| {
                    world
                        .battle
                        .command
                        .as_ref()
                        .filter(|c| c.actor == seat)
                        .and_then(|c| c.picker())
                });
            if let Some(picker) = picker
                && let legaia_engine_core::target_picker::PickerState::Cursor {
                    row: legaia_engine_core::target_picker::CursorRow::Enemy,
                    slot,
                } = picker.state()
                && slot != want
                && aim_alive(world, want)
            {
                return Some(PadButton::Right);
            }
        }
        if matches!(self, Self::Action { steer, .. } if steer.arts)
            && world.battle.arts_input.is_some()
        {
            return Some(PadButton::Cross);
        }
        let cmd = world.battle.command.as_ref()?;
        match *self {
            Self::Opening { .. } => None,
            Self::Menu { flow, seat } => {
                let ours = cmd.actor == seat && flow != BattleFlowState::CommitBegin;
                match cmd.phase {
                    CommandPhase::RoundPrompt { .. } => Some(PadButton::Left),
                    CommandPhase::Menu { .. } if !ours => Some(PadButton::Left),
                    CommandPhase::Menu { .. } => match flow {
                        BattleFlowState::ItemWindow => Some(PadButton::Up),
                        BattleFlowState::MagicWindow => Some(PadButton::Right),
                        BattleFlowState::ArtsCommandEntry
                        | BattleFlowState::AttackModePrompt
                        | BattleFlowState::TargetSelect => Some(PadButton::Left),
                        _ => None,
                    },
                    CommandPhase::AttackMode { .. } if !ours => Some(PadButton::Left),
                    CommandPhase::AttackMode { .. } => match flow {
                        BattleFlowState::ArtsCommandEntry => Some(PadButton::Right),
                        BattleFlowState::TargetSelect => Some(PadButton::Left),
                        _ => None,
                    },
                    CommandPhase::Targeting { .. } if !ours => Some(PadButton::Cross),
                    _ => None,
                }
            }
            Self::Action {
                seat,
                category,
                spare,
                steer,
                ..
            } => Some(match cmd.phase {
                CommandPhase::Menu { .. } if cmd.actor == seat && category == 4 => PadButton::Down,
                CommandPhase::Menu { .. } if cmd.actor != seat && spare => PadButton::Down,
                // A capture mid monster cast: the party commits Spirit, so no
                // swing lands on the caster before its replayed turn comes up
                // (a strong party otherwise kills it first, and the cast the
                // seed names is never taken).
                CommandPhase::Menu { .. } if seat >= 3 && category == 2 => PadButton::Down,
                CommandPhase::AttackMode { .. }
                    if cmd.actor == seat && steer.arts && saved_command_string(world, seat) =>
                {
                    PadButton::Right
                }
                CommandPhase::RoundPrompt { .. }
                | CommandPhase::Menu { .. }
                | CommandPhase::AttackMode { .. } => PadButton::Left,
                _ => PadButton::Cross,
            }),
        }
    }

    /// This tick's pad word: the step's press on even ticks, released on odd
    /// ones, so every press is an edge.
    pub fn pad_word_at(&self, world: &legaia_engine_core::world::World, tick: u64) -> u16 {
        if !tick.is_multiple_of(2) {
            return 0;
        }
        self.press(world).map_or(0, |b| b.mask())
    }
}

/// Whether party seat `seat`'s character record carries a saved auto
/// command string - the arrows the arts entry preseeds its window from
/// (`FUN_801DA34C`) and a bare confirm replays.
///
/// A party capture's queue `+0x1DF` is the turn the player committed, and a
/// record that holds a string is a player who entered (and so saved) one: a
/// replay through `Command` rebuilds that queue, where `Auto` rebuilds the
/// queue from the direction commands and learned arts and plays a different
/// action under the same state byte.
pub(super) fn saved_command_string(world: &legaia_engine_core::world::World, seat: u8) -> bool {
    use legaia_save::character::AutoCommandBand;
    let slot = world.party_roster_slot(usize::from(seat));
    world.party.roster.members.get(slot).is_some_and(|rec| {
        rec.auto_command_string(AutoCommandBand::Primary)[0] != 0
            || rec.auto_command_string(AutoCommandBand::Secondary)[0] != 0
    })
}

/// The arrows a committed arts queue was entered as, as the swing bytes
/// `0x0C..=0x0F` the record's saved band holds.
///
/// The queue builder keeps a matched art's leading arrows as swings and
/// rewrites only its last one into the starter + constant pair (`0x19` /
/// `0x1A`, then the art), so each pair stands for its art's final arrow and
/// every other byte is an arrow already. `combo_of` names an art's arrows;
/// an art it does not know - a Super or Miracle replacement, whose tail the
/// finish rewrote whole - leaves the entry unrecoverable (`None`).
pub fn entered_arrows(
    queue: &[u8; 16],
    combo_of: impl Fn(u8) -> Option<Vec<legaia_art::Command>>,
) -> Option<[u8; 16]> {
    let mut out = [0u8; 16];
    let mut n = 0;
    let mut i = 0;
    while i < queue.len() && queue[i] != 0 {
        let arrow = match queue[i] {
            b @ 0x0C..=0x0F => b,
            0x19 | 0x1A => {
                i += 1;
                let last = *combo_of(*queue.get(i)?)?.last()?;
                0x0B + last as u8
            }
            _ => return None,
        };
        *out.get_mut(n)? = arrow;
        n += 1;
        i += 1;
    }
    (n > 0).then_some(out)
}

/// Make `seat`'s saved command string the arrows `queue` was entered as, so
/// the arts entry preseeds the captured turn.
///
/// Which band the preseed reads is the live gauge's choice at the arts
/// entry, which a replayed round can move after this runs (the seat's gauge
/// restore is the drive's, not the band's). A record that already holds the
/// arrows in either band is left alone - the capture's own preseed then
/// reads the string retail did - and otherwise both bands take them.
pub(super) fn seed_entered_arrows(
    world: &mut legaia_engine_core::world::World,
    seat: u8,
    queue: &[u8; 16],
) {
    use legaia_save::character::AutoCommandBand;
    let roster = world.party_roster_slot(usize::from(seat));
    let character = legaia_engine_core::battle_arts::character_for_slot(roster as u8);
    let Some(arrows) = entered_arrows(queue, |art| {
        let action = legaia_art::ActionConstant::from_byte(art)?;
        world
            .tables
            .art_records
            .get(&(character, action))
            .map(|r| r.commands.clone())
    }) else {
        return;
    };
    let Some(rec) = world.party.roster.members.get_mut(roster) else {
        return;
    };
    let bands = [AutoCommandBand::Primary, AutoCommandBand::Secondary];
    if bands.iter().any(|&b| rec.auto_command_string(b) == arrows) {
        return;
    }
    for band in bands {
        rec.set_auto_command_string(band, arrows);
    }
}
