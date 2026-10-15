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
    /// It takes the random encounters it expects to win ([`worth_fighting`]),
    /// shops ([`pad_shop`]), wears what it buys and finds
    /// ([`pad_equip_best`]), rests ([`pad_rest`]), plays each scene's story
    /// on arrival and trains from a bed ([`pad_train`]). The segmented
    /// passes leave it clear.
    pub(super) static PLAYS_ON: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// What the run's fights have cost so far: party HP lost and foe HP
    /// taken, summed over every battle. Their ratio is the price of a fight
    /// the hand has learned, which falls as the party levels and gears up.
    /// Opens on a lone level-1 member's first fights, a little over one HP
    /// lost for each one dealt.
    /// The scene of the milestone after next, and the flags the next two
    /// anchors carry between them: what the continuous hand's beats pass
    /// plays toward when it stands in that scene early.
    pub(super) static PLAYED_AHEAD: std::cell::RefCell<Option<(String, BTreeSet<u16>)>> =
        const { std::cell::RefCell::new(None) };
    pub(super) static EXCHANGE: std::cell::Cell<(u64, u64)> = const { std::cell::Cell::new((110, 100)) };
    /// Set while the beats pass plays a scene whose story waits behind a
    /// staged fight on a walk-on band: [`worth_fighting`] then takes no
    /// random encounter.
    pub(super) static SAVING_FOR_BOSS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The scenes the current stretch's `continuous_via` names: story the
    /// spine asks the run to finish there, so a story talk in another room
    /// of one is crossed to ([`pad_far_talks`]), not only an errand's.
    pub(super) static STORY_WAYPOINTS: std::cell::RefCell<BTreeSet<String>> =
        const { std::cell::RefCell::new(BTreeSet::new()) };
}

/// Holds [`SAVING_FOR_BOSS`] for a scope and puts back what was there.
pub(super) struct SavingForBoss(bool);

impl SavingForBoss {
    pub(super) fn arm(on: bool) -> Self {
        Self(SAVING_FOR_BOSS.with(|s| s.replace(on)))
    }
}

