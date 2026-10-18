//! Running the session forward under scripted input: the pad a parked
//! script wants, pickers and flag windows, the tick budgets, and the `Run`
//! outcome every caller branches on.

use super::*;

// ---------------------------------------------------------------------------
// Tick drivers
// ---------------------------------------------------------------------------

/// Ticks an entry script gets to hand control back.
pub(super) const SETTLE_TICKS: usize = 3_600;
/// Ticks a door record gets to reach its `0x3F` after the step, and the deep
/// budget a record still running at the end of it gets.
pub(super) const EXIT_TICKS: usize = 2_400;
pub(super) const DEEP_EXIT_TICKS: usize = 24_000;
/// Consecutive idle ticks that end a post-step wait.
pub(super) const EXIT_IDLE_TICKS: usize = 4;
/// A battle's tick budget under the pad fighter. A boss the fighter wears
/// down with summons, arts and heals runs well past 30 000 ticks.
pub(super) const BATTLE_TICKS: usize = 60_000;
/// Past [`BATTLE_TICKS`] a fight goes on while the foes are still losing HP:
/// it ends unresolved only after this long without a foe's HP dropping, or at
/// [`BATTLE_TICKS_CAP`]. A boss whose specials land (Cort's Guilty Cross in
/// `chitei2`) costs the party turns on heals, and a fight it is winning
/// outlasts the flat budget - a player keeps fighting it.
pub(super) const BATTLE_STALL_TICKS: usize = 36_000;
pub(super) const BATTLE_TICKS_CAP: usize = 600_000;
/// The most a scripted sequence may run while its park site keeps moving.
pub(super) const SCRIPT_CEILING: usize = 60_000;
/// Hops a segment may take before it is called lost.
pub(super) const MAX_HOPS: usize = 16;
/// Door sites tried per hop.
pub(super) const MAX_SITES: usize = 12;

/// What one tick-run ended on.
#[derive(Debug)]
pub(super) enum Run {
    /// A scene change landed (directly, or via an FMV hand-off).
    Entered(String),
    /// Control came back (nothing is running).
    Released,
    /// The budget ran out with something still holding the pad.
    Parked(String),
    /// A battle ended in a wipe or never resolved.
    Battle(String),
    /// The host returned an error.
    Error(String),
}

/// What a player presses while a script holds the frame: Cross on a
/// press-2-release-14 duty cycle (pages are edge-triggered), except on the
/// naming prompt's Yes/No confirm, which opens on No - there the hand moves
/// Up to Yes first, or the prompt drops back to editing forever.
pub(super) fn script_pad(session: &NativeSession, f: usize) -> u16 {
    use legaia_engine_menus::name_entry::NameEntryState;
    if let Some(pad) = held_pad_poll(session) {
        return pad;
    }
    if f % 16 >= 2 {
        return 0;
    }
    if let Some(pad) = picker_pad(session) {
        return pad;
    }
    if let Some(pad) = flag_window_pad(session) {
        return pad;
    }
    if let Some(pad) = code_lock_pad(session) {
        return pad;
    }
    if let Some(ne) = session.host.world.party.name_entry.as_ref()
        && ne.state == NameEntryState::Confirm
        && !ne.confirm_yes
    {
        return PadButton::Up.mask();
    }
    // Nothing holds the frame: a Cross here is a talk, not a page turn. A
    // door seat beside an NPC (`rayman`'s gate guard, re-seated beside the
    // `tunnelb` door once `0x1FC` is up) otherwise opened his talk on the
    // frame the walk-on band was due.
    if released(session) {
        return 0;
    }
    PadButton::Cross.mask()
}

/// The d-pad a record is polling for: a cutscene timeline (or spawned
/// record) sitting on op `42 01 <i>`, the held-pad compare against the
/// compass table (`0x801F28D0`: Down, Down+Left, Left, Left+Up, Up,
/// Up+Right, Right, Right+Down). Rim Elm's "stand here and press" beats
/// (`town01` P2[12..14], `town0e` P2[0..4]) re-run every tick the player
/// stands on the tile (the `2E 13` re-poll bit) and take their action only
/// while the direction is held.
pub(super) fn held_pad_poll(session: &NativeSession) -> Option<u16> {
    let w = &session.host.world;
    let (bc, pc) = if let Some(tl) = w.cutscene.timeline.as_ref() {
        (&tl.bytecode, tl.pc)
    } else {
        let h = w.field_vm.helper_contexts.first()?;
        (&h.bytecode, h.pc)
    };
    // The record re-spawns every tick, so between ticks it usually sits on
    // the `2E 13` that raises the re-poll bit, one op ahead of the poll.
    let mut pc = if bc.get(pc..pc + 2) == Some(&[0x2E, 0x13]) {
        pc + 2
    } else {
        pc
    };
    if bc.get(pc) != Some(&0x42) || bc.get(pc + 1) != Some(&0x01) {
        // A modal timeline waiting ahead of its poll: the test runs on the
        // tick the wait ends, between two of the hand's looks, so the
        // direction has to be held through the wait.
        pc = w
            .cutscene
            .timeline
            .as_ref()
            .and_then(|tl| poll_ahead(&tl.bytecode, tl.pc))?;
    }
    let (d, l, u, r) = (
        PadButton::Down.mask(),
        PadButton::Left.mask(),
        PadButton::Up.mask(),
        PadButton::Right.mask(),
    );
    [d, d | l, l, l | u, u, u | r, r, r | d]
        .get(usize::from(*bc.get(pc + 2)?))
        .copied()
}

