//! The pad walker: one leg toward a goal tile, what it reports when it
//! stalls, draining scripted sequences, and waiting for input to come back.

use super::*;

// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------

/// Everything known about the spot a leg died on.
///
/// The two wall arms are reported separately because they fail for different
/// reasons: `wall` is [`World::field_dir_blocked`] (retail `FUN_801cfe4c`'s
/// static-wall probe, the same one the planner consults), `actor` is
/// [`World::field_actor_dir_blocked`] (the NPC-body arm, which the planner
/// does **not** consult - a walker parked in a doorway is invisible to the
/// route and blocks it anyway).
pub(super) struct StallSite {
    pub(super) tile: (i16, i16),
    pub(super) world: (i16, i16),
    pub(super) goal: (i16, i16),
    /// Next tile the planner wanted, if it still had a route.
    pub(super) want: Option<(i16, i16)>,
    /// Per-direction block, indexed as [`STEPS`]: Z-, X-, Z+, X+.
    pub(super) wall: [bool; 4],
    pub(super) actor: [bool; 4],
    /// How many scripted sequences (dialogue / cutscene timeline) ran during
    /// the leg. Non-zero at a stall means a walk-on trigger *did* fire and
    /// its record *did* execute - the leg failed after the dispatch, not
    /// before it, which is a different defect entirely.
    pub(super) scripted: u32,
    /// Random encounters fought during the leg ([`drain_battle`]). Reported
    /// separately because it splits "the walk never got anywhere" from "the
    /// walk was interrupted N times and still got nowhere" - and because a
    /// leg that fights *nothing* while crossing an overworld with a live
    /// region-encounter table is itself a finding.
    pub(super) fought: u32,
}

/// The tile as the **walk-on dispatch** quantises it: a raw `world >> 7`,
/// not the `(world - 0x40) >> 7` the planner and the region refresh use.
/// The two agree at tile centres and differ by a half-tile band elsewhere,
/// so a stall reported only in planner tiles can sit in a trigger band while
/// naming the tile before it. Retail's own dispatcher uses this form
/// (`FUN_801D1EC4`); see `SceneHost::dispatch_walk_on_trigger`.
pub(super) fn dispatch_tile(x: i16, z: i16) -> (i32, i32) {
    (i32::from(x) >> 7, i32::from(z) >> 7)
}

impl StallSite {
    pub(super) fn dirs(flags: [bool; 4]) -> String {
        const NAME: [&str; 4] = ["Z-", "X-", "Z+", "X+"];
        let hit: Vec<_> = NAME
            .iter()
            .zip(flags)
            .filter_map(|(n, b)| b.then_some(*n))
            .collect();
        if hit.is_empty() {
            "none".to_string()
        } else {
            hit.join(",")
        }
    }
}

/// Outcome of driving one leg of the ladder.
pub(super) enum Leg {
    /// A scene transition fired mid-leg - the leg's real success shape when
    /// the goal is a door tile.
    Transitioned(String),
    /// Standing on the goal tile with no transition.
    Reached,
    /// Control never came back: the opening choreography or a dialogue box
    /// held input for the whole budget. Carries the world position it
    /// happened at, because a scripted sequence that never ends is a
    /// **record**, and the tile is what names which one - a lock at the leg's
    /// first frame and a lock seven tiles in are different defects.
    ///
    /// The world pair, not the tile: the trigger that spawned the sequence was
    /// matched in the dispatch frame (`world >> 7`) and the two frames differ
    /// by a half tile, so a lock reported only in planner tiles names the tile
    /// *before* the band it is standing in. This one did - `(56, 29)` for a
    /// record whose band starts at `(56, 30)`.
    InputLocked { at: (i16, i16) },
    /// The walkability grid offers no route from here to the goal.
    NoPath { at: (i16, i16), goal: (i16, i16) },
    /// Movement stopped closing distance. Carries what the engine itself
    /// thinks of the spot, because "the player stopped here" and "the engine
    /// says every useful direction is a wall" are different findings and the
    /// bare tile cannot tell them apart.
    Stalled(Box<StallSite>),
    /// Still moving, but out of frames.
    Timeout { at: (i16, i16), goal: (i16, i16) },
    /// A random encounter never returned control to the walking mode.
    BattleUnresolved {
        at: (i16, i16),
        ended_in: Option<SceneMode>,
    },
    /// A random encounter killed the whole party. This is a leg outcome of
    /// its own because it must never be conflated with either neighbour: it
    /// is not `BattleUnresolved` (the battle resolved, against us) and it
    /// must never score as survival - the pre-hold engine returned a wiped
    /// party to the field and this ladder walked three rungs with dead
    /// heroes.
    PartyWiped { heading_for: (i16, i16) },
}

