//! Battle-depth replay: every command class issued **by pad**, and scored.
//!
//! [`critical_path_replay`](critical_path_replay.rs) asks how far a player
//! pressing buttons can walk. This asks what a player pressing buttons can
//! *do once a battle starts*, and it exists because the answer was "one
//! thing".
//!
//! ## Why a neutral pad hid this
//!
//! The critical path fights exactly one random encounter and fights it with
//! `set_pad(0)` every frame, deliberately - pressing into it would make the
//! walk's score a function of the battle UI. That works only because the
//! walk never sets `battle_player_driven`, so the action SM auto-resolves
//! each party turn as a physical Attack.
//!
//! The consequence is structural, not incidental: **no test anywhere drove a
//! Magic cast, an Item use, a Summon, Spirit or Run end-to-end through
//! `World::tick` from the pad.** Arts had a pad-driven test; the other five
//! command classes were covered only at SM level, at formula level, or by
//! `crates/engine-core/src/world/tests/` calling `pub(in crate::world)`
//! methods directly - which bypasses `World::tick` and is unreachable from an
//! integration test. A surface can therefore be wired to nothing and stay
//! green everywhere.
//!
//! So every rung here goes through `World::set_pad` + `BootSession::tick`.
//! Nothing calls a battle internal, and nothing pokes a session into the
//! state it is supposed to reach. Setup stocks the *player* (spells learned,
//! items carried, MP available) because a player who has none cannot press
//! the button either - but the button is always pressed.
//!
//! ## The command model is three phases, not one menu
//!
//! Retail's `FUN_801D0748` walks `ctx[+0x06]` through three selection
//! surfaces ([`legaia_engine_core::battle_input`]), and a driver that assumes
//! a single flat menu never reaches half the ladder:
//!
//! | `ctx[+0x06]` | phase | chips |
//! |---|---|---|
//! | `0x1E` | `RoundPrompt` | `Begin` / `Run` - once per **round** |
//! | `0x28` | `Menu` (4-arm ring) | Up=Item, Left=Attack, Right=magic, Down=Spirit |
//! | `0x78` | `AttackMode` | `Auto` / `Command` |
//!
//! Three consequences the rungs below depend on. **Arts is not a ring arm** -
//! it is `Attack` -> `AttackMode::Command`, the directional entry. **Run is
//! not a ring arm** - it is on the round prompt, taken by Circle outright and
//! refused when `battle_no_escape`. **There is no Summon arm** - a summon is
//! a *spell* with id `0x81..=0x95` cast from the Magic submenu, detected in
//! `world/battle/casting.rs` and drained by the host through
//! `take_pending_summon_spawn`.
//!
//! Every surface reads `just_pressed`, so a held mask is one event: presses
//! here are two-frame taps ([`tap`]).
//!
//! ## The ladder
//!
//! Rungs are ordered and cumulative - the run stops at the first one it
//! cannot reach and the score is the count it cleared. A stall names the
//! surface that refused and what it was showing, because *that* is the
//! finding; reaching around a refusal by poking session state would make the
//! rung meaningless.
//!
//! | # | rung | what it proves |
//! |---|---|---|
//! | 1 | battle entered, command menu open | the disc scene hands a real fight to the pad |
//! | 2 | Attack | the ring's Attack arm -> `Auto` -> target -> a strike lands |
//! | 3 | Arts entry | `Attack` -> `Command` opens the per-press entry and it debits AP |
//! | 4 | Arts executed | `Begin` runs the typed sequence out as strikes |
//! | 5 | Magic | the magic arm's menu casts and charges MP |
//! | 6 | Summon | a `0x81..=0x8B` cast raises a summon spawn |
//! | 7 | Item | the Item arm's menu consumes a carried item |
//! | 8 | Spirit | the Spirit arm resolves and charges the gauge |
//! | 9 | Run refused | `battle_no_escape` denies the round prompt's Run |
//! | 10 | Run | Circle on the round prompt escapes the battle |
//!
//! ## What the order is carrying
//!
//! Two engine defects shaped it, both found by this file and both since
//! fixed; the ordering that exposed them is kept because it is what makes the
//! ladder able to tell "broken" from "unmeasured".
//!
//! **An item use parked the fight.** After a battle heal, no further party
//! command session opened - measured to 9000 ticks with the mode still
//! `Battle` and every surface closed, and with no in-battle exit at all: not
//! even winning it, because the turn pump that would notice the opponent at
//! 0 HP is the thing that stopped. The cause was the readout, not the
//! command: `World::use_item` wrote live HP without seeding the HP-bar
//! accumulator retail's applier `FUN_800402F4` assigns, and `hp != hp_display`
//! with a zero accumulator is absorbing, so the action SM's `0x51` gate waited
//! on a bar nothing could move. Rungs 7 and 8 now run in the *same* battle as
//! everything before them, which is what makes each rung's opening
//! `wait_for_command` an assertion that the previous command did not park it.
//!
//! Summon and Spirit were reported as parking too. Measured one at a time
//! from a fresh fight, neither does: a summon cast and a Spirit guard each
//! hand out the next command session inside ~230 ticks. They resolve without
//! a strike, like Item, but they touch no party HP - the shared symptom was
//! the item's, and the other two were inference from it.
//!
//! **No battle opened the round prompt.** `CommandPhase::RoundPrompt` and
//! `step_round_prompt` were both implemented and reachable, but the session
//! was *built* on the ring and rewritten onto the prompt by the next tick's
//! `arm_round_open_prompt`. `battle_command.is_some()` is the only edge an
//! observer has, so the prompt was one frame behind every look at it and read
//! as absent - taking `Run`, which lives nowhere else, with it. Retail stores
//! `ctx[+0x06] = 0x1E` in `0x14` before anything is drawn, and rungs 9 and 10
//! check the prompt on exactly that frame.
//!
//! ## Ratchet
//!
//! [`BASELINE`] carries the highest score reached so far. The test asserts
//! `score >= BASELINE` (no regression) and prints the line to paste when the
//! score goes up. Raising it is a reviewed edit, the contract
//! `scripts/ci/disc-coverage.py` and the critical-path baseline both use.
//!
//! Skip-pass (CLAUDE.md disc-gated convention): `LEGAIA_DISC_BIN` unset or
//! `extracted/` missing.

