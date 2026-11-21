//! The command prompts of `FUN_801D0748` (PROT 0898): the round prompt
//! `0x1E`, the ring `0x28`, the flee confirm `0x32`, the commit confirm
//! `0x6E` and the attack-mode prompt `0x78`, each run one pass over a
//! generated pad word, with the cross-button entry's highlight walk ahead of
//! them (`0x801D07FC..0x801D0B08`) on or off. The port is
//! `BattleCommandSession::walk_highlight` and `BattleCommandSession::input`
//! with the ring's `RingGates`, as `World::tick_battle_command` drives them:
//! where each surface goes, the highlight word `ctx[+0x880]`, the cursor cue
//! the walk keys, the refusal cue a refused ring arm keys, and the four marks
//! the ring draws ahead of the pad (`battle_ring_marks`).
//!
//! The window opener `FUN_801D388C` is hooked and its case read off `a0`:
//! retail leaves a surface by storing the next state and opening the next
//! window, and a step back or a Spirit commit opens a window with the state
//! byte left where it was.

use crate::common::{Rng, disc, scus_cpu, seat_status};
use legaia_engine_battle::target_picker::SlotState;
use legaia_engine_battle_menus::battle_input::{
    BattleCommand, BattleCommandInput, BattleCommandSession, CommandPhase, RingGates,
    ring_press_refused,
};
use legaia_engine_system::option_values::SelectAttackOpt;
use psx_retail_exec::{A0, Cpu, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const STATE_MACHINE: u32 = 0x801D_0748;
const OVERLAY_BASE: u32 = 0x801C_E818;
const OVERLAY_END: u32 = 0x8020_0000;
const CTX: u32 = 0x8012_0000;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8013_0000;
const STRIDE: u32 = 0x400;
const PAD: u32 = 0x8007_B874;
const PAD_HELD: u32 = 0x8007_B938;
const SELECT_ATTACK: u32 = 0x8008_46C4;
const CROSS_ENTRY: u32 = 0x8008_46C8;
const CONFIRM_MASK: u32 = 0x8008_46D0;
const CANCEL_MASK: u32 = 0x8008_46D4;
const SPECIAL_WORD: u32 = 0x8007_BAC0;
const BATTLE_ID: u32 = 0x8007_BD0C;
const WINDOW: u32 = 0x801D_388C;
const CUE: u32 = 0x8004_FCC8;
const FIRST_ABLE: u32 = 0x801D_BA04;
const NEXT_ABLE: u32 = 0x801D_B81C;
const CROSS_OUT: u32 = 0x801D_BC30;
const ROT_MARK: u32 = 0x801D_BD04;
const CURSE_MARK: u32 = 0x801D_BEC4;
const GLIDE: u32 = 0x801D_9BBC;
const RING_STEP: u32 = 0x801D_8D00;
const CONFIRM: u32 = 0x0040;
const CANCEL: u32 = 0x0020;
const LEFT: u32 = 0x8000;
const UP: u32 = 0x1000;
const RIGHT: u32 = 0x2000;
const DOWN: u32 = 0x4000;

/// Where one pass left the surface, read the same way off both sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Outcome {
    Stay,
    Ring,
    RoundPrompt,
    RunConfirm,
    RunAway,
    BeginRound,
    Reselect,
    Item,
    Spell,
    Arts,
    AttackMode,
    Target,
    StepBack,
    Spirit,
}

/// Retail's outcome from the state byte before and after and the window
/// cases the pass opened.
fn retail_outcome(before: u8, after: u8, windows: &[u32]) -> Outcome {
    let opened = |case: u32| windows.contains(&case);
    match (before, after) {
        (0x1E, 0x28) | (0x1E, 0x6E) => Outcome::Ring,
        (0x1E, 0x32) => Outcome::RunConfirm,
        (0x32, 0x1E) => Outcome::RoundPrompt,
        (0x32, 0xFE) => Outcome::RunAway,
        (0x6E, 0xFE) => Outcome::BeginRound,
        (0x6E, 0x1E) | (0x6E, 0x28) => Outcome::Reselect,
        (0x78, 0x28) => Outcome::Ring,
        (0x78, 0x50) | (0x28, 0x50) => Outcome::Arts,
        (0x78, 0x5A) | (0x28, 0x5A) => Outcome::Target,
        (0x28, 0x3C) => Outcome::Item,
        (0x28, 0x46) => Outcome::Spell,
        (0x28, 0x78) => Outcome::AttackMode,
        (0x28, 0x1E) => Outcome::StepBack,
        (0x28, 0x6E) => Outcome::Spirit,
        (0x28, 0x28) if opened(0x10) => Outcome::StepBack,
        (0x28, 0x28) if opened(0x11) => Outcome::Spirit,
        (b, a) if b == a && windows.is_empty() => Outcome::Stay,
        (b, a) => panic!("unmapped retail move {b:#x} -> {a:#x}, windows {windows:x?}"),
    }
}

/// The port's outcome from the phase it left and the phase it reached.
fn port_outcome(before: &CommandPhase, after: &CommandPhase) -> Outcome {
    if std::mem::discriminant(before) == std::mem::discriminant(after) {
        return Outcome::Stay;
    }
    match after {
        CommandPhase::Menu { .. } => Outcome::Ring,
        CommandPhase::RoundPrompt { .. } => Outcome::RoundPrompt,
        CommandPhase::RunConfirm { .. } => Outcome::RunConfirm,
        CommandPhase::RunAway => Outcome::RunAway,
        CommandPhase::BeginRound => Outcome::BeginRound,
        CommandPhase::Reselect => Outcome::Reselect,
        CommandPhase::OpenItemMenu => Outcome::Item,
        CommandPhase::OpenSpellMenu => Outcome::Spell,
        CommandPhase::OpenArtsMenu => Outcome::Arts,
        CommandPhase::AttackMode { .. } => Outcome::AttackMode,
        CommandPhase::Targeting {
            command: BattleCommand::Attack,
            ..
        } => Outcome::Target,
        CommandPhase::StepBack => Outcome::StepBack,
        CommandPhase::SpiritGuard => Outcome::Spirit,
        other => panic!("unmapped port phase {other:?}"),
    }
}

/// The highlight word of a lit arm, and back: the ring's arms are Item (Up),
/// Attack (Left), magic (Right), Spirit (Down); a pair prompt's chips are
/// Left and Right.
fn highlight_word(ring: bool, cursor: u8) -> u32 {
    match (ring, cursor) {
        (true, 0) => UP,
        (true, 1) | (false, 0) => LEFT,
        (true, 2) | (false, 1) => RIGHT,
        _ => DOWN,
    }
}

fn session_at(state: u8, cursor: u8) -> BattleCommandSession {
    let mut s = BattleCommandSession::new(0, 0);
    s.phase = match state {
        0x1E => CommandPhase::RoundPrompt { cursor },
        0x28 => CommandPhase::Menu { cursor },
        0x32 => CommandPhase::RunConfirm { cursor },
        0x6E => CommandPhase::CommitConfirm { cursor },
        _ => CommandPhase::AttackMode { cursor },
    };
    s
}

/// A recorder of one hooked leaf's `a0`s.
type Log = Rc<RefCell<Vec<u32>>>;

fn retail_cpu() -> Option<(Cpu, Log, Log, Log)> {
    let (mut cpu, d) = (scus_cpu()?, disc()?);
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(898).expect("PROT 0898"));
    cpu.stub_outside = Some((OVERLAY_BASE, OVERLAY_END));
    for leaf in [
        0x801D_3444u32, // the frame's HUD pass
        0x801D_B8F4,    // the chip draw
        0x801D_B8B4,    // the target re-pick
        0x801D_A34C,
        0x801D_8A88,
        0x801D_A6B4,
        0x801D_BB8C,
    ] {
        cpu.stub(leaf, 3);
    }
    // The window glide has settled.
    cpu.stub(GLIDE, 0);
    let windows = Rc::new(RefCell::new(Vec::new()));
    let w2 = windows.clone();
    cpu.hook(WINDOW, move |r, _| w2.borrow_mut().push(r.r[A0]));
    let cues = Rc::new(RefCell::new(Vec::new()));
    let c2 = cues.clone();
    cpu.hook(CUE, move |r, _| c2.borrow_mut().push(r.r[A0]));
    let marks = Rc::new(RefCell::new(Vec::new()));
    for (leaf, tag) in [(CROSS_OUT, 0u32), (ROT_MARK, 1), (CURSE_MARK, 2)] {
        let m2 = marks.clone();
        cpu.hook(leaf, move |r, _| m2.borrow_mut().push(tag << 16 | r.r[A0]));
    }
    Some((cpu, windows, cues, marks))
}

