//! Talking to an actor and examining a prop, seated and by pad.

use super::*;

/// Talk to the NPC in placement `slot` as a player does: stand on a tile
/// next to it, face it so the retail interact probe (64 units ahead, the
/// NPC's 72-unit box) lands on it, press Cross, and page the conversation.
/// A talk record often branches on where the player stands (a `0x4D`
/// box test on the player), so each side is tried in turn until one gains a
/// flag. The player goes back where it stood afterwards, so the next press
/// does not re-open the same talk.
pub(super) fn talk_to(session: &mut NativeSession, slot: u8) -> Run {
    if let Some(r) = talk_in_entry_box(session, slot) {
        return r;
    }
    if pad_hand() {
        return pad_talk_to(session, slot);
    }
    let Some(&(nx, nz)) = session.host.world.npcs.positions.get(&slot) else {
        return Run::Released;
    };
    interact_at(
        session,
        (nx, nz),
        &|w: &legaia_engine_core::world::World| w.field_interact_probe_slot() == Some(slot),
        false,
    )
}

/// The tiles of the player box a talk record tests before it says anything:
/// a player-channel `BBoxTest` (`CD F8 x0 z0 x1 z1`) among the first
/// instructions at the cursor the talk starts from. `rayman` P1[44..=46],
/// the three on the east ledge, open on `CD F8 6B 46 6B 50`: column 107,
/// rows 70..=80, and from anywhere else the record skips to its closing
/// `0x21`. Empty for a record that tests no box there.
pub(super) fn talk_entry_box(session: &NativeSession, slot: u8) -> Vec<(u8, u8)> {
    let Some(rec) = session.host.world.npcs.dialog_prologue.get(&slot) else {
        return Vec::new();
    };
    entry_box_tiles(entry_box(&rec.body, rec.entry_pc))
}

/// The player box test a script reaches first from `pc`, its jumps
/// followed, within its first few instructions. A talk that ended on the
/// record's `0x21` resumes one past it, and the record jumps back from
/// there to the test (`rayman` P1[44]: `21`, `26 02 00`, `26 CC F4`).
pub(super) fn entry_box(body: &[u8], mut pc: usize) -> Option<(u8, u8, u8, u8)> {
    use legaia_scene_formats::field_disasm::{InsnInfo, decode};
    for _ in 0..6 {
        let insn = decode(body, pc).ok()?;
        match insn.info {
            InsnInfo::BBoxTest {
                x_min,
                z_min,
                x_max,
                z_max,
                ..
            } if insn.extended == Some(0xF8) => return Some((x_min, z_min, x_max, z_max)),
            InsnInfo::JmpRel { target, .. } => pc = target,
            InsnInfo::TextSegment { .. } | InsnInfo::Byte { .. } => return None,
            _ if insn.size == 0 => return None,
            _ => pc += insn.size,
        }
    }
    None
}

/// The tiles of box `(x0, z0, x1, z1)`, inclusive; none for no box or one
/// whose corners are out of order.
pub(super) fn entry_box_tiles(bx: Option<(u8, u8, u8, u8)>) -> Vec<(u8, u8)> {
    let Some((x0, z0, x1, z1)) = bx else {
        return Vec::new();
    };
    (z0..=z1)
        .flat_map(|z| (x0..=x1).map(move |x| (x, z)))
        .collect()
}

/// The tiles of `tiles` a talk with the actor on `npc` can be held from,
/// nearest first: those within two tiles of it on both axes. The interact
/// probe reaches 64 units ahead into a 72-unit box, so a stand further off
/// reaches nobody.
pub(super) fn box_stands(tiles: &[(u8, u8)], npc: (i32, i32)) -> Vec<(u8, u8)> {
    let far = |t: &(u8, u8)| {
        let (dx, dz) = (
            (i32::from(t.0) - npc.0).abs(),
            (i32::from(t.1) - npc.1).abs(),
        );
        (dx.max(dz), dx + dz, t.1, t.0)
    };
    let mut near: Vec<(u8, u8)> = tiles.iter().copied().filter(|t| far(t).0 <= 2).collect();
    near.sort_by_key(far);
    near
}

