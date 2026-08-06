//! Native-window minigame ladder: the `play-window` surfaces no `#[test]` can
//! *call* into, driven by **spawning the subcommand**.
//!
//! ## Why a spawn, and why that is not a workaround
//!
//! `crates/engine-shell/src/bin/legaia-engine/` holds the native window's
//! whole composition layer, and no integration test links against a `bin/`
//! target - so nothing there can be invoked directly. That exclusion is real
//! and it is about **calls**. It is not about **coverage**: `LLVM_PROFILE_FILE`
//! is inherited by child processes, so a test that spawns
//! `CARGO_BIN_EXE_legaia-engine` gets the child's own profile written and
//! merged into the same export. Running the subcommand is therefore a first-
//! class way to reach `bin/`-resident code under `cargo llvm-cov`, and the
//! `mdec` FMV ladder already measured it (40 executions of a routine whose
//! only driver was such a spawn).
//!
//! ## What was actually blocking the minigames, and what fixed it
//!
//! The blocker was never `bin/`. `--pad-script` writes a **pad word** straight
//! into the frame loop and the window's keyboard handler never runs - but every
//! native minigame is opened from that handler (`K` dance, `U` dance how-to,
//! `L` fishing, `O` casino slots, `M` Muscle Dome, `B` Baka Fighter), as is the
//! fishing prize exchange (`P`). No pad word names a minigame, so a pad-only
//! scripted run could not enter one however long it ran.
//!
//! `--key-script` is the missing channel: `TICK:KEY` pairs delivered through
//! the real keyboard arms from inside the per-tick loop. The two scripts
//! compose - `--key-script` opens the minigame, `--pad-script` plays it - which
//! is what every rung below does.
//!
//! ## What each rung asserts
//!
//! Exiting 0 proves nothing here: a HUD builder that emits an empty draw list
//! passes any "did it run" check. So each rung captures a PNG with
//! `--screenshot` and requires the frame to **differ from the same tick of the
//! same scene with no minigame open**. A minigame that opened but painted
//! nothing fails on that comparison, not on the exit status.
//!
//! ## Coverage export - and why it is two steps
//!
//! ```text
//! cargo llvm-cov clean --workspace
//! cargo llvm-cov -p legaia-engine-shell --test w5_native_minigame_ladder \
//!     --no-report -- --test-threads=1
//! cargo llvm-cov report --json \
//!     --output-path target/cov-w5_native_minigame_ladder.json
//! ```
//!
//! The split is load-bearing. `-p <pkg>` scopes the *report* to that package's
//! sources, not just the build: a one-shot `-p legaia-engine-shell ... --json`
//! export of this ladder carries 42 files, every one of them under
//! `crates/engine-shell/`. Almost nothing this ladder exists to reach is in
//! that crate - the dance HUD, the fishing chrome and actors, the Baka number
//! drawers and the casino counter are all `engine-core`, and the draw-list
//! builders under them are `engine-ui`. Reporting without `-p` over the same
//! profiles carries 652 files across fifteen crates, which is the measurement
//! the reach report wants. A scoped export would show this ladder joining and
//! changing nothing, which is indistinguishable from a ladder that did not
//! work.
//!
//! It also reaches a crate the report has recorded as structurally
//! unreachable: `engine-render` is a hard wgpu link the browser composition
//! ladder cannot carry, and the native window *is* that link.
//!
//! **No `--release`.** An optimised build inlines the small kernels and leaves
//! their out-of-line coverage records at zero, which the reach report cannot
//! tell from "never called".
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset **or** when no display is
//! available: `play-window` needs a real wgpu surface even for its offscreen
//! capture, so a headless CI box cannot run these rungs at all. Both skips are
//! printed, because a rung that silently did nothing reads exactly like a rung
//! that passed.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Scene every rung boots. The minigame entries are dev affordances that work
/// from any field scene, so the cheapest one that loads is the right one.
const SCENE: &str = "town01";

/// Tick every rung captures at. Far enough past the entry keys for the HUD to
/// have painted several frames.
const SHOT_TICK: u64 = 300;

fn disc_path() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then_some(p)
}

