//! Disc-gated regression: the two boss fights the full-game ladder found
//! parked forever in the attack short step (`0x19`) must fight to a finish.
//!
//! Monster 169 (formation row 9 in `taiku`) and monster 168 (row 6 in
//! `rugi`) stalled with the monster walking at a party member it could never
//! reach. The cause was not the approach itself: the engine held each
//! actor's `+0x3C`/`+0x40` pair still for the length of an action, where
//! retail's pose decoder (`FUN_8004998C`) re-derives it from the live pair on
//! every drawn frame. The separation pass measures overlap on that pair and
//! nudges the live pairs, so two party members whose held pairs overlapped
//! were pushed apart every frame without the overlap ever clearing, until
//! they stood tens of thousands of units off the stage and the monster's
//! approach - measured against the stale pair - never came in range.
//!
//! Each fight is entered through the retail scripted-battle arm with a
//! three-member party given enough HP to outlast many monster turns, then
//! driven with pad presses only. The oracle: the battle resolves (win or
//! wipe) within the tick budget, no approach state holds for longer than any
//! real walk takes, and no party member leaves the stage.
//!
//! Skip-passes without `LEGAIA_DISC_BIN` (and an extracted tree or the disc
//! image itself to boot from).

use std::path::PathBuf;

use legaia_engine_core::input::PadButton;
use legaia_engine_core::world::SceneMode;
use legaia_engine_shell::boot::{BootConfig, BootSession, FieldLiveOpts};
use legaia_engine_vm::battle_action::ActionState;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn open_session() -> Option<BootSession> {
    let disc = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from);
    let Some(disc) = disc.filter(|p| p.exists()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or missing (disc-gated convention)");
        return None;
    };
    let cfg = BootConfig {
        scene: legaia_engine_shell::boot::DEFAULT_BOOT_SCENE.to_string(),
        enable_audio: false,
    };
    let mut candidates = Vec::new();
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR") {
        candidates.push(PathBuf::from(d));
    }
    candidates.push(repo_root().join("extracted"));
    for d in candidates {
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(BootSession::open(&d, &cfg).expect("open BootSession (extracted)"));
        }
    }
    Some(BootSession::open_disc(&disc, &cfg).expect("open BootSession (disc)"))
}

/// The presentation queues a play host drains after every tick.
fn host_drains(session: &mut BootSession) {
    let w = &mut session.host.world;
    let _ = w.drain_field_events();
    let _ = w.drain_battle_events();
    let _ = w.drain_battle_hit_fx();
    let _ = w.drain_battle_hit_events();
    let _ = w.drain_battle_sfx_cues();
    let _ = w.drain_battle_shout_cues();
    let _ = w.drain_battle_xa_cues();
    let _ = w.drain_battle_xa_prestage();
    let _ = w.route_battle_effect_spawns();
    let _ = w.drain_battle_clut_stages();
    let _ = w.drain_minigame_sfx_cues();
}

/// The route ladder's fighter: Begin, Attack, Auto, confirm the target,
/// confirm a message box, back out of a bag - every one a pad press.
fn fight_pad(session: &BootSession) -> u16 {
    use legaia_engine_core::battle_input::CommandPhase;
    let w = &session.host.world;
    if !w.battle.tutorial_boxes.is_empty() {
        return PadButton::Cross.mask();
    }
    if w.battle.item_menu.is_some() {
        return PadButton::Circle.mask();
    }
    if let Some(cmd) = w.battle.command.as_ref() {
        return match &cmd.phase {
            CommandPhase::RoundPrompt { .. }
            | CommandPhase::Menu { .. }
            | CommandPhase::AttackMode { .. }
            | CommandPhase::CommitConfirm { .. } => PadButton::Left.mask(),
            CommandPhase::Targeting { .. } => PadButton::Cross.mask(),
            _ => 0,
        };
    }
    PadButton::Cross.mask()
}

/// Longest a real approach walk takes: the widest formation gap is under
/// 3000 units and the slowest walk covers ~10 units a frame.
const APPROACH_HOLD_LIMIT: u32 = 900;
/// Tick budget for the whole fight. Every swing plays its real clip length
/// (the engine installs the party's battle forms at battle entry), so a
/// padded-HP party's fight to a wipe runs past sixty thousand ticks - and
/// past a hundred thousand once defenders block (`FUN_801EC3E4`'s block
/// roll: a blocked hit lands no damage, so the boss's HP falls slower).
const BATTLE_TICKS: u32 = 240_000;
/// No battle position leaves this box (the stage is a few thousand units
/// across; the parked fights had members at +-17000).
const STAGE_BOUND: i16 = 6000;