#[test]
fn the_command_prompts_go_where_retail_goes() {
    differential!(prot(898, STATE_MACHINE));
    let Some((mut cpu, windows, cues, marks)) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5052_4F4D);
    let mut seen = std::collections::HashMap::<(u8, Outcome), u32>::new();
    let (mut moved_n, mut refused_n) = (0, 0);
    for round in 0..6000 {
        let state = [0x1E, 0x28, 0x28, 0x28, 0x32, 0x6E, 0x78][rng.below(7) as usize];
        let ring = state == 0x28;
        let cross_entry = rng.below(2) == 1;
        let party = 1 + rng.below(3) as u8;
        let seat = rng.below(u32::from(party)) as u8;
        let order = rng.below(2) as u8;
        let status: u16 = [0, 0, 0x38, 0x18, 0x1000, 0x1038, 0x0100][rng.below(7) as usize];
        let special: u32 = [0, 0, 0x100, 0x200, 0x300][rng.below(5) as usize];
        let raseru = rng.below(4) != 0;
        let opt = rng.below(3);
        let cursor = rng.below(if ring { 4 } else { 2 }) as u8;
        // Who is left to command: the first able member, and the next one
        // after this member - either the party count (nobody) or a seat.
        let first_able = if rng.below(3) == 0 { party } else { 0 };
        let next_able = if rng.below(2) == 0 {
            party
        } else {
            seat.min(party - 1)
        };
        let dirs = [
            0,
            LEFT,
            RIGHT,
            UP,
            DOWN,
            LEFT | UP,
            LEFT | DOWN,
            RIGHT | UP,
            RIGHT | DOWN,
        ];
        let mut pad = dirs[rng.below(dirs.len() as u32) as usize];
        if rng.below(3) == 0 {
            pad |= CONFIRM;
        }
        if rng.below(5) == 0 {
            pad |= CANCEL;
        }
        if rng.below(30) == 0 {
            pad = LEFT | RIGHT | (rng.below(2) * CONFIRM);
        }

        // Retail.
        cpu.mem.wr32(CTX_PTR, CTX);
        cpu.mem.load(CTX, &[0u8; 0x1400]);
        for i in 0..3 {
            cpu.mem.wr32(ACTORS + 4 * i, ACTOR + STRIDE * i);
            cpu.mem.load(ACTOR + STRIDE * i, &[0u8; STRIDE as usize]);
        }
        cpu.mem.wr8(CTX, party);
        cpu.mem.wr8(CTX + 0x06, state);
        cpu.mem.wr8(CTX + 0x13, seat);
        cpu.mem.wr8(CTX + 0x1F, order);
        cpu.mem.wr8(CTX + 0x25F + u32::from(seat), u8::from(raseru));
        cpu.mem.wr8(CTX + 0x275, if ring { 4 } else { 2 });
        let hl = highlight_word(ring, cursor);
        cpu.mem.wr32(CTX + 0x880, hl);
        cpu.mem
            .wr16(ACTOR + STRIDE * u32::from(seat) + 0x16E, status);
        cpu.mem.wr32(PAD, pad);
        cpu.mem.wr32(PAD_HELD, 0);
        cpu.mem.wr32(SELECT_ATTACK, opt);
        cpu.mem.wr32(CROSS_ENTRY, u32::from(cross_entry));
        cpu.mem.wr32(CONFIRM_MASK, CONFIRM);
        cpu.mem.wr32(CANCEL_MASK, CANCEL);
        cpu.mem.wr32(SPECIAL_WORD, special);
        cpu.mem.wr8(BATTLE_ID, 0);
        cpu.stub(FIRST_ABLE, u32::from(first_able));
        cpu.stub(NEXT_ABLE, u32::from(next_able));
        windows.borrow_mut().clear();
        cues.borrow_mut().clear();
        marks.borrow_mut().clear();
        assert_eq!(
            cpu.call(STATE_MACHINE, &[]),
            Stop::Returned,
            "round {round}"
        );
        let after = cpu.mem.rd8(CTX + 0x06);
        let win = windows.borrow().clone();
        let cue = cues.borrow().clone();
        let retail = retail_outcome(state, after, &win);

        // The port.
        let rest = if cross_entry {
            cursor
        } else if ring {
            1
        } else {
            0
        };
        let mut session = session_at(state, rest);
        let mut ev = BattleCommandInput {
            up: pad & UP != 0,
            down: pad & DOWN != 0,
            left: pad & LEFT != 0,
            right: pad & RIGHT != 0,
            cross: pad & CONFIRM != 0,
            circle: pad & CANCEL != 0,
            select_attack: match opt {
                0 => SelectAttackOpt::Select,
                1 => SelectAttackOpt::Automatic,
                _ => SelectAttackOpt::Command,
            },
            gates: RingGates::default(),
            cross_entry,
        };
        let moved = cross_entry && session.walk_highlight(&mut ev);
        let mut refused = false;
        if let CommandPhase::Menu { cursor } = session.phase {
            ev.gates = RingGates {
                status,
                no_raseru: !raseru,
                items_forbidden: special & 0x100 != 0,
                magic_forbidden: special & 0x200 != 0,
            };
            refused = ring_press_refused(cursor, &ev);
        }
        let before_phase = session.phase.clone();
        let alive = SlotState::alive(true, true).with_seat(0x100, 0x100);
        let monsters = [
            alive.with_seat(-0x200, 0x300),
            alive.with_seat(0, 0x400),
            alive.with_seat(0x200, 0x300),
            SlotState::dead(false),
            SlotState::dead(false),
        ];
        session.input(ev, [alive; 3], monsters);
        let port = port_outcome(&before_phase, &session.phase);

        let label = format!(
            "round {round}: state {state:#x} pad {pad:#x} cross-entry {cross_entry} hl {hl:#x} \
             party {party} seat {seat} order {order} status {status:#x} special {special:#x} \
             raseru {raseru} opt {opt} windows {win:x?} cues {cue:x?} -> {after:#x}"
        );
        assert_eq!(retail, port, "outcome: {label}");
        assert_eq!(cue.contains(&0x22), moved, "cursor cue: {label}");
        if moved {
            moved_n += 1;
        }
        if ring && after == 0x28 && win.is_empty() && cue.contains(&0x23) {
            refused_n += 1;
            assert!(refused, "refusal: {label}");
        } else {
            assert!(!refused, "refusal: {label}");
        }
        if cross_entry && retail == Outcome::Stay {
            let lit = match &session.phase {
                CommandPhase::Menu { cursor }
                | CommandPhase::RoundPrompt { cursor }
                | CommandPhase::RunConfirm { cursor }
                | CommandPhase::CommitConfirm { cursor }
                | CommandPhase::AttackMode { cursor } => *cursor,
                other => panic!("{other:?}"),
            };
            assert_eq!(
                cpu.mem.rd32(CTX + 0x880),
                highlight_word(ring, lit),
                "highlight: {label}"
            );
        }
        if ring && pad & CANCEL == 0 {
            let mut w = legaia_engine_core::world::World::new();
            w.mode = legaia_engine_mode::mode::SceneMode::Battle;
            w.battle.special_word = special;
            w.battle.command = Some(session_at(0x28, 1));
            seat_status(&mut w, 0, status, false);
            let m = legaia_engine_battle_hud::battle_ring_marks(&w);
            let drawn = marks.borrow().clone();
            let port_marks: Vec<u32> = [
                (m.item_forbidden, 0xCC),
                (m.raseru_forbidden, 0xF8),
                (m.attack_rotted, 1 << 16 | 0xA0),
                (m.magic_cursed, 2 << 16 | 0xF8),
            ]
            .into_iter()
            .filter_map(|(on, tag)| on.then_some(tag))
            .collect();
            assert_eq!(drawn, port_marks, "marks: {label}");
        }
        *seen.entry((state, retail)).or_default() += 1;
    }
    for (state, out) in [
        (0x1E, Outcome::Ring),
        (0x1E, Outcome::RunConfirm),
        (0x1E, Outcome::Stay),
        (0x32, Outcome::RoundPrompt),
        (0x32, Outcome::RunAway),
        (0x6E, Outcome::BeginRound),
        (0x6E, Outcome::Reselect),
        (0x78, Outcome::Ring),
        (0x78, Outcome::Arts),
        (0x78, Outcome::Target),
        (0x28, Outcome::Item),
        (0x28, Outcome::Spell),
        (0x28, Outcome::AttackMode),
        (0x28, Outcome::Target),
        (0x28, Outcome::Arts),
        (0x28, Outcome::StepBack),
        (0x28, Outcome::Spirit),
    ] {
        assert!(
            seen.get(&(state, out)).copied().unwrap_or(0) > 10,
            "{state:#x} {out:?}: {seen:?}"
        );
    }
    assert!(moved_n > 200 && refused_n > 100, "{moved_n} {refused_n}");
}