/// Rendered into the printed table. Written by hand rather than derived so
/// the failure shapes name their tile in the line a reader actually sees -
/// a stall's whole value is the coordinate it died on.
impl std::fmt::Display for Leg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Leg::Transitioned(name) => write!(f, "entered {name}"),
            Leg::Reached => write!(f, "reached the goal tile"),
            Leg::InputLocked { at } => write!(
                f,
                "control never released at world {at:?} tile {:?} (dispatch tile {:?})",
                tile_of(at.0, at.1),
                dispatch_tile(at.0, at.1)
            ),
            Leg::NoPath { at, goal } => {
                write!(f, "no walkable route {at:?} -> {goal:?}")
            }
            Leg::Stalled(s) => write!(
                f,
                "STALLED at tile {:?} world {:?} (dispatch tile {:?}) heading for {:?} \
                 (next waypoint {:?}); walls {}; actors {}",
                s.tile,
                s.world,
                dispatch_tile(s.world.0, s.world.1),
                s.goal,
                s.want,
                StallSite::dirs(s.wall),
                StallSite::dirs(s.actor),
            )
            .and_then(|()| {
                if s.scripted > 0 {
                    write!(f, "; {} scripted sequence(s) ran", s.scripted)
                } else {
                    Ok(())
                }
            })
            .and_then(|()| {
                if s.fought > 0 {
                    write!(f, "; {} random encounter(s) fought", s.fought)
                } else {
                    Ok(())
                }
            }),
            Leg::Timeout { at, goal } => {
                write!(f, "out of frames at tile {at:?} heading for {goal:?}")
            }
            Leg::BattleUnresolved { at, ended_in } => write!(
                f,
                "a random encounter near tile {at:?} never returned control \
                 (mode {ended_in:?})"
            ),
            Leg::PartyWiped { heading_for } => write!(
                f,
                "PARTY WIPED in a random encounter (game-over hold) heading for {heading_for:?}"
            ),
        }
    }
}

pub(super) fn player_world(host: &SceneHost) -> (i16, i16) {
    let slot = host.world.player_actor_slot.expect("player actor") as usize;
    let ms = &host.world.actors[slot].move_state;
    (ms.world_x, ms.world_z)
}

/// Outcome of letting a scripted sequence run to completion.
pub(super) enum Drain {
    /// The sequence performed a scene change - the leg's success condition.
    Entered(String),
    /// It finished and handed input back.
    Released,
    /// It never handed input back inside the budget.
    Locked,
}

