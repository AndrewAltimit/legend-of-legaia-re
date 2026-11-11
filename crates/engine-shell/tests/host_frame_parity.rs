//! Disc-gated: the native play window's **own frame body** and the browser
//! play page's, driven by one pad script, leave the same world state.
//!
//! `field_walk_host_parity` and `mode_seat_host_parity` drive the native
//! side through `NativeSession`, which is the session both hosts already
//! share. What a host wraps around the session tick - its per-tick modal
//! arms, the steps before and after the tick, the draw path's world-moving
//! steps - is where the two drift. So this drives the window itself
//! (`window::HeadlessPlayWindow`: the app `play-window` assembles, minus the
//! renderer, with its audio director over a device-less output) against the
//! page's `LegaiaRuntime` the way `site/js/play-view-frame.js` drives it -
//! the frame's ticks through `play_frame_ticks`, the export that owns their
//! order on the page - and compares a host-agnostic sample of the session
//! (`legaia_engine_session::host_trace`, the audio director's command record
//! included) after every display frame, channel by channel.
//!
//! Each scenario runs at more than one display rate. A display frame that
//! runs two sim ticks (a 30 Hz display) or none (a redraw between two ticks
//! on a fast display) is where a step one host runs per redraw and the other
//! per tick parts them; a 60 Hz run cannot see it.
//!
//! What the comparison cannot see: anything a renderer owns (both drivers
//! draw nothing), what an audio output sounds (the record is of what each
//! host asked for), and the page's JavaScript beyond the bookkeeping
//! replayed in [`PageDriver::frame`].
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_session::host_trace::{self, HostTraceFrame};
use legaia_engine_shell::window::HeadlessPlayWindow;
use legaia_engine_system::input::PadButton;
use legaia_web_play::runtime::LegaiaRuntime;

/// How many sim ticks each display frame runs, cycled.
#[derive(Clone, Copy, Debug)]
enum Cadence {
    /// One tick per frame: a 60 Hz display.
    Hz60,
    /// Two ticks per frame: a 30 Hz display.
    Hz30,
    /// One tick every other frame: a 120 Hz display.
    Hz120,
}

impl Cadence {
    fn ticks(self, frame: u32) -> u32 {
        match self {
            Cadence::Hz60 => 1,
            Cadence::Hz30 => 2,
            Cadence::Hz120 => frame % 2,
        }
    }
}

struct Scenario {
    scene: &'static str,
    /// Pad word held over a display frame whose first sim tick is `t`
    /// (1-based), given the native host's latest sample. The native run's
    /// words are recorded and replayed to the page, so a pad that steers off
    /// the world still hands both hosts the same input.
    pad: fn(u32, &HostTraceFrame) -> u16,
    ticks: u32,
    /// What the run must have exercised, so a green run is not a vacuous
    /// one (the hosts agreeing on a world that never moved).
    exercised: fn(&[HostTraceFrame]) -> Result<(), String>,
    /// `(tick, idx)`: stage scene shop `idx` the way its merchant's op-`0x49`
    /// record would (`World::debug_arm_scene_shop`) ahead of the display
    /// frame that reaches `tick`, on both hosts at the same frame.
    arm_shop: Option<(u32, usize)>,
    /// `(tick, code)`: press host hotkey `code` (a `KeyboardEvent.code`,
    /// `legaia_engine_session::hotkeys`) ahead of the display frame that
    /// reaches `tick` - natively through the window's keyboard handler, on
    /// the page through `play_host_hotkey` plus the page action that key
    /// runs.
    hotkeys: &'static [(u32, &'static str)],
    /// While the native world is in battle, a pad read off the battle's own
    /// command phase replaces [`Self::pad`]'s word (the native word is
    /// recorded and replayed to the page like any other).
    battle_pad: Option<fn(&legaia_engine_core::world::World) -> u16>,
    /// `(tick, sub_id, key)`: launch minigame `sub_id` ahead of the display
    /// frame that reaches `tick` - natively through the window's own
    /// minigame key (`key`, which arms the mode-24 door warp, or opens the
    /// pond for fishing), on the page through its picker's
    /// `play_mg_debug_warp` (`play_fishing_start` for sub-id `0`). Two
    /// ids past the warp space stand for the game-mode seat:
    /// [`MODE_PICKER`] opens its picker and [`MODE_LEAVE`] leaves it (the
    /// backquote key natively, `play_pack_mode_picker` / `_exit` on the
    /// page).
    warps: &'static [(u32, u8, &'static str)],
    /// Casino coins seated on both hosts before the first frame
    /// (`WorldCheats::cheat_set_coins`, the page's `cheat_apply("coins")`);
    /// `0` seats none.
    coins: u64,
}

/// [`Scenario::warps`] id: open the game-mode picker.
const MODE_PICKER: u8 = 100;
/// [`Scenario::warps`] id: leave the game-mode seat.
const MODE_LEAVE: u8 = 101;

const UP: u16 = 0x0010;
const RIGHT: u16 = 0x0020;
const DOWN: u16 = 0x0040;
const LEFT: u16 = 0x0080;
const CROSS: u16 = 0x4000;
const CIRCLE: u16 = 0x2000;
const START: u16 = 0x0008;

