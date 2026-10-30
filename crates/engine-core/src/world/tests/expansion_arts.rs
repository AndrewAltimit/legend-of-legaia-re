//! Pack arts and spells on a disc-free world (`world::expansion_arts`): the
//! bank-walk extension byte-exact against retail's builder, retail arts
//! tokenizing unchanged beside pack arts, segment expansion and chains, the
//! Spirit charge, the borrowed clips in the art bank and the commit latch,
//! the hit and cast seams, learning, Seru capture of a pack monster, the
//! save round trip, the New Game+ carry, and no pack leaving every hook the
//! identity.

use std::sync::Arc;

use super::*;
use legaia_art::Command::{Down, Left, Right, Up};
use legaia_engine_mode::mode::SceneMode;
use legaia_expansion_format::{ExpansionPack, PackFiles};

const ARTS_TOML: &str = r#"
[arts.ember]
character = "vahn"
name = "Ember Cascade"
commands = "RURDU"
ap = 30
segments = [{ art = 0x23, power = ["U22", "L22"] }, { art = 0x27, power = ["L28"] }]
damage_pct = 150
rider = { status = "venom", chance = 100 }
learn = { level = 10 }

[arts.finale]
character = "vahn"
name = "Rebirth Blaze"
ap = 20
segments = [{ art = 0x2A }]
learn = { level = 80 }

[arts.late]
character = "vahn"
name = "Late Art"
commands = "UUUD"
segments = [{ art = 0x27 }]
learn = { level = 90 }

[chains.finale]
character = "vahn"
arts = ["ember", 0x29]
finisher = "finale"
"#;

const SPELLS_TOML: &str = r#"
[spells.wyrm]
name = "Cinder Wyrm"
description = "A wyrm.|Of embers."
base = 0x81
mp = 34
power = 200
growth = 10
element = 2
status = { status = "venom", chance = 100 }
learn = { capture = "drake" }

[spells.aegis]
name = "Aegis"
base = 0x83
"#;

const BATTLE_TOML: &str = r#"
[monsters.drake]
base = { kind = "monster", id = 9 }
name = "Ember Drake"

[formations.den]
monsters = ["drake"]
"#;

fn pack() -> Arc<ExpansionPack> {
    let mut files = PackFiles::new();
    files.insert(
        "pack.toml".into(),
        b"id = \"t\"\nname = \"T\"\nversion = \"1.0.0\"\n".to_vec(),
    );
    files.insert("arts.toml".into(), ARTS_TOML.as_bytes().to_vec());
    files.insert("spells.toml".into(), SPELLS_TOML.as_bytes().to_vec());
    files.insert("battle.toml".into(), BATTLE_TOML.as_bytes().to_vec());
    Arc::new(ExpansionPack::from_files(&files).expect("pack parses"))
}

fn record(action: u8, commands: &[legaia_art::Command]) -> legaia_art::ArtRecord {
    legaia_art::ArtRecord {
        action: legaia_art::ActionConstant::from_byte(action).expect("art constant"),
        commands: commands.to_vec(),
        anim_index: 0,
        anim_extra: vec![1, 0, 0, 0],
        name: None,
        power: vec![],
        dmg_timing: vec![],
        effect_cues: Default::default(),
        hit_cues: vec![],
        identifier: 0,
        anim_speed: 0,
        enemy_effect: legaia_art::EnemyEffect::None,
        repeat_frames: Default::default(),
        background: 0,
        runtime_address: None,
    }
}