/// Talk to `slot` from inside the player box its record tests first
/// ([`talk_entry_box`]), as a player has to: retail's dialog SM runs the
/// record from its cursor, and a talk begun outside the box ends on the
/// record's `0x21` with nothing said. `None` when the record tests no box,
/// or no stand inside it reaches the actor - the caller's usual approach
/// then runs.
///
/// The seated hand is seated on the stand; the pad hand walks there. Both
/// then lean in without leaving the tile and press Cross
/// ([`talk_from_box`]).
fn talk_in_entry_box(session: &mut NativeSession, slot: u8) -> Option<Run> {
    if session.host.world.mode != SceneMode::Field {
        return None;
    }
    let tiles = talk_entry_box(session, slot);
    if tiles.is_empty() {
        return None;
    }
    let &(nx, nz) = session.host.world.npcs.positions.get(&slot)?;
    let claimed = claimed_tiles(session);
    if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
        eprintln!(
            "      [box-talk] P1[{slot}] at {:?}: box {:?}..{:?}, stands {:?}",
            (nx, nz),
            tiles.first(),
            tiles.last(),
            box_stands(&tiles, dispatch_tile(nx, nz))
        );
    }
    let stands: Vec<(u8, u8)> = box_stands(&tiles, dispatch_tile(nx, nz))
        .into_iter()
        .filter(|t| !claimed.contains(t))
        .collect();
    for &t in &stands {
        let back = player_xz(session);
        if pad_hand() {
            let goal = (i16::from(t.0), i16::from(t.1));
            match pad_walk(session, goal, &pad_avoid(session, Some(goal)), 0) {
                Ok(Walk::Entered(s)) => return Some(Run::Entered(s)),
                Ok(Walk::Arrived) => {}
                Err(_) => continue,
            }
            if let Some(s) = center_on_box_tile(session, t) {
                return Some(Run::Entered(s));
            }
        } else {
            let at = (i16::from(t.0) * 128 + 96, i16::from(t.1) * 128 + 96);
            if session.host.world.field_tile_is_wall(at.0, at.1) {
                continue;
            }
            session.host_pad(0);
            restore_player_xz(session, at);
        }
        let others: Vec<(u8, u8)> = if pad_hand() {
            stands.iter().copied().filter(|&o| o != t).collect()
        } else {
            Vec::new()
        };
        let r = talk_from_box(session, slot, t, &others);
        if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
            eprintln!(
                "      [box-talk] P1[{slot}] at {:?} from box tile {t:?} (stood {:?}) -> {r:?}",
                (nx, nz),
                player_xz(session)
            );
        }
        if !pad_hand() && session.host.world.mode == SceneMode::Field {
            restore_player_xz(session, back);
        }
        if r.is_some() {
            return r;
        }
    }
    None
}

/// Examine the interact-gated prop anchored at `anchor` (the cupboard class,
/// retail `FUN_801CFC40` result bit `1`): the same stand-beside, face and
/// press-Cross approach as a talk, aimed so the prop arm of the facing probe
/// (`FUN_801CF9F4`) lands on its contact box.
///
/// `walked`: stand only on seats the walk reaches from where the player
/// stands ([`interact_seats`]) - the continuous hand's gate stones.
pub(super) fn examine_prop(
    session: &mut NativeSession,
    anchor: (u8, u8),
    at: (i32, i32),
    walked: bool,
) -> Run {
    let pos = (at.0 as i16, at.1 as i16);
    interact_at(
        session,
        pos,
        &|w: &legaia_engine_core::world::World| {
            w.field_interact_probe_slot().is_none()
                && w.field_interact_prop_anchor() == Some(anchor)
        },
        walked,
    )
}

