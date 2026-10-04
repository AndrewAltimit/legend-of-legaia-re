//! Battle **item use** presentation, end to end from the pad: which clip the
//! acting character plays and which sound cues the turn raises.
//!
//! Retail's command ring stamps the action's staged clip `actor[+0x1E7]` at
//! the commit, per arm (`FUN_801D0748`, `overlay_battle_action_801d0748.txt`):
//!
//! | arm | category `+0x1DE` | clip `+0x1E7` | site |
//! |---|---|---|---|
//! | Item (Up) | `1` | `9` | `li v0,0x9` / `sb v0,0x1e7(v1)` at `0x801D13E4..0x801D13E8` |
//! | Magic (Right) | `2` | `9` | `0x801D14BC..0x801D14C0` |
//! | Spirit (Down) | `4` | `0x10` | `0x801D16A8..0x801D16B0` |
//!
//! and action state `0x3C` stages that byte as the queued clip
//! (`lbu v0,0x1e7(s3)` / `sb v0,0x1da(s3)` at `0x801E3B4C..0x801E3B54`).
//!
//! The port stamped only the Spirit arm's byte and never reset it, so an item
//! used after a Spirit turn re-staged the **Spirit** clip - and with it the
//! Spirit clip's own cue track, which is the sound the player heard.
//!
//! Disc-gated: the clips and their cue tracks come off the player battle
//! files.

use std::path::PathBuf;

use legaia_engine_core::battle_input::{AttackMode, BattleCommand, CommandPhase, RoundChoice};
use legaia_engine_core::input::{InputState, PadButton};
use legaia_engine_core::world::SceneMode;
use legaia_engine_shell::boot::{BootConfig, BootSession, FieldLiveOpts};

const SCENE: &str = "town01";
const TRAINING_FORMATION_ID: u16 = 4;
const ITEM_HEALING_LEAF: u8 = 0x77;
const SETTLE_TICKS: usize = 9000;

/// Retail's item / magic commit clip (`+0x1E7 = 9`).
const ITEM_COMMIT_CLIP: u8 = 9;

fn extracted_dir() -> Option<PathBuf> {
    let env = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from);
    let rel = ["extracted", "../extracted", "../../extracted"].map(PathBuf::from);
    env.into_iter()
        .chain(rel)
        .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists())
}

fn tap(session: &mut BootSession, button: PadButton) {
    session.host.world.set_pad(InputState::mask_of([button]));
    let _ = session.tick();
    session.host.world.set_pad(0);
    let _ = session.tick();
}

/// Everything audible or visible one tick raised for slot 0.
#[derive(Default, Debug)]
struct Trace {
    clips: Vec<u8>,
    sfx: Vec<u16>,
    xa: Vec<(u32, u32)>,
    shouts: usize,
}

impl Trace {
    fn record(&mut self, session: &mut BootSession) {
        let w = &mut session.host.world;
        let clip = w.actors[0].battle.current_anim;
        if clip != 0 && self.clips.last() != Some(&clip) {
            self.clips.push(clip);
        }
        self.sfx
            .extend(w.drain_battle_sfx_cues().into_iter().map(|c| c.kind));
        self.xa.extend(
            w.drain_battle_xa_cues()
                .into_iter()
                .map(|c| (c.clip, c.channel)),
        );
        self.shouts += w.drain_battle_shout_cues().len();
    }
}

/// Tick with a neutral pad (taking the commit confirm's `Begin`) until `f`
/// holds, tracing every tick.
fn settle(session: &mut BootSession, trace: &mut Trace, f: impl Fn(&BootSession) -> bool) -> bool {
    for _ in 0..SETTLE_TICKS {
        if f(session) {
            return true;
        }
        let confirm_up = matches!(
            session.host.world.battle.command.as_ref().map(|c| &c.phase),
            Some(CommandPhase::CommitConfirm { .. })
        );
        if confirm_up {
            tap(session, PadButton::Cross);
        } else {
            session.host.world.set_pad(0);
            let _ = session.tick();
        }
        trace.record(session);
    }
    f(session)
}

fn command_open(s: &BootSession) -> bool {
    s.host
        .world
        .battle
        .command
        .as_ref()
        .is_some_and(|c| !matches!(c.phase, CommandPhase::CommitConfirm { .. }))
}

fn pick(session: &mut BootSession, want: BattleCommand) {
    for _ in 0..64 {
        let press = {
            let Some(cmd) = session.host.world.battle.command.as_ref() else {
                return;
            };
            match cmd.phase {
                CommandPhase::RoundPrompt { .. } => {
                    if cmd.round_choice() == Some(RoundChoice::Begin) {
                        PadButton::Cross
                    } else {
                        PadButton::Left
                    }
                }
                CommandPhase::Menu { .. } => {
                    if cmd.menu_command() == Some(want) {
                        PadButton::Cross
                    } else {
                        match want {
                            BattleCommand::Item => PadButton::Up,
                            BattleCommand::Attack => PadButton::Left,
                            BattleCommand::Magic => PadButton::Right,
                            _ => PadButton::Down,
                        }
                    }
                }
                CommandPhase::AttackMode { .. } => {
                    if cmd.attack_mode() == Some(AttackMode::Auto) {
                        PadButton::Cross
                    } else {
                        PadButton::Left
                    }
                }
                _ => return,
            }
        };
        tap(session, press);
    }
    panic!("never reached {want:?}");
}

