use super::*;

/// Field dialogue opens from the **field-interact op** (`0x3E` with
/// `op0 < 100`) reading the interacted actor's inline interaction-script
/// text (keyed by the op's `slot` = the actor's MAN record index) - the real
/// field-dialogue mechanism that replaces the `0x3F`-as-dialog stand-in.
#[test]
fn field_interact_opens_actor_inline_dialogue() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    // Seed actor slot 3's inline interaction-script dialogue.
    world
        .field_npc_dialog
        .insert(3, vec![0x1F, b'h', b'i', 0x00]);
    // 0x3E with op0 = 5 (< 100 -> field interact), op1 = slot 3.
    world.load_field_script(vec![0x3E, 0x05, 0x03]);
    let _ = world.tick();
    let req = world
        .current_dialog
        .as_ref()
        .expect("field_interact on an actor with inline text must open dialogue");
    assert_eq!(req.inline, vec![0x1F, b'h', b'i', 0x00]);
    let evs = world.drain_field_events();
    assert!(
        evs.iter()
            .any(|e| matches!(e, FieldEvent::OpenDialog { inline, .. } if !inline.is_empty())),
        "expected OpenDialog from the field-interact path, got {evs:?}"
    );
    assert!(
        evs.iter().any(|e| matches!(
            e,
            FieldEvent::FieldInteract {
                interact_id: 5,
                slot: 3
            }
        )),
        "field_interact must still surface the FieldInteract event"
    );
}

/// A field-interact on an actor with **no** inline text just surfaces the
/// interaction (a sign / flag-only NPC) - no dialogue box.
#[test]
fn field_interact_without_inline_text_opens_no_dialogue() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.load_field_script(vec![0x3E, 0x05, 0x07]);
    let _ = world.tick();
    assert!(
        world.current_dialog.is_none(),
        "no inline text for slot 7 -> no dialogue"
    );
}

/// The field-VM dialogue-accept auto-arms a scripted-encounter carrier.
///
/// Interacting with the carrier's placement (field-interact op `0x3E`,
/// `op0 < 100`) opens its dialogue and arms the engage; accepting the prompt
/// (the dialog-advance dismiss, op `0x4C` n5 sub-4) engages the carrier, so the
/// SM (`FUN_801DA51C`) runs its scene-transition and flips Field -> Battle -
/// with no manual `engage_field_carrier` call. This is the field-VM-driven
/// counterpart to the carrier-engage API.
#[test]
fn field_dialogue_accept_auto_arms_scripted_carrier() {
    use crate::input::PadButton;

    let mut world = World::new();
    world.set_formation_table(
        crate::monster_catalog::vanilla_formation_table(),
        crate::monster_catalog::vanilla_monster_catalog(),
    );
    world.set_active_scene_label("town01");
    world.mode = SceneMode::Field;

    // Carrier 0 = scripted encounter (vanilla formation 1); carrier 1 = plain
    // NPC. Wire the slot map the way install_field_carriers_from_man would:
    // only the scripted carrier gets a slot entry (slot 3 -> carrier 0). The
    // plain NPC's slot 7 has dialogue but no carrier-slot entry.
    world.install_field_carriers(vec![
        FieldCarrierConfig::ScriptedEncounter { formation_id: 1 },
        FieldCarrierConfig::Npc { interact_id: 7 },
    ]);
    world.field_carrier_slots.insert(3, 0);
    world
        .field_npc_dialog
        .insert(3, vec![0x1F, b'h', b'i', 0x00]);
    world
        .field_npc_dialog
        .insert(7, vec![0x1F, b'y', b'o', 0x00]);

    // Interact with the scripted carrier's slot, then poll the dialog.
    world.load_field_script(vec![0x3E, 0x05, 0x03, 0x4C, 0x54]);
    world.input.set_pad(0);
    let _ = world.tick();
    assert!(
        world.current_dialog.is_some(),
        "interacting with the carrier opens its dialogue"
    );
    assert_eq!(
        world.pending_carrier_engage,
        Some(0),
        "the scripted carrier's engage is armed, waiting for the accept"
    );
    assert_eq!(
        world.mode,
        SceneMode::Field,
        "no battle while the prompt is still up"
    );

    // Accept (just-pressed Cross): dismiss -> engage -> SM -> the intro
    // transition -> Battle once its 132 display frames elapse.
    world.input.set_pad(PadButton::Cross.mask());
    let _ = world.tick();
    assert!(
        world.pending_carrier_engage.is_none(),
        "the armed engage is consumed on the accept"
    );
    world.input.set_pad(0);
    for _ in 0..200 {
        if world.mode == SceneMode::Battle {
            break;
        }
        let _ = world.tick();
    }
    assert_eq!(
        world.mode,
        SceneMode::Battle,
        "accepting the scripted carrier's prompt launches the fight via the SM"
    );
}

/// The interaction probe (retail `FUN_801cf9f4` via the `DAT_801f2254`
/// facing compass): a just-pressed action button talks to the NPC the player
/// is *facing* (probe point 64 ahead, ±72 box), and only that one - a
/// distant NPC is not triggered, and after the talk the player has been
/// turned toward the matched NPC (the face-the-NPC step).
#[test]
fn interaction_probe_talks_to_adjacent_npc_only() {
    use crate::input::PadButton;

    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.player_actor_slot = Some(0);
    world.actors[0].active = true;
    // Player at tile 20 (world 20*128 + 0x40 = 2624), facing X+ (engine
    // heading 0x400) toward the adjacent NPC one tile ahead.
    world.actors[0].move_state.world_x = 2624;
    world.actors[0].move_state.world_z = 2624;
    world.actors[0].move_state.render_26 = 0x400;
    // Adjacent NPC at tile (21, 20); a far NPC at tile 40 that must not trigger.
    world
        .field_npc_dialog
        .insert(5, vec![0x1F, b'h', b'i', 0x00]);
    world.field_npc_positions.insert(5, (2752, 2624)); // tile (21, 20)
    world.field_npc_dialog.insert(6, vec![0x1F, b'x', 0x00]);
    world.field_npc_positions.insert(6, (5120, 5120)); // tile (40, 40)

    world.input.set_pad(PadButton::Cross.mask());
    let _ = world.tick();
    let req = world
        .current_dialog
        .as_ref()
        .expect("action button near an NPC opens its dialogue");
    assert_eq!(
        req.inline,
        vec![0x1F, b'h', b'i', 0x00],
        "the probe opened the faced NPC (slot 5), not the far one"
    );
    assert_eq!(
        world.actors[0].move_state.render_26, 0x400,
        "face-the-NPC: the player heading points at the matched NPC (X+)"
    );
}

