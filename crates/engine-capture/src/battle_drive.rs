use legaia_engine_battle_menus::battle_flow::BattleFlowState;
use legaia_engine_mode::mode::SceneMode;

/// Ticks the pad path may take to reach a menu capture's surface.
pub(crate) const MENU_DRIVE_TICKS: u32 = 600;
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
pub(crate) const ACTION_DRIVE_TICKS: u32 = 4000;
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
    /// An opening capture (`SeedPlan::Opening`): nothing is pressed, and
    /// the frame is the first one in battle mode whose entry sweep has run
    /// as far as retail's - the instant the headless seed samples.
    ///
    /// `swept` is set for a capture whose flow byte is past the intro timer
    /// (`0x0C` / `0x14`): the enemy-name labels `0x0B` sweeps are gone there,
    /// so the frame waits for the engine's own intro names to clear
    /// (`World::battle.intro_names_frames`) and for the round start they were
    /// holding back, which in the sparring fight is what raises the side-band's
    /// caption a `0x14` capture shows.
    ///
    /// `entry` is retail's frame-driver counter `gp+0x330`: the frame also
    /// waits for the engine's battle-entry sweep to run as far
    /// ([`entry_sweep_reached`]).
    Opening { swept: bool, entry: u8 },
    /// A command-selection surface on party seat `seat`: members ahead of
    /// it commit a plain Attack, or Spirit for each seat set in `spirit`
    /// (the retail commit each member's `+0x1DE` holds, which the commit
    /// log draws), and the seat itself takes the arm that leads to `flow`.
    ///
    /// `pick` is the action id the window row under the cursor holds on a
    /// capture inside the item or magic window or one of their target
    /// sub-cursors - the seat's `+0x1DF`, which the item window stamps off
    /// the bag slot (`0x801D189C`) and the magic window off the hovered spell
    /// (`0x801D1A34`) - or `0` for the window's opening row. The drive walks
    /// the window's cursor down onto that row before it confirms, so the
    /// sub-cursor reached is the one the row's target flags raise. On the
    /// Attack's target cursor (`0x5A`) it is the retail seat (`3..8`) the
    /// cursor rests on (the seat's `+0x1DD`), which the drive steps the
    /// cursor onto, or `0` for the cursor's opening monster.
    Menu {
        flow: BattleFlowState,
        seat: u8,
        spirit: u8,
        pick: u8,
    },
    /// An action in flight: rounds are committed (a plain Attack each, the
    /// capture's own seat Spirit when that is what it had committed) until
    /// the action SM holds `state` on `seat`. A monster seat that was
    /// casting (`category == 2`) casts the capture's spell `queued` on its
    /// next turn ([`legaia_engine_battle_loop::battle_state::BattleState::forced_monster_cast`]);
    /// the party commits Spirit throughout, so the caster is still standing
    /// when that turn comes (Zeto's captures sit in a party able to kill him
    /// in two swings).
    ///
    /// `spare` is set on a killing-blow capture
    /// (`RetailBattle::action_victims`): the victim is replayed at `1` HP,
    /// so any member's swing would kill it, and the members other than the
    /// capture's seat commit Spirit instead of Attack - the kill is then the
    /// seat's own, whatever order initiative put them in. It is also set on
    /// a monster's Attack captured in a round the whole party sat out on
    /// Spirit (`RetailBattle::party_spirit_round`): the replay's party then
    /// commits Spirit too, and strikes down no monster the capture holds
    /// standing.
    ///
    /// `absorbed` is the Seru a capture on the Done band already staged
    /// (`ctx[+0x269]`, `RetailBattle::absorbed_seru`). Retail's grant ran
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
    /// holds the capture's state (outside the capture band, `CAPTURE_BAND`)
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
/// (`RetailBattle::action_victims`). The phase is then reached only with
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
/// (`RetailBattle::arts_queue`): the player entered that turn through
/// `Command`, so the drive opens the arts entry and confirms the string it
/// preseeds from the record (`FUN_801DA34C`) instead of taking `Auto`, which
/// builds a different queue under the same state byte. `gauge` is that
/// seat's live gauge over its base (`RetailBattle::acting_gauge`), set when
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
/// On a monster seat `queue` is the attacker's strike stream instead - its
/// swing picks as archive entry indices - on a capture gated on the first
/// swing's clip ([`Self::clip`]): [`BattleDrive::steer`] stands the stream
/// on it from the seed to the approach's end, so the gate does not hang on
/// which picks the replay's stream drew.
///
/// What the end-of-action gate `0x5A` armed its last framing from, on a capture
/// parked in `0x0A` behind an earlier actor of the round (retail seat
/// `ctx[+0x13]`, its category `+0x1DE`, target `+0x1DD`, style `ctx[+0xD]` and
/// frame depth `ctx[+0x6D0]`). The replay's Spirit rounds do not run that
/// actor last, and a spent monster does not run at all, so the drive stands
/// the camera on [`World::seat_prior_action_framing`] instead: the framing law
/// over these inputs and the capture's ground.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PriorFraming {
    pub seat: u8,
    pub category: u8,
    pub target: u8,
    pub style: u8,
    pub depth: u16,
}

impl PriorFraming {
    /// The actor's engine seat and its target's, on a fight seating `pc`.
    pub fn engine_seats(&self, pc: u8) -> (u8, u8) {
        let target = if self.target < 8 {
            engine_seat(self.target, pc)
        } else {
            self.target
        };
        (engine_seat(self.seat, pc), target)
    }
}