/// The approach [`talk_to`] and [`examine_prop`] share: from each tile
/// beside `(nx, nz)`, face the sector where `hits` holds, press Cross and
/// page what opens.
pub(super) fn interact_at(
    session: &mut NativeSession,
    (nx, nz): (i16, i16),
    hits: &dyn Fn(&legaia_engine_core::world::World) -> bool,
    walked: bool,
) -> Run {
    let (bx, bz) = player_xz(session);
    let mut last = Run::Released;
    // Whether the last seat tried was a placement, which is given back.
    let mut placed_last = !pad_props();
    // The pad hand walks to a seat and leans in until the probe lands, as a
    // player does; only when its walk reaches none of them does it fall back
    // to placing the player ([`shortcut`] `examine_seat`). Both passes keep
    // the seated hand's order of sides: a record that answers by where the
    // player stands answers alike (`stone`'s statues turn to face the side
    // they are examined from, and the way to `tunnela` opens on one set of
    // facings; tried nearest side first, the dial spun past it).
    let seats = interact_seats(session, (nx, nz), walked);
    let attempts: Vec<((i16, i16, bool), bool)> = if pad_props() {
        seats
            .iter()
            .map(|&s| (s, true))
            .chain(seats.iter().map(|&s| (s, false)))
            .collect()
    } else {
        seats.iter().map(|&s| (s, false)).collect()
    };
    let mut pad_reached = false;
    // A seat left out for its distance ([`prop_seat_reach`]).
    let mut far = false;
    // Why each walked seat fell short, for `LEGAIA_FGL_SHORTCUT_SITES=1`.
    let mut whys: Vec<String> = Vec::new();
    // A seat walk that a script on the way carried off refuses the
    // placement, unless some seat was only far
    // ([`unwalkable_placement_refused`]).
    // Whether a script on the way carried some seat's walk off.
    let mut carried = false;
    for ((sx, sz, extra), try_pad) in attempts {
        if !try_pad && pad_hand() && (pad_reached || shortcuts_refused() || !walking(session)) {
            break;
        }
        if !try_pad && pad_hand() && carried && !far && unwalkable_placement_refused() {
            refuse_placement(format_args!(
                "examine in {} for {:?}: {}",
                scene_name(session),
                (nx, nz),
                whys.join("; ")
            ));
            break;
        }
        session.host_pad(0);
        if try_pad {
            match prop_seat_reach(session, (sx, sz)) {
                SeatReach::Walk => {}
                SeatReach::Far => {
                    far = true;
                    whys.push(format!("{:?} far", (sx, sz)));
                    continue;
                }
                SeatReach::Unreachable => {
                    whys.push(format!("{:?} unreachable", (sx, sz)));
                    continue;
                }
            }
        }
        let by_pad = if try_pad {
            match pad_reach(session, (sx, sz), (nx, nz), hits) {
                Reach::Entered(s) => return Run::Entered(s),
                Reach::Wiped(e) => return Run::Battle(e),
                Reach::Facing => {
                    pad_reached = true;
                    true
                }
                Reach::Short(why) => {
                    carried |= walk_carried_off(&why);
                    whys.push(format!(
                        "{:?} {}",
                        (sx, sz),
                        why.chars().take(90).collect::<String>()
                    ));
                    if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                        eprintln!(
                            "      [examine] seat {:?} for {:?}: {why}",
                            (sx, sz),
                            (nx, nz)
                        );
                    }
                    continue;
                }
            }
        } else {
            false
        };
        placed_last = !by_pad;
        let mut facing = by_pad.then_some(0u8);
        if !by_pad {
            session.host.world.seat_player_at_tile(sx as u8, sz as u8);
            shortcut(if far { "examine_far" } else { "examine_seat" });
            if shortcut_sites_on() {
                eprintln!(
                    "      [placed] examine in {} for {:?} on {:?} from {:?}: {}",
                    scene_name(session),
                    (nx, nz),
                    (sx, sz),
                    (bx, bz),
                    whys.join("; ")
                );
            }
            // The one compass sector whose probe point lands in the NPC's box.
            facing = (0..8u8).find(|&d| {
                session.host.world.face_player_sector(d);
                hits(&session.host.world)
            });
        }
        // Out of the probe's reach from a seat beside the target's own tile,
        // the continuous hand walks up against it first, as a player does:
        // the `korb3` gate stones' boxes end exactly where the probe from
        // the corridor's centre line reaches. It stops as soon as the target
        // is in reach: a party with a faster walk otherwise slides along the
        // wall and out of the probe's reach again before the step ends.
        if facing.is_none() && extra {
            let (px, pz) = player_xz(session);
            let pad = pad_for_step(session, (nx - px).signum(), (nz - pz).signum());
            for _ in 0..24 {
                let at = player_xz(session);
                session.host_pad(pad);
                if session.tick().is_err() {
                    break;
                }
                let reached = (0..8u8).any(|d| {
                    session.host.world.face_player_sector(d);
                    hits(&session.host.world)
                });
                if reached || player_xz(session) == at {
                    break;
                }
            }
            session.host_pad(0);
            let _ = session.tick();
            facing = (0..8u8).find(|&d| {
                session.host.world.face_player_sector(d);
                hits(&session.host.world)
            });
        }
        let Some(_) = facing else {
            continue;
        };
        let before = flags_of_world(session);
        let seat_xz = player_xz(session);
        if !by_pad {
            shortcut("examine_face");
        }
        match press_edge(session, PadButton::Cross.mask()) {
            Ok(Some(name)) => return Run::Entered(name),
            Ok(None) => {}
            Err(e) => return Run::Error(e),
        }
        // Page the conversation to its end, then step back off the NPC
        // before anything else runs: a player facing the NPC with the
        // confirm button pulsing re-opens the same talk the frame it ends,
        // and that restart is not what retail runs next - a record the
        // talk spawned is (`town01` P1[40] spawns the P2[25] mist night).
        let mut ended = false;
        for f in 0..DEEP_EXIT_TICKS {
            if session.host.world.mode == SceneMode::Battle {
                if let Some(r) = drain_battle(session) {
                    return r;
                }
                continue;
            }
            let w = &session.host.world;
            if f >= 2 && w.dialog.inline.is_none() && !w.dialogue_owns_input() {
                ended = true;
                break;
            }
            let pad = script_pad(session, f);
            session.host_pad(pad);
            match session.tick() {
                Ok(SceneTickEvent::SceneEntered { name }) => return Run::Entered(name),
                Ok(_) => {}
                Err(e) => return Run::Error(format!("{e:#}")),
            }
        }
        if !ended {
            return Run::Parked(format!("{} at {}", holder(session), park_site(session)));
        }
        // A talk that hands the frame to a scripted beat - a record it
        // spawned, queued or already running - leaves the player to that
        // beat: `station`'s ticket seller spawns P2[19], whose first act is
        // to walk the player from the counter (`C7 F8`) to the cart.
        // Teleporting the player back to where it stood before the talk
        // strands that walk across the map.
        let w = &session.host.world;
        let beat_follows = !w.field_vm.pending_record_spawns.is_empty()
            || !w.field_vm.helper_contexts.is_empty()
            || w.cutscene.timeline.is_some();
        // The pad hand walked here and walks on from here: only a placed
        // seat is given back.
        if walking(session) && !beat_follows && !by_pad {
            restore_player_xz(session, (bx, bz));
        }
        let r = run_while_moving(session, DEEP_EXIT_TICKS);
        // A beat that followed the talk and left the player on the seat
        // moved nobody: the seat is given back. The seat beside an object
        // can be a pocket the object itself walls in (`dolk` P0[7], the
        // possessed sleeper at (110, 117), examined from (109, 117); its
        // talk walks another actor, which counts as a beat following).
        if beat_follows
            && !by_pad
            && matches!(r, Run::Released)
            && walking(session)
            && player_xz(session) == seat_xz
        {
            restore_player_xz(session, (bx, bz));
        }
        if !matches!(r, Run::Released) || !walking(session) {
            return r;
        }
        // A merchant's talk gains no flag; the visit itself is the gain.
        let gained = flags_of_world(session) != before || take_shop_visited();
        last = r;
        if gained {
            break;
        }
    }
    if walking(session) && placed_last {
        restore_player_xz(session, (bx, bz));
    }
    last
}