use std::path::PathBuf;

use legaia_engine_core::arts_command_input::ArtsInputScreen;
use legaia_engine_core::battle_input::{AttackMode, BattleCommand, CommandPhase, RoundChoice};
use legaia_engine_core::input::{InputState, PadButton};
use legaia_engine_core::world::SceneMode;
use legaia_engine_shell::boot::{BootConfig, BootSession, FieldLiveOpts};

/// The opening town, and the only scene carrying the sparring formation the
/// ladder fights.
const SCENE: &str = "town01";

/// town01 MAN formation index 4 - the lone-monster Rim Elm sparring partner
/// (`training_battle.rs` pins the id and its 999 HP off the disc archive).
/// A long-lived opponent is what lets one battle carry eight rungs.
const TRAINING_FORMATION_ID: u16 = 4;

/// Highest rung count reached so far. Raise deliberately; never auto-written.
const BASELINE: usize = 10;

/// Healing Leaf, the restorative rung 5 uses. Stocked explicitly so the rung
/// tests the *menu*, not the starting bag's contents.
///
/// The id is `0x77` and that matters: `ItemCatalog::vanilla` carries the real
/// retail ids, and its own unit test exists to keep anyone from reaching for
/// "the old fabricated `0x01..` sequence". An id outside the catalog stocks
/// an item the menu will never list, which reads as a broken Item arm.
const ITEM_HEALING_LEAF: u8 = 0x77;

/// The two spells the magic rungs cast.
///
/// Note what the disc actually offers here: the catalog `enter_field_live`
/// installs for a player-driven battle is
/// `retail_magic::retail_seru_magic_catalog`, and it is **exactly**
/// `0x81..=0x8B` - eleven ids, every one of them inside
/// `summon::SERU_SUMMON_IDS`. There is no non-summon player-castable spell in
/// retail, so rungs 6 and 7 cannot be split by picking a "plain" spell. They
/// are split by what they assert instead: rung 6 proves the magic arm casts
/// and charges MP, rung 7 proves a cast raises a summon spawn for the host to
/// drain. (`SpellCatalog::vanilla()`'s `0x10..` healing/damage ids are a
/// disc-free fixture, not what boot installs.)
const SPELL_FOR_MP_RUNG: u8 = 0x8B;
const SPELL_SUMMON: u8 = 0x81;

