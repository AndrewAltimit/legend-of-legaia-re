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
    run_window_with(disc, shot, key_script, pad_script, shot_tick, &[])
}

/// [`run_window`] with extra `play-window` arguments.
fn run_window_with(
    disc: &Path,
    shot: &Path,
    key_script: &str,
    pad_script: Option<&str>,
    shot_tick: u64,
    extra: &[&std::ffi::OsStr],
) -> (String, String) {
    run_window_env(
        disc,
        shot,
        key_script,
        pad_script,
        shot_tick,
        extra,
        &[],
        false,
    )
}

/// [`run_window_with`] with extra environment variables on the child (the
/// opt-in surfaces such as `LEGAIA_DEV_MENU` are switched by environment, not
/// by flag). `audio` drops `--no-audio`, so the window opens its output device.
#[allow(clippy::too_many_arguments)]
fn run_window_env(
    disc: &Path,
    shot: &Path,
    key_script: &str,
    pad_script: Option<&str>,
    shot_tick: u64,
    extra: &[&std::ffi::OsStr],
    envs: &[(&str, &str)],
    audio: bool,
) -> (String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_legaia-engine"));
    cmd.envs(envs.iter().copied());
    cmd.arg("play-window")
        .arg("--scene")
        .arg(SCENE)
        .arg("--disc")
        .arg(disc)
        .arg("--screenshot")
        .arg(shot)
        .arg("--screenshot-tick")
        .arg(shot_tick.to_string());
    if !audio {
        cmd.arg("--no-audio");
    }
    if !key_script.is_empty() {
        cmd.arg("--key-script").arg(key_script);
    }
    if let Some(p) = pad_script {
        cmd.arg("--pad-script").arg(p);
    }
    cmd.args(extra);
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
    assert_eq!(
        (aw, ah),
        (bw, bh),
        "captures must be the same size: {} is {aw}x{ah}, {} is {bw}x{bh} \
         (both come from one process, so the window manager resized the \
         surface between two runs of the same command)",
        a.display(),
        b.display()
    );
    let differing = ap
        .as_chunks::<4>()
        .0
        .iter()
        .zip(bp.as_chunks::<4>().0)
        .filter(|(x, y)| x != y)
        .count();
    differing as f64 / (aw as f64 * ah as f64)
}