/// A window can only be opened with a display server. Checked explicitly
/// rather than inferred from a failed run: "the binary exited non-zero" is not
/// a reason to pass a test, and "there is no display" is.
fn have_display() -> bool {
    std::env::var_os("DISPLAY").is_some_and(|v| !v.is_empty())
        || std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty())
}

/// `Some(out_dir)` when the rungs can run, with the skip reason printed
/// otherwise.
fn ladder_env() -> Option<(PathBuf, PathBuf)> {
    let Some(disc) = disc_path() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    };
    if !have_display() {
        eprintln!("[skip] no DISPLAY / WAYLAND_DISPLAY - play-window needs a wgpu surface");
        return None;
    }
    let out = std::env::temp_dir().join("legaia-w5-native-minigame");
    std::fs::create_dir_all(&out).expect("scratch dir");
    Some((disc, out))
}

/// One scripted `play-window` run. Returns `(stdout, stderr)`.
///
/// The child inherits `LLVM_PROFILE_FILE`, which is the whole point: under
/// `cargo llvm-cov` its profile is merged into this test's export, so every
/// `bin/`-resident routine the run entered counts as executed.
fn run_window(
    disc: &Path,
    shot: &Path,
    key_script: &str,
    pad_script: Option<&str>,
    shot_tick: u64,
) -> (String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_legaia-engine"));
    cmd.arg("play-window")
        .arg("--scene")
        .arg(SCENE)
        .arg("--disc")
        .arg(disc)
        .arg("--no-audio")
        .arg("--screenshot")
        .arg(shot)
        .arg("--screenshot-tick")
        .arg(shot_tick.to_string());
    if !key_script.is_empty() {
        cmd.arg("--key-script").arg(key_script);
    }
    if let Some(p) = pad_script {
        cmd.arg("--pad-script").arg(p);
    }
    let out = cmd.output().expect("spawn legaia-engine play-window");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Decode a captured PNG into `(width, height, rgba)`.
fn read_png(path: &Path) -> (u32, u32, Vec<u8>) {
    let file = std::fs::File::open(path).unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
    let dec = png::Decoder::new(std::io::BufReader::new(file));
    let mut reader = dec.read_info().expect("png header");
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).expect("png data");
    buf.truncate(info.buffer_size());
    (info.width, info.height, buf)
}

/// Fraction of pixels that differ between two captures of the same size.
fn pixel_delta(a: &Path, b: &Path) -> f64 {
    let (aw, ah, ap) = read_png(a);
    let (bw, bh, bp) = read_png(b);
    assert_eq!((aw, ah), (bw, bh), "captures must be the same size");
    let differing = ap
        .chunks_exact(4)
        .zip(bp.chunks_exact(4))
        .filter(|(x, y)| x != y)
        .count();
    differing as f64 / (aw as f64 * ah as f64)
}

/// The baseline frame: the same scene at the same tick with nothing open.
/// Captured once and reused, so every rung's delta is against the same frame.
fn baseline(disc: &Path, out: &Path) -> PathBuf {
    let path = out.join("baseline.png");
    if !path.is_file() {
        let (stdout, _) = run_window(disc, &path, "", None, SHOT_TICK);
        assert!(
            stdout.contains("[ok] screenshot"),
            "baseline capture failed:\n{stdout}"
        );
    }
    path
}

/// Every rung's shared shape: open the surface with `key_script`, optionally
/// play it with `pad_script`, and require both the entry log line and a frame
/// that is not the untouched scene.
///
/// `min_delta` is a floor on the share of changed pixels. A minigame HUD is
/// text over a static field frame, so even the sparsest one moves well over a
/// thousandth of the screen; an entry that opened and drew nothing does not.
fn rung(
    label: &str,
    key_script: &str,
    pad_script: Option<&str>,
    expect_log: &str,
    min_delta: f64,
    shot_tick: u64,
) {
    let Some((disc, out)) = ladder_env() else {
        return;
    };
    let base = baseline(&disc, &out);
    let shot = out.join(format!("{label}.png"));
    let _ = std::fs::remove_file(&shot);
    let (stdout, stderr) = run_window(&disc, &shot, key_script, pad_script, shot_tick);
    assert!(
        stdout.contains("[ok] screenshot"),
        "{label}: no capture written\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains(expect_log),
        "{label}: the entry never logged '{expect_log}'\nstderr:\n{stderr}"
    );
    let delta = pixel_delta(&base, &shot);
    assert!(
        delta >= min_delta,
        "{label}: the frame is {:.4}% different from the untouched scene \
         (floor {:.4}%) - the surface opened but painted nothing",
        delta * 100.0,
        min_delta * 100.0
    );
    eprintln!(
        "[ok] {label}: {:.3}% of the frame differs from baseline",
        delta * 100.0
    );
}

