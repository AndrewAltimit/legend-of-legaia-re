//! Disc-free check of the battle **arts-voice** sites of the anim commit
//! (`FUN_8004AD80`): executing Tactical Arts through the live player-driven
//! Arts command input asks the arts-voice selector (`FUN_8004C140`) once per
//! **known** art the turn performs - on the commit of that art's `0x19`
//! starter clip, keyed on the constant behind it - and a newly learned art
//! (staged behind `0x1A`) asks for none, playing the SpecialStarter's
//! generic voice line instead. A synthetic entry with no matched record
//! stages no art and queues nothing. The disc-gated sibling
//! (`engine-session/tests/arts_shout_battle.rs`) carries the cue through the
//! real tables and XA banks into the audio mix.
//!
//! The pad path here is the one a player actually walks under the retail
//! model: command menu → Arts → per-press directional entry → the entry's own
//! end (auto on the exhausting press, or Cross once the sequence is in) →
//! Begin → target. There is no saved-chain row to confirm; a chain is
//! something the player *types* with the d-pad, which is why a driver that
//! only presses Cross reaches no art at all.

use legaia_engine_battle::arts_command_input::ArtsInputScreen;
use legaia_engine_battle::monster_catalog::{vanilla_formation_table, vanilla_monster_catalog};
use legaia_engine_core::world::{Actor, World};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_system::input::{InputState, PadButton};

/// Vahn's Somersault (`0x27`) with its real `↑↓↑` command - the queue-builder
/// is byte-exact, so the entry has to be typed the way retail types it.
fn stage_somersault(w: &mut World) {
    use legaia_art::Command::{Down, Up};
    let action = legaia_art::ActionConstant::from_byte(0x27).unwrap();
    let rec = legaia_art::ArtRecord {
        action,
        commands: vec![Up, Down, Up],
        anim_index: 0,
        anim_extra: vec![],
        name: None,
        power: vec![legaia_art::power::PowerByte::from_byte(0x16); 2],
        dmg_timing: vec![],
        effect_cues: Default::default(),
        hit_cues: vec![],
        identifier: 0,
        anim_speed: 0,
        enemy_effect: legaia_art::EnemyEffect::None,
        repeat_frames: Default::default(),
        background: 0,
        runtime_address: None,
    };
    w.set_art_record(legaia_art::Character::Vahn, action, rec);
}

/// Put Somersault on Vahn's learned list (`+0x185` count, `+0x186..` ids =
/// constant less `0x1B`), so the builder stages it behind `0x19`.
fn learn_somersault(w: &mut World) {
    let rec = &mut w.party.roster.members[0];
    let mut list = rec.displayed_skills();
    list.count = 1;
    list.ids[0] = 0x27 - 0x1B;
    rec.set_displayed_skills(list);
}

fn build_world(with_record: bool) -> World {
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party.party_count = 3;
    // The zeroed records seed HP 0 / no seat, so they go in FIRST: retail's
    // member walk (`FUN_801DB81C`) hands no ring to a member with no HP.
    w.load_party(legaia_save::Party::zeroed(3));
    for i in 0..3 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 100;
        w.actors[i].battle.max_hp = 100;
        w.actors[i].battle.liveness = 1;
        w.set_battle_attack(i as u8, 90);
    }
    w.set_formation_table(vanilla_formation_table(), vanilla_monster_catalog());
    if with_record {
        stage_somersault(&mut w);
        learn_somersault(&mut w);
    }

    w.player_actor_slot = Some(0);
    w.actors[0].move_state.world_x = 300;
    w.actors[0].move_state.world_z = 300;
    w.actors[0].move_state.field_72 = 4096;
    w.locomotion.camera_azimuth = 0;

    use legaia_engine_battle::encounter::{
        EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
    };
    let mut table = EncounterTable::new("shout_cue_test");
    table.set_trigger_rate(0xFF);
    table.push(EncounterEntry::new(1, 1));
    let mut session = EncounterSession::new(EncounterTracker::new(table));
    session.transition_frames = 2;
    session.grace_frames = 2;
    w.set_encounter_session(Some(session));

    w.mode = SceneMode::Field;
    w.toggles.live_gameplay_loop = true;
    w.battle.player_driven = true;
    w
}

/// Walk into the encounter (the driver's precondition), asserting it fires.
fn walk_into_battle(w: &mut World) {
    let up = InputState::mask_of([PadButton::Up]);
    for _ in 0..6000 {
        w.set_pad(up);
        w.tick();
        if w.mode == SceneMode::Battle {
            return;
        }
    }
    panic!("walking should trigger Field -> Battle");
}

/// Drive the battle out from the pad, taking **Arts** on the first party turn
/// and entering `entry` one direction per press, then Attack on any later
/// turn so the collected cues measure exactly one arts entry.
///
/// Shout cues are drained every tick - battle teardown clears the queue, so a
/// drain that only runs at the end sees nothing.
fn drive_arts_entry_and_collect_shouts(
    w: &mut World,
    entry: &[PadButton],
) -> Vec<legaia_engine_battle::battle_events::BattleShoutCue> {
    drive_arts_entry(w, entry).0
}