/// Tick with a neutral pad while a dialogue or cutscene owns input, surfacing
/// a scene change if the sequence performs one.
pub(super) fn drain_scripted(host: &mut SceneHost) -> Drain {
    for f in 0..INPUT_RELEASE_BUDGET {
        // A scene change parked behind the streaming actor's countdown is the
        // sequence's own exit, still in flight - not a release.
        if host.world.scene_transition_hold.is_none()
            && !host.world.cutscene_timeline_active()
            && !host.world.dialogue_owns_input()
        {
            return Drain::Released;
        }
        // Page through it the way a player does. A neutral pad is enough for a
        // sequence that runs on its own clock (`town01`'s opening, the arrival
        // beats), but it is **not** enough for one that pages: `keikoku`'s
        // partition-2 record `7` - the shared band across all four chambers'
        // inner doorways, guarded by the one-shot `SysFlag 0x2BB` - is a story
        // cutscene with narration pages, and a page waits on a confirm. Held
        // neutral it never advances and the whole leg reads as an engine hang
        // at the tile the band starts on.
        //
        // Pulsed, not held: the advance is edge-triggered, so a held Cross
        // reads as one press and pages once. The duty cycle is
        // press-2-release-14, which is a human mash rather than a per-frame
        // strobe - fast enough to clear a long sequence inside the budget,
        // slow enough that a page that opens an option picker is not
        // double-answered by the same press.
        host.world.set_pad(if f % 16 < 2 {
            PadButton::Cross.mask()
        } else {
            0
        });
        match host.tick() {
            Ok(SceneTickEvent::SceneEntered { name }) => return Drain::Entered(name),
            Ok(_) => {}
            Err(_) => return Drain::Locked,
        }
    }
    Drain::Locked
}

/// Tick with a neutral pad until the world hands control back, so a leg does
/// not spend its budget shoving against a cutscene lock.
pub(super) fn wait_for_input(host: &mut SceneHost) -> bool {
    held_frames_before_input(host).is_some()
}

/// [`wait_for_input`] with the answer nobody was reading: **how many frames
/// anything actually held input**, or `None` if it never let go.
///
/// `Some(0)` is not "control was released promptly", it is "no scripted
/// sequence ever ran". The two are indistinguishable to a caller that only
/// asks whether input is free now, and rung 1 asked exactly that - so a scene
/// whose opening choreography does not play at all scored the rung labelled
/// "cold boot hands control to the player". `town01` under
/// `SceneHost::enter_field_scene(name, 0)` returns `Some(0)`: no timeline
/// frame, no dialogue frame, not during the wait and not across a further
/// 1200 idle ticks. So rung 1 is evidence that the scene loads into a
/// controllable Field state, and is **not** evidence that the opening plays.
/// The count is printed in the rung's own row so the distinction is visible
/// rather than inferred.
///
/// **Zero is the right answer here**, and that is now settled rather than
/// suspected. Rim Elm's opening is not a property of loading `town01`; it is
/// partition-2 record 3, whose header gate is "system flag `0x225` is clear",
/// and the engine installs it only on the New Game hand-off
/// (`World::cutscene.entering_town01_opening` / `opening_chain_active`). This ladder
/// walks a *route*, so it enters Rim Elm the way a returning player does, and
/// a returning player gets no opening - retail's own scene-entry script
/// (`town01` P1[0]) likewise reads `0x225` and skips its first-visit arms once
/// it is set. The opening's own ladder is the browser `play_compose` rung 2,
/// which enters through the hand-off and asserts the timeline played.
pub(super) fn held_frames_before_input(host: &mut SceneHost) -> Option<u32> {
    for f in 0..INPUT_RELEASE_BUDGET {
        if !host.world.cutscene_timeline_active() && !host.world.dialogue_owns_input() {
            return Some(f);
        }
        host.world.set_pad(0);
        host.tick().ok()?;
    }
    None
}

/// What a leg did, independently of whether it ended where it was aimed.
///
/// A pass/fail outcome cannot distinguish "the walk never started" from "the
/// walk crossed the continent and was turned back at the last ridge", and on a
/// 60-tile leg that is the whole of the interesting range. These are what the
/// ladder scores the crossing on.
#[derive(Default)]
pub(super) struct LegStats {
    /// Random encounters entered and survived ([`drain_battle`]).
    pub(super) fought: u32,
    /// Furthest Manhattan tile distance from the leg's start the player ever
    /// reached - not the final position, which a wall-slide back can shorten.
    pub(super) reach: i32,
}

