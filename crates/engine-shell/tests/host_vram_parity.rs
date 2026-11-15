//! Disc-gated: the field texture image the native play window samples and
//! the one the browser play page samples are the same image after every
//! display frame, through a door, a fight and back.
//!
//! `host_frame_parity` compares what the two hosts' worlds hold. The image
//! is outside that sample, and it is where the two hosts are built
//! differently: the window keeps its own copy of the scene VRAM
//! (`cpu_vram_base`) beside its own animator and replaces both in one scene
//! rebuild, on the tick that entered the scene; the page animates the scene
//! host's image in place, keeps its CLUT-walk animator beside the host, and
//! rebuilds its render state at the end of the entry tick's tail. An
//! animator, a stamp or a restore that one host runs against the other
//! scene's image, or a tick late, changes texels and no world state.
//!
//! Both hosts run the same pad at more than one display rate, the native
//! run's words replayed to the page. The image is compared on every frame
//! outside a fight (each host stages a fight's image its own way): the
//! window's `cpu_vram_base` against the scene host's resources, which is the
//! image the page uploads.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_core::world::World;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_shell::window::HeadlessPlayWindow;
use legaia_web_play::runtime::LegaiaRuntime;

const UP: u16 = 0x0010;
const RIGHT: u16 = 0x0020;
const DOWN: u16 = 0x0040;
const LEFT: u16 = 0x0080;
const CROSS: u16 = 0x4000;
const CIRCLE: u16 = 0x2000;

struct Scenario {
    scene: &'static str,
    /// Standing ticks ahead of the seat: where a scene's walkers stand on
    /// the tick a door is crossed depends on it.
    idle: u32,
    /// Where the lead is put after the idle ticks (`World::debug_seat_player`).
    seat: Option<(i16, i16)>,
    /// Pad word for the display frame whose first sim tick is the given one
    /// (1-based, counted from the seat), read off the native world.
    pad: fn(u32, &World) -> u16,
    /// Sim ticks after the seat.
    ticks: u32,
    /// What the native run must have done, so a green run is not one that
    /// stood outside the door.
    exercised: Exercised,
    cadences: &'static [&'static [u32]],
}

type Exercised = fn(&[Frame]) -> Result<(), String>;

/// Sim ticks per display frame, cycled: 60 Hz, 30 Hz and a 120 Hz display.
const ALL: &[&[u32]] = &[&[1], &[2], &[0, 1]];

struct Frame {
    scene: Option<String>,
    mode: SceneMode,
    /// One hash per VRAM row.
    rows: Vec<u64>,
}

fn row_hashes(vram: &[u8]) -> Vec<u64> {
    vram.as_chunks::<2048>()
        .0
        .iter()
        .map(|row| {
            row.as_chunks::<8>()
                .0
                .iter()
                .fold(0xcbf2_9ce4_8422_2325, |h, w| {
                    (h ^ u64::from_le_bytes(*w))
                        .wrapping_mul(0x0000_0100_0000_01b3)
                        .rotate_left(23)
                })
        })
        .collect()
}

fn native_run(disc: &std::path::Path, sc: &Scenario, cadence: &[u32]) -> (Vec<Frame>, Vec<u16>) {
    let mut w = HeadlessPlayWindow::open(disc, sc.scene).expect("open the window");
    let sample = |w: &HeadlessPlayWindow| Frame {
        scene: w.session().host.scene.as_ref().map(|s| s.name.clone()),
        mode: w.session().host.world.mode,
        rows: w
            .field_vram()
            .map(|v| row_hashes(v.as_bytes()))
            .unwrap_or_default(),
    };
    for _ in 0..sc.idle {
        w.display_frame(0, 1);
    }
    if let Some((x, z)) = sc.seat {
        assert!(
            w.session_mut().host.world.debug_seat_player(x, z),
            "native: a player to seat"
        );
    }
    let mut out = vec![sample(&w)];
    let mut pads = Vec::new();
    let (mut t, mut f) = (0u32, 0usize);
    while t < sc.ticks {
        let n = cadence[f % cadence.len()].min(sc.ticks - t);
        let pad = (sc.pad)(t + 1, &w.session().host.world);
        pads.push(pad);
        w.display_frame(pad, n);
        t += n;
        f += 1;
        out.push(sample(&w));
    }
    (out, pads)
}

