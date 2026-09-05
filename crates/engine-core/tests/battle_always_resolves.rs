//! Regressions for the two defects that made a default session unplayable:
//! a battle that could never end, and a battle whose HP never left the fight.
//!
//! **1. Battle entry was ungated; battle driving was not.** A field carrier's
//! scripted fight (`3E FF`) and a world-map region encounter both flipped the
//! world into [`SceneMode::Battle`] regardless of
//! [`World::live_gameplay_loop`], while `World::tick`'s Battle arm only drove
//! the full [`World::live_battle_tick`] when that flag was set - otherwise it
//! ran one bare `step_battle` per frame, which applies no damage, arms no
//! turn and never calls `finish_battle`. A default `play-window` session that
//! walked into a scripted fight therefore entered a battle with no exit.
//! Retail has no "loop enabled" concept: `FUN_801E295C` always drives the
//! battle it is in.
//!
//! **2. Post-battle HP / MP was discarded.** `finish_battle` restored the
//! actor table from the pre-battle snapshot, so every fight ended with the
//! party back at the HP it started with - which also made a party wipe
//! unobservable (you lost, and woke up in the field at full health).
//!
//! Disc-free: synthetic party + the vanilla monster/formation tables.

use legaia_engine_core::monster_catalog::{vanilla_formation_table, vanilla_monster_catalog};
use legaia_engine_core::world::{Actor, SceneMode, World};

/// A world seated for a battle against the vanilla Goblin formation (id 1),
/// with the live loop **off** - the default a bare `World::new()` boots with.
fn world_in_a_battle() -> World {
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party_count = 3;
    for i in 0..3 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 100;
        w.actors[i].battle.max_hp = 100;
        w.actors[i].battle.mp = 40;
        w.actors[i].battle.liveness = 1;
        w.set_battle_attack(i as u8, 60);
    }
    w.load_party(legaia_save::Party::zeroed(3));
    // `load_party` overwrites the mirrors from the (zeroed) records; put the
    // synthetic combat stats back and keep the records in step.
    let mut party = w.roster.clone();
    for rec in party.members.iter_mut() {
        let mut hms = rec.hp_mp_sp();
        hms.hp_cur = 100;
        hms.hp_max = 100;
        hms.mp_cur = 40;
        rec.set_hp_mp_sp(hms);
    }
    w.load_party(party);
    w.set_formation_table(vanilla_formation_table(), vanilla_monster_catalog());
    w.mode = SceneMode::Field;
    assert!(
        !w.live_gameplay_loop,
        "the default must stay off - that is what this test is about"
    );
    w
}

/// Enter the battle the way an ungated entry path does (a field carrier's
/// scripted `3E FF`, a world-map region encounter), then tick with the
/// default flags. The battle MUST reach a terminal state.
#[test]
fn a_battle_entered_with_default_flags_still_resolves() {
    let mut w = world_in_a_battle();
    assert!(
        w.trigger_scripted_battle(0) || w.trigger_scripted_battle(1),
        "the vanilla formation table should register a scripted row"
    );
    // Ordinary `tick`s in Field mode drain the latched scripted battle
    // through `tick_field_carriers` and clock its intro transition (132
    // display frames) - no test-side glue.
    for _ in 0..200 {
        if w.mode == SceneMode::Battle {
            break;
        }
        w.tick();
    }
    assert_eq!(
        w.mode,
        SceneMode::Battle,
        "the scripted-battle path enters battle regardless of the live-loop flag"
    );

    let mut resolved = false;
    for _ in 0..20_000 {
        w.tick();
        if w.mode != SceneMode::Battle {
            resolved = true;
            break;
        }
    }
    assert!(
        resolved,
        "a battle entered with the live loop off must still be driven to a \
         terminal state - otherwise the session is soft-locked in SceneMode::Battle"
    );
    assert!(
        !w.actors[3..].iter().any(|a| a.battle.liveness != 0),
        "the battle resolved by wiping the monsters"
    );
}

