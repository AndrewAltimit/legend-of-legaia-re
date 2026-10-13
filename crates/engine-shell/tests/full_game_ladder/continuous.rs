//! Part C: one continuous run. A cold New Game played toward the credits
//! on pad input, never re-seeded from a retail save.
//!
//! Part B starts every segment from the retail save taken at its milestone,
//! so each segment plays with the party, bag and gold a retail player had
//! there. That scores the doors and scripts of each stretch and says nothing
//! about whether the engine's own run arrives at a stretch able to clear it:
//! the levels it earned, what it bought, what it still holds. This pass
//! keeps one story state from the first frame. The only saves it loads are
//! ones the engine wrote itself, at each milestone it reached, and it loads
//! one only after a party wipe - the player's reload.
//!
//! The score is the count of milestones reached in order. It is expected to
//! sit well under part B's: the distance between the two is the measurement.

use super::*;

/// Reloads a continuous run gets at one milestone's stretch after a party
/// wipe, the guard retry included - the pad pass's allowance.
const CONTINUOUS_WIPE_ATTEMPTS: u32 = PAD_WIPE_ATTEMPTS;

thread_local! {
    /// Set for the continuous run: the hand plays to keep a party it will
    /// still be holding thirty milestones on, not to cross one stretch.
    /// It takes the random encounters it expects to win ([`worth_fighting`])
    /// and wears what it picks up ([`pad_equip_best`]). The segmented passes
    /// leave it clear and play exactly as before.
    pub(super) static PLAYS_ON: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// What the run's fights have cost so far: party HP lost and foe HP
    /// taken, summed over every battle. Their ratio is the price of a fight
    /// the hand has learned, which falls as the party levels and gears up.
    /// Opens on a lone level-1 member's first fights, a little over one HP
    /// lost for each one dealt.
    pub(super) static EXCHANGE: std::cell::Cell<(u64, u64)> = const { std::cell::Cell::new((110, 100)) };
}

/// Is the random encounter on the table one the continuous hand takes? A
/// player who means to finish the game fights what it can beat: that is
/// where levels and gold come from, and a party that flees everything
/// reaches the first boss at level 1.
///
/// The read is the one a player makes at the round prompt: what the foes
/// still hold, what fights have cost so far ([`EXCHANGE`]), and what the
/// party has left. Foes act until they fall, so a group of `n` costs about
/// `(n + 1) / 2` times what its HP alone would. The fight is taken while
/// that cost is under three fifths of the party's remaining HP, and it is
/// asked again every round - a fight going badly turns back into a run.
pub(super) fn worth_fighting(w: &legaia_engine_core::world::World) -> bool {
    if !PLAYS_ON.with(std::cell::Cell::get) {
        return false;
    }
    let n = w.party.party_count.clamp(1, 3) as usize;
    let party: u64 = (0..n).map(|i| u64::from(w.actors[i].battle.hp)).sum();
    let foes: Vec<u64> = w
        .actors
        .iter()
        .skip(n)
        .filter(|a| a.battle.max_hp > 0 && a.battle.hp > 0)
        .map(|a| u64::from(a.battle.hp))
        .collect();
    if foes.is_empty() {
        return false;
    }
    let (lost, dealt) = EXCHANGE.with(std::cell::Cell::get);
    let cost = foes.iter().sum::<u64>() * lost / dealt.max(1) * (foes.len() as u64 + 1) / 2;
    cost * 5 <= party * 3
}

/// The engine's own save at the last milestone reached.
struct Checkpoint {
    save: legaia_save::SaveFile,
    scene: String,
}

fn checkpoint(session: &mut NativeSession) -> Checkpoint {
    Checkpoint {
        save: session.host.world.save_full(),
        scene: scene_name(session),
    }
}

/// A fresh session resumed from `cp`, as a card load of that save.
fn reload(inp: &Inputs, cp: &Checkpoint, opts: &FieldLiveOpts) -> Result<NativeSession, String> {
    let mut session = open_session(&inp.extracted);
    session.host.world.load_full(cp.save.clone());
    let landing = session.resume_save(cp.save.clone(), &cp.scene, opts);
    if !landing.entered_scene() {
        return Err(format!("reload landed {landing:?}"));
    }
    Ok(session)
}