/// Cells of planned path the pad hand walks to a prop's seat in a scene
/// that rolls encounters (sixteen tiles).
const PROP_WALK_REACH: usize = 64;

/// How the pad hand's walk stands to a prop's seat ([`prop_seat_reach`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum SeatReach {
    /// The hand walks there.
    Walk,
    /// The seat is in the walker's component but beyond [`PROP_WALK_REACH`]
    /// of planned path in a scene that rolls encounters.
    Far,
    /// No plan from where the player stands gets within
    /// [`DOOR_APPROACH_SLACK`] tiles of the seat: it lies in a walk
    /// component the player's does not join, so a placement there is a
    /// teleport no player makes.
    Unreachable,
}

/// Whether the pad hand walks to seat `seat`. The plan has to end on the
/// seat, or within the slack a pad walk presses on through
/// ([`DOOR_APPROACH_SLACK`]), as [`pad_walk`] tests before it sets out: a
/// plan to a goal outside the walker's component comes back as the route to
/// its nearest cell ([`plan_path`]), and a short such route once read as a
/// seat in reach. Where an encounter rolls, a seat
/// further than [`PROP_WALK_REACH`] of planned path is placed and counted
/// (`examine_far` / `touch_far`), like the walk to a bed ([`pad_rest`]): the
/// beats pass plays props the retail player left alone - `dolk`'s Lightning
/// Gate rooms raise flags no anchor carries - and the way to them across a
/// castle whose every region rolls wiped a lone member on twelve reloads
/// running.
pub(super) fn prop_seat_reach(session: &NativeSession, seat: (i16, i16)) -> SeatReach {
    let (px, pz) = player_xz(session);
    // The plan [`pad_walk`] makes before it sets out, and its test of it.
    let mut doors = hazards(session, "");
    doors.remove(&(i32::from(seat.0), i32::from(seat.1)));
    let plan = plan_around(
        session,
        cell_of(px, pz),
        seat,
        &pad_avoid(session, Some(seat)),
        &doors,
    );
    let miss = plan.last().map_or(0, |&c| {
        let (x, z) = cell_center(c);
        let (tx, tz) = dispatch_tile(x, z);
        (tx - i32::from(seat.0)).abs() + (tz - i32::from(seat.1)).abs()
    });
    if miss > DOOR_APPROACH_SLACK {
        SeatReach::Unreachable
    } else if session.host.world.scene_can_roll_encounters() && plan.len() > PROP_WALK_REACH {
        SeatReach::Far
    } else {
        SeatReach::Walk
    }
}

/// How a pad approach to an interaction seat ended ([`pad_reach`]).
pub(super) enum Reach {
    /// A scene change landed on the way.
    Entered(String),
    /// The player stands on the seat and the probe lands on the target.
    Facing,
    /// The walk did not reach the seat, or no lean from it lands the probe.
    Short(String),
    /// A fight on the walk wiped the party.
    Wiped(String),
}

/// Walk to seat tile `seat` with the pad, then lean toward `target` until
/// `hits` holds: a press toward a solid body turns the player to face it
/// and the collision stops the step, which is how a player lines up an
/// examine. The pad hand's half of [`interact_at`]; the seated hand places
/// the player and sets the facing instead.
pub(super) fn pad_reach(
    session: &mut NativeSession,
    seat: (i16, i16),
    target: (i16, i16),
    hits: &dyn Fn(&legaia_engine_core::world::World) -> bool,
) -> Reach {
    match pad_walk(session, seat, &pad_avoid(session, Some(seat)), 0) {
        Ok(Walk::Entered(s)) => return Reach::Entered(s),
        Ok(Walk::Arrived) => {}
        Err(e) if e.contains("party wiped") => return Reach::Wiped(e),
        Err(e) => return Reach::Short(e.chars().take(160).collect()),
    }
    if !hits(&session.host.world)
        && let Some(s) = pad_lean(session, |_| Some(target), 48, |s| hits(&s.host.world))
    {
        return Reach::Entered(s);
    }
    session.host_pad(0);
    if walking(session) && hits(&session.host.world) {
        Reach::Facing
    } else {
        Reach::Short(format!(
            "no lean from {:?} lands the probe on {target:?}",
            player_xz(session)
        ))
    }
}