/// The battle's HP / MP must survive the return to the field, in BOTH the
/// live actor mirrors and the roster records a save is written from.
#[test]
fn party_hp_and_mp_survive_the_battle() {
    let mut w = world_in_a_battle();
    assert!(w.trigger_scripted_battle(0) || w.trigger_scripted_battle(1));
    // The scripted entry runs the field-to-battle intro transition
    // (132 display frames) before the mode flips.
    for _ in 0..200 {
        if w.mode == SceneMode::Battle {
            break;
        }
        w.tick();
    }
    assert_eq!(w.mode, SceneMode::Battle);

    // Take a chunk out of party slot 0 mid-battle and spend some MP, the way
    // a monster turn and a cast would.
    w.actors[0].battle.hp = 37;
    w.actors[0].battle.mp = 11;

    for _ in 0..20_000 {
        w.tick();
        if w.mode != SceneMode::Battle {
            break;
        }
    }
    assert_ne!(w.mode, SceneMode::Battle, "battle must have resolved");

    assert_eq!(
        w.roster.members[0].hp_mp_sp().hp_cur,
        37,
        "post-battle HP must be persisted into the character record"
    );
    assert_eq!(
        w.roster.members[0].hp_mp_sp().mp_cur,
        11,
        "post-battle MP must be persisted into the character record"
    );
    assert_eq!(
        w.actors[0].battle.hp, 37,
        "the restored field actor must carry the post-battle HP, not the \
         pre-battle snapshot's"
    );
    assert_eq!(w.actors[0].battle.mp, 11, "same for MP");
    // Untouched members keep theirs.
    assert_eq!(w.roster.members[1].hp_mp_sp().hp_cur, 100);
}

/// A party wipe must raise `game_over` (the flag both hosts now read) rather
/// than quietly returning a full-HP party to the field.
#[test]
fn a_party_wipe_raises_game_over_and_leaves_the_party_down() {
    let mut w = world_in_a_battle();
    assert!(w.trigger_scripted_battle(0) || w.trigger_scripted_battle(1));
    // The scripted entry runs the field-to-battle intro transition
    // (132 display frames) before the mode flips.
    for _ in 0..200 {
        if w.mode == SceneMode::Battle {
            break;
        }
        w.tick();
    }
    assert_eq!(w.mode, SceneMode::Battle);

    for i in 0..3 {
        w.actors[i].battle.hp = 0;
        w.actors[i].battle.liveness = 0;
    }
    for _ in 0..20_000 {
        w.tick();
        if w.game_over {
            break;
        }
    }
    assert!(w.game_over, "a party wipe raises game over");
    // The wipe fold defers the field restore (`World::game_over_hold`) so
    // hosts hold the frozen battle frame; the deferred restore is what a
    // host runs when its hand-off session resolves.
    assert!(w.game_over_hold, "the field restore is deferred");
    assert_eq!(w.mode, SceneMode::Battle, "the hold parks the battle scene");
    w.resolve_game_over_hold();
    assert_ne!(
        w.mode,
        SceneMode::Battle,
        "the resolve completes the battle exit"
    );
    // Retail's annihilated arm floors every member at exactly 1 HP on the
    // exit-fade frame (`FUN_8004E568` `0x8004FB94..0x8004FBA4`, one
    // `sh 1,0x14c` per party seat) - a scripted loss returns to the field
    // with the party standing, and a real wipe hands the CARD flow a
    // 1-HP party. Neither is a heal.
    assert_eq!(
        w.roster.members[0].hp_mp_sp().hp_cur,
        1,
        "losing floors the party at 1 HP, never heals it"
    );
    assert!(w.last_battle_rewards.is_none(), "a wipe grants no loot");

    // `revive_party_full` is what a host's "Retry" row runs; without it the
    // party would re-wipe on the next encounter.
    w.revive_party_full();
    assert_eq!(w.roster.members[0].hp_mp_sp().hp_cur, 100);
    assert_eq!(w.actors[0].battle.hp, 100);
    assert_eq!(w.actors[0].battle.liveness, 1);
}