impl Drop for SavingForBoss {
    fn drop(&mut self) {
        SAVING_FOR_BOSS.with(|s| s.set(self.0));
    }
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
    // On the way to a boss the party's HP and the bag are the boss's: a
    // walk there runs from what it meets. Training is the exception - its
    // outings are for the fights.
    if SAVING_FOR_BOSS.with(std::cell::Cell::get) && !TRAINING.with(std::cell::Cell::get) {
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

/// Where `LEGAIA_FGL_CHECKPOINT_DIR` keeps milestone `id`'s checkpoint: the
/// engine's own save, and beside it the scene, the rand stream and the
/// learned exchange rate.
fn checkpoint_paths(id: &str) -> Option<(PathBuf, PathBuf)> {
    let dir = PathBuf::from(std::env::var_os("LEGAIA_FGL_CHECKPOINT_DIR")?);
    Some((
        dir.join(format!("{id}.lgsf")),
        dir.join(format!("{id}.txt")),
    ))
}

/// Keep `cp` on disk for a later `LEGAIA_FGL_CONTINUOUS_RESUME`. The file is
/// a save of disc-derived state, so the directory belongs under `target/`.
fn store_checkpoint(id: &str, cp: &Checkpoint, session: &NativeSession) {
    let Some((save, side)) = checkpoint_paths(id) else {
        return;
    };
    let (lost, dealt) = EXCHANGE.with(std::cell::Cell::get);
    let text = format!(
        "{}\n{}\n{lost}\n{dealt}\n",
        cp.scene, session.host.world.rng_state
    );
    if let Some(dir) = save.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::write(&save, cp.save.write()).is_err() || std::fs::write(&side, text).is_err() {
        eprintln!("    [checkpoint] could not write {}", save.display());
    }
}

/// The checkpoint an earlier run stored for milestone `id`, with its rand
/// stream and exchange rate put back.
fn stored_checkpoint(id: &str) -> Result<(Checkpoint, u32), String> {
    let (save, side) = checkpoint_paths(id).ok_or("LEGAIA_FGL_CHECKPOINT_DIR is not set")?;
    let bytes = std::fs::read(&save).map_err(|e| format!("{}: {e}", save.display()))?;
    let save = legaia_save::SaveFile::parse(&bytes).map_err(|e| format!("{e:#}"))?;
    let text = std::fs::read_to_string(&side).map_err(|e| format!("{}: {e}", side.display()))?;
    let mut lines = text.lines();
    let scene = lines.next().unwrap_or_default().to_string();
    let mut num = || {
        lines
            .next()
            .and_then(|l| l.parse::<u64>().ok())
            .unwrap_or(0)
    };
    let rng = num() as u32;
    let (lost, dealt) = (num(), num());
    if dealt > 0 {
        EXCHANGE.with(|e| e.set((lost, dealt)));
    }
    Ok((Checkpoint { save, scene }, rng))
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
        let s = m.live_stats();
        let r = m.record_stats();
        out.push_str(&format!(
            "      member {i}: lv {} xp {} hp {}/{} mp {}/{} agl {}/{} atk {}/{} spd {}/{} equip {:02X?}\n",
            m.level(),
            m.cumulative_xp(),
            v.hp_cur,
            v.hp_max,
            v.mp_cur,
            v.mp_max,
            s.agl,
            r.agl,
            s.atk,
            r.atk,
            s.spd,
            r.spd,
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
    let census = system_flag_census(&index, index.cdname_scene_names());
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

    // The level each character holds in each anchor, read as a player's
    // own growth curve: a line counts only as far as every later anchor
    // agrees with it (several chapter-1 anchors are other sessions).
    let mut anchor_levels: Vec<Vec<u8>> = spine
        .iter()
        .map(|m| {
            // An anchor outside the order check is not a point on the
            // playthrough's curve (a door-tile poke, the debug credits).
            m.anchor
                .as_ref()
                .filter(|_| m.order_check)
                .and_then(|r| load_anchor(&inp.library, r).ok())
                .and_then(|a| {
                    legaia_save::SaveFile::from_retail_sc_block(
                        &a.sc,
                        legaia_save::RETAIL_SC_PARTY_RECORDS,
                    )
                    .ok()
                })
                .map(|sf| {
                    sf.party
                        .members
                        .iter()
                        .map(|m| {
                            if m.hp_mp_sp().hp_max == 0 {
                                u8::MAX
                            } else {
                                m.level()
                            }
                        })
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect();
    for j in (0..anchor_levels.len().saturating_sub(1)).rev() {
        let next = anchor_levels[j + 1].clone();
        for (c, lv) in anchor_levels[j].iter_mut().enumerate() {
            *lv = (*lv).min(next.get(c).copied().unwrap_or(u8::MAX));
        }
        if anchor_levels[j].is_empty() {
            anchor_levels[j] = next;
        }
    }

    let opts = live_opts(true);
    PLAYS_ON.with(|p| p.set(true));
    reset_shopping();
    let mut session = open_session(&inp.extracted);
    // `LEGAIA_FGL_CONTINUOUS_FROM=<milestone>` starts the run at a later
    // milestone, seeded once from that milestone's retail anchor and never
    // again. It is a probe, not the score: the party it sets out with is
    // retail's, so the baseline is not asserted.
    // `LEGAIA_FGL_CONTINUOUS_RESUME=<milestone>` starts it there too, from
    // the engine's own save an earlier run kept under
    // `LEGAIA_FGL_CHECKPOINT_DIR`: the party is the run's, the picker
    // rotation and the shopping memory are not, so it is a probe as well.
    let resume_id = std::env::var("LEGAIA_FGL_CONTINUOUS_RESUME").ok();
    // `LEGAIA_FGL_CONTINUOUS_UNTIL=<milestone>` ends the run once it reaches
    // that milestone: a probe of one stretch, so the baseline is not asserted.
    let until = std::env::var("LEGAIA_FGL_CONTINUOUS_UNTIL").ok();
    let from_id = resume_id
        .clone()
        .or_else(|| std::env::var("LEGAIA_FGL_CONTINUOUS_FROM").ok());
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
    let mut cp = if let Some(id) = &resume_id {
        let (cp, rng) =
            stored_checkpoint(id).unwrap_or_else(|e| panic!("LEGAIA_FGL_CONTINUOUS_RESUME: {e}"));
        session = reload(&inp, &cp, &opts)
            .unwrap_or_else(|e| panic!("LEGAIA_FGL_CONTINUOUS_RESUME: {e}"));
        session.host.world.rng_state = rng;
        cp
    } else {
        if let Err(e) = seed(&mut session, &spine[seed_at], seed_anchor.as_ref(), &opts) {
            panic!("continuous run: {} did not seed: {e}", spine[seed_at].id);
        }
        checkpoint(&mut session)
    };
    // Another hand of the rand stream from the first frame (or from the
    // resumed checkpoint): how a score is checked for resting on one deal.
    if let Some(s) = rng_seed_override() {
        session.host.world.rng_state = s;
    }

    let mut reached_count = 0usize;
    let mut reloads = 0u32;
    let mut stall: Option<String> = None;
    'spine: for i in start..spine.len() - 1 {
        // The continuous run's own waypoints, where the spine gives them.
        let mut to_run = spine[i + 1].clone();
        STORY_WAYPOINTS.with(|w| w.borrow_mut().clear());
        if let Some(v) = &to_run.continuous_via {
            to_run.via = v.clone();
            STORY_WAYPOINTS.with(|w| w.borrow_mut().extend(v.iter().cloned()));
        }
        let to = &to_run;
        // What the retail run had played by the next milestone and the one
        // after it: chapter 1's anchors come from several sessions, and a
        // stretch whose own anchor is another session's (the `ravine` state)
        // would otherwise walk past the beats the next one records.
        // Only in that later milestone's own scene: a run that stands there
        // early is already where its stretch begins.
        let played = spine.get(i + 2).and_then(|m| {
            let mut flags = anchor_flags[i + 1].clone()?;
            flags.extend(anchor_flags[i + 2].as_ref()?);
            Some((m.scene.clone(), flags))
        });
        PLAYED_AHEAD.with(|a| *a.borrow_mut() = played);
        NEXT_ANCHOR_FLAGS.with(|a| *a.borrow_mut() = anchor_flags[i + 1].clone());
        // The story items the stretch consumes ([`stretch_errands`]): what
        // its waypoints' and milestone scene's records take from the bag on
        // the way to a flag the next anchor carries.
        reset_errands();
        let errands = anchor_flags[i + 1]
            .as_ref()
            .map(|wanted| {
                let scenes: BTreeSet<&str> = to
                    .via
                    .iter()
                    .map(String::as_str)
                    .chain([to.scene.as_str()])
                    .collect();
                stretch_errands(&index, &census, wanted, &scenes)
            })
            .unwrap_or_default();
        if !errands.is_empty() && std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
            eprintln!(
                "    [errand] {}: {}",
                to.id,
                errands
                    .iter()
                    .map(|e| format!("item 0x{:02X} x{}", e.item, e.sites.len()))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        ERRANDS.with(|e| *e.borrow_mut() = errands);
        LEARNED_CADENCE.with(|l| l.set(None));
        let ahead = (i + LEVEL_LOOKAHEAD).min(spine.len() - 1);
        TARGET_LEVELS.with(|t| {
            *t.borrow_mut() = anchor_levels[ahead]
                .iter()
                .map(|&lv| if lv == u8::MAX { 0 } else { lv })
                .collect();
        });
        let mut prepare: Option<u8> = None;
        let mut attempt = 0u32;
        let t0 = std::time::Instant::now();
        loop {
            attempt += 1;
            LOST_TO_ELEMENT.with(|l| l.set(None));
            let played = catch_unwind(AssertUnwindSafe(|| {
                let budget = PAD_SEGMENT_FRAMES + to.via.len() as u64 * PAD_SEGMENT_FRAMES / 4;
                STRETCH_FRAMES.with(|f| f.set(budget));
                PAD_DEADLINE.with(|d| d.set(session.frames + budget));
                // This hand takes the fights it can win, so a stretch's
                // battles run long past the fleeing hand's allowance.
                PAD_BATTLE_LEFT.with(|l| l.set(4 * PAD_BATTLE_ALLOWANCE));
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
                    store_checkpoint(&to.id, &cp, &session);
                    if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                        print_state_diff(&mut session, &inp, to);
                    }
                    eprintln!(
                        "[run {i:2}] {:24} -> {:24} reached ({:.1}s, attempt {attempt})",
                        spine[i].id,
                        to.id,
                        t0.elapsed().as_secs_f32()
                    );
                    if until.as_deref() == Some(to.id.as_str()) {
                        break 'spine;
                    }
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
            // The player's Field Move option survives the reload: retail
            // keeps it in the save block (`0x800846CC`, SC `+0x58C`), and the
            // engine as a host option the fresh session would otherwise
            // reset to Walk - which misses `jouind`'s 50-vsync switch pair.
            let run_default = session.host.world.locomotion.run_default;
            session = match reload(&inp, &cp, &opts) {
                Ok(s) => s,
                Err(e) => {
                    stall = Some(format!("{} -> {}: {e}", spine[i].id, to.id));
                    break 'spine;
                }
            };
            session.host.world.locomotion.run_default = run_default;
            if !(attempt == 1 && prepare.is_some()) {
                let w = &mut session.host.world;
                w.rng_state = w
                    .rng_state
                    .wrapping_add((attempt + 1).wrapping_mul(0x9E37_79B9));
            }
            // A dealt hand reaches the reloads too: the reload's stream is
            // the save's, the same under every deal otherwise.
            if let Some(s) = rng_seed_override() {
                let w = &mut session.host.world;
                w.rng_state ^= s;
            }
        }
    }

    PLAYS_ON.with(|p| p.set(false));
    STORY_WAYPOINTS.with(|w| w.borrow_mut().clear());
    STRETCH_FRAMES.with(|f| f.set(PAD_SEGMENT_FRAMES));
    TARGET_LEVELS.with(|t| t.borrow_mut().clear());
    PLAYED_AHEAD.with(|a| *a.borrow_mut() = None);
    reset_errands();
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
    if from_id.is_some() || until.is_some() {
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