/// The tiles [`interact_at`] stands on to reach the target at `(nx, nz)`:
/// each side of the seat grid's tile for it, then (continuous hand) each
/// side of the tile its centre lies on, when that differs - `korb3` P0[3],
/// the gate stone at (46, 31), is examined from the corridor at (47, 31),
/// and two sides of (45, 30) are walls whose seat faces the stone and posts
/// no examine. A tile another body claims is left out.
///
/// With `walked`, only the seats the walk can reach from where the player
/// stands. `korb3`'s stones each open a gate across the maze; a stone
/// examined from a seat the party cannot walk to opened gates in an order no
/// player could, and the band behind the first gate shut it again in the
/// party's face.
pub(super) fn interact_seats(
    session: &NativeSession,
    (nx, nz): (i16, i16),
    walked: bool,
) -> Vec<(i16, i16, bool)> {
    let (tx, tz) = tile_of(nx, nz);
    let claimed = claimed_tiles(session);
    let sides = [(-1i16, 0i16), (1, 0), (0, -1), (0, 1)];
    let mut seats: Vec<(i16, i16, bool)> = sides
        .iter()
        .map(|&(dx, dz)| (tx + dx, tz + dz, false))
        .collect();
    let continuous = continuous::PLAYS_ON.with(std::cell::Cell::get);
    if continuous {
        let (cx, cz) = dispatch_tile(nx, nz);
        for (dx, dz) in sides {
            let s = (cx as i16 + dx, cz as i16 + dz, true);
            if !seats.iter().any(|t| (t.0, t.1) == (s.0, s.1)) {
                seats.push(s);
            }
        }
    }
    seats.retain(|&(sx, sz, _)| {
        (0..128).contains(&sx) && (0..128).contains(&sz) && !claimed.contains(&(sx as u8, sz as u8))
    });
    if walked && session.host.world.mode == SceneMode::Field {
        let (px, pz) = player_xz(session);
        let here = dispatch_tile(px, pz);
        let avoid = pad_avoid(session, None);
        seats.retain(|&(sx, sz, _)| {
            let seat = (sx, sz);
            let end = plan_path(session, cell_of(px, pz), seat, &avoid)
                .and_then(|p| p.last().copied())
                .map(|c| {
                    let (x, z) = cell_center(c);
                    dispatch_tile(x, z)
                });
            (i32::from(seat.0), i32::from(seat.1)) == here
                || end == Some((i32::from(seat.0), i32::from(seat.1)))
        });
    }
    seats
}

/// Put the player back on the exact spot it stood before an interaction.
/// Re-seating on the centre of `tile_of` the spot instead moved a player
/// standing off-centre onto the neighbouring tile, and against a wall that
/// tile can be solid: `jiji`'s prop examine left the party at (69, 71),
/// walled on all four sides, after starting from (9020, 9216).
///
/// The pad hand leaves a player already standing there alone, and counts
/// nothing: the give-back after an interaction that ended where it began is
/// no teleport.
#[track_caller]
pub(super) fn restore_player_xz(session: &mut NativeSession, (x, z): (i16, i16)) {
    if pad_hand() && player_xz(session) == (x, z) {
        return;
    }
    if pad_hand() && shortcut_sites_on() {
        let at = std::panic::Location::caller();
        let (px, pz) = player_xz(session);
        eprintln!(
            "      [restore] {}:{} in {} from {:?} {:?} to {:?} {:?} (frame {})",
            at.file().rsplit('/').next().unwrap_or(at.file()),
            at.line(),
            scene_name(session),
            (px, pz),
            tile_of(px, pz),
            (x, z),
            tile_of(x, z),
            session.frames
        );
    }
    shortcut("restore_xz");
    let w = &mut session.host.world;
    let y = w.sample_field_floor_height(i32::from(x), i32::from(z)) as i16;
    let slot = w.player_actor_slot.unwrap_or(0) as usize;
    if let Some(a) = w.actors.get_mut(slot) {
        a.move_state.world_x = x;
        a.move_state.world_y = y;
        a.move_state.world_z = z;
    }
}

/// Page an open conversation to its end with [`script_pad`]. `None` when it
/// ended with the frame back on the field; `Some(run)` for anything else.
pub(super) fn page_talk(session: &mut NativeSession) -> Option<Run> {
    for f in 0..DEEP_EXIT_TICKS {
        if let Err(e) = pad_budget(session) {
            return Some(Run::Parked(e));
        }
        if session.host.world.mode == SceneMode::Battle {
            if let Some(r) = drain_battle(session) {
                return Some(r);
            }
            continue;
        }
        let w = &session.host.world;
        if f >= 2 && w.dialog.inline.is_none() && !w.dialogue_owns_input() {
            return None;
        }
        let pad = script_pad(session, f);
        session.host_pad(pad);
        match session.tick() {
            Ok(SceneTickEvent::SceneEntered { name }) => return Some(Run::Entered(name)),
            Ok(_) => {}
            Err(e) => return Some(Run::Error(format!("{e:#}"))),
        }
        service_shop(session);
    }
    Some(Run::Parked(format!(
        "{} at {}",
        holder(session),
        park_site(session)
    )))
}