/// Drive the player to `goal` using only the pad. Returns the leg outcome.
pub(super) fn walk_to(
    host: &mut SceneHost,
    goal: (i16, i16),
    avoid: &HashSet<(i32, i32)>,
    stats: &mut LegStats,
    fight: &FightPolicy,
) -> Leg {
    if !wait_for_input(host) {
        return Leg::InputLocked {
            at: player_world(host),
        };
    }

    let start_w = player_world(host);
    let start = tile_of(start_w.0, start_w.1);
    if plan_path(host, cell_of(start_w.0, start_w.1), goal, avoid).is_none() {
        return Leg::NoPath { at: start, goal };
    }

    let dist = |a: (i16, i16), b: (i16, i16)| ((a.0 - b.0).abs() + (a.1 - b.1).abs()) as i32;
    let mut best = dist(start, goal);
    let mut since_progress = 0u32;
    let mut scripted = 0u32;
    // The mode this leg walks in. A leg never changes it: leaving it means a
    // battle (drained below) or a scene change (the leg's success).
    let walking_mode = host.world.mode;

    // The plan is always rooted at the tile the player is standing on, and
    // replanned the moment they leave it. Following a fixed waypoint list
    // does not survive contact with the collision step: `advance_with_collision`
    // wall-slides, so the player routinely arrives on a tile that is not the
    // next waypoint, and a follower that only pops on an exact match then
    // steers back at a waypoint behind them and oscillates in place.
    let mut planned_from = cell_of(start_w.0, start_w.1);
    let mut path = plan_path(host, planned_from, goal, avoid).unwrap_or_default();

    for _ in 0..LEG_FRAME_BUDGET {
        // A random encounter took the frame. `player_world` is meaningless
        // until it hands back (see `drain_battle`), so sit it out *before*
        // sampling anything - and charge none of it to the leg.
        if host.world.mode == SceneMode::Battle {
            stats.fought += 1;
            match drain_battle(host, walking_mode, fight) {
                Fight::Survived => {
                    // Fighting is not stalling: the walk resumes from where it
                    // was interrupted, so give it a fresh stall window.
                    since_progress = 0;
                    continue;
                }
                Fight::Stuck => {
                    return Leg::BattleUnresolved {
                        at: goal,
                        ended_in: None,
                    };
                }
                Fight::Diverted(mode) => {
                    return Leg::BattleUnresolved {
                        at: goal,
                        ended_in: Some(mode),
                    };
                }
                Fight::Wiped => {
                    return Leg::PartyWiped { heading_for: goal };
                }
            }
        }

        let (wx, wz) = player_world(host);
        let here = tile_of(wx, wz);
        stats.reach = stats.reach.max(dist(here, start));
        if here == goal {
            return Leg::Reached;
        }

        let cell = cell_of(wx, wz);
        if cell != planned_from {
            // A plan that cannot improve is not a dead end: the last stretch
            // into a scene exit is *supposed* to run out of standable nodes,
            // because the door tile reads as a wall. Fall through with an
            // empty path and press straight at the goal - that press is what
            // crosses the walk-on band. The stall detector below still bounds
            // it, so a genuine dead end is reported rather than looped on.
            path = plan_path(host, cell, goal, avoid).unwrap_or_default();
            planned_from = cell;
        }
        let (tx, tz) = match path.first() {
            Some(&c) => cell_center(c),
            None => tile_center(goal),
        };
        let pad = pad_for_step(host, (tx - wx).signum(), (tz - wz).signum());
        // Per-frame follower trace (`LEGAIA_CPR_TRACE=1`). Separate from
        // `LEGAIA_CPR_DEBUG` because it is thousands of lines per leg, and
        // worth having: a stall report names the tile the leg *ended* on,
        // which is silent about a mid-leg teleport. The Ravine rung's
        // scripted-relocation defect showed up here as a single frame
        // stepping from the arrival tile to the `(127,127)` hide box, and
        // could not be seen any other way.
        if std::env::var_os("LEGAIA_CPR_TRACE").is_some() {
            eprintln!(
                "[trace] w=({wx},{wz}) tile={here:?} goal={goal:?} want={:?} pad={pad:04x} az={} scripted={scripted} tl={} dlg={}",
                path.first().map(|&c| tile_of_cell(c)),
                host.world.locomotion.camera_azimuth,
                host.world.cutscene_timeline_active(),
                host.world.dialogue_owns_input(),
            );
        }
        host.world.set_pad(pad);
        match host.tick() {
            Ok(SceneTickEvent::SceneEntered { name }) => return Leg::Transitioned(name),
            Ok(_) => {}
            Err(_) => return Leg::Timeout { at: here, goal },
        }

        // The tick that just ran may have rolled an encounter. Hand the frame
        // straight back to the loop head rather than measuring progress off a
        // battle-arena coordinate.
        if host.world.mode == SceneMode::Battle {
            continue;
        }

        // A dialogue or cutscene that opened mid-leg owns input now; let it
        // finish rather than burning frames against it. Crucially it may also
        // be the *success* path - a walk-on trigger's partition-2 record
        // installs as a timeline and that timeline is what performs the scene
        // change - so the drain has to surface `SceneEntered` rather than
        // swallow it. (It did swallow it, which read as a stall at the gate.)
        if host.world.cutscene_timeline_active()
            || host.world.dialogue_owns_input()
            || host.world.scene_transition_hold.is_some()
        {
            scripted += 1;
            match drain_scripted(host) {
                Drain::Entered(name) => return Leg::Transitioned(name),
                Drain::Released => {}
                Drain::Locked => {
                    let (x, z) = player_world(host);
                    if std::env::var_os("LEGAIA_CPR_DEBUG").is_some() {
                        eprintln!(
                            "[dbg] input lock at world ({x},{z}) tile {:?} dispatch {:?}: \
                             timeline {} dialog {} inline {}",
                            tile_of(x, z),
                            dispatch_tile(x, z),
                            host.world.cutscene_timeline_active(),
                            host.world.dialog.current.is_some(),
                            host.world.dialog.inline.is_some(),
                        );
                    }
                    return Leg::InputLocked { at: (x, z) };
                }
            }
        }

        // Progress is measured against the best distance ever achieved, so
        // wall-sliding sideways for a while is tolerated but circling is not.
        // Replanning happens above on every tile change, so reaching
        // STALL_FRAMES here means the route is being followed and still not
        // closing - that is a stall, not a stale plan.
        let now = tile_of(player_world(host).0, player_world(host).1);
        let d = dist(now, goal);
        if d < best {
            best = d;
            since_progress = 0;
        } else {
            since_progress += 1;
            if since_progress >= STALL_FRAMES {
                let (sx, sz) = player_world(host);
                if std::env::var_os("LEGAIA_CPR_DEBUG").is_some() {
                    for c in &host.world.props.colliders {
                        if (c.center.0 - i32::from(sx)).abs() < 400
                            && (c.center.1 - i32::from(sz)).abs() < 400
                        {
                            eprintln!("[dbg] near prop {c:?}");
                        }
                    }
                    eprintln!(
                        "[dbg] npc positions near: {:?}",
                        host.world
                            .npcs
                            .positions
                            .values()
                            .filter(|&&(ax, az)| (i32::from(ax) - i32::from(sx)).abs() < 400
                                && (i32::from(az) - i32::from(sz)).abs() < 400)
                            .collect::<Vec<_>>()
                    );
                }
                let probe = |f: &dyn Fn(usize) -> bool| [f(0), f(1), f(2), f(3)];
                return Leg::Stalled(Box::new(StallSite {
                    tile: now,
                    world: (sx, sz),
                    goal,
                    want: plan_path(host, cell_of(sx, sz), goal, avoid)
                        .and_then(|p| p.first().copied())
                        .map(tile_of_cell),
                    wall: probe(&|d| host.world.field_dir_blocked(sx, sz, d)),
                    actor: probe(&|d| host.world.field_actor_dir_blocked(sx, sz, d)),
                    scripted,
                    fought: stats.fought,
                }));
            }
        }
    }
    let at = tile_of(player_world(host).0, player_world(host).1);
    Leg::Timeout { at, goal }
}