/// The probe is facing-indexed: the same adjacent NPC does NOT answer when
/// the player looks away from it (retail probes a single compass point 64
/// units ahead of the facing, not a radius around the player).
#[test]
fn interaction_probe_requires_facing_the_npc() {
    use crate::input::PadButton;

    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.player_actor_slot = Some(0);
    world.actors[0].active = true;
    world.actors[0].move_state.world_x = 2624;
    world.actors[0].move_state.world_z = 2624;
    // NPC one tile X+ ahead, but the player faces Z+ (engine heading 0).
    world.actors[0].move_state.render_26 = 0;
    world
        .field_npc_dialog
        .insert(5, vec![0x1F, b'h', b'i', 0x00]);
    world.field_npc_positions.insert(5, (2752, 2624));

    world.input.set_pad(PadButton::Cross.mask());
    let _ = world.tick();
    assert!(
        world.current_dialog.is_none(),
        "an NPC beside the player is not talked to while facing away"
    );
}

/// The probe is inert when no NPC is within range: pressing the action button in
/// open field opens nothing.
#[test]
fn interaction_probe_no_npc_in_range_opens_nothing() {
    use crate::input::PadButton;

    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.player_actor_slot = Some(0);
    world.actors[0].active = true;
    world.actors[0].move_state.world_x = 2624;
    world.actors[0].move_state.world_z = 2624;
    world.field_npc_dialog.insert(6, vec![0x1F, b'x', 0x00]);
    world.field_npc_positions.insert(6, (5120, 5120)); // tile (40, 40), far

    world.input.set_pad(PadButton::Cross.mask());
    let _ = world.tick();
    assert!(
        world.current_dialog.is_none(),
        "no NPC near the facing probe point -> the action button opens no dialogue"
    );
}

/// Capture-grounded probe geometry: the `rimelm_npc_press_tetsu` frame has
/// the player at (2762, 1782) pressed Z+ into Tetsu at (2752, 1856). With
/// the player facing Z+, the `DAT_801f2254` sector-4 probe point lands at
/// (2762, 1846) - deltas (10, 10) from Tetsu, well inside the ±72 interact
/// box - so the action button talks to him from the captured rest position.
#[test]
fn interaction_probe_matches_tetsu_capture_geometry() {
    use crate::input::PadButton;

    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.player_actor_slot = Some(0);
    world.actors[0].active = true;
    world.actors[0].move_state.world_x = 2762;
    world.actors[0].move_state.world_z = 1782;
    world.actors[0].move_state.render_26 = 0; // engine heading 0 = facing Z+
    world
        .field_npc_dialog
        .insert(4, vec![0x1F, b'y', b'o', 0x00]);
    world.field_npc_positions.insert(4, (2752, 1856));

    world.input.set_pad(PadButton::Cross.mask());
    let _ = world.tick();
    assert!(
        world.current_dialog.is_some(),
        "the captured press-rest position talks to Tetsu through the facing probe"
    );
}

/// Walking up to the scripted-encounter carrier and pressing the action button
/// twice (talk, then accept) starts the fight through the probe - the fully
/// input-driven counterpart to the field-VM dialogue-accept.
#[test]
fn interaction_probe_walk_up_to_scripted_carrier_starts_fight() {
    use crate::input::PadButton;

    let mut world = World::new();
    world.set_formation_table(
        crate::monster_catalog::vanilla_formation_table(),
        crate::monster_catalog::vanilla_monster_catalog(),
    );
    world.set_active_scene_label("town01");
    world.mode = SceneMode::Field;
    world.player_actor_slot = Some(0);
    world.actors[0].active = true;
    world.actors[0].move_state.world_x = 2624; // tile 20
    world.actors[0].move_state.world_z = 2624;
    world.actors[0].move_state.render_26 = 0x400; // facing X+, toward the NPC

    // Carrier 0 = scripted encounter; its NPC (slot 5) stands at the adjacent
    // tile (21, 20) with the sparring dialogue.
    world.install_field_carriers(vec![FieldCarrierConfig::ScriptedEncounter {
        formation_id: 1,
    }]);
    world.field_carrier_slots.insert(5, 0);
    world
        .field_npc_dialog
        .insert(5, vec![0x1F, b'h', b'i', 0x00]);
    world.field_npc_positions.insert(5, (2752, 2624));

    // Talk: the probe opens the carrier's dialogue and arms the engage.
    world.input.set_pad(PadButton::Cross.mask());
    let _ = world.tick();
    assert!(
        world.current_dialog.is_some(),
        "walking up + action button opens the carrier's dialogue"
    );
    assert_eq!(world.pending_carrier_engage, Some(0), "engage armed");
    assert_eq!(
        world.mode,
        SceneMode::Field,
        "no battle while the prompt is up"
    );

    // Release, then accept: the probe dismisses the box and engages, and
    // the intro transition clocks through to the Battle flip.
    world.input.set_pad(0);
    let _ = world.tick();
    world.input.set_pad(PadButton::Cross.mask());
    let _ = world.tick();
    world.input.set_pad(0);
    for _ in 0..200 {
        if world.mode == SceneMode::Battle {
            break;
        }
        let _ = world.tick();
    }
    assert_eq!(
        world.mode,
        SceneMode::Battle,
        "accepting the probe-opened prompt starts the fight (no script, no manual engage)"
    );
}