/// A press held over `[at, at + 4)`.
fn tap(t: u32, at: u32, mask: u16) -> u16 {
    if (at..at + 4).contains(&t) { mask } else { 0 }
}

fn walk_pad(t: u32, _: &HostTraceFrame) -> u16 {
    if (20..160).contains(&t) {
        PadButton::Up.mask()
    } else if (160..285).contains(&t) {
        PadButton::Right.mask()
    } else {
        0
    }
}

/// Walk, open the pause menu, step its root list, enter one sub-screen and
/// back out of it, close the menu (Circle on the root list), walk again.
fn menu_pad(t: u32, _: &HostTraceFrame) -> u16 {
    let walk = if (10..40).contains(&t) || (330..380).contains(&t) {
        UP
    } else {
        0
    };
    walk | tap(t, 50, START)
        | tap(t, 110, DOWN)
        | tap(t, 130, DOWN)
        | tap(t, 150, UP)
        | tap(t, 170, CROSS)
        | tap(t, 230, CIRCLE)
        | tap(t, 270, CIRCLE)
}

/// Walk a box on an encounter field, confirming every so often: the walk
/// rolls encounters, the confirms drive whatever battle opens.
fn encounter_pad(t: u32, _: &HostTraceFrame) -> u16 {
    let dir = match (t / 90) % 4 {
        0 => UP,
        1 => RIGHT,
        2 => DOWN,
        _ => LEFT,
    };
    let confirm = if t % 24 < 3 { CROSS } else { 0 };
    dir | confirm
}

fn moved(tr: &[HostTraceFrame]) -> Result<(), String> {
    let first = tr
        .first()
        .and_then(|f| f.actors.first())
        .map(|a| (a.x, a.z));
    let last = tr.last().and_then(|f| f.actors.first()).map(|a| (a.x, a.z));
    match (first, last) {
        (Some(a), Some(b)) if (a.0 - b.0).abs() + (a.1 - b.1).abs() > 128 => Ok(()),
        _ => Err(format!("the player did not move ({first:?} -> {last:?})")),
    }
}

fn visited_menu(tr: &[HostTraceFrame]) -> Result<(), String> {
    if !tr.iter().any(|f| f.mode == "Menu") {
        return Err("the pause menu never opened".into());
    }
    if tr.last().is_some_and(|f| f.mode == "Menu") {
        return Err("the pause menu never closed".into());
    }
    moved(tr)
}

fn visited_battle(tr: &[HostTraceFrame]) -> Result<(), String> {
    if tr.iter().any(|f| f.mode == "Battle") {
        Ok(())
    } else {
        Err("no encounter opened a battle".into())
    }
}

