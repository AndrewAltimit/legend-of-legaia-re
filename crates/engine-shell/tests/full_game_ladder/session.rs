//! Opening and seeding a `NativeSession`, and the small reads every pass takes
//! off it (scene, mode, player tile, who holds the pad, whether control is
//! back).

use super::*;
use legaia_engine_save_state::WorldSaveState;

// ---------------------------------------------------------------------------
// Session + seeding
// ---------------------------------------------------------------------------

/// The hand's pad, fed the way both play hosts feed the field's.
///
/// The window and the browser page hand the field a neutral pad while the
/// world holds it (`BootSession::claim_tick`: a narration crawl, a title
/// card). A hand that wrote the world's pad directly played straight through
/// such a surface where a player could not: the Uru Mais softlock (#653), a
/// card left up over the box the player had to turn, never showed here.
/// Every press the hand makes goes through [`HostPad::host_pad`].
///
/// `LEGAIA_FGL_RAW_PAD=1` restores the direct write, for bisecting a stall
/// this routing exposes.
pub(super) trait HostPad {
    fn host_pad(&mut self, pad: u16);
}

/// `LEGAIA_FGL_WATCH_FLAGS=0x403,...`: print each flip of those system flags
/// the hand's presses see, with the scene, frame and the pad holder's park
/// site - how a flag a later gate needs is traced to what lowered it.
fn watch_flags(session: &NativeSession) {
    thread_local! {
        static SEEN: std::cell::RefCell<BTreeMap<u16, bool>> =
            const { std::cell::RefCell::new(BTreeMap::new()) };
    }
    static WATCH: std::sync::OnceLock<Vec<u16>> = std::sync::OnceLock::new();
    let watch = WATCH.get_or_init(|| {
        std::env::var("LEGAIA_FGL_WATCH_FLAGS")
            .map(|v| {
                v.split(',')
                    .filter_map(|f| u16::from_str_radix(f.trim().trim_start_matches("0x"), 16).ok())
                    .collect()
            })
            .unwrap_or_default()
    });
    if watch.is_empty() {
        return;
    }
    let flags = &session.host.world.flags.system_flags;
    for &f in watch {
        let on = flags
            .get(usize::from(f / 8))
            .is_some_and(|b| b & (0x80 >> (f % 8)) != 0);
        let was = SEEN.with(|s| s.borrow_mut().insert(f, on));
        if was.is_some_and(|w| w != on) {
            eprintln!(
                "      [watch] 0x{f:03X} -> {} in {} at frame {} ({}, {})",
                u8::from(on),
                scene_name(session),
                session.frames,
                holder(session),
                park_site(session)
            );
        }
    }
}

impl HostPad for NativeSession {
    fn host_pad(&mut self, pad: u16) {
        watch_flags(self);
        // The opening's intro hand-off (`claim_tick`'s `PrologueSkip`, retail
        // `FUN_801D1344`): once the opening chain arms it, a held Select ends
        // the opening on both play hosts. The headless tick never takes it.
        //
        // The hand watches the opening, as a player who wants to see it
        // does: it never holds Select, and it presses no Cross while the
        // hand-off is armed, so the ladder plays the whole chain at the
        // narration's own pace. `LEGAIA_FGL_SKIP_OPENING=1` lets the hand's
        // presses through instead, and the gate takes the hand-off on the
        // first Cross (a ladder shortcut standing in for the hosts' Select).
        let armed = {
            let w = &self.host.world;
            w.flags.story_flags & legaia_engine_core::world::PROLOGUE_HANDOFF_FLAG != 0
                && w.cutscene.opening_chain_active
        };
        let pad = if armed && !skip_opening() {
            pad & !PadButton::Cross.mask()
        } else {
            pad
        };
        if armed
            && pad & !self.host.world.input.pad() & PadButton::Cross.mask() != 0
            && let Some(target) = self.host.world.take_prologue_handoff(true)
        {
            shortcut("prologue_skip");
            if let Err(e) = self.enter_field_live(target, &live_opts(pad_hand())) {
                panic!("prologue hand-off: enter '{target}' failed: {e:#}");
            }
        }
        // The neutral pad is the field's, in `claim_tick`'s order: the
        // pause menu and the naming prompt take the tick ahead of it and
        // read the pad as pressed, on both hosts.
        let field_owns_tick = self.field_menu.is_none() && !self.host.world.name_entry_active();
        let pad = if field_owns_tick && self.world_holds_field_pad() && !raw_pad() {
            0
        } else {
            if !field_owns_tick && self.world_holds_field_pad() && pad != 0 {
                shortcut("gate_order");
            }
            pad
        };
        self.host.world.set_pad(pad);
    }
}

fn skip_opening() -> bool {
    static SKIP: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *SKIP.get_or_init(|| std::env::var_os("LEGAIA_FGL_SKIP_OPENING").is_some_and(|v| v != "0"))
}

