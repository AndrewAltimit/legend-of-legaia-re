//! Soak / softlock harness: seeded, game-shaped pseudo-random pad input over
//! every playable scene, with automatic failure detectors.
//!
//! Every other progression instrument in this crate walks a route someone
//! chose (`critical_path_replay`, `menu_replay`, `minigame_replay`) or scores
//! one rung per scene (`chapter1_frontier_ladder`). A scripted ladder never
//! steps on the input a player produces by accident - the confirm pressed
//! during a door fade, the menu opened on the frame a battle starts, the
//! direction held into a shop picker. This harness produces that input on
//! purpose and watches for the engine falling over.
//!
//! See `docs/tooling/soak-harness.md` for the policies, the detectors, how to
//! run a long soak, and how to reproduce a finding. In short:
//!
//! - **Driver.** One fresh [`NativeSession`] per run (scene, seed): new-game
//!   party, live loop and player-driven battles armed, an empty card rack
//!   mounted, then `frames` ticks of pad input from a seeded [`Policy`]. The
//!   only actuator is `World::set_pad`; FMVs are skipped through the shared
//!   hand-off kernel, as the headless `play` subcommand does.
//! - **Detectors.** Panic (per-tick `catch_unwind`), `tick()` error, unknown
//!   scene id, softlock (the progress digest frozen for a window while input
//!   varies), a pause menu that will not close, a battle that never ends, a
//!   drop to `Title`, non-finite camera floats, absurd values (HP / MP over
//!   max, money, bag invariants), unbounded queue growth, and a hung tick
//!   (wall-clock watchdog).
//! - **Output.** `target/soak/<tag>/report.md` plus one `j-replay-v1` file per
//!   distinct finding signature (pad stream only), truncated to the finding
//!   frame and confirmed to reproduce.
//!
//! Tests:
//!
//! - `soak_smoke_no_panics` - a small fixed budget (a few scenes x one seed x
//!   a few hundred frames) that asserts no panic, tick error or hang. The
//!   CI-shaped gate.
//! - `soak_long` - opt-in, runs only when `LEGAIA_SOAK_SEEDS` or
//!   `LEGAIA_SOAK_FRAMES` is set. Report-only unless `LEGAIA_SOAK_STRICT=1`.
//! - `soak_replay` - opt-in, runs only when `LEGAIA_SOAK_REPLAY=<file>` is set:
//!   replays one saved finding and prints what the detectors see.
//! - `front_end_new_game_reaches_free_roam` /
//!   `front_end_continue_lands_a_library_save` - the boot front end
//!   (logos, title, New Game through the naming prompt; Continue through the
//!   save-select) on the shared `engine-session` front end. A run labelled
//!   `title` / `title+continue[@save]` soaks from the same entry leg.
//! - `soak_fixtures` - replays every committed fixture under
//!   `scripts/replays/soak/` and reports whether each still reproduces.
//!
//! Skip-pass (CLAUDE.md disc-gated convention): `LEGAIA_DISC_BIN` unset. The
//! assets come from `LEGAIA_EXTRACTED_DIR`, else `<repo>/extracted`, else
//! straight from the disc image.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::hash::Hasher;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, Instant};

use legaia_engine_menu_runtime::menu_runtime::MenuRuntime;
use legaia_engine_menus::save_screen::card_port_snapshot;
use legaia_engine_menus::save_select::{SaveRack, SlotSnapshot};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene::scene::{Scene, is_world_map_scene};
use legaia_engine_scene_host::scene_host::SceneTickEvent;
use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_engine_shell::replay::{ReplayFile, ReplayMeta};
use legaia_engine_system::input::PadButton;

mod driver;
mod environment;
mod findings;
mod front_end;
mod policy;
mod progress_trace;
mod replay_files;
mod report;
mod run;
mod save_round_trip;
mod scene_set;
mod talk_visit;

use driver::*;
use environment::*;
use findings::*;
use front_end::*;
use policy::*;
use progress_trace::*;
use replay_files::*;
use report::*;
use run::*;
use save_round_trip::*;
use scene_set::*;
use talk_visit::*;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Scenes the fixed-budget smoke run covers: the cold-boot town, the
/// chapter-1 overworld, a dungeon, the casino floor (minigame doors) and a
/// late-game dungeon, plus one save / load round-trip run.
const SMOKE_SCENES: [&str; 6] = ["town01", "map01", "keikoku", "koin1", "jou", "town01+rt"];
const SMOKE_FRAMES: u64 = 900;