/// The attack target cursor `0x5A` (`0x801D21CC..0x801D23EC`), with the ring
/// step `FUN_801D8D00` run for real over a ring seeded from the port's builder
/// (`build_attack_target_queue`, held to `FUN_801D8A88` by `battle_leaves`),
/// against the session's `Targeting` phase: Right and Left step the ring,
/// Up or Down commit under the directional entry, the confirm mask commits,
/// and the cancel mask goes back where the Attack arm came from.
#[test]
fn the_attack_target_cursor_steps_and_commits_as_retail() {
    differential!(prot(898, STATE_MACHINE), prot(898, RING_STEP));
    use legaia_engine_battle::target_picker::{
        CursorRow, PickerState, TargetKind, TargetPickerSession,
    };
    use legaia_engine_battle_vm::battle_action::{
        PoolActor, bearing_12bit_approx, build_attack_target_queue,
    };
    let Some((mut cpu, windows, cues, _)) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // The step runs nested; the ledger notes it as it is entered.
    cpu.watched.insert(RING_STEP);
    let mut rng = Rng(0x5441_5247);
    let mut seen = std::collections::HashMap::<&str, u32>::new();
    for round in 0..6000 {
        let party = 1 + rng.below(3) as u8;
        let seat = rng.below(u32::from(party)) as u8;
        let count = 1 + rng.below(4) as usize;
        let cross_entry = rng.below(2) == 1;
        let automatic = rng.below(2) == 1;
        let mut alive = [false; 4];
        for a in alive.iter_mut().take(count) {
            *a = rng.below(4) != 0;
        }
        let live: Vec<u8> = (0..count as u8)
            .filter(|&i| alive[usize::from(i)])
            .collect();
        if live.is_empty() {
            continue;
        }
        let cur = live[rng.below(live.len() as u32) as usize];
        let span = 0x800;
        let mut pos = || {
            (
                (rng.below(span * 2) as i32 - span as i32) as i16,
                (rng.below(span * 2) as i32 - span as i32) as i16,
            )
        };
        let me = pos();
        let monster_pos: [(i16, i16); 4] = std::array::from_fn(|_| pos());
        let dirs = [
            0,
            LEFT,
            RIGHT,
            UP,
            DOWN,
            LEFT | UP,
            RIGHT | DOWN,
            LEFT | DOWN,
        ];
        let mut pad = dirs[rng.below(dirs.len() as u32) as usize];
        if rng.below(3) == 0 {
            pad |= CONFIRM;
        }
        if rng.below(4) == 0 {
            pad |= CANCEL;
        }

        // The ring as the state's entry built it.
        let mut pool = [PoolActor::default(); 8];
        pool[0] = PoolActor {
            alive: true,
            x: me.0,
            z: me.1,
            target_slot: 3 + cur,
            status: 0,
        };
        for i in 0..4 {
            pool[3 + i] = PoolActor {
                alive: alive[i],
                x: monster_pos[i].0,
                z: monster_pos[i].1,
                target_slot: 0,
                status: 0,
            };
        }
        let q = build_attack_target_queue(&pool, 0, bearing_12bit_approx);

        // Retail.
        cpu.mem.wr32(CTX_PTR, CTX);
        cpu.mem.load(CTX, &[0u8; 0x1400]);
        for i in 0..8 {
            cpu.mem.wr32(ACTORS + 4 * i, ACTOR + STRIDE * i);
            cpu.mem.load(ACTOR + STRIDE * i, &[0u8; STRIDE as usize]);
        }
        let actor = ACTOR + STRIDE * u32::from(seat);
        cpu.mem.wr8(CTX, party);
        cpu.mem.wr8(CTX + 0x06, 0x5A);
        cpu.mem.wr8(CTX + 0x13, seat);
        cpu.mem.wr8(CTX + 0x266 + u32::from(seat), 1);
        for (i, b) in [
            q.count,
            q.wrap_slot,
            q.ordered[0],
            q.ordered[1],
            q.ordered[2],
        ]
        .iter()
        .enumerate()
        {
            cpu.mem.wr8(CTX + 0x244 + i as u32, *b);
        }
        cpu.mem.wr8(actor + 0x1DD, 3 + cur);
        cpu.mem.wr32(PAD, pad);
        cpu.mem.wr32(PAD_HELD, 0);
        cpu.mem.wr32(SELECT_ATTACK, u32::from(automatic));
        cpu.mem.wr32(CROSS_ENTRY, u32::from(cross_entry));
        cpu.mem.wr32(CONFIRM_MASK, CONFIRM);
        cpu.mem.wr32(CANCEL_MASK, CANCEL);
        cpu.mem.wr8(BATTLE_ID, 0);
        cpu.stub(NEXT_ABLE, u32::from(party));
        windows.borrow_mut().clear();
        cues.borrow_mut().clear();
        assert_eq!(
            cpu.call(STATE_MACHINE, &[]),
            Stop::Returned,
            "round {round}"
        );
        let after = cpu.mem.rd8(CTX + 0x06);
        let target = cpu.mem.rd8(actor + 0x1DD);

        // The port.
        let dead = SlotState::dead(false);
        let party_rows = [SlotState::alive(true, true).with_seat(me.0, me.1); 3];
        let mut monsters = [dead; 5];
        for i in 0..count {
            monsters[i] =
                SlotState::alive(true, alive[i]).with_seat(monster_pos[i].0, monster_pos[i].1);
        }
        let mut picker = TargetPickerSession::new(TargetKind::SingleEnemy, 0, party_rows, monsters);
        assert!(picker.seat_cursor(CursorRow::Enemy, cur), "round {round}");
        let mut session = BattleCommandSession::new(0, 0);
        session.phase = CommandPhase::Targeting {
            command: BattleCommand::Attack,
            picker,
        };
        let ev = BattleCommandInput {
            up: pad & UP != 0,
            down: pad & DOWN != 0,
            left: pad & LEFT != 0,
            right: pad & RIGHT != 0,
            cross: pad & CONFIRM != 0,
            circle: pad & CANCEL != 0,
            select_attack: if automatic {
                SelectAttackOpt::Automatic
            } else {
                SelectAttackOpt::Select
            },
            gates: RingGates::default(),
            cross_entry,
        };
        session.input(ev, party_rows, monsters);
        let label = format!(
            "round {round}: pad {pad:#x} cross-entry {cross_entry} automatic {automatic} \
             cur {cur} alive {alive:?} ring {:?} windows {:x?} cues {:x?} -> {after:#x} target {target}",
            [
                q.count,
                q.wrap_slot,
                q.ordered[0],
                q.ordered[1],
                q.ordered[2]
            ],
            windows.borrow(),
            cues.borrow()
        );
        let outcome = match (&session.phase, after) {
            (CommandPhase::Targeting { picker, .. }, 0x5A) => {
                let PickerState::Cursor { slot, .. } = picker.state() else {
                    panic!("{label}");
                };
                assert_eq!(target, 3 + slot, "cursor: {label}");
                assert_eq!(
                    cues.borrow().contains(&0x22),
                    slot != cur,
                    "step cue: {label}"
                );
                if slot == cur { "stay" } else { "step" }
            }
            (
                CommandPhase::Confirmed {
                    target_row: CursorRow::Enemy,
                    target_slot,
                    ..
                },
                0x28 | 0x6E,
            ) => {
                assert_eq!(target, 3 + target_slot, "commit target: {label}");
                "commit"
            }
            (CommandPhase::Menu { .. }, 0x28) => "cancel to ring",
            (CommandPhase::AttackMode { .. }, 0x78) => "cancel to prompt",
            (phase, after) => panic!("port {phase:?} retail {after:#x}: {label}"),
        };
        *seen.entry(outcome).or_default() += 1;
    }
    for o in [
        "stay",
        "step",
        "commit",
        "cancel to ring",
        "cancel to prompt",
    ] {
        assert!(seen.get(o).copied().unwrap_or(0) > 30, "{o}: {seen:?}");
    }
}