fn raw_pad() -> bool {
    static RAW: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *RAW.get_or_init(|| std::env::var_os("LEGAIA_FGL_RAW_PAD").is_some())
}

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
    // The CD timing players get (console pacing for the field and for
    // battle), as both play hosts seat it: a ladder on the harness's
    // PCSX-Redux timing paced voiced beats and fights the way no player sees
    // them.
    legaia_engine_session::native::play_cd_timing().apply(&mut session.host.world.toggles);
    // NPC bodies are solid, as both play hosts have them: a body stops the
    // pad tier's walk as it stops a player, and a story gate that is a body
    // (`kor`'s guard dogs, `deene`'s P1[4] / P1[5]) holds.
    // `LEGAIA_FGL_SOLID_NPCS=0` leaves them passable: a probe, not the
    // measured configuration.
    session.host.world.npcs.solid =
        std::env::var_os("LEGAIA_FGL_SOLID_NPCS").is_none_or(|v| v != "0");
    // `LEGAIA_FGL_PIN_RETAIL=1`: every fidelity toggle at retail's value
    // (`WorldToggles::pin_retail`), so the run measures the retail-faithful
    // mode - the ledge-hop landing guard off among them.
    if std::env::var_os("LEGAIA_FGL_PIN_RETAIL").is_some_and(|v| v != "0") {
        session.host.world.toggles.pin_retail();
    }
    // The hand's menu and shop presses are one-tick edges it decides on the
    // screen state of the tick before (`tap_pad`, `service_shop`): host
    // screens read them on their tick under every tier, as before the
    // retail-faithful mode ran them through the pad pipeline
    // (`World::screen_edge`). The field keeps the tier's pipeline. A harness
    // seat.
    session.host.world.input.set_screen_immediate(true);
    // The options players play on: both play hosts push their options onto
    // the world every tick (`BootSession::apply_option_knobs`), and their
    // defaults turn precise movement on. The hand takes the same push here
    // (and again in every session a reload opens). `LEGAIA_FGL_RETAIL_DECODE=1`
    // keeps retail's quantised direction decode instead; `LEGAIA_FGL_PIN_RETAIL`
    // pins the options' retail values too.
    let mut opts = legaia_engine_menus::options::OptionsState::default();
    if std::env::var_os("LEGAIA_FGL_PIN_RETAIL").is_some_and(|v| v != "0") {
        opts.pin_retail();
    }
    let retail_decode = std::env::var_os("LEGAIA_FGL_RETAIL_DECODE").is_some_and(|v| v != "0");
    session.apply_option_knobs(&opts, retail_decode.then_some(false));
    // `LEGAIA_BATTLE_MEMO_VERIFY=1`: every hit of the battle edge's memos is
    // rebuilt behind and compared.
    session.host.world.toggles.battle_memo_verify =
        std::env::var_os("LEGAIA_BATTLE_MEMO_VERIFY").is_some_and(|v| v != "0");
    session
}