/// A synthetic sparring dialogue carrying the immediate-labels 4-option picker
/// (option 2 = the "practice" / fight choice), mirroring the real Rim Elm spar.
fn spar_dialogue() -> Vec<u8> {
    let mut b = vec![0x1F, b'S', b'p', b'a', b'r', 0x00]; // prompt, 0x00-terminated
    b.push(0x29); // open, N=4
    for j in [0x10i16, 0x20, 0x30, 0x40] {
        b.extend_from_slice(&j.to_le_bytes()); // 4 jump entries
    }
    // labels immediately (no continuation byte) - index 2 is the fight option
    for lbl in [&b"go"[..], &b"no"[..], &b"practice"[..], &b"bye"[..]] {
        b.push(0x1F);
        b.extend_from_slice(lbl);
        b.push(0x00);
    }
    // Branch bodies the four jumps land in. `legaia_mes::scan_pickers` only
    // accepts a picker whose every option target is a byte of this script -
    // a real record has its branches after the labels - so the fixture needs
    // them too. Filler is field-VM `0x21` NOPs (no second picker).
    b.resize(80, 0x21);
    b
}

/// `spar_menu_of` derives the fight option from the disc op, not the label:
/// a 4-option picker whose **labels are all non-English** but whose option-2
/// branch installs a scripted battle (`3E FF 04`) must still resolve to
/// `fight_option == 2`. This fails against a `"practice"`-label match (the pre-
/// change behaviour returns `None` here) and passes once the branch scan is in.
#[test]
fn spar_menu_of_derives_fight_option_from_the_scripted_battle_install() {
    // Layout: [1F 'p' 'q' 00] prompt, then the `0x29` open, 4 jump entries, four
    // immediate-labels segments (non-English "aa"/"bb"/"cc"/"dd"), then four
    // 8-byte branch regions - only region 2 carries `3E FF 04`.
    let mut b = vec![0x1F, b'p', b'q', 0x00]; // prompt segment (0x00-terminated)
    let open = b.len(); // == 4
    b.push(0x29); // open byte, N=4
    // Placeholder jump entries (patched below once branch offsets are known).
    let jt = b.len();
    b.extend_from_slice(&[0u8; 8]);
    // Immediate labels - deliberately NOT the English "practice".
    for lbl in [&b"aa"[..], &b"bb"[..], &b"cc"[..], &b"dd"[..]] {
        b.push(0x1F);
        b.extend_from_slice(lbl);
        b.push(0x00);
    }
    // Four branch regions; region 2 is the fight branch.
    let regions: [usize; 4] = std::array::from_fn(|i| b.len() + i * 8);
    for i in 0..4 {
        if i == 2 {
            b.extend_from_slice(&[0x3E, 0xFF, 0x04, 0, 0, 0, 0, 0]);
        } else {
            b.extend_from_slice(&[0u8; 8]);
        }
    }
    // Patch each jump entry so jump_target(i) lands on region i:
    //   jump_target(i) = (open + 1 + i*2) + rel_jump(i)  =>  rel = region_i - base.
    for i in 0..4 {
        let base = (open + 1 + i * 2) as i64;
        let rel = (regions[i] as i64 - base) as i16;
        b[jt + i * 2..jt + i * 2 + 2].copy_from_slice(&rel.to_le_bytes());
    }

    // Sanity: the picker decodes with non-English labels and the right jumps.
    let p = legaia_mes::scan_pickers(&b)
        .into_iter()
        .find(|p| p.n == 4)
        .expect("4-option picker decodes");
    assert_eq!(p.options[2].label, b"cc", "option 2 label is non-English");
    assert_eq!(p.jump_target(2), Some(regions[2]));
    assert_eq!(&b[regions[2]..regions[2] + 3], &[0x3E, 0xFF, 0x04]);

    let (n, fight_option) =
        spar_menu_of(&b).expect("a 4-option picker with a scripted-battle branch is a spar menu");
    assert_eq!(n, 4);
    assert_eq!(
        fight_option, 2,
        "the fight option is derived from the `3E FF 04` install in option 2's branch, \
         not from any English label"
    );
}

/// The faithful inline runner resumes across an op-0x4A `WaitFrames`: an
/// effect scripted *behind* the wait (a `0x50` SET, standing in for the Tetsu
/// `3E FF 04` install) must still run once the frames elapse, not be dropped
/// when the wait first halts. Before the resume fix the WaitFrames halt ended
/// the conversation and the SET never ran.
#[test]
fn inline_runner_resumes_across_wait_frames_to_run_the_post_wait_effect() {
    // First box "hi", then WaitFrames 16, then SET system flag 7 (the effect),
    // then reply "ok", then end.
    let mut buf = vec![0x1F, b'h', b'i', 0x00];
    buf.extend_from_slice(&[0x4A, 0x10, 0x00]); // WaitFrames 16 (u16 LE target)
    buf.extend_from_slice(&[0x50, 0x07]); // SET system flag 7 - the gated effect
    buf.extend_from_slice(&[0x1F, b'o', b'k', 0x00]); // reply box
    buf.push(0x00); // conversation end

    let mut world = World::new();
    world.start_inline_dialogue(buf);

    // Tick until the first box is fully revealed, then confirm to dismiss it.
    let mut guard = 0;
    while world.inline_dialogue.as_ref().unwrap().page_bytes() != b"hi" {
        world.step_inline_dialogue(false, false, false);
        guard += 1;
        assert!(guard < 50, "first box never typed");
    }
    world.step_inline_dialogue(true, false, false); // dismiss "hi"

    // The very next VM step hits WaitFrames: it must NOT end the conversation,
    // and the effect behind it must not have run yet.
    world.step_inline_dialogue(false, false, false);
    assert!(
        !world.inline_dialogue.as_ref().unwrap().is_done(),
        "WaitFrames must suspend, not end, the conversation"
    );
    assert!(
        !world.system_flag_test(7),
        "the post-wait effect has not run while the wait is still counting"
    );

    // Keep ticking: within the 16-frame window the wait elapses, the SET runs,
    // and the reply box opens - the conversation never ends early.
    let mut ran = false;
    for _ in 0..40 {
        world.step_inline_dialogue(false, false, false);
        assert!(
            !world.inline_dialogue.as_ref().unwrap().is_done(),
            "conversation ended before the post-wait effect ran"
        );
        if world.system_flag_test(7) {
            ran = true;
            break;
        }
    }
    assert!(
        ran,
        "the effect scripted behind WaitFrames ran once the wait elapsed"
    );
}