// ---------------------------------------------------------------------------
// Rungs
// ---------------------------------------------------------------------------

/// Fishing: the venue actors (wander / floor solve / camera publish / line),
/// the persistent + catch HUD rows and the venue chrome.
///
/// `L` opens it and Cross casts. The cast is pad input, which is exactly why
/// both scripts are needed: neither channel alone reaches this frame.
#[test]
fn rung1_fishing_opens_and_paints_its_hud() {
    rung(
        "fishing",
        "40:L",
        Some("80:Cross,140:Cross,200:Cross"),
        "fishing: started",
        0.001,
        SHOT_TICK,
    );
}

/// The fishing prize exchange: the venue sub-screen panel, the per-row
/// availability gate and the **quantity cap** a committed purchase runs.
///
/// The long held-Cross window is not padding. The cap is only reached through
/// an *available* row, and a row is available only when the point pool can
/// pay for it - so the rung has to fish long enough to earn a cheapest-row
/// price before it opens the counter. A short run reaches the panel and stops
/// one gate short of the arithmetic, which is exactly how this row stayed
/// dark while the exchange itself was on screen.
#[test]
fn rung2_fishing_prize_exchange_buys_a_row() {
    let Some((disc, out)) = ladder_env() else {
        return;
    };
    let base = baseline(&disc, &out);
    let shot = out.join("fishing_exchange.png");
    let _ = std::fs::remove_file(&shot);
    let (stdout, stderr) = run_window(
        &disc,
        &shot,
        // Five `Down`s walk the cursor to the last row whatever the pool
        // floors it to: the dear one-time prizes sit at the top of every
        // venue table and the cheap repeatable ones at the bottom, so the
        // clamped-at-`last` row is the one a modest point total can pay for.
        "40:L,1800:P,1820:Down,1840:Down,1860:Down,1880:Down,1900:Down,1940:Enter,1960:Right",
        Some("80-1700:Cross"),
        2000,
    );
    assert!(
        stdout.contains("[ok] screenshot"),
        "fishing_exchange: no capture written\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains("fishing: started"),
        "fishing_exchange: fishing never opened\nstderr:\n{stderr}"
    );
    // The purchase is the assertion. Both outcomes the buy can log are
    // failures of this rung for different reasons, so neither is accepted:
    // "unavailable" means the pool never reached a price (the fishing window
    // was too short), and no line at all means `Enter` never reached the
    // commit.
    assert!(
        stderr.contains("fishing exchange: bought item"),
        "fishing_exchange: no row was bought, so the quantity cap never ran\nstderr:\n{stderr}"
    );
    let delta = pixel_delta(&base, &shot);
    assert!(
        delta >= 0.001,
        "fishing_exchange: the frame is {:.4}% different from the untouched scene",
        delta * 100.0
    );
    eprintln!(
        "[ok] fishing_exchange: {:.3}% of the frame differs from baseline",
        delta * 100.0
    );
}

/// The Noa dance: the HUD driver's per-frame list (score boxes, gauges, the
/// rival beat tracks) plus the quad + sprite-part layers.
#[test]
fn rung3_dance_opens_and_paints_its_hud() {
    rung(
        "dance",
        "40:K",
        Some("120:Square,160:Circle,200:Triangle,240:Square"),
        "dance: count-in",
        0.001,
        SHOT_TICK,
    );
}