/// Frames a single surface hand-off is allowed before the rung is called
/// stalled. Generous: a cast plays an effect timeline before the SM parks.
const SETTLE_TICKS: usize = 9000;

fn disc_present() -> bool {
    std::env::var_os("LEGAIA_DISC_BIN").is_some()
}

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// One press frame followed by one release frame. Every battle surface reads
/// `just_pressed`, so a held mask is a single event no matter how long it is
/// held - a driver that holds a direction enters one command, not four.
fn tap(session: &mut BootSession, button: PadButton) {
    session.host.world.set_pad(InputState::mask_of([button]));
    let _ = session.tick();
    session.host.world.set_pad(0);
    let _ = session.tick();
}

/// Advance with a neutral pad until `f` holds, returning whether it did.
/// The neutral pad matters: the action SM runs its timelines between player
/// turns and a held button would be re-read by whatever opens next.
fn settle_until(session: &mut BootSession, ticks: usize, f: impl Fn(&BootSession) -> bool) -> bool {
    for _ in 0..ticks {
        if f(session) {
            return true;
        }
        session.host.world.set_pad(0);
        let _ = session.tick();
    }
    f(session)
}

fn monster_hp_total(session: &BootSession) -> u32 {
    let w = &session.host.world;
    (w.party_count as usize..w.actors.len())
        .map(|i| w.actors[i].battle.hp as u32)
        .sum()
}

/// Which battle submenu surfaces are open, named rather than boolean so a
/// stall report says which arm answered.
fn open_surfaces(session: &BootSession) -> Vec<&'static str> {
    let w = &session.host.world;
    let mut out = Vec::new();
    if w.arts_input_active() {
        out.push("arts_input");
    }
    if w.battle_arts_menu.is_some() {
        out.push("arts_menu");
    }
    if w.battle_spell_menu.is_some() {
        out.push("spell_menu");
    }
    if w.battle_item_menu.is_some() {
        out.push("item_menu");
    }
    if w.battle_command.is_some() {
        out.push("command");
    }
    out
}

/// A one-line description of what the pad is currently facing, for stalls.
fn surface_report(session: &BootSession) -> String {
    let w = &session.host.world;
    let phase = w.battle_command.as_ref().map(|s| match s.phase {
        CommandPhase::RoundPrompt { .. } => format!("RoundPrompt({:?})", s.round_choice()),
        CommandPhase::Menu { .. } => format!("Menu({:?})", s.menu_command()),
        CommandPhase::AttackMode { .. } => format!("AttackMode({:?})", s.attack_mode()),
        ref other => format!("{other:?}"),
    });
    // Party / monster HP are in the report because the two ways a battle
    // stops handing out command sessions - the party is wiped, or the
    // opponent is dead - look identical from the surfaces alone.
    let party_hp: u32 = (0..w.party_count as usize)
        .map(|i| w.actors[i].battle.hp as u32)
        .sum();
    format!(
        "mode={:?} surfaces={:?} command_phase={:?} party_hp={} monster_hp={}",
        w.mode,
        open_surfaces(session),
        phase,
        party_hp,
        monster_hp_total(session)
    )
}

/// Stall text for "the next turn never opened", with the surface named. A
/// bare "no command session" cannot distinguish a parked SM from a battle
/// that ended, and those want opposite fixes.
fn no_command(session: &BootSession, after: &str) -> String {
    format!(
        "no command session after {after} ({})",
        surface_report(session)
    )
}

/// Wait for a party turn to hand the pad a command session.
fn wait_for_command(session: &mut BootSession) -> bool {
    settle_until(session, SETTLE_TICKS, |s| {
        s.host.world.battle_command.is_some()
    })
}