/// The held-pad poll (`42 01 <i>`) a timeline at `pc` reaches next with
/// nothing in between but waits, yields and the flag, move and camera ops
/// that cannot branch. `jouinb`'s rapids (P2[6..8]) are the case: the ride
/// loops a lap of walk-to-tile yields on the player (`C7 F8 ..`) with a poll
/// chain after a yield at each landing - any compass point or Circle jumps
/// off there - and a hand that held nothing rode lap after lap, about a
/// thousand frames each, until the ride ended some other way: a crossing
/// cost the walking budget some 75000 frames.
pub(super) fn poll_ahead(bc: &[u8], mut pc: usize) -> Option<usize> {
    use legaia_scene_formats::field_disasm::{FlagKind, InsnInfo, decode};
    for _ in 0..32 {
        let insn = decode(bc, pc).ok()?;
        match insn.info {
            InsnInfo::CondJmp { mode: 1, op1, .. } if op1 < 8 => return Some(pc),
            InsnInfo::Nop
            | InsnInfo::ExecMove { .. }
            | InsnInfo::MoveTo { .. }
            | InsnInfo::Yield { .. }
            | InsnInfo::CamCfg { .. }
            | InsnInfo::Animate { .. }
            | InsnInfo::SceneFade { .. }
            | InsnInfo::Camera { .. }
            | InsnInfo::WaitFrames { .. }
            | InsnInfo::ActorCtrl { .. } => {}
            InsnInfo::LFlag { .. } | InsnInfo::CFlag { .. } => {}
            InsnInfo::GFlag { kind, .. } if kind != FlagKind::Test => {}
            _ => return None,
        }
        pc += insn.size.max(1);
    }
    None
}

/// The pads a script's leading held-pad poll chain passes, when the pad
/// holder sits on one: consecutive op-`0x42` mode-1 tests (`42 01 <i> ..`),
/// each against one compass point (`0x801F28D0`).
pub(super) fn poll_chain_pads(session: &NativeSession) -> Option<Vec<u16>> {
    let w = &session.host.world;
    let (bc, mut pc) = if let Some(tl) = w.cutscene.timeline.as_ref() {
        (&tl.bytecode, tl.pc)
    } else {
        let h = w.field_vm.helper_contexts.first()?;
        (&h.bytecode, h.pc)
    };
    let (d, l, u, r) = (
        PadButton::Down.mask(),
        PadButton::Left.mask(),
        PadButton::Up.mask(),
        PadButton::Right.mask(),
    );
    let compass = [d, d | l, l, l | u, u, u | r, r, r | d];
    let mut pads = Vec::new();
    while bc.get(pc) == Some(&0x42) && bc.get(pc + 1) == Some(&0x01) {
        pads.push(*compass.get(usize::from(*bc.get(pc + 2)?))?);
        pc += 5;
    }
    (!pads.is_empty()).then_some(pads)
}

/// A conversation picker: the talk record's address and the picker's offset
/// in it.
pub(super) type PickerKey = (usize, usize);

thread_local! {
    /// Visits per conversation picker, keyed by the talk record and the
    /// picker's offset in it, and the picker open on the last pad read.
    pub(super) static PICKS: std::cell::RefCell<(HashMap<PickerKey, usize>, Option<PickerKey>)> =
        std::cell::RefCell::new((HashMap::new(), None));
}

/// The pad on a conversation picker (the inline runner's option menu).
/// Option 0 is often "tell me again" - a branch that jumps back to the same
/// speech - so a hand that always confirms the default loops forever. The
/// hand instead takes option `k` on the picker's `k`-th opening (0, then 1,
/// ...), which walks a menu to its story branch or its exit.
pub(super) fn picker_pad(session: &NativeSession) -> Option<u16> {
    let w = &session.host.world;
    let menu = |bytecode: &std::sync::Arc<Vec<u8>>,
                panel: &legaia_engine_dialog::dialog::OwnedDialogPanel| {
        let pk = panel.picker()?;
        // Keyed on the record's bytes, not its allocation: each talk
        // re-installs the record in a fresh buffer, so a pointer key made
        // every conversation the picker's first opening and the hand never
        // got past option 0 (`tunnelc` P1[4], Xain: only option 1 raises
        // the `0x325` the fight waits on).
        let id = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            bytecode.as_slice().hash(&mut h);
            h.finish() as usize
        };
        panel.menu_active().then_some((
            (id, pk.open),
            pk.n.max(1),
            panel.picker_cursor(),
            panel.picker_takes_input(),
        ))
    };
    // A talk's box, else a script's: the modal timeline's, else the first
    // spawned record holding one (the pad goes to the same box the engine
    // routes it to). A cutscene picker that always takes its default can
    // loop a record forever (`town0d` P2[31], the song rehearsal).
    let open = w
        .dialog
        .inline
        .as_ref()
        .and_then(|id| menu(&id.bytecode, id.panel.as_ref()?))
        .or_else(|| {
            // The FIRST box, picker or not: a later record's picker the
            // engine does not route to must not steer the pad (`dolk2`'s
            // market beat stacks three spawned records' boxes; pressing Down
            // for the third's picker left the first's page unturned).
            let tl = w
                .cutscene
                .timeline
                .iter()
                .filter(|t| t.dialog.is_some())
                .chain(
                    w.field_vm
                        .helper_contexts
                        .iter()
                        .filter(|t| t.dialog.is_some()),
                )
                .next()?;
            menu(&tl.bytecode, tl.dialog.as_ref()?)
        });
    let Some((key, n, cursor, takes)) = open else {
        PICKS.with(|p| p.borrow_mut().1 = None);
        return None;
    };
    let visit = PICKS.with(|p| {
        let mut p = p.borrow_mut();
        if p.1 != Some(key) {
            p.1 = Some(key);
            *p.0.entry(key).or_insert(0) += 1;
        }
        p.0[&key]
    });
    if !takes {
        return Some(0);
    }
    let want = ride_option(session)
        .or_else(|| split_stay_option(session))
        .or_else(|| beat_progress_option(session))
        .or_else(|| (visit == 1).then(|| wanted_flag_option(session)).flatten())
        .or_else(|| open_way_option(session))
        .or_else(|| quiz_option(session))
        .or_else(|| rest_option(session))
        .unwrap_or((visit - 1) % n);
    Some(if cursor < want {
        PadButton::Down.mask()
    } else if cursor > want {
        PadButton::Up.mask()
    } else {
        PadButton::Cross.mask()
    })
}