/// The Disco King how-to run: the tutorial script's step machine and its
/// caption / option / cursor frame.
#[test]
fn rung4_dance_how_to_runs_its_tutorial() {
    rung(
        "dance_how_to",
        "40:U",
        Some("120:Cross,180:Cross,240:Cross"),
        "dance: how-to started",
        0.001,
        SHOT_TICK,
    );
}

/// One `Up` per exchange from tick 80 to 800 - the duel the ladder plays.
///
/// The pattern is not arbitrary and is not a placeholder. The opponent's AI
/// rolls its move (random, or its own scripted pattern walked backwards), so
/// the outcome is a property of the whole `(entry tick, press schedule)` pair
/// and nothing weaker: the same schedule reproduces the same duel, and a
/// *different* one loses. A cycling `Left,Right,Up` pattern entered on the
/// same tick loses 0-2, which is why the win below has to be asserted rather
/// than assumed.
const BAKA_ATTACKS: &str = "80:Up,100:Up,120:Up,140:Up,160:Up,180:Up,200:Up,220:Up,240:Up,\
260:Up,280:Up,300:Up,320:Up,340:Up,360:Up,380:Up,400:Up,420:Up,440:Up,460:Up,480:Up,500:Up,\
520:Up,540:Up,560:Up,580:Up,600:Up,620:Up,640:Up,660:Up,680:Up,700:Up,720:Up,740:Up,760:Up,\
780:Up,800:Up";

/// The Baka Fighter duel played to a **player win** - which is the only thing
/// that installs the end-of-match tally.
///
/// The tally is what the two number drawers on this page read: the
/// right-aligned score field and the "GET COIN" numeral strip are drawn under
/// `if let Some(t) = f.tally()`, and a lost match sets no tally at all. So a
/// duel that merely *runs* leaves both dark while looking, in a screenshot,
/// exactly like one that reached them - which is how they stayed on the
/// never-entered list while the duel HUD was plainly on screen.
///
/// Two runs, because the two facts are observable in different places: the
/// tally is on the frame at tick 900, and the outcome only reaches the log
/// when the player leaves a decided match.
#[test]
fn rung5_baka_fighter_duel_is_won_and_shows_its_tally() {
    let Some((disc, out)) = ladder_env() else {
        return;
    };
    let base = baseline(&disc, &out);
    let shot = out.join("baka.png");
    let _ = std::fs::remove_file(&shot);
    let (stdout, stderr) = run_window(&disc, &shot, "40:B", Some(BAKA_ATTACKS), 900);
    assert!(
        stdout.contains("[ok] screenshot"),
        "baka: no capture written\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains("baka: started"),
        "baka: the duel never opened\nstderr:\n{stderr}"
    );
    let delta = pixel_delta(&base, &shot);
    assert!(
        delta >= 0.001,
        "baka: the frame is {:.4}% different from the untouched scene",
        delta * 100.0
    );

    // Same schedule, plus the `B` that leaves the decided match - which is
    // where the host logs who won. A loss logs "match lost" and fails here,
    // so this cannot pass on a duel that never reached the tally.
    let throwaway = out.join("baka_exit.png");
    let (_, stderr) = run_window(&disc, &throwaway, "40:B,850:B", Some(BAKA_ATTACKS), 900);
    assert!(
        stderr.contains("baka: match WON"),
        "baka: the scripted duel did not win, so no tally was installed\nstderr:\n{stderr}"
    );
    eprintln!(
        "[ok] baka: won, {:.3}% of the frame differs from baseline",
        delta * 100.0
    );
}

/// The casino slot machine, whose entry also runs the coin-exchange counter
/// (the bank starts empty, so the entry buys the dev stake through it).
#[test]
fn rung6_slot_machine_opens_and_spins() {
    rung(
        "slots",
        "40:O",
        Some("100:Cross,160:Cross,220:Cross"),
        "slots: started",
        0.001,
        SHOT_TICK,
    );
}

/// The Muscle Dome leg: the contest hub's HUD lines and the round time meter.
#[test]
fn rung7_muscle_dome_leg_opens() {
    rung(
        "muscle",
        "40:M",
        Some("100:Left,140:Right,180:Cross"),
        "muscle: started",
        0.001,
        SHOT_TICK,
    );
}