/// Drive the open command session to `want`, through whichever of retail's
/// three selection surfaces stand between it and the pad.
///
/// `Arts` is reached *through* `Attack` (the attack-mode prompt's `Command`
/// chip), which is where retail puts the directional entry; every other
/// command is a ring arm reached by walking the cursor with Down.
///
/// Returns `false` rather than panicking so the caller can report the stall
/// with the surface named.
fn pick_command(session: &mut BootSession, want: BattleCommand) -> bool {
    let ring_arm = match want {
        BattleCommand::Arts => BattleCommand::Attack,
        other => other,
    };
    let want_mode = match want {
        BattleCommand::Arts => AttackMode::Command,
        _ => AttackMode::Auto,
    };
    for _ in 0..64 {
        // Decide the press with the borrow, then drop it - `tap` needs the
        // session mutably and the surface may change under it.
        let press = {
            let Some(cmd) = session.host.world.battle_command.as_ref() else {
                // The session handed the pad on - either to a submenu or to
                // the action SM. Either way the walk is done.
                return true;
            };
            match cmd.phase {
                // Begin falls through to the ring; the Run rungs own that chip.
                CommandPhase::RoundPrompt { .. } => {
                    if cmd.round_choice() == Some(RoundChoice::Begin) {
                        PadButton::Cross
                    } else {
                        PadButton::Left
                    }
                }
                CommandPhase::Menu { .. } => {
                    if cmd.menu_command() == Some(ring_arm) {
                        PadButton::Cross
                    } else {
                        // Spatial seating: tap the wanted arm's own side.
                        match ring_arm {
                            BattleCommand::Item => PadButton::Up,
                            BattleCommand::Attack => PadButton::Left,
                            BattleCommand::Magic => PadButton::Right,
                            _ => PadButton::Down,
                        }
                    }
                }
                CommandPhase::AttackMode { .. } => {
                    if cmd.attack_mode() == Some(want_mode) {
                        PadButton::Cross
                    } else if want_mode == AttackMode::Auto {
                        PadButton::Left
                    } else {
                        PadButton::Right
                    }
                }
                // Targeting or already resolved: the walk is done.
                _ => return true,
            }
        };
        tap(session, press);
    }
    false
}

/// Confirm through a target picker if one is up. Attack / Magic open a cursor
/// after the arm is taken; the default seat is a live enemy, so a single
/// confirm commits.
fn confirm_target(session: &mut BootSession) {
    for _ in 0..8 {
        let up = session
            .host
            .world
            .battle_command
            .as_ref()
            .map(|s| matches!(s.phase, CommandPhase::Targeting { .. }))
            .unwrap_or(false);
        if !up {
            return;
        }
        tap(session, PadButton::Cross);
    }
}

/// Is the open command session showing the round prompt?
fn at_round_prompt(session: &BootSession) -> bool {
    session
        .host
        .world
        .battle_command
        .as_ref()
        .map(|s| matches!(s.phase, CommandPhase::RoundPrompt { .. }))
        .unwrap_or(false)
}

/// Re-enter the field scene from scratch, abandoning any battle in progress.
///
/// The two Run rungs need it: `battle_no_escape` is copied into the command
/// session when the session opens, so the flag has to be set before the
/// battle starts, and the two rungs want opposite values. Rung 10 also *ends*
/// its battle by escaping, so anything after it would need a fresh one anyway.
///
/// It used to carry a second job - working around commands that parked the
/// fight - and no longer does; rungs 7 and 8 run in the battle before them so
/// that a park would cost rungs rather than be routed around. This is a
/// host-level scene restart, the same call the boot path makes; it touches no
/// battle internal and no command surface.
fn reset_to_field(session: &mut BootSession) -> bool {
    session
        .enter_field_live(
            SCENE,
            &FieldLiveOpts {
                live_loop: true,
                player_battle: true,
                ..Default::default()
            },
        )
        .is_ok()
        && session.host.world.mode == SceneMode::Field
}