fn page_run(disc: Vec<u8>, sc: &Scenario, cadence: &[u32], pads: &[u16]) -> Vec<Frame> {
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(disc, String::new()).expect("load_disc");
    assert!(rt.debug_attach_audio(), "page audio director");
    rt.enter_field(sc.scene).expect("enter_field");
    let sample = |rt: &LegaiaRuntime| {
        let s = rt.debug_session().expect("session");
        Frame {
            scene: s.host.scene.as_ref().map(|s| s.name.clone()),
            mode: s.host.world.mode,
            rows: s
                .host
                .resources
                .as_ref()
                .map(|r| row_hashes(r.vram.as_bytes()))
                .unwrap_or_default(),
        }
    };
    // `site/js/play-view-frame.js::_frame`, as `host_frame_parity` replays
    // it: the frame's ticks, then the draw's camera read.
    let (mut prev, mut pulse) = (0u16, 0u16);
    let mut frame = |rt: &mut LegaiaRuntime, pad: u16, steps: u32| {
        pulse |= pad & !prev;
        prev = pad;
        let r = rt.frame_ticks(steps, pulse, pad).expect("play_frame_ticks");
        if r.ticks > 0 {
            pulse = 0;
        }
        let _ = rt.play_camera_vp(320.0, 240.0);
        rt.play_field_hud_project(320.0, 240.0);
    };
    for _ in 0..sc.idle {
        frame(&mut rt, 0, 1);
    }
    if let Some((x, z)) = sc.seat {
        assert!(rt.debug_seat_player(x, z), "page: a player to seat");
    }
    let mut out = vec![sample(&rt)];
    let (mut t, mut f) = (0u32, 0usize);
    while t < sc.ticks {
        let n = cadence[f % cadence.len()].min(sc.ticks - t);
        frame(&mut rt, pads[f], n);
        t += n;
        f += 1;
        out.push(sample(&rt));
    }
    out
}

/// Run `sc` on both hosts at each of its cadences; a line per cadence whose
/// images part.
fn run(sc: &Scenario) -> Vec<String> {
    let Some(disc) = std::env::var("LEGAIA_DISC_BIN")
        .ok()
        .filter(|d| std::path::Path::new(d).is_file())
    else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return Vec::new();
    };
    let bytes = std::fs::read(&disc).expect("read disc");
    let mut parted = Vec::new();
    for cadence in sc.cadences {
        let label = format!("{} idle {} cadence {cadence:?}", sc.scene, sc.idle);
        let (n, pads) = native_run(std::path::Path::new(&disc), sc, cadence);
        let p = page_run(bytes.clone(), sc, cadence, &pads);
        assert_eq!(n.len(), p.len());
        if let Err(e) = (sc.exercised)(&n) {
            parted.push(format!("{label}: the native run is vacuous: {e}"));
        }
        let mut compared = 0usize;
        let mut bad: Vec<String> = Vec::new();
        for (i, (a, b)) in n.iter().zip(&p).enumerate() {
            if a.scene != b.scene || a.mode != b.mode {
                bad.push(format!(
                    "frame {i}: native {:?} {:?}, page {:?} {:?}",
                    a.scene, a.mode, b.scene, b.mode
                ));
                break;
            }
            if a.mode == SceneMode::Battle {
                continue;
            }
            compared += 1;
            let rows: Vec<usize> = (0..a.rows.len().max(b.rows.len()))
                .filter(|&y| a.rows.get(y) != b.rows.get(y))
                .collect();
            if !rows.is_empty() {
                bad.push(format!(
                    "frame {i} ({:?}, {:?}): VRAM rows {rows:?} differ",
                    a.scene, a.mode
                ));
                if bad.len() >= 8 {
                    break;
                }
            }
        }
        eprintln!(
            "[host-vram] {label}: {} frames, {compared} compared, {} differing",
            n.len(),
            bad.len()
        );
        for line in &bad {
            eprintln!("  {line}");
        }
        if let Some(first) = bad.first() {
            parted.push(format!("{label}: {first}"));
        }
    }
    parted
}

fn stand(_: u32, _: &World) -> u16 {
    0
}

fn walk_down(_: u32, _: &World) -> u16 {
    DOWN
}

fn walk_up(_: u32, _: &World) -> u16 {
    UP
}

fn stayed(tr: &[Frame]) -> Result<(), String> {
    let scenes = scene_runs(tr);
    (scenes.len() == 1)
        .then_some(())
        .ok_or_else(|| format!("left the scene: {scenes:?}"))
}

/// The scenes the run stood in, in order.
fn scene_runs(tr: &[Frame]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for f in tr {
        let name = f.scene.clone().unwrap_or_default();
        if out.last() != Some(&name) {
            out.push(name);
        }
    }
    out
}

/// The world map's CLUT walkers, stood under for long enough that every
/// hold counter of theirs wraps.
#[test]
fn world_map_image_matches_across_hosts() {
    let sc = Scenario {
        scene: "map01",
        idle: 0,
        seat: None,
        pad: stand,
        ticks: 300,
        exercised: stayed,
        cadences: ALL,
    };
    let parted = run(&sc);
    assert!(parted.is_empty(), "hosts' images part: {parted:#?}");
}

/// A scene with ambient palette cyclers (`jou`'s pulsating flesh).
#[test]
fn ambient_cycler_image_matches_across_hosts() {
    let sc = Scenario {
        scene: "jou",
        idle: 0,
        seat: None,
        pad: stand,
        ticks: 300,
        exercised: stayed,
        cadences: ALL,
    };
    let parted = run(&sc);
    assert!(parted.is_empty(), "hosts' images part: {parted:#?}");
}

