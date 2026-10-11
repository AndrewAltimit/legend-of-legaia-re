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
    )
}

/// Examine the interact-gated prop anchored at `anchor` (the cupboard class,
/// retail `FUN_801CFC40` result bit `1`): the same stand-beside, face and
/// press-Cross approach as a talk, aimed so the prop arm of the facing probe
/// (`FUN_801CF9F4`) lands on its contact box.
pub(super) fn examine_prop(session: &mut NativeSession, anchor: (u8, u8), at: (i32, i32)) -> Run {
    let pos = (at.0 as i16, at.1 as i16);
    interact_at(session, pos, &|w: &legaia_engine_core::world::World| {
        w.field_interact_probe_slot().is_none() && w.field_interact_prop_anchor() == Some(anchor)
    })
}

/// The approach [`talk_to`] and [`examine_prop`] share: from each tile
/// beside `(nx, nz)`, face the sector where `hits` holds, press Cross and
/// page what opens.
pub(super) fn interact_at(
    session: &mut NativeSession,
    (nx, nz): (i16, i16),
    hits: &dyn Fn(&legaia_engine_core::world::World) -> bool,
) -> Run {
    let (bx, bz) = player_xz(session);
    let (tx, tz) = tile_of(nx, nz);
    let claimed = claimed_tiles(session);
    let mut last = Run::Released;
    for (dx, dz) in [(-1i16, 0i16), (1, 0), (0, -1), (0, 1)] {
        let (sx, sz) = (tx + dx, tz + dz);
        if !(0..128).contains(&sx)
            || !(0..128).contains(&sz)
            || claimed.contains(&(sx as u8, sz as u8))
        {
            continue;
        }
        session.host.world.set_pad(0);
        session.host.world.seat_player_at_tile(sx as u8, sz as u8);
        // The one compass sector whose probe point lands in the NPC's box.
        let facing = (0..8u8).find(|&d| {
            session.host.world.face_player_sector(d);
            hits(&session.host.world)
        });
        let Some(_) = facing else {
            continue;
        };
        let before = flags_of_world(session);
        session.host.world.set_pad(PadButton::Cross.mask());
        match session.tick() {
            Ok(SceneTickEvent::SceneEntered { name }) => return Run::Entered(name),
            Ok(_) => {}
            Err(e) => return Run::Error(format!("{e:#}")),
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
            session.host.world.set_pad(pad);
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
        if walking(session) && !beat_follows {
            restore_player_xz(session, (bx, bz));
        }
        let r = run_while_moving(session, DEEP_EXIT_TICKS);
        if !matches!(r, Run::Released) || !walking(session) {
            return r;
        }
        let gained = flags_of_world(session) != before;
        last = r;
        if gained {
            break;
        }
    }
    if walking(session) {
        restore_player_xz(session, (bx, bz));
    }
    last
}

/// Put the player back on the exact spot it stood before an interaction.
/// Re-seating on the centre of `tile_of` the spot instead moved a player
/// standing off-centre onto the neighbouring tile, and against a wall that
/// tile can be solid: `jiji`'s prop examine left the party at (69, 71),
/// walled on all four sides, after starting from (9020, 9216).
pub(super) fn restore_player_xz(session: &mut NativeSession, (x, z): (i16, i16)) {
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
        session.host.world.set_pad(pad);
        match session.tick() {
            Ok(SceneTickEvent::SceneEntered { name }) => return Some(Run::Entered(name)),
            Ok(_) => {}
            Err(e) => return Some(Run::Error(format!("{e:#}"))),
        }
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
                let near = (me.0 - i32::from(npc_tile.0)).abs() <= 2
                    && (me.1 - i32::from(npc_tile.1)).abs() <= 2;
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
        // Release, then press: the interact is edge-triggered.
        session.host.world.set_pad(0);
        if let Ok(SceneTickEvent::SceneEntered { name }) = session.tick() {
            return Run::Entered(name);
        }
        session.host.world.set_pad(PadButton::Cross.mask());
        match session.tick() {
            Ok(SceneTickEvent::SceneEntered { name }) => return Run::Entered(name),
            Ok(_) => {}
            Err(e) => return Run::Error(format!("{e:#}")),
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
        let gained = flags_of_world(session) != before;
        last = r;
        if gained {
            break;
        }
    }
    last
}