/// `cursor` is that queue's strike cursor `ctx[+0x15]` on an
/// [`SpanGate::Age`] capture inside the strike loop. With the queue
/// replayed byte for byte, the age alone matches the first clip of the turn
/// that runs as long - `battle_vahn_tri_somersault_super` sits at age `160`
/// on the Somersault at cursor `5`, and the drive took the turn's first Down
/// swing at cursor `3` - so the cursor names the clip the art slot's `clip`
/// gate cannot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ActionSteer {
    /// The actor whose turn the capture's framing still shows, on a capture
    /// ahead of the seed pass ([`SpanGate::Landed`]).
    pub prior: Option<PriorFraming>,
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
    /// The capture's queue under a monster strike's [`Self::cursor`]: the
    /// phase is met only on a stream whose picks up to the cursor are the
    /// capture's. A stream that rolled a shorter queue stops its cursor on
    /// the capture's value on a later swing of the same clip.
    pub played: Option<[u8; 16]>,
    /// The capture's approach terms `ctx[+0x6D2]` / `ctx[+0x6D4]`, which its
    /// first hit's block roll reads, and the striker's working ATK `+0x158`, a
    /// Power Charge the replay never cast included: carried beside
    /// [`Self::played`] for the cast-sequence oracle's strike deal, no gate
    /// reads them.
    pub ramps: Option<(i16, i16, u16)>,
    /// Retail pool seat of a victim the capture shows in its defeat fade, and
    /// the fade lane it reads: an [`SpanGate::Age`] phase is met only once
    /// the engine's victim has faded at least as far.
    pub fading: Option<(u8, u16)>,
    /// The capture shows the death-spoils caption (HUD element `0x5B`): the
    /// phase is met only on a stream whose kill rolled the steal too.
    pub spoils: bool,
    /// A results-frame capture's drop outcome: the item its report's drop
    /// row names, `0` for none (`RetailBattle::results_drop`), seeded into
    /// the replay's roll (`BattleState::forced_drop`).
    pub loot: Option<u8>,
    /// The capture is a monster parked in the approach poll `0x19` with its
    /// walk clip dead (`+0x1D9 == 0`; `docs/subsystems/battle-action-exit-gates.md`,
    /// the `0x19` park). The engine always walks an approach, so the replay
    /// holds that seat where it stands for the action
    /// (`BattleState::approach_hold`), and the yaw counter, which runs on
    /// through the park for as long as retail sat in it, takes the capture's
    /// word outright rather than only within the clock's slack.
    pub park: bool,
    /// The frame step every pass of the capture's frame-time history ran at
    /// (`RetailBattle::step_run` over all sixteen entries), when that is one
    /// step. The yaw counter drifts `max(1, 4 * step / 3)` a pass
    /// (`0x801E29E4..0x801E2A24`), so over the same vsyncs a steady step `3`
    /// carries it four units in three where the engine's step `2` carries
    /// it one a vsync; [`BattleDrive::steer`] rescales the engine's drift by
    /// that ratio before testing the counter against retail's.
    pub drift_step: Option<u8>,
    /// The capture is a monster's strike loop `0x1E` still on its walk clip
    /// (`+0x1D9 == 1`). The arrival poll stages idle under the `+0x1DC`
    /// bit-0 cut (`0x801E35BC..0x801E35C8`), and a walk entry carrying the
    /// `+0x76` lock refuses it (`0x80047940..0x80047948`), so the walk plays
    /// on through the loop's passes - the strike byte they stage waits for
    /// its natural end. The accumulator then dates the walk's last loop
    /// re-commit, which the engine's clip age leaves out; the yaw counter
    /// `yaw` places the capture instead: met once the engine's counter has
    /// run to retail's, and not pinned to it before.
    pub walk_clock: bool,
    /// A [`Self::walk_clock`] capture saved on the arrival itself: the poll
    /// `0x19` has just stored `0x1E` with idle staged and `+0x1DC` bit 0 cut
    /// (`0x801E35B4..0x801E35C8`), and no strike-loop pass has run yet - the
    /// cursor `ctx[+0x15]` is still `0`. A one-swing stream's first loop
    /// pass stages its byte and leaves for `0x1F` together
    /// (`0x801E3734`, `0x801E3998`), so the state lasts one pass and the
    /// yaw clock cannot wait for it: a replay whose walk was shorter arrives
    /// with its counter short of retail's and is past the state before the
    /// clock is due. The arrival is the event the capture names, so the gate
    /// is that pass, and the counter is pinned to retail's word there.
    pub arrival: bool,
    /// A monster strike's aim, as [`strike_aim_rescue`] takes it, carried on
    /// the drive: raised by the retail-compare search only once no stream
    /// reached the capture on the targets the replay picked itself.
    pub strike_aim: bool,
    /// The other monsters a monster strike's capture holds on their idle clip
    /// (`+0x1D9 == 0`), as a mask of pool slots: the gate waits for a turn
    /// that finds each of them idle too. A replay reaches the striker's
    /// turns on its own stream, and a later turn than retail's can open the
    /// same phase with another body still on a reaction clip a party swing
    /// left it in (`battle_dohaty_nova_v2_strike`: the second Nova mid
    /// knockdown, off its ground, where the capture's stands idle).
    pub idle_bystanders: u8,
    /// The capture's harvest probe restored the party's HP at every round
    /// prompt and raised the maximum to `9999` (`autorun_battle_harvest.lua`
    /// `LEGAIA_TOUGH=2`, kept in the state): its live HP is the probe's, not
    /// the rounds'. A replay that runs rounds ahead of the capture's own takes
    /// the monsters' swings with nothing restoring them, so the drive makes
    /// the same writes at its round prompts - every standing party seat back
    /// to its maximum, readout and pending total with it.
    pub tough: bool,
    /// The rung of the per-action yaw ladder retail's counter `yaw` climbed
    /// from, when the capture pins it: an Attack's `0x200`
    /// (`0x801E2F20`) with no swing re-seed possible since - a monster seat
    /// (the re-seed is gated on `ctx[+0x13] < 3`) or a party seat none of
    /// whose action entries carries the `+0x87 == 2` byte `FUN_8004E13C`
    /// re-seeds on. With the engine's counter on the same rung, the two are
    /// one clock read at different instants, however far apart they have
    /// run - retail's action can take longer than the engine's, and its
    /// adaptive frame step drifts the counter faster - so the capture's
    /// word is taken outright rather than only within [`YAW_CLOCK_SLACK`].
    pub rung: Option<u16>,
    /// The acting seat's latched raw staged id `+0x1DB` on an
    /// [`SpanGate::Age`] capture on a dynamic art slot: the byte the anim
    /// commit copies off `+0x1DA` before it rewrites an id `>= 0x10` to slot
    /// `0x10` / `0x11` (`0x8004AEB0..0x8004AEB8`). A starter and the art
    /// behind it install into the same slot, and once the strike loop has
    /// staged the art the strike cursor names both, so an `accum 0` capture
    /// met on clip and cursor alone takes the starter's commit for the art's
    /// (`minigame_muscle_dome_retail_skeleton_hyper_arts`: retail latched on
    /// the Hyper Art `0x1E` at accumulator `0`, the engine still on its
    /// SpecialStarter `0x1A`).
    pub latched: Option<u8>,
    /// The capture was played by `autorun_battle_harvest.lua`'s party-strike
    /// arm (scenario topic `harvest`), whose pad presses the target cursor's
    /// `Cross` 24 vsyncs after the arts entry's: the drive holds that cursor
    /// until the camera's case-1 glide onto it has landed
    /// ([`BattleDrive::press`]). A pacing of the capture's own input, read
    /// off the manifest; no play path reads it.
    pub harvest_pacing: bool,
    /// The win pose the capture's battle-end sequence latched
    /// (`RetailBattle::win_pose`), on a capture past the end signal. The
    /// pose pick draws it from `rand()` (`0x8004E78C..0x8004EAF0`), and the
    /// results framing is that pose's script, so a replay that finishes the
    /// fight on another stream frames another shot. The drive seats it
    /// (`BattleState::forced_victory_pose`) as retail's own held-button
    /// select would (`0x8004EAF4..0x8004EB8C`): over the pick, after its two
    /// draws, which it does not undo.
    pub win_pose: Option<u8>,
    /// The acting party seat's target and its idle-loop cursor on the frame
    /// the seat's action entered the seed state `0x0C`, read off the
    /// capture's cursor history ring (`RetailBattle::approach_start`). The
    /// next pass's recentre and the Attack arm `0x14` measure the range law
    /// against the target's body pair, which the idle loop's pose sways, so
    /// the drive lands the loop there once
    /// (`World::seat_battle_clip_cursor`) before the approach polls it. A
    /// seat of the capture's history; no play path reads it.
    pub approach_idle: Option<(u8, i32)>,
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
/// ([`legaia_engine_battle::victory::VictoryPhase`] is the engine's twin):
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
    /// ([`legaia_engine_cast::cast_module_camera::capture_countdown_va`]): the
    /// phase is held until the engine's module sits in that arm with its
    /// countdown run down at least as far - the module's own clock, which
    /// places a camera shot or drift in flight. A body that gates on no
    /// module word carries [`PHASE_ONLY`]: the engine's directed module is
    /// held to the capture's arm alone (`koru_blade_breath_mid_cast` sits in
    /// arm 3; without the arm the replay was filmed on `0x70`'s first tick).
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
    /// (`EngineBattle::age_short`).
    Age { accum: u16 },
    /// A capture ahead of the seed pass ([`PRE_SEED_STATES`]) whose camera
    /// has landed on its framing (`camera_landed`). `0x0A` waits on the CD
    /// (`FUN_8003F2B8(1)` at `0x801E2B78`) for as long as the next actor's
    /// data takes, which the engine's always-ready poll does not; the
    /// `evil_medallion_rage_battle` capture sits there with the commit
    /// confirm's case-9 glide finished. The drive holds the engine's wait
    /// until its own glide lands ([`BattleDrive::steer`]).
    Landed,
}

/// The countdown half of a `0x70` capture's arm gate when the body gates on
/// no module word ([`SpanGate::CaptureFade`]): the gate is the phase alone,
/// and only for a body the engine directs.
pub const PHASE_ONLY: i32 = i32::MAX;

/// The capture band `0x6E..=0x71`. `0x70` pins the style to `1`
/// (`sb v0,0xd(v1)` at `0x801E50CC`) without re-arming a framing, so a
/// capture there reads `1` over a camera the rolled style placed; the style
/// is not aligned in the band.
pub(crate) const CAPTURE_BAND: std::ops::RangeInclusive<u8> = 0x6E..=0x71;

/// The depth `0x6F` takes off `ctx[+0x6D0]` a vsync tick:
/// `16 * DAT_1F800393` a pass (`sll v1,v1,0x4` at `0x801E500C`), at the
/// port's one-vsync step.
const CAPTURE_RAMP_PER_TICK: u32 = 16;

/// The capture band's states placed by its CD holds
/// ([`SpanGate::CaptureFade`]).
pub const CAPTURE_FADE_STATES: [u8; 3] = [0x6E, 0x6F, 0x70];

/// How far the engine's `0x6F` depth ramp still has to come down to reach
/// retail's `height` (`ctx[+0x6D0]`, an unsigned halfword the ramp wraps),
/// `0` once it is there.
pub(crate) fn capture_ramp_left(world: &legaia_engine_core::world::World, height: u16) -> u16 {
    ((world.battle.camera_frame_height as u16).wrapping_sub(height) as i16).max(0) as u16
}

/// How old the active actor's clip is, in the close-up accumulator's units
/// ([`BattleCamera::clip_age`](legaia_engine_battle_vm::battle_cam_script::BattleCamera::clip_age)):
/// the engine's `ctx[+0x87C]` with the natural-end re-commits of a looping
/// clip left out. Retail's
/// accumulator restarts on those (`player_steal_skeleton_banner` reads `176`
/// 22 vsyncs after Vahn's idle wrapped), so the age is an upper bound on it
/// whenever the engine plays retail's clip - the gates read it with `>=` -
/// and it keeps placing a phase whose engine clip loops where retail's did
/// not (a parked Gaza on idle against the engine's Gaza still walking).
/// The active actor's close-up accumulator, retail's `ctx[+0x87C]`
/// ([`BattleCamera::close_up_accum`](legaia_engine_battle_vm::battle_cam_script::BattleCamera::close_up_accum)),
/// zeroed by every clip commit of the acting seat, a same-id one included
/// (`FUN_8004AD80`, `0x8004BF74`). An [`SpanGate::Age`] action phase waits on
/// it: the capture's own word is this one, and a commit the engine counts
/// as a re-commit still restarts it. The Spirit band stages its clip at
/// `0x46` (`+0x1DA = +0x1E7`); when that id is the one already committed,
/// the commit installs nothing new, so the clip age ran on from before the
/// action while retail's accumulator restarted (`delilas_gi_spirit_*`:
/// the gate met 22 to 42 ticks early). The capture band keeps the clip age
/// ([`capture_accum_done`]): a monster's capture-class cast commits nothing
/// on its way into `0x6E`, so both words run from its idle loop there.
pub fn active_close_up_accum(world: &legaia_engine_core::world::World) -> u32 {
    world
        .battle
        .camera
        .as_ref()
        .map_or(u32::MAX, |c| c.close_up_accum())
}