/// [`talk_to`] with the pad only: walk to a free tile beside the NPC, lean
/// toward it until the retail interact probe lands on it (the press turns
/// the player; the NPC's body stops the step), release, press Cross, page
/// the conversation, then turn and step away so the confirm cannot re-open
/// the same talk. Each side is tried until one gains a flag, as the seated
/// hand does.
pub(super) fn pad_talk_to(session: &mut NativeSession, slot: u8) -> Run {
    if !session.host.world.npcs.positions.contains_key(&slot) {
        return Run::Released;
    }
    let avoid = pad_avoid(session, None);
    let mut last = Run::Released;
    let mut talked = false;
    // Up to the NPC first (a counter or a table often fills the tile
    // beside it, and the probe reaches over one), then from each side. The
    // NPC's tile is re-read per attempt: a routed NPC walks while the
    // player does.
    for attempt in 0..5usize {
        let Some(&(nx, nz)) = session.host.world.npcs.positions.get(&slot) else {
            break;
        };
        let npc_tile = {
            let t = dispatch_tile(nx, nz);
            (t.0 as i16, t.1 as i16)
        };
        let (side, within) = if attempt == 0 {
            (npc_tile, 1)
        } else {
            match beside(session, npc_tile).get(attempt - 1) {
                Some(&t) => (t, 0),
                None => break,
            }
        };
        // A side inside a wall is not a side at all, and the walk toward one
        // presses straight at it once the route runs out: `cave01` P1[2],
        // Terra at (9, 89) with the wall row (9, 90) behind her. The press
        // slides down her west flank into a pocket whose way back her body
        // closes - the probes ahead of a step south clear her box from a row
        // where the probes ahead of a step north do not.
        if attempt > 0
            && session.host.world.npcs.solid
            && session.host.world.mode == SceneMode::Field
        {
            let (cx, cz) = tile_center(side);
            if session.host.world.field_tile_is_wall(cx, cz) {
                continue;
            }
        }
        // A side off the authored floor is not somewhere a player stands to
        // talk. Retail's locomotion reads only the wall bits, so the black
        // between a town's interior cutaways is open ground: `geremi`
        // P1[68] stands in the gap at (90, 124) in the south wall of a house
        // whose interior ends there, and stepping round it to (90, 125) put
        // the party in the void, from which the next walk crossed to the
        // outside of the tower door instead of the house behind it.
        // The continuous hand alone: the segmented passes are scored on the
        // routes they were tuned on, and a side skipped there moves the
        // walker's next start (`jiji`'s inn room, whose talks then cost the
        // segment its walking budget).
        if attempt > 0
            && session.host.world.mode == SceneMode::Field
            && continuous::PLAYS_ON.with(std::cell::Cell::get)
        {
            let (cx, cz) = tile_center(side);
            if !session.host.world.field_tile_is_walk_visible(cx, cz) {
                if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                    eprintln!(
                        "      [talk-walk] P1[{slot}] attempt {attempt}: {side:?} is off the floor"
                    );
                }
                continue;
            }
        }
        let walk = pad_walk(session, side, &avoid, within);
        if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
            eprintln!(
                "      [talk-walk] P1[{slot}] attempt {attempt} to {side:?}: {:?} at frame {}",
                walk.as_ref()
                    .map_err(|e| e.chars().take(100).collect::<String>()),
                session.frames
            );
        }
        match walk {
            Ok(Walk::Entered(s)) => return Run::Entered(s),
            Ok(Walk::Arrived) => {}
            // A fight on the walk that wiped the party ends the talk: the
            // next side's walk only met the same game over again, reported
            // as a fresh wipe ("wiped after 0 ticks", `taiku` P1[35]).
            Err(e) if e.contains("party wiped") => {
                return Run::Battle(e);
            }
            Err(e) => {
                if !talked {
                    last = Run::Parked(format!("pad walk to talk P1[{slot}]: {e}"));
                }
                // A walk stopped by the counter in front of the NPC still
                // leans in: the probe decides whether it reaches over
                // (`concnow` P1[12] / P1[13], the gate guards at (90, 119),
                // behind the wall row 118 their talk paints away).
                let (px, pz) = player_xz(session);
                let me = dispatch_tile(px, pz);
                // The continuous hand leans in from a tile further off: a
                // long body (`cave01` P1[2], Terra) stops the walk three
                // tiles short of its anchor.
                let reach = if continuous::PLAYS_ON.with(std::cell::Cell::get) {
                    3
                } else {
                    2
                };
                let near = (me.0 - i32::from(npc_tile.0)).abs() <= reach
                    && (me.1 - i32::from(npc_tile.1)).abs() <= reach;
                if attempt > 0 || !near {
                    continue;
                }
            }
        }
        let npc = move |s: &NativeSession| s.host.world.npcs.positions.get(&slot).copied();
        let facing = |s: &NativeSession| s.host.world.field_interact_probe_slot() == Some(slot);
        if let Some(scene) = pad_lean(session, npc, 48, facing) {
            return Run::Entered(scene);
        }
        // An encounter rolled on the lean ends it where the player stood:
        // the fight is played and the lean taken up again, as a player walks
        // back up to the counter. Read as a failure to face, it marked
        // `deroa` P1[7], the last rest before Jette's fortress, dry for the
        // run: the party walked in on 15% of its HP and no MP.
        if continuous::PLAYS_ON.with(std::cell::Cell::get) {
            for _ in 0..3 {
                if facing(session) || session.host.world.mode != SceneMode::Battle {
                    break;
                }
                FLEE_ENCOUNTERS.with(|f| f.set(true));
                let fought = drain_battle(session);
                FLEE_ENCOUNTERS.with(|f| f.set(false));
                if let Some(r) = fought {
                    return r;
                }
                if let Some(scene) = pad_lean(session, npc, 48, facing) {
                    return Run::Entered(scene);
                }
            }
        }
        // Another body stops the lean short of the probe's reach: `koin4`'s
        // baker P1[3], with a customer at the counter a tile to the south.
        // A player steps along the counter and leans in again. A probe that
        // lands on another NPC is the same case whatever `npcs.solid` says:
        // that body stands between.
        let probe_on_other = |s: &NativeSession| {
            s.host
                .world
                .field_interact_probe_slot()
                .filter(|&o| o != slot)
        };
        if session.host.world.npcs.solid || probe_on_other(session).is_some() {
            for (dx, dz) in [(0, -56), (0, 56), (-56, 0), (56, 0)] {
                if facing(session) || !walking(session) {
                    break;
                }
                let aside = move |s: &NativeSession| {
                    npc(s).map(|(x, z)| (x.saturating_add(dx), z.saturating_add(dz)))
                };
                if let Some(scene) = pad_lean(session, aside, 24, facing) {
                    return Run::Entered(scene);
                }
                if let Some(scene) = pad_lean(session, npc, 24, facing) {
                    return Run::Entered(scene);
                }
            }
        }
        // The steps above are taken about the NPC, and a body standing
        // square between the player and it is still between after each:
        // `koin1b` P1[9], whose probe lands on P1[18] from every one of them.
        // A player walks round the one in the way, a tile to either side of
        // it across the line to the NPC, and leans in from there.
        if !facing(session)
            && walking(session)
            && let Some(other) = probe_on_other(session)
            && let Some(&(ox, oz)) = session.host.world.npcs.positions.get(&other)
        {
            let (px, pz) = player_xz(session);
            let across_x = (ox - px).abs() < (oz - pz).abs();
            for side in [-1i16, 1] {
                if facing(session) || !walking(session) {
                    break;
                }
                let (sx, sz) = if across_x {
                    (side * 160, 0)
                } else {
                    (0, side * 160)
                };
                let round =
                    move |_: &NativeSession| Some((ox.saturating_add(sx), oz.saturating_add(sz)));
                if let Some(scene) = pad_lean(session, round, 32, facing) {
                    return Run::Entered(scene);
                }
                if let Some(scene) = pad_lean(session, npc, 48, facing) {
                    return Run::Entered(scene);
                }
            }
            if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                eprintln!(
                    "      [talk-round] P1[{slot}] round P1[{other}] at {:?}: player {:?} probe {:?}",
                    (ox, oz),
                    player_xz(session),
                    session.host.world.field_interact_probe_slot()
                );
            }
        }
        if !facing(session) {
            if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                eprintln!(
                    "      [talk-face] P1[{slot}] player {:?} npc {:?} probe {:?}",
                    player_xz(session),
                    session.host.world.npcs.positions.get(&slot),
                    session.host.world.field_interact_probe_slot()
                );
            }
            if !talked {
                let w = &session.host.world;
                last = Run::Parked(format!(
                    "could not face P1[{slot}] from {side:?}: player {:?} npc {:?} probe hits {:?} holder {}",
                    player_xz(session),
                    w.npcs.positions.get(&slot),
                    w.field_interact_probe_slot(),
                    holder(session)
                ));
            }
            continue;
        }
        let before = flags_of_world(session);
        // Release, then press: the interact is edge-triggered. Each half is
        // held across a whole game frame ([`press_ticks`]): in the
        // retail-faithful mode the field latches the pad once a frame, and
        // a one-tick Cross fell between two latches on `suimon`'s ferryman
        // (P1[4]), whose talk then never opened.
        for pad in [0, PadButton::Cross.mask()] {
            for _ in 0..press_ticks(session) {
                session.host_pad(pad);
                match session.tick() {
                    Ok(SceneTickEvent::SceneEntered { name }) => return Run::Entered(name),
                    Ok(_) => {}
                    Err(e) if pad != 0 => return Run::Error(format!("{e:#}")),
                    Err(_) => {}
                }
            }
        }
        talked = true;
        if let Some(r) = page_talk(session) {
            return r;
        }
        let w = &session.host.world;
        let beat_follows = !w.field_vm.pending_record_spawns.is_empty()
            || !w.field_vm.helper_contexts.is_empty()
            || w.cutscene.timeline.is_some();
        if walking(session) && !beat_follows {
            // Turn away and step off, as a player does before pressing on.
            let (px, pz) = player_xz(session);
            let away = move |_: &NativeSession| {
                Some((
                    px.saturating_add(px.saturating_sub(nx).signum() * 256),
                    pz.saturating_add(pz.saturating_sub(nz).signum() * 256),
                ))
            };
            if let Some(scene) = pad_lean(session, away, 12, |_| false) {
                return Run::Entered(scene);
            }
        }
        let r = run_while_moving(session, DEEP_EXIT_TICKS);
        if !matches!(r, Run::Released) || !walking(session) {
            return r;
        }
        // A merchant's talk gains no flag; the visit itself is the gain.
        let gained = flags_of_world(session) != before || take_shop_visited();
        last = r;
        if gained {
            break;
        }
    }
    last
}