/// What the native run hands the page run: its trace, the pad word of every
/// display frame, and the frame the shop was staged ahead of.
struct NativeRun {
    trace: Vec<HostTraceFrame>,
    /// The field party readout's state after every display frame - a host
    /// holding, not a session one, so outside `HostTraceFrame`: each host
    /// steps it on every tick, the early-exit ticks included.
    hud: Vec<String>,
    /// The director's last commands, for a divergence report.
    audio_recent: Vec<String>,
    pads: Vec<u16>,
    arm_frame: Option<u32>,
    /// `(display frame, code)` for every hotkey pressed.
    key_frames: Vec<(u32, &'static str)>,
    /// `(display frame, sub_id)` for every minigame launched.
    warp_frames: Vec<(u32, u8)>,
}

fn native_trace(disc: &str, sc: &Scenario, cadence: Cadence) -> NativeRun {
    let mut w = HeadlessPlayWindow::open(std::path::Path::new(disc), sc.scene).expect("open");
    if sc.coins > 0 {
        use legaia_engine_cheats::cheats::WorldCheats;
        w.session_mut().host.world.cheat_set_coins(sc.coins);
    }
    let mut out = vec![host_trace::sample_with_overlay(
        w.session(),
        w.menu_runtime(),
    )];
    let mut hud = vec![format!("{:?}", w.field_party_hud())];
    let mut pads = Vec::new();
    let mut arm_frame = None;
    let mut key_frames = Vec::new();
    let mut next_key = 0usize;
    let mut warp_frames = Vec::new();
    let mut next_warp = 0usize;
    let (mut t, mut f) = (0u32, 0u32);
    while t < sc.ticks {
        let n = cadence.ticks(f).min(sc.ticks - t);
        while let Some(&(at, code)) = sc.hotkeys.get(next_key)
            && t + 1 >= at
        {
            assert!(w.press_key(code), "native key {code}");
            key_frames.push((f, code));
            next_key += 1;
        }
        while let Some(&(at, sub_id, key)) = sc.warps.get(next_warp)
            && t + 1 >= at
        {
            // A practice game's length is `rand()`, so the seat may have
            // closed already; the leave press only means "leave", which the
            // page's `play_pack_mode_exit` does and native's toggle key would
            // otherwise read as "open".
            let leave_noop = sub_id == MODE_LEAVE
                && w.session_mut().host.world.mode
                    != legaia_engine_mode::mode::SceneMode::Expansion;
            if !leave_noop {
                assert!(w.press_key(key), "native minigame key {key}");
            }
            warp_frames.push((f, sub_id));
            next_warp += 1;
        }
        if let Some((at, idx)) = sc.arm_shop
            && arm_frame.is_none()
            && t + 1 >= at
        {
            assert!(
                w.session_mut().host.world.debug_arm_scene_shop(idx),
                "scene shop {idx} stages"
            );
            arm_frame = Some(f);
        }
        let world = &w.session().host.world;
        let pad = match sc.battle_pad {
            Some(fight) if world.mode == legaia_engine_mode::mode::SceneMode::Battle => {
                fight(world)
            }
            _ => (sc.pad)(t + 1, out.last().expect("sampled")),
        };
        pads.push(pad);
        w.display_frame(pad, n);
        t += n;
        f += 1;
        out.push(host_trace::sample_with_overlay(
            w.session(),
            w.menu_runtime(),
        ));
        hud.push(format!("{:?}", w.field_party_hud()));
    }
    NativeRun {
        audio_recent: audio_recent(w.session()),
        trace: out,
        hud,
        pads,
        arm_frame,
        key_frames,
        warp_frames,
    }
}

/// The play page's frame: `site/js/play-view-frame.js::_frame` hands the
/// frame's ticks to the one export that owns their order
/// (`LegaiaRuntime::frame_ticks`, behind `play_frame_ticks`) and keeps only
/// its own follow-up, of which the parts that reach the engine are replayed
/// here: the pad bookkeeping (a held word, and the just-pressed edges a frame
/// that runs no tick keeps), the menu's card-load take, and the draw's camera
/// read.
struct PageDriver {
    rt: LegaiaRuntime,
    prev: u16,
    /// `this.pulse`: keys pressed since the last consumer cleared them. A
    /// frame that drains no sim tick keeps them for the next one.
    pulse: u16,
    /// The page's "No encounters" checkbox.
    no_encounters: bool,
}

impl PageDriver {
    /// A hotkey press: the engine half (`play_host_hotkey`, which applies an
    /// options key itself) and the page action `site/_content/play.html`
    /// runs for the id that comes back, where that action reaches the
    /// engine.
    fn hotkey(&mut self, code: &str) {
        let id = self.rt.play_host_hotkey(code);
        assert!(!id.is_empty(), "page hotkey {code}");
        if id == "cheat_encounters" {
            // `cheatNoEncEl.click()` -> the Battle tools' `applyBattleTools`.
            self.no_encounters = !self.no_encounters;
            self.rt.set_live_battles(!self.no_encounters);
            self.rt.set_player_driven_battles(true);
        }
    }