/// Stock the player so every command class has something to select. This is
/// progression the player would own, not a bypass: each rung still walks the
/// ring and presses the button that uses it.
fn stock_player(session: &mut BootSession) {
    let w = &mut session.host.world;
    w.inventory.insert(ITEM_HEALING_LEAF, 9);
    for member in w.roster.members.iter_mut() {
        // Prepend order fixes the menu rows: the last one prepended sits at
        // row 0, so the summon is row 0 and the MP-rung spell is row 1.
        legaia_engine_core::magic_xp::learn_spell_prepend(member, SPELL_FOR_MP_RUNG);
        legaia_engine_core::magic_xp::learn_spell_prepend(member, SPELL_SUMMON);
    }
    // MP the casts are charged against, and a party that survives 999 HP of
    // sparring partner long enough to finish the ladder. (`BattleActor` has
    // no `max_mp` - `mp` is the whole pool.)
    // Deliberately *not* at full HP: a heal item on a full-HP party is a
    // legitimately refused selection, and rung 5 would then be measuring the
    // usability gate rather than the Item arm.
    // Large, and deliberately *below* max: the ladder spends tens of
    // thousands of frames in battle across its settle loops, and the
    // opponent swings on every one of them, so a normal HP bar is wiped long
    // before the late rungs. Below max because a heal item on a full-HP party
    // is a legitimately refused selection, and the Item rung would then be
    // measuring the usability gate rather than the Item arm.
    for i in 0..w.party_count as usize {
        w.actors[i].battle.max_hp = 60000;
        w.actors[i].battle.hp = 30000;
        w.actors[i].battle.mp = 999;
    }
}

/// Boot the scene, stock the player, and drive into the sparring battle.
fn enter_battle(session: &mut BootSession) -> bool {
    stock_player(session);
    if session
        .host
        .world
        .install_man_formation(TRAINING_FORMATION_ID)
        != Some(TRAINING_FORMATION_ID)
    {
        return false;
    }
    if !session.host.world.on_field_step() {
        return false;
    }
    if !settle_until(session, 480, |s| s.host.world.mode == SceneMode::Battle) {
        return false;
    }
    // Make the opponent outlast the ladder. Eight rungs land damage (rungs 9
    // and 10 spend whole turns attacking just to roll the round over), and
    // Tetsu's 999 HP does not survive that - the fight would end mid-ladder
    // and the late rungs would report "no command session" when what actually
    // happened is that the ladder won.
    //
    // This arranges the *fixture*, not the result: nothing here touches a
    // command surface, and every rung still reaches its outcome by pressing
    // the button that causes it.
    let w = &mut session.host.world;
    for i in w.party_count as usize..w.actors.len() {
        if w.actors[i].battle.max_hp > 0 {
            w.actors[i].battle.max_hp = 60000;
            w.actors[i].battle.hp = 60000;
        }
    }
    true
}

