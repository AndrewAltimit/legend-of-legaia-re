//! Disc + save-library gated: the arts voice of a Tactical Arts string,
//! engine against a retail capture of the same string.
//!
//! The retail side is `scripts/pcsx-redux/autorun_arts_voice_parity.lua` on
//! the `arts_bar_ideal_noa_fangs` state (retail executable): Noa types
//! `Up Down Up Up Down Down` - Lizard Tail (`0x2B`, `Up Down Up`) then
//! Acrobatic Blitz (`0x2A`, `Up Down Down`), both on her learned list - and
//! Cross; Gala takes Spirit. The probe logged, against the strike cursor
//! `ctx[+0x15]` and the clip latch `ctx[+0x243]`:
//!
//! | what | retail |
//! |---|---|
//! | built queue `+0x1DF` | `0F 0E 19 2B 0F 0E 19 2A` |
//! | selector calls `FUN_8004C140(char, action, flag)` | `(2, 0x2B, 0x0F)` at cursor 3, `(2, 0x2A, 0x00)` at cursor 7 |
//! | `ctx[+0x243]` at each call | `0` |
//! | clip starts `FUN_8003D53C` | slot 3 (`XA4.XA`) each, channel / `dur` `(4, 0x36)` then `(0xF, 0x21)` |
//! | Noa's committed clip `+0x1D9`, vsyncs after the first `0x0F` | `0x0E` +18, `0x10` +35 (call), `0x11` +68, `0x0F` +129, `0x0E` +149, `0x10` +166 (call), `0x11` +198 |
//!
//! The cue router `FUN_8004FE5C` took, in order, `0x11` x3 (the approach
//! walk's footfalls), `5F 1A 08 AB 1A`, `1A 4C`, `5F 1A 08 AB 1A`, `1A 4C`
//! (category `1`, Noa) and the monster's `0x15` (category `3`).
//!
//! The channel is a `rand()` pick, so the engine is held to the pool and the
//! duration law rather than to retail's two draws; the two retail picks are
//! checked to be members of the pools the engine reads.
//!
//! Skip-passes without `LEGAIA_DISC_BIN`, an extracted tree or the library.

use legaia_engine_save_state::WorldSaveState;
use std::path::PathBuf;

use legaia_engine_battle::arts_command_input::ArtsInputPhase;
use legaia_engine_battle_menus::battle_input::{AttackMode, BattleCommand, CommandPhase};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_engine_system::input::PadButton;

/// `arts_bar_ideal_noa_fangs` (retail executable, catalogued unpatched).
const STATE_FP: &str = "da710f5a5a5ddc5c9b83a298e9ff4dc8034b0f77016f20f143c6079b4d4501b1";

/// Noa's string: Lizard Tail then Acrobatic Blitz.
const NOA_ENTRY: [PadButton; 6] = [
    PadButton::Up,
    PadButton::Down,
    PadButton::Up,
    PadButton::Up,
    PadButton::Down,
    PadButton::Down,
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn first_dir(env: &str, rel: &str, probe: &str) -> Option<PathBuf> {
    std::env::var_os(env)
        .map(PathBuf::from)
        .into_iter()
        .chain([repo_root().join(rel)])
        .find(|d| d.join(probe).exists())
}

/// Vahn and Gala take Spirit, Noa types [`NOA_ENTRY`] and every prompt is
/// confirmed - the hand the retail probe played.
fn hand(session: &NativeSession, typed: &mut usize) -> u16 {
    let w = &session.host.world;
    if w.battle.item_menu.is_some() {
        return PadButton::Circle.mask();
    }
    if let Some(arts) = w.battle.arts_input.as_ref() {
        return match &arts.phase {
            ArtsInputPhase::Entering if *typed < NOA_ENTRY.len() => {
                *typed += 1;
                NOA_ENTRY[*typed - 1].mask()
            }
            _ => PadButton::Cross.mask(),
        };
    }
    let Some(cmd) = w.battle.command.as_ref() else {
        return PadButton::Cross.mask();
    };
    let noa = cmd.actor == 1;
    match &cmd.phase {
        CommandPhase::Menu { .. } => {
            let want = if noa {
                BattleCommand::Attack
            } else {
                BattleCommand::Spirit
            };
            if cmd.menu_command() == Some(want) {
                PadButton::Cross.mask()
            } else if want == BattleCommand::Attack {
                PadButton::Left.mask()
            } else {
                PadButton::Down.mask()
            }
        }
        CommandPhase::AttackMode { .. } => {
            if cmd.attack_mode() == Some(AttackMode::Command) {
                PadButton::Cross.mask()
            } else {
                PadButton::Right.mask()
            }
        }
        CommandPhase::RoundPrompt { .. } | CommandPhase::CommitConfirm { .. } => {
            PadButton::Left.mask()
        }
        _ => PadButton::Cross.mask(),
    }
}

/// The cue router's per-call debug line (`battle_cue_route`), collected so
/// the SFX side of the turn can be laid against the retail router calls.
static ROUTES: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
struct RouteLog;
impl log::Log for RouteLog {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.target() == "battle_cue_route"
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            ROUTES.lock().unwrap().push(r.args().to_string());
        }
    }
    fn flush(&self) {}
}