    fn frame(&mut self, pad: u16, steps: u32) {
        self.pulse |= pad & !self.prev;
        self.prev = pad;
        let r = self
            .rt
            .frame_ticks(steps, self.pulse, pad)
            .expect("play_frame_ticks");
        if r.ticks > 0 {
            self.pulse = 0;
        }
        // `_afterMenuTicks`.
        if r.menu_ticked {
            let _ = self.rt.play_menu_take_load();
        }
        // The draw: the engine camera the page draws through, which advances
        // the cutscene glide.
        let _ = self.rt.play_camera_vp(320.0, 240.0);
        // The party readout's band test: the lead projected through the
        // follow camera (`play_field_hud_project`, which `_frame` calls on
        // every drawn frame).
        self.rt.play_field_hud_project(320.0, 240.0);
    }
}

/// The director's last commands, for a divergence report.
fn audio_recent<S: legaia_engine_audio::AudioSink>(
    s: &legaia_engine_session::BootSession<S>,
) -> Vec<String> {
    s.bgm
        .as_ref()
        .map(|d| d.commands().recent().map(|c| format!("{c:?}")).collect())
        .unwrap_or_default()
}

fn page_trace(
    disc: Vec<u8>,
    sc: &Scenario,
    cadence: Cadence,
    run: &NativeRun,
) -> (Vec<HostTraceFrame>, Vec<String>, Vec<String>) {
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(disc, String::new()).expect("load_disc");
    // Audio up from the start, as the native window's director is: the page
    // drops every command until its director exists.
    assert!(rt.debug_attach_audio(), "page audio director");
    rt.enter_field(sc.scene).expect("enter_field");
    if sc.coins > 0 {
        let _ = rt.cheat_apply("coins", sc.coins as f64, "");
    }
    let mut d = PageDriver {
        rt,
        prev: 0,
        pulse: 0,
        no_encounters: false,
    };
    let mut out = vec![host_trace::sample_with_overlay(
        d.rt.debug_session().expect("session"),
        d.rt.debug_menu_runtime(),
    )];
    let mut hud = vec![format!("{:?}", d.rt.debug_field_party_hud())];
    let (mut t, mut f) = (0u32, 0u32);
    while t < sc.ticks {
        let n = cadence.ticks(f).min(sc.ticks - t);
        if run.arm_frame == Some(f) {
            let idx = sc.arm_shop.expect("armed").1;
            assert!(
                d.rt.debug_arm_scene_shop(idx as u32),
                "page stages shop {idx}"
            );
        }
        for &(_, code) in run.key_frames.iter().filter(|(kf, _)| *kf == f) {
            d.hotkey(code);
        }
        for &(_, sub_id) in run.warp_frames.iter().filter(|(wf, _)| *wf == f) {
            // The picker's Fishing row opens the pond directly, as `L` does;
            // every other row arms the door warp.
            if sub_id == 0 {
                assert!(d.rt.play_fishing_start(), "page fishing start");
            } else if sub_id == MODE_PICKER {
                assert!(d.rt.play_pack_mode_picker(), "page mode picker");
            } else if sub_id == MODE_LEAVE {
                d.rt.play_pack_mode_exit();
            } else {
                assert!(d.rt.play_mg_debug_warp(sub_id), "page warp {sub_id}");
            }
        }
        d.frame(run.pads[f as usize], n);
        t += n;
        f += 1;
        out.push(host_trace::sample_with_overlay(
            d.rt.debug_session().expect("session"),
            d.rt.debug_menu_runtime(),
        ));
        hud.push(format!("{:?}", d.rt.debug_field_party_hud()));
    }
    let recent = audio_recent(d.rt.debug_session().expect("session"));
    (out, recent, hud)
}

/// Run `sc` at each cadence on both hosts; the lines name every channel that
/// parted, with the frame it parted on.
fn run(sc: &Scenario, cadences: &[Cadence]) -> Vec<String> {
    let Some(disc) = std::env::var("LEGAIA_DISC_BIN")
        .ok()
        .filter(|d| std::path::Path::new(d).is_file())
    else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return Vec::new();
    };
    let bytes = std::fs::read(&disc).expect("read disc");
    let mut all = Vec::new();
    for &c in cadences {
        let run = native_trace(&disc, sc, c);
        let (p, page_audio, page_hud) = page_trace(bytes.clone(), sc, c, &run);
        let label = format!("{} {:?}", sc.scene, c);
        assert_eq!(run.hud.len(), page_hud.len());
        if let Some(i) = run.hud.iter().zip(&page_hud).position(|(a, b)| a != b) {
            eprintln!(
                "[host-frame] {label}: party readout parts at frame {i}\n  native = {}\n  page   = {}",
                run.hud[i], page_hud[i]
            );
            all.push(format!("{label}: `party_hud` parts at frame {i}"));
        }
        let n = run.trace;
        assert_eq!(n.len(), p.len());
        if let Err(e) = (sc.exercised)(&n) {
            eprintln!(
                "[host-frame] {label}: vacuous ({e}); last native sample {:?}",
                n.last()
            );
            all.push(format!("{label}: native run is vacuous: {e}"));
        }
        let divs = host_trace::first_divergences(&n, &p);
        for (ch, at) in &divs {
            if let Some(i) = at {
                all.push(format!("{label}: `{ch}` parts at frame {i}"));
            }
        }
        match divs.iter().filter_map(|(_, at)| *at).min() {
            None => eprintln!(
                "[host-frame] {label}: {} frames, every channel equal",
                n.len()
            ),
            Some(i) => {
                eprintln!("[host-frame] {label}: first divergence at frame {i}");
                for (ch, at) in &divs {
                    if let Some(j) = at {
                        eprintln!("  `{ch}` from frame {j}");
                    }
                }
                if divs.iter().any(|(ch, at)| *ch == "audio" && at.is_some()) {
                    eprintln!("  native audio (last): {:?}", run.audio_recent);
                    eprintln!("  page audio (last):   {page_audio:?}");
                }
                let lo = i.saturating_sub(1);
                for k in lo..=(i + 1).min(n.len() - 1) {
                    eprintln!("  native[{k}] = {:?}\n  page[{k}]   = {:?}", n[k], p[k]);
                }
            }
        }
    }
    all
}

const ALL: &[Cadence] = &[Cadence::Hz60, Cadence::Hz30, Cadence::Hz120];

#[test]
fn town01_walk_frames_match_across_hosts() {
    let sc = Scenario {
        scene: "town01",
        pad: walk_pad,
        ticks: 300,
        exercised: moved,
        arm_shop: None,
        hotkeys: &[],
        battle_pad: None,
        warps: &[],
        coins: 0,
    };
    let parted = run(&sc, ALL);
    assert!(parted.is_empty(), "hosts diverge: {parted:#?}");
}

#[test]
fn town01_pause_menu_frames_match_across_hosts() {
    let sc = Scenario {
        scene: "town01",
        pad: menu_pad,
        ticks: 420,
        exercised: visited_menu,
        arm_shop: None,
        hotkeys: &[],
        battle_pad: None,
        warps: &[],
        coins: 0,
    };
    let parted = run(&sc, ALL);
    assert!(parted.is_empty(), "hosts diverge: {parted:#?}");
}