/// [`drive_arts_entry_and_collect_shouts`], also returning every CD-XA clip
/// request the battle queued.
fn drive_arts_entry(
    w: &mut World,
    entry: &[PadButton],
) -> (
    Vec<legaia_engine_battle::battle_events::BattleShoutCue>,
    Vec<legaia_engine_system::sound_state::XaVoiceClip>,
) {
    use legaia_engine_battle_menus::battle_input::BattleCommand;

    walk_into_battle(w);

    let mut shouts = Vec::new();
    let mut xa = Vec::new();
    let mut press = true;
    let mut next_dir = 0usize;
    let mut arts_turns = 0usize;
    let mut opened_input = false;
    for _ in 0..4000 {
        let pad = if !press {
            // Edge-triggered: every session reads `just_pressed`, so a press
            // frame has to be followed by a release frame.
            0
        } else if let Some(view) = w.arts_input_view() {
            opened_input = true;
            match view.phase {
                ArtsInputScreen::Entering => {
                    if next_dir < entry.len() {
                        let dir = entry[next_dir];
                        next_dir += 1;
                        InputState::mask_of([dir])
                    } else {
                        // Sequence is in and the pool hasn't exhausted itself:
                        // Cross ends the entry early (the port's disclosed
                        // convenience over retail's auto-end-only rule).
                        InputState::mask_of([PadButton::Cross])
                    }
                }
                // Review → Begin|Reselect (cursor 0 = Begin) → target picker:
                // Cross walks all three.
                _ => InputState::mask_of([PadButton::Cross]),
            }
        } else if let Some(cmd) = w.battle.command.as_ref() {
            // Retail's open flow: `Begin` on the round prompt, the ring's
            // `Attack` arm, then the `Auto | Command` prompt - `Command` is
            // the directional arts entry, `Auto` the plain swing. Cross takes
            // whatever the cursor is on; the other presses are the spatial
            // seatings - Left onto the `Attack` arm, Left/Right onto the
            // `Auto`/`Command` chip.
            use legaia_engine_battle_menus::battle_input::{AttackMode, CommandPhase};
            match cmd.phase {
                CommandPhase::Menu { .. } if cmd.menu_command() != Some(BattleCommand::Attack) => {
                    InputState::mask_of([PadButton::Left])
                }
                CommandPhase::AttackMode { .. } => {
                    // First party turn drives the arts entry; later turns take
                    // the plain swing so the fight actually ends.
                    let want = if arts_turns == 0 {
                        AttackMode::Command
                    } else {
                        AttackMode::Auto
                    };
                    if cmd.attack_mode() == Some(want) {
                        InputState::mask_of([PadButton::Cross])
                    } else if want == AttackMode::Auto {
                        InputState::mask_of([PadButton::Left])
                    } else {
                        InputState::mask_of([PadButton::Right])
                    }
                }
                _ => InputState::mask_of([PadButton::Cross]),
            }
        } else {
            0
        };
        let input_was_open = w.arts_input_active();
        w.set_pad(pad);
        press = !press;
        w.tick();
        if input_was_open && !w.arts_input_active() {
            arts_turns += 1;
        }
        shouts.extend(w.drain_battle_shout_cues());
        xa.extend(w.drain_battle_xa_cues());
        if w.mode == SceneMode::Field && w.battle.last_rewards.is_some() {
            break;
        }
    }
    assert!(
        opened_input,
        "the Arts command must open the per-press input session"
    );
    assert_eq!(arts_turns, 1, "exactly one arts entry was driven");
    // Non-vacuity: an entry that aborted (or a loop that spun out) would leave
    // the world in Battle, and a shout-free expectation would pass for the
    // wrong reason.
    assert_eq!(w.mode, SceneMode::Field, "the battle resolved");
    assert!(
        w.battle.last_rewards.is_some(),
        "the party won, so an art actually landed"
    );
    (shouts, xa)
}

#[test]
fn matched_art_emits_one_shout_cue_with_its_action_constant() {
    let mut w = build_world(true);
    // Up Down Up = Somersault's whole command string (three presses is also
    // the disc-free pool's own end: 100 AP at the favored-class cost 0x1E).
    let shouts = drive_arts_entry_and_collect_shouts(
        &mut w,
        &[PadButton::Up, PadButton::Down, PadButton::Up],
    );
    assert_eq!(shouts.len(), 1, "one cue per executed art: {shouts:?}");
    assert_eq!(shouts[0].cslot, 0, "Vahn = character slot 0 (XA2 bank)");
    assert_eq!(shouts[0].action, 0x27, "the matched record's constant");
}

#[test]
fn synthetic_art_without_record_emits_no_shout_cue() {
    let mut w = build_world(false);
    let shouts = drive_arts_entry_and_collect_shouts(&mut w, &[PadButton::Up]);
    assert!(shouts.is_empty(), "synthetic art stays silent: {shouts:?}");
}