/// Set up a world with a scripted-encounter carrier whose dialogue is the spar
/// menu, the player adjacent and facing it (`(slot 5)` at tile (21, 20)).
fn world_with_spar_carrier() -> World {
    let mut world = World::new();
    world.set_formation_table(
        crate::monster_catalog::vanilla_formation_table(),
        crate::monster_catalog::vanilla_monster_catalog(),
    );
    world.set_active_scene_label("town01");
    world.mode = SceneMode::Field;
    world.player_actor_slot = Some(0);
    world.actors[0].active = true;
    world.actors[0].move_state.world_x = 2624;
    world.actors[0].move_state.world_z = 2624;
    world.actors[0].move_state.render_26 = 0x400; // facing X+, toward the NPC
    world.install_field_carriers(vec![FieldCarrierConfig::ScriptedEncounter {
        formation_id: 1,
    }]);
    world.field_carrier_slots.insert(5, 0);
    world.field_npc_dialog.insert(5, spar_dialogue());
    world.field_npc_positions.insert(5, (2752, 2624));
    world
}

/// Talking to the sparring carrier raises its 4-option spar menu (NOT the
/// any-accept arm), and **confirming a non-fight option does not start a fight** -
/// the box just closes. The fight is gated on the index-2 ("practice") option.
#[test]
fn carrier_spar_menu_gates_engage_on_the_fight_option() {
    use crate::input::PadButton;

    let mut world = world_with_spar_carrier();

    // Talk: opens the menu (not the any-accept engage).
    world.input.set_pad(PadButton::Cross.mask());
    let _ = world.tick();
    assert!(world.current_dialog.is_some(), "carrier dialogue opens");
    assert!(
        world.pending_carrier_engage.is_none(),
        "the menu path is used, not the any-accept arm"
    );
    let menu = world.carrier_menu.expect("the spar's 4-option menu is up");
    assert_eq!(menu.n, 4, "4-option picker");
    assert_eq!(
        menu.fight_option, 2,
        "the fight option is index 2 (\"practice\")"
    );
    assert_eq!(menu.cursor, 0, "cursor starts on option 0");
    assert_eq!(world.mode, SceneMode::Field);

    // Confirm at cursor 0 (a talk option): the box closes, no fight.
    world.input.set_pad(0);
    let _ = world.tick();
    world.input.set_pad(PadButton::Cross.mask());
    let _ = world.tick();
    assert_eq!(
        world.mode,
        SceneMode::Field,
        "confirming a non-fight option does not start the fight"
    );
    assert!(world.carrier_menu.is_none(), "the menu closed");
    assert!(world.current_dialog.is_none(), "the box closed");
}

/// Navigating the spar menu down to the index-2 fight option and confirming
/// flips Field -> Battle (the faithful 4-option path).
#[test]
fn carrier_spar_menu_fight_option_starts_battle() {
    use crate::input::PadButton;

    let mut world = world_with_spar_carrier();
    world.input.set_pad(PadButton::Cross.mask());
    let _ = world.tick();
    let fight = world.carrier_menu.expect("menu up").fight_option;

    // Move the cursor down to the fight option (one fresh Down edge per step).
    for _ in 0..fight {
        world.input.set_pad(0);
        let _ = world.tick();
        world.input.set_pad(PadButton::Down.mask());
        let _ = world.tick();
    }
    assert_eq!(
        world.carrier_menu.expect("menu still up").cursor,
        fight,
        "cursor on the fight option"
    );
    assert_eq!(world.mode, SceneMode::Field, "still field while navigating");

    // Confirm: the engage arms the intro transition, and the mode flips
    // once its 132 display frames elapse.
    world.input.set_pad(0);
    let _ = world.tick();
    world.input.set_pad(PadButton::Cross.mask());
    let mut reached = false;
    for _ in 0..200 {
        let _ = world.tick();
        if world.mode == SceneMode::Battle {
            reached = true;
            break;
        }
        world.input.set_pad(0);
    }
    assert!(reached, "confirming the fight option starts the spar");
}

/// `nav_step_toward` walks the player to a target across open field (no walls).
#[test]
fn nav_step_toward_walks_player_to_target() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.player_actor_slot = Some(0);
    world.actors[0].active = true;
    world.actors[0].move_state.world_x = 2624;
    world.actors[0].move_state.world_z = 2624;
    // Open field (no collision grid installed -> nothing is a wall). Target ~6
    // tiles away; the player should reach it within a generous frame budget.
    let (tx, tz) = (2752i16, 1856i16);
    let mut arrived = false;
    for _ in 0..4000 {
        if world.nav_step_toward(tx, tz, 32) {
            arrived = true;
            break;
        }
    }
    assert!(arrived, "nav walks the player to the target in open field");
    let ms = &world.actors[0].move_state;
    assert!(
        (ms.world_x - tx).abs() <= 32 && (ms.world_z - tz).abs() <= 32,
        "player ends within tolerance of the target ({}, {})",
        ms.world_x,
        ms.world_z
    );
}

/// A plain talk NPC never auto-arms a battle: interacting opens its dialogue and
/// dismissing it returns to free roam (no carrier-slot entry -> nothing armed).
#[test]
fn field_dialogue_accept_on_plain_npc_does_not_arm_battle() {
    use crate::input::PadButton;

    let mut world = World::new();
    world.set_formation_table(
        crate::monster_catalog::vanilla_formation_table(),
        crate::monster_catalog::vanilla_monster_catalog(),
    );
    world.mode = SceneMode::Field;
    world.install_field_carriers(vec![FieldCarrierConfig::Npc { interact_id: 7 }]);
    // No scripted carrier -> field_carrier_slots stays empty.
    world
        .field_npc_dialog
        .insert(7, vec![0x1F, b'y', b'o', 0x00]);

    world.load_field_script(vec![0x3E, 0x05, 0x07, 0x4C, 0x54]);
    world.input.set_pad(0);
    let _ = world.tick();
    assert!(
        world.current_dialog.is_some(),
        "plain NPC opens its dialogue"
    );
    assert_eq!(
        world.pending_carrier_engage, None,
        "a plain NPC arms no engage"
    );

    world.input.set_pad(PadButton::Cross.mask());
    let _ = world.tick();
    assert_eq!(
        world.mode,
        SceneMode::Field,
        "dismissing a plain NPC's dialogue stays in the field"
    );
}

