//! The progress digest the softlock detector watches, the menu digest, who
//! holds the pad, and the per-frame trace line a finding carries.

use super::*;

// ---------------------------------------------------------------------------
// Progress digest
// ---------------------------------------------------------------------------

/// `fmt::Write` straight into a hasher, so a `Debug` rendering is hashed
/// without being allocated.
pub(super) struct HashWriter(pub(super) std::collections::hash_map::DefaultHasher);

impl std::fmt::Write for HashWriter {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.0.write(s.as_bytes());
        Ok(())
    }
}

pub(super) fn player_slot(s: &NativeSession) -> usize {
    s.host.world.player_actor_slot.unwrap_or(0) as usize
}

/// Everything a player could see move. Softlock = this frozen for a whole
/// window while the pad varies. Deliberately broad: a false "frozen" costs a
/// triage, a false "moving" only a missed finding.
pub(super) fn progress_digest(s: &NativeSession) -> u64 {
    let w = &s.host.world;
    // `DefaultHasher::new()` is keyed with fixed zeros, so the digest is
    // stable across runs and processes.
    let mut h = HashWriter(std::collections::hash_map::DefaultHasher::new());
    let _ = write!(h, "{:?}|{}|", w.mode, w.active_scene_label);
    if let Some(a) = w.actors.get(player_slot(s)) {
        let m = &a.move_state;
        let _ = write!(h, "p{},{},{},{}|", m.world_x, m.world_y, m.world_z, m.y_rot);
    }
    let _ = write!(h, "pc{}|", w.field_pc);
    if let Some(tl) = w.cutscene.timeline.as_ref() {
        // A timeline-owned dialog panel pages through a conversation - and a
        // picker loops it back to its own segment - while `tl.pc` stays on
        // the segment's lead byte; the slice counter and the panel's cursor
        // are what move.
        let _ = write!(
            h,
            "tl{}|{}|{:?}|",
            tl.pc,
            tl.frames,
            tl.dialog.as_ref().map(|d| d.pc)
        );
    }
    for hc in &w.field_vm.helper_contexts {
        let _ = write!(h, "hc{}|", hc.pc);
    }
    let _ = write!(h, "{:?}|{:?}|", w.dialog.current, w.dialog.inline.is_some());
    if let Some(inline) = w.dialog.inline.as_ref() {
        let _ = write!(h, "{inline:?}|");
    }
    if let Some(m) = s.field_menu.as_ref() {
        let _ = write!(h, "menu{m:?}|");
    }
    if let Some(sub) = s.field_menu_sub.as_ref() {
        let _ = write!(h, "sub{}|", sub.row().index());
    }
    let _ = write!(h, "{:?}|{}|", w.shops.pending_shop, w.shops.shop_open);
    let _ = write!(h, "{:?}|", w.party.name_entry);
    let _ = write!(h, "{:?}|", w.cutscene.active_fmv);
    // A cutscene camera glide counting down is the shot moving: a `4C CD`
    // parks the script on it for the glide's whole length (`jouine`
    // `P2[16]` pans for 2200 frames), and nothing else in the digest moves.
    let _ = write!(h, "glide{}|", w.camera.state.glide_frames);
    // Scripted NPC glides: an actor walking off-screen is still progress.
    for (slot, m) in &w.npcs.motions {
        let _ = write!(
            h,
            "npc{slot}{:?}{:?}|",
            w.npcs.positions.get(slot),
            m.target
        );
    }
    match w.mode {
        SceneMode::Battle => {
            let _ = write!(
                h,
                "{:?}|{:?}|{}|",
                w.battle.command,
                w.battle.item_menu,
                w.battle.tutorial_boxes.len()
            );
            let _ = write!(h, "{:?}|", w.battle_ctx);
            for a in w.actors.iter().take(8) {
                let _ = write!(h, "{},{},{}|", a.battle.hp, a.battle.mp, a.battle.liveness);
            }
        }
        SceneMode::Dance => {
            let _ = write!(h, "{:?}", w.minigames.dance);
        }
        SceneMode::Fishing => {
            let _ = write!(
                h,
                "{:?}|{:?}",
                w.minigames.fishing, w.minigames.fishing_exchange
            );
        }
        SceneMode::SlotMachine => {
            let _ = write!(h, "{:?}", w.minigames.slot_machine);
        }
        SceneMode::BakaFighter => {
            let _ = write!(h, "{:?}", w.minigames.baka_fighter);
        }
        // The arena: the leg (fighters, phase, selection menu, choreography),
        // the contest between legs and its settlement. Presence alone left a
        // cursor walk on the selection grid invisible.
        SceneMode::MuscleDome => {
            let m = &w.minigames;
            let _ = write!(
                h,
                "{:?}|{:?}|{:?}|{}|{:?}",
                m.muscle_dome,
                m.muscle_contest,
                m.muscle_settlement,
                m.muscle_hub_between_legs,
                m.muscle_ringside_still
            );
        }
        _ => {
            let _ = write!(
                h,
                "{:?}{:?}",
                w.minigames.muscle_dome.is_some(),
                w.minigames.muscle_contest.is_some()
            );
        }
    }
    // The L1 map display owns the pad on a kingdom map: its mode, cursor,
    // overlay and panned camera are what a press moves there. The fade-up
    // ramp into it counts too.
    let _ = write!(
        h,
        "{:?}|{:?}|",
        w.world_map.map_display,
        w.world_map.ctrl.as_ref().map(|c| c.entry_fade.ramp)
    );
    let _ = write!(h, "${}|bag{}", w.party.money, w.party.inventory.len());
    h.0.finish()
}