/// Retail's entry runs until the AP pool is spent, so performing **several**
/// arts in one turn is the ordinary case, not an edge one - and each known
/// art is a separately staged `0x19` starter whose commit calls the cue
/// selector. A turn that performs three Somersaults must therefore request
/// three shouts.
///
/// Nine presses (three `↑↓↑`) is also the pool's own end: at the disc-free
/// fallback pool (100 AP) with the swing costs seeded at 11, the ninth press
/// leaves nothing affordable and the entry auto-ends, so this walks retail's
/// no-confirm path end to end.
#[test]
fn every_art_in_a_multi_art_entry_gets_its_own_shout_cue() {
    use legaia_engine_system::input::PadButton::{Down, Up};
    let mut w = build_world(true);
    w.battle.swing_costs[0] = [11; 4];
    let shouts =
        drive_arts_entry_and_collect_shouts(&mut w, &[Up, Down, Up, Up, Down, Up, Up, Down, Up]);
    assert_eq!(
        shouts.len(),
        3,
        "one cue per performed art, not one per turn: {shouts:?}"
    );
    for cue in &shouts {
        assert_eq!(cue.cslot, 0, "Vahn = character slot 0 (XA2 bank)");
        assert_eq!(cue.action, 0x27, "each performed art keys its own cue");
    }
}

/// A first use learns the art: the builder writes the `0x1A` starter in
/// place of `0x19`, so the commit asks the selector for nothing. The
/// SpecialStarter's own block plays instead - the build loop's starter mark
/// (`1`, the `NEW ARTS!!` banner) selects the generic voice line, id `0x101`
/// = `XA1.XA` channel 1 through the dispatcher.
#[test]
fn a_newly_learned_art_plays_the_starter_line_not_a_shout() {
    let mut w = build_world(true);
    {
        let rec = &mut w.party.roster.members[0];
        let mut list = rec.displayed_skills();
        list.count = 0;
        rec.set_displayed_skills(list);
    }
    w.audio.xa_cue_durations = Some(vec![10; 0x200]);
    let (shouts, xa) = drive_arts_entry(&mut w, &[PadButton::Up, PadButton::Down, PadButton::Up]);
    assert!(
        shouts.is_empty(),
        "a learned-on-use art shouts nothing: {shouts:?}"
    );
    assert!(
        xa.iter().any(|c| (c.clip, c.channel) == (0, 1)),
        "the NEW ARTS line, XA1 channel 1: {xa:?}"
    );
}

/// With the selector's SCUS bytes staged the world resolves the call itself:
/// the cue is marked resolved and the clip rides the CD-XA queue as the
/// starter's raw `(slot, channel, dur)` - `XA2` (slot 1) for Vahn, a member of
/// the art's pool, the duration from the per-channel table.
#[test]
fn a_staged_selector_resolves_the_shout_onto_the_xa_queue() {
    use legaia_art::arts_voice::{
        ArtsVoiceSelectorTables, DUR_TABLE_VA, FIRST_HALF_VARIANT_BASES, RANGE_TABLE_VA,
        SELECTOR_SPAN_END, SELECTOR_SPAN_START,
    };
    let mut b = vec![0u8; (SELECTOR_SPAN_END - SELECTOR_SPAN_START) as usize];
    let mut put = |va: u32, bytes: &[u8]| {
        let o = (va - SELECTOR_SPAN_START) as usize;
        b[o..o + bytes.len()].copy_from_slice(bytes);
    };
    put(RANGE_TABLE_VA, &[0x1F, 0x29, 0x2B]);
    // Somersault ends the string, so the flag is `0`: the plain variant.
    put(
        FIRST_HALF_VARIANT_BASES[0][0] + (0x29 - 0x27) * 0x0F,
        &[0, 6],
    );
    put(DUR_TABLE_VA, &[0x4B, 0, 0, 0, 0, 0, 0x65]);
    let mut w = build_world(true);
    w.audio.arts_voice_tables = Some(std::sync::Arc::new(
        ArtsVoiceSelectorTables::from_span_bytes(b),
    ));
    let (shouts, xa) = drive_arts_entry(&mut w, &[PadButton::Up, PadButton::Down, PadButton::Up]);
    assert_eq!(shouts.len(), 1, "{shouts:?}");
    assert!(shouts[0].resolved);
    let shout: Vec<_> = xa.iter().filter(|c| c.clip == 1).collect();
    assert_eq!(shout.len(), 1, "one XA2 clip: {xa:?}");
    let (ch, dur) = (shout[0].channel, shout[0].duration_sectors);
    assert!(
        (ch, dur) == (0, 0x2D) || (ch, dur) == (6, 0x3D),
        "a pool member with its table duration: {:?}",
        shout[0]
    );
    assert_eq!(u32::from(w.audio.arts_voice_last_channel), ch);
}