/// A box's tiles are its inclusive rectangle, and the stands a talk is held
/// from are the ones beside the actor, nearest first.
#[test]
fn a_talk_box_is_stood_in_beside_the_actor() {
    // `rayman` P1[44]: column 107, rows 70..=80, the actor on (108, 78).
    let tiles = entry_box_tiles(Some((107, 70, 107, 80)));
    assert_eq!(tiles.len(), 11);
    assert_eq!(tiles.first(), Some(&(107, 70)));
    assert_eq!(tiles.last(), Some(&(107, 80)));
    let stands = box_stands(&tiles, (108, 78));
    assert_eq!(
        stands.first(),
        Some(&(107, 78)),
        "the tile beside the actor"
    );
    assert_eq!(
        stands,
        vec![(107, 78), (107, 77), (107, 79), (107, 76), (107, 80)]
    );
    // A box nowhere near the actor is no stand for a talk with it.
    assert!(box_stands(&tiles, (120, 30)).is_empty());
    assert!(entry_box_tiles(None).is_empty());
    assert!(entry_box_tiles(Some((9, 9, 3, 3))).is_empty());
}

/// The test is found at the cursor, behind the `0x21` an ended talk parked
/// on, and behind the jumps back to it; a record that speaks first has none.
#[test]
fn a_talk_entry_box_is_read_through_the_jumps_back_to_it() {
    // 0: CD F8 x0 z0 x1 z1 <skip to 9>; 8: 21; 9: 21; 10: 26 02 00 (-> 13);
    // 13: 26 <back to 0>.
    let mut body = vec![0xCD, 0xF8, 107, 70, 107, 80, 0x09, 0x00, 0x21, 0x21];
    body.extend_from_slice(&[0x26, 0x02, 0x00]);
    let back = (0i32 - 13 - 1) as i16;
    body.push(0x26);
    body.extend_from_slice(&back.to_le_bytes());
    let bx = Some((107, 70, 107, 80));
    assert_eq!(entry_box(&body, 0), bx);
    assert_eq!(entry_box(&body, 9), bx, "resumed on the closing 0x21");
    assert_eq!(entry_box(&body, 10), bx, "resumed one past it");
    // A record that opens on its text tests no box first.
    let text = [0x1F, b'h', b'i', 0x00, 0x21];
    assert_eq!(entry_box(&text, 0), None);
}