/// The baseline frame: the same scene at the same tick with nothing open.
/// Captured once **per test process** and shared by every rung, so each
/// rung's delta is against the same frame.
///
/// Once per process, never once per scratch directory. The capture is the
/// window's surface, and the surface is whatever the window manager granted:
/// on a display too small for a 960x720 window plus its decorations the WM
/// clamps the height, and a PNG cached from an earlier session on a larger
/// display then disagrees on size with every fresh rung capture. A baseline
/// that outlives the process is a baseline from an environment the rungs may
/// no longer run in, so a stale file is removed rather than reused.
fn baseline(disc: &Path, out: &Path) -> PathBuf {
    static BASELINE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BASELINE
        .get_or_init(|| {
            let path = out.join("baseline.png");
            let _ = std::fs::remove_file(&path);
            let (stdout, stderr) = run_window(disc, &path, "", None, SHOT_TICK);
            assert!(
                stdout.contains("[ok] screenshot"),
                "baseline capture failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
            );
            path
        })
        .clone()
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
/// `L` opens it, Circle starts the cast and locks the power meter, and Cross
/// reels. The cast is pad input, which is exactly why both scripts are
/// needed: neither channel alone reaches this frame.
#[test]
fn rung1_fishing_opens_and_paints_its_hud() {
    rung(
        "fishing",
        "40:L",
        Some("80:Circle,120:Circle,160-200:Cross"),
        "fishing: started",
        0.001,
        SHOT_TICK,
    );
}

/// The fishing prize exchange: the venue sub-screen panel, the per-row
/// availability gate and the **quantity cap** a committed purchase runs.
///
/// The cap is only reached through an *available* row, and a row is
/// available only when the point pool can pay for it. The session is the
/// retail loop now - a catch needs a strike off the band roll, which a short
/// scripted run cannot count on - so the rung seeds the pool the way a player
/// with a GameShark would: a cheat file writing `0x8008444C`, the pool the
/// exchange spends from. A pool that cannot pay reaches the panel and stops
/// one gate short of the arithmetic, which is exactly how this row once
/// stayed dark while the exchange itself was on screen.
#[test]
fn rung2_fishing_prize_exchange_buys_a_row() {
    let Some((disc, out)) = ladder_env() else {
        return;
    };
    let base = baseline(&disc, &out);
    let shot = out.join("fishing_exchange.png");
    let _ = std::fs::remove_file(&shot);
    // 5000 points (a u16 write, the cheat database's own shape).
    let cheat = out.join("fishing_points.gs.txt");
    std::fs::write(&cheat, "R I 2 L 0 8008444C 1388 Fishing points\n").expect("write cheat file");
    let (stdout, stderr) = run_window_with(
        &disc,
        &shot,
        // Five `Down`s walk the cursor to the last row whatever the pool
        // floors it to: the dear one-time prizes sit at the top of every
        // venue table and the cheap repeatable ones at the bottom, so the
        // clamped-at-`last` row is the one a modest point total can pay for.
        "40:L,100:P,120:Down,140:Down,160:Down,180:Down,200:Down,240:Enter,260:Right",
        None,
        300,
        &[std::ffi::OsStr::new("--cheat-file"), cheat.as_os_str()],
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
    // was too small), and no line at all means `Enter` never reached the
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

/// The cabinet's front end, then one Square per exchange from tick 180 to 920.
///
/// The cabinet boots on its attract card (as retail's does): Cross at tick 50
/// leaves it, the fade-out runs `0x3D` frames, and Cross at 130 confirms the
/// player select's first column (Vahn); the duel starts after the wipe.
///
/// The pattern is not arbitrary and is not a placeholder. The opponent's AI
/// rolls its move (random, or its own scripted pattern walked backwards), so
/// the outcome is a property of the whole `(entry tick, press schedule)` pair
/// and nothing weaker: the same schedule reproduces the same duel, and a
/// *different* one loses. The same front end followed by Circle on the same
/// ticks loses, which is why the win below has to be asserted rather than
/// assumed.
const BAKA_ATTACKS: &str = "50:Cross,130:Cross,180:Square,200:Square,220:Square,240:Square,260:Square,280:Square,300:Square,\
320:Square,340:Square,360:Square,380:Square,400:Square,420:Square,440:Square,460:Square,480:Square,\
500:Square,520:Square,540:Square,560:Square,580:Square,600:Square,620:Square,640:Square,660:Square,\
680:Square,700:Square,720:Square,740:Square,760:Square,780:Square,800:Square,820:Square,840:Square,\
860:Square,880:Square,900:Square,920:Square";

/// The Baka Fighter duel played to a **player win** - which is the only thing
/// that installs the end-of-match tally. The attacks are thrown with Square,
/// the face button retail reads for attack type 1 (`andi 0x80` at
/// `0x801D43B4`); the d-pad throws nothing.
///
/// The tally is what the two number drawers on this page read: the
/// right-aligned score field and the "GET COIN" numeral strip are drawn under
/// `if let Some(t) = f.tally()`, and a lost match sets no tally at all. So a
/// duel that merely *runs* leaves both dark while looking, in a screenshot,
/// exactly like one that reached them - which is how they stayed on the
/// never-entered list while the duel HUD was plainly on screen.
///
/// Two runs, because the two facts are observable in different places: the
/// tally is on the frame at tick 1000, and the outcome only reaches the log
/// when the player leaves a decided match.
#[test]
fn rung5_baka_fighter_duel_is_won_and_shows_its_tally() {
    let Some((disc, out)) = ladder_env() else {
        return;
    };
    let base = baseline(&disc, &out);
    let shot = out.join("baka.png");
    let _ = std::fs::remove_file(&shot);
    let (stdout, stderr) = run_window(&disc, &shot, "40:B", Some(BAKA_ATTACKS), 1000);
    assert!(
        stdout.contains("[ok] screenshot"),
        "baka: no capture written\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains("minigame warp: entered baka_fighter"),
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
    let (_, stderr) = run_window(&disc, &throwaway, "40:B,970:B", Some(BAKA_ATTACKS), 1000);
    assert!(
        stderr.contains("baka: match WON"),
        "baka: the scripted duel did not win, so no tally was installed\nstderr:\n{stderr}"
    );
    eprintln!(
        "[ok] baka: won, {:.3}% of the frame differs from baseline",
        delta * 100.0
    );
}

/// The casino slot machine, entered through the mode-24 door warp the
/// cabinet takes (`O` arms it, as `B` / `M` arm theirs). The balance is the
/// coin bank's; on a fresh game that is empty, so the Cross presses meet the
/// machine's own state-1 gate and the rung pins the entry and the drawn
/// cabinet rather than a spin.
#[test]
fn rung6_slot_machine_opens() {
    rung(
        "slots",
        "40:O",
        Some("100:Cross,160:Cross,220:Cross"),
        "minigame warp: entered slot_machine",
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
        "minigame warp: entered muscle_dome",
        0.001,
        SHOT_TICK,
    );
}

/// The fishing overlay's developer readout (`FUN_801D2050`'s debug arm and
/// the tracked-point separation `FUN_801D765C` it prints): the wander actor's
/// tile pair, settled height, facing and its distance from the venue anchor.
///
/// Retail gates it on two things at once - the global print flag and a held
/// modifier bit (`_DAT_8007B850 & 2`) - and the native window maps them to
/// the developer-menu session (`LEGAIA_DEV_MENU`) and a held R2 (the raw pad
/// word's `0x0200`, which the packed word carries as `0x0002`). Opening the
/// minigame does neither, which is why rung 1 never enters the readout.
///
/// Both captures run with the developer menu up, so the only difference
/// between them is the held modifier: a readout that never drew leaves the
/// two frames identical, which is the failure this rung exists to catch.
#[test]
fn rung8_fishing_developer_readout_needs_menu_and_modifier() {
    let Some((disc, out)) = ladder_env() else {
        return;
    };
    let dev = [("LEGAIA_DEV_MENU", "1")];
    let cast = "80:Circle,120:Circle";
    // Every frame this window draws is opaque, so a capture holding a
    // non-opaque pixel was damaged on its way out of the GPU, not drawn that
    // way. The shared CI runner has twice returned one with a 16-pixel run of
    // `(0, 0, 0, 0)` near the top edge, which then reads as a change outside
    // the readout band. Such a capture is shot again instead of compared.
    const ATTEMPTS: usize = 3;
    let shoot = |label: &str, pad: &str| {
        let shot = out.join(format!("{label}.png"));
        for attempt in 1..=ATTEMPTS {
            let _ = std::fs::remove_file(&shot);
            let (stdout, stderr) =
                run_window_env(&disc, &shot, "40:L", Some(pad), SHOT_TICK, &[], &dev, false);
            assert!(
                stdout.contains("[ok] screenshot"),
                "{label}: no capture written\nstdout:\n{stdout}\nstderr:\n{stderr}"
            );
            assert!(
                stderr.contains("fishing: started"),
                "{label}: fishing never opened\nstderr:\n{stderr}"
            );
            let (_, _, rgba) = read_png(&shot);
            let holes = rgba
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|px| px[3] != 255)
                .count();
            if holes == 0 {
                return shot;
            }
            eprintln!(
                "[warn] {label}: capture {attempt}/{ATTEMPTS} holds {holes} non-opaque \
                 pixel(s) - damaged readback, shooting again"
            );
        }
        panic!("{label}: every one of {ATTEMPTS} captures held non-opaque pixels");
    };
    let plain = shoot("fishing_dev_plain", cast);
    let readout = shoot(
        "fishing_dev_readout",
        &format!("{cast},200-{}:R2", SHOT_TICK + 5),
    );
    let delta = pixel_delta(&plain, &readout);
    assert!(
        delta > 0.0,
        "fishing_dev_readout: holding the modifier under the developer menu \
         changed nothing on screen - the readout never drew"
    );
    // And the change is the readout's row, not some other effect of the held
    // bit: every differing pixel sits in the band the HUD stages the line at
    // (stage pen y = 116 of 240, one text row tall).
    let (w, h, a) = read_png(&plain);
    let (_, _, b) = read_png(&readout);
    let rows: Vec<u32> = (0..w * h)
        .filter(|&i| a[i as usize * 4..][..4] != b[i as usize * 4..][..4])
        .map(|i| i / w)
        .collect();
    let (lo, hi) = (
        *rows.iter().min().expect("non-empty"),
        *rows.iter().max().expect("non-empty"),
    );
    let stage = |y: u32| y as f64 * 240.0 / h as f64;
    let out_of_band = rows
        .iter()
        .filter(|&&y| !(110.0..=140.0).contains(&stage(y)))
        .count();
    assert!(
        stage(lo) >= 110.0 && stage(hi) <= 140.0,
        "fishing_dev_readout: the held modifier changed stage rows {:.0}..{:.0}, \
         outside the readout line's band ({out_of_band} of {} differing pixels \
         out of band)",
        stage(lo),
        stage(hi),
        rows.len()
    );
    eprintln!(
        "[ok] fishing_dev_readout: {:.3}% of the frame differs from the unmodified capture",
        delta * 100.0
    );
}

/// The slot machine's reel motor: a directly keyed SPU voice
/// (`World::take_sfx_voice_keys` -> `AudioBgmDirector::key_on_voice_attr` ->
/// `legaia_engine_audio::key_on_voice_attr`), the one cue path that bypasses
/// both the SFX ring and the descriptor bank.
///
/// Two things keep every other rung off it. A spin needs coins - rung 6
/// meets the machine's state-1 gate on an empty bank - so a cheat file
/// seeds the coin bank (`0x800845A4`) the way rung 2 seeds fishing points.
/// And the key needs the audio director, which exists only with a live
/// output device, so this is the one rung that runs without `--no-audio`.
/// A machine with no device cannot run it: the rung skips when the window
/// reports no audio device rather than passing on a run that keyed
/// nothing.
#[test]
fn rung9_slot_reel_motor_keys_a_voice() {
    let Some((disc, out)) = ladder_env() else {
        return;
    };
    let shot = out.join("slots_audio.png");
    let _ = std::fs::remove_file(&shot);
    let cheat = out.join("slot_coins.gs.txt");
    // 1000 coins (a u16 write into the bank word).
    std::fs::write(&cheat, "R I 2 L 0 800845A4 03E8 Coin bank\n").expect("write cheat file");
    let (stdout, stderr) = run_window_env(
        &disc,
        &shot,
        "40:O",
        Some("100:Cross,160:Cross,220:Cross,280:Cross"),
        400,
        &[std::ffi::OsStr::new("--cheat-file"), cheat.as_os_str()],
        &[("RUST_LOG", "info,legaia_engine_shell=debug")],
        true,
    );
    if !stderr.contains("audio: device=") {
        eprintln!("[skip] no audio output device - the voice-key path needs the director");
        return;
    }
    assert!(
        stdout.contains("[ok] screenshot"),
        "slots_audio: no capture written\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains("minigame warp: entered slot_machine"),
        "slots_audio: the machine never opened\nstderr:\n{stderr}"
    );
    assert!(
        stderr
            .lines()
            .any(|l| l.contains("direct voice") && l.ends_with("keyed: true")),
        "slots_audio: a spin with coins in the bank keyed no reel-motor voice\nstderr:\n{stderr}"
    );
    eprintln!("[ok] slots_audio: the reel motor keyed a voice");
}