// --- Op-0x43 sub-2 three-actor talk (FUN_801D2D38) --------------------------

/// Build the 8-byte `[43, 2, a1, a2, a3, lo, hi, b6]` instruction.
fn talk_op(ids: [u8; 3], word: u16, byte: u8) -> Vec<u8> {
    let mut op = vec![0x43, 0x02, ids[0], ids[1], ids[2]];
    op.extend_from_slice(&word.to_le_bytes());
    op.push(byte);
    op
}

#[test]
fn three_actor_talk_first_arm_collapses_party_and_sets_flags() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.party_actor_slots = vec![Some(1), Some(0), Some(2)];
    world.party_leader_slot = Some(1);
    world.field_npc_positions.insert(5, (100, 200));
    world.field_npc_headings.insert(5, 0x400);

    let op = talk_op([5, 6, 7], 0x3412, 0xAB);
    let mut ctx = FieldCtx::default();
    let mut host = FieldHostImpl { world: &mut world };
    match vm::field::step(&mut host, &mut ctx, &op, 0) {
        FieldStepResult::Advance { next_pc } => assert_eq!(next_pc, 8),
        other => panic!("sub-2 should advance 8 bytes, got {other:?}"),
    }

    // Party collapsed to the leader (retail count=1, ids=[leader,0,0,0]).
    assert_eq!(world.party_actor_slots, vec![Some(1)]);
    // Talk lock + per-character flag choreography.
    assert!(world.system_flag_test(0xD), "talk-active lock set");
    assert!(!world.system_flag_test(0x10));
    assert!(world.system_flag_test(0x11), "flag 0x10 + leader(1) set");
    assert!(!world.system_flag_test(0x12));
    // Session record captured, including actor 5's live position.
    let talk = world.three_actor_talk.expect("session installed");
    assert_eq!(talk.actor_ids, [5, 6, 7]);
    assert_eq!(talk.script_id, 0x3412);
    assert_eq!(talk.duration, 0xAB);
    assert_eq!(talk.saved[0], Some(((100, 200), 0x400)));
    assert_eq!(talk.saved[1], None, "unseeded participant has no capture");
}

#[test]
fn three_actor_talk_rearm_restores_saved_positions() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.party_leader_slot = Some(0);
    world.field_npc_positions.insert(5, (100, 200));
    world.field_npc_headings.insert(5, 0x400);

    // First arm captures actor 5's position.
    let op = talk_op([5, 6, 7], 1, 10);
    let mut ctx = FieldCtx::default();
    {
        let mut host = FieldHostImpl { world: &mut world };
        let _ = vm::field::step(&mut host, &mut ctx, &op, 0);
    }
    // The talk moves the actor.
    world.field_npc_positions.insert(5, (900, 900));
    world.field_npc_headings.insert(5, 0);

    // Re-arm while flag 0xD is up: retail's else-branch restores the saved
    // table onto the new instruction's participants.
    {
        let mut host = FieldHostImpl { world: &mut world };
        let _ = vm::field::step(&mut host, &mut ctx, &op, 0);
    }
    assert_eq!(world.field_npc_positions.get(&5), Some(&(100, 200)));
    assert_eq!(world.field_npc_headings.get(&5), Some(&0x400));
    assert!(world.system_flag_test(0xD), "lock stays up");
}

/// The talk end: retail's controller (`FUN_801D27E0`) polls the talk lock
/// every frame and despawns when the script clears it; the engine's poll
/// ([`World::tick_three_actor_talk`]) additionally restores the party
/// count + leader from the arm-time snapshot, so the collapse is not a
/// one-way door.
#[test]
fn three_actor_talk_end_restores_party_and_drops_lock() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.party_actor_slots = vec![Some(1), Some(0), Some(2)];
    world.party_leader_slot = Some(1);

    let op = talk_op([5, 6, 7], 1, 10);
    let mut ctx = FieldCtx::default();
    {
        let mut host = FieldHostImpl { world: &mut world };
        let _ = vm::field::step(&mut host, &mut ctx, &op, 0);
    }
    assert_eq!(
        world.party_actor_slots,
        vec![Some(1)],
        "the arm collapsed the party"
    );
    assert!(world.system_flag_test(0xD));

    // A frame with the lock still up changes nothing.
    let _ = world.tick();
    assert_eq!(world.party_actor_slots, vec![Some(1)]);
    assert!(world.three_actor_talk.is_some());

    // The script ends the talk by clearing the lock (retail: the generic
    // field-VM flag-clear op); the controller's next poll despawns.
    world.system_flag_clear(0xD);
    let _ = world.tick();
    assert_eq!(
        world.party_actor_slots,
        vec![Some(1), Some(0), Some(2)],
        "party count restored from the pre-collapse snapshot"
    );
    assert_eq!(world.party_leader_slot, Some(1), "leader restored");
    assert!(!world.system_flag_test(0xD), "lock stays down");
    assert!(world.three_actor_talk.is_none(), "controller despawned");
}

/// A re-arm mid-talk must not clobber the pre-collapse snapshot: the
/// restore still yields the original party.
#[test]
fn three_actor_talk_rearm_preserves_the_party_snapshot() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.party_actor_slots = vec![Some(0), Some(2)];
    world.party_leader_slot = Some(0);

    let op = talk_op([5, 6, 7], 1, 10);
    let mut ctx = FieldCtx::default();
    for _ in 0..2 {
        // First arm, then a re-arm while the lock is up.
        let mut host = FieldHostImpl { world: &mut world };
        let _ = vm::field::step(&mut host, &mut ctx, &op, 0);
    }
    world.system_flag_clear(0xD);
    let _ = world.tick();
    assert_eq!(world.party_actor_slots, vec![Some(0), Some(2)]);
    assert_eq!(world.party_leader_slot, Some(0));
}