/// Vahn's retail normal arts with the combos the arts-name table spells
/// (`data/gamedata/arts.toml`: Arms = Left, Ra-Seru = Right, High = Up, Low
/// = Down), and his bank head.
fn vahn_retail() -> Vec<(u8, Vec<legaia_art::Command>)> {
    vec![
        (
            0x1B,
            vec![Right, Down, Left, Up, Left, Up, Right, Down, Left],
        ),
        (0x1C, vec![Right, Down, Left, Down, Left]),
        (0x1D, vec![Right, Right, Down, Left]),
        (0x1E, vec![Right, Right, Left]),
        (0x1F, vec![Down, Up, Up, Up]),
        (0x20, vec![Up, Up, Down, Down]),
        (0x21, vec![Down, Up, Up, Left]),
        (0x22, vec![Up, Down, Right, Left]),
        (0x23, vec![Left, Right, Up, Left]),
        (0x24, vec![Down, Down, Down, Up]),
        (0x25, vec![Left, Left, Down]),
        (0x26, vec![Up, Down, Left]),
        (0x27, vec![Up, Down, Up]),
        (0x28, vec![Down, Right, Up]),
        (0x29, vec![Left, Right, Up]),
    ]
}

fn clip(tag: u8, power: [u8; 4], beats: [u8; 4]) -> MonsterAnimation {
    use legaia_actor_formats::monster_archive::PartPose;
    let mut head = vec![0u8; 0x88];
    head[..4].copy_from_slice(&power);
    head[0x10..0x14].copy_from_slice(&beats);
    MonsterAnimation {
        variant_track: Default::default(),
        action_id: tag,
        rate: 2,
        attach_key: 0,
        solo_flag: 0,
        impact_class: 0,
        effect_script: head,
        part_count: 1,
        frame_count: 20,
        frames: (0..20)
            .map(|_| {
                vec![PartPose {
                    tx: tag as i16,
                    ty: 0,
                    tz: 0,
                    rx: 0,
                    ry: 0,
                    rz: 0,
                }]
            })
            .collect(),
    }
}

/// A one-member battle world with Vahn's retail bank installed.
fn arts_world() -> World {
    let mut w = World::new();
    while w.actors.len() < 4 {
        w.actors.push(Actor::default());
    }
    w.party.party_count = 1;
    let mut party = legaia_save::Party::zeroed(1);
    let mut hms = party.members[0].hp_mp_sp();
    hms.hp_max = 900;
    hms.hp_cur = 900;
    party.members[0].set_hp_mp_sp(hms);
    w.load_party(party);
    w.mode = SceneMode::Battle;
    for i in 0..4 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 500;
        w.actors[i].battle.max_hp = 500;
        w.actors[i].battle.liveness = 1;
    }
    let ch = legaia_art::Character::Vahn;
    for (b, cmds) in vahn_retail() {
        let rec = record(b, &cmds);
        w.set_art_record(ch, rec.action, rec);
    }
    w.actors[0].battle.character = ch;
    w.actors[0].battle.spirit_gauge = 100;
    w
}

fn with_pack(mut w: World) -> World {
    w.activate_expansion(pack()).expect("pack activates");
    w
}

/// Every retail art's combo, alone and behind a leading arrow, tokenizes to
/// the same queue with the pack active and every pack art known as with no
/// pack: the pack rows sit past retail's 24 and are tried last.
#[test]
fn retail_arts_tokenize_exactly_as_without_a_pack() {
    let mut inputs: Vec<Vec<legaia_art::Command>> =
        vahn_retail().into_iter().map(|(_, c)| c).collect();
    for (_, c) in vahn_retail() {
        let mut lead = vec![Down];
        lead.extend(c);
        inputs.push(lead);
    }
    inputs.push(vec![Up, Down, Up, Left, Right, Up]);
    for input in &inputs {
        let mut plain = arts_world();
        let (q0, a0, m0) = plain.build_arts_action_queue(0, input);
        let mut packed = with_pack(arts_world());
        assert!(packed.expansion_teach_art("ember"));
        assert!(packed.expansion_teach_art("finale"));
        let (q1, a1, m1) = packed.build_arts_action_queue(0, input);
        assert_eq!(q0, q1, "{input:?}");
        assert_eq!(a0, a1, "{input:?}");
        assert_eq!(m0, m1, "{input:?}");
    }
}