/// The shop / prize-counter session's share of the progress digest: its
/// menu context and whatever sub-session it holds.
pub(super) fn menu_digest(m: &MenuRuntime) -> u64 {
    if !m.is_open() {
        return 0;
    }
    let mut h = HashWriter(std::collections::hash_map::DefaultHasher::new());
    let _ = write!(
        h,
        "{:?}|{:?}|{:?}",
        m.ctx, m.shop_session, m.recipient_session
    );
    h.0.finish()
}

/// Step an open shop / prize-counter / inn session on this frame's edges,
/// then unpark the field script a closed one left suspended - the play
/// window's `tick_menu_runtime_session`.
pub(super) fn tick_menu_session(
    menu: &mut MenuRuntime,
    world: &mut legaia_engine_core::world::World,
    edge: u16,
) {
    if menu.is_open() {
        let input = legaia_engine_menu_runtime::menu_runtime::menu_input_from_pad_edges(edge);
        menu.tick(world, input);
    }
    if world.shops.shop_open && !menu.is_open() {
        world.finish_field_shop();
    }
    if world.shops.prize_exchange_open && !menu.is_open() {
        world.finish_prize_exchange();
    }
}

/// What holds the frame, as a short signature fragment.
pub(super) fn held_by(s: &NativeSession) -> String {
    let w = &s.host.world;
    let op_at = |bc: &[u8], pc: usize| bc.get(pc).copied().unwrap_or(0xFF);
    if let Some(m) = s.field_menu.as_ref() {
        let sub = s
            .field_menu_sub
            .as_ref()
            .map(|x| format!("{:?}", x.row()))
            .unwrap_or_else(|| "root".into());
        return format!("pause-menu:{sub}:{}", variant(&format!("{:?}", m.phase())));
    }
    match w.mode {
        SceneMode::Battle => {
            if !w.battle.tutorial_boxes.is_empty() {
                return "battle:message-box".into();
            }
            if let Some(m) = w.battle.item_menu.as_ref() {
                return format!("battle:item-menu:{}", variant(&format!("{:?}", m.state)));
            }
            if let Some(c) = w.battle.command.as_ref() {
                return format!("battle:command:{}", variant(&format!("{:?}", c.phase)));
            }
            // The absorbing HP-bar pair (`hp != hp_display` with a zero
            // accumulator) is what parks the `0x51` bar-drain gate for good;
            // name it so every such park groups under one signature.
            let absorbing = w.actors.iter().take(8).any(|a| {
                a.battle
                    .hp_display
                    .is_some_and(|d| d != a.battle.hp && a.battle.hp_bar_pending == 0)
            });
            return format!(
                "battle:state:{}{}",
                variant(&format!("{:?}", w.battle_ctx.action_state)),
                if absorbing { ":hp-bar-absorbing" } else { "" }
            );
        }
        SceneMode::WorldMap if w.world_map.map_display.is_some() => {
            let mode = w.world_map.map_display.as_ref().map_or(0, |d| d.mode);
            return format!("map-display:mode{mode:#x}");
        }
        SceneMode::Field | SceneMode::WorldMap => {}
        // The pond names its sub-screen: an idle shore waiting for a cast
        // and a prize list that will not close are different parks, and a
        // reduction must not trade one for the other.
        SceneMode::Fishing => {
            let Some(f) = w.minigames.fishing.as_ref() else {
                return "mode:Fishing".into();
            };
            let screen = if w.minigames.fishing_exchange.is_some() {
                "exchange".to_string()
            } else if let Some(hub) = f.hub() {
                format!("hub-{}", variant(&format!("{:?}", hub.screen)))
            } else {
                variant(&format!("{:?}", f.phase()))
            };
            return format!("mode:Fishing:{screen}");
        }
        // The arena names its leg phase, or the contest state between legs.
        SceneMode::MuscleDome => {
            let m = &w.minigames;
            let screen = if let Some(leg) = m.muscle_dome.as_ref() {
                format!("leg-{}", variant(&format!("{:?}", leg.phase())))
            } else if let Some(c) = m.muscle_contest.as_ref() {
                format!("contest-{:?}", c.state())
            } else {
                "hub".into()
            };
            return format!("mode:MuscleDome:{screen}");
        }
        other => return format!("mode:{other:?}"),
    }
    if w.party.name_entry.is_some() {
        return "name-entry".into();
    }
    if w.shops.shop_open || w.shops.pending_shop.is_some() {
        return "shop".into();
    }
    if let Some(tl) = w.cutscene.timeline.as_ref() {
        return format!(
            "timeline@{:#06x}:op{:02x}",
            tl.pc,
            op_at(&tl.bytecode, tl.pc)
        );
    }
    if w.dialog.current.is_some() || w.dialog.inline.is_some() {
        return "dialogue".into();
    }
    if let Some(hc) = w.field_vm.helper_contexts.first() {
        return format!("helper@{:#06x}:op{:02x}", hc.pc, op_at(&hc.bytecode, hc.pc));
    }
    let m = &w.actors[player_slot(s).min(w.actors.len().saturating_sub(1))].move_state;
    format!(
        "free-roam:immobile@tile({},{})",
        (m.world_x - 0x40) >> 7,
        (m.world_z - 0x40) >> 7
    )
}