thread_local! {
    /// Set while the continuous hand takes a rest ([`pad_rest_to`]): a
    /// picker met then is the keeper's offer.
    pub(super) static RESTING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Set while the continuous hand talks to a merchant ([`pad_shop`]): a
    /// picker met then is the counter's "what will it be?".
    pub(super) static SHOPPING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// While the continuous hand rests or shops, the option of the open picker
/// whose branch reaches what it came for soonest - the restore (`4C 82`) or
/// the shop op (`49 00`): an innkeeper's "stay?" and a counter's "buy?" are
/// pickers, and the rotation's answer to one is as often No.
pub(super) fn rest_option(session: &NativeSession) -> Option<usize> {
    use legaia_scene_formats::field_disasm::{InsnInfo, decode};
    let errand: [u8; 2] = if RESTING.with(std::cell::Cell::get) {
        [0x4C, 0x82]
    } else if SHOPPING.with(std::cell::Cell::get) {
        [0x49, 0x00]
    } else {
        return None;
    };
    let w = &session.host.world;
    let (bc, pc0) = match w.dialog.inline.as_ref() {
        Some(id) if id.panel.as_ref().is_some_and(|p| p.menu_active()) => {
            (id.bytecode.clone(), id.pc)
        }
        _ => {
            let tl = w
                .cutscene
                .timeline
                .iter()
                .filter(|t| t.dialog.is_some())
                .chain(
                    w.field_vm
                        .helper_contexts
                        .iter()
                        .filter(|t| t.dialog.is_some()),
                )
                .next()?;
            (tl.bytecode.clone(), tl.pc)
        }
    };
    let bc = bc.as_slice();
    let mut pc = pc0;
    let mut picker = None;
    for _ in 0..16 {
        let Ok(i) = decode(bc, pc) else { break };
        if let InsnInfo::Picker { count, targets, .. } = i.info {
            picker = Some((count as usize, targets));
            break;
        }
        pc += i.size.max(1);
    }
    let (count, targets) = picker?;
    let reach = |mut at: usize| -> Option<usize> {
        for n in 0..400 {
            if bc.get(at) == Some(&errand[0]) && bc.get(at + 1) == Some(&errand[1]) {
                return Some(n);
            }
            let i = decode(bc, at).ok()?;
            at = match i.info {
                InsnInfo::JmpRel { target, .. } => target,
                _ => at + i.size.max(1),
            };
        }
        None
    };
    (0..count.min(4))
        .filter_map(|k| reach(targets[k]).map(|n| (n, k)))
        .min()
        .map(|(_, k)| k)
}

thread_local! {
    /// The partition-2 record of the walk-on beat [`play_beats`] is walking
    /// onto, while it does.
    pub(super) static PLAYING_BEAT: std::cell::Cell<Option<u8>> = const { std::cell::Cell::new(None) };
}

/// On the walk-on beat being played, the option of its own picker that
/// carries the story on: the first whose branch spawns a record (`0x44`)
/// and changes no scene before its first `0x21`. `nilboa` P2[20] ("We have
/// to jump down to where Koru is! Are you ready?") spawns the Delilas
/// family's challenge P2[21] only on "I'm ready", and the band is walked
/// once per visit, so the rotation's first answer, "Now wait a second!",
/// left the chain - and the party's way off the north half - unplayed.
pub(super) fn beat_progress_option(session: &NativeSession) -> Option<usize> {
    let beat = PLAYING_BEAT.with(std::cell::Cell::get)?;
    if timeline_p2_record(session) != Some(beat) {
        return None;
    }
    let (count, targets, bc) = open_picker(session)?;
    // The branch runs to its first `0x21`: "I'm ready" plays some eighty
    // instructions of the jump down before its `44 63`.
    let spawns = |mut at: usize| {
        for _ in 0..512 {
            let Ok(i) = legaia_scene_formats::field_disasm::decode(&bc, at) else {
                return false;
            };
            match bc[at] & 0x7F {
                0x44 => return true,
                0x3F | 0x21 => return false,
                _ => {}
            }
            at = match i.info {
                legaia_scene_formats::field_disasm::InsnInfo::JmpRel { target, .. } => target,
                _ => at + i.size.max(1),
            };
        }
        false
    };
    (0..count.min(4)).find(|&k| spawns(targets[k]))
}

/// The option of the open picker whose branch, and only its branch, raises
/// a still-clear flag the next anchor carries: an answer the story records
/// (`stone`'s four statues, P0[2..5], each a four-way question whose
/// options raise their own flag of `0x1D8..0x1E7`; the right four answers
/// are what spawn P2[5], which opens the Stone Flower). The branch is
/// followed through its jumps to its first `0x21` or scene change. `None`
/// when no option, or more than one, does. Asked on a picker's first
/// opening only: a staged conversation can want another answer first
/// (`tunnelc` P1[4], Xain, whose option 1 the rotation reaches on the
/// second opening), and a reopened picker goes back to the rotation.
pub(super) fn wanted_flag_option(session: &NativeSession) -> Option<usize> {
    use legaia_scene_formats::field_disasm::{FlagKind, InsnInfo, decode};
    let (bc, pc, count, targets) = talk_or_script_picker(session)?;
    let bc = bc.as_slice();
    let wanted = NEXT_ANCHOR_FLAGS.with(|a| a.borrow().clone())?;
    let have = flags_of_world(session);
    let raises = |mut at: usize| {
        for _ in 0..256 {
            let Ok(i) = decode(bc, at) else {
                return false;
            };
            match i.info {
                InsnInfo::SystemFlag {
                    kind: FlagKind::Set,
                    idx,
                    ..
                } if wanted.contains(&idx) && !have.contains(&idx) => return true,
                InsnInfo::JmpRel { target, .. } => {
                    at = target;
                    continue;
                }
                _ => {}
            }
            if matches!(bc[at] & 0x7F, 0x21 | 0x3F) {
                return false;
            }
            at += i.size.max(1);
        }
        false
    };
    let hits: Vec<usize> = (0..count.min(4)).filter(|&k| raises(targets[k])).collect();
    if hits.len() == 1 && std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
        eprintln!(
            "      [pick] {} picker at 0x{pc:04X}: option {} raises a wanted flag",
            scene_name(session),
            hits[0]
        );
    }
    (hits.len() == 1).then(|| hits[0])
}