/// `R U R D U` with Ember Cascade known: the walk keeps the leading arrows,
/// writes the `0x19` starter over the run's last one and the art's first
/// byte after it, then the second segment follows - byte-exact.
#[test]
fn a_pack_art_builds_its_queue_byte_exact() {
    let mut w = with_pack(arts_world());
    let input = [Right, Up, Right, Down, Up];
    // Not known: plain swings.
    let (q, _, _) = w.build_arts_action_queue(0, &input);
    assert_eq!(&q[..6], &[0x0D, 0x0F, 0x0D, 0x0E, 0x0F, 0x00]);
    assert!(w.expansion_teach_art("ember"));
    let (q, actions, marks) = w.build_arts_action_queue(0, &input);
    assert_eq!(&q[..7], &[0x0D, 0x0F, 0x0D, 0x0E, 0x19, 0x33, 0x34]);
    assert_eq!(q[7], 0);
    assert_eq!(marks[4], 1, "the normal art's starter mark");
    assert!(actions.is_empty(), "no retail art performed");
    assert_eq!(w.expansion.arts.turn_performed, [0]);
}

/// `R U R D U` then `L R U` (Hyper Elbow): the chain appends `0x1A` (marked
/// for the SUPER banner) and the finisher's byte, once the finisher is
/// known; Spirit pays both pack arts.
#[test]
fn a_chain_appends_its_finisher_and_both_arts_are_charged() {
    let mut w = with_pack(arts_world());
    w.expansion_teach_art("ember");
    let input = [Right, Up, Right, Down, Up, Left, Right, Up];
    let (q, actions, marks) = w.build_arts_action_queue(0, &input);
    // Hyper Elbow is new to the record: learn-on-use gives it the `0x1A`
    // starter this once (`0x801EF6F0`).
    let mut want = [
        0x0D, 0x0F, 0x0D, 0x0E, 0x19, 0x33, 0x34, 0x0C, 0x0D, 0x1A, 0x29,
    ];
    assert_eq!(&q[..11], &want);
    assert_eq!(q[11], 0, "finisher unknown: no chain");
    assert_eq!(
        actions.iter().map(|a| a.as_byte()).collect::<Vec<_>>(),
        [0x29]
    );
    let _ = marks;
    w.expansion_teach_art("finale");
    let (q, _, marks) = w.build_arts_action_queue(0, &input);
    want[9] = 0x19;
    assert_eq!(&q[..11], &want);
    assert_eq!(&q[11..14], &[0x1A, 0x35, 0x00]);
    assert_eq!(marks[11], legaia_engine_expansion::arts::SUPER_MARK);
    let performed = w.expansion.arts.turn_performed.clone();
    assert_eq!(performed.len(), 2);
    let before = w.actors[0].battle.spirit_gauge;
    w.expansion_charge_pack_arts(0, &performed);
    assert_eq!(before - w.actors[0].battle.spirit_gauge, 30 + 20);
}