/// Ticks a one-shot press (or the release before it) is held so the field
/// reads it: one, or in the retail-faithful mode a whole game frame. There
/// the field latches the pad at the end of a frame the frame-cost model
/// prices at up to three vsyncs (`WorldToggles::immediate_pad` off), so a
/// shorter press can fall between two latches ([`script_pad`] holds its
/// Cross as wide for the same reason).
pub(super) fn press_ticks(session: &NativeSession) -> usize {
    if session.host.world.toggles.immediate_pad {
        1
    } else {
        4
    }
}

/// Press `mask` once, as an edge the field reads: one tick in the default
/// mode (the caller's pad before it is the release), and in the
/// retail-faithful mode [`press_ticks`] released and then as many held.
/// `Ok(Some(scene))` when a scene change landed meanwhile.
pub(super) fn press_edge(session: &mut NativeSession, mask: u16) -> Result<Option<String>, String> {
    let n = press_ticks(session);
    let release = if n > 1 { n } else { 0 };
    for (pad, ticks) in [(0, release), (mask, n)] {
        for _ in 0..ticks {
            session.host_pad(pad);
            match session.tick() {
                Ok(SceneTickEvent::SceneEntered { name }) => return Ok(Some(name)),
                Ok(_) => {}
                Err(e) if pad != 0 => return Err(format!("{e:#}")),
                Err(_) => {}
            }
        }
    }
    Ok(None)
}