/// The picker open in a talk (the inline runner's record) or, failing
/// that, in a script's box (the modal timeline's, else the first spawned
/// record holding one): the record's bytes, the picker's pc, its option
/// count and branch targets.
fn talk_or_script_picker(session: &NativeSession) -> Option<(Vec<u8>, usize, usize, [usize; 4])> {
    use legaia_scene_formats::field_disasm::{InsnInfo, decode};
    let w = &session.host.world;
    let (bc, mut pc): (&[u8], usize) = if let Some(id) = w.dialog.inline.as_ref() {
        (id.bytecode.as_slice(), id.pc)
    } else {
        let tl = w
            .cutscene
            .timeline
            .iter()
            .filter(|t| t.dialog.is_some())
            .chain(
                w.field_vm
                    .helper_contexts
                    .iter()
                    .filter(|t| t.dialog.is_some()),
            )
            .next()?;
        (tl.bytecode.as_slice(), tl.pc)
    };
    for _ in 0..16 {
        let Ok(i) = decode(bc, pc) else { break };
        if let InsnInfo::Picker { count, targets, .. } = i.info {
            return Some((bc.to_vec(), pc, usize::from(count), targets));
        }
        pc += i.size.max(1);
    }
    None
}

/// The answer to a quiz: of a picker with four options, the one
/// whose branch goes somewhere of its own while every other option's goes
/// to one shared place (the wrong-answer speech). `doman` P1[3] puts four
/// such questions to the party before it raises `0x4FF` and `0x375`; every
/// wrong answer branches to the one "that's not it" reply and ends the
/// talk, so a rotation that walks the options in turn needs a talk per
/// wrong answer and starts the quiz over each time.
pub(super) fn quiz_option(session: &NativeSession) -> Option<usize> {
    let (_, pc, count, targets) = talk_or_script_picker(session)?;
    let n = count.min(4);
    if n < 4 {
        return None;
    }
    let t = &targets[..n];
    let odd: Vec<usize> = (0..n)
        .filter(|&k| t.iter().filter(|&&o| o == t[k]).count() == 1)
        .collect();
    let shared = (0..n)
        .filter(|&k| t[k] != t[odd.first().copied().unwrap_or(0)])
        .count();
    let pick = (odd.len() == 1 && shared == n - 1).then(|| odd[0]);
    if let Some(k) = pick
        && std::env::var_os("LEGAIA_FGL_TRACE").is_some()
    {
        eprintln!(
            "      [pick] {} picker at 0x{pc:04X}: option {k} is the quiz's answer",
            scene_name(session)
        );
    }
    pick
}