/// The magic window's target sub-cursors (`0x801D2590..0x801D3020`): the
/// one-enemy cursor `0x5C`, the all-enemies confirm `0x5E`, the one-ally
/// cursor `0x65` and the all-allies confirm `0x67`, against
/// `BattleSpellSession`'s `Targeting` and `SweepConfirm` phases. The enemy
/// ring steps through `FUN_801D8D00` run for real, as on `0x5A`.
#[test]
fn the_spell_target_cursors_go_where_retail_goes() {
    differential!(prot(898, STATE_MACHINE), prot(898, RING_STEP));
    use legaia_engine_battle::battle_magic::{BattleSpellInput, BattleSpellSession, SpellPhase};
    use legaia_engine_battle::spells::SpellCatalog;
    use legaia_engine_battle::target_picker::{
        CursorRow, PickerState, TargetKind, TargetPickerSession,
    };
    use legaia_engine_battle_vm::battle_action::{
        PoolActor, bearing_12bit_approx, build_attack_target_queue,
    };
    let Some((mut cpu, windows, cues, _)) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.watched.insert(RING_STEP);
    let catalog = SpellCatalog::vanilla();
    let mut rng = Rng(0x5350_454C);
    let mut seen = std::collections::HashMap::<(u8, &str), u32>::new();
    for round in 0..8000 {
        let state = [0x5C, 0x5E, 0x65, 0x67][rng.below(4) as usize];
        let party = 1 + rng.below(3) as u8;
        let seat = rng.below(u32::from(party)) as u8;
        let count = 1 + rng.below(4) as usize;
        let cross_entry = rng.below(2) == 1;
        let member_alive: [bool; 3] =
            std::array::from_fn(|i| i == usize::from(seat) || rng.below(3) != 0);
        let mut alive = [false; 4];
        for a in alive.iter_mut().take(count) {
            *a = rng.below(4) != 0;
        }
        let live: Vec<u8> = (0..count as u8)
            .filter(|&i| alive[usize::from(i)])
            .collect();
        if live.is_empty() {
            continue;
        }
        let cur = live[rng.below(live.len() as u32) as usize];
        let span = 0x800;
        let mut pos = || {
            (
                (rng.below(span * 2) as i32 - span as i32) as i16,
                (rng.below(span * 2) as i32 - span as i32) as i16,
            )
        };
        let me = pos();
        let monster_pos: [(i16, i16); 4] = std::array::from_fn(|_| pos());
        let dirs = [
            0,
            LEFT,
            RIGHT,
            UP,
            DOWN,
            LEFT | UP,
            RIGHT | DOWN,
            LEFT | DOWN,
        ];
        let mut pad = dirs[rng.below(dirs.len() as u32) as usize];
        if rng.below(3) == 0 {
            pad |= CONFIRM;
        }
        if rng.below(4) == 0 {
            pad |= CANCEL;
        }
        let mut pool = [PoolActor::default(); 8];
        pool[0] = PoolActor {
            alive: true,
            x: me.0,
            z: me.1,
            target_slot: 3 + cur,
            status: 0,
        };
        for i in 0..4 {
            pool[3 + i] = PoolActor {
                alive: alive[i],
                x: monster_pos[i].0,
                z: monster_pos[i].1,
                target_slot: 0,
                status: 0,
            };
        }
        let q = build_attack_target_queue(&pool, 0, bearing_12bit_approx);

        // Retail.
        cpu.mem.wr32(CTX_PTR, CTX);
        cpu.mem.load(CTX, &[0u8; 0x1400]);
        for i in 0..8 {
            cpu.mem.wr32(ACTORS + 4 * i, ACTOR + STRIDE * i);
            cpu.mem.load(ACTOR + STRIDE * i, &[0u8; STRIDE as usize]);
        }
        let actor = ACTOR + STRIDE * u32::from(seat);
        cpu.mem.wr8(CTX, party);
        cpu.mem.wr8(CTX + 0x06, state);
        cpu.mem.wr8(CTX + 0x13, seat);
        for (i, b) in [
            q.count,
            q.wrap_slot,
            q.ordered[0],
            q.ordered[1],
            q.ordered[2],
        ]
        .iter()
        .enumerate()
        {
            cpu.mem.wr8(CTX + 0x244 + i as u32, *b);
        }
        let start = match state {
            0x5C => 3 + cur,
            0x65 => seat,
            0x5E => 9,
            _ => 8,
        };
        cpu.mem.wr8(actor + 0x1DD, start);
        cpu.mem.wr32(PAD, pad);
        cpu.mem.wr32(PAD_HELD, 0);
        cpu.mem.wr32(CROSS_ENTRY, u32::from(cross_entry));
        cpu.mem.wr32(CONFIRM_MASK, CONFIRM);
        cpu.mem.wr32(CANCEL_MASK, CANCEL);
        cpu.mem.wr8(BATTLE_ID, 0);
        cpu.stub(NEXT_ABLE, u32::from(party));
        windows.borrow_mut().clear();
        cues.borrow_mut().clear();
        assert_eq!(
            cpu.call(STATE_MACHINE, &[]),
            Stop::Returned,
            "round {round}"
        );
        let after = cpu.mem.rd8(CTX + 0x06);
        let target = cpu.mem.rd8(actor + 0x1DD);

        // The port.
        let party_rows: [SlotState; 3] = std::array::from_fn(|i| {
            if i < usize::from(party) {
                SlotState::alive(true, member_alive[i]).with_seat(me.0, me.1)
            } else {
                SlotState::dead(false)
            }
        });
        let mut monsters = [SlotState::dead(false); 5];
        for i in 0..count {
            monsters[i] =
                SlotState::alive(true, alive[i]).with_seat(monster_pos[i].0, monster_pos[i].1);
        }
        let mut session = BattleSpellSession::new(seat, seat, &[], &catalog, 0, 0);
        session.phase = match state {
            0x5C => {
                let mut p =
                    TargetPickerSession::new(TargetKind::SingleEnemy, 0, party_rows, monsters);
                assert!(p.seat_cursor(CursorRow::Enemy, cur), "round {round}");
                SpellPhase::Targeting {
                    spell_id: 0x20,
                    picker: p,
                }
            }
            0x65 => SpellPhase::Targeting {
                spell_id: 0x10,
                picker: TargetPickerSession::new(
                    TargetKind::SingleAllyOrSelf,
                    seat,
                    party_rows,
                    monsters,
                ),
            },
            0x5E => SpellPhase::SweepConfirm {
                spell_id: 0x20,
                row: CursorRow::Enemy,
            },
            _ => SpellPhase::SweepConfirm {
                spell_id: 0x10,
                row: CursorRow::Ally,
            },
        };
        let ev = BattleSpellInput {
            up: pad & UP != 0,
            down: pad & DOWN != 0,
            left: pad & LEFT != 0,
            right: pad & RIGHT != 0,
            cross: pad & CONFIRM != 0,
            circle: pad & CANCEL != 0,
            cross_entry,
        };
        session.input(ev, &catalog, party_rows, monsters);
        let label = format!(
            "round {round}: state {state:#x} pad {pad:#x} cross-entry {cross_entry} party {party} \
             seat {seat} members {member_alive:?} cur {cur} alive {alive:?} windows {:x?} cues {:x?} \
             -> {after:#x} target {target}",
            windows.borrow(),
            cues.borrow()
        );
        let outcome = match (&session.phase, after) {
            (SpellPhase::Targeting { picker, .. }, s) if s == state => {
                let PickerState::Cursor { row, slot } = picker.state() else {
                    panic!("{label}");
                };
                let want = if row == CursorRow::Enemy {
                    3 + slot
                } else {
                    slot
                };
                assert_eq!(target, want, "cursor: {label}");
                if target == start { "stay" } else { "step" }
            }
            (SpellPhase::SweepConfirm { .. }, s) if s == state => "stay",
            (
                SpellPhase::Confirmed {
                    target_row,
                    target_slot,
                    ..
                },
                0x28 | 0x6E,
            ) => {
                let want = match (state, target_row) {
                    (0x5E | 0x67, _) => start,
                    (_, CursorRow::Enemy) => 3 + target_slot,
                    (_, CursorRow::Ally) => *target_slot,
                };
                assert_eq!(target, want, "commit target: {label}");
                "commit"
            }
            (SpellPhase::Select { .. }, 0x46) => "cancel",
            (phase, after) => panic!("port {phase:?} retail {after:#x}: {label}"),
        };
        *seen.entry((state, outcome)).or_default() += 1;
    }
    for key in [
        (0x5C, "stay"),
        (0x5C, "step"),
        (0x5C, "commit"),
        (0x5C, "cancel"),
        (0x65, "step"),
        (0x65, "commit"),
        (0x65, "cancel"),
        (0x5E, "stay"),
        (0x5E, "commit"),
        (0x5E, "cancel"),
        (0x67, "commit"),
        (0x67, "cancel"),
    ] {
        assert!(
            seen.get(&key).copied().unwrap_or(0) > 20,
            "{key:?}: {seen:?}"
        );
    }
}

/// State `0xFE`, the armed round (`0x801D31E8..0x801D328C`): the flow parks
/// on `0xFF` with the action SM restarted at `0x00`, and a Run committed in a
/// special battle forfeits the leg (`ctx[+0x274] = 0`, `_DAT_80084448 = 4`)
/// unless the first monster is one of the four exempt ids, against
/// `special_battle_run_forfeit`.
#[test]
fn the_armed_round_forfeits_a_special_battle_run_as_retail() {
    differential!(prot(898, STATE_MACHINE));
    use legaia_engine_battle_vm::battle_formulas::special_battle_run_forfeit;
    let Some((mut cpu, _, _, _)) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const LEG_OUTCOME: u32 = 0x8008_4448;
    cpu.stub(0x801D_99BC, 0);
    cpu.stub(0x801D_8DE8, 0);
    let mut rng = Rng(0x4152_4D44);
    let mut forfeits = 0;
    for round in 0..3000 {
        let word: u32 = [0, 0, 0x100, 0x200, 0x300][rng.below(5) as usize];
        let category = [0u8, 3, 5, 5][rng.below(4) as usize];
        let first = [0xAF, 0x3D, 0x3E, 0x3F, 0x40, 0x3C, 0x01, 0xB6][rng.below(8) as usize];
        cpu.mem.wr32(CTX_PTR, CTX);
        cpu.mem.load(CTX, &[0u8; 0x1400]);
        for i in 0..3 {
            cpu.mem.wr32(ACTORS + 4 * i, ACTOR + STRIDE * i);
            cpu.mem.load(ACTOR + STRIDE * i, &[0u8; STRIDE as usize]);
        }
        cpu.mem.wr8(CTX, 1);
        cpu.mem.wr8(CTX + 0x06, 0xFE);
        cpu.mem.wr8(CTX + 0x07, 0x3C);
        cpu.mem.wr8(CTX + 0x274, 7);
        cpu.mem.wr8(CTX + 0x290, rng.below(2) as u8);
        cpu.mem.wr8(ACTOR + 0x1DE, category);
        cpu.mem.wr32(SPECIAL_WORD, word);
        cpu.mem.wr8(BATTLE_ID, first);
        cpu.mem.wr32(LEG_OUTCOME, 0);
        cpu.mem.wr32(PAD, 0);
        cpu.mem.wr32(PAD_HELD, 0);
        cpu.mem.wr32(CROSS_ENTRY, 0);
        assert_eq!(
            cpu.call(STATE_MACHINE, &[]),
            Stop::Returned,
            "round {round}"
        );
        let label = format!("round {round}: word {word:#x} category {category} first {first:#x}");
        assert_eq!(cpu.mem.rd8(CTX + 0x06), 0xFF, "{label}");
        assert_eq!(cpu.mem.rd8(CTX + 0x07), 0, "{label}");
        let retail = cpu.mem.rd8(CTX + 0x274) == 0 && cpu.mem.rd32(LEG_OUTCOME) == 4;
        assert_eq!(
            retail,
            special_battle_run_forfeit(word, category, first),
            "{label}"
        );
        forfeits += u32::from(retail);
    }
    assert!(forfeits > 100, "{forfeits}");
}