fn fight(scene: &str, row: u8, monster: u16) {
    let Some(mut session) = open_session() else {
        return;
    };
    session.begin_new_game();
    let opts = FieldLiveOpts {
        live_loop: false,
        player_battle: true,
        battle_bgm: None,
    };
    session
        .enter_scene_live(scene, &opts)
        .unwrap_or_else(|e| panic!("enter {scene}: {e:#}"));
    let starting = session
        .starting_party
        .clone()
        .expect("SCUS starting-party template");
    {
        let w = &mut session.host.world;
        // Noa and Gala from the same SCUS template retail seeds them from
        // (the `play-window --party` harness path).
        assert_eq!(w.seed_party_members(&starting, &[0, 1, 2]), 2);
        let mut roster = w.party.roster.clone();
        for rec in roster.members.iter_mut().take(3) {
            let mut hms = rec.hp_mp_sp();
            hms.hp_max = 9999;
            hms.hp_cur = 9999;
            rec.set_hp_mp_sp(hms);
        }
        w.load_party(roster);
        w.set_active_party(vec![0, 1, 2]);
        let ids: Vec<u16> = w
            .tables
            .formation_table
            .formation(u16::from(row))
            .map(|f| f.slots.iter().map(|s| s.monster_id).collect())
            .unwrap_or_default();
        assert!(
            ids.contains(&monster),
            "{scene} row {row} carries {ids:?}, not monster {monster}"
        );
        assert!(w.trigger_scripted_battle(row), "{scene} row {row} entry");
    }

    let mut prev = 0u16;
    let mut in_battle = false;
    let mut hold = 0u32;
    let mut longest_hold = 0u32;
    let mut ticks = 0u32;
    let mut resolved = None;
    while ticks < BATTLE_TICKS {
        let want = if session.host.world.mode == SceneMode::Battle {
            fight_pad(&session)
        } else {
            0
        };
        let pad = if prev == 0 { want } else { 0 };
        prev = pad;
        session.host.world.set_pad(pad);
        session.tick().expect("tick");
        host_drains(&mut session);
        ticks += 1;
        let w = &session.host.world;
        if w.mode == SceneMode::Battle {
            in_battle = true;
            let state = ActionState::from_byte(w.battle_ctx.action_state);
            let approaching = matches!(
                state,
                Some(
                    ActionState::AttackShortStep
                        | ActionState::AttackAdvance
                        | ActionState::AttackWindup
                )
            );
            hold = if approaching { hold + 1 } else { 0 };
            longest_hold = longest_hold.max(hold);
            assert!(
                hold <= APPROACH_HOLD_LIMIT,
                "{scene} F{row}[{monster}]: approach state {state:?} held {hold} frames \
                 (actor {}) at tick {ticks}",
                w.battle_ctx.active_actor
            );
            for (i, a) in w.actors.iter().enumerate().take(8) {
                if a.battle.max_hp == 0 {
                    continue;
                }
                let (x, z) = (a.move_state.world_x, a.move_state.world_z);
                assert!(
                    x.abs() <= STAGE_BOUND && z.abs() <= STAGE_BOUND,
                    "{scene} F{row}[{monster}]: slot {i} left the stage at ({x},{z}), tick {ticks}"
                );
            }
        }
        if w.game_over || w.game_over_hold {
            resolved = Some("party wiped");
            break;
        }
        if in_battle && w.mode != SceneMode::Battle {
            resolved = Some("battle ended");
            break;
        }
    }
    let outcome = resolved.unwrap_or_else(|| {
        let w = &session.host.world;
        panic!(
            "{scene} F{row}[{monster}]: unresolved after {ticks} ticks in state {:#04x} (actor {})",
            w.battle_ctx.action_state, w.battle_ctx.active_actor
        )
    });
    eprintln!(
        "[boss-approach] {scene} F{row}[{monster}]: {outcome} after {ticks} ticks; \
         longest approach hold {longest_hold} frames"
    );
}

#[test]
fn zora_in_taiku_fights_to_a_finish() {
    fight("taiku", 9, 169);
}

#[test]
fn rogue_in_rugi_fights_to_a_finish() {
    fight("rugi", 6, 168);
}