pub fn active_clip_age(world: &legaia_engine_core::world::World) -> u32 {
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
///
/// The engine's side is the clip age ([`active_clip_age`]), the accumulator
/// with a looping clip's natural-end re-commits left out, as the action
/// gates read it. A caster that idles through the band restarts the
/// accumulator at every wrap of its idle, on its own clock: read raw, the
/// engine's could wrap just as the yaw counter came due and hold the band
/// a whole idle cycle more (`zora_glare_petrify_pre`, fourteen counter units
/// past retail's `0x848`).
pub(crate) fn capture_accum_done(
    world: &legaia_engine_core::world::World,
    height: u16,
    accum: u16,
) -> bool {
    let want = accum.saturating_sub(capture_ramp_left(world, height) / 2);
    active_clip_age(world) >= u32::from(want)
}

/// The yaw counter's rung at the seed pass (`li v0,0x800` /
/// `sh v0,0x4(s7)` at `0x801E2CEC..0x801E2CF8`): every capture-band cast
/// counts from it.
const SEED_YAW_RUNG: i32 = 0x800;

/// Whether a capture-band capture's accumulator `accum` (`ctx[+0x87C]`)
/// started before the action's seed pass, so it cannot place the `0x6E`
/// hold.
///
/// Only a clip commit zeroes the accumulator (`FUN_8004AD80`,
/// `sw zero,0x87c` at `0x8004BF74`; the per-art latch at `0x801D7FF8` is an
/// Attack-band arm), and a monster's capture-class cast commits none on its
/// way into the band: the seed pass calls `FUN_801EFE44` and the framing,
/// neither of which stages a clip, and the cast begin's capture route
/// (`0x801E4480..0x801E44FC`) goes straight to `0x6E`. Its accumulator
/// therefore counts from the caster's last idle commit or wrap, wherever
/// that fell. The yaw counter does count from the seed: `0x800` there, then
/// at least one unit a display frame (`max(1, 4 * step / 3)` a pass of
/// `step` frames, `0x801E29E4`), while the accumulator gains exactly `8` a
/// frame (`8 * step` a framing call, `0x801D5900..0x801D5920`). An
/// accumulator that has run more frames than the counter has units since
/// the seed therefore began before it, and says where the caster's idle
/// loop stood rather than how long the band has run
/// (`battle_korb2_mage_void_accessories_mid_cast`: accumulator `384`, `48`
/// frames, against a counter `0x820`, `32` units past the seed). The
/// counter alone places such a capture.
pub(crate) fn accum_predates_seed(accum: u16, yaw: Option<u16>) -> bool {
    yaw.is_some_and(|y| {
        let run = i32::from(y) - SEED_YAW_RUNG;
        run >= 0 && i32::from(accum) / 8 > run
    })
}

/// The engine's yaw counter `counter` with its drift since the last outright
/// store (`drift`, run at the engine's step `engine_step`) rescaled to the
/// rate a steady `retail_step` drifts it at over the same vsyncs:
/// `inc(s) / s` a vsync, `inc(s) = max(1, 4 * s / 3)` a pass
/// ([`legaia_engine_battle_vm::battle_cam_script::action_yaw_pass_increment`]).
/// The rung the drift started from stays the engine's.
pub fn rescaled_yaw_counter(counter: i32, drift: i32, engine_step: u8, retail_step: u8) -> i32 {
    use legaia_engine_battle_vm::battle_cam_script::action_yaw_pass_increment as inc;
    let (se, sr) = (i32::from(engine_step.max(1)), i32::from(retail_step.max(1)));
    let scaled = drift * i32::from(inc(sr as u8)) * se / (i32::from(inc(se as u8)) * sr);
    counter - drift + scaled
}

/// The Spirit band's wait (`0x3D`): it holds until the caster's staged clip
/// commits, then hands on to the fire state `0x3E`.
const SPIRIT_WAIT_STATE: u8 = 0x3D;

/// The action SM's Done arm and the fade-down it hands to.
const ACTION_DONE_STATE: u8 = 0x50;
/// The action seed (`0x0C`), whose pass recentres the formation and hands an
/// Attack to `0x14`.
const ACTION_SEED_STATE: u8 = 0x0C;
const ACTION_DONE_FADE_STATE: u8 = 0x51;

/// How far apart the engine's yaw counter `ctx[+0x6DA]` and a capture's may
/// stand and still be the same rung of the per-action ladder read at another
/// instant (`BattleDrive::steer`): under half the `0x80` between the nearest
/// rungs (`0x200` / `0x280`), about a second of drift.
const YAW_CLOCK_SLACK: i32 = 0x3F;

/// How far short of retail's yaw counter a [`ActionSteer::walk_clock`]
/// capture's engine counter may stand and be due: one step-`2` SM pass of
/// drift (`max(1, 4 * 2 / 3)`, `0x801E29E4`), so the gate lands on the pass
/// whose drift reaches retail's word rather than the one after it.
const WALK_CLOCK_DUE: i32 = 2;

/// Whether a [`ActionSteer::walk_clock`] capture's engine yaw counter has
/// run to within [`WALK_CLOCK_DUE`] of retail's `yaw`.
fn walk_clock_due(world: &legaia_engine_core::world::World, yaw: Option<u16>) -> bool {
    let Some(yaw) = yaw else {
        return true;
    };
    world
        .battle
        .camera
        .as_ref()
        .is_none_or(|c| i32::from(yaw) - c.action_yaw_base() <= WALK_CLOCK_DUE)
}

/// Whether the engine's yaw counter has run as far through `0x6E` as
/// retail's `yaw` (`ctx[+0x6DA]`) says retail's did: the capture's value
/// less the `0x6F` frames still to come (the depth ramps `16` a display
/// frame against the counter's one). `None` - the Far option - holds
/// nothing.
pub(crate) fn capture_yaw_done(
    world: &legaia_engine_core::world::World,
    height: u16,
    yaw: Option<u16>,
    lead: bool,
) -> bool {
    let Some(yaw) = yaw else {
        return true;
    };
    // Per display frame the ramp falls `16 * step` and the counter climbs
    // `4 * step / 3` (`action_yaw_pass_increment`): `16` depth units a
    // counter unit at step `2`, `12` at step `3`.
    let step = world
        .battle
        .camera
        .as_ref()
        .map_or(2, |c| c.frame_step())
        .max(1);
    let per_unit = (16 * u16::from(step))
        / legaia_engine_battle_vm::battle_cam_script::action_yaw_pass_increment(step).max(1);
    let want = i32::from(yaw) - i32::from(capture_ramp_left(world, height) / per_unit.max(1));
    // `lead`: the poll this hold releases is the one the next SM pass makes,
    // and that pass drifts the counter in its prologue first
    // (`0x801E29E4..0x801E2A24`), so the counter `0x6E` leaves on is the one
    // the pass will have reached. A `0x70` capture's counter was read inside
    // the band and stood back by its ticks there, which already counts it.
    let inc = if lead {
        i32::from(legaia_engine_battle_vm::battle_cam_script::action_yaw_pass_increment(step))
    } else {
        0
    };
    world
        .battle
        .camera
        .as_ref()
        .is_none_or(|c| c.action_yaw_base() + inc >= want)
}

/// The module ticks a `0x70` capture's band spends before its arm gate
/// `(phase, countdown)` is met, counted from the band's first tick: the
/// module's own countdown walk (the capture director's, or the body's
/// countdown table), run from a default state. The yaw counter `ctx[+0x6DA]`
/// a capture holds was read that many ticks into `0x70`, so the `0x6E` hold
/// ([`capture_yaw_done`]) stands that far short of it. `0` for a module with
/// neither (or a walk that does not reach the gate).
pub(crate) fn capture_band_ticks_to_arm(
    world: &legaia_engine_core::world::World,
    seat: u8,
    arm: (u8, i32),
) -> u16 {
    use legaia_engine_cast::cast_module_camera as cmc;
    let Some((entry, body)) = capture_body(world, seat) else {
        return 0;
    };
    let director = cmc::capture_camera_director(entry, body);
    let table = cmc::capture_arm_countdowns(entry, body);
    if director.is_none() && table.is_none() {
        return 0;
    }
    let mut st = cmc::ModuleCamState::default();
    let seats = cmc::ModuleCamSeats::default();
    let mut phase = 0u8;
    for n in 1..=4096u16 {
        if let Some(d) = director {
            st.opens_pass = true;
            let a = d(&mut st, phase, seats);
            if !a.hold {
                phase = a.next.unwrap_or(phase.wrapping_add(1));
            }
        } else {
            match table.and_then(|t| cmc::arm_countdown(t, phase)) {
                Some(c) if c.holds(&mut st.countdown) => {}
                Some(c) => {
                    c.pass(&mut st.countdown);
                    phase = phase.wrapping_add(1);
                }
                None => return 0,
            }
        }
        if phase > arm.0 || (phase == arm.0 && st.countdown.0 <= arm.1) {
            return n;
        }
    }
    0
}

/// The `(PROT entry, tick body)` of the capture-class cast `seat` has
/// queued, keyed as [`legaia_engine_cast::cast_module_camera::capture_camera_director`]
/// takes it.
fn capture_body(world: &legaia_engine_core::world::World, seat: u8) -> Option<(u32, u32)> {
    let action = world
        .actors
        .get(usize::from(seat))
        .map(|a| a.battle.params[0])?;
    let entry = world.cast_module_for(action)?;
    let body = legaia_engine_cast::cast_module_ticks::capture_tick_body(entry, action)
        .unwrap_or(legaia_engine_cast::cast_module_camera::SINGLE_BODY);
    Some((entry, body))
}

/// Whether the engine walks the phase of the capture-class cast `seat` has
/// queued on retail's arms: a body with a camera director. Only those place
/// a [`PHASE_ONLY`] gate; another body's engine phase is not retail's clock.
fn capture_phase_directed(world: &legaia_engine_core::world::World, seat: u8) -> bool {
    capture_body(world, seat).is_some_and(|(entry, body)| {
        // PROT 0954 has no director: its whole body runs retail's arms.
        entry == 954
            || legaia_engine_cast::cast_module_camera::capture_camera_director(entry, body)
                .is_some()
    })
}

/// Whether the engine's `0x6F` pull-in has come down at least as far as
/// retail's `height` (`ctx[+0x6D0]`, an unsigned halfword the ramp wraps).
pub(crate) fn capture_ramp_done(world: &legaia_engine_core::world::World, height: u16) -> bool {
    capture_ramp_left(world, height) == 0
}

impl SpanGate {
    pub fn to_env(self) -> (u8, u16) {
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
    pub(crate) fn env_value(self) -> String {
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

    pub fn from_env(kind: &str, value: &str) -> Option<Self> {
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

    /// Whether the engine's battle-end sequence sits in this gate's phase,
    /// whatever its count.
    pub fn phase_kind_held(self, world: &legaia_engine_core::world::World) -> bool {
        use legaia_engine_battle::victory::VictoryPhase;
        world.battle.victory.is_some_and(|seq| {
            matches!(
                (self, seq.phase),
                (Self::Loading, VictoryPhase::Loading { .. })
                    | (Self::Results { .. }, VictoryPhase::Results { .. })
                    | (Self::Exit { .. }, VictoryPhase::Exit { .. })
            )
        })
    }

    /// Whether the engine's battle-end sequence holds this gate.
    pub fn met(self, world: &legaia_engine_core::world::World) -> bool {
        use legaia_engine_battle::victory::VictoryPhase;
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
            Self::Menu {
                flow,
                seat,
                spirit,
                pick,
            } if pick != 0 => format!("menu,{},{seat},{spirit},{pick}", flow.raw()),
            Self::Menu {
                flow, seat, spirit, ..
            } if spirit != 0 => {
                format!("menu,{},{seat},{spirit}", flow.raw())
            }
            Self::Menu { flow, seat, .. } => format!("menu,{},{seat}", flow.raw()),
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
                if let Some(e) = steer.prior {
                    s.push_str(&format!(
                        ",e{}/{}/{}/{}/{}",
                        e.seat, e.category, e.target, e.style, e.depth
                    ));
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
                if let Some(item) = steer.loot {
                    s.push_str(&format!(",L{item}"));
                }
                if steer.park {
                    s.push_str(",h");
                }
                if let Some(r) = steer.drift_step {
                    s.push_str(&format!(",r{r}"));
                }
                if steer.walk_clock {
                    s.push_str(",w");
                }
                if steer.arrival {
                    s.push_str(",v");
                }
                if steer.strike_aim {
                    s.push_str(",x");
                }
                if steer.idle_bystanders != 0 {
                    s.push_str(&format!(",B{:x}", steer.idle_bystanders));
                }
                if steer.tough {
                    s.push_str(",z");
                }
                if steer.harvest_pacing {
                    s.push_str(",d");
                }
                if let Some(w) = steer.win_pose {
                    s.push_str(&format!(",W{w}"));
                }
                if let Some((t, c)) = steer.approach_idle {
                    s.push_str(&format!(",i{t}/{c}"));
                }
                if let Some(n) = steer.rung {
                    s.push_str(&format!(",n{n}"));
                }
                if let Some(l) = steer.latched {
                    s.push_str(&format!(",l{l}"));
                }
                if let Some(q) = steer.queue {
                    s.push_str(",q");
                    for b in q {
                        s.push_str(&format!("{b:02x}"));
                    }
                }
                if let Some((attack, guard, atk)) = steer.ramps {
                    s.push_str(&format!(",j{attack}/{guard}/{atk}"));
                }
                if let Some(q) = steer.played {
                    s.push_str(",b");
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
            if let Some(e) = last.strip_prefix('e') {
                let mut f = e.split('/');
                steer.prior = Some(PriorFraming {
                    seat: f.next()?.parse().ok()?,
                    category: f.next()?.parse().ok()?,
                    target: f.next()?.parse().ok()?,
                    style: f.next()?.parse().ok()?,
                    depth: f.next()?.parse().ok()?,
                });
            } else if let Some(t) = last.strip_prefix('t') {
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
            } else if let Some(item) = last.strip_prefix('L') {
                steer.loot = Some(item.parse().ok()?);
            } else if last == "h" {
                steer.park = true;
            } else if last == "w" {
                steer.walk_clock = true;
            } else if last == "v" {
                steer.arrival = true;
            } else if last == "x" {
                steer.strike_aim = true;
            } else if let Some(m) = last.strip_prefix('B') {
                steer.idle_bystanders = u8::from_str_radix(m, 16).ok()?;
            } else if last == "z" {
                steer.tough = true;
            } else if last == "d" {
                steer.harvest_pacing = true;
            } else if let Some(w) = last.strip_prefix('W') {
                steer.win_pose = Some(w.parse().ok()?);
            } else if let Some(i) = last.strip_prefix('i') {
                let (t, c) = i.split_once('/')?;
                steer.approach_idle = Some((t.parse().ok()?, c.parse().ok()?));
            } else if let Some(r) = last.strip_prefix('r') {
                steer.drift_step = Some(r.parse().ok()?);
            } else if let Some(n) = last.strip_prefix('n') {
                steer.rung = Some(n.parse().ok()?);
            } else if let Some(l) = last.strip_prefix('l') {
                steer.latched = Some(l.parse().ok()?);
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
            } else if let Some(j) = last.strip_prefix('j') {
                let mut f = j.split('/');
                steer.ramps = Some((
                    f.next()?.parse().ok()?,
                    f.next()?.parse().ok()?,
                    f.next()?.parse().ok()?,
                ));
            } else if let Some(q) = last.strip_prefix('b') {
                if q.len() != 32 {
                    return None;
                }
                let mut played = [0u8; 16];
                for (i, b) in played.iter_mut().enumerate() {
                    *b = u8::from_str_radix(q.get(i * 2..i * 2 + 2)?, 16).ok()?;
                }
                steer.played = Some(played);
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
                spirit: 0,
                pick: 0,
            }),
            ("menu", &[flow, seat, spirit]) => Some(Self::Menu {
                flow: BattleFlowState::from_raw(flow),
                seat,
                spirit,
                pick: 0,
            }),
            ("menu", &[flow, seat, spirit, pick]) => Some(Self::Menu {
                flow: BattleFlowState::from_raw(flow),
                seat,
                spirit,
                pick,
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
    pub fn steering(&self) -> Option<ActionSteer> {
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

    /// The close-up accumulator an [`SpanGate::Age`] action phase waits for,
    /// `None` on every other drive.
    pub fn age_gate(&self) -> Option<u16> {
        match *self {
            Self::Action {
                end: SpanGate::Age { accum },
                ..
            } => Some(accum),
            _ => None,
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
        // A capture showing the steal caption holds a landed `rand() % 100`
        // the replay's stream does not share: the outcome is the capture's,
        // so it is seeded rather than searched for across streams (a search
        // let any change to the swing's draws decide whether the phase was
        // reachable at all).
        if let Self::Action { seat, steer, .. } = *self
            && seat < 3
            && steer.spoils
        {
            world.battle.forced_steal = true;
        }
        // A capture past the end signal names its pose seat, the acting slot
        // the victory arm left (`ctx[+0x13]`). The replay seats the capture's
        // beaten monster at the HP the capture reads, so the first action to
        // end closes the fight on whoever acted: in `noa_levelup_banner` that
        // was Gala, where retail's Vahn poses.
        if let Self::Action { seat, end, .. } = *self
            && seat < 3
            && end.is_end()
        {
            let pc = world.party.party_count.clamp(1, 3);
            world.battle.forced_victory_seat = Some(engine_seat(seat, pc));
        }
        // ... and the pose the capture's sequence latched on it.
        if let Self::Action { end, steer, .. } = *self
            && end.is_end()
        {
            world.battle.forced_victory_pose = steer.win_pose;
        }
        // Likewise the drop a results-frame capture's report shows.
        if let Self::Action { steer, .. } = *self
            && let Some(item) = steer.loot
        {
            world.battle.forced_drop = Some(item);
        }
        // Likewise the absorb roll of a capture whose kill took a Seru.
        if let Self::Action { seat, absorbed, .. } = *self
            && seat < 3
            && absorbed != 0
        {
            world.battle.forced_absorb = true;
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
                // A party seat keeps its number; a monster seat, the caster's
                // own or the patient of its Heal, is re-keyed to its engine seat.
                world.battle.forced_monster_target = self
                    .steering()
                    .and_then(|s| s.target)
                    .and_then(|t| match t {
                        0..3 => Some(t),
                        t if t < 8 => Some(engine_seat(t, pc)),
                        _ => None,
                    });
            }
        }
    }

    /// What a strike's drive holds of the capture's hand: a monster's queue
    /// and either striker's approach terms.
    pub fn strike_hand(&self) -> Option<StrikeHand> {
        match self {
            Self::Action { seat, steer, .. } if steer.played.is_some() || steer.ramps.is_some() => {
                Some(StrikeHand {
                    seat: *seat,
                    queue: steer.played,
                    ramps: steer.ramps,
                })
            }
            _ => None,
        }
    }

    pub fn reached(&self, world: &legaia_engine_core::world::World) -> bool {
        if world.mode != SceneMode::Battle {
            return false;
        }
        let pc = world.party.party_count.clamp(1, 3);
        match *self {
            // The first frame whose monsters are drawable: the bodies bind
            // a few ticks after the mode flip. A body the stage loader hid
            // (the evolved Cort, until PROT 0968 drops him in) is as ready
            // as it will be.
            Self::Opening { swept, entry } => {
                // A sparring capture past the intro timer also holds the
                // side-band's caption, which the engine raises on the round
                // start the open was holding back
                // (`BattleState::battle_open_held`).
                let ok = (!swept
                    || world.battle.intro_names_frames == 0 && !world.battle.round_open_pending)
                    && entry_sweep_reached(world, entry)
                    && world.actors.iter().enumerate().all(|(i, a)| {
                        a.battle_monster_id.is_none()
                            || !a.active
                            || a.tmd_binding.is_some()
                                && (world.battle_body_hidden(i)
                                    || world
                                        .battle_actor_draw_plan(i, None, 4.0, false)
                                        .is_none_or(|p| p.drawn))
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
            Self::Menu {
                flow, seat, pick, ..
            } => {
                world.battle.flow == flow
                    && (!menu_seat_matters(flow) || world.battle_ctx.active_actor == seat)
                    && !world.battle.camera.as_ref().is_some_and(|c| c.is_gliding())
                    && cursor_on_pick(world, flow, pick)
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
                    && (0..8u8)
                        .filter(|s| steer.idle_bystanders & (1 << s) != 0)
                        .all(|s| {
                            let i = usize::from(engine_seat(s, pc));
                            world.actors.get(i).is_none_or(|a| {
                                !a.active || a.battle.hp == 0 || world.battle_current_anim(i) == 0
                            })
                        })
                    && match end {
                        SpanGate::DoneHold { timer } => world.battle_ctx.frame_timer <= timer,
                        // A phase past arm 0 with no module word is placed
                        // inside the arm by the close-up accumulator, which
                        // every pass's framing call advances and the arm's
                        // clip commit zeroed: `battle_tunnelc_xain_bloody_horns_mid_cast`
                        // reads `16`, a pass past the walk's commit, and the
                        // arm's first tick showed the frame a pass early.
                        // Arm 0 commits no clip of its own, so its
                        // accumulator counts the band, not the arm.
                        SpanGate::CaptureFade {
                            arm: Some((phase, PHASE_ONLY)),
                            accum,
                            ..
                        } if state == 0x70 => {
                            !capture_phase_directed(world, engine_seat(seat, pc))
                                || world.casting.module_phase > phase
                                || world.casting.module_phase == phase
                                    && (phase == 0
                                        || active_close_up_accum(world) >= u32::from(accum))
                        }
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
                                && capture_yaw_done(world, height, yaw, false)
                        }
                        SpanGate::CaptureFade { height, .. } if state == 0x6F => {
                            capture_ramp_done(world, height)
                        }
                        SpanGate::Landed => {
                            !world.battle.camera.as_ref().is_some_and(|c| c.is_gliding())
                        }
                        SpanGate::Age { accum } => {
                            (if steer.walk_clock && steer.arrival {
                                world
                                    .actors
                                    .get(usize::from(engine_seat(seat, pc)))
                                    .is_some_and(|a| a.battle.strike_index == 0)
                            } else if steer.walk_clock {
                                walk_clock_due(world, steer.yaw)
                            } else {
                                active_close_up_accum(world) >= u32::from(accum)
                            }) && steer.clip.is_none_or(|k| {
                                world.battle_current_anim(usize::from(engine_seat(seat, pc))) == k
                            }) && steer.cursor.is_none_or(|u| {
                                world
                                    .actors
                                    .get(usize::from(engine_seat(seat, pc)))
                                    .is_some_and(|a| a.battle.strike_index == u)
                            }) && steer.played.zip(steer.cursor).is_none_or(|(q, u)| {
                                let n = usize::from(u).min(q.len());
                                world
                                    .actors
                                    .get(usize::from(engine_seat(seat, pc)))
                                    .is_some_and(|a| a.battle.params.get(..n) == Some(&q[..n]))
                            }) && steer.latched.is_none_or(|l| {
                                world
                                    .actors
                                    .get(usize::from(engine_seat(seat, pc)))
                                    .is_some_and(|a| a.battle.latched_anim == l)
                            }) && (!steer.spoils
                                || world.battle_ctx.message_id
                                    == legaia_engine_battle::battle_steal::STEAL_CAPTION_ELEMENT)
                                && steer.fading.is_none_or(|(v, lane)| {
                                    world
                                        .actors
                                        .get(usize::from(engine_seat(v, pc)))
                                        .is_some_and(|a| {
                                            a.battle.render_flag
                                    == legaia_engine_battle_vm::battle_formulas::STATE_DEFEAT_FADE
                                    && (a.battle.render_color & 0x3FF) as u16 <= lane
                                        })
                                })
                        }
                        _ => true,
                    }
            }
        }
    }

    /// Whether the engine sits in an action phase's state - the acting seat,
    /// its state and the steer's target - whatever its span gate reads.
    pub fn in_action_state(&self, world: &legaia_engine_core::world::World) -> bool {
        let mut base = *self;
        match &mut base {
            Self::Action { end, steer, .. } => {
                // Past the end signal the "state" is the sequencer's phase:
                // the action byte holds its last value through the load
                // window and the results alike, and the results frame is the
                // entry a retake counts across. The seat's target is whatever
                // its last action left (the load window only forces it into
                // the monster band), which a replay that closes the fight on
                // another monster does not share; the sequencer frames the
                // pose seat, not its target.
                let phase_kind = end.is_end().then_some(*end);
                if phase_kind.is_some() {
                    steer.target = None;
                }
                *end = SpanGate::None;
                base.reached(world) && phase_kind.is_none_or(|gate| gate.phase_kind_held(world))
            }
            _ => false,
        }
    }

    /// Whether the engine sits in an [`SpanGate::Age`] action phase's state
    /// at all, whatever its age.
    pub fn in_aged_state(&self, world: &legaia_engine_core::world::World) -> bool {
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
        // The drive presses through the command flow on its own clock; the
        // glides that hold a player's pad are landed under it, so reaching
        // a phase costs the ticks it always did. Once the phase is reached
        // its own glide is the capture's to seat, and runs untouched.
        if !self.reached(world) {
            world.battle.release_command_pad_hold();
            world.land_command_band_sideband_read();
        }
        let Self::Action {
            seat,
            state: want,
            category,
            end,
            style,
            steer,
            ..
        } = *self
        else {
            return;
        };
        // The yaw counter's half-turn coin ([`ActionSteer::yaw`]): from the
        // seed's Attack arm (`0x14`) to the capture's state. The seed pass
        // `0x0C` itself stands the counter on the fixed `0x800` rung
        // (`0x801E2CF8`), which is no coin; with the SM running one pass a
        // battle frame that rung is live for a whole frame, and flipping it
        // swung the seed's framing half a turn.
        let mut coin_seat = None;
        {
            let pc = world.party.party_count.clamp(1, 3);
            let state = world.battle_ctx.action_state;
            let pass_next = world.battle_sm_pass_due_next_tick();
            let step = world.battle_frame_step();
            if let Some(yaw) = steer.yaw
                && world.mode == SceneMode::Battle
                && world.battle_ctx.active_actor == engine_seat(seat, pc)
                && (0x14.min(want)..=want).contains(&state)
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
                // The Done band's fade-down `0x51` arms case 8 from the
                // counter on its first pass, which a DoneHold capture's own
                // first tick already shows: the rescaled pin runs from the
                // Done arm `0x50` that hands over to it.
                let done_entry = matches!(end, SpanGate::DoneHold { .. })
                    && want == ACTION_DONE_FADE_STATE
                    && state == ACTION_DONE_STATE;
                // A Spirit-band capture whose wait `0x3D` hands the engine
                // straight onto its gate: the clip commit that ends the
                // wait zeroes the accumulator, so an [`SpanGate::Age`] gate
                // can be met on the very pass that enters the capture's
                // state, and the drive stops before steering there. The
                // wait's length is the caster's idle phase, which the replay
                // does not share, so the counter is pinned through the wait
                // instead - each pass lands it on retail's word
                // (`battle_garmel_vera_v2_heal_mid_cast`: retail `2060`, the
                // replay's wait `36` passes longer).
                let stage_lead = steer.rung == Some(SEED_YAW_RUNG as u16)
                    && (SPIRIT_WAIT_STATE..want).contains(&state);
                if (state == want || done_entry || stage_lead)
                    && !matches!(end, SpanGate::CaptureFade { .. })
                {
                    let near = |c: i32| {
                        let d = (c - i32::from(yaw)).rem_euclid(0x1000);
                        d.min(0x1000 - d) <= YAW_CLOCK_SLACK
                    };
                    let rescaled = steer.drift_step.is_some_and(|sr| {
                        near(rescaled_yaw_counter(
                            cam.action_yaw_base(),
                            cam.action_yaw_drift(),
                            step,
                            sr,
                        ))
                    });
                    // The same rung on both sides ([`ActionSteer::rung`]):
                    // retail's word is this counter further along.
                    let same_rung = steer.rung.is_some_and(|r| {
                        let rung = i32::from(r);
                        cam.action_yaw_base() - cam.action_yaw_drift() == rung
                            && i32::from(yaw) >= rung
                    });
                    let clock_held = steer.walk_clock
                        && !steer.arrival
                        && i32::from(yaw) - cam.action_yaw_base() > WALK_CLOCK_DUE;
                    if !clock_held
                        && (((state == want || stage_lead)
                            && (steer.park || steer.arrival || near(cam.action_yaw_base())))
                            || rescaled
                            || same_rung)
                    {
                        // Retail's word is the one its last pass left, after
                        // that pass's prologue drift. On retail's cadence the
                        // pin lands ahead of a tick whose pass adds the drift
                        // again, so it stands one pass short there.
                        let drift = if pass_next {
                            i32::from(
                                legaia_engine_battle_vm::battle_cam_script::action_yaw_pass_increment(
                                    step,
                                ),
                            )
                        } else {
                            0
                        };
                        cam.set_action_yaw_base(i32::from(yaw) - drift);
                    }
                }
                if let Some(coin) = steer.coin {
                    cam.align_phase_cursor(coin);
                    coin_seat = Some(coin);
                }
            }
        }
        // The coin is a ctx byte the camera reads every drive
        // (`BattleCamInputs::track_coin`): seat it on the world.
        if let Some(coin) = coin_seat {
            world.battle_ctx.track_coin = coin & 1;
        }
        let pc = world.party.party_count.clamp(1, 3);
        // The harvest probe's round-prompt writes ([`ActionSteer::tough`]).
        if steer.tough {
            tough_round_restore(world);
        }
        let ours = world.mode == SceneMode::Battle
            && world.battle_ctx.active_actor == engine_seat(seat, pc);
        let state = world.battle_ctx.action_state;
        // The target's idle loop at the action seed
        // ([`ActionSteer::approach_idle`]): once, on the first tick the
        // driven seat's action sits in the seed state `0x0C`.
        if ours
            && state == ACTION_SEED_STATE
            && !world.battle.approach_idle_seated
            && let Some((target, cursor)) = steer.approach_idle
        {
            world.battle.approach_idle_seated = true;
            world.seat_battle_clip_cursor(usize::from(engine_seat(target, pc)), 0, cursor);
        }
        // A monster cast's clip bytes ([`ActionSteer::queue`] on a monster
        // seat whose drive is a cast): the capture's `+0x1E0` and the Spirit
        // band's `+0x1E7` (`params[1]` / `params[8]`), held from the seed to
        // the stage that commits one of them (`0x29`, or the Spirit band's
        // `0x3C`). A harness seat: the picker stored them under draws the
        // replay does not share.
        if seat >= 3
            && ours
            && category == 2
            && (0x0A..0x3D).contains(&state)
            && let Some(queue) = steer.queue
            && let Some(a) = world.actors.get_mut(usize::from(engine_seat(seat, pc)))
            && a.battle.params[0] == queue[0]
        {
            a.battle.params[1] = queue[1];
            if queue[8] != 0 {
                a.battle.queued_anim_b = queue[8];
            }
        }
        // A monster strike gated on its first swing clip
        // ([`ActionSteer::queue`] on a monster seat): from the seed to the
        // approach's end, the stream the strike loop stages its swings from
        // is the capture's own. The picker drew its picks already, so the
        // stream moves no draw.
        if seat >= 3
            && ours
            && (0x0C..0x1E).contains(&state)
            && let Some(queue) = steer.queue
            && let Some(a) = world.actors.get_mut(usize::from(engine_seat(seat, pc)))
        {
            let n = a.battle.params.len().min(queue.len());
            a.battle.params[..n].copy_from_slice(&queue[..n]);
        }
        // A monster strike's aim, on a rescue run only (harness only,
        // `LEGAIA_BATTLE_STRIKE_AIM`): the replay reaches the striker's turns
        // on another `rand()` count than retail's, so one unit of an earlier
        // walk or nudge can aim every one of them at another member and leave
        // the gate unmet. From the seed pass to the Attack arm, a physical
        // turn of the striker then aims at the capture's `+0x1DD` while that
        // member stands (a cast keeps its own aim). Off by default: aiming
        // every turn lets an earlier turn than the one retail stands on meet
        // the gate (`battle_jou_gola_gola_v3_strike`'s yaw), so the image
        // child takes it only once a run has missed the gate.
        if seat >= 3
            && ours
            && category == 3
            && (strike_aim_rescue() || steer.strike_aim)
            && (0x0C..0x14).contains(&state)
            && let Some(t) = steer.target.map(|t| engine_seat(t, pc)).filter(|&t| t < pc)
            && world
                .actors
                .get(usize::from(t))
                .is_some_and(|a| a.battle.hp > 0)
            && let Some(a) = world.actors.get_mut(usize::from(engine_seat(seat, pc)))
            && a.battle.action_category == 3
        {
            a.battle.active_target = t;
        }
        // The parked approach ([`ActionSteer::park`]): from the seat's Attack
        // arm `0x14` to the poll, its walk is held where it stands.
        world.battle.approach_hold =
            (steer.park && ours && (0x14..=want).contains(&state)).then(|| engine_seat(seat, pc));
        // ... and in the poll its walk clip is the dead one: the flinch's
        // stale `+0x1DC` bit 2 turns the Move clip's natural end into idle
        // (`sb zero,0x1da` at `0x80047B44`), so the park shows the idle loop,
        // not the walk.
        if steer.park
            && ours
            && state == want
            && let Some(a) = world.actors.get_mut(usize::from(engine_seat(seat, pc)))
        {
            a.battle.queued_anim = 0;
        }
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
            && (state == want || (seat < 3 || category == 3) && (0x0C..=want).contains(&state))
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
        // A walk-in body's arm 0 rolls the style afresh (`rand() % 4` at
        // `0x801F71D0` in PROT 0952) ahead of its case 6, and a capture on
        // that pass holds the roll; every later pass re-pins `1` before the
        // module ticks, which the roll then matches anyway. The approach leg
        // standing in for the arm takes the pin over its own roll.
        let arm0_roll = want == 0x70
            && CAPTURE_BAND.contains(&state)
            && world.casting.module_phase == 0
            && matches!(end, SpanGate::CaptureFade { arm: Some((p, _)), .. } if p >= 1)
            && capture_body(world, world.battle_ctx.active_actor).is_some_and(|(e, b)| {
                legaia_engine_cast::cast_module_camera::capture_approach_yaw_base(e, Some(b))
                    .is_some()
            });
        world.battle_ctx.camera_variant_pin = style.filter(|_| {
            ours && ((seat < 3 || category == 3)
                && (PRE_SEED_STATES.contains(&state) || state == 0x0C)
                && !CAPTURE_BAND.contains(&want)
                || arm0_roll)
        });
        if end == SpanGate::Landed {
            // From the tick the replay arms the captured seat: the setup
            // states arm no framing, so the earlier actor's stands through
            // them, and the drive can end on the tick the SM reaches `want`.
            if let Some(e) = steer.prior
                && ours
                && (state == want || PRE_SEED_STATES.contains(&state))
            {
                let (seat, target) = e.engine_seats(pc);
                world.seat_prior_action_framing(seat, e.category, target, e.style, e.depth as i16);
            }
            let gliding = world.battle.camera.as_ref().is_some_and(|c| c.is_gliding());
            world.battle.prev_action_cleared = !(ours && state == want && state == 0x0A && gliding);
            return;
        }
        let SpanGate::CaptureFade {
            height,
            accum,
            yaw,
            arm,
        } = end
        else {
            return;
        };
        // A directed module's countdown drains `scalar * step` a pass, so on
        // retail's cadence the replay's countdown moves on a lattice of whole
        // passes from the arm's own start. Retail's frames do not all share
        // a step (`FUN_80016B6C` measures each one's cost), and the paging
        // and stream-in that open the band run long: Guilty Cross's arm-1
        // word `456` sits half a step-2 pass off the replay's `464` / `448`,
        // as does Cort's clip cursor (`142` at rate `4`, with the two counted
        // from the same pass). A pass a step longer moves both onto
        // retail's; the replay takes it on the arm's first pass, the gate
        // then lands on retail's word, and the display lag's frame two passes
        // back on retail's displayed one.
        if let Some((phase, countdown)) = arm
            && countdown != PHASE_ONLY
            && ours
            && want == 0x70
            && state == 0x70
            && world.casting.module_phase == phase
            && world.battle_sm_pass_due_next_tick()
        {
            let step = i32::from(world.battle_frame_step().max(1));
            let per_vsync = legaia_engine_cast::cast_module_camera::MODULE_DRAIN_PER_TICK;
            let ahead = world.casting.module_cam.countdown.0 - countdown;
            let off = ahead.rem_euclid(per_vsync * step);
            if ahead > per_vsync * step && off != 0 && off % per_vsync == 0 {
                world.stretch_battle_frame((step + off / per_vsync) as u8);
            }
        }
        // Whether retail's accumulator was already running before the seed
        // pass, read before the band adjustment below.
        let accum_stale = accum_predates_seed(accum, yaw);
        // A `0x70` capture read its yaw counter some ticks into the band:
        // the `0x6E` hold stands that far short of it.
        let yaw = match arm {
            Some(a) if want == 0x70 && state == 0x6E => yaw.map(|y| {
                y.saturating_sub(capture_band_ticks_to_arm(
                    world,
                    world.battle_ctx.active_actor,
                    a,
                ))
            }),
            _ => yaw,
        };
        world.audio.sound_bank_ready = !(ours
            && state == 0x6E
            && !((accum_stale || capture_accum_done(world, height, accum))
                && capture_yaw_done(world, height, yaw, want != 0x70)));
        // `0x6F` ramps the depth before it polls (`0x801E4FFC..0x801E5014`,
        // then `FUN_8003F2B8(1)`), so the pass that leaves the state has
        // ramped too: retail's depth on entering `0x70` is the exit pass's.
        // A capture past `0x6F` releases the poll on the pass whose ramp lands
        // the capture's depth, not on the one after it (which took one more
        // ramp step, `16` a vsync - `48` at a frame step of `3` - off the
        // depth every `0x70` shot is cut at). A capture in `0x6F` itself is
        // sampled on that landing tick, so it keeps the state held.
        let per_pass = CAPTURE_RAMP_PER_TICK
            * if world.toggles.battle_sm_frame_cadence {
                u32::from(world.battle_frame_step().max(1))
            } else {
                1
            };
        let release_at = if want == 0x6F { 0 } else { per_pass };
        world.battle.prev_action_cleared =
            !(ours && state == 0x6F && u32::from(capture_ramp_left(world, height)) > release_at);
        // Retail's ramp ran on its own frame steps (2 and 3 vsyncs a pass,
        // as each frame's cost set them), so the depth it left `0x6F` on
        // sits on a lattice the replay's passes need not share: at a fixed
        // step the engine's last ramp can land up to a pass short of it.
        // Before the module's arm 0 cuts its shot on that depth (`0x70`,
        // the module phase still `0`), stand it on retail's.
        let short = i32::from(height as i16) - i32::from(world.battle.camera_frame_height);
        if ours
            && want == 0x70
            && state == 0x70
            && world.casting.module_phase == 0
            && short > 0
            && (short as u32) < per_pass
        {
            world.battle.camera_frame_height = height as i16;
        }
        // `0x6F` ramps `ctx[+0x6D0]` and then arms case 6 on the new depth in
        // the same pass (`sh a0,0x6d0(v0)` at `0x801E5014`, `jal 0x801D5854`
        // at `0x801E501C`), so the pass that lands the capture's depth also
        // framed it. The engine ramps every vsync tick and steps its camera
        // every frame step on its own phase, so the gate tick may frame the
        // depth a tick behind (`zora_glare_petrify_pre`: TR z `4633`, the
        // ramp's previous `2896`, against retail's `4608`). On a capture
        // taken in `0x6F`, put the camera's step on the tick the ramp lands.
        let left = u32::from(capture_ramp_left(world, height));
        if ours
            && state == 0x6F
            && want == 0x6F
            && left > 0
            && let Some(cam) = world.battle.camera.as_mut()
        {
            let step = u32::from(cam.frame_step().max(1));
            let ticks = left.div_ceil(CAPTURE_RAMP_PER_TICK);
            // `sync_step_phase(t)` leaves the camera stepping on every tick
            // whose count from now is `step - t` modulo the step; land one on
            // the tick `ticks` from now.
            let t = (step - (ticks - 1) % step) % step;
            cam.sync_step_phase(u64::from(t));
        }
    }

    /// The press that walks the engine one step toward the phase, or `None`
    /// when nothing on screen wants one (the action SM owns the frame).
    pub fn press(
        &self,
        world: &legaia_engine_core::world::World,
    ) -> Option<legaia_engine_system::input::PadButton> {
        use legaia_engine_battle_menus::battle_input::CommandPhase;
        use legaia_engine_system::input::PadButton;
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
                && let legaia_engine_battle::target_picker::PickerState::Cursor {
                    row: legaia_engine_battle::target_picker::CursorRow::Enemy,
                    slot,
                } = picker.state()
                && slot != want
                && aim_alive(world, want)
            {
                return Some(PadButton::Right);
            }
        }
        if let Self::Action { steer, .. } = self
            && steer.arts
            && world.battle.arts_input.is_some()
        {
            // The entry's confirm opens the target cursor and arms its
            // case-1 shot (`0x50 -> 0x5A`, `FUN_801D388C(0xC, 0)` at
            // `0x801D20B0`). The party-strike captures were played by
            // `autorun_battle_harvest.lua`, which presses the cursor's
            // `Cross` 24 vsyncs after the entry's - about the shot's
            // `0xC`-frame tween - so the round's first framing starts from a
            // landed case 1. Confirmed on the next tick, the replay began it
            // mid-glide (`battle_ropeway_party_gala_flying_knee_mid` `camera`
            // `.964` to `.981`, `image` `.592` to `.910`). A harness pacing
            // ([`ActionSteer::harvest_pacing`]): no play path reads it.
            if steer.harvest_pacing
                && world
                    .battle
                    .arts_input
                    .as_ref()
                    .and_then(|a| a.picker())
                    .is_some()
                && world.battle.camera.as_ref().is_some_and(|c| c.is_gliding())
            {
                return None;
            }
            return Some(PadButton::Cross);
        }
        // A window's target sub-cursor: walk the window's cursor onto the
        // capture's row (`pick`, the seat's `+0x1DF`), then `Cross`, which
        // raises the sub-cursor that row's target flags name. With no pick,
        // the row the window opens on.
        if let Self::Menu { flow, pick, .. } = *self
            && let Some(window) = flow.window_of_target()
            && world.battle.flow == window
        {
            let under = if window == BattleFlowState::ItemWindow {
                world
                    .battle
                    .item_menu
                    .as_ref()
                    .map(|m| m.current_item().map(|e| e.id))
            } else {
                world
                    .battle
                    .spell_menu
                    .as_ref()
                    .map(|m| m.menu_spell().map(|r| r.id))
            };
            if let Some(under) = under {
                return Some(if pick != 0 && under != Some(pick) {
                    PadButton::Down
                } else {
                    PadButton::Cross
                });
            }
        }
        let cmd = world.battle.command.as_ref()?;
        match *self {
            Self::Opening { .. } => None,
            Self::Menu {
                flow,
                seat,
                spirit,
                pick,
            } => {
                let ours = cmd.actor == seat && flow != BattleFlowState::CommitBegin;
                // The Attack cursor's capture: the screen walk that made it
                // pressed with idle vsyncs between, so each framing had
                // landed before the next press - and the cursor's case-1
                // yaw reads the camera's live focus (`0x801D5A6C`), which
                // mid-glide is not yet the member. A harness pacing.
                if ours
                    && flow == BattleFlowState::TargetSelect
                    && pick != 0
                    && world.battle.camera.as_ref().is_some_and(|c| c.is_gliding())
                {
                    return None;
                }
                if ours
                    && flow == BattleFlowState::TargetSelect
                    && matches!(cmd.phase, CommandPhase::Targeting { .. })
                {
                    return (!cursor_on_pick(world, flow, pick)).then_some(PadButton::Right);
                }
                match cmd.phase {
                    CommandPhase::RoundPrompt { .. } if flow == BattleFlowState::EscapePrompt => {
                        Some(PadButton::Right)
                    }
                    CommandPhase::RoundPrompt { .. } => Some(PadButton::Left),
                    CommandPhase::Menu { .. }
                        if !ours && cmd.actor < seat && spirit & (1 << cmd.actor) != 0 =>
                    {
                        Some(PadButton::Down)
                    }
                    CommandPhase::Menu { .. } if !ours => Some(PadButton::Left),
                    CommandPhase::Menu { .. } => match flow.window_of_target().unwrap_or(flow) {
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
pub(crate) fn saved_command_string(world: &legaia_engine_core::world::World, seat: u8) -> bool {
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
/// The queue builder keeps a matched normal art's leading arrows as swings
/// and rewrites only its last one into the starter + constant pair (`0x19` /
/// `0x1A`, then the art), so each such pair stands for its art's final arrow
/// and every other byte is an arrow already. A Hyper Art's run collapses
/// whole into `0x1A` and its constant(s) (`legaia_art::tokenize_bank`), so a
/// pair whose constant is in `collapsed` stands for the art's every arrow,
/// and the chained constants behind it for nothing. `combo_of` names an
/// art's arrows; an art it does not know - a Super or Miracle replacement,
/// whose tail the finish rewrote whole - leaves the entry unrecoverable
/// (`None`).
pub fn entered_arrows(
    queue: &[u8; 16],
    collapsed: &[u8],
    combo_of: impl Fn(u8) -> Option<Vec<legaia_art::Command>>,
) -> Option<[u8; 16]> {
    let mut out = [0u8; 16];
    let mut n = 0;
    let mut i = 0;
    while i < queue.len() && queue[i] != 0 {
        match queue[i] {
            b @ 0x0C..=0x0F => {
                *out.get_mut(n)? = b;
                n += 1;
            }
            0x19 | 0x1A => {
                i += 1;
                let art = *queue.get(i)?;
                let combo = combo_of(art)?;
                if collapsed.contains(&art) {
                    for c in &combo {
                        *out.get_mut(n)? = 0x0B + *c as u8;
                        n += 1;
                    }
                    while queue.get(i + 1).is_some_and(|b| collapsed.contains(b)) {
                        i += 1;
                    }
                } else {
                    *out.get_mut(n)? = 0x0B + *combo.last()? as u8;
                    n += 1;
                }
            }
            _ => return None,
        }
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
pub(crate) fn seed_entered_arrows(
    world: &mut legaia_engine_core::world::World,
    seat: u8,
    queue: &[u8; 16],
) {
    use legaia_save::character::AutoCommandBand;
    let roster = world.party_roster_slot(usize::from(seat));
    let character = legaia_engine_battle::battle_arts::character_for_slot(roster as u8);
    let bank = world.art_bank_rows(character);
    let records = legaia_art::bank_view(&bank);
    let collapsed: Vec<u8> = legaia_art::hyper_queue_constants(&records)
        .into_iter()
        .flat_map(|(_, c)| c)
        .collect();
    let Some(arrows) = entered_arrows(queue, &collapsed, |art| {
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

/// Whether a menu surface belongs to one member (the ring, a submenu, the
/// target cursor) rather than to the party (the round prompt, the commit
/// confirm).
pub fn menu_seat_matters(flow: BattleFlowState) -> bool {
    !matches!(
        flow,
        BattleFlowState::TurnPrompt | BattleFlowState::EscapePrompt | BattleFlowState::CommitBegin
    )
}

/// Whether monster row `row` stands in the engine's pool (party count +
/// `row`), so the target cursor can land on it.
/// Whether the Attack cursor rests on the menu drive's `pick` (a retail
/// monster seat), or the drive names none.
fn cursor_on_pick(
    world: &legaia_engine_core::world::World,
    flow: BattleFlowState,
    pick: u8,
) -> bool {
    if flow != BattleFlowState::TargetSelect
        || !(3..8).contains(&pick)
        || !aim_alive(world, pick - 3)
    {
        return true;
    }
    let picker = world.battle.command.as_ref().and_then(|c| c.picker());
    match picker.map(|p| p.state()) {
        Some(legaia_engine_battle::target_picker::PickerState::Cursor {
            row: legaia_engine_battle::target_picker::CursorRow::Enemy,
            slot,
        }) => slot == pick - 3,
        _ => true,
    }
}

pub(crate) fn aim_alive(world: &legaia_engine_core::world::World, row: u8) -> bool {
    let pc = usize::from(world.party.party_count.clamp(1, 3));
    world
        .actors
        .get(pc + usize::from(row))
        .is_some_and(|a| a.active && a.battle.hp > 0)
}

/// Whether the engine's battle-entry sweep
/// ([`legaia_engine_battle_vm::battle_cam_script::BattleCamera::start_entry_sweep`])
/// has run as far as retail's frame-driver counter `entry` reads: a counter
/// inside the sweep (`0x80..=0xC0`) is met on the engine pass whose counter
/// lies nearest it, one past it (`0xFF`) once the engine's sweep is over, and
/// one below it (the fight still loading) at once.
///
/// The counter advances by the frame step a pass (`0x80046FE0..0x80046FF4`),
/// and retail's step varies with load, so its counter need not share the
/// engine's parity: `s5_tetsu_battle` reads `0xAF` on a step of `2`, where the
/// engine's even counter passes `0xAE` and then `0xB0`. Its case-2 glide has
/// run six two-vsync passes there (eye Y and Z `0.34` / `0.32` of the way
/// from where the drift left them, `(10/12)^6 = 0.33`), the pass at `0xAE`;
/// waiting for `0xB0` sampled a seventh. So a pass within half a step short of
/// `entry` meets it, the earlier one on a tie.
pub fn entry_sweep_reached(world: &legaia_engine_core::world::World, entry: u8) -> bool {
    if entry < 0x80 {
        return true;
    }
    world.battle.camera.as_ref().is_some_and(|c| {
        c.entry_sweep_counter()
            .is_none_or(|n| entry != 0xFF && n + u32::from(c.frame_step()) / 2 >= u32::from(entry))
    })
}

/// The engine battle ordinal of retail pool slot `seat`: party slots are
/// shared, but retail's monsters sit at fixed pool slots `3..` whatever the
/// party size while the engine seats them straight after the party.
/// The capture's strike as its state stood: the retail seat, a monster's
/// queue `+0x1DF..` (a party striker's is its own command input, which the
/// drive already replays), the approach terms `ctx[+0x6D2]` / `ctx[+0x6D4]`
/// and the working ATK `+0x158`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StrikeHand {
    pub seat: u8,
    pub queue: Option<[u8; 16]>,
    pub ramps: Option<(i16, i16, u16)>,
}

pub fn engine_seat(seat: u8, party_count: u8) -> u8 {
    if seat >= 3 {
        party_count + (seat - 3)
    } else {
        seat
    }
}

/// The action SM's states ahead of the seed pass `0x0C`: the round-begin
/// `0x00`, the pre-action wait `0x0A` and the menu-queued hold `0x0B`. None of
/// them has picked the next actor into `ctx[+0x13]` yet - the seed pass copies
/// it there from `ctx[+0x274]` (`lbu v0,0x274(v1)` / `sb v0,0x2(s5)` at
/// `0x801E2C50..0x801E2C5C`) - so a capture in one reads the **previous**
/// actor (or the last ring member, at a round's start) in `ctx[+0x13]`.
pub const PRE_SEED_STATES: [u8; 3] = [0x00, 0x0A, 0x0B];

/// Take spell `spell_id` back off a record's list - the inverse of the
/// Done band's prepend (`legaia_engine_battle::magic_xp::learn_spell_prepend`):
/// ids, levels and the parallel XP words above it shift down one. A list
/// without the spell is left alone.
pub(crate) fn unlearn_spell(record: &mut legaia_save::CharacterRecord, spell_id: u8) {
    const SPELL_XP_OFFSET: usize = 0x8;
    let mut list = record.spell_list();
    let count = usize::from(list.count).min(list.ids.len());
    let Some(at) = list.ids[..count].iter().position(|&id| id == spell_id) else {
        return;
    };
    for i in at..count - 1 {
        list.ids[i] = list.ids[i + 1];
        list.levels[i] = list.levels[i + 1];
        let src = SPELL_XP_OFFSET + (i + 1) * 4;
        let dst = SPELL_XP_OFFSET + i * 4;
        record.raw.copy_within(src..src + 4, dst);
    }
    list.ids[count - 1] = 0;
    list.levels[count - 1] = 0;
    let last = SPELL_XP_OFFSET + (count - 1) * 4;
    record.raw[last..last + 4].fill(0);
    list.count -= 1;
    record.set_spell_list(list);
}

#[cfg(test)]
mod tests;

/// The harvest probe's `LEGAIA_TOUGH` writes at a round prompt
/// ([`ActionSteer::tough`]): every standing party seat's HP back to its
/// maximum, with its readout and its pending total. Nothing outside the
/// round prompt.
pub fn tough_round_restore(world: &mut legaia_engine_core::world::World) {
    if world.mode != SceneMode::Battle || world.battle.flow != BattleFlowState::TurnPrompt {
        return;
    }
    let pc = usize::from(world.party.party_count.clamp(1, 3));
    for a in world.actors.iter_mut().take(pc) {
        if a.battle.hp > 0 && a.battle.max_hp > 0 {
            a.battle.hp = a.battle.max_hp;
            a.battle.liveness = a.battle.hp;
            if a.battle.hp_display.is_some() {
                a.battle.hp_display = Some(a.battle.hp);
            }
            a.battle.damage_accum = 0;
        }
    }
}

/// Whether this run is a strike-aim rescue (`LEGAIA_BATTLE_STRIKE_AIM=1`,
/// set by the retail-compare image child's retry once a run missed its gate).
fn strike_aim_rescue() -> bool {
    static AIM: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AIM.get_or_init(|| std::env::var("LEGAIA_BATTLE_STRIKE_AIM").is_ok_and(|v| v.trim() == "1"))
}