/// The pre-dispatch glide gate (`0x801D0B2C..0x801D0C58`) against
/// `World::battle_command_glide_gate` and `World::take_command_pad_buffer`:
/// a pass leaves while the glide stepper `FUN_801D9BBC` returns non-zero,
/// parks a press made in its last five passes under the directional entry,
/// spends one more pass clearing the latch in a staged battle, and the first
/// dispatched pass reads the parked press when it has none of its own.
#[test]
fn the_glide_gate_holds_and_parks_as_retail() {
    differential!(prot(898, STATE_MACHINE));
    use legaia_engine_core::world::World;
    use legaia_engine_core::world::test_drivers::BattleTestDrivers;
    use legaia_engine_system::option_values::BattleCommandOpt;
    use legaia_engine_system::pad::PadButton;
    let Some((mut cpu, _, _, _)) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const STAGE_ID: u32 = 0x8007_B64A;
    // The stepper's return, and the pad word the dispatched pass reads
    // (`s2` as the round prompt's chip draw is called).
    let ret = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let r2 = ret.clone();
    cpu.hook(GLIDE, move |r, _| r.r[psx_retail_exec::V0] = r2.get());
    let dispatched = std::rc::Rc::new(std::cell::Cell::new(None::<u32>));
    let d2 = dispatched.clone();
    cpu.hook(0x801D_B8F4, move |r, _| d2.set(Some(r.r[18])));
    let packed = [
        (LEFT, PadButton::Left),
        (RIGHT, PadButton::Right),
        (UP, PadButton::Up),
        (DOWN, PadButton::Down),
        (CONFIRM, PadButton::Cross),
        (CANCEL, PadButton::Circle),
    ];
    let to_input = |word: u32| BattleCommandInput {
        left: word & LEFT != 0,
        right: word & RIGHT != 0,
        up: word & UP != 0,
        down: word & DOWN != 0,
        cross: word & CONFIRM != 0,
        circle: word & CANCEL != 0,
        ..Default::default()
    };
    let mut rng = Rng(0x474C_4944);
    let (mut gated, mut parked, mut latched, mut consumed) = (0, 0, 0, 0);
    for round in 0..4000 {
        let r = [0u32, 0, 0, 1, 3, 5, 6, 9][rng.below(8) as usize];
        let latch = [0u8, 0, 2, 5][rng.below(4) as usize];
        let stage = [0u8, 0, 3][rng.below(3) as usize];
        let cross_entry = rng.below(4) == 0;
        let pad = [0, 0, LEFT, RIGHT, CONFIRM, UP | CONFIRM][rng.below(6) as usize];
        let park = [0, 0, LEFT, CONFIRM][rng.below(4) as usize];

        cpu.mem.wr32(CTX_PTR, CTX);
        cpu.mem.load(CTX, &[0u8; 0x1400]);
        for i in 0..3 {
            cpu.mem.wr32(ACTORS + 4 * i, ACTOR + STRIDE * i);
            cpu.mem.load(ACTOR + STRIDE * i, &[0u8; STRIDE as usize]);
        }
        cpu.mem.wr8(CTX, 1);
        cpu.mem.wr8(CTX + 0x06, 0x1E);
        cpu.mem.wr16(CTX + 0x6B2, u16::from(latch));
        cpu.mem.wr32(CTX + 0x884, park);
        cpu.mem.wr8(CTX + 0x275, 2);
        cpu.mem.wr32(CTX + 0x880, LEFT);
        cpu.mem.wr8(STAGE_ID, stage);
        cpu.mem.wr32(PAD, pad);
        cpu.mem.wr32(PAD_HELD, 0);
        cpu.mem.wr32(CROSS_ENTRY, u32::from(cross_entry));
        cpu.mem.wr32(CONFIRM_MASK, CONFIRM);
        cpu.mem.wr32(CANCEL_MASK, CANCEL);
        cpu.mem.wr8(BATTLE_ID, 0);
        ret.set(r);
        dispatched.set(None);
        assert_eq!(
            cpu.call(STATE_MACHINE, &[]),
            Stop::Returned,
            "round {round}"
        );
        let retail_gated = dispatched.get().is_none();
        let retail_park = cpu.mem.rd32(CTX + 0x884);
        let retail_latch = cpu.mem.rd16(CTX + 0x6B2);

        let mut w = World::new();
        w.toggles.battle_command = if cross_entry {
            BattleCommandOpt::CrossButton
        } else {
            BattleCommandOpt::DirectionalButtons
        };
        w.battle.widget_glide_return = r as u8;
        w.battle.widget_glide_latch = latch;
        w.battle.stage_id = stage;
        w.battle.command_pad_buffer = (park != 0).then(|| to_input(park));
        let raw = packed
            .iter()
            .filter(|(p, _)| pad & p != 0)
            .fold(0u16, |m, (_, b)| m | b.mask());
        w.input.set_pad(raw);
        let port_gated = w.battle_command_glide_gate();
        let label = format!(
            "round {round}: ret {r} latch {latch} stage {stage} cross-entry {cross_entry} \
             pad {pad:#x} park {park:#x} -> gated {retail_gated} park {retail_park:#x} latch {retail_latch}"
        );
        assert_eq!(retail_gated, port_gated, "gate: {label}");
        if retail_gated {
            gated += 1;
            let port_park = w.battle.command_pad_buffer;
            let want = (retail_park != 0).then(|| to_input(retail_park));
            assert_eq!(
                port_park.map(|p| (p.left, p.right, p.up, p.down, p.cross, p.circle)),
                want.map(|p| (p.left, p.right, p.up, p.down, p.cross, p.circle)),
                "park: {label}"
            );
            parked += u32::from(retail_park != park);
            if stage != 0 {
                assert_eq!(
                    u16::from(w.battle.widget_glide_latch),
                    retail_latch,
                    "latch: {label}"
                );
            }
            latched += u32::from(r == 0);
        } else if !cross_entry {
            // Under the cross-button entry the walk ahead of the gate has
            // already rewritten `s2` (`command_prompts` above holds that);
            // the park is the directional entry's alone.
            let s2 = dispatched.get().unwrap();
            let p = w.take_command_pad_buffer();
            assert_eq!(
                (p.left, p.right, p.up, p.down, p.cross, p.circle),
                {
                    let q = to_input(s2);
                    (q.left, q.right, q.up, q.down, q.cross, q.circle)
                },
                "dispatched word: {label} s2 {s2:#x}"
            );
            assert_eq!(cpu.mem.rd32(CTX + 0x884), 0, "park emptied: {label}");
            consumed += u32::from(pad == 0 && park != 0);
        }
    }
    assert!(
        gated > 500 && parked > 50 && latched > 50 && consumed > 50,
        "{gated} {parked} {latched} {consumed}"
    );
}

