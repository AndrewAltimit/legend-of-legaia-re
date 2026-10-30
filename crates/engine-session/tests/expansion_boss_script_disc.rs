//! Disc-gated: the Rebirth boss scripts against the disc's own monster
//! records, and the Cinder Warden fought on auto through the native session
//! to its below-half phase change.
//!
//! Every scripted move must be backed by retail data: a swing index inside
//! the base monster's attack list (its archive swing records), a spell id
//! the disc spell table resolves. The fight then checks the phase change
//! reaches the battle: the script's phase moves on, its banner line shows,
//! and the fight still resolves.
//! Skips when `LEGAIA_DISC_BIN` is unset.

use std::sync::Arc;

use legaia_engine_cheats::cheats::{PlayerCheat, WorldCheats};
use legaia_engine_expansion::boss_script::MoveKind;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_expansion_format::ExpansionPack;

const MAP_ID: &str = "rebirth_tower_1f";
const BATTLE_TICKS: u32 = 150_000;

fn open() -> Option<NativeSession> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = ["extracted", "../extracted", "../../extracted"]
        .into_iter()
        .map(std::path::PathBuf::from)
        .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists());
    let Some(extracted) = extracted else {
        eprintln!("[skip] extracted/ missing");
        return None;
    };
    let cfg = BootConfig {
        scene: "town01".into(),
        enable_audio: false,
    };
    let mut session = NativeSession::open(&extracted, &cfg).expect("open boot session");
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/rebirth");
    let files = legaia_expansion_format::read_files(std::path::Path::new(dir)).expect("read");
    let pack = ExpansionPack::from_files(&files).expect("the Rebirth pack parses");
    session
        .host
        .activate_expansion_pack(Arc::new(pack))
        .expect("the Rebirth pack activates");
    let opts = FieldLiveOpts {
        live_loop: true,
        player_battle: false,
        battle_bgm: None,
    };
    session
        .enter_expansion_map_live(MAP_ID, "", &opts)
        .expect("enter the pack map");
    Some(session)
}

#[test]
fn every_rebirth_scripted_move_is_backed_by_disc_data() {
    let Some(session) = open() else {
        return;
    };
    let w = &session.host.world;
    let compiled = w
        .expansion
        .battle
        .compiled
        .clone()
        .expect("the pack compiled");
    let mut scripted = 0;
    let mut missing = Vec::new();
    for f in &compiled.formations {
        for (&id, script) in &f.scripts {
            scripted += 1;
            let def = w
                .tables
                .monster_catalog
                .get(id)
                .unwrap_or_else(|| panic!("{}: base monster {id} merged", f.key));
            let phases = script.phases.iter().chain(script.enrage.iter());
            for m in phases.flat_map(|p| p.moves.iter().flatten()) {
                match m.kind {
                    MoveKind::Attack { index, .. } => assert!(
                        usize::from(index) < def.action_entries.len(),
                        "{}: monster {id} ({}) has {} swings, the script names swing {index}",
                        f.key,
                        def.name,
                        def.action_entries.len()
                    ),
                    MoveKind::Spell(sid) => {
                        // What the monster cast path resolves: the catalog,
                        // or a capture-class special off the disc table.
                        let special = w
                            .menu
                            .text
                            .as_ref()
                            .and_then(|t| t.spell_names.as_ref())
                            .and_then(|t| {
                                legaia_engine_battle::retail_magic::capture_special_def(t, sid)
                            });
                        if w.tables.spell_catalog.get(sid).is_none() && special.is_none() {
                            missing.push(format!("{}: 0x{sid:02X}", f.key));
                        }
                    }
                }
            }
            for &s in script.phases.iter().flat_map(|p| p.summon.iter()) {
                assert!(
                    w.tables.monster_catalog.get(s).is_some(),
                    "{}: summon {s} merged",
                    f.key
                );
            }
        }
    }
    assert!(
        missing.is_empty(),
        "spells the disc spell table does not hold: {missing:?}"
    );
    eprintln!("[ran] {scripted} scripted formation seats checked");
    assert!(scripted >= 6, "the six Rebirth boss scripts");
}

/// What one boss fight showed.
#[derive(Debug, Default)]
struct Seen {
    phases: Vec<usize>,
    banners: Vec<String>,
    /// Spell ids a scripted seat armed.
    casts: Vec<u8>,
    resolved: bool,
}