#[test]
fn three_actor_talk_first_arm_without_leader_defaults_to_slot_zero() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    // No leader, no party list: retail reads whatever the leader byte holds;
    // the engine defaults to roster slot 0.
    let op = talk_op([1, 2, 3], 0, 0);
    let mut ctx = FieldCtx::default();
    let mut host = FieldHostImpl { world: &mut world };
    let _ = vm::field::step(&mut host, &mut ctx, &op, 0);
    assert_eq!(world.party_actor_slots, vec![Some(0)]);
    assert!(world.system_flag_test(0x10), "flag 0x10 + leader(0)");
}

// ---- the mid-talk leader cycle (FUN_801D27E0 states 1..=4) ----

/// A world with a live three-actor talk: player actor in slot 0 at
/// `(500, 600)` heading `0x200`, participants 5/6/7 seeded as field NPCs,
/// leader slot 0, presence-flag base `0x10` (the retail alignment where the
/// instruction's u16 names the same flags the arm sets).
fn talk_world_with_participants() -> World {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.player_actor_slot = Some(0);
    world.actors[0].active = true;
    world.actors[0].move_state.world_x = 500;
    world.actors[0].move_state.world_z = 600;
    world.actors[0].move_state.render_26 = 0x200;
    world.party_actor_slots = vec![Some(0), Some(1), Some(2)];
    world.party_leader_slot = Some(0);
    for (slot, pos, heading) in [
        (5u8, (100i16, 200i16), 0x400i16),
        (6, (300, 400), 0x600),
        (7, (700, 800), 0x000),
    ] {
        world.field_npc_positions.insert(slot, pos);
        world.field_npc_headings.insert(slot, heading);
    }
    let op = talk_op([5, 6, 7], 0x10, 10);
    let mut ctx = FieldCtx::default();
    let mut host = FieldHostImpl { world: &mut world };
    let _ = vm::field::step(&mut host, &mut ctx, &op, 0);
    world
}

/// The switch cycle (`FUN_801D27E0` states 0->1->2->3->4->0): a latched
/// request arms the fade-out, the swap hands leadership to the next
/// participant whose presence flag reads clear, the outgoing leader's NPC
/// takes the player's pose, the player takes the incoming NPC's pose, the
/// incoming NPC parks at the `0x3F80` sentinel, and the SM returns to the
/// state-0 poll with the talk still live.
#[test]
fn three_actor_talk_switch_cycles_leader_and_returns_to_poll() {
    let mut world = talk_world_with_participants();
    assert!(world.system_flag_test(0x10), "arm set the leader's flag");

    world.request_talk_leader_switch();
    world.tick_three_actor_talk();
    let talk = world.three_actor_talk.expect("talk stays live");
    assert_eq!(talk.swap.phase, 1, "request arms the fade-out state");
    assert!(
        world.screen_fade.is_some(),
        "state 0 spawned the fade-to-white"
    );

    // Run the SM through the fade / swap / fade cycle back to the poll.
    for _ in 0..100 {
        world.tick_three_actor_talk();
        if world.three_actor_talk.is_some_and(|t| t.swap.phase == 0) {
            break;
        }
    }
    let talk = world.three_actor_talk.expect("talk still live after cycle");
    assert_eq!(talk.swap.phase, 0, "SM returned to the state-0 poll");
    assert!(world.system_flag_test(0xD), "talk lock still up");

    // Leadership moved to slot 1 (scan from leader+1; flag 0x11 was clear).
    assert_eq!(world.party_leader_slot, Some(1));
    assert_eq!(world.party_actor_slots, vec![Some(1)]);
    assert!(!world.system_flag_test(0x10));
    assert!(world.system_flag_test(0x11), "flag 0x10 + new leader set");
    assert!(!world.system_flag_test(0x12));

    // Outgoing leader's participant (id 5) took the player's pose.
    assert_eq!(world.field_npc_positions.get(&5), Some(&(500, 600)));
    assert_eq!(world.field_npc_headings.get(&5), Some(&0x200));
    // The player took the incoming participant's (id 6) pose + heading.
    let ms = &world.actors[0].move_state;
    assert_eq!((ms.world_x, ms.world_z), (300, 400));
    assert_eq!(ms.render_26, 0x600);
    // The incoming participant parked at the 0x3F80 sentinel.
    assert_eq!(world.field_npc_positions.get(&6), Some(&(0x3F80, 0x3F80)));
}

/// All three presence flags set = nobody left to switch to: the arm gate
/// returns before the request route is even consulted (`801d2920`).
#[test]
fn three_actor_talk_switch_blocked_when_all_participants_flagged() {
    let mut world = talk_world_with_participants();
    world.system_flag_set(0x11);
    world.system_flag_set(0x12);
    world.request_talk_leader_switch();
    world.tick_three_actor_talk();
    let talk = world.three_actor_talk.expect("talk stays live");
    assert_eq!(talk.swap.phase, 0, "arm gate blocks with all flags set");
    assert!(world.screen_fade.is_none(), "no fade spawned");
}

/// Exactly two flags set requires the current leader's own flag among them
/// (`801d2928..801d2944`): two set with the leader's clear blocks the arm.
#[test]
fn three_actor_talk_switch_two_flagged_needs_leader_flag() {
    let mut world = talk_world_with_participants();
    // Arm set 0x10 (leader 0). Re-point the two flags at the non-leaders.
    world.system_flag_clear(0x10);
    world.system_flag_set(0x11);
    world.system_flag_set(0x12);
    world.request_talk_leader_switch();
    world.tick_three_actor_talk();
    let talk = world.three_actor_talk.expect("talk stays live");
    assert_eq!(talk.swap.phase, 0, "two-set gate needs the leader's flag");
}