/// One save's carried state, a line per member: what a stretch is played
/// with. Ids only, no names.
fn carried(sf: &legaia_save::SaveFile) -> String {
    let mut out = String::new();
    for (i, m) in sf.party.members.iter().enumerate() {
        let v = m.hp_mp_sp();
        if v.hp_max == 0 {
            continue;
        }
        out.push_str(&format!(
            "      member {i}: lv {} xp {} hp {}/{} mp {}/{} equip {:02X?}\n",
            m.level(),
            m.cumulative_xp(),
            v.hp_cur,
            v.hp_max,
            v.mp_cur,
            v.mp_max,
            m.equipment().slots
        ));
    }
    let bag: Vec<String> = sf
        .ext
        .inventory
        .iter()
        .map(|(id, n)| format!("{id:02X}x{n}"))
        .collect();
    out.push_str(&format!(
        "      roster {:?} gold {} bag [{}]",
        sf.ext_v2.active_party,
        sf.ext.money,
        bag.join(" ")
    ));
    out
}

/// The engine's own state beside the retail anchor's for milestone `m`: the
/// diff that tells an engine defect (a flag, a member or an item the run
/// should hold) from a party a retail player had grown further.
fn print_state_diff(session: &mut NativeSession, inp: &Inputs, m: &Milestone) {
    let mine = session.host.world.save_full();
    eprintln!("    [state] engine, against the {} anchor:", m.id);
    eprintln!("{}", carried(&mine));
    let Some(a) = m
        .anchor
        .as_ref()
        .and_then(|r| load_anchor(&inp.library, r).ok())
    else {
        return;
    };
    if let Ok(sf) =
        legaia_save::SaveFile::from_retail_sc_block(&a.sc, legaia_save::RETAIL_SC_PARTY_RECORDS)
    {
        eprintln!("    [state] retail anchor at {}:", m.id);
        eprintln!("{}", carried(&sf));
    }
    let have = flags_of_world(session);
    let show = |s: Vec<&u16>| {
        s.iter()
            .map(|f| format!("0x{f:03X}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    eprintln!(
        "    [state] flags the anchor has and the engine lacks: {}",
        show(a.flags.difference(&have).collect())
    );
    eprintln!(
        "    [state] flags the engine has and the anchor lacks: {}",
        show(have.difference(&a.flags).collect())
    );
}

#[test]
fn part_c_continuous_run() {
    let Some(inp) = inputs() else { return };
    let spine = load_spine();
    let index = ProtIndex::open_extracted(&inp.extracted).expect("open ProtIndex");
    let graph = DiscGraph::build(&index);
    // The next anchor's flags tell the beats pass which story beats the
    // retail run played before the milestone. They are route knowledge, not
    // state: nothing of the anchor's party or bag reaches the session.
    let anchor_flags: Vec<Option<BTreeSet<u16>>> = spine
        .iter()
        .map(|m| {
            m.anchor
                .as_ref()
                .and_then(|r| load_anchor(&inp.library, r).ok())
                .map(|a| a.flags)
        })
        .collect();

    let opts = live_opts(true);
    PLAYS_ON.with(|p| p.set(true));
    let mut session = open_session(&inp.extracted);
    // `LEGAIA_FGL_CONTINUOUS_FROM=<milestone>` starts the run at a later
    // milestone, seeded once from that milestone's retail anchor and never
    // again. It is a probe, not the score: the party it sets out with is
    // retail's, so the baseline is not asserted.
    let from_id = std::env::var("LEGAIA_FGL_CONTINUOUS_FROM").ok();
    let start = from_id.as_deref().map_or(0, |id| {
        spine
            .iter()
            .position(|m| m.id == id)
            .unwrap_or_else(|| panic!("LEGAIA_FGL_CONTINUOUS_FROM: no milestone `{id}`"))
    });
    let seed_at = (0..=start)
        .rev()
        .find(|&j| spine[j].seeds_next)
        .unwrap_or(start);
    let seed_anchor = spine[seed_at]
        .anchor
        .as_ref()
        .and_then(|r| load_anchor(&inp.library, r).ok());
    if let Err(e) = seed(&mut session, &spine[seed_at], seed_anchor.as_ref(), &opts) {
        panic!("continuous run: {} did not seed: {e}", spine[seed_at].id);
    }
    let mut cp = checkpoint(&mut session);

    let mut reached_count = 0usize;
    let mut reloads = 0u32;
    let mut stall: Option<String> = None;
    'spine: for i in start..spine.len() - 1 {
        let to = &spine[i + 1];
        NEXT_ANCHOR_FLAGS.with(|a| *a.borrow_mut() = anchor_flags[i + 1].clone());
        LEARNED_CADENCE.with(|l| l.set(None));
        let mut prepare: Option<u8> = None;
        let mut attempt = 0u32;
        let t0 = std::time::Instant::now();
        loop {
            attempt += 1;
            LOST_TO_ELEMENT.with(|l| l.set(None));
            let played = catch_unwind(AssertUnwindSafe(|| {
                PAD_DEADLINE.with(|d| d.set(session.frames + PAD_SEGMENT_FRAMES));
                PAD_BATTLE_LEFT.with(|l| l.set(PAD_BATTLE_ALLOWANCE));
                if let Some(element) = prepare {
                    pad_equip_guard(&mut session, element);
                }
                pad_equip_best(&mut session);
                let mut trail = vec![scene_name(&session)];
                traverse(&mut session, &graph, to, true, &mut trail)
                    .map_err(|e| format!("{e} [trail {}]", trail.join(">")))
            }));
            let err = match played {
                Ok(Ok(())) => {
                    reached_count = i + 1 - start;
                    cp = checkpoint(&mut session);
                    if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                        print_state_diff(&mut session, &inp, to);
                    }
                    eprintln!(
                        "[run {i:2}] {:24} -> {:24} reached ({:.1}s, attempt {attempt})",
                        spine[i].id,
                        to.id,
                        t0.elapsed().as_secs_f32()
                    );
                    continue 'spine;
                }
                Ok(Err(e)) => e,
                Err(p) => {
                    stall = Some(format!(
                        "{} -> {}: PANIC: {}",
                        spine[i].id,
                        to.id,
                        panic_text(&*p)
                    ));
                    break 'spine;
                }
            };
            let wiped = err.contains("party wiped");
            let guard = LOST_TO_ELEMENT.with(std::cell::Cell::get);
            let again = match guard {
                Some(el) if wiped && prepare.is_none() => {
                    prepare = Some(el);
                    true
                }
                _ => wiped && attempt < CONTINUOUS_WIPE_ATTEMPTS,
            };
            if !again {
                print_state_diff(&mut session, &inp, to);
                stall = Some(format!(
                    "{} -> {} (attempt {attempt}): {err}",
                    spine[i].id, to.id
                ));
                break 'spine;
            }
            // The player's reload: the engine's own save at the last
            // milestone, and another hand of the rand stream unless this is
            // the guard retry (which measures the guard alone).
            reloads += 1;
            session = match reload(&inp, &cp, &opts) {
                Ok(s) => s,
                Err(e) => {
                    stall = Some(format!("{} -> {}: {e}", spine[i].id, to.id));
                    break 'spine;
                }
            };
            if !(attempt == 1 && prepare.is_some()) {
                let w = &mut session.host.world;
                w.rng_state = w
                    .rng_state
                    .wrapping_add((attempt + 1).wrapping_mul(0x9E37_79B9));
            }
        }
    }

    PLAYS_ON.with(|p| p.set(false));
    let denom = spine.len() - 1 - start;
    eprintln!();
    eprintln!(
        "[headline] one continuous run from {} reaches {reached_count}/{denom} milestones on pad input, {reloads} reload(s) of its own saves",
        spine[start].id
    );
    if let Some(s) = &stall {
        eprintln!("[stall] {s}");
    }
    eprintln!();
    if from_id.is_some() {
        return;
    }
    eprintln!("# paste into scripts/replays/full_game_baseline.toml when the score rises:");
    eprintln!("[continuous]");
    eprintln!("reached = {reached_count}");

    let want = read_baseline()
        .continuous
        .get("reached")
        .copied()
        .unwrap_or(0);
    assert!(
        reached_count >= want,
        "continuous run reached {reached_count} milestone(s), baseline {want}: {}",
        stall.as_deref().unwrap_or("no stall recorded")
    );
}