#[test]
fn fogged_field_frames_match_across_hosts() {
    let sc = Scenario {
        scene: "vell",
        pad: walk_pad,
        ticks: 300,
        exercised: moved,
        arm_shop: None,
        hotkeys: &[],
        battle_pad: None,
        warps: &[],
        coins: 0,
    };
    let parted = run(&sc, ALL);
    assert!(parted.is_empty(), "hosts diverge: {parted:#?}");
}

#[test]
fn encounter_and_battle_frames_match_across_hosts() {
    let sc = Scenario {
        scene: "map03",
        pad: encounter_pad,
        ticks: 1800,
        exercised: visited_battle,
        arm_shop: None,
        hotkeys: &[],
        battle_pad: None,
        warps: &[],
        coins: 0,
    };
    let parted = run(&sc, &[Cadence::Hz60, Cadence::Hz30]);
    assert!(parted.is_empty(), "hosts diverge: {parted:#?}");
}

/// Confirm every 40 ticks: skips the opening chain to `town01`, then pages
/// whatever the scene opens - the naming prompt included.
fn confirm_pad(t: u32, _: &HostTraceFrame) -> u16 {
    if t % 40 < 3 { CROSS } else { 0 }
}

/// Walk to the nearest villager and confirm at it: steer the lead toward
/// the nearest field NPC (Up = +Z, Right = +X on this scene's
/// framing), and once within reach stand and press Cross every 30 ticks.
fn talk_pad(t: u32, f: &HostTraceFrame) -> u16 {
    let Some(me) = f.actors.first() else {
        return 0;
    };
    let near = f
        .npcs
        .iter()
        .min_by_key(|n| (i32::from(n.1) - me.x).abs() + (i32::from(n.2) - me.z).abs());
    let Some(&(_, nx, nz)) = near else {
        return 0;
    };
    let (dx, dz) = (i32::from(nx) - me.x, i32::from(nz) - me.z);
    if dx.abs() + dz.abs() < 200 {
        // In reach: lean into the villager along the dominant axis (a solid
        // NPC holds the lead in place, facing it) and confirm.
        let face = if dx.abs() > dz.abs() {
            if dx > 0 { RIGHT } else { LEFT }
        } else if dz > 0 {
            UP
        } else {
            DOWN
        };
        return face | if t % 30 < 3 { CROSS } else { 0 };
    }
    let mut pad = 0;
    if dz.abs() > 40 {
        pad |= if dz > 0 { UP } else { DOWN };
    }
    if dx.abs() > 40 {
        pad |= if dx > 0 { RIGHT } else { LEFT };
    }
    pad
}

fn left_the_prologue(tr: &[HostTraceFrame]) -> Result<(), String> {
    if tr.last().and_then(|f| f.scene.as_deref()) == Some("opdeene") {
        return Err("the opening chain was never skipped".into());
    }
    if !tr.iter().any(|f| f.naming.is_some()) {
        return Err("the naming prompt never opened".into());
    }
    Ok(())
}

fn opened_a_dialogue(tr: &[HostTraceFrame]) -> Result<(), String> {
    if tr
        .iter()
        .any(|f| f.dialog.0.is_some() || f.dialog.1 || f.dialog.2.is_some())
    {
        Ok(())
    } else {
        Err("no dialogue opened".into())
    }
}

#[test]
fn prologue_skip_and_naming_frames_match_across_hosts() {
    let sc = Scenario {
        scene: "opdeene",
        pad: confirm_pad,
        ticks: 2400,
        exercised: left_the_prologue,
        arm_shop: None,
        hotkeys: &[],
        battle_pad: None,
        warps: &[],
        coins: 0,
    };
    let parted = run(&sc, ALL);
    assert!(parted.is_empty(), "hosts diverge: {parted:#?}");
}

/// [`confirm_pad`] until the naming prompt is up, then Right held for most
/// of every 120 ticks: the grid's hand steps once on the press and then on
/// the window kernel's auto-repeat (`0x14` vsyncs, then every three).
fn naming_hold_pad(t: u32, f: &HostTraceFrame) -> u16 {
    if f.naming.is_none() {
        return confirm_pad(t, f);
    }
    if (10..90).contains(&(t % 120)) {
        RIGHT
    } else {
        0
    }
}

/// The prompt's state changed far more often than the holds were pressed,
/// which only the auto-repeat does.
fn naming_hand_auto_repeated(tr: &[HostTraceFrame]) -> Result<(), String> {
    let mut states: Vec<u64> = tr.iter().filter_map(|f| f.naming).collect();
    let frames = states.len();
    states.dedup();
    let holds = frames / 120 + 1;
    if states.len() > holds * 4 {
        Ok(())
    } else {
        Err(format!(
            "{} naming states over {frames} frames ({holds} holds): no auto-repeat",
            states.len()
        ))
    }
}