#[test]
fn soak_smoke_no_panics() {
    let Some(src) = source() else { return };
    let probe = open_session(&src);
    let all = scene_set(&probe);
    drop(probe);
    let known: BTreeSet<String> = all.iter().cloned().collect();
    let scenes: Vec<String> = SMOKE_SCENES
        .iter()
        .chain(MINIGAME_RUNS.iter())
        .filter(|s| known.contains(split_minigame(split_round_trip(s).0).0))
        .map(|s| s.to_string())
        .collect();
    assert!(
        !scenes.is_empty(),
        "no smoke scene resolves - scene_set broken"
    );
    let cfg = SoakConfig {
        tag: "smoke".into(),
        scenes,
        seeds: vec![1],
        frames: SMOKE_FRAMES,
        jobs: default_jobs(),
        confirm: false,
    };
    eprintln!("[soak-smoke] scenes: {}", all.join(" "));
    eprintln!(
        "[soak-smoke] playable scene set = {} scenes; running {:?} x seed 1 x {} frames from {:?}",
        all.len(),
        cfg.scenes,
        cfg.frames,
        src
    );
    let r = soak(&src, &known, &cfg);
    let fatal: Vec<String> = r
        .outcomes
        .iter()
        .flat_map(|o| o.findings.iter())
        .filter(|f| matches!(f.detector, "panic" | "tick_error"))
        .map(|f| {
            format!(
                "{} (start {}, frame {}): {}",
                f.signature(),
                f.start_scene,
                f.frame,
                f.detail
            )
        })
        .collect();
    assert!(r.hangs.is_empty(), "hung ticks: {:?}", r.hangs);
    assert!(
        fatal.is_empty(),
        "soak smoke hit panics:\n{}",
        fatal.join("\n")
    );
}

#[test]
fn soak_long() {
    if std::env::var_os("LEGAIA_SOAK_SEEDS").is_none()
        && std::env::var_os("LEGAIA_SOAK_FRAMES").is_none()
    {
        eprintln!("[skip] soak_long: set LEGAIA_SOAK_SEEDS / LEGAIA_SOAK_FRAMES to run");
        return;
    }
    let Some(src) = source() else { return };
    let probe = open_session(&src);
    let all = scene_set(&probe);
    drop(probe);
    let known: BTreeSet<String> = all.iter().cloned().collect();
    let shops = if std::env::var_os("LEGAIA_SOAK_SCENES").is_some() {
        Vec::new()
    } else {
        shop_scenes(&src, &all)
    };
    let scenes = filter_scenes(&all, &shops);
    let cfg = SoakConfig {
        tag: std::env::var("LEGAIA_SOAK_TAG").unwrap_or_else(|_| "long".into()),
        scenes,
        seeds: seed_list(1),
        frames: env_u64("LEGAIA_SOAK_FRAMES").unwrap_or(3600),
        jobs: default_jobs(),
        confirm: !env_flag("LEGAIA_SOAK_NO_CONFIRM"),
    };
    eprintln!(
        "[soak-long] {} scenes x {} seeds x {} frames, {} jobs, source {:?}",
        cfg.scenes.len(),
        cfg.seeds.len(),
        cfg.frames,
        cfg.jobs,
        src
    );
    let r = soak(&src, &known, &cfg);
    if env_flag("LEGAIA_SOAK_STRICT") {
        let fatal = r
            .outcomes
            .iter()
            .flat_map(|o| o.findings.iter())
            .filter(|f| f.rank() == 0)
            .count();
        assert_eq!(fatal, 0, "strict soak: {fatal} rank-0 findings");
    }
}

#[test]
fn soak_replay() {
    let Some(path) = std::env::var_os("LEGAIA_SOAK_REPLAY") else {
        eprintln!("[skip] soak_replay: set LEGAIA_SOAK_REPLAY=<file or dir>");
        return;
    };
    let Some(src) = source() else { return };
    install_panic_hook();
    let probe = open_session(&src);
    let known: BTreeSet<String> = scene_set(&probe).into_iter().collect();
    drop(probe);
    for file in replay_files(Path::new(&path)) {
        let lr = load_replay(&file);
        eprintln!(
            "[soak-replay] {} ({} frames, scene {})",
            file.display(),
            lr.spec.frames,
            lr.spec.scene
        );
        let t = lr.tunables();
        let (spec, pads) = (lr.spec, lr.pads);
        let (src, known) = (src.clone(), known.clone());
        let h = std::thread::Builder::new()
            .name("soak-replay".into())
            .stack_size(64 << 20)
            .spawn(move || {
                let r = run_one(&src, &known, &spec, Some(&pads), &t, None);
                (r.findings, r.stats)
            })
            .unwrap();
        let (findings, stats) = h.join().expect("replay thread");
        eprintln!("[soak-replay] stats: {stats:?}");
        for f in &findings {
            eprintln!(
                "[soak-replay] finding {} at frame {}: {}",
                f.signature(),
                f.frame,
                f.detail
            );
        }
        if let Some(sig) = lr.signature {
            let hit = findings.iter().any(|f| f.signature() == sig);
            eprintln!("[soak-replay] recorded signature {sig:?} reproduced: {hit}");
        }
    }
}