fn boot_into_battle() -> Option<BootSession> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return None;
    };
    let cfg = BootConfig {
        scene: SCENE.to_string(),
        enable_audio: false,
    };
    let mut session = BootSession::open(&extracted, &cfg).expect("open boot session");
    session
        .enter_field_live(
            SCENE,
            &FieldLiveOpts {
                live_loop: true,
                player_battle: true,
                ..Default::default()
            },
        )
        .expect("enter field live");
    let w = &mut session.host.world;
    w.party.inventory.insert(ITEM_HEALING_LEAF, 9);
    for i in 0..w.party.party_count as usize {
        w.actors[i].battle.max_hp = 60000;
        w.actors[i].battle.hp = 30000;
    }
    assert_eq!(
        session
            .host
            .world
            .install_man_formation(TRAINING_FORMATION_ID),
        Some(TRAINING_FORMATION_ID)
    );
    assert!(session.host.world.on_field_step());
    let mut t = Trace::default();
    assert!(settle(&mut session, &mut t, |s| s.host.world.mode
        == SceneMode::Battle));
    let w = &mut session.host.world;
    for i in w.party.party_count as usize..w.actors.len() {
        if w.actors[i].battle.max_hp > 0 {
            w.actors[i].battle.max_hp = 60000;
            w.actors[i].battle.hp = 60000;
        }
    }
    assert!(settle(&mut session, &mut t, command_open));
    Some(session)
}

/// Use one Healing Leaf from the open command ring and trace the turn until
/// the next command session opens.
fn use_healing_leaf(session: &mut BootSession) -> Trace {
    let carried = session
        .host
        .world
        .party
        .inventory
        .get(&ITEM_HEALING_LEAF)
        .copied()
        .unwrap_or(0);
    pick(session, BattleCommand::Item);
    assert!(session.host.world.battle.item_menu.is_some());
    let mut trace = Trace::default();
    for _ in 0..12 {
        if session.host.world.battle.item_menu.is_none() {
            break;
        }
        tap(session, PadButton::Cross);
        trace.record(session);
    }
    assert!(settle(session, &mut trace, command_open));
    assert!(
        session
            .host
            .world
            .party
            .inventory
            .get(&ITEM_HEALING_LEAF)
            .copied()
            .unwrap_or(0)
            < carried,
        "the leaf was used"
    );
    trace
}

fn spirit_turn(session: &mut BootSession) -> Trace {
    pick(session, BattleCommand::Spirit);
    let mut trace = Trace::default();
    assert!(settle(session, &mut trace, command_open));
    trace
}

#[test]
fn an_item_turn_plays_the_item_clip_even_after_a_spirit_turn() {
    let Some(mut session) = boot_into_battle() else {
        return;
    };
    eprintln!("[ran] town01 sparring battle");
    let first = use_healing_leaf(&mut session);
    eprintln!("item (fresh): {first:?}");
    let spirit = spirit_turn(&mut session);
    eprintln!("spirit: {spirit:?}");
    let after = use_healing_leaf(&mut session);
    eprintln!("item (after spirit): {after:?}");

    assert!(
        !spirit.clips.is_empty() && !spirit.clips.contains(&ITEM_COMMIT_CLIP),
        "the Spirit turn plays its own clip: {spirit:?}"
    );
    for (label, item) in [("fresh", &first), ("after spirit", &after)] {
        assert_eq!(
            item.clips,
            [ITEM_COMMIT_CLIP],
            "item turn ({label}) plays clip 9 and only clip 9: {item:?}"
        );
        // Before the fix the post-Spirit item turn replayed the Spirit clip
        // and, through its cue track, the Spirit clip's XA cue - the sound
        // the player reported.
        assert!(
            !item.xa.iter().any(|c| spirit.xa.contains(c)),
            "item turn ({label}) must not raise the Spirit clip's cues: \
             item {item:?} spirit {spirit:?}"
        );
    }
    // The item turn sounds like itself whatever came before it.
    assert_eq!(
        first.sfx, after.sfx,
        "an item turn's cues do not depend on the previous command"
    );
    // The cast-cue band's voice: Vahn (roster id 1) on a class-0/1 item is
    // cue `0x108`, which the dispatcher sends to CD-XA clip slot `0x1A`
    // (`(0x108 - 0x100) >> 3 == 1 -> 0x1A`), channel `0`.
    for (label, item) in [("fresh", &first), ("after spirit", &after)] {
        assert!(
            item.xa.iter().any(|&(clip, _)| clip == VAHN_VOICE_SLOT),
            "item turn ({label}) raises Vahn's cast voice: {item:?}"
        );
    }
    assert!(
        !spirit.xa.iter().any(|&(clip, _)| clip == VAHN_VOICE_SLOT),
        "a Spirit turn never reaches the cast-cue band (state 0x3D): {spirit:?}"
    );
}

/// The CD-XA clip slot the cast-cue band's `0x108..0x10F` ids land on.
const VAHN_VOICE_SLOT: u32 = 0x1A;