/// A direction held on the naming grid repeats on the vsync clock, so a
/// 30 Hz and a 120 Hz display step the hand as far as a 60 Hz one, and the
/// page as far as the window.
#[test]
fn naming_grid_auto_repeat_frames_match_across_hosts() {
    let sc = Scenario {
        scene: "opdeene",
        pad: naming_hold_pad,
        ticks: 2400,
        exercised: naming_hand_auto_repeated,
        arm_shop: None,
        hotkeys: &[],
        battle_pad: None,
        warps: &[],
        coins: 0,
    };
    let parted = run(&sc, ALL);
    assert!(parted.is_empty(), "hosts diverge: {parted:#?}");
}

#[test]
fn village_dialogue_frames_match_across_hosts() {
    let sc = Scenario {
        scene: "town01",
        pad: talk_pad,
        ticks: 900,
        exercised: opened_a_dialogue,
        arm_shop: None,
        hotkeys: &[],
        battle_pad: None,
        warps: &[],
        coins: 0,
    };
    let parted = run(&sc, ALL);
    assert!(parted.is_empty(), "hosts diverge: {parted:#?}");
}

/// The shop's screens, pressed through: Buy, a row, the quantity and the
/// confirm, then back out.
fn shop_pad(t: u32, _: &HostTraceFrame) -> u16 {
    tap(t, 70, CROSS)
        | tap(t, 110, DOWN)
        | tap(t, 140, CROSS)
        | tap(t, 180, CROSS)
        | tap(t, 220, CROSS)
        | tap(t, 260, CIRCLE)
        | tap(t, 300, CIRCLE)
        | tap(t, 340, DOWN)
        | tap(t, 360, DOWN)
        | tap(t, 380, CROSS)
        | tap(t, 420, CROSS)
}

fn opened_the_shop(tr: &[HostTraceFrame]) -> Result<(), String> {
    if tr.iter().any(|f| f.shop.1) {
        Ok(())
    } else {
        Err("the shop never opened".into())
    }
}

#[test]
fn shop_frames_match_across_hosts() {
    let sc = Scenario {
        scene: "bylon",
        pad: shop_pad,
        ticks: 480,
        exercised: opened_the_shop,
        arm_shop: Some((30, 0)),
        hotkeys: &[],
        battle_pad: None,
        warps: &[],
        coins: 0,
    };
    let parted = run(&sc, ALL);
    assert!(parted.is_empty(), "hosts diverge: {parted:#?}");
}

/// Buy, then Down held on the stock list: one step on the press, then the
/// window kernel's auto-repeat.
fn shop_hold_pad(t: u32, _: &HostTraceFrame) -> u16 {
    let hold = if (130..250).contains(&t) { DOWN } else { 0 };
    tap(t, 70, CROSS) | hold | tap(t, 290, CIRCLE) | tap(t, 330, CIRCLE)
}

/// The list's hand moved more often than the one press steps it.
fn shop_hand_auto_repeated(tr: &[HostTraceFrame]) -> Result<(), String> {
    opened_the_shop(tr)?;
    let mut cursors: Vec<u8> = tr
        .iter()
        .filter_map(|f| f.overlay.as_ref())
        .filter(|o| o.open)
        .map(|o| o.cursor)
        .collect();
    cursors.dedup();
    if cursors.len() > 5 {
        Ok(())
    } else {
        Err(format!(
            "the shop's hand stood on {cursors:?}: no auto-repeat"
        ))
    }
}

/// A shop owns its ticks, so no field `set_pad` pumps the pad under it: the
/// tick claim does, and a held direction repeats on both hosts.
#[test]
fn shop_list_auto_repeat_frames_match_across_hosts() {
    let sc = Scenario {
        scene: "bylon",
        pad: shop_hold_pad,
        ticks: 380,
        exercised: shop_hand_auto_repeated,
        arm_shop: Some((30, 0)),
        hotkeys: &[],
        battle_pad: None,
        warps: &[],
        coins: 0,
    };
    let parted = run(&sc, ALL);
    assert!(parted.is_empty(), "hosts diverge: {parted:#?}");
}

/// Open the pause menu and hold Down on its root list.
fn menu_hold_pad(t: u32, _: &HostTraceFrame) -> u16 {
    let hold = if (130..250).contains(&t) { DOWN } else { 0 };
    tap(t, 50, START) | hold | tap(t, 290, CIRCLE)
}

/// The menu cued far more cursor steps than the one press makes: each step
/// is a command on the audio record.
fn menu_hand_auto_repeated(tr: &[HostTraceFrame]) -> Result<(), String> {
    let cues: Vec<u64> = tr
        .iter()
        .filter(|f| f.menu.0)
        .filter_map(|f| f.audio.map(|a| a.0))
        .collect();
    let (Some(first), Some(last)) = (cues.first(), cues.last()) else {
        return Err("the pause menu never opened".into());
    };
    if last - first > 8 {
        Ok(())
    } else {
        Err(format!(
            "{} cues under the pause menu: no auto-repeat",
            last - first
        ))
    }
}

/// The pause menu is a host-held screen: its lists take the auto-repeat
/// through `BootSession::pause_menu_edge` on both hosts.
#[test]
fn pause_menu_auto_repeat_frames_match_across_hosts() {
    let sc = Scenario {
        scene: "town01",
        pad: menu_hold_pad,
        ticks: 340,
        exercised: menu_hand_auto_repeated,
        arm_shop: None,
        hotkeys: &[],
        battle_pad: None,
        warps: &[],
        coins: 0,
    };
    let parted = run(&sc, ALL);
    assert!(parted.is_empty(), "hosts diverge: {parted:#?}");
}