/// The item window's target sub-cursors (`0x801D23F0..0x801D2EF0`): the
/// one-enemy cursor `0x5B`, the all-enemies confirm `0x5D`, the one-ally
/// cursor `0x64` and the all-allies confirm `0x66`, against
/// `World::tick_battle_item_menu` over an open item session. The enemy ring
/// steps through `FUN_801D8D00` run for real, as on `0x5A`.
#[test]
fn the_item_target_cursors_go_where_retail_goes() {
    differential!(prot(898, STATE_MACHINE), prot(898, RING_STEP));
    use legaia_engine_battle_loop::round_flow::PendingPartyAction;
    use legaia_engine_battle_vm::battle_action::{
        PoolActor, bearing_12bit_approx, build_attack_target_queue,
    };
    use legaia_engine_core::world::World;
    use legaia_engine_core::world::test_drivers::BattleTestDrivers;
    use legaia_engine_menus::inventory_use::InventoryUseState;
    use legaia_engine_menus::items::{ItemCatalog, ItemEffect, ItemEntry};
    use legaia_engine_system::option_values::BattleCommandOpt;
    use legaia_engine_system::pad::PadButton;
    let Some((mut cpu, _, _, _)) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.watched.insert(RING_STEP);
    // One item per surface: a single heal, a single bomb, a party-wide heal,
    // an every-enemy bomb.
    let items: [(u8, ItemEffect, bool, bool); 4] = [
        (0x01, ItemEffect::Heal { amount: 50 }, false, false),
        (0x13, ItemEffect::Damage { amount: 50 }, true, false),
        (0x02, ItemEffect::Heal { amount: 50 }, false, true),
        (0x14, ItemEffect::Damage { amount: 50 }, true, true),
    ];
    let mut catalog = ItemCatalog::new();
    for (id, effect, enemy, all) in items {
        catalog.insert(ItemEntry {
            id,
            name: "item",
            effect,
            usable_in_battle: true,
            usable_in_field: false,
        });
        catalog.set_enemy_side(id, enemy);
        catalog.set_all_party(id, all);
    }
    let mut rng = Rng(0x4954_454D);
    let mut seen = std::collections::HashMap::<(u8, &str), u32>::new();
    for round in 0..3000 {
        let state = [0x64u8, 0x5B, 0x66, 0x5D][rng.below(4) as usize];
        let (item, _, enemy_side, _) = items[[0x64u8, 0x5B, 0x66, 0x5D]
            .iter()
            .position(|s| *s == state)
            .unwrap()];
        let party = 1 + rng.below(3) as u8;
        let seat = rng.below(u32::from(party)) as u8;
        let count = 1 + rng.below(4) as usize;
        let cross_entry = rng.below(2) == 1;
        let mut alive = [false; 4];
        for a in alive.iter_mut().take(count) {
            *a = rng.below(4) != 0;
        }
        let live: Vec<u8> = (0..count as u8)
            .filter(|&i| alive[usize::from(i)])
            .collect();
        if live.is_empty() {
            continue;
        }
        let cur_enemy = live[rng.below(live.len() as u32) as usize];
        let cur_ally = rng.below(u32::from(party)) as u8;
        // The acting member stands, hurt (so a party-wide heal always has a
        // seat it helps); the others may be at full HP or down.
        let member_hp: [u16; 3] = std::array::from_fn(|i| {
            if i == usize::from(seat) {
                100
            } else {
                [100, 200, 0][rng.below(3) as usize]
            }
        });
        let span = 0x800;
        let mut pos = || {
            (
                (rng.below(span * 2) as i32 - span as i32) as i16,
                (rng.below(span * 2) as i32 - span as i32) as i16,
            )
        };
        let me = pos();
        let monster_pos: [(i16, i16); 4] = std::array::from_fn(|_| pos());
        let dirs = [
            0,
            LEFT,
            RIGHT,
            UP,
            DOWN,
            LEFT | UP,
            RIGHT | DOWN,
            LEFT | DOWN,
        ];
        let mut pad = dirs[rng.below(dirs.len() as u32) as usize];
        if rng.below(3) == 0 {
            pad |= CONFIRM;
        }
        if rng.below(4) == 0 {
            pad |= CANCEL;
        }
        let mut pool = [PoolActor::default(); 8];
        pool[0] = PoolActor {
            alive: true,
            x: me.0,
            z: me.1,
            target_slot: 3 + cur_enemy,
            status: 0,
        };
        for i in 0..4 {
            pool[3 + i] = PoolActor {
                alive: alive[i],
                x: monster_pos[i].0,
                z: monster_pos[i].1,
                target_slot: 0,
                status: 0,
            };
        }
        let q = build_attack_target_queue(&pool, 0, bearing_12bit_approx);

        // Retail.
        cpu.mem.wr32(CTX_PTR, CTX);
        cpu.mem.load(CTX, &[0u8; 0x1400]);
        for i in 0..8 {
            cpu.mem.wr32(ACTORS + 4 * i, ACTOR + STRIDE * i);
            cpu.mem.load(ACTOR + STRIDE * i, &[0u8; STRIDE as usize]);
        }
        let actor = ACTOR + STRIDE * u32::from(seat);
        cpu.mem.wr8(CTX, party);
        cpu.mem.wr8(CTX + 0x06, state);
        cpu.mem.wr8(CTX + 0x13, seat);
        for (i, b) in [
            q.count,
            q.wrap_slot,
            q.ordered[0],
            q.ordered[1],
            q.ordered[2],
        ]
        .iter()
        .enumerate()
        {
            cpu.mem.wr8(CTX + 0x244 + i as u32, *b);
        }
        let start = match state {
            0x5B => 3 + cur_enemy,
            0x64 => cur_ally,
            0x5D => 9,
            _ => 8,
        };
        cpu.mem.wr8(actor + 0x1DD, start);
        cpu.mem.wr8(actor + 0x1DF, item);
        cpu.mem.wr32(PAD, pad);
        cpu.mem.wr32(PAD_HELD, 0);
        cpu.mem.wr32(CROSS_ENTRY, u32::from(cross_entry));
        cpu.mem.wr32(CONFIRM_MASK, CONFIRM);
        cpu.mem.wr32(CANCEL_MASK, CANCEL);
        cpu.mem.wr8(BATTLE_ID, 0);
        cpu.stub(NEXT_ABLE, u32::from(party));
        assert_eq!(
            cpu.call(STATE_MACHINE, &[]),
            Stop::Returned,
            "round {round}"
        );
        let after = cpu.mem.rd8(CTX + 0x06);
        let target = cpu.mem.rd8(actor + 0x1DD);

        // The port: an item window open on the same surface.
        let mut w = World {
            mode: legaia_engine_mode::mode::SceneMode::Battle,
            ..World::default()
        };
        w.battle.player_driven = true;
        w.party.party_count = party;
        w.set_item_catalog(catalog.clone());
        w.toggles.battle_command = if cross_entry {
            BattleCommandOpt::CrossButton
        } else {
            BattleCommandOpt::DirectionalButtons
        };
        let pc = usize::from(party);
        // A member the item cannot help (at full HP, or down) takes the
        // cursor and the confirm as any other: retail tests no seat.
        for (i, &hp) in member_hp.iter().enumerate().take(pc) {
            let a = &mut w.actors[i];
            a.active = true;
            a.battle.max_hp = 200;
            a.battle.hp = hp;
            a.battle.liveness = u16::from(hp != 0);
            a.move_state.world_x = me.0;
            a.move_state.world_z = me.1;
        }
        for i in 0..count {
            let a = &mut w.actors[pc + i];
            a.active = true;
            a.battle.max_hp = 200;
            a.battle.hp = if alive[i] { 200 } else { 0 };
            a.battle.liveness = u16::from(alive[i]);
            a.battle_monster_id = Some(0x40 + i as u16);
            a.move_state.world_x = monster_pos[i].0;
            a.move_state.world_z = monster_pos[i].1;
        }
        w.battle_ctx.active_actor = seat;
        w.party.inventory.insert(item, 1);
        let mut menu = w.build_battle_item_session();
        let row = |slot: usize, enemy: bool| {
            menu.targets
                .iter()
                .position(|t| t.is_enemy == enemy && usize::from(t.slot) == slot)
        };
        menu.state = match state {
            0x5B => InventoryUseState::TargetSelect {
                item_cursor: 0,
                cursor: row(pc + usize::from(cur_enemy), true).expect("enemy row"),
            },
            0x64 => InventoryUseState::TargetSelect {
                item_cursor: 0,
                cursor: row(usize::from(cur_ally), false).expect("party row"),
            },
            0x5D => InventoryUseState::AllEnemiesConfirm { item_cursor: 0 },
            _ => InventoryUseState::AllPartyConfirm { item_cursor: 0 },
        };
        w.battle.item_menu = Some(menu);
        let mask = [
            (LEFT, PadButton::Left),
            (RIGHT, PadButton::Right),
            (UP, PadButton::Up),
            (DOWN, PadButton::Down),
            (CONFIRM, PadButton::Cross),
            (CANCEL, PadButton::Circle),
        ]
        .iter()
        .filter(|(p, _)| pad & p != 0)
        .fold(0u16, |m, (_, b)| m | b.mask());
        w.set_pad(0);
        w.set_pad(mask);
        w.tick_battle_item_menu();
        let label = format!(
            "round {round}: state {state:#x} pad {pad:#x} cross-entry {cross_entry} party {party} \
             seat {seat} cur {cur_enemy}/{cur_ally} alive {alive:?} -> {after:#x} target {target}"
        );
        let outcome = match (&w.battle.item_menu, after) {
            (Some(m), s) if s == state => match m.state {
                InventoryUseState::TargetSelect { cursor, .. } => {
                    let t = &m.targets[cursor];
                    let slot = usize::from(t.slot);
                    let want = if t.is_enemy { slot - pc + 3 } else { slot };
                    assert_eq!(usize::from(target), want, "cursor: {label}");
                    if usize::from(target) == usize::from(start) {
                        "stay"
                    } else {
                        "step"
                    }
                }
                InventoryUseState::AllPartyConfirm { .. }
                | InventoryUseState::AllEnemiesConfirm { .. } => "stay",
                ref other => panic!("port {other:?}: {label}"),
            },
            (Some(m), 0x3C) if matches!(m.state, InventoryUseState::Browsing { .. }) => "cancel",
            (None, 0x28 | 0x6E) => {
                let Some(Some(PendingPartyAction::Item { used_slots, .. })) =
                    w.battle.round_flow.pending.get(usize::from(seat))
                else {
                    panic!("no item committed: {label}");
                };
                if state == 0x5B || state == 0x64 {
                    let slot = usize::from(used_slots[0]);
                    let want = if enemy_side { slot - pc + 3 } else { slot };
                    assert_eq!(usize::from(target), want, "commit target: {label}");
                }
                "commit"
            }
            (m, after) => panic!(
                "port {:?} retail {after:#x}: {label}",
                m.as_ref().map(|m| m.state)
            ),
        };
        *seen.entry((state, outcome)).or_default() += 1;
    }
    for key in [
        (0x5B, "stay"),
        (0x5B, "step"),
        (0x5B, "commit"),
        (0x5B, "cancel"),
        (0x64, "step"),
        (0x64, "commit"),
        (0x64, "cancel"),
        (0x5D, "commit"),
        (0x5D, "cancel"),
        (0x66, "commit"),
        (0x66, "cancel"),
    ] {
        assert!(
            seen.get(&key).copied().unwrap_or(0) > 20,
            "{key:?}: {seen:?}"
        );
    }
}