/// The option of the open picker that opens the way when another option
/// shuts it: the one branch that clears collision-grid wall bits (op `4C`
/// nibble 7, sub 0 or 2) while another sets them (sub 1 or 3), each
/// followed through its jumps to its first `0x21` or scene change. A
/// player answers the question that lets them through. `balden` P2[6], the
/// lower elevator car's call band, asks "Are you human?": Yes clears the
/// car door's wall bits (`4C 72`) and starts the car, No paints them back
/// (`4C 73`). The rotation's No on a later crossing shut the door, and the
/// upper car (P0[14]) then set the party down inside the shut car, whose
/// touch its own ride turns off until the call band - beyond the shut
/// door - turns it back on.
pub(super) fn open_way_option(session: &NativeSession) -> Option<usize> {
    let (count, targets, bc) = open_picker(session)?;
    // `Some(true)` opens, `Some(false)` shuts, `None` paints nothing.
    let paints = |mut at: usize| -> Option<bool> {
        for _ in 0..256 {
            let i = legaia_scene_formats::field_disasm::decode(&bc, at).ok()?;
            if bc[at] == 0x4C && bc.get(at + 1).is_some_and(|b| b & 0xF0 == 0x70) {
                return Some(bc[at + 1] & 1 == 0);
            }
            if matches!(bc[at] & 0x7F, 0x21 | 0x3F) {
                return None;
            }
            at = match i.info {
                legaia_scene_formats::field_disasm::InsnInfo::JmpRel { target, .. } => target,
                _ => at + i.size.max(1),
            };
        }
        None
    };
    let painted: Vec<Option<bool>> = (0..count.min(4)).map(|k| paints(targets[k])).collect();
    let opens: Vec<usize> = (0..painted.len())
        .filter(|&k| painted[k] == Some(true))
        .collect();
    let pick = (opens.len() == 1 && painted.contains(&Some(false))).then(|| opens[0]);
    if let Some(k) = pick
        && std::env::var_os("LEGAIA_FGL_TRACE").is_some()
    {
        eprintln!(
            "      [pick] {} picker: option {k} opens the way",
            scene_name(session)
        );
    }
    pick
}