/// The options hotkeys between walk steps: ground fog, time of day, camera
/// distance, precise movement, enhanced lighting, mute and the encounter
/// cheat, then fog and distance again. Each is one shared edit
/// (`legaia_engine_session::hotkeys`) and the per-tick knob step on both
/// hosts, so the `switches` channel must move on the same frame.
const OPTION_HOTKEYS: &[(u32, &str)] = &[
    (30, "F9"),
    (50, "F8"),
    (70, "KeyT"),
    (90, "KeyR"),
    (110, "KeyI"),
    (130, "F2"),
    (150, "F7"),
    (170, "F9"),
    (190, "KeyT"),
];

fn switches_moved(tr: &[HostTraceFrame]) -> Result<(), String> {
    let first = tr.first().map(|f| f.switches);
    let changes = tr
        .windows(2)
        .filter(|w| w[0].switches != w[1].switches)
        .count();
    if changes >= 6 {
        Ok(())
    } else {
        Err(format!(
            "the hotkeys moved the switches {changes} times (from {first:?})"
        ))
    }
}

#[test]
fn option_hotkey_frames_match_across_hosts() {
    let sc = Scenario {
        scene: "town01",
        pad: walk_pad,
        ticks: 240,
        exercised: switches_moved,
        arm_shop: None,
        hotkeys: OPTION_HOTKEYS,
        battle_pad: None,
        warps: &[],
        coins: 0,
    };
    let parted = run(&sc, ALL);
    assert!(parted.is_empty(), "hosts diverge: {parted:#?}");
}

/// Drive a live fight off its command phase: Left through the command ring
/// and its confirm prompts (the attack row), Cross on a target, Circle out of
/// an item list - the pad the disc-gated fight ladders use, pulsed.
fn fight_pad(w: &legaia_engine_core::world::World) -> u16 {
    use legaia_engine_battle_menus::battle_input::CommandPhase;
    // A held word is one press: the command screens step on edges, so the
    // drive releases every other pair of ticks.
    if w.frame % 4 >= 2 {
        return 0;
    }
    if w.battle.item_menu.is_some() {
        return CIRCLE;
    }
    match w.battle.command.as_ref().map(|c| &c.phase) {
        Some(
            CommandPhase::RoundPrompt { .. }
            | CommandPhase::Menu { .. }
            | CommandPhase::AttackMode { .. }
            | CommandPhase::CommitConfirm { .. },
        ) => LEFT,
        Some(CommandPhase::Targeting { .. }) | None => CROSS,
        Some(_) => 0,
    }
}

/// A battle opened and the run left it again: the spoils and the field
/// return, or the party wipe's game-over hand-off.
fn fought_and_left(tr: &[HostTraceFrame]) -> Result<(), String> {
    let Some(open) = tr.iter().position(|f| f.mode == "Battle") else {
        return Err("no encounter opened a battle".into());
    };
    if let Some(after) = tr[open..].iter().find(|f| f.mode != "Battle") {
        eprintln!("[host-frame] the battle handed over to {}", after.mode);
        Ok(())
    } else {
        Err("the battle never ended".into())
    }
}

#[test]
fn battle_to_exit_frames_match_across_hosts() {
    let sc = Scenario {
        scene: "map01",
        pad: encounter_pad,
        ticks: 6000,
        exercised: fought_and_left,
        arm_shop: None,
        hotkeys: &[],
        battle_pad: Some(fight_pad),
        warps: &[],
        coins: 0,
    };
    let parted = run(&sc, ALL);
    assert!(parted.is_empty(), "hosts diverge: {parted:#?}");
}

/// The play inside a minigame: the face buttons in turn (they spin, stop
/// reels, throw the duel's attacks, hit the dance's arrows), then Start,
/// which leaves whichever minigame is live (`World::poll_minigame_escape`).
fn minigame_pad(t: u32, _: &HostTraceFrame) -> u16 {
    const SQUARE: u16 = 0x8000;
    if t >= 700 {
        return tap(t, 700, START) | tap(t, 760, START);
    }
    if t < 40 || t % 16 >= 3 {
        return 0;
    }
    match (t / 16) % 3 {
        0 => CROSS,
        1 => SQUARE,
        _ => CIRCLE,
    }
}

/// The run entered the minigame `mode` names and came back out of it.
fn entered_and_left(tr: &[HostTraceFrame], mode: &str) -> Result<(), String> {
    let Some(open) = tr.iter().position(|f| f.mode == mode) else {
        return Err(format!("{mode} never opened"));
    };
    if tr[open..].iter().any(|f| f.mode != mode) {
        Ok(())
    } else {
        Err(format!("{mode} was never left"))
    }
}

fn slots_entered_and_left(tr: &[HostTraceFrame]) -> Result<(), String> {
    entered_and_left(tr, "SlotMachine")
}