fn reached_rim_elm(tr: &[Frame]) -> Result<(), String> {
    let scenes = scene_runs(tr);
    (scenes == ["map01", "town0c"])
        .then_some(())
        .ok_or_else(|| format!("stood in {scenes:?}"))
}

fn reached_hunters_spring(tr: &[Frame]) -> Result<(), String> {
    let scenes = scene_runs(tr);
    (scenes == ["map01", "izumi"])
        .then_some(())
        .ok_or_else(|| format!("stood in {scenes:?}"))
}

/// A door off `map01` at every idle offset of one cycle of the overworld
/// walker that writes the entered scene's ground palette cell (it comes due
/// once in 21 ticks, and a walker that outlives the door only shows when the
/// entry tick is one of them), past the arrival, whose offsets share one
/// entry tick. The cycle runs at 60 Hz; offset `0` runs at every cadence.
fn door_parts(seat: (i16, i16), pad: fn(u32, &World) -> u16, exercised: Exercised) -> Vec<String> {
    let mut parted = Vec::new();
    for idle in std::iter::once(0).chain(60..81) {
        let sc = Scenario {
            scene: "map01",
            idle,
            seat: Some(seat),
            pad,
            ticks: 150,
            exercised,
            cadences: if idle == 0 { ALL } else { &[&[1]] },
        };
        parted.extend(run(&sc));
    }
    parted
}

/// Off the world map into Rim Elm (`town0c`), one step north of the
/// entrance tile: the overworld's walker must not outlive the door on
/// either host.
#[test]
fn rim_elm_door_image_matches_across_hosts() {
    let parted = door_parts((12362, 3218), walk_down, reached_rim_elm);
    assert!(parted.is_empty(), "hosts' images part: {parted:#?}");
}

/// Off the world map into Hunter's Spring (`izumi`), from south of its
/// pond.
#[test]
fn hunters_spring_door_image_matches_across_hosts() {
    let parted = door_parts((8960, 5060), walk_up, reached_hunters_spring);
    assert!(parted.is_empty(), "hosts' images part: {parted:#?}");
}

/// Up and down across `map03`'s Buma entrance.
fn pace_pad(t: u32, _: &World) -> u16 {
    if (t / 60).is_multiple_of(2) { UP } else { DOWN }
}

fn crossed_bumas_door_both_ways(tr: &[Frame]) -> Result<(), String> {
    let scenes = scene_runs(tr);
    scenes
        .windows(3)
        .any(|w| w == ["map03", "bubu2", "map03"])
        .then_some(())
        .ok_or_else(|| format!("stood in {scenes:?}"))
}

/// Into frozen Buma (`bubu2`) and back out onto the world map. `bubu2`
/// ships its own palettes for the party on CLUT row 478, under the cells
/// the field-character atlas would write: a host that layers the atlas over
/// its scene build draws the party there on the pack's palettes. The way
/// back enters a scene with a walker of its own, which starts on the entry
/// tick on both hosts.
#[test]
fn buma_door_round_trip_image_matches_across_hosts() {
    let sc = Scenario {
        scene: "map03",
        idle: 0,
        seat: None,
        pad: pace_pad,
        ticks: 600,
        exercised: crossed_bumas_door_both_ways,
        cadences: ALL,
    };
    let parted = run(&sc);
    assert!(parted.is_empty(), "hosts' images part: {parted:#?}");
}

/// Walk a box on an encounter field and drive whatever fight opens off its
/// command phase (`host_frame_parity`'s pad).
fn fight_pad(t: u32, w: &World) -> u16 {
    use legaia_engine_battle_menus::battle_input::CommandPhase;
    if w.mode != SceneMode::Battle {
        let dir = match (t / 90) % 4 {
            0 => UP,
            1 => RIGHT,
            2 => DOWN,
            _ => LEFT,
        };
        return dir | if t % 24 < 3 { CROSS } else { 0 };
    }
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

fn fought_and_returned(tr: &[Frame]) -> Result<(), String> {
    let Some(open) = tr.iter().position(|f| f.mode == SceneMode::Battle) else {
        return Err("no encounter opened a battle".into());
    };
    let after = tr[open..]
        .iter()
        .filter(|f| f.mode != SceneMode::Battle)
        .count();
    (after >= 60)
        .then_some(())
        .ok_or_else(|| format!("{after} frames after the fight"))
}

/// A fight and the field after it: each host puts its field image back on
/// the way out, and the walkers carry on from where the fight held them.
#[test]
fn field_image_after_a_fight_matches_across_hosts() {
    let sc = Scenario {
        scene: "map01",
        idle: 0,
        seat: None,
        pad: fight_pad,
        ticks: 6000,
        exercised: fought_and_returned,
        cadences: &[&[1], &[2]],
    };
    let parted = run(&sc);
    assert!(parted.is_empty(), "hosts' images part: {parted:#?}");
}