/// The battle form's art bank gets a borrowed copy of each segment's retail
/// clip at `byte - 0x10`, with the pack's power bytes; the commit latches the
/// borrowed constant and plays the copy; the hit scales by `damage_pct` and
/// lands the rider.
#[test]
fn pack_clips_extend_the_bank_and_latch_their_base() {
    let mut w = with_pack(arts_world());
    let mut bank: Vec<Option<MonsterAnimation>> = vec![None; 0x23];
    bank[0x23 - 0x10] = Some(clip(0x23, [0x16, 0x16, 0, 0], [5, 9, 0, 0]));
    bank[0x27 - 0x10] = Some(clip(0x27, [0x1A, 0, 0, 0], [7, 0, 0, 0]));
    w.set_actor_battle_art_bank(0, Arc::new(bank));
    w.expansion_extend_art_bank(0, legaia_art::Character::Vahn);
    let b = w.actors[0].battle_art_bank.clone().unwrap();
    let c33 = b[0x33 - 0x10].as_ref().expect("segment 0 clip");
    assert_eq!(c33.entry_power_run(), Some([0x19, 0x1E, 0, 0]));
    assert_eq!(c33.entry_event_frames(), Some([5, 9, 0, 0]));
    let c34 = b[0x34 - 0x10].as_ref().expect("segment 1 clip");
    assert_eq!(c34.entry_power_run(), Some([0x1F, 0, 0, 0]));
    assert_eq!(c34.frames, b[0x27 - 0x10].as_ref().unwrap().frames);
    // The retail clips are untouched.
    assert_eq!(
        b[0x23 - 0x10].as_ref().unwrap().entry_power_run(),
        Some([0x16, 0x16, 0, 0])
    );
    // Commit the pack byte: latch the base, play the copy.
    w.actors[0].battle.queued_anim = 0x33;
    w.commit_staged_battle_anim(0);
    assert_eq!(w.actors[0].battle.latched_anim, 0x23);
    assert_eq!(w.expansion.arts.playing.get(&0), Some(&(0, 0)));
    assert_eq!(w.expansion_scale_art_hit(0, 1, 100), 150);
    assert_eq!(
        w.expansion_pack_art_name(0, 0x34).as_deref(),
        Some("Ember Cascade")
    );
    w.expansion_art_hit_rider(0, 1, 50);
    assert!(w.battle.status_effects.has(
        1,
        legaia_engine_battle_vm::status_effects::StatusKind::Venom
    ));
    // A retail commit (once the pack clip has played out) drops the record.
    w.actors[0].battle_staged_anim = None;
    w.actors[0].battle.queued_anim = 0x27;
    w.commit_staged_battle_anim(0);
    assert_eq!(w.actors[0].battle.latched_anim, 0x27);
    assert!(w.expansion.arts.playing.is_empty());
    assert_eq!(w.expansion_scale_art_hit(0, 1, 100), 100);
}

/// Learning: conditions met at activation teach silently; a later level
/// teaches with a toast.
#[test]
fn arts_learn_on_their_conditions() {
    let mut w = with_pack(arts_world());
    assert!(!w.pack_art_known("ember"));
    assert!(!w.pack_art_known("finale"));
    w.party.level_up_tracker.level[0] = 85;
    w.expansion_refresh_learning(true);
    assert!(w.pack_art_known("ember"));
    assert!(w.pack_art_known("finale"));
    assert!(!w.pack_art_known("late"));
    assert!(
        w.expansion
            .toasts
            .current()
            .is_some_and(|t| t == "Vahn learned Ember Cascade!")
    );
}