/// The spoils panel is armed by a victory and ages out - the reader
/// `last_battle_rewards` never had.
#[test]
fn a_victory_arms_the_spoils_panel() {
    let mut w = world_in_a_battle();
    // A win-pose table in the shape the SCUS carries (one pair per tier,
    // every id in the `0x11..=0x18` band), so the results frame has a
    // pose to stage.
    w.victory_pose_table = Some([[0x13, 0x14, 0x11, 0x12, 0x15, 0x16]; 4]);
    assert!(w.trigger_scripted_battle(0) || w.trigger_scripted_battle(1));
    // Through the intro transition into the fight first, so the resolution
    // loop below cannot pass vacuously off the pre-battle Field mode.
    for _ in 0..200 {
        if w.mode == SceneMode::Battle {
            break;
        }
        w.tick();
    }
    assert_eq!(w.mode, SceneMode::Battle);
    // Retail raises the result windows on the results frame of the
    // end-of-battle sequence - still IN battle, after the load window -
    // and takes them down with the battle (`world::battle::victory`).
    let mut results_frame = None;
    for i in 0..20_000 {
        w.tick();
        if w.battle_result_screen_active() {
            results_frame = Some(i);
            break;
        }
        assert_eq!(
            w.mode,
            SceneMode::Battle,
            "no field return before the results"
        );
    }
    assert!(
        results_frame.is_some(),
        "a victory raises the result screen"
    );
    // The results frame staged the LEADER's win pose (`ctx[+0x13]` = seat
    // 0): an id off the table row lands in the pose actor's `+0x1DA`
    // mirror, and the commit keeps it as the committed value in a world
    // without a clip bank.
    let seq = w.battle_victory.expect("the sequence is armed");
    assert_eq!(seq.pose_actor, 0, "the leader poses");
    let pose = seq.pose_id.expect("the table picks a pose");
    assert!((0x11..=0x18).contains(&pose), "win-pose band: {pose:#x}");
    assert_eq!(
        w.actors[0].battle.queued_anim, pose,
        "the win pose is staged on the pose actor"
    );
    assert!(
        w.screen_fade.is_none(),
        "the exit fade waits for the results hold"
    );
    let banner = w
        .battle_spoils_banner()
        .expect("the result screen carries the spoils panel");
    assert!(banner.xp > 0 || banner.gold > 0, "the panel shows the loot");
    assert_eq!(
        w.mode,
        SceneMode::Battle,
        "the results are drawn over the battle"
    );

    // The hold, the exit fade and the exit gate: results + 0x100 + 0x43.
    let mut exited = false;
    let mut fade_frames = 0u32;
    for _ in 0..(World::VICTORY_RESULTS_HOLD_FRAMES + World::VICTORY_EXIT_PHASE + 8) {
        w.tick();
        if w.mode != SceneMode::Battle {
            exited = true;
            break;
        }
        assert!(
            w.battle_spoils_banner().is_some(),
            "the panel stays up through the exit fade"
        );
        // While the phase halfword counts (`ctx[+0x6CE] >= 2`) the kind-2
        // fade template is live: `B - F`, black -> white, i.e. the scene
        // darkening to black, result windows included.
        if let Some(legaia_engine_core::world::VictorySequence {
            phase: legaia_engine_core::world::VictoryPhase::Exit { .. },
            ..
        }) = w.battle_victory
        {
            let fade = w
                .screen_fade
                .expect("the exit fade is live while the phase halfword counts");
            assert_eq!(fade.kind, 2, "the escape / results template is kind 2");
            assert_eq!(fade.abr(), 2, "kind 2 draws B - F: a fade to black");
            fade_frames += 1;
        }
    }
    assert!(exited, "the exit gate returns to the field");
    // Phases `2..=0x42` each drew the fade - the ramp lands after `0x40`
    // steps and the template's `-1` hold word keeps the black up past it.
    assert_eq!(
        u32::from(World::VICTORY_EXIT_PHASE - World::VICTORY_FADE_PHASE_SEED),
        fade_frames,
        "the fade is up for every counted phase frame before the gate"
    );
    assert!(
        w.screen_fade.is_none(),
        "the fade actor dies with the battle: the teardown clears it"
    );
    assert!(
        w.battle_spoils_banner().is_none(),
        "the windows come down with the battle"
    );
}