/// The state-2 search steps past participants whose presence flag is set:
/// with slot 1 already flagged, leadership lands on slot 2.
#[test]
fn three_actor_talk_switch_skips_flagged_participant() {
    let mut world = talk_world_with_participants();
    world.system_flag_set(0x11); // slot 1 already had its turn
    world.request_talk_leader_switch();
    for _ in 0..100 {
        world.tick_three_actor_talk();
        if world.three_actor_talk.is_some_and(|t| t.swap.phase == 0)
            && world.party_leader_slot != Some(0)
        {
            break;
        }
    }
    assert_eq!(world.party_leader_slot, Some(2), "search skipped slot 1");
    assert!(world.system_flag_test(0x12), "flag re-pointed at slot 2");
    // The player stands where participant 7 stood.
    let ms = &world.actors[0].move_state;
    assert_eq!((ms.world_x, ms.world_z), (700, 800));
}

/// Only state 0 polls the talk lock: a lock drop mid-swap (states 1..=4)
/// leaves the talk live until the SM returns to the poll.
#[test]
fn three_actor_talk_lock_drop_mid_swap_waits_for_the_poll() {
    let mut world = talk_world_with_participants();
    world.request_talk_leader_switch();
    world.tick_three_actor_talk();
    assert_eq!(world.three_actor_talk.unwrap().swap.phase, 1);

    world.system_flag_clear(0xD);
    world.tick_three_actor_talk();
    assert!(
        world.three_actor_talk.is_some(),
        "states 1..=4 never poll the lock"
    );
    // Back at state 0, the next poll despawns and restores the party.
    for _ in 0..100 {
        world.tick_three_actor_talk();
        if world.three_actor_talk.is_none() {
            break;
        }
    }
    assert!(world.three_actor_talk.is_none(), "poll despawned the talk");
    assert_eq!(
        world.party_actor_slots,
        vec![Some(0), Some(1), Some(2)],
        "pre-collapse party restored"
    );
}

/// The suppressor gate: while a dialogue owns the pad, a latched request
/// does not arm (engine stand-in for retail's `_DAT_8007B6B4` suppressor).
#[test]
fn three_actor_talk_switch_suppressed_under_dialogue() {
    let mut world = talk_world_with_participants();
    world.current_dialog = Some(crate::world::DialogRequest {
        text_id: 0,
        inline: vec![0x1F, b'x', 0x00],
        world_x: 0,
        world_z: 0,
        depth_id: 0,
    });
    world.request_talk_leader_switch();
    world.tick_three_actor_talk();
    assert_eq!(
        world.three_actor_talk.unwrap().swap.phase,
        0,
        "no switch arms under an open text box"
    );
}

// ---- the walk-up talk: engagement, loop, locomotion, facing ----

/// A scene the player can walk in: every collision cell open, every object
/// cell walk-visible, player actor in slot 0 at the origin facing Z+.
fn walkable_talk_scene() -> World {
    let mut w = World::new();
    w.mode = SceneMode::Field;
    w.load_field_collision_grid(&vec![0u8; FIELD_GRID_LEN]);
    w.load_field_object_cells(&[0x00u8, 0x10].repeat(FIELD_GRID_LEN));
    w.player_actor_slot = Some(0);
    w.actors[0].active = true;
    w.actors[0].move_state.world_x = 0;
    w.actors[0].move_state.world_z = 0;
    // Heading 0 = travel Z+, which the probe maps to compass sector 4 and a
    // probe point 64 units up-Z: exactly where the NPC below stands.
    w.actors[0].move_state.render_26 = 0;
    w.actors[0].move_state.field_72 = 0x1000;
    w
}

/// Seat a talkable NPC in front of the player, with an interaction record
/// whose whole content is one text segment and the `0x21` pass terminator -
/// the shape the field-VM inline runner drives. Deliberately registered
/// **only** as a prologue record (no `field_npc_dialog` entry), because that
/// is the case where `current_dialog` stays `None` for the whole conversation
/// and the runner alone owns it - the case every `current_dialog`-only gate
/// in the field tick was blind to.
fn seat_prologue_npc(w: &mut World, slot: u8) {
    w.field_npc_positions.insert(slot, (0, 64));
    w.field_npc_headings.insert(slot, 0x400);
    w.field_npc_dialog_prologue.insert(
        slot,
        crate::man_field_scripts::InlineDialogPrologue {
            body: vec![0x1F, b'h', b'i', 0x00, 0x21],
            entry_pc: 0,
            first_segment: 0,
        },
    );
    w.use_vm_dialogue = true;
}

/// Tick `n` frames with `mask` held. The pad mask is republished every frame
/// because that is what rotates `pad_prev`: a host that sets it once and then
/// ticks leaves `just_pressed` latched true forever, which is a property of
/// the test harness rather than of the engine (every real host calls
/// `set_pad` per frame).
fn hold(w: &mut World, mask: u16, n: usize) {
    for _ in 0..n {
        w.input.set_pad(mask);
        let _ = w.tick();
    }
}

/// Play one conversation the way a player does - press, release, press - and
/// return once no dialogue channel owns the frame.
fn play_out_conversation(w: &mut World) {
    use crate::input::PadButton;
    for _ in 0..40 {
        if !w.dialogue_owns_input() {
            return;
        }
        hold(w, 0, 1);
        // Checked between the two halves: the release frame is often the one
        // that runs the record's tail past the dismissed box, and pressing
        // again after it would simply start the *next* conversation.
        if !w.dialogue_owns_input() {
            return;
        }
        hold(w, PadButton::Cross.mask(), 1);
    }
    panic!("the conversation never ended");
}