/// The item window `0x3C` (`0x801D17DC..0x801D19F4`): the list kernel's
/// verdict `_DAT_8007BB94` - `2` a confirm, `3` a cancel - read against
/// `World::tick_battle_item_menu` on a Cross or a Circle. A confirm raises
/// the sub-cursor the item's target flags name (`+2` of its effect record,
/// `0x40` enemy side, `0x20` all) and seats the target byte: the caster on
/// the party side, the member's own monster target or the first living
/// monster on the enemy side. A cancel goes back to the ring.
#[test]
fn the_item_window_raises_the_sub_cursor_retail_raises() {
    differential!(prot(898, STATE_MACHINE));
    use legaia_engine_core::world::World;
    use legaia_engine_core::world::test_drivers::BattleTestDrivers;
    use legaia_engine_menus::inventory_use::InventoryUseState;
    use legaia_engine_menus::items::{ItemCatalog, ItemEffect, ItemEntry};
    use legaia_engine_system::pad::PadButton;
    let Some((mut cpu, _, _, _)) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const VERDICT: u32 = 0x8007_BB94;
    const SELECTION: u32 = 0x8007_BB88;
    const BAG: u32 = 0x8008_5958;
    const ITEM_TABLE: u32 = 0x8007_4368;
    const EFFECTS: u32 = 0x8007_52C0;
    // DB8B4's first living monster: the test seats it as pool slot 3 + the
    // first live row.
    let first_live = std::rc::Rc::new(std::cell::Cell::new(3u32));
    let fl = first_live.clone();
    cpu.hook(0x801D_B8B4, move |r, _| r.r[psx_retail_exec::V0] = fl.get());
    let mut rng = Rng(0x4954_574E);
    let mut seen = std::collections::HashMap::<u8, u32>::new();
    for round in 0..8000 {
        let party = 1 + rng.below(3) as u8;
        let seat = rng.below(u32::from(party)) as u8;
        let count = 1 + rng.below(4) as usize;
        let mut alive = [false; 4];
        for a in alive.iter_mut().take(count) {
            *a = rng.below(4) != 0;
        }
        let Some(first) = (0..count).find(|&i| alive[i]) else {
            continue;
        };
        first_live.set(3 + first as u32);
        let id = 1 + rng.below(0x60) as u8;
        let flags = {
            let effect = cpu.mem.rd8(ITEM_TABLE + 12 * u32::from(id) + 1);
            cpu.mem.rd8(EFFECTS + 4 * u32::from(effect) + 2)
        };
        let (enemy, all) = (flags & 0x40 != 0, flags & 0x20 != 0);
        // The member's target byte, pool space: a party seat, a live
        // monster seat (the round sweep `FUN_801D88CC` re-picks a dead one)
        // or a group code.
        let live: Vec<u8> = (0..count as u8)
            .filter(|&i| alive[usize::from(i)])
            .collect();
        let prev_pool: u8 = match rng.below(3) {
            0 => rng.below(u32::from(party)) as u8,
            1 => 3 + live[rng.below(live.len() as u32) as usize],
            _ => [8, 9][rng.below(2) as usize],
        };
        let verdict = [2u32, 3][rng.below(2) as usize];

        // Retail.
        cpu.mem.wr32(CTX_PTR, CTX);
        cpu.mem.load(CTX, &[0u8; 0x1400]);
        for i in 0..8 {
            cpu.mem.wr32(ACTORS + 4 * i, ACTOR + STRIDE * i);
            cpu.mem.load(ACTOR + STRIDE * i, &[0u8; STRIDE as usize]);
        }
        let actor = ACTOR + STRIDE * u32::from(seat);
        cpu.mem.wr8(CTX, party);
        cpu.mem.wr8(CTX + 0x06, 0x3C);
        cpu.mem.wr8(CTX + 0x13, seat);
        cpu.mem.wr8(CTX + 0x11 + 0xC, prev_pool);
        cpu.mem.wr8(actor + 0x1DD, prev_pool);
        cpu.mem.wr32(VERDICT, verdict);
        cpu.mem.wr32(SELECTION, 0);
        cpu.mem.wr8(BAG, id);
        cpu.mem.wr8(BAG + 1, 1);
        cpu.mem.wr32(PAD, 0);
        cpu.mem.wr32(PAD_HELD, 0);
        cpu.mem.wr32(CROSS_ENTRY, 0);
        cpu.mem.wr8(BATTLE_ID, 0);
        assert_eq!(
            cpu.call(STATE_MACHINE, &[]),
            Stop::Returned,
            "round {round}"
        );
        let after = cpu.mem.rd8(CTX + 0x06);
        let target = cpu.mem.rd8(actor + 0x1DD);

        // The port: the window open on the item, the same verdict pressed.
        let mut w = World {
            mode: legaia_engine_mode::mode::SceneMode::Battle,
            ..World::default()
        };
        w.battle.player_driven = true;
        w.party.party_count = party;
        let mut catalog = ItemCatalog::new();
        catalog.insert(ItemEntry {
            id,
            name: "item",
            effect: if enemy {
                ItemEffect::Damage { amount: 50 }
            } else {
                ItemEffect::Heal { amount: 50 }
            },
            usable_in_battle: true,
            usable_in_field: false,
        });
        catalog.set_enemy_side(id, enemy);
        catalog.set_all_party(id, all);
        w.set_item_catalog(catalog);
        let pc = usize::from(party);
        for i in 0..pc {
            let a = &mut w.actors[i];
            a.active = true;
            a.battle.max_hp = 200;
            a.battle.hp = 100;
            a.battle.liveness = 1;
        }
        for (i, &up) in alive.iter().enumerate().take(count) {
            let a = &mut w.actors[pc + i];
            a.active = true;
            a.battle.max_hp = 200;
            a.battle.hp = if up { 200 } else { 0 };
            a.battle.liveness = u16::from(up);
            a.battle_monster_id = Some(0x40 + i as u16);
        }
        // The pool target byte in the engine's seats.
        let prev_engine = match prev_pool {
            3..=7 => prev_pool - 3 + party,
            p => p,
        };
        w.actors[usize::from(seat)].battle.active_target = prev_engine;
        w.battle_ctx.active_actor = seat;
        w.party.inventory.insert(id, 1);
        w.battle.item_menu = Some(w.build_battle_item_session());
        w.set_pad(0);
        w.set_pad(if verdict == 2 {
            PadButton::Cross.mask()
        } else {
            PadButton::Circle.mask()
        });
        w.tick_battle_item_menu();
        let label = format!(
            "round {round}: item {id:#x} flags {flags:#x} verdict {verdict} party {party} seat {seat} \
             alive {alive:?} prev {prev_pool} -> {after:#x} target {target}"
        );
        let state = w.battle.item_menu.as_ref().map(|m| m.state);
        match (after, state) {
            (0x28, None) => {}
            (0x5B | 0x64, Some(InventoryUseState::TargetSelect { cursor, .. })) => {
                let m = w.battle.item_menu.as_ref().unwrap();
                let t = &m.targets[cursor];
                assert_eq!(t.is_enemy, after == 0x5B, "side: {label}");
                let slot = usize::from(t.slot);
                let want = if t.is_enemy { slot - pc + 3 } else { slot };
                assert_eq!(usize::from(target), want, "opening seat: {label}");
            }
            (0x5D, Some(InventoryUseState::AllEnemiesConfirm { .. }))
            | (0x66, Some(InventoryUseState::AllPartyConfirm { .. })) => {}
            (after, state) => panic!("retail {after:#x} port {state:?}: {label}"),
        }
        *seen.entry(after).or_default() += 1;
    }
    for s in [0x28u8, 0x5B, 0x5D, 0x64, 0x66] {
        assert!(seen.get(&s).copied().unwrap_or(0) > 20, "{s:#x}: {seen:?}");
    }
}

