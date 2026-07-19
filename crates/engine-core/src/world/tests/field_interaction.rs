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

    // Accept (just-pressed Cross): dismiss -> engage -> SM -> Field -> Battle.
    world.input.set_pad(PadButton::Cross.mask());
    let _ = world.tick();
    assert!(
        world.pending_carrier_engage.is_none(),
        "the armed engage is consumed on the accept"
    );
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

    // Release, then accept: the probe dismisses the box and engages -> Battle.
    world.input.set_pad(0);
    let _ = world.tick();
    world.input.set_pad(PadButton::Cross.mask());
    let _ = world.tick();
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

    // Confirm: flips to Battle within a tick or two.
    world.input.set_pad(0);
    let _ = world.tick();
    world.input.set_pad(PadButton::Cross.mask());
    let mut reached = false;
    for _ in 0..4 {
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