/// Fight Rebirth boss `boss` on auto at `permille` of its stats, a level-99
/// party at full strength.
fn fight(boss: &str, permille: u16, seed: u32) -> Option<Seen> {
    let mut session = open()?;
    let w = &mut session.host.world;
    w.set_active_party(vec![0, 1, 2]);
    w.apply_cheat(&PlayerCheat::PartyLevel(99), None);
    w.apply_cheat(&PlayerCheat::Restore, None);
    w.toggles.enemy_stat_permille = permille;
    w.rng_state = seed;
    assert!(w.start_expansion_boss(boss), "{boss} starts");
    let mut seen = Seen::default();
    let mut in_battle = false;
    let mut last_cat = [0u8; 8];
    for tick in 0..BATTLE_TICKS {
        session.host.world.set_pad(0);
        session.tick().expect("tick");
        let w = &session.host.world;
        if w.mode == SceneMode::Battle {
            in_battle = true;
            for (&slot, (_, run)) in w.expansion.battle.boss_runs.iter() {
                if let Some(p) = run.phase
                    && !seen.phases.contains(&p)
                {
                    seen.phases.push(p);
                }
                let a = &w.actors[slot];
                let cat = a.battle.action_category;
                if cat == 2 && last_cat[slot] != 2 {
                    seen.casts.push(a.battle.params[0]);
                }
                last_cat[slot] = cat;
            }
            if let Some(b) = &w.battle.message_banner
                && seen.banners.last() != Some(&b.text)
            {
                seen.banners.push(b.text.clone());
            }
        } else if in_battle {
            seen.resolved = true;
            eprintln!("[ran] {boss}: resolved after {tick} ticks; {seen:?}");
            break;
        }
        assert!(
            !w.game_over && !w.game_over_hold,
            "{boss}: party wiped at tick {tick}; {seen:?}"
        );
    }
    Some(seen)
}

/// Every Rebirth boss, fought at a fraction of its stats, crosses its
/// first phase change, shows that phase's banner and resolves - a scripted
/// cast of a capture-class special (the streamed cast modules) included.
#[test]
fn every_rebirth_boss_reaches_a_phase_change_and_resolves() {
    for (boss, banner) in [
        ("verdant_colossus", "Roots knot the Colossus whole again!"),
        ("iron_regent", "\"Guards! To your Regent!\""),
        ("azure_seraph", "The Seraph's wings turn to storm!"),
        ("hollow_sovereign", "\"You fight a shell. I am beneath.\""),
        ("throne_echo", "The Unbound tears its chains apart!"),
    ] {
        let Some(seen) = fight(boss, 150, 0x5EED_0002) else {
            return;
        };
        assert!(seen.resolved, "{boss} never resolved: {seen:?}");
        assert!(seen.phases.contains(&1), "{boss}: {seen:?}");
        assert!(seen.banners.iter().any(|b| b == banner), "{boss}: {seen:?}");
    }
}

#[test]
fn the_cinder_warden_reaches_its_second_phase() {
    let Some(mut session) = open() else {
        return;
    };
    let w = &mut session.host.world;
    w.set_active_party(vec![0, 1, 2]);
    w.apply_cheat(&PlayerCheat::PartyLevel(99), None);
    w.apply_cheat(&PlayerCheat::Restore, None);
    // A tenth of the Warden's HP leaves the fight short but still crosses
    // the half-HP threshold on a real hit rather than in one blow.
    w.toggles.enemy_stat_permille = 100;
    w.rng_state = 0x5EED_0001;
    assert!(w.start_expansion_boss("cinder_warden"));

    let mut in_battle = false;
    let mut phases: Vec<usize> = Vec::new();
    let mut banners: Vec<String> = Vec::new();
    for tick in 0..BATTLE_TICKS {
        session.host.world.set_pad(0);
        session.tick().expect("tick");
        let w = &session.host.world;
        if w.mode == SceneMode::Battle {
            in_battle = true;
            for (_, run) in w.expansion.battle.boss_runs.values() {
                if let Some(p) = run.phase
                    && phases.last() != Some(&p)
                {
                    phases.push(p);
                }
            }
            if let Some(b) = &w.battle.message_banner
                && banners.last() != Some(&b.text)
            {
                banners.push(b.text.clone());
            }
        } else if in_battle {
            eprintln!(
                "[ran] warden fight resolved after {tick} ticks; phases {phases:?}; banners {banners:?}"
            );
            break;
        }
        assert!(
            !w.game_over && !w.game_over_hold,
            "party wiped at tick {tick}; phases {phases:?}"
        );
    }
    assert!(in_battle, "the boss fight started");
    assert_eq!(phases.first(), Some(&0), "{phases:?}");
    assert!(phases.contains(&1), "the half-HP phase began: {phases:?}");
    assert!(
        banners
            .iter()
            .any(|b| b == "The Warden's armour splits in fire!"),
        "{banners:?}"
    );
    assert_ne!(
        session.host.world.mode,
        SceneMode::Battle,
        "the fight resolved"
    );
}