/// The probe-driven NPC talk must end when the player dismisses it and stay
/// ended - with the confirm still held and the player still standing in the
/// NPC's interact box.
///
/// The defect this pins: the probe tested only `current_dialog`, so every
/// page-advance press on a runner-owned conversation ALSO re-ran
/// `trigger_field_interact` and re-armed `active_inline_prologue`. The frame
/// the conversation ended, `drive_inline_dialogue` found that leftover and
/// relaunched the same record with no input at all - a conversation that
/// reopened itself, forever, which is what "dialogue loops and is hard to get
/// out of" looks like from the pad.
#[test]
fn probe_talk_ends_and_does_not_reopen_under_a_held_confirm() {
    use crate::input::PadButton;
    let mut world = walkable_talk_scene();
    seat_prologue_npc(&mut world, 3);

    hold(&mut world, PadButton::Cross.mask(), 1);
    assert!(
        world.inline_dialogue.is_some(),
        "the facing probe must open the NPC's interaction record"
    );
    // Let it type; a held button is not a second press.
    hold(&mut world, PadButton::Cross.mask(), 8);
    assert!(world.dialogue_owns_input(), "the box is still up");

    // Dismiss with a real press - release, press - and then NEVER let go. The
    // record's tail (`0x21`) runs out over the next couple of frames while the
    // button is still down, so the conversation ends under a held confirm,
    // which is the state the player is actually in when they mash through a
    // line. From there no pad edge exists at all.
    hold(&mut world, 0, 1);
    for frame in 0..160 {
        world.input.set_pad(PadButton::Cross.mask());
        let _ = world.tick();
        // Two frames to close the box and run the record's tail; after that
        // nothing may bring it back.
        if frame >= 4 {
            assert!(
                !world.dialogue_owns_input(),
                "the conversation reopened by itself on frame {frame} with no \
                 new button edge (held confirm, player still in the interact \
                 box)"
            );
        }
    }
    // And with the pad fully released it must stay closed too - the original
    // relaunch needed no input whatsoever.
    for frame in 0..120 {
        world.input.set_pad(0);
        let _ = world.tick();
        assert!(
            !world.dialogue_owns_input(),
            "the conversation reopened by itself on idle frame {frame}"
        );
    }
    assert!(
        world.active_inline_prologue.is_none(),
        "no interaction staging may survive the conversation"
    );

    // And a fresh press still works - the fix must not have made the NPC
    // permanently unaddressable.
    hold(&mut world, 0, 1);
    hold(&mut world, PadButton::Cross.mask(), 1);
    assert!(
        world.inline_dialogue.is_some(),
        "a new press re-opens the conversation"
    );
}

/// The player must not walk while a dialogue owns the frame.
///
/// `step_field_locomotion` gated on `current_dialog` alone, and the ordinary
/// NPC talk runs through the inline field-VM runner - which for a
/// prologue-selected record never sets `current_dialog`. So the pad walked the
/// player around underneath the box, including while navigating options.
#[test]
fn player_cannot_walk_while_the_inline_runner_owns_the_frame() {
    use crate::input::PadButton;

    // Control: with no dialogue up, a held direction does move the player.
    let mut free = walkable_talk_scene();
    seat_prologue_npc(&mut free, 3);
    hold(&mut free, PadButton::Up.mask(), 1);
    assert_ne!(
        free.actors[0].move_state.world_z, 0,
        "control: the pad walks the player when nothing owns the frame"
    );

    let mut world = walkable_talk_scene();
    seat_prologue_npc(&mut world, 3);
    hold(&mut world, PadButton::Cross.mask(), 1);
    assert!(world.inline_dialogue.is_some(), "the conversation opened");
    let seat = (
        world.actors[0].move_state.world_x,
        world.actors[0].move_state.world_z,
    );
    hold(
        &mut world,
        PadButton::Up.mask() | PadButton::Right.mask(),
        60,
    );
    assert!(
        world.dialogue_owns_input(),
        "the box is still up (a direction is not a dismiss)"
    );
    assert_eq!(
        (
            world.actors[0].move_state.world_x,
            world.actors[0].move_state.world_z
        ),
        seat,
        "the player must stand still while the dialogue owns the pad"
    );
}

/// Talking to an NPC turns it to face the player, instantly, and gives its
/// authored heading back when the conversation ends.
///
/// Retail does both in the dialog SM: the touch post saves `+0x26` into
/// `+0x5A` (`FUN_801D5B5C`), the SM writes the player bearing into `+0x26`
/// with a single `sh` (`FUN_80039B7C` at `0x80039F80` - no ramp, no budget),
/// and the teardown writes the save back.
#[test]
fn talking_turns_the_npc_to_the_player_and_restores_its_facing() {
    use crate::input::PadButton;
    let mut world = walkable_talk_scene();
    seat_prologue_npc(&mut world, 3);
    assert_eq!(
        world.field_npc_headings.get(&3),
        Some(&0x400),
        "the NPC starts on its authored heading"
    );

    hold(&mut world, PadButton::Cross.mask(), 1);
    // Player at (0, 0), NPC at (0, 64): the bearing NPC -> player is -Z, which
    // is 0x800 in the engine's heading space (0 = +Z). One frame, not a ramp.
    assert_eq!(
        world.field_npc_headings.get(&3),
        Some(&0x800),
        "the addressed NPC faces the player on the frame the talk starts"
    );
    assert_eq!(world.field_npc_facing_save, Some((3, 0x400)));

    play_out_conversation(&mut world);
    hold(&mut world, 0, 1);
    assert!(!world.dialogue_owns_input(), "the conversation ended");
    assert_eq!(
        world.field_npc_headings.get(&3),
        Some(&0x400),
        "the NPC goes back to the heading it was authored with"
    );
    assert!(world.field_npc_facing_save.is_none());
}

/// The pause menu is refused while a dialogue owns the player - retail's
/// menu-open accept sits behind `FUN_801D01B0`'s engaged-bit branch, so a
/// talking player's Start never reaches it.
#[test]
fn dialogue_owns_input_covers_both_channels() {
    let mut w = World::new();
    assert!(!w.dialogue_owns_input(), "idle world owns no input");
    w.start_inline_dialogue(vec![0x1F, b'a', 0x00]);
    assert!(
        w.dialogue_owns_input(),
        "the inline runner alone must count - it is the ordinary NPC talk"
    );
    w.inline_dialogue = None;
    w.current_dialog = Some(crate::world::DialogRequest {
        text_id: 0,
        inline: Vec::new(),
        world_x: 0,
        world_z: 0,
        depth_id: 0,
    });
    assert!(w.dialogue_owns_input(), "the simplified request counts too");
}