thread_local! {
    /// Set while the pad hand rides a lift out of a pocket
    /// ([`super::crossings::ride_out`]): a picker met then is the car's
    /// "ride?".
    pub(super) static RIDING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// While the hand rides a lift ([`RIDING`]), the answer that boards: the one
/// option whose branch does not just step the player back off the band.
/// `jouinb`'s lifts (P2[6] / P2[7] / P2[8]) ask before they move; their No
/// is a walk leg of one to three tiles off the band (`(26, 65)` to
/// `(27, 68)`, `(91, 44)` to `(92, 43)`), and their Yes walks the party into
/// the car and rides it to the floor the car flags `0x450` / `0x461` pick -
/// a branch the flag walk often cannot follow, so the refusal is what it
/// recognises.
pub(super) fn ride_option(session: &NativeSession) -> Option<usize> {
    if !RIDING.with(std::cell::Cell::get) {
        return None;
    }
    let (count, targets, bc) = open_picker(session)?;
    let w = &session.host.world;
    let (px, pz) = player_xz(session);
    let tile = dispatch_tile(px, pz);
    let rides: Vec<usize> = (0..count.min(4))
        .filter(|&k| {
            !flag_path_landing_with(&|f| w.system_flag_test(f), &bc, targets[k], Some(tile))
                .is_some_and(|c| {
                    let (cx, cz) = cell_center(c);
                    let (lx, lz) = dispatch_tile(cx, cz);
                    (lx - tile.0).abs() + (lz - tile.1).abs() <= 5
                })
        })
        .collect();
    let pick = (count >= 2 && rides.len() == 1).then(|| rides[0]);
    if let Some(k) = pick
        && std::env::var_os("LEGAIA_FGL_TRACE").is_some()
    {
        eprintln!(
            "      [pick] {} picker: option {k} boards the lift",
            scene_name(session)
        );
    }
    pick
}

/// The picker the modal timeline (or the first spawned record with a box)
/// has open: option count, branch targets and the record's bytes.
pub(super) fn open_picker(session: &NativeSession) -> Option<(usize, [usize; 4], Vec<u8>)> {
    use legaia_scene_formats::field_disasm::{InsnInfo, decode};
    let w = &session.host.world;
    let tl = w
        .cutscene
        .timeline
        .iter()
        .filter(|t| t.dialog.is_some())
        .chain(
            w.field_vm
                .helper_contexts
                .iter()
                .filter(|t| t.dialog.is_some()),
        )
        .next()?;
    let bc = tl.bytecode.as_slice();
    let mut pc = tl.pc;
    for _ in 0..16 {
        let Ok(i) = decode(bc, pc) else { break };
        if let InsnInfo::Picker { count, targets, .. } = i.info {
            return Some((count as usize, targets, bc.to_vec()));
        }
        pc += i.size.max(1);
    }
    None
}

/// While a three-actor talk holds the party split (`43 02`, `nilboa`'s Three
/// Tunnels), the option of a script's picker that keeps the split: the
/// first whose branch spawns no record (`0x44`) and changes no scene
/// (`0x3F`) before its first `0x21`. The tunnel mouths (`nilboa` P2[4] /
/// P2[5] / P2[6]) ask "go back outside?" and their Yes spawns P2[28], which
/// regroups the party and clears the members' progress flags; their No
/// steps the walker back. `None` outside a split, or when no option stays.
pub(super) fn split_stay_option(session: &NativeSession) -> Option<usize> {
    use legaia_scene_formats::field_disasm::{InsnInfo, decode};
    let w = &session.host.world;
    w.dialog.three_actor_talk.as_ref()?;
    let tl = w
        .cutscene
        .timeline
        .iter()
        .filter(|t| t.dialog.is_some())
        .chain(
            w.field_vm
                .helper_contexts
                .iter()
                .filter(|t| t.dialog.is_some()),
        )
        .next()?;
    let bc = tl.bytecode.as_slice();
    // The box's text runs from the record's PC up to the picker it opens.
    let mut pc = tl.pc;
    let mut picker = None;
    for _ in 0..16 {
        let Ok(i) = decode(bc, pc) else { break };
        if let InsnInfo::Picker { count, targets, .. } = i.info {
            picker = Some((count as usize, targets));
            break;
        }
        pc += i.size.max(1);
    }
    let (count, targets) = picker?;
    let stays = |mut at: usize| {
        for _ in 0..48 {
            let Ok(i) = decode(bc, at) else { return true };
            match bc[at] & 0x7F {
                0x44 | 0x3F => return false,
                0x21 => return true,
                _ => {}
            }
            at = match i.info {
                InsnInfo::JmpRel { target, .. } => target,
                _ => at + i.size.max(1),
            };
        }
        true
    };
    (0..count.min(4)).find(|&k| stays(targets[k]))
}

thread_local! {
    /// The scene the current hop is heading for, read by
    /// [`flag_window_pad`].
    pub(super) static HOP_DEST: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// The pad on an op-`49 04` flag-window picker (slot `0x23`, the Uru Mais
/// warp pads of `kor` / `kor3` / `kor4`). The picker opens on the pad the
/// party stands on and commits the highlighted row as the one set flag of
/// `base..base+count`; the record then branches on that flag to one scene
/// change per row. Confirming the default re-enters the pad's own floor, so
/// the hand reads the record's branch table, finds the row whose branch
/// names the hop's destination, moves the highlight there and confirms.
/// `None` when no flag window is up (or no row leads to the destination).
pub(super) fn flag_window_pad(session: &NativeSession) -> Option<u16> {
    use legaia_engine_core::field_submode_flag_window::{
        FLAG_WINDOW_SLOT, descriptor_from_operand,
    };
    use legaia_scene_formats::field_disasm::{InsnInfo, decode, scene_change_name};
    let w = &session.host.world;
    let screen = &w.field_vm.submode_screen;
    if !screen.open || screen.actor.state != FLAG_WINDOW_SLOT {
        return None;
    }
    let dest = HOP_DEST.with(|d| d.borrow().clone())?;
    let desc = descriptor_from_operand(&screen.op49_operand);
    let base = desc.base_flag;
    let count = i32::from(desc.count);
    let bc = &w.cutscene.timeline.as_ref()?.bytecode;
    // Each row's branch: the TEST of `base + row`, then the first scene
    // change its target reaches.
    let mut want = None;
    let mut pc = 0usize;
    while pc < bc.len() && want.is_none() {
        let Ok(insn) = decode(bc, pc) else {
            pc += 1;
            continue;
        };
        if let InsnInfo::SystemFlag {
            idx,
            target: Some(t),
            ..
        } = insn.info
            && (base..base + count).contains(&i32::from(idx))
        {
            let mut q = t;
            for _ in 0..64 {
                let Ok(i) = decode(bc, q) else { break };
                if let Some(name) = scene_change_name(bc, &i) {
                    if name == dest {
                        want = Some(i32::from(idx) - base);
                    }
                    break;
                }
                if i.size == 0 {
                    break;
                }
                q += i.size;
            }
        }
        pc += insn.size.max(1);
    }
    let want = want?;
    // The rows are drawn flipped (`flag_window_row_flip`): the highest flag
    // sits on the top row, so Up raises the selection. A row outside the
    // window's visible band cannot be reached from this pad.
    let sel = screen.flag_window.selection;
    let first = i32::from(desc.first_visible);
    if !(first..first + i32::from(desc.rows)).contains(&want) {
        return None;
    }
    Some(if sel < want {
        PadButton::Up.mask()
    } else if sel > want {
        PadButton::Down.mask()
    } else {
        PadButton::Cross.mask()
    })
}

/// The face button for the next symbol of an open op-`49 02` code lock
/// (slot `0x21`, `FUN_801EED58`). The code is the parked operand's five
/// bytes; a player who was told the password types it. Symbols are Cross
/// `0`, Circle `1`, Square `2`, Triangle `3` (the packed edge bits `0x40` /
/// `0x20` / `0x80` / `0x10`). `doman` P1[18], Dr. Usha's intercom, asks for
/// it once Sol's `koin2` P1[19] has raised `0x36D`; Cross on every symbol
/// is the wrong answer, `0x37D`. `None` when no lock is taking symbols.
pub(super) fn code_lock_pad(session: &NativeSession) -> Option<u16> {
    use legaia_engine_field::field_submode_code_lock::{CODE_LOCK_SLOT, code_from_operand};
    let screen = &session.host.world.field_vm.submode_screen;
    if !screen.is_open() || screen.actor.state != CODE_LOCK_SLOT || screen.code_lock.phase != 1 {
        return None;
    }
    let code = code_from_operand(&screen.op49_operand);
    let sym = *code.get(screen.code_lock.index)?;
    Some(
        match sym {
            0 => PadButton::Cross,
            1 => PadButton::Circle,
            2 => PadButton::Square,
            _ => PadButton::Triangle,
        }
        .mask(),
    )
}

/// The scenes a partition-2 record reaches through an op-`49 04` floor list
/// (empty for any other record). The record is followed from its start with
/// only the flags it sets itself raised, to the list it parks on - a pad
/// first raises its own floor and one of two list variants (`kor3` P2[9]
/// sets `0x136`, the full eight-floor list; P2[10] sets `0x137`, the
/// four-row list from row 4 that leaves out `kor`) - and a row counts only
/// inside that list's visible band. Each row's scene is the first named
/// scene change behind the record's test of `base + row`.
pub(super) fn flag_window_destinations(
    mf: &legaia_scene_formats::man_section::ManFile,
    man: &[u8],
    record: usize,
) -> BTreeSet<String> {
    use legaia_engine_core::field_submode_flag_window::descriptor_from_operand;
    use legaia_engine_field::man_field_scripts::partition_record_span;
    use legaia_scene_formats::field_disasm::{FlagKind, InsnInfo, decode, scene_change_name};
    let mut out = BTreeSet::new();
    let Some((start, pc0, len)) = partition_record_span(mf, man, 2, record) else {
        return out;
    };
    let body = &man[start..start + len];
    let mut raised: BTreeSet<u16> = BTreeSet::new();
    let mut pc = pc0;
    let mut list = None;
    for _ in 0..512 {
        let Ok(i) = decode(body, pc) else { break };
        match i.info {
            InsnInfo::StateResume { sub_op: 4, .. } => {
                list = Some(descriptor_from_operand(
                    body.get(pc + 1..pc + 7).unwrap_or(&[]),
                ));
                break;
            }
            InsnInfo::SystemFlag {
                kind: FlagKind::Set,
                idx,
                ..
            } => {
                raised.insert(idx);
            }
            InsnInfo::SystemFlag {
                kind: FlagKind::Clear,
                idx,
                ..
            } => {
                raised.remove(&idx);
            }
            InsnInfo::SystemFlag {
                kind: FlagKind::Test,
                idx,
                target: Some(t),
                ..
            } if raised.contains(&idx) => {
                pc = t;
                continue;
            }
            InsnInfo::JmpRel { target, .. } => {
                pc = target;
                continue;
            }
            InsnInfo::SceneChange { .. } => break,
            _ => {}
        }
        if i.size == 0 {
            break;
        }
        pc += i.size;
    }
    let Some(desc) = list else {
        return out;
    };
    let (base, first) = (desc.base_flag, i32::from(desc.first_visible));
    let visible = first..first + i32::from(desc.rows);
    let mut pc = 0usize;
    while pc < body.len() {
        let Ok(insn) = decode(body, pc) else {
            pc += 1;
            continue;
        };
        if let InsnInfo::SystemFlag {
            kind: FlagKind::Test,
            idx,
            target: Some(t),
            ..
        } = insn.info
            && visible.contains(&(i32::from(idx) - base))
        {
            let mut q = t;
            for _ in 0..64 {
                let Ok(i) = decode(body, q) else { break };
                if let Some(name) = scene_change_name(body, &i) {
                    out.insert(name.to_ascii_lowercase());
                    break;
                }
                if i.size == 0 {
                    break;
                }
                q += i.size;
            }
        }
        pc += insn.size.max(1);
    }
    out
}

/// Tick with Cross pulsed on a human duty cycle (edge-triggered pages advance
/// on a press), completing FMVs and auto-resolving battles, until a scene
/// change, release, or the budget.
///
/// `stop_on_release` ends the run as soon as nothing holds the pad (after
/// `EXIT_IDLE_TICKS` idle ticks); otherwise only a scene change ends it.
pub(super) fn run(session: &mut NativeSession, budget: usize, stop_on_release: bool) -> Run {
    let mut idle = 0usize;
    for f in 0..budget {
        if let Err(e) = pad_budget(session) {
            return Run::Parked(e);
        }
        if session.host.world.mode == SceneMode::Battle {
            // A random encounter the region roll raised while a script ran
            // (`jouinb` P2[6], the rapids, carries the party across rolling
            // regions) is fled as it is on a travel leg; anything a script
            // staged is fought.
            let flee = RANDOM_ROLL.with(|r| r.replace(false));
            FLEE_ENCOUNTERS.with(|f| f.set(flee));
            let r = drain_battle(session);
            FLEE_ENCOUNTERS.with(|f| f.set(false));
            if let Some(r) = r {
                return r;
            }
            continue;
        }
        // A save point or a ready check presses the menu button itself
        // (`World::scripted_menu_open_pending`), and `NativeSession::tick` opens
        // the menu; answer it the way a player who is not saving does.
        // A minigame the hand did not choose (a venue cabinet's talk, asked
        // and answered on the walk past it: `koin1`'s slot machines,
        // P1[51..56]) is left the way a player walks away from it: Start,
        // the engine's shared minigame exit (`World::poll_minigame_escape`),
        // which also closes the door-warp round trip back to the field.
        let pad = if session.host.world.in_minigame() {
            if f % 4 == 0 {
                PadButton::Start.mask()
            } else {
                0
            }
        } else {
            match scripted_menu_pad(session, f) {
                Some(p) => p,
                None => script_pad(session, f),
            }
        };
        // The naming prompt (the opening's op-0x49) takes this pad's edge
        // inside `NativeSession::tick`, as it does in both play hosts.
        session.host.world.set_pad(pad);
        let before = player_xz(session);
        let site = std::env::var_os("LEGAIA_FGL_POS_TRACE").map(|_| park_site(session));
        let roll_before = encounter_roll_state(session);
        let r = session.tick();
        note_caps(session);
        note_random_roll(session, roll_before);
        if let Some(site) = site {
            let after = player_xz(session);
            let now = park_site(session);
            if before != after || now != site {
                eprintln!("      [pos] {before:?} -> {after:?} at {site} -> {now}");
            }
        }
        match r {
            Ok(SceneTickEvent::SceneEntered { name }) => return Run::Entered(name),
            Ok(_) => {}
            Err(e) => return Run::Error(format!("{e:#}")),
        }
        service_shop(session);
        if session.host.world.active_fmv().is_some() {
            session.host.world.finish_cutscene();
            if let Some(FmvHandoffOutcome::Entered { scene, .. }) =
                session.apply_pending_fmv_handoff()
            {
                return Run::Entered(scene.to_string());
            }
        }
        if f >= 2 && released(session) {
            idle += 1;
            if stop_on_release && idle >= EXIT_IDLE_TICKS {
                session.host.world.set_pad(0);
                return Run::Released;
            }
        } else {
            idle = 0;
        }
    }
    session.host.world.set_pad(0);
    if released(session) {
        Run::Released
    } else {
        Run::Parked(format!("{} at {}", holder(session), park_site(session)))
    }
}

thread_local! {
    /// Set when the region roll (not a staged fight) raised the encounter
    /// whose transition is under way; consumed when its battle starts.
    pub(super) static RANDOM_ROLL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// What [`note_random_roll`] compares across one tick: whether the encounter
/// session sat idle, whether a scripted formation was armed, and the region
/// step counter (`World::encounters.step_counter`, written only by the
/// region arm of `World::on_field_step`).
pub(super) fn encounter_roll_state(session: &NativeSession) -> (bool, bool, i32) {
    use legaia_engine_battle::encounter::EncounterPhase;
    let e = &session.host.world.encounters;
    (
        e.session
            .as_ref()
            .is_none_or(|s| matches!(s.phase(), EncounterPhase::Idle)),
        e.scripted_formation_pending,
        e.step_counter,
    )
}

/// Mark [`RANDOM_ROLL`] when this tick's step left the idle session for a
/// transition through the region arm: no scripted formation was armed going
/// in (the scripted arm fires first and returns), and the region arm moved
/// the step counter.
pub(super) fn note_random_roll(
    session: &NativeSession,
    (idle, pending, counter): (bool, bool, i32),
) {
    let (idle_now, _, counter_now) = encounter_roll_state(session);
    if idle && !idle_now && !pending && counter_now != counter {
        RANDOM_ROLL.with(|r| r.set(true));
    }
}

/// The pad that closes a pause menu a script opened: back out of the save
/// screen, dismiss the notice panel, and answer the ready check Yes. Pressed
/// on alternate frames, because the menu reads edges. `None` while no menu is
/// up.
pub(super) fn scripted_menu_pad(session: &NativeSession, f: usize) -> Option<u16> {
    use legaia_engine_menus::field_menu::FieldMenuPhase;
    let menu = session.field_menu.as_ref()?;
    let button = if session.field_menu_sub.is_some() {
        PadButton::Circle
    } else {
        match menu.phase() {
            FieldMenuPhase::Notice => PadButton::Cross,
            FieldMenuPhase::ReadyConfirm { cursor: 0, .. } => PadButton::Cross,
            FieldMenuPhase::ReadyConfirm { .. } => PadButton::Left,
            _ => PadButton::Circle,
        }
    };
    Some(if f.is_multiple_of(2) {
        button.mask()
    } else {
        0
    })
}

/// Close a pause menu a script opened mid-walk (a save point the route
/// brushed), answering it with [`scripted_menu_pad`]. Returns whether one was
/// up.
pub(super) fn close_scripted_menu(session: &mut NativeSession) -> bool {
    if session.field_menu.is_none() {
        return false;
    }
    for g in 0..600 {
        let Some(pad) = scripted_menu_pad(session, g) else {
            break;
        };
        session.host.world.set_pad(pad);
        let _ = session.tick();
    }
    session.host.world.set_pad(0);
    true
}

/// [`run`] until a scene change or release, for as long as the pad holder's
/// park site keeps moving: a window of [`SETTLE_TICKS`] is re-granted
/// whenever the site changed across the last one, up to `ceiling` ticks in
/// all. A long cutscene is not a stall; a script that sits on one
/// instruction for a whole window is.
pub(super) fn run_while_moving(session: &mut NativeSession, ceiling: usize) -> Run {
    let mut spent = 0usize;
    loop {
        let before = park_site(session);
        let r = run(session, SETTLE_TICKS, true);
        spent += SETTLE_TICKS;
        match r {
            Run::Parked(p) => {
                if park_site(session) == before || spent >= ceiling {
                    return Run::Parked(p);
                }
            }
            other => return other,
        }
    }
}

thread_local! {
    /// Set while [`pad_hop`] drains a battle that interrupted its walk - a
    /// random encounter on a travel leg, which [`fight_pad`] flees.
    pub(super) static FLEE_ENCOUNTERS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// `(actor, party HP sum)` pairs whose item window held nothing worth
    /// using: the fighter does not reopen it until someone's HP moves.
    pub(super) static NO_ITEM: std::cell::RefCell<HashSet<(u8, u32)>> = Default::default();
    /// Actors whose Magic window held no affordable damage spell this battle.
    pub(super) static NO_MAGIC: std::cell::RefCell<HashSet<u8>> = Default::default();
    /// `(actor, party HP sum)` pairs whose Magic window could not cast the
    /// heal [`spell_heal`] asked for: not reopened for one until
    /// someone's HP moves.
    pub(super) static NO_HEAL_SPELL: std::cell::RefCell<HashSet<(u8, u32)>> = Default::default();
    /// Magic picks from the command ring that opened no spell window, per
    /// actor this battle ([`fight_pad`]).
    pub(super) static MAGIC_REFUSED: std::cell::RefCell<HashMap<u8, u32>> = Default::default();
    /// The foe round a solo duel last chose Spirit in
    /// ([`duel_wants_spirit`]): the round after it swings the gauge it built.
    pub(super) static DUEL_SPIRIT_ROUND: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}