#[test]
fn noa_lizard_tail_acrobatic_blitz_voices_as_retail() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = first_dir("LEGAIA_EXTRACTED_DIR", "extracted", "PROT.DAT") else {
        eprintln!("[skip] extracted tree missing (set LEGAIA_EXTRACTED_DIR)");
        return;
    };
    let Some(library) = first_dir("LEGAIA_SAVES_LIBRARY", "saves/library", "pcsx-redux") else {
        eprintln!("[skip] save library missing (set LEGAIA_SAVES_LIBRARY)");
        return;
    };
    if std::env::var_os("LEGAIA_SCUS").is_none() {
        // SAFETY: set before any save-state read, from the test thread; every
        // writer in this binary sets the same value.
        unsafe { std::env::set_var("LEGAIA_SCUS", extracted.join("SCUS_942.54")) };
    }
    let path = library
        .join("pcsx-redux")
        .join(format!("{STATE_FP}.sstate"));
    let state = match legaia_pcsx_redux::SaveState::from_path(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[skip] state {STATE_FP} not readable from the library: {e}");
            return;
        }
    };
    let ram = state.main_ram();
    let battle =
        legaia_parity::retail_compare_battle::RetailBattle::from_ram(ram).expect("a battle state");
    let base = (0x8008_4140u32 & 0x1F_FFFF) as usize;
    let sf = legaia_save::SaveFile::from_retail_sc_block(
        &ram[base..base + 0x2000],
        legaia_save::RETAIL_SC_PARTY_RECORDS,
    )
    .expect("lift the SC block");

    let cfg = BootConfig {
        scene: "map01".into(),
        enable_audio: false,
    };
    let mut session = NativeSession::open(&extracted, &cfg).expect("open NativeSession");
    let opts = FieldLiveOpts {
        live_loop: false,
        player_battle: true,
        battle_bgm: None,
    };
    session.host.world.load_full(sf.clone());
    assert!(session.resume_save(sf, "map01", &opts).entered_scene());
    assert!(
        session.host.world.audio.arts_voice_tables.is_some(),
        "the boot installs the selector's tables"
    );
    let prot = extracted.join("PROT");
    let fid = battle
        .formation_cell()
        .register(&mut session.host.world, "map01", || {
            std::fs::read(prot.join("0867_battle_data.BIN")).ok()
        })
        .expect("register the capture's formation");
    assert!(session.host.world.force_encounter(fid));

    let _ = log::set_logger(&RouteLog).map(|()| log::set_max_level(log::LevelFilter::Debug));
    let mut typed = 0usize;
    let mut prev = 0u16;
    let mut in_battle = false;
    let mut last_clip = None;
    // `(tick, Noa's committed clip)` on every change once the turn runs.
    let mut timeline = Vec::new();
    // `(tick, cslot, action, flag, cursor, latch, queue)` per selector call.
    let mut calls = Vec::new();
    // `(tick, clip slot, channel, dur)` per CD-XA request.
    let mut clips = Vec::new();
    let mut sfx = Vec::new();
    for tick in 0..20_000u32 {
        // Edge-triggered sessions: a press frame, then a release frame.
        let pad = if prev == 0 && session.host.world.mode == SceneMode::Battle {
            hand(&session, &mut typed)
        } else {
            0
        };
        prev = pad;
        session.host.world.set_pad(pad);
        // The latch the commit reads is the one standing before the tick.
        let latch = session.host.world.battle_ctx.gauge_rearm_latch;
        session.tick().expect("tick");
        let w = &mut session.host.world;
        if w.mode == SceneMode::Battle {
            in_battle = true;
        } else if in_battle {
            break;
        }
        let (clip, cursor, q) = {
            let noa = &w.actors[1].battle;
            (noa.current_anim, noa.strike_index, noa.params)
        };
        if typed == NOA_ENTRY.len() && last_clip != Some(clip) {
            last_clip = Some(clip);
            timeline.push((tick, clip));
        }
        for cue in w.drain_battle_shout_cues() {
            let at = |k: usize| q.get(k).copied().unwrap_or(0);
            let c = usize::from(cursor);
            calls.push((
                tick,
                cue.cslot,
                at(c),
                at(c + 1),
                cursor,
                latch,
                q[..8].to_vec(),
            ));
            assert_eq!(
                cue.action,
                at(c),
                "the call keys on the byte behind the starter"
            );
            assert!(
                cue.resolved,
                "the world resolves the call off the disc tables"
            );
        }
        if typed == NOA_ENTRY.len() {
            sfx.extend(ROUTES.lock().unwrap().drain(..).map(|m| (tick, m)));
        }
        for c in w.drain_battle_xa_cues() {
            clips.push((tick, c.clip, c.channel, c.duration_sectors));
        }
        let _ = w.drain_battle_events();
    }
    eprintln!("[ran] Noa clip timeline {timeline:02X?}");
    eprintln!("[ran] selector calls {calls:02X?}");
    eprintln!("[ran] CD-XA requests {clips:02X?}");
    eprintln!("[ran] cue routes {sfx:?}");

    // The SFX side: every cue router call `FUN_8004FE5C(id, category)` from
    // the approach walk to the monster's reaction, in retail's order. The
    // walk's three footfalls (`0x11`) need the looping walk's re-commit to
    // re-arm its cue track each cycle.
    const RETAIL_ROUTES: [(u16, u8); 18] = [
        (0x11, 1),
        (0x11, 1),
        (0x11, 1),
        (0x5F, 1),
        (0x1A, 1),
        (0x08, 1),
        (0xAB, 1),
        (0x1A, 1),
        (0x1A, 1),
        (0x4C, 1),
        (0x5F, 1),
        (0x1A, 1),
        (0x08, 1),
        (0xAB, 1),
        (0x1A, 1),
        (0x1A, 1),
        (0x4C, 1),
        (0x15, 3),
    ];
    let routes: Vec<(u16, u8)> = sfx
        .iter()
        .filter_map(|(_, m)| {
            let mut w = m.split_whitespace();
            let id = u16::from_str_radix(w.nth(1)?.trim_start_matches("0x"), 16).ok()?;
            let cat = w.nth(1)?.parse().ok()?;
            Some((id, cat))
        })
        .take(RETAIL_ROUTES.len())
        .collect();
    assert_eq!(routes, RETAIL_ROUTES, "the turn's cue router calls");

    // The queue the builder made and the two calls retail made, with the
    // solo latch clear at both.
    let got: Vec<_> = calls.iter().map(|c| (c.1, c.2, c.3, c.4, c.5)).collect();
    assert_eq!(
        got,
        vec![(1, 0x2B, 0x0F, 3, 0), (1, 0x2A, 0x00, 7, 0)],
        "(cslot, action, flag, cursor, ctx[+0x243]) per call"
    );
    assert_eq!(calls[0].6, [0x0F, 0x0E, 0x19, 0x2B, 0x0F, 0x0E, 0x19, 0x2A]);

    // Each call starts its clip on the same tick, from XA4.XA, and it is the
    // commit of the starter's clip (`0x19` -> dynamic slot `0x10`).
    assert_eq!(clips.len(), 2, "one clip per call: {clips:?}");
    for (call, clip) in calls.iter().zip(&clips) {
        assert_eq!(clip.0, call.0, "the clip starts on the call's tick");
        assert_eq!(clip.1, 3, "Noa's shout bank is clip slot 3 (XA4.XA)");
        assert!(
            timeline.contains(&(call.0, 0x10)),
            "the call rides the commit of the starter's clip: {timeline:?}"
        );
    }

    // The channel is a rand() pick: the engine's must sit in the pool the
    // call reads, with the duration law's span; retail's two picks sit in
    // the same pools and reproduce retail's spans.
    use legaia_engine_battle::arts_voice_cue::{ArtsVoiceCall, first_half_pool, shout_dur};
    let tables = session
        .host
        .world
        .audio
        .arts_voice_tables
        .clone()
        .expect("tables");
    const RETAIL_PICKS: [(u8, u32); 2] = [(0x4, 0x36), (0xF, 0x21)];
    for ((call, clip), (retail_ch, retail_dur)) in calls.iter().zip(&clips).zip(RETAIL_PICKS) {
        let pool = first_half_pool(
            &tables,
            &ArtsVoiceCall {
                char_id: 2,
                action: call.2,
                flag: call.3,
                solo_latch: call.5,
                first_enemy: None,
                caller_seat: 1,
            },
        )
        .expect("a first-half art");
        let ch = clip.2 as u8;
        assert!(pool.contains(&ch), "engine pick {ch:#x} in {pool:02X?}");
        assert_eq!(clip.3, shout_dur(&tables, 1, ch), "engine span");
        assert!(
            pool.contains(&retail_ch),
            "retail pick {retail_ch:#x} in {pool:02X?}"
        );
        assert_eq!(shout_dur(&tables, 1, retail_ch), retail_dur, "retail span");
    }
}