/// One triage line: mode, scene, holder, player, and the bytes at every live
/// script context's PC.
pub(super) fn trace_line(s: &NativeSession, pad: u16) -> String {
    let w = &s.host.world;
    let bytes = |bc: &[u8], pc: usize| -> String {
        bc.get(pc..(pc + 6).min(bc.len()))
            .map(|b| {
                b.iter()
                    .map(|x| format!("{x:02x}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default()
    };
    let m = &w.actors[player_slot(s).min(w.actors.len().saturating_sub(1))].move_state;
    let mut out = format!(
        "pad {pad:04x} {:?} {} held={} player=({},{},{}) field_pc={:#x}",
        w.mode,
        w.active_scene_label,
        held_by(s),
        m.world_x,
        m.world_y,
        m.world_z,
        w.field_pc
    );
    if let Some(tl) = w.cutscene.timeline.as_ref() {
        let _ = write!(out, " tl@{:#x}[{}]", tl.pc, bytes(&tl.bytecode, tl.pc));
    }
    for hc in &w.field_vm.helper_contexts {
        let _ = write!(out, " hc@{:#x}[{}]", hc.pc, bytes(&hc.bytecode, hc.pc));
    }
    if w.dialog.current.is_some() || w.dialog.inline.is_some() {
        out.push_str(" dialog");
    }
    // Free roam: which of the locomotion controller's gates holds the pad,
    // and which directions the wall / actor probes refuse - the first
    // question for a `free-roam:immobile` park.
    if matches!(w.mode, SceneMode::Field) && !w.dialogue_owns_input() {
        let walls: String = (0..4)
            .map(|d| {
                let wall = w.field_dir_blocked(m.world_x, m.world_z, d);
                let actor = w.field_actor_dir_blocked(m.world_x, m.world_z, d);
                match (wall, actor) {
                    (true, true) => 'B',
                    (true, false) => 'W',
                    (false, true) => 'A',
                    (false, false) => '.',
                }
            })
            .collect();
        // `LEGAIA_SOAK_TRACE_WALLS=1`: the wall bits of the 9x9 sub-cells
        // around the player (`#` wall, `.` open, `@` the player's own),
        // rows Z- to Z+ - what a walled-in landing is walled by.
        if std::env::var_os("LEGAIA_SOAK_TRACE_WALLS").is_some() {
            let (sx, sz) = (i32::from(m.world_x) >> 6, i32::from(m.world_z) >> 6);
            let rows: Vec<String> = (-4..=4)
                .map(|dz| {
                    (-4..=4)
                        .map(|dx| {
                            if dx == 0 && dz == 0 {
                                return '@';
                            }
                            let (x, z) = ((sx + dx) * 64 + 32, (sz + dz) * 64 + 32);
                            match (i16::try_from(x), i16::try_from(z)) {
                                (Ok(x), Ok(z)) if w.field_tile_is_wall(x, z) => '#',
                                _ => '.',
                            }
                        })
                        .collect()
                })
                .collect();
            let _ = write!(out, " walls[{}]", rows.join("/"));
        }
        let _ = write!(
            out,
            " gates(lock80000 {} engaged {} battle {} warp {} scale {:#x}) dirs[Z-,X-,Z+,X+]={walls}",
            m.flags & 0x0008_0000 != 0,
            w.script_context_engages_player(),
            w.field_scripts_held_for_battle(),
            w.field_warp_in_flight(),
            m.field_72,
        );
    }
    // Whose talk record the inline runner is stepping, and where.
    if let Some(d) = w.dialog.inline.as_ref() {
        let _ = write!(
            out,
            "(npc {:?} prop {:?} pc {:#x}[{}])",
            d.npc_slot,
            d.prop_anchor,
            d.pc,
            bytes(&d.bytecode, d.pc)
        );
    }
    if w.mode == SceneMode::Battle {
        let _ = write!(
            out,
            " ctx(state {:?} active {} timer {} menu_open {} end {:?} game_over {}/{})",
            w.battle_ctx.action_state,
            w.battle_ctx.active_actor,
            w.battle_ctx.frame_timer,
            w.battle_ctx.menu_open,
            w.battle.end,
            w.game_over,
            w.game_over_hold
        );
        // The acting actor's command: category `+0x1DE` and its first
        // parameter (the spell / move id a cast-module park keys on).
        if let Some(a) = w.actors.get(usize::from(w.battle_ctx.active_actor)) {
            let _ = write!(
                out,
                " act(cat {} p0 {:#04x} monster {:?})",
                a.battle.action_category,
                a.battle.params.first().copied().unwrap_or(0),
                a.battle_monster_id
            );
        }
        for (i, a) in w.actors.iter().take(8).enumerate() {
            if a.battle.max_hp > 0 || a.battle.liveness != 0 {
                let _ = write!(
                    out,
                    " a{i}:{}/{}L{}D{:?}P{}A{}T{}C{}",
                    a.battle.hp,
                    a.battle.max_hp,
                    a.battle.liveness,
                    a.battle.hp_display,
                    a.battle.hp_bar_pending,
                    a.battle.damage_accum,
                    a.battle.active_target,
                    a.battle.current_anim
                );
            }
        }
    }
    if let Some(ps) = w.menu.pause_session.as_ref() {
        let _ = write!(
            out,
            " travel({} @ ({},{}) handler {:#04x})",
            ps.target.scene, ps.target.tile_x, ps.target.tile_z, ps.handler_id
        );
    }
    if let Some(f) = w.minigames.fishing.as_ref() {
        let _ = write!(
            out,
            " pond(phase {:?} hub {:?} exchange {} power {} depth {} tension {})",
            f.phase(),
            f.hub(),
            w.minigames.fishing_exchange.is_some(),
            f.cast_power(),
            f.depth(),
            f.tension()
        );
    }
    for (slot, m) in &w.npcs.motions {
        let _ = write!(
            out,
            " npc{slot}@{:?}->({},{})",
            w.npcs.positions.get(slot),
            m.target.0,
            m.target.1
        );
    }
    let sub = &w.field_vm.submode_screen;
    if sub.open {
        let _ = write!(
            out,
            " submode(slot {:#04x} sub {} owner {:?} driver {:?} soft-reset {:?})",
            sub.actor.state,
            sub.actor.sub,
            sub.owner,
            w.find_actor_by_handler(
                legaia_engine_field::actor_handler::ActorHandler::SubmodeDriver
            ),
            sub.soft_reset
        );
    }
    // `LEGAIA_SOAK_TRACE_NPC=<slot>`: that slot's position on every line,
    // walking or not - where a placement stood before its walk started.
    if let Some(slot) = env_u64("LEGAIA_SOAK_TRACE_NPC") {
        let _ = write!(out, " watch{slot}@{:?}", w.npcs.positions.get(&(slot as _)));
    }
    out
}

/// `Foo { .. }` / `Foo(..)` / `Foo` -> `Foo`.
pub(super) fn variant(debug: &str) -> String {
    debug
        .split([' ', '{', '(', ','])
        .next()
        .unwrap_or("")
        .to_string()
}