/// Seru capture of the pack monster teaches the pack spell (and not the
/// base monster's retail Seru); the menu lists it under its menu id; a cast
/// of that id runs as the base with the pack's MP, level and magnitude, and
/// trains the pack spell's own level.
#[test]
fn a_captured_pack_monster_teaches_its_spell_and_the_cast_runs_as_its_base() {
    let mut w = arts_world();
    w.tables.spell_catalog = legaia_engine_battle::spells::SpellCatalog::new();
    w.tables
        .spell_catalog
        .insert(legaia_engine_battle::spells::SpellDef {
            id: 0x81,
            name: "Gimard".into(),
            mp_cost: 10,
            ..Default::default()
        });
    let mut w = with_pack(w);
    // "aegis" has no learn conditions: Vahn knows it from the start.
    assert_eq!(w.expansion_pack_spell_ids(0), [0xC0]);
    let formation = w.expansion.battle.compiled.as_ref().unwrap().formations[0]
        .def
        .clone();
    w.battle.active_formation = Some(formation);
    let left = w.expansion_capture_spells(vec![9, 44]);
    assert_eq!(
        left,
        [44],
        "the pack capture is taken; the retail one is left"
    );
    assert!(w.pack_spell_progress().knows(0, "wyrm"));
    assert_eq!(w.expansion_pack_spell_ids(0), [0xC0, 0xC1]);
    let entry = w.tables.spell_catalog.get(0xC1).expect("menu entry");
    assert_eq!((entry.name.as_str(), entry.mp_cost), ("Cinder Wyrm", 34));
    // Cast it.
    let def = w.expansion_begin_pack_cast(0, 0xC1).expect("a pack cast");
    assert_eq!(def.id, 0x81, "cast as the base");
    assert_eq!(def.mp_cost, 34);
    assert_eq!(w.expansion_cast_mp(0x81), Some(34));
    assert_eq!(w.expansion_cast_level(0, 0x81), Some(1));
    assert_eq!(w.caster_magic_power_byte(0, 0x81), 1);
    assert_eq!(w.expansion_cast_element(0x81), Some(2));
    assert_eq!(w.expansion_scale_cast(0, 0x81, 500), 1000);
    assert_eq!(w.expansion_cast_name(0x81).as_deref(), Some("Cinder Wyrm"));
    assert!(w.expansion_cast_xp(0, 0x81, 40));
    assert_eq!(w.pack_spell_progress().level(0, "wyrm").unwrap().xp, 40);
    w.expansion_cast_rider(0, 0x81, &[1]);
    assert!(w.battle.status_effects.has(
        1,
        legaia_engine_battle_vm::status_effects::StatusKind::Venom
    ));
    w.expansion_end_pack_cast();
    assert_eq!(
        w.expansion_cast_mp(0x81),
        None,
        "no cast in flight: the base's own"
    );
    assert_eq!(w.expansion_scale_cast(0, 0x81, 500), 500);
    // Deactivation takes the menu entries back out.
    w.deactivate_expansion();
    assert!(w.tables.spell_catalog.get(0xC1).is_none());
    assert!(w.tables.spell_catalog.get(0x81).is_some());
}

/// Learned arts and spells ride the save's expansion block (tail record 5),
/// and survive a load; a New Game+ carry merges them back after the reset.
#[test]
fn learned_arts_and_spells_round_trip_and_carry() {
    let mut w = with_pack(arts_world());
    w.party.level_up_tracker.level[0] = 85;
    w.expansion_refresh_learning(false);
    w.expansion_teach_spell(0, "wyrm");
    w.expansion
        .quests
        .progress
        .spells
        .known
        .get_mut(&(0, "wyrm".into()))
        .unwrap()
        .level = 4;
    let block = w.expansion_save_block().expect("a block");
    let mut loaded = with_pack(arts_world());
    loaded.expansion.quests.progress.arts.learned.clear();
    loaded.expansion_load_block(Some(&block));
    assert!(loaded.pack_art_known("finale"));
    assert_eq!(
        loaded.pack_spell_progress().level(0, "wyrm").unwrap().level,
        4
    );
    let (arts, spells) = loaded.expansion_learned();
    loaded.expansion_begin_new_game();
    assert!(!loaded.pack_art_known("finale"), "a New Game starts over");
    loaded.expansion_carry_learned(arts, spells);
    assert!(loaded.pack_art_known("finale"));
    assert_eq!(
        loaded.pack_spell_progress().level(0, "wyrm").unwrap().level,
        4
    );
}

/// No pack: no bank rows, the latch is the byte, no menu ids, no cast
/// seam, and the queue builder's output is the retail one.
#[test]
fn without_a_pack_every_hook_is_the_identity() {
    let mut w = arts_world();
    assert!(
        w.expansion_bank_rows(legaia_art::Character::Vahn)
            .is_empty()
    );
    assert_eq!(w.expansion_art_latch(0, 0x33), 0x33);
    assert!(w.expansion_pack_spell_ids(0).is_empty());
    assert_eq!(w.expansion_cast_mp(0x81), None);
    assert_eq!(w.expansion_scale_art_hit(0, 1, 77), 77);
    assert_eq!(w.expansion_capture_spells(vec![9]), [9]);
    let (q, _, _) = w.build_arts_action_queue(0, &[Right, Up, Right, Down, Up]);
    assert_eq!(&q[..6], &[0x0D, 0x0F, 0x0D, 0x0E, 0x0F, 0x00]);
    assert!(w.expansion_save_block().is_none());
}