#[test]
fn battle_depth_ladder() {
    if !disc_present() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
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
                // Without this the SM auto-resolves every party turn as a
                // physical Attack and the pad is inert - the exact blind spot
                // this file exists to close.
                player_battle: true,
                ..Default::default()
            },
        )
        .expect("enter field live");

    let mut cleared: Vec<&'static str> = Vec::new();
    let mut stall: Option<String> = None;

    // Each rung returns Ok(()) or Err(reason). The ladder stops at the first
    // Err and reports it; a stall is the finding, not an assertion failure.
    let mut run = || -> Result<(), String> {
        // --- rung 1: a real battle, opened to the pad ---------------------
        if !enter_battle(&mut session) {
            return Err(format!(
                "never reached Battle from town01 ({})",
                surface_report(&session)
            ));
        }
        if !wait_for_command(&mut session) {
            return Err(format!(
                "battle opened but no command session reached the pad ({})",
                surface_report(&session)
            ));
        }
        cleared.push("battle-entered");

        // --- rung 2: Attack ----------------------------------------------
        let hp_before = monster_hp_total(&session);
        if !pick_command(&mut session, BattleCommand::Attack) {
            return Err(format!(
                "Attack arm unreachable ({})",
                surface_report(&session)
            ));
        }
        confirm_target(&mut session);
        if !settle_until(&mut session, SETTLE_TICKS, |s| {
            monster_hp_total(s) < hp_before
        }) {
            return Err(format!(
                "Attack committed but no damage landed ({})",
                surface_report(&session)
            ));
        }
        cleared.push("attack");

        // --- rung 3: Arts command entry ----------------------------------
        if !wait_for_command(&mut session) {
            return Err(no_command(&session, "Attack"));
        }
        if !pick_command(&mut session, BattleCommand::Arts) {
            return Err(format!(
                "Arts entry unreachable ({})",
                surface_report(&session)
            ));
        }
        if !session.host.world.arts_input_active() {
            return Err(format!(
                "Attack->Command did not open the arts entry ({})",
                surface_report(&session)
            ));
        }
        let pool_before = session
            .host
            .world
            .arts_input_view()
            .map(|v| v.pool)
            .ok_or("the arts entry publishes no view")?;
        tap(&mut session, PadButton::Up);
        let after = session
            .host
            .world
            .arts_input_view()
            .map(|v| (v.buffer.len(), v.pool));
        match after {
            // The entry may auto-end on the very press that exhausts a small
            // pool, which is retail's `0x50 -> 0x5A`; either way the press
            // was read and paid for.
            Some((n, pool)) if n > 0 && pool < pool_before => {}
            Some((n, pool)) => {
                return Err(format!(
                    "the entry did not read the direction (buffer={n}, pool {pool_before}->{pool})"
                ));
            }
            None => {
                return Err(format!(
                    "the arts entry closed on one direction ({})",
                    surface_report(&session)
                ));
            }
        }
        cleared.push("arts-entry");

        // --- rung 4: the typed sequence executes -------------------------
        let hp_before = monster_hp_total(&session);
        // Type until the *entry phase* ends, then confirm through
        // Review -> Begin -> target.
        //
        // The loop condition has to be the phase, not `arts_input_active()`:
        // the session stays alive through Review, Begin|Reselect and
        // Targeting, so a driver that keeps pressing a direction while it is
        // "active" walks past the auto-end, toggles the Begin|Reselect cursor
        // onto **Reselect**, and the next confirm wipes the buffer and drops
        // it back into a fresh entry - a perfect loop that never strikes.
        for _ in 0..24 {
            let entering = session
                .host
                .world
                .arts_input_view()
                .map(|v| matches!(v.phase, ArtsInputScreen::Entering))
                .unwrap_or(false);
            if !entering {
                break;
            }
            tap(&mut session, PadButton::Up);
        }
        // From Review, Cross alone walks Review -> BeginMenu{cursor:0} ->
        // Begin -> target -> confirmed. Cursor 0 is Begin, so no navigation.
        for _ in 0..24 {
            if !session.host.world.arts_input_active() {
                break;
            }
            tap(&mut session, PadButton::Cross);
        }
        if !settle_until(&mut session, SETTLE_TICKS, |s| {
            monster_hp_total(s) < hp_before
        }) {
            return Err(format!(
                "the typed arts sequence landed no strike ({})",
                surface_report(&session)
            ));
        }
        cleared.push("arts-executed");

        // --- rung 5: Magic ------------------------------------------------
        if !wait_for_command(&mut session) {
            return Err(no_command(&session, "Arts"));
        }
        let caster = session
            .host
            .world
            .battle_command
            .as_ref()
            .map(|s| s.actor as usize)
            .unwrap_or(0);
        let mp_before = session.host.world.actors[caster].battle.mp;
        if !pick_command(&mut session, BattleCommand::Magic) {
            return Err(format!(
                "magic arm unreachable ({})",
                surface_report(&session)
            ));
        }
        if session.host.world.battle_spell_menu.is_none() {
            return Err(format!(
                "the magic arm opened no spell menu ({})",
                surface_report(&session)
            ));
        }
        // Seat the plain spell: the summon was prepended last, so it sits at
        // row 0 and the plain spell one below it.
        tap(&mut session, PadButton::Down);
        for _ in 0..12 {
            if session.host.world.battle_spell_menu.is_none() {
                break;
            }
            tap(&mut session, PadButton::Cross);
        }
        if !settle_until(&mut session, SETTLE_TICKS, |s| {
            s.host.world.actors[caster].battle.mp < mp_before
        }) {
            return Err(format!(
                "the spell menu charged no MP (mp={mp_before}, {})",
                surface_report(&session)
            ));
        }
        cleared.push("magic");

        // --- rung 6: Summon ------------------------------------------------
        if !wait_for_command(&mut session) {
            return Err(no_command(&session, "Magic"));
        }
        // Drain anything the previous cast queued so the spawn observed below
        // belongs to this rung.
        let _ = session.host.world.take_pending_summon_spawn();
        if !pick_command(&mut session, BattleCommand::Magic) {
            return Err(format!(
                "magic arm unreachable for the summon ({})",
                surface_report(&session)
            ));
        }
        if session.host.world.battle_spell_menu.is_none() {
            return Err(format!(
                "no spell menu for the summon ({})",
                surface_report(&session)
            ));
        }
        // Row 0 is the summon (prepended last).
        for _ in 0..12 {
            if session.host.world.battle_spell_menu.is_none() {
                break;
            }
            tap(&mut session, PadButton::Cross);
        }
        let mut spawned = false;
        for _ in 0..SETTLE_TICKS {
            if session.host.world.take_pending_summon_spawn().is_some() {
                spawned = true;
                break;
            }
            session.host.world.set_pad(0);
            let _ = session.tick();
        }
        if !spawned {
            return Err(format!(
                "casting summon {SPELL_SUMMON:#04x} raised no summon spawn ({})",
                surface_report(&session)
            ));
        }
        cleared.push("summon");

        // --- rung 7: Item -------------------------------------------------
        // Item stays late, and stays in the *same* fight, both on purpose.
        // This is the command that parked the battle: once the heal resolved,
        // no further party command session ever opened - measured to 9000
        // ticks with the mode still `Battle` and every surface closed. It sat
        // behind a restart while that was true, because anywhere earlier it
        // masked every rung after it (the first pass of this file scored 5 and
        // left Magic / Summon / Spirit / Run **unmeasured**, which reads
        // identically to broken).
        //
        // Running it in the battle rungs 1-6 already used is what turns rung
        // 8's opening `wait_for_command` into the standing regression test:
        // if an item use ever stops the turn pump again, the ladder loses two
        // rungs rather than silently restarting around it.
        if !wait_for_command(&mut session) {
            return Err(no_command(&session, "Summon"));
        }
        let carried = session
            .host
            .world
            .inventory
            .get(&ITEM_HEALING_LEAF)
            .copied()
            .unwrap_or(0);
        if !pick_command(&mut session, BattleCommand::Item) {
            return Err(format!(
                "Item arm unreachable ({})",
                surface_report(&session)
            ));
        }
        if session.host.world.battle_item_menu.is_none() {
            return Err(format!(
                "the Item arm opened no item menu ({})",
                surface_report(&session)
            ));
        }
        // The battle item menu maps only Up / Down / Cross / Circle - there is
        // no horizontal navigation and no page flip, so Cross is the whole
        // vocabulary for "use the seated row on the seated target".
        for _ in 0..12 {
            if session.host.world.battle_item_menu.is_none() {
                break;
            }
            tap(&mut session, PadButton::Cross);
        }
        if !settle_until(&mut session, SETTLE_TICKS, |s| {
            s.host
                .world
                .inventory
                .get(&ITEM_HEALING_LEAF)
                .copied()
                .unwrap_or(0)
                < carried
        }) {
            return Err(format!(
                "the item menu consumed nothing (carried {carried}, {})",
                surface_report(&session)
            ));
        }
        cleared.push("item");

        // --- rung 8: Spirit -----------------------------------------------
        // Spirit was reported as parking the battle the way Item did, and it
        // does not: measured on its own from a fresh fight, a Spirit guard
        // hands the pad the next command session inside ~230 ticks. It shares
        // Item's shape - it resolves without a strike - but it touches no
        // party HP, and party HP was the whole mechanism. So this rung runs in
        // the same fight as everything before it, and the `wait_for_command`
        // above it is the assertion that the Item rung did not park anything.
        if !wait_for_command(&mut session) {
            return Err(no_command(&session, "Item"));
        }
        let guard = session
            .host
            .world
            .battle_command
            .as_ref()
            .map(|s| s.actor)
            .unwrap_or(0);
        // Assert on the gauge Spirit actually charges.
        //
        // This is the repo's standing **two-AP-systems** trap and it bit here
        // first. `Spirit` charges `World::ap_gauges[slot]` (+5, via
        // `ApGauge::charge_spirit`) and raises `battle_guarding[slot]`, while
        // `World::spirit_gauge()` reads the *other* gauge entirely -
        // `actors[slot].battle.spirit_gauge`, the spirit-art meter. Watching
        // the wrong one reported a working Spirit as broken.
        //
        // Worse, it read plausibly in both directions: ordinary combat moves
        // the spirit-art meter by itself, so an earlier ordering of this
        // ladder *passed* this rung for a reason that had nothing to do with
        // the press. A rung that can pass without its own cause is not
        // measuring its command.
        let slot = guard as usize;
        let ap_before = session.host.world.ap_gauges[slot].current_ap;
        if !pick_command(&mut session, BattleCommand::Spirit) {
            return Err(format!(
                "Spirit arm unreachable ({})",
                surface_report(&session)
            ));
        }
        // Wait on the charge, not on the session closing: the command session
        // is consumed the moment Spirit is picked, frames before the SM
        // applies it.
        if !settle_until(&mut session, SETTLE_TICKS, |s| {
            s.host.world.ap_gauges[slot].current_ap > ap_before
        }) {
            return Err(format!(
                "Spirit charged no AP (stuck at {ap_before}, {})",
                surface_report(&session)
            ));
        }
        if !session.host.world.battle_guarding[slot] {
            return Err("Spirit charged AP but raised no guard stance".into());
        }
        cleared.push("spirit");

        // --- rung 9: Run refused under battle_no_escape ---------------------
        // Both Run rungs take a *fresh battle*, and here that really is a
        // convenience rather than a finding: `no_escape` is copied into the
        // session when it opens, so the flag has to be set before entering,
        // and rung 10 needs it back off. The prompt itself is not scarce -
        // every round boundary re-arms it (retail's `0x14`, re-entered from
        // the action SM's `0xFF` arm at `0x801E67E8`), so `wait_for_command`
        // now lands on a `RoundPrompt` mid-fight as readily as at battle open.
        //
        // What the fresh battle *does* still buy is the tightest possible read
        // of the opening frame: `at_round_prompt` is checked on the very frame
        // `battle_command` becomes `Some`, which is where the prompt used to
        // be missing.
        if !reset_to_field(&mut session) {
            return Err(format!(
                "could not restart the scene after the first battle ({})",
                surface_report(&session)
            ));
        }
        session.host.world.battle_no_escape = true;
        if !enter_battle(&mut session) {
            return Err(format!(
                "could not enter the no-escape battle ({})",
                surface_report(&session)
            ));
        }
        if !wait_for_command(&mut session) {
            return Err(no_command(&session, "entering the no-escape battle"));
        }
        if !at_round_prompt(&session) {
            return Err(format!(
                "a battle's opening session is not the round prompt ({})",
                surface_report(&session)
            ));
        }
        if session
            .host
            .world
            .battle_command
            .as_ref()
            .map(|s| BattleCommand::Run.available(s.no_escape))
            != Some(false)
        {
            return Err("battle_no_escape did not reach the command session".into());
        }
        tap(&mut session, PadButton::Circle);
        if session.host.world.mode != SceneMode::Battle {
            return Err("Run escaped a no-escape battle".into());
        }
        cleared.push("run-refused");

        // --- rung 10: Run ---------------------------------------------------
        // The same opening prompt, with the flag cleared.
        if !reset_to_field(&mut session) {
            return Err(format!(
                "could not restart the scene after the no-escape battle ({})",
                surface_report(&session)
            ));
        }
        session.host.world.battle_no_escape = false;
        if !enter_battle(&mut session) {
            return Err(format!(
                "could not enter the escapable battle ({})",
                surface_report(&session)
            ));
        }
        if !wait_for_command(&mut session) {
            return Err(no_command(&session, "entering the escapable battle"));
        }
        if !at_round_prompt(&session) {
            return Err(format!(
                "no round prompt to Run from ({})",
                surface_report(&session)
            ));
        }
        tap(&mut session, PadButton::Circle);
        if !settle_until(&mut session, SETTLE_TICKS, |s| {
            s.host.world.mode != SceneMode::Battle
        }) {
            return Err(format!(
                "Run never left the battle ({})",
                surface_report(&session)
            ));
        }
        cleared.push("run");
        Ok(())
    };

    if let Err(reason) = run() {
        stall = Some(reason);
    }

    let score = cleared.len();
    println!("battle-depth rungs cleared: {score} {cleared:?}");
    if let Some(reason) = &stall {
        println!("stalled at rung {}: {reason}", score + 1);
    }
    if score > BASELINE {
        println!("baseline can rise: const BASELINE: usize = {score};");
    }
    assert!(
        score >= BASELINE,
        "battle-depth ladder regressed: {score} < {BASELINE}. cleared={cleared:?}, stall={stall:?}"
    );
}
