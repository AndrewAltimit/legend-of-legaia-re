//! Disc-gated: the native play window's **own frame body** and the browser
//! play page's, driven by one pad script, leave the same world state.
//!
//! `field_walk_host_parity` and `mode_seat_host_parity` drive the native
//! side through `NativeSession`, which is the session both hosts already
//! share. What a host wraps around the session tick - its per-tick modal
//! arms, the steps before and after the tick, the draw path's world-moving
//! steps - is where the two drift. So this drives the window itself
//! (`window::HeadlessPlayWindow`: the app `play-window` assembles, minus the
//! renderer) against the page's `LegaiaRuntime` the way
//! `site/js/play-view-frame.js` and `play-view-input.js` drive it (the pause
//! menu, naming prompt and shop arms, the prologue skip, the cutscene pad
//! lock, the modal break), and compares a host-agnostic sample of the session
//! (`legaia_engine_session::host_trace`) after every display frame, channel
//! by channel.
//!
//! Each scenario runs at more than one display rate. A display frame that
//! runs two sim ticks (a 30 Hz display) or none (a redraw between two ticks
//! on a fast display) is where a step one host runs per redraw and the other
//! per tick parts them; a 60 Hz run cannot see it.
//!
//! What the comparison cannot see: anything a renderer owns (both drivers
//! draw nothing), the audio director (the headless window runs with audio
//! off), and the page's JavaScript beyond the arms replayed in
//! [`PageDriver::frame`] - a change to those JS arms has to be mirrored
//! there.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_session::host_trace::{self, HostTraceFrame};
use legaia_engine_shell::window::HeadlessPlayWindow;
use legaia_engine_system::input::PadButton;
use legaia_web_viewer::runtime::LegaiaRuntime;

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
}

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
    /// The director's last commands, for a divergence report.
    audio_recent: Vec<String>,
    pads: Vec<u16>,
    arm_frame: Option<u32>,
}

fn native_trace(disc: &str, sc: &Scenario, cadence: Cadence) -> NativeRun {
    let mut w = HeadlessPlayWindow::open(std::path::Path::new(disc), sc.scene).expect("open");
    let mut out = vec![host_trace::sample_with_overlay(
        w.session(),
        w.menu_runtime(),
    )];
    let mut pads = Vec::new();
    let mut arm_frame = None;
    let (mut t, mut f) = (0u32, 0u32);
    while t < sc.ticks {
        let n = cadence.ticks(f).min(sc.ticks - t);
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
        let pad = (sc.pad)(t + 1, out.last().expect("sampled"));
        pads.push(pad);
        w.display_frame(pad, n);
        t += n;
        f += 1;
        out.push(host_trace::sample_with_overlay(
            w.session(),
            w.menu_runtime(),
        ));
    }
    NativeRun {
        audio_recent: audio_recent(w.session()),
        trace: out,
        pads,
        arm_frame,
    }
}

/// The play page's frame, replayed from `site/js/play-view-frame.js::_frame`
/// and the modal steps in `play-view-input.js`, with the JS pad bookkeeping
/// reduced to a held word and this frame's just-pressed edges.
struct PageDriver {
    rt: LegaiaRuntime,
    prev: u16,
    /// `this.pulse`: keys pressed since the last consumer cleared them. A
    /// frame that drains no sim tick keeps them for the next one.
    pulse: u16,
    /// `this._cut`: the post-tick re-read of `play_cutscene_state_json`.
    cut_locked: bool,
    cut_chain: bool,
}

impl PageDriver {
    fn read_cut(&mut self) {
        let v: serde_json::Value =
            serde_json::from_str(&self.rt.play_cutscene_state_json()).unwrap_or_default();
        self.cut_locked = v["locked"].as_bool().unwrap_or(false);
        self.cut_chain = v["chain"].as_bool().unwrap_or(false);
    }

    fn frame(&mut self, pad: u16, steps: u32) {
        self.pulse |= pad & !self.prev;
        self.prev = pad;
        let mut pulse = self.pulse;
        // `_updateFieldMenu` / `_updateNameEntry` / `_updateFieldShop`: with
        // the per-tick exports the frame level only stages blitters; the
        // screens' ticks run in the loop.
        {
            let mut menu_ticked = false;
            for _ in 0..steps {
                // `play_menu_tick`: the pause menu's per-tick arms.
                if self.rt.play_menu_tick(pulse) != 0 {
                    menu_ticked = true;
                    pulse = 0;
                    self.rt.play_tick_overlay_sfx();
                    continue;
                }
                // `play_name_entry_tick`: the naming prompt's per-tick arm.
                if self.rt.play_name_entry_tick(pulse) != 0 {
                    pulse = 0;
                    continue;
                }
                if self.cut_chain && pulse & CROSS != 0 {
                    let target = self.rt.play_take_prologue_handoff(true);
                    if !target.is_empty() {
                        self.rt.enter_field(&target).expect("prologue hand-off");
                        pulse = 0;
                        self.read_cut();
                        continue;
                    }
                }
                if self.rt.is_game_over() {
                    let _ = self.rt.game_over_input(pulse);
                    break;
                }
                // `play_shop_tick`: a suspending screen takes the tick; the
                // inn holds the pad neutral and steps after it.
                let tick_edge = pulse;
                let shop = self.rt.play_shop_tick(tick_edge);
                if shop == 1 {
                    pulse = 0;
                    self.rt.play_tick_overlay_sfx();
                    continue;
                }
                let word = if self.cut_locked || shop == 2 { 0 } else { pad };
                self.rt.set_pad(word);
                pulse = 0;
                self.rt.tick_frame().expect("tick_frame");
                self.rt.play_shop_after_tick(tick_edge);
                self.read_cut();
            }
            // `_afterMenuTicks`.
            if menu_ticked {
                let _ = self.rt.play_menu_take_load();
            }
        }
        self.pulse = pulse;
        // The draw: the engine camera the page draws through, which advances
        // the cutscene glide.
        let _ = self.rt.play_camera_vp(320.0, 240.0);
    }
}

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
) -> (Vec<HostTraceFrame>, Vec<String>) {
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(disc, String::new()).expect("load_disc");
    // Audio up from the start, as the native window's director is: the page
    // drops every command until its director exists.
    assert!(rt.debug_attach_audio(), "page audio director");
    rt.enter_field(sc.scene).expect("enter_field");
    let mut d = PageDriver {
        rt,
        prev: 0,
        pulse: 0,
        cut_locked: false,
        cut_chain: false,
    };
    d.read_cut();
    let mut out = vec![host_trace::sample_with_overlay(
        d.rt.debug_session().expect("session"),
        d.rt.debug_menu_runtime(),
    )];
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
        d.frame(run.pads[f as usize], n);
        t += n;
        f += 1;
        out.push(host_trace::sample_with_overlay(
            d.rt.debug_session().expect("session"),
            d.rt.debug_menu_runtime(),
        ));
    }
    let recent = audio_recent(d.rt.debug_session().expect("session"));
    (out, recent)
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
        let (p, page_audio) = page_trace(bytes.clone(), sc, c, &run);
        let n = run.trace;
        assert_eq!(n.len(), p.len());
        let label = format!("{} {:?}", sc.scene, c);
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
    };
    let parted = run(&sc, ALL);
    assert!(parted.is_empty(), "hosts diverge: {parted:#?}");
}