/// The magic window `0x46` (`0x801D19F8..0x801D1D80`): the list kernel's
/// result word `DAT_8007BB94` read once per pass. The pass prices the
/// highlighted spell `DAT_8007BB88` off the SCUS table's `+3` byte with the
/// caster's ability-word discount (`+0x6BC` bits `0x20` / `0x10`), then a
/// cancel (`3`) goes back to the ring and a confirm (`2`) is refused while
/// `actor+0x150` is below the price (the word cleared, the window kept) or
/// raises the sub-cursor the table's `+2` target bits pick: `0x5C` one
/// enemy, `0x5E` all enemies, `0x65` one ally on the caster's seat, `0x67`
/// all allies. Against `BattleSpellSession`'s list step over the disc
/// catalog (`seru_magic_catalog_from_scus`): Circle, Cross, or nothing on
/// the highlighted row.
#[test]
fn the_magic_window_prices_and_commits_as_retail() {
    differential!(prot(898, STATE_MACHINE));
    use legaia_engine_battle::battle_magic::{BattleSpellInput, BattleSpellSession, SpellPhase};
    use legaia_engine_battle::target_picker::{CursorRow, PickerState};
    let (Some((mut cpu, windows, _, _)), Some(scus)) = (retail_cpu(), crate::common::scus()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const SPELL: u32 = 0x8007_BB88;
    const RESULT: u32 = 0x8007_BB94;
    const CHAR_IDS: u32 = 0x8007_BD10;
    const RECORDS: u32 = 0x8008_4140;
    const RECORD_STRIDE: u32 = 0x414;
    let catalog =
        legaia_engine_battle::retail_magic::seru_magic_catalog_from_scus(&scus).expect("SCUS");
    let ids: Vec<u8> = catalog.iter().map(|d| d.id).collect();
    assert!(ids.len() > 20, "{} spells", ids.len());
    let mut rng = Rng(0x4D41_4749);
    let mut seen = std::collections::HashMap::<&str, u32>::new();
    for round in 0..6000 {
        let party = 1 + rng.below(3) as u8;
        let seat = rng.below(u32::from(party)) as u8;
        let char_id = 1 + rng.below(3) as u8;
        // The caster's learned list, the highlighted spell among it.
        let n = 1 + rng.below(12) as usize;
        let mut learned = Vec::new();
        while learned.len() < n {
            let id = ids[rng.below(ids.len() as u32) as usize];
            if !learned.contains(&id) {
                learned.push(id);
            }
        }
        let row = rng.below(n as u32) as usize;
        let spell = learned[row];
        let ability = [0u32, 0x10, 0x20, 0x30, 0x8000_0001][rng.below(5) as usize];
        let cost = legaia_engine_battle::spells::caster_mp_cost(
            catalog.get(spell).expect("listed"),
            ability,
        );
        let mp = match rng.below(4) {
            0 => cost.saturating_sub(1),
            1 => cost,
            2 => rng.below(u32::from(cost) + 1) as u16,
            _ => rng.below(1000) as u16,
        };
        let result = [0u32, 2, 2, 3, 1][rng.below(5) as usize];

        // Retail.
        cpu.mem.wr32(CTX_PTR, CTX);
        cpu.mem.load(CTX, &[0u8; 0x1400]);
        for i in 0..8 {
            cpu.mem.wr32(ACTORS + 4 * i, ACTOR + STRIDE * i);
            cpu.mem.load(ACTOR + STRIDE * i, &[0u8; STRIDE as usize]);
        }
        let actor = ACTOR + STRIDE * u32::from(seat);
        cpu.mem.wr8(CTX, party);
        cpu.mem.wr8(CTX + 0x06, 0x46);
        cpu.mem.wr8(CTX + 0x13, seat);
        cpu.mem.wr8(CTX + 0x1D, 4);
        cpu.mem.wr16(actor + 0x150, mp);
        cpu.mem.wr8(actor + 0x1DD, 3);
        for s in 0..3 {
            cpu.mem.wr8(CHAR_IDS + s, 0);
        }
        cpu.mem.wr8(CHAR_IDS + u32::from(seat), char_id);
        let rec = RECORDS + RECORD_STRIDE * u32::from(char_id - 1);
        cpu.mem.load(rec + 0x6BC, &[0u8; 0x70]);
        cpu.mem.wr32(rec + 0x6BC, ability);
        cpu.mem.wr8(rec + 0x704, n as u8);
        for (i, id) in learned.iter().enumerate() {
            cpu.mem.wr8(rec + 0x705 + i as u32, *id);
            cpu.mem.wr8(rec + 0x729 + i as u32, 1 + i as u8);
        }
        cpu.mem.wr32(SPELL, u32::from(spell));
        cpu.mem.wr32(RESULT, result);
        cpu.mem.wr32(PAD, 0);
        cpu.mem.wr32(PAD_HELD, 0);
        cpu.mem.wr32(CROSS_ENTRY, 0);
        cpu.mem.wr32(CONFIRM_MASK, CONFIRM);
        cpu.mem.wr32(CANCEL_MASK, CANCEL);
        cpu.mem.wr8(BATTLE_ID, 0);
        windows.borrow_mut().clear();
        assert_eq!(
            cpu.call(STATE_MACHINE, &[]),
            Stop::Returned,
            "round {round}"
        );
        let after = cpu.mem.rd8(CTX + 0x06);
        let target = cpu.mem.rd8(actor + 0x1DD);
        assert_eq!(cpu.mem.rd8(actor + 0x1DF), spell, "round {round}");
        assert_eq!(
            u16::from(cpu.mem.rd8(CTX + 0x24)),
            cost,
            "round {round}: the price, spell {spell:#x} ability {ability:#x}"
        );
        assert_eq!(
            cpu.mem.rd8(CTX + 0x26),
            1 + row as u8,
            "round {round}: level"
        );

        // The port.
        let party_rows: [SlotState; 3] = std::array::from_fn(|i| {
            if i < usize::from(party) {
                SlotState::alive(true, true)
            } else {
                SlotState::dead(false)
            }
        });
        let monsters: [SlotState; 5] = std::array::from_fn(|i| SlotState::alive(i < 2, i < 2));
        let mut session = BattleSpellSession::new(seat, seat, &learned, &catalog, mp, ability);
        session.phase = SpellPhase::Select { cursor: row as u8 };
        session.input(
            BattleSpellInput {
                cross: result == 2,
                circle: result == 3,
                ..BattleSpellInput::default()
            },
            &catalog,
            party_rows,
            monsters,
        );
        let label = format!(
            "round {round}: spell {spell:#x} cost {cost} mp {mp} result {result} party {party} \
             seat {seat} -> {after:#x} target {target} windows {:x?} port {:?}",
            windows.borrow(),
            session.phase
        );
        let outcome = match (&session.phase, after) {
            (SpellPhase::Select { cursor }, 0x46) => {
                assert_eq!(usize::from(*cursor), row, "{label}");
                if result == 2 {
                    assert_eq!(cpu.mem.rd32(RESULT), 0, "refused: {label}");
                    "refused"
                } else {
                    "browse"
                }
            }
            (SpellPhase::Aborted, 0x28) => "cancel",
            (SpellPhase::Targeting { picker, .. }, 0x5C) => {
                assert!(
                    matches!(
                        picker.state(),
                        PickerState::Cursor {
                            row: CursorRow::Enemy,
                            ..
                        }
                    ),
                    "{label}"
                );
                "one enemy"
            }
            (SpellPhase::Targeting { picker, .. }, 0x65) => {
                assert_eq!(
                    picker.state(),
                    PickerState::Cursor {
                        row: CursorRow::Ally,
                        slot: seat
                    },
                    "{label}"
                );
                assert_eq!(target, seat, "{label}");
                "one ally"
            }
            (
                SpellPhase::SweepConfirm {
                    row: CursorRow::Enemy,
                    ..
                },
                0x5E,
            ) => {
                assert_eq!(target, 9, "{label}");
                "all enemies"
            }
            (
                SpellPhase::SweepConfirm {
                    row: CursorRow::Ally,
                    ..
                },
                0x67,
            ) => {
                assert_eq!(target, 8, "{label}");
                "all allies"
            }
            _ => panic!("{label}"),
        };
        *seen.entry(outcome).or_default() += 1;
    }
    for key in [
        "browse",
        "refused",
        "cancel",
        "one enemy",
        "one ally",
        "all enemies",
        "all allies",
    ] {
        assert!(seen.get(key).copied().unwrap_or(0) > 20, "{key}: {seen:?}");
    }
}

/// The battle open, flow `0x00 -> 0x0A -> 0x0B` and the round start `0x14`
/// (`0x801D0DD0..0x801D0F0C`): `0x00` seats the panel anchors and stores
/// `0x0A`; `0x0A` composes the enemy names and seeds the intro timer
/// `ctx[+0x6D6]` (`0x5A`, or `0x78` when the formation byte `ctx[+0x290]` is
/// set), except for a first monster `0xB5`, which parks on `0x0C` with no
/// names; `0x0B` drains the timer by the frame step and leaves for `0xFE`
/// on a back attack (`+0x290 == 1`) or `0x14`; `0x14` raises the round
/// prompt `0x1E` and ticks the status DoTs from the second round on
/// (`ctx[+0x28A] != 0`). Against `World::arm_battle_intro_names` and
/// `BattleState::step_battle_intro_names` (the hold's seed and its drain),
/// the formation byte's decode the round start's ambush test reads, and the
/// round counter `World::battle_mode` the DoT gate reads.
#[test]
fn the_battle_open_holds_and_opens_the_round_as_retail() {
    differential!(prot(898, STATE_MACHINE));
    use legaia_engine_battle_vm::battle_formulas::FormationAdvantage;
    use legaia_engine_core::world::World;
    use legaia_engine_core::world::test_drivers::BattleTestDrivers;
    let Some((mut cpu, windows, _, _)) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const ANCHORS: u32 = 0x801D_84C0;
    const NAMES: u32 = 0x801D_9D3C;
    const DOT: u32 = 0x801E_752C;
    const STEP: u32 = 0x1F80_0393;
    let calls = Rc::new(RefCell::new(Vec::<u32>::new()));
    for leaf in [
        ANCHORS,
        NAMES,
        DOT,
        0x801D_88CC,
        0x801D_A780,
        0x801D_5854,
        0x801D_8DE8,
    ] {
        let c2 = calls.clone();
        cpu.hook(leaf, move |_, _| c2.borrow_mut().push(leaf));
    }
    let mut rng = Rng(0x4F50_454E);
    let mut seen = std::collections::HashMap::<&str, u32>::new();
    for round in 0..400 {
        let adv = [0u8, 0, 1, 2, 3][rng.below(5) as usize];
        let first = if rng.below(4) == 0 {
            0xB5
        } else {
            rng.below(0xB5) as u8
        };
        let rounds_done = [0u8, 0, 1, 5][rng.below(4) as usize];
        cpu.mem.wr32(CTX_PTR, CTX);
        cpu.mem.load(CTX, &[0u8; 0x1400]);
        for i in 0..8 {
            cpu.mem.wr32(ACTORS + 4 * i, ACTOR + STRIDE * i);
            cpu.mem.load(ACTOR + STRIDE * i, &[0u8; STRIDE as usize]);
        }
        cpu.mem.wr8(CTX, 3);
        cpu.mem.wr8(CTX + 0x290, adv);
        cpu.mem.wr8(CTX + 0x28A, rounds_done);
        cpu.mem.wr8(BATTLE_ID, first);
        cpu.mem.wr8(STEP, 1);
        cpu.mem.wr32(PAD, 0);
        cpu.mem.wr32(CROSS_ENTRY, 0);
        let pass = |cpu: &mut Cpu| {
            calls.borrow_mut().clear();
            windows.borrow_mut().clear();
            assert_eq!(
                cpu.call(STATE_MACHINE, &[]),
                Stop::Returned,
                "round {round}"
            );
            cpu.mem.rd8(CTX + 0x06)
        };
        assert_eq!(pass(&mut cpu), 0x0A, "round {round}");
        assert_eq!(*calls.borrow(), [ANCHORS], "round {round}");
        let after = pass(&mut cpu);
        let seed = cpu.mem.rd16(CTX + 0x6D6);
        let named = calls.borrow().contains(&NAMES);

        // The port's seed.
        let mut w = World::new();
        w.load_party(legaia_save::Party::zeroed(3));
        w.enter_battle(3, 2);
        w.party.party_count = 3;
        w.actors[3].battle_monster_id = Some(u16::from(first));
        w.battle_ctx.formation_advantage = adv;
        w.arm_battle_intro_names();
        let label = format!("round {round}: adv {adv} first {first:#x} rounds {rounds_done}");
        if first == 0xB5 {
            // Parked for the arrival module: no names, no hold of the port's.
            assert_eq!(after, 0x0C, "{label}");
            assert!(!named, "{label}");
            assert_eq!(w.battle.intro_lead_ticks, 0, "{label}");
            assert_eq!(w.battle.intro_names_frames, 0, "{label}");
            assert_eq!(pass(&mut cpu), 0x0C, "{label}: 0x0C is not dispatched");
            *seen.entry("nameless").or_default() += 1;
            continue;
        }
        assert_eq!(after, 0x0B, "{label}");
        assert!(named, "{label}");
        assert_eq!(seed, w.battle.intro_names_frames, "{label}: the hold");
        // The drain, one pass a vsync.
        w.battle.intro_lead_ticks = 0;
        let mut retail_passes = 0;
        let exit = loop {
            let s = pass(&mut cpu);
            retail_passes += 1;
            if s != 0x0B {
                break s;
            }
            assert!(retail_passes < 0x100, "{label}");
        };
        let mut port_ticks = 0;
        while w.battle.battle_open_held() {
            w.battle.step_battle_intro_names();
            port_ticks += 1;
        }
        assert_eq!(retail_passes, port_ticks, "{label}");
        let ambushed = FormationAdvantage::from_byte(adv) == FormationAdvantage::BackAttack;
        if ambushed {
            assert_eq!(exit, 0xFE, "{label}");
            *seen.entry("ambushed").or_default() += 1;
            continue;
        }
        assert_eq!(exit, 0x14, "{label}");
        // The round start.
        assert_eq!(pass(&mut cpu), 0x1E, "{label}");
        assert_eq!(*windows.borrow(), [0], "{label}: FUN_801D388C(0, 0)");
        w.battle.monster_ai_state.mode_flags = rounds_done;
        assert_eq!(
            calls.borrow().contains(&DOT),
            w.battle_mode() != 0,
            "{label}: the DoT tick"
        );
        *seen
            .entry(if rounds_done == 0 {
                "first round"
            } else {
                "later round"
            })
            .or_default() += 1;
    }
    for key in ["nameless", "ambushed", "first round", "later round"] {
        assert!(seen.get(key).copied().unwrap_or(0) > 10, "{key}: {seen:?}");
    }
}