/// Live-loop options per tier. Both tiers make battles player-driven and
/// fight them with the pad ([`fight_pad`]); the seated tier runs with the
/// random-encounter roll off (it measures doors and scripts, not
/// encounters), the pad tier arms it. `LEGAIA_FGL_NO_ENCOUNTERS=1` leaves it
/// off for the pad tier too: the digest a gate replay plays on retail, which
/// then runs with retail's own roll off, so the two walks do not part on two
/// `rand()` streams (`docs/tooling/gate-replay.md`).
pub(super) fn live_opts(pad: bool) -> FieldLiveOpts {
    let no_encounters = std::env::var_os("LEGAIA_FGL_NO_ENCOUNTERS").is_some_and(|v| v != "0");
    FieldLiveOpts {
        live_loop: pad && !no_encounters,
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
    // Harness (seed): a segment starts from retail's save at its milestone;
    // part C seeds only its New Game this way.
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
    // Nor has a field MAIN INIT hold (the credits' load, `field_init_hold`):
    // the destination's entry records have not had a frame yet.
    walking(session)
        && w.scene_transition_hold.is_none()
        && w.field_init_hold == 0
        && !w.cutscene_timeline_active()
        && !w.dialogue_owns_input()
        && w.field_vm.helper_contexts.is_empty()
        && w.field_vm.pending_record_spawns.is_empty()
        && w.active_fmv().is_none()
}

/// The player actor's engaged bit (`+0x10 & 0x80000`, the movement lock the
/// walk controller reads) is up while nothing the hand pages holds the pad:
/// no scene change, timeline, dialogue, spawned record or movie. Something
/// else in the field raised it and waits on the player.
pub(super) fn player_held(session: &NativeSession) -> bool {
    let w = &session.host.world;
    released(session)
        && session.field_menu.is_none()
        && w.player_actor_slot
            .and_then(|s| w.actors.get(usize::from(s)))
            .is_some_and(|a| a.move_state.flags & 0x0008_0000 != 0)
}

/// Ticks [`answer_notice`] gives a notice to close.
const NOTICE_ANSWER_TICKS: usize = 120;

/// Answer a notice the field raised on its own, as a player does: let go of
/// the pad, press Cross, and again until it is gone. `true` when one was up.
///
/// The Incense wear-off notice is the one there is (retail `FUN_801D0B90`'s
/// zero edge, `World::raise_incense_notice`): the walk tick that drains the
/// window's last count locks the player's movement and shows a one-line
/// panel, and the panel's handler (`FUN_801F1E48`) waits for a confirm or
/// cancel **edge**. No script runs under it, so nothing the hand reads as a
/// pad holder is up, and a walk that only holds a direction stands there for
/// good. The walk's once-a-second action tap used to close it by accident;
/// beside a person that tap is withheld, and a notice that came up within
/// reach of `kor3`'s resident P1[9] at (18, 38) stopped every walk through
/// the stair doorway until the stretch gave up.
pub(super) fn answer_notice(session: &mut NativeSession) -> bool {
    answer_notice_on(
        session,
        |s| &mut s.host.world,
        |s, pad| s.host_pad(pad),
        |s| {
            let _ = s.tick();
        },
    )
}

/// [`answer_notice`] over anything that owns a world and can tick it. Each
/// press goes through `pad`: the session's is the hosts' pad gate
/// ([`HostPad::host_pad`]), as every other press the hand makes.
pub(super) fn answer_notice_on<S>(
    s: &mut S,
    world: impl Fn(&mut S) -> &mut legaia_engine_core::world::World,
    pad: impl Fn(&mut S, u16),
    mut tick: impl FnMut(&mut S),
) -> bool {
    if world(s).locomotion.incense_notice.is_none() {
        return false;
    }
    for i in 0..NOTICE_ANSWER_TICKS {
        let w = world(s);
        if w.locomotion.incense_notice.is_none()
            || !matches!(w.mode, SceneMode::Field | SceneMode::WorldMap)
        {
            break;
        }
        // The handler reads an edge: a released frame, then the press.
        pad(
            s,
            if i % 2 == 0 {
                0
            } else {
                PadButton::Cross.mask()
            },
        );
        tick(s);
    }
    pad(s, 0);
    true
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
/// Ticks after a committed fight's battle before the idle count starts.
const AFTER_BATTLE_SETTLE_TICKS: usize = 12;

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
            // The post-fight record starts a few ticks after the mode returns
            // (`tunnelc` P1[4], five after Xain's battle, the tick the release
            // count below reaches its four idle ticks): idle is not counted
            // until it has had the time to spawn.
            for _ in 0..AFTER_BATTLE_SETTLE_TICKS {
                session.host_pad(0);
                match session.tick() {
                    Ok(SceneTickEvent::SceneEntered { name }) => return Run::Entered(name),
                    Ok(_) => {}
                    Err(e) => return Run::Error(format!("{e:#}")),
                }
            }
            return run_while_moving(session, DEEP_EXIT_TICKS);
        }
        session.host_pad(0);
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

/// A field world with a player and the Incense notice just raised.
#[cfg(test)]
fn world_under_the_notice() -> legaia_engine_core::world::World {
    let mut w = legaia_engine_core::world::World {
        mode: SceneMode::Field,
        ..Default::default()
    };
    w.spawn_actor(0).active = true;
    w.player_actor_slot = Some(0);
    w.raise_incense_notice();
    w
}

/// The stall the notice made: a hand that only holds a direction - and one
/// that holds Cross without letting go - never gives the handler its edge,
/// so the notice and the movement lock stay up.
#[test]
fn a_held_pad_never_answers_the_incense_notice() {
    for pad in [PadButton::Down.mask(), PadButton::Cross.mask()] {
        let mut w = world_under_the_notice();
        // The frame the notice comes up on already has the pad down.
        w.set_pad(pad);
        let _ = w.tick();
        for _ in 0..300 {
            w.set_pad(pad);
            let _ = w.tick();
        }
        assert!(
            w.locomotion.incense_notice.is_some(),
            "pad {pad:#06x} held closed the notice"
        );
        assert_ne!(w.actors[0].move_state.flags & 0x0008_0000, 0);
    }
}

/// [`answer_notice_on`] closes it whatever the pad held before, and hands
/// the player back unlocked with the pad released.
#[test]
fn the_hand_answers_the_incense_notice() {
    for before in [0, PadButton::Down.mask(), PadButton::Cross.mask()] {
        let mut w = world_under_the_notice();
        w.set_pad(before);
        let _ = w.tick();
        let mut ticks = 0;
        assert!(answer_notice_on(
            &mut w,
            |w| w,
            |w, pad| w.set_pad(pad),
            |w| {
                ticks += 1;
                let _ = w.tick();
            }
        ));
        assert!(w.locomotion.incense_notice.is_none(), "after {ticks} ticks");
        assert_eq!(w.actors[0].move_state.flags & 0x0008_0000, 0);
        assert_eq!(w.input.pad(), 0);
        assert!(ticks < 20, "a player answers it at once, not in {ticks}");
    }
}

/// With no notice up the hand spends no frame on it.
#[test]
fn no_notice_costs_no_tick() {
    let mut w = world_under_the_notice();
    w.locomotion.incense_notice = None;
    let mut ticks = 0;
    assert!(!answer_notice_on(
        &mut w,
        |w| w,
        |w, pad| w.set_pad(pad),
        |_| ticks += 1
    ));
    assert_eq!(ticks, 0);
}