/// A **cast** must not park the action SM.
///
/// `MagicSustain` (`0x2B`) holds while the caster's `spell_iter`
/// (`actor+0x1FA`) is non-zero, and the SM only ever *sets* that byte -
/// retail's cast-animation system counts it back down, and the port has no
/// such driver. The result was that any battle in which a monster cast a
/// spell stopped dead in `MagicSustain` forever, which is most real
/// encounters. `live_battle_tick` retires it on the frame the state is
/// reached, the same way it retires `ADVANCE_DONE` on the recovery edge.
///
/// The band that reaches `0x2B` is the anim-chain one: a monster's cast (the
/// `0x29` exit bumps past the spell id, stages the clip at `params[1]`, and
/// the chain runs to its terminator). A **party** Seru cast never gets there
/// - see the sibling below.
#[test]
fn a_monster_cast_does_not_park_the_action_sm() {
    use legaia_engine_vm::battle_action::ActionState;

    let mut w = World::new();
    w.mode = SceneMode::Battle;
    w.party_count = 3;
    // A **priced** catalog, so the band's MP debit is a real subtraction and
    // not the zero an unwired `spell_mp_cost` used to hand it. Flame is 5 MP.
    w.set_spell_catalog(legaia_engine_core::spells::SpellCatalog::vanilla());
    let flame_cost = u16::from(w.spell_catalog.mp_cost(0x20));
    assert!(flame_cost > 0, "the catalog prices Flame");
    for i in 0..8 {
        let a = w.spawn_actor(i);
        a.battle.liveness = 1;
        a.battle.hp = 500;
        a.battle.max_hp = 500;
        a.battle.mp = 60;
    }
    // Monster slot 3 casts Flame at party slot 0: the spell id, one cast clip,
    // the terminator - the stream `World::arm_monster_cast` writes.
    w.actors[3].battle.action_category = 2; // Magic
    w.actors[3].battle.active_target = 0;
    w.actors[3].battle.params[0] = 0x20;
    w.actors[3].battle.params[1] = 0x21;
    w.actors[3].battle.params[2] = 0xFF;
    w.battle_ctx.active_actor = 3;
    w.battle_ctx.queued_action = 2;
    w.battle_ctx.action_state = ActionState::Begin.as_byte();

    let mut reached_sustain = false;
    let mut left_sustain = false;
    for _ in 0..4_000 {
        w.tick();
        let st = w.battle_ctx.action_state;
        if st == ActionState::MagicSustain.as_byte() {
            reached_sustain = true;
        } else if reached_sustain {
            left_sustain = true;
            break;
        }
    }
    assert!(
        reached_sustain,
        "the Magic band must reach MagicSustain at all"
    );
    assert!(
        left_sustain,
        "MagicSustain must not hold forever - nothing else in the engine \
         ever clears the caster's spell_iter"
    );
    // ...and the cast was actually paid for. `MagicCastBegin` reads the price
    // through `BattleActionHost::spell_mp_cost`, which the engine answers from
    // the same catalog the live cast path charges from; a host wired to a
    // table nothing fills makes this line read 60.
    assert_eq!(
        w.actors[3].battle.mp,
        60 - flame_cost,
        "Flame costs {flame_cost} MP - the Magic band must debit it, not cast for free"
    );
}

/// The party twin: a player Seru id is a **summon** to retail's cast trigger
/// (`FUN_801DBF9C` stages `actor[+0x1E0] = 9` for every id `>= 0x25`), so
/// the band leaves `0x29` for `0x32..0x38`, never `0x2B`. The summon band
/// holds `0x36` on the stager's return, and a headless driver seats no
/// creature - the stager must still run the choreography out and the band
/// must end, with the cast paid for once at `0x28`.
#[test]
fn a_party_seru_cast_runs_the_summon_band_out_and_pays_once() {
    use legaia_engine_vm::battle_action::ActionState;

    let mut w = World::new();
    w.mode = SceneMode::Battle;
    w.party_count = 3;
    // Gimard is 10 MP byte-exact from SCUS.
    w.set_spell_catalog(legaia_engine_core::retail_magic::retail_seru_magic_catalog());
    for i in 0..8 {
        let a = w.spawn_actor(i);
        a.battle.liveness = 1;
        a.battle.hp = 500;
        a.battle.max_hp = 500;
        a.battle.mp = 60;
    }
    // Party slot 0 casts a player Seru spell at monster slot 3.
    w.actors[0].battle.action_category = 2; // Magic
    w.actors[0].battle.active_target = 3;
    w.actors[0].battle.params[0] = 0x81;
    w.battle_ctx.active_actor = 0;
    w.battle_ctx.queued_action = 2;
    w.battle_ctx.action_state = ActionState::Begin.as_byte();

    let mut visited = Vec::new();
    let mut reached_sustain = false;
    let mut left_sustain = false;
    let mut band_over = false;
    for _ in 0..4_000 {
        w.tick();
        let st = w.battle_ctx.action_state;
        if visited.last() != Some(&st) {
            visited.push(st);
        }
        if st == ActionState::SummonSustain.as_byte() {
            reached_sustain = true;
        } else if reached_sustain && !left_sustain {
            left_sustain = true;
        }
        if left_sustain && st >= ActionState::DoneCleanup.as_byte() {
            band_over = true;
            break;
        }
    }
    assert!(
        !visited.contains(&ActionState::MagicSustain.as_byte()),
        "a party Seru cast is the summon route, not the anim chain: {visited:02x?}"
    );
    assert!(
        reached_sustain,
        "the summon band must reach SummonSustain: {visited:02x?}"
    );
    assert!(
        left_sustain && band_over,
        "the stager's hold on 0x36 must release with no host seat, and the \
         band must end: {visited:02x?}"
    );
    assert!(
        w.summon_stager.is_none(),
        "the stager retired with the band"
    );
    assert_eq!(
        w.actors[0].battle.mp, 50,
        "Gimard costs 10 MP - the Magic band must debit it once, at 0x28"
    );
}
