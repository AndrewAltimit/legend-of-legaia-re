//! Opening and seeding a `NativeSession`, and the small reads every pass takes
//! off it (scene, mode, player tile, who holds the pad, whether control is
//! back).

use super::*;
use legaia_engine_save_state::WorldSaveState;

// ---------------------------------------------------------------------------
// Session + seeding
// ---------------------------------------------------------------------------

pub(super) fn open_session(extracted: &Path) -> NativeSession {
    let cfg = BootConfig {
        scene: "town01".into(),
        enable_audio: false,
    };
    let mut session = NativeSession::open(extracted, &cfg).expect("open boot session");
    // Both play hosts run field dialogue through the inline-script field-VM
    // runner by default (`play-window`'s `--simple-dialogue` and the browser
    // play page's runtime both key on this toggle), so a talk executes its
    // record's flag writes, branches and scene changes. `NativeSession` leaves
    // it off; without it a conversation only types its first text segment.
    session.host.world.toggles.use_vm_dialogue = true;
    // `LEGAIA_FGL_PIN_RETAIL=1`: every fidelity toggle at retail's value
    // (`WorldToggles::pin_retail`), so the run measures the retail-faithful
    // mode - the ledge-hop landing guard off among them.
    if std::env::var_os("LEGAIA_FGL_PIN_RETAIL").is_some_and(|v| v != "0") {
        session.host.world.toggles.pin_retail();
    }
    session
}

/// Live-loop options per tier. Both tiers make battles player-driven and
/// fight them with the pad ([`fight_pad`]); the seated tier runs with the
/// random-encounter roll off (it measures doors and scripts, not
/// encounters), the pad tier arms it.
pub(super) fn live_opts(pad: bool) -> FieldLiveOpts {
    FieldLiveOpts {
        live_loop: pad,
        player_battle: true,
        battle_bgm: None,
    }
}

/// Seed `session` at milestone `m`. `Ok(scene)` when the landing entered the
/// milestone's scene.
pub(super) fn seed(
    session: &mut NativeSession,
    m: &Milestone,
    anchor: Option<&Anchor>,
    opts: &FieldLiveOpts,
) -> Result<String, String> {
    if m.seed.as_deref() == Some("new_game") {
        return session
            .start_new_game(opts)
            .map(str::to_string)
            .ok_or_else(|| "start_new_game entered nothing".to_string());
    }
    let a = anchor.ok_or("no anchor to seed from")?;
    let sf =
        legaia_save::SaveFile::from_retail_sc_block(&a.sc, legaia_save::RETAIL_SC_PARTY_RECORDS)
            .map_err(|e| format!("lift SC block: {e:#}"))?;
    // Flags go in before the entry so the entry script's first slice and the
    // scene's gated spawns see the saved story state, then the host's own
    // resume path runs (which applies the save again over the landing, and
    // seats the party at the save's position unless a live one is armed).
    // The save's options words (Field Move among them) come in with it, as
    // a card load restores them: `jouind`'s timed switch pair is paced for
    // the anchor's player, who ran.
    session.host.world.load_full(sf.clone());
    if let Some((x, z)) = a.seat {
        session.host.set_entry_seat(x, z);
    }
    let landing = session.resume_save(sf, &m.scene, opts);
    if !landing.entered_scene() {
        return Err(format!("resume landed {landing:?}"));
    }
    Ok(landing.scene().unwrap_or_default().to_string())
}

pub(super) fn scene_name(session: &NativeSession) -> String {
    session
        .host
        .scene
        .as_ref()
        .map(|s| s.name.clone())
        .unwrap_or_default()
}

pub(super) fn walking(session: &NativeSession) -> bool {
    matches!(
        session.host.world.mode,
        SceneMode::Field | SceneMode::WorldMap
    )
}

pub(super) fn player_xz(session: &NativeSession) -> (i16, i16) {
    let w = &session.host.world;
    let slot = w.player_actor_slot.unwrap_or(0) as usize;
    let ms = &w.actors[slot].move_state;
    (ms.world_x, ms.world_z)
}

pub(super) fn tile_of(x: i16, z: i16) -> (i16, i16) {
    ((x - 0x40) >> 7, (z - 0x40) >> 7)
}