fn baka_entered_and_left(tr: &[HostTraceFrame]) -> Result<(), String> {
    entered_and_left(tr, "BakaFighter")
}

fn dance_entered_and_left(tr: &[HostTraceFrame]) -> Result<(), String> {
    entered_and_left(tr, "Dance")
}

fn fishing_entered_and_left(tr: &[HostTraceFrame]) -> Result<(), String> {
    entered_and_left(tr, "Fishing")
}

fn dome_entered_and_left(tr: &[HostTraceFrame]) -> Result<(), String> {
    entered_and_left(tr, "MuscleDome")
}

/// Launch one minigame on `town01` 20 ticks in, with coins in the bank.
fn minigame_scenario(
    warp: &'static [(u32, u8, &'static str)],
    exercised: fn(&[HostTraceFrame]) -> Result<(), String>,
) -> Scenario {
    Scenario {
        scene: "town01",
        pad: minigame_pad,
        ticks: 840,
        exercised,
        arm_shop: None,
        hotkeys: &[],
        battle_pad: None,
        warps: warp,
        coins: 300,
    }
}

#[test]
fn slot_machine_frames_match_across_hosts() {
    let parted = run(
        &minigame_scenario(&[(20, 3, "KeyO")], slots_entered_and_left),
        ALL,
    );
    assert!(parted.is_empty(), "hosts diverge: {parted:#?}");
}

#[test]
fn baka_fighter_frames_match_across_hosts() {
    let parted = run(
        &minigame_scenario(&[(20, 4, "KeyB")], baka_entered_and_left),
        ALL,
    );
    assert!(parted.is_empty(), "hosts diverge: {parted:#?}");
}

#[test]
fn dance_frames_match_across_hosts() {
    let parted = run(
        &minigame_scenario(&[(20, 6, "KeyK")], dance_entered_and_left),
        ALL,
    );
    assert!(parted.is_empty(), "hosts diverge: {parted:#?}");
}

#[test]
fn fishing_frames_match_across_hosts() {
    let parted = run(
        &minigame_scenario(&[(20, 0, "KeyL")], fishing_entered_and_left),
        ALL,
    );
    assert!(parted.is_empty(), "hosts diverge: {parted:#?}");
}

#[test]
fn muscle_dome_frames_match_across_hosts() {
    let parted = run(
        &minigame_scenario(&[(20, 5, "KeyM")], dome_entered_and_left),
        ALL,
    );
    assert!(parted.is_empty(), "hosts diverge: {parted:#?}");
}

fn mode_seat_entered_and_left(tr: &[HostTraceFrame]) -> Result<(), String> {
    entered_and_left(tr, "Expansion")
}

/// The game-mode picker with no pack (the built-in practice set): open it,
/// play the first mode on the face buttons, leave the seat.
#[test]
fn game_mode_seat_frames_match_across_hosts() {
    let sc = Scenario {
        scene: "town01",
        pad: minigame_pad,
        ticks: 840,
        exercised: mode_seat_entered_and_left,
        arm_shop: None,
        hotkeys: &[],
        battle_pad: None,
        warps: &[
            (20, MODE_PICKER, "Backquote"),
            (680, MODE_LEAVE, "Backquote"),
        ],
        coins: 0,
    };
    let parted = run(&sc, ALL);
    assert!(parted.is_empty(), "hosts diverge: {parted:#?}");
}

/// Open the pause menu with Start and close it with Start, each press held
/// over four ticks as a player's key or button is.
fn start_close_pad(t: u32, _: &HostTraceFrame) -> u16 {
    tap(t, 50, START) | tap(t, 200, START)
}

/// The menu opened, closed on the second press, and stayed closed while
/// that press was still held.
fn closed_and_stayed_closed(tr: &[HostTraceFrame]) -> Result<(), String> {
    let Some(open) = tr.iter().position(|f| f.mode == "Menu") else {
        return Err("the pause menu never opened".into());
    };
    let Some(closed) = tr[open..].iter().position(|f| f.mode != "Menu") else {
        return Err("the pause menu never closed".into());
    };
    if tr[open + closed..].iter().any(|f| f.mode == "Menu") {
        return Err("the pause menu opened again after the press that closed it".into());
    }
    Ok(())
}

/// Start closes the pause menu, and the same press held over the next ticks
/// does not open it again. The native window used to leave the session's own
/// open path on (`BootSession::set_host_owns_pause_menu`), which reads the
/// world's pad edge; the menu's ticks never advance that edge, so the held
/// Start read as a fresh press and the menu reopened on the next field tick.
#[test]
fn pause_menu_start_close_frames_match_across_hosts() {
    let sc = Scenario {
        scene: "town01",
        pad: start_close_pad,
        ticks: 300,
        exercised: closed_and_stayed_closed,
        arm_shop: None,
        hotkeys: &[],
        battle_pad: None,
        warps: &[],
        coins: 0,
    };
    let parted = run(&sc, ALL);
    assert!(parted.is_empty(), "hosts diverge: {parted:#?}");
}