/// Reduce a saved replay again: `LEGAIA_SOAK_REDUCE=<file>` replays it, picks
/// the finding `LEGAIA_SOAK_REDUCE_SIG` names (default: the file's recorded
/// signature), runs the minimiser on it under `LEGAIA_SOAK_MINIMIZE`, and
/// writes the result to `LEGAIA_SOAK_REDUCE_OUT` (default `<file>.reduced`).
/// For a replay whose first reduction ran out of budget, or one that should
/// pin a different finding of the same run.
#[test]
fn soak_reduce() {
    let Some(path) = std::env::var_os("LEGAIA_SOAK_REDUCE").map(PathBuf::from) else {
        eprintln!("[skip] soak_reduce: set LEGAIA_SOAK_REDUCE=<file>");
        return;
    };
    let Some(src) = source() else { return };
    install_panic_hook();
    let probe = open_session(&src);
    let known: BTreeSet<String> = scene_set(&probe).into_iter().collect();
    drop(probe);
    let lr = load_replay(&path);
    let want = std::env::var("LEGAIA_SOAK_REDUCE_SIG")
        .ok()
        .or(lr.signature.clone())
        .expect("a signature to reduce towards");
    let t = lr.tunables();
    let arming = t.arming;
    let (spec, pads) = (lr.spec, lr.pads);
    let outcome = {
        let (src, known) = (src.clone(), known.clone());
        std::thread::Builder::new()
            .name("soak-reduce".into())
            .stack_size(64 << 20)
            .spawn(move || run_one(&src, &known, &spec, Some(&pads), &t, None))
            .unwrap()
            .join()
            .expect("reduce thread")
    };
    let Some(f) = outcome.findings.iter().find(|f| f.signature() == want) else {
        panic!(
            "{} does not raise `{want}`; it raises {:?}",
            path.display(),
            outcome
                .findings
                .iter()
                .map(|f| f.signature())
                .collect::<Vec<_>>()
        );
    };
    let (ok, reduced, relaxed) = confirm(&src, &known, &outcome, f);
    assert!(ok, "`{want}` did not reproduce from its own pads");
    let out = std::env::var_os("LEGAIA_SOAK_REDUCE_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| path.with_extension("reduced"));
    let spec = RunSpec {
        frames: reduced.len() as u64,
        ..outcome.spec.clone()
    };
    std::fs::write(&out, replay_text(&spec, &reduced, Some(f), relaxed, arming))
        .expect("write replay");
    eprintln!(
        "[soak-reduce] `{want}`: {} frames, {} pad changes -> {}",
        reduced.len(),
        reduced.windows(2).filter(|w| w[0] != w[1]).count(),
        out.display()
    );
}

/// A replay path, or every `*.replay.toml` directly inside a directory.
fn replay_files(path: &Path) -> Vec<PathBuf> {
    if !path.is_dir() {
        return vec![path.to_path_buf()];
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(path)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.to_string_lossy().ends_with(".replay.toml"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

#[test]
fn soak_fixtures() {
    let dir = repo_root().join("scripts/replays/soak");
    let files = if dir.is_dir() {
        replay_files(&dir)
    } else {
        Vec::new()
    };
    // Disc-free half: every committed fixture parses and carries a scene.
    for f in &files {
        let lr = load_replay(f);
        assert!(
            !lr.spec.scene.is_empty(),
            "{} has no soak-scene",
            f.display()
        );
        assert_eq!(lr.pads.len() as u64, lr.spec.frames, "{}", f.display());
    }
    let Some(src) = source() else { return };
    install_panic_hook();
    let probe = open_session(&src);
    let known: BTreeSet<String> = scene_set(&probe).into_iter().collect();
    drop(probe);
    for f in files {
        let lr = load_replay(&f);
        let t = lr.tunables();
        let (src, known) = (src.clone(), known.clone());
        let (spec, pads) = (lr.spec, lr.pads);
        let findings = std::thread::Builder::new()
            .name("soak-fixture".into())
            .stack_size(64 << 20)
            .spawn(move || run_one(&src, &known, &spec, Some(&pads), &t, None).findings)
            .unwrap()
            .join()
            .expect("fixture thread");
        if let Some(e) = card_save_error(&findings) {
            eprintln!(
                "[soak-fixture] {} -> DID NOT RUN: {e}",
                f.file_name().unwrap().to_string_lossy()
            );
            continue;
        }
        let hit = lr
            .signature
            .as_ref()
            .is_some_and(|sig| findings.iter().any(|g| &g.signature() == sig));
        eprintln!(
            "[soak-fixture] {} -> {}",
            f.file_name().unwrap().to_string_lossy(),
            if hit {
                "still reproduces"
            } else {
                "NO LONGER REPRODUCES (fixed? delete the fixture)"
            }
        );
    }
}

/// Upper bound on the battles a fixed-finding replay may open
/// (`# soak-expect-max-battles = N`): a loop that stops *signing* as a
/// `battle_loop` could still fight twice.
const MAX_BATTLES_TAG: &str = "# soak-expect-max-battles = ";

/// The regression half of the fixture set: `scripts/replays/soak/fixed/`
/// holds replays of findings a fix closed. Each must replay WITHOUT its
/// recorded signature (and within its battle bound) - a hard failure, unlike
/// [`soak_fixtures`], which only reports.
#[test]
fn soak_fixed_fixtures() {
    let dir = repo_root().join("scripts/replays/soak/fixed");
    let files = if dir.is_dir() {
        replay_files(&dir)
    } else {
        Vec::new()
    };
    for f in &files {
        let lr = load_replay(f);
        assert!(
            lr.signature.is_some(),
            "{} has no soak-signature",
            f.display()
        );
        assert_eq!(lr.pads.len() as u64, lr.spec.frames, "{}", f.display());
    }
    let Some(src) = source() else { return };
    install_panic_hook();
    let probe = open_session(&src);
    let known: BTreeSet<String> = scene_set(&probe).into_iter().collect();
    drop(probe);
    let mut skipped: Vec<String> = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(&f).expect("read replay");
        let max_battles: Option<u32> =
            parse_tag(&text, MAX_BATTLES_TAG).and_then(|s| s.parse().ok());
        let lr = load_replay(&f);
        let t = lr.tunables();
        let (src, known) = (src.clone(), known.clone());
        let (spec, pads) = (lr.spec, lr.pads);
        let out = std::thread::Builder::new()
            .name("soak-fixed".into())
            .stack_size(64 << 20)
            .spawn(move || run_one(&src, &known, &spec, Some(&pads), &t, None))
            .unwrap()
            .join()
            .expect("fixed-fixture thread");
        let name = f.file_name().unwrap().to_string_lossy().into_owned();
        let sig = lr.signature.expect("checked above");
        // A replay that never started measured nothing: an `@<save>` replay
        // whose card save cannot be lifted ran 0 frames and so cannot raise
        // its signature. With no library at all it is a counted skip; with a
        // library that lacks the card or the save it is a failure.
        if let Some(e) = card_save_error(&out.findings) {
            assert!(
                e.starts_with(NO_SAVE_LIBRARY),
                "{name}: the replay's card save does not load: {e}"
            );
            eprintln!("[soak-fixed] {name}: SKIPPED, {e}");
            skipped.push(name);
            continue;
        }
        eprintln!(
            "[soak-fixed] {name}: {} frames, {} battles, findings {:?}",
            out.stats.frames_run,
            out.stats.battles,
            out.findings
                .iter()
                .map(|g| g.signature())
                .collect::<Vec<_>>()
        );
        assert!(
            !out.findings.iter().any(|g| g.signature() == sig),
            "{name}: the fixed finding `{sig}` reproduces again"
        );
        if let Some(max) = max_battles {
            assert!(
                out.stats.battles <= max,
                "{name}: {} battles opened, at most {max} expected",
                out.stats.battles
            );
        }
    }
    if !skipped.is_empty() {
        eprintln!(
            "[soak-fixed] {} of the fixed replays did NOT run (no save library): {}",
            skipped.len(),
            skipped.join(", ")
        );
    }
}

/// The error of a run that could not lift its `@<save>` card save (it ran
/// no frames), if that is how it ended.
fn card_save_error(findings: &[Finding]) -> Option<&str> {
    findings
        .iter()
        .find(|g| g.detector == "tick_error" && g.location == "card save")
        .map(|g| g.detail.as_str())
}

// Disc-free unit tests.

#[test]
fn replay_header_round_trips() {
    let spec = RunSpec {
        scene: "town01".into(),
        seed: 7,
        frames: 5,
    };
    let pads = [0u16, 0x4000, 0x4000, 0, 0x0010];
    let f = Finding {
        detector: "softlock",
        start_scene: "town01".into(),
        scene: "town01".into(),
        mode: "Field".into(),
        frame: 4,
        location: "dialogue".into(),
        detail: "x".into(),
    };
    let text = replay_text(
        &spec,
        &pads,
        Some(&f),
        Some(1),
        Arming {
            host: true,
            retail: true,
        },
    );
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("a.replay.toml");
    std::fs::write(&p, text).unwrap();
    let lr = load_replay(&p);
    assert_eq!(lr.spec.scene, "town01");
    assert_eq!(lr.spec.seed, 7);
    assert_eq!(lr.pads, pads);
    assert_eq!(lr.signature.as_deref(), Some("softlock|town01|dialogue"));
    assert_eq!(lr.min_distinct, Some(1));
    assert!(lr.arming.host, "a replay records the arming it ran under");
    assert!(lr.arming.retail, "and whether it held retail mode");
}

/// Every member under the Evil Medallion's Rage: the anim tick's Rage arm
/// delegates the whole party (`+0x16E |= 0x380`), the command ring refuses
/// every seat, and each round must still open on `Begin | Run`, go straight
/// to the commit confirm and play out on the delegated picks - in the default
/// configuration and under the hosts' retail mode. A deadlocked round reads
/// as `softlock` / `battle_endless` here, and a ring that opens anyway as the
/// `Menu` surface.
#[test]
fn soak_rage_party_plays_its_rounds() {
    let Some(src) = source() else { return };
    if saves_library().is_none() {
        eprintln!("[skip] soak_rage_party_plays_its_rounds: no save library");
        return;
    }
    install_panic_hook();
    let probe = open_session(&src);
    let known: BTreeSet<String> = scene_set(&probe).into_iter().collect();
    drop(probe);
    for retail in [false, true] {
        let (mut battles, mut won) = (0, 0);
        for seed in 1..=3 {
            let spec = RunSpec {
                scene: "keikoku+rage@playthrough-ladder-pro00-14.mcr:PRO-03".into(),
                seed,
                frames: 12_000,
            };
            let (src, known) = (src.clone(), known.clone());
            let r = std::thread::Builder::new()
                .name("soak-rage".into())
                .stack_size(64 << 20)
                .spawn(move || {
                    let mut t = Tunables::from_env();
                    t.arming = Arming { host: true, retail };
                    run_one(&src, &known, &spec, None, &t, None)
                })
                .unwrap()
                .join()
                .expect("rage run");
            let sigs: Vec<String> = r.findings.iter().map(Finding::signature).collect();
            assert!(sigs.is_empty(), "retail {retail} seed {seed}: {sigs:?}");
            assert!(
                !r.stats.battle_surfaces.contains("Menu"),
                "retail {retail} seed {seed}: a command ring opened for a delegated member"
            );
            battles += r.stats.battles;
            won += r.stats.battles_won;
        }
        assert!(
            battles > 0 && won > 0,
            "retail {retail}: {battles} battles, {won} won"
        );
    }
}

/// A replay's `# soak-arming` value reads back to the configuration that
/// wrote it; no tag is the pre-arming bare session.
#[test]
fn arming_tag_reads_back() {
    let a = |host, retail| Arming { host, retail };
    assert_eq!(Arming::from_tag(None), a(false, false));
    assert_eq!(Arming::from_tag(Some("host")), a(true, false));
    assert_eq!(Arming::from_tag(Some("bare")), a(false, false));
    assert_eq!(Arming::from_tag(Some("host-retail")), a(true, true));
    assert_eq!(Arming::from_tag(Some("bare-retail")), a(false, true));
}

/// `+rage` is the outermost run suffix.
#[test]
fn rage_suffix_splits_outermost() {
    assert_eq!(split_rage("jou+talk+rage"), ("jou+talk", true));
    assert_eq!(split_rage("jou+talk"), ("jou+talk", false));
}

#[test]
fn run_seed_is_order_independent_and_distinct() {
    assert_eq!(run_seed("town01", 1), run_seed("town01", 1));
    assert_ne!(run_seed("town01", 1), run_seed("town01", 2));
    assert_ne!(run_seed("town01", 1), run_seed("town02", 1));
    let mut a = Rng::new(3);
    let mut b = Rng::new(3);
    for _ in 0..64 {
        assert_eq!(a.next_u64(), b.next_u64());
    }
}

#[test]
fn variant_strips_payload() {
    assert_eq!(variant("Menu { cursor: 2 }"), "Menu");
    assert_eq!(variant("Some(3)"), "Some");
    assert_eq!(variant("Idle"), "Idle");
}