/// Where the context holding the pad came to rest, as `(pc, opcode)`: the
/// modal timeline's, else the first live helper record's, else the entry
/// script's (the frontier ladder's `park_site`).
pub(super) fn park_site(session: &NativeSession) -> String {
    let w = &session.host.world;
    let at = |who: &str, bc: &[u8], pc: usize| match bc.get(pc) {
        Some(op) => format!("{who} pc=0x{pc:04X} op=0x{op:02X}"),
        None => format!("{who} pc=0x{pc:04X} (past end {})", bc.len()),
    };
    if let Some(tl) = w.cutscene.timeline.as_ref() {
        return at("timeline", &tl.bytecode, tl.pc);
    }
    if let Some(id) = w.dialog.inline.as_ref() {
        return at("talk", &id.bytecode, id.pc);
    }
    if let Some(h) = w.field_vm.helper_contexts.first() {
        return at("helper", &h.bytecode, h.pc);
    }
    at("entry", &w.field_bytecode, w.field_pc)
}

pub(super) fn holder(session: &NativeSession) -> &'static str {
    let w = &session.host.world;
    if w.scene_transition_hold.is_some() {
        "a scene change in flight"
    } else if w.cutscene_timeline_active() {
        "cutscene timeline"
    } else if w.dialogue_owns_input() {
        "dialogue"
    } else if !w.field_vm.helper_contexts.is_empty() {
        "a spawned record"
    } else if !w.field_vm.pending_record_spawns.is_empty() {
        "a queued spawn"
    } else {
        "nothing"
    }
}

/// The partition-2 record the modal cutscene timeline is running, matched
/// by its bytes against the scene's records.
pub(super) fn timeline_p2_record(session: &NativeSession) -> Option<u8> {
    let tl = session.host.world.cutscene.timeline.as_ref()?;
    let (mf, man, _) = scene_man_and_triggers(session)?;
    let n2 = mf.partitions.get(2)?.len();
    (0..n2).find_map(|i| {
        let (start, _, len) =
            legaia_engine_field::man_field_scripts::partition_record_span(&mf, &man, 2, i)?;
        (len == tl.bytecode.len() && man.get(start..start + len)? == tl.bytecode.as_slice())
            .then_some(i as u8)
    })
}

pub(super) fn released(session: &NativeSession) -> bool {
    let w = &session.host.world;
    // A scene change parked behind the streaming actor's countdown has not
    // released anything: the door is mid-flight.
    walking(session)
        && w.scene_transition_hold.is_none()
        && !w.cutscene_timeline_active()
        && !w.dialogue_owns_input()
        && w.field_vm.helper_contexts.is_empty()
        && w.field_vm.pending_record_spawns.is_empty()
        && w.active_fmv().is_none()
}

/// Fight what a beat committed. A record that ends on `3E FF` (`chitei2`
/// P2[13], the Jette fight) is gone before its battle's intro runs, so the
/// beat reads as released while the fight is still to come - and what the
/// story does next hangs on the post-battle return (the scene system script
/// re-runs and spawns the next record). Anything but a released run, or a
/// release with no fight committed, passes through.
///
/// A beat that reaches the milestone it is played for stops there, fight or
/// no fight: `chitei2` P2[11] sets the `0x470` its milestone waits on, then
/// stages a battle that belongs to the next stretch.
pub(super) fn fight_committed(session: &mut NativeSession, r: Run) -> Run {
    if !matches!(r, Run::Released) || !session.host.world.field_scripts_held_for_battle() {
        return r;
    }
    let reached_here =
        BEAT_TARGET.with(|t| t.borrow().as_ref().is_some_and(|m| reached(session, m)));
    if reached_here {
        return r;
    }
    for _ in 0..SETTLE_TICKS {
        if session.host.world.mode == SceneMode::Battle {
            if let Some(r) = drain_battle(session) {
                return r;
            }
            return run_while_moving(session, DEEP_EXIT_TICKS);
        }
        session.host.world.set_pad(0);
        match session.tick() {
            Ok(SceneTickEvent::SceneEntered { name }) => return Run::Entered(name),
            Ok(_) => {}
            Err(e) => return Run::Error(format!("{e:#}")),
        }
    }
    r
}

/// Print and drain the script contexts the engine's anti-hang timeline cap
/// ended since the last call (`World::cutscene.capped`), under
/// `LEGAIA_FGL_CAPS`: the census of shipped records the cap cuts short.
pub(super) fn note_caps(session: &mut NativeSession) {
    if session.host.world.cutscene.capped.is_empty() {
        return;
    }
    let caps = std::mem::take(&mut session.host.world.cutscene.capped);
    if std::env::var_os("LEGAIA_FGL_CAPS").is_some() {
        for c in caps {
            eprintln!(
                "    [cap] {} {} len {:#x} pc {:#06x} op {:#04x} frame {}",
                c.scene,
                if c.modal { "timeline" } else { "helper" },
                c.len,
                c.pc,
                c.op,
                session.frames
            );
        }
    }
}
