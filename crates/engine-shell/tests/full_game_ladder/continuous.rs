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
use legaia_engine_save_state::WorldSaveState;

/// Reloads a continuous run gets at one milestone's stretch after a party
/// wipe, the guard retry included: twice the pad pass's allowance. A player
/// keeps retrying, and one stream in fourteen takes twelve goes at Cort.
const CONTINUOUS_WIPE_ATTEMPTS: u32 = 2 * PAD_WIPE_ATTEMPTS;
/// Reloads a boss-door stretch - one that starts at a milestone reached with
/// a fight committed (`cort_chamber`) - gets for wipes before the boss is
/// beaten. Its own allowance, so a change to how often the boss wins moves
/// this count and not the walk on after it, which is counted apart from it
/// ([`CONTINUOUS_WIPE_ATTEMPTS`] of its own once [`BOSS_WON`] is set). Never
/// below the shared allowance, so a run that never runs it out plays the
/// same frames.
const BOSS_DOOR_WIPE_ATTEMPTS: u32 = CONTINUOUS_WIPE_ATTEMPTS;
/// Reloads a stretch takes for a walk that stalled with nothing holding the
/// player, before the stall ends the run.
const WALK_STALL_RELOADS: u32 = 2;

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
    /// The milestone the current stretch heads for (its spine id).
    pub(super) static STRETCH_TO: std::cell::RefCell<String> =
        const { std::cell::RefCell::new(String::new()) };
    /// The scenes the current stretch's `continuous_via` names: story the
    /// spine asks the run to finish there, so a story talk in another room
    /// of one is crossed to ([`pad_far_talks`]), not only an errand's.
    pub(super) static STORY_WAYPOINTS: std::cell::RefCell<BTreeSet<String>> =
        const { std::cell::RefCell::new(BTreeSet::new()) };
    /// Scenes whose arrival commits a fight, learned where a milestone was
    /// reached with one committed: the hop into one walks in ready
    /// ([`pad_ready`]), as a player who met the boss at that door does.
    pub(super) static FIGHT_ON_ARRIVAL: std::cell::RefCell<BTreeSet<String>> =
        const { std::cell::RefCell::new(BTreeSet::new()) };
    /// Set when a fight that forbids running ends with the party standing
    /// ([`super::fight_turn::drain_battle`]); cleared at the start of every
    /// attempt at a stretch. On a boss-door stretch it splits the attempt's
    /// failures: before it, a wipe draws on [`BOSS_DOOR_WIPE_ATTEMPTS`];
    /// after it, the walk on has an allowance of its own.
    pub(super) static BOSS_WON: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Set for a stretch whose milestone is a door with a fight committed
    /// behind it (`cort_chamber`): the walk to that door runs from the random
    /// encounters on the way ([`worth_fighting`]), as [`SAVING_FOR_BOSS`]
    /// does for a scene with a staged fight of its own.
    pub(super) static BOSS_LEAD_IN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The flags the order-checked anchors up to [`LEVEL_LOOKAHEAD`]
    /// milestones ahead carry between them: a chest whose flag is among
    /// them is one the retail run had opened by then, and the beats pass
    /// opens it where it stands ([`chest_ahead`]).
    pub(super) static CHESTS_AHEAD: std::cell::RefCell<BTreeSet<u16>> =
        const { std::cell::RefCell::new(BTreeSet::new()) };
}

/// Whether the continuous hand opens a chest that sets `flag`: the retail
/// run had it open within [`LEVEL_LOOKAHEAD`] milestones ([`CHESTS_AHEAD`]).
/// The segmented hands play to the next anchor only.
pub(super) fn chest_ahead(flag: u16) -> bool {
    PLAYS_ON.with(std::cell::Cell::get) && CHESTS_AHEAD.with(|c| c.borrow().contains(&flag))
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
    // So does the walk to a door the boss waits behind: `jouind`'s Puera
    // triples (F19, three 2000-HP casters) wiped a party at seven tenths on
    // the way to Cort's door and left a member at 1 HP from full, and each
    // such wipe reloaded the stretch before Cort was met.
    if (SAVING_FOR_BOSS.with(std::cell::Cell::get) || BOSS_LEAD_IN.with(std::cell::Cell::get))
        && !TRAINING.with(std::cell::Cell::get)
    {
        return false;
    }
    // Against a running escape clock (`4C D3`: the Floating Castle's six
    // minutes) a fight's length comes off the clock, and its return to the
    // field another twenty seconds (MAIN INIT `0x8003B62C`): a player runs.
    if w.battle.escape_timer.armed {
        return false;
    }
    let n = w.party.party_count.clamp(1, 3) as usize;
    // A member a quarter of the way to empty falls to the first round that
    // finds it, and the sum over the party hides that: `concnow` after
    // Zora's fight took a pair of foes on 443 + 366 + 2040 HP, and again on
    // 1 + 1 + 1708 once that fight's revive had set the fallen at 1 HP,
    // until a surprise round dropped the last member too. A player runs
    // with a member that low.
    if (0..n).any(|i| {
        let b = &w.actors[i].battle;
        b.max_hp > 0 && u32::from(b.hp) * 4 < u32::from(b.max_hp)
    }) {
        return false;
    }
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

/// Milestone `m` as the continuous run plays toward it: its own
/// waypoints, where the spine gives them.
fn continuous_target(m: &Milestone) -> Milestone {
    let mut m = m.clone();
    if let Some(v) = &m.continuous_via {
        m.via = v.clone();
    }
    m
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

/// Milestones a run keeps a fit save behind it while it holds no fitter one.
const FIT_SAVE_REACH: usize = 3;

/// Is the party one a player would walk on from: nobody under two fifths of its
/// HP, a third of its MP or the bag's restoratives to refill it, and the bag's
/// reserve of heals? A save that is not is a doomed save to reload again and
/// again: a run reached `jette_fortress_late` on 15% HP, no MP and an empty
/// bag, and six reloads replayed that same party into Jette (50000 HP). A scene
/// whose rest still works makes any party fit, since the save stands beside
/// what mends it: read otherwise, a run that wiped twice past a `town0c` save
/// went back through mist-era `dolk` with Vahn at level 2 and stopped there.
fn fit_to_go_on(session: &NativeSession) -> bool {
    if session.host.world.mode == SceneMode::Field && rest_works_here(session) {
        return true;
    }
    let w = &session.host.world;
    let party = w.present_party_list();
    let members = party.len().max(1) as u32;
    let (mp, max) = party
        .iter()
        .filter_map(|&m| w.party.roster.members.get(usize::from(m)))
        .map(|r| r.hp_mp_sp())
        .fold((0u32, 0u32), |(c, x), v| {
            (c + u32::from(v.mp_cur), x + u32::from(v.mp_max))
        });
    party_hp_permille(session) >= 400
        && (mp * 3 >= max || mp_items_held(w) >= members * MP_RESERVE)
        && heals_held(w) >= members * HEAL_RESERVE
}

/// Where `LEGAIA_FGL_CHECKPOINT_DIR` keeps milestone `id`'s checkpoint: the
/// engine's own save, and beside it the scene, the rand stream, the
/// learned exchange rate and the Field Move option.
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
    // The save names its own scene (the `LGX5` resume point), so a
    // checkpoint resumes with `legaia-engine --resume-save` alone.
    let resume = legaia_save::SaveResume {
        scene: cp.scene.clone(),
        location: String::new(),
    };
    if std::fs::write(&save, cp.save.write_with_resume(&resume)).is_err()
        || std::fs::write(&side, text).is_err()
    {
        eprintln!("    [checkpoint] could not write {}", save.display());
    }
}

/// The checkpoint an earlier run stored for milestone `id`, with its rand
/// stream and exchange rate put back. Field Move travels in the save itself
/// (the options words, SC `+0x58C`), as on a retail card.
fn stored_checkpoint(id: &str) -> Result<(Checkpoint, u32), String> {
    let (save, side) = checkpoint_paths(id).ok_or("LEGAIA_FGL_CHECKPOINT_DIR is not set")?;
    let bytes = std::fs::read(&save).map_err(|e| format!("{}: {e}", save.display()))?;
    let (save, _) =
        legaia_save::SaveFile::parse_with_resume(&bytes).map_err(|e| format!("{e:#}"))?;
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
    // Harness (reload): the game-over hold, the title and the card screen a
    // player passes are not played; the save round trip is
    // `native_new_game_save_continue`'s and the card tests'.
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
    // Every shortcut the pad hand takes on the run, by name ([`shortcut`]).
    let _ = take_shortcuts();
    let mut shortcut_total: BTreeMap<&'static str, u64> = BTreeMap::new();
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

    // The Field Move option each playthrough anchor was saved with (SC
    // `+0x58C`): the retail player walked chapter 1 and ran from Mt.
    // Rikuroa on. The engine keeps it as a host option, not save data, and
    // `jouind`'s timed switch pair is out of a walker's reach.
    let anchor_run: Vec<Option<bool>> = spine
        .iter()
        .map(|m| {
            m.anchor
                .as_ref()
                .filter(|_| m.order_check)
                .and_then(|r| load_anchor(&inp.library, r).ok())
                .map(|a| i32_at(&a.sc, SC_FIELD_MOVE) != 0)
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
    // The spine's own word for the scenes whose arrival commits a fight;
    // the run adds any it meets that way ([`FIGHT_ON_ARRIVAL`]).
    FIGHT_ON_ARRIVAL.with(|f| {
        let mut f = f.borrow_mut();
        f.clear();
        f.extend(
            spine
                .iter()
                .filter(|m| m.arrival_fight)
                .map(|m| m.scene.clone()),
        );
    });
    reset_shopping();
    reset_duel_dress();
    HEAL_HUNTED.with(|h| h.borrow_mut().clear());
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
        // Harness (deal): the checkpoint's own stream, as the run left it.
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
        // Harness (deal): the seed set's stream, no player state.
        session.host.world.rng_state = s;
    }

    let mut reached_count = 0usize;
    // The milestone whose state `cp` holds: behind the one last reached
    // when that one was reached with a fight committed (see below).
    let mut cp_at = start;
    // The latest save the party was fit to go on from ([`fit_to_go_on`]), and
    // its milestone: where a run goes back to when the latest save has lost
    // twice, as a player keeps an older slot.
    let mut fit_cp: Option<(Checkpoint, usize)> = None;
    let mut cp_fit = true;
    let mut reloads = 0u32;
    // The reloads spent on a boss-door stretch before its boss fell.
    let mut boss_reloads = 0u32;
    // Milestones reached with a fight committed: the stretch from one is a
    // boss-door stretch ([`BOSS_DOOR_WIPE_ATTEMPTS`]).
    let mut boss_doors: BTreeSet<usize> = spine
        .iter()
        .enumerate()
        .filter(|(_, m)| m.arrival_fight)
        .map(|(k, _)| k)
        .collect();
    let mut run_tried = false;
    // The stretches a stalled walk has already reloaded once ([`WALK_STALL_RELOADS`]).
    let mut stall_reloads: std::collections::HashMap<usize, u32> = std::collections::HashMap::new();
    let mut stall: Option<String> = None;
    // Attempts a stretch had used when a reload sent the run back behind
    // it, by stretch.
    // The third is the attempts that failed after the stretch's boss fell.
    let mut retries: BTreeMap<usize, (u32, Option<u8>, u32)> = BTreeMap::new();
    let mut next = start;
    'spine: while next < spine.len() - 1 {
        let i = next;
        next += 1;
        // The continuous run's own waypoints, where the spine gives them.
        let to_run = continuous_target(&spine[i + 1]);
        STORY_WAYPOINTS.with(|w| {
            let mut w = w.borrow_mut();
            w.clear();
            if spine[i + 1].continuous_via.is_some() {
                w.extend(to_run.via.iter().cloned());
            }
        });
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
        // An anchor that seeds nothing is a poke: it names the scene, not
        // what the retail run had played on the way there, so the beats of
        // its stretch steer by the save after it as well. `vell_fog_field`
        // is a Rim Elm-era state; Biron's first-visit chain (`bylon`
        // 0x297..0x2A0, every talk of which branches to the mist once the
        // West Voz tree's 0x2A1 is up) is in PRO-01 and must play before
        // the tree.
        let next_flags = if spine[i + 1].seeds_next {
            anchor_flags[i + 1].clone()
        } else {
            anchor_flags[i + 1].clone().map(|mut f| {
                f.extend(anchor_flags.get(i + 2).into_iter().flatten().flatten());
                f
            })
        };
        NEXT_ANCHOR_FLAGS.with(|a| *a.borrow_mut() = next_flags);
        CHESTS_AHEAD.with(|c| {
            let mut c = c.borrow_mut();
            c.clear();
            for k in i + 1..=(i + LEVEL_LOOKAHEAD).min(spine.len() - 1) {
                if spine[k].order_check
                    && let Some(f) = &anchor_flags[k]
                {
                    c.extend(f.iter().copied());
                }
            }
        });
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
        STRETCH_TO.with(|s| *s.borrow_mut() = to.id.clone());
        LEARNED_CADENCE.with(|l| l.set(None));
        // The furthest anchor in reach that records a party: the spine's
        // last milestones (the ending's debug-credits anchors) carry none,
        // and a lookahead that lands on one left the last stretches with no
        // target at all - the run met Songi in `noaru` at level 40 where
        // the retail run had grown to 50 by Biron's castle.
        let ahead = (i + 1..=(i + LEVEL_LOOKAHEAD).min(spine.len() - 1))
            .rev()
            .find(|&k| !anchor_levels[k].is_empty())
            .unwrap_or((i + LEVEL_LOOKAHEAD).min(spine.len() - 1));
        TARGET_LEVELS.with(|t| {
            *t.borrow_mut() = anchor_levels[ahead]
                .iter()
                .map(|&lv| if lv == u8::MAX { 0 } else { lv })
                .collect();
        });
        // A stretch played again after a reload from behind it keeps the
        // attempts it had used.
        let (mut attempt, mut prepare, mut walk_fails) = retries.remove(&i).unwrap_or((0, None, 0));
        let boss_door = boss_doors.contains(&i);
        BOSS_LEAD_IN.with(|b| b.set(boss_doors.contains(&(i + 1))));
        if attempt == 0 {
            RACE_LOST.with(|l| l.set(false));
        }
        let t0 = std::time::Instant::now();
        // Set when the last pass ended on a Field Move switch made in place
        // (see `run_retry` below): the next pass plays on in the same
        // session, on what is left of the same budget, and is not another
        // attempt.
        let mut switched_in_place = false;
        loop {
            if !std::mem::take(&mut switched_in_place) {
                attempt += 1;
                let budget = PAD_SEGMENT_FRAMES + to.via.len() as u64 * PAD_SEGMENT_FRAMES / 4;
                STRETCH_FRAMES.with(|f| f.set(budget));
                PAD_DEADLINE.with(|d| d.set(session.frames + budget));
                // This hand takes the fights it can win, so a stretch's
                // battles run long past the fleeing hand's allowance.
                PAD_BATTLE_LEFT.with(|l| l.set(4 * PAD_BATTLE_ALLOWANCE));
                BOSS_WON.with(|b| b.set(false));
            }
            LOST_TO_ELEMENT.with(|l| l.set(None));
            let played = catch_unwind(AssertUnwindSafe(|| {
                if let Some(element) = prepare {
                    pad_equip_guard(&mut session, element);
                }
                pad_equip_best(&mut session);
                if BOSS_LEAD_IN.with(std::cell::Cell::get) {
                    lead_in_heal(&mut session);
                }
                let mut trail = vec![scene_name(&session)];
                traverse(&mut session, &graph, to, true, &mut trail)
                    .map_err(|e| format!("{e} [trail {}]", trail.join(">")))
            }));
            let err = match played {
                Ok(Ok(())) => {
                    reached_count = reached_count.max(i + 1 - start);
                    // A milestone reached with a scripted fight committed and
                    // not yet begun is no state a player could save in: the
                    // save carries the flags the record set past its `3E FF`
                    // but not the fight, and a reload of it walks on as if
                    // the fight were won (`jouine` P2[5] commits Cort, and a
                    // reload from its arrival landed in the epilogue). The
                    // player's last save is still the one before.
                    if !session.host.world.host_save_allowed() {
                        FIGHT_ON_ARRIVAL.with(|f| f.borrow_mut().insert(scene_name(&session)));
                        boss_doors.insert(i + 1);
                        if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                            eprintln!(
                                "    [checkpoint] {} reached with a fight committed: keeping the {} save",
                                to.id, cp.scene
                            );
                        }
                    } else {
                        cp = checkpoint(&mut session);
                        cp_at = i + 1;
                        cp_fit = fit_to_go_on(&session);
                        if cp_fit
                            || fit_cp
                                .as_ref()
                                .is_none_or(|(_, at)| cp_at - at >= FIT_SAVE_REACH)
                        {
                            fit_cp = Some((checkpoint(&mut session), cp_at));
                        }
                    }
                    store_checkpoint(&to.id, &cp, &session);
                    if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                        print_state_diff(&mut session, &inp, to);
                    }
                    // The deal so far: the session's frame and the world
                    // rand state on arrival. Two runs that print the same
                    // pair here played the same hand to this milestone; the
                    // first milestone whose pair differs is where a change
                    // re-dealt the run ([`the_opening_deals_alike_in_two_processes`]).
                    eprintln!(
                        "[run {i:2}] {:24} -> {:24} reached ({:.1}s, attempt {attempt}) frame {} rng {:08X}",
                        spine[i].id,
                        to.id,
                        t0.elapsed().as_secs_f32(),
                        session.frames,
                        session.host.world.rng_state
                    );
                    let stretch = take_shortcuts();
                    if !stretch.is_empty() {
                        eprintln!("    {}", shortcut_line(&stretch));
                    }
                    for (k, n) in stretch {
                        *shortcut_total.entry(k).or_insert(0) += n;
                    }
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
            // A scripted game over (`taiku`'s escape clock run out, `4C EA`)
            // is a loss the player reloads from, as a wipe is.
            let wiped = err.contains("party wiped") || err.contains("scripted game over");
            let guard = LOST_TO_ELEMENT.with(std::cell::Cell::get);
            // A walking player stuck where the playthrough ran switches
            // Field Move to Run where it stands, once, and keeps running:
            // the option is a pause-menu toggle, not a reason to reload.
            // Turned on from the start of the playthrough's running stretch
            // it re-dealt the whole middle of the game (and a runner wedges
            // and pushes stones differently), so it waits for the stretch
            // that needs it: `jouind`'s switch pair. Reloading for it cost
            // the Cort stretch a second attempt on every dealt stream.
            let run_retry = !wiped
                && !run_tried
                && !session.host.world.locomotion.run_default
                && anchor_run[i + 1] == Some(true);
            // A walk that stalls where nothing holds the player - no wipe, no
            // script, the next cell open - is a player who reloads the last
            // save and plays the stretch again, the stream dealt another
            // hand. `jouinc`'s corridor to `jouind` stalled a walk west past
            // the P1[16] plug after two back-to-back fights on one deal and
            // walked on every other; ending the run there scored the deal,
            // not the stretch. Bounded per stretch, and every one is counted
            // in the reloads the headline prints.
            let stall_retry = !wiped
                && !run_retry
                && err.contains("pad walk stalled")
                && err.contains("tiles short of")
                && !err.contains("the player is held")
                && stall_reloads.get(&i).copied().unwrap_or(0) < WALK_STALL_RELOADS;
            if stall_retry {
                *stall_reloads.entry(i).or_insert(0) += 1;
                if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                    eprintln!(
                        "    [reload] {} -> {}: walk stalled, playing the stretch again: {err}",
                        spine[i].id, to.id
                    );
                }
            }
            if run_retry {
                run_tried = true;
                pad_field_move_run(&mut session);
                switched_in_place = true;
                if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                    eprintln!(
                        "    [run] {} -> {}: Field Move set to Run where the walk stalled: {err}",
                        spine[i].id, to.id
                    );
                }
                continue;
            }
            let again = match guard {
                Some(el) if wiped && prepare.is_none() => {
                    prepare = Some(el);
                    true
                }
                // A boss-door stretch counts its attempts in two parts: the
                // ones lost before the boss fell, against its own allowance,
                // and the ones lost on the walk after it, against a fresh
                // one. Elsewhere every attempt counts against the stretch's.
                _ if boss_door && BOSS_WON.with(std::cell::Cell::get) => {
                    walk_fails += 1;
                    (wiped && walk_fails < CONTINUOUS_WIPE_ATTEMPTS) || stall_retry
                }
                _ if boss_door => {
                    (wiped && attempt - walk_fails < BOSS_DOOR_WIPE_ATTEMPTS) || stall_retry
                }
                _ => (wiped && attempt < CONTINUOUS_WIPE_ATTEMPTS) || stall_retry,
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
            if boss_door && !BOSS_WON.with(std::cell::Cell::get) {
                boss_reloads += 1;
            }
            if boss_door && std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                eprintln!(
                    "    [boss door] {} -> {}: attempt {attempt}, {} lost to the boss, {walk_fails} after it",
                    spine[i].id,
                    to.id,
                    attempt - walk_fails
                );
            }
            if wiped
                && !cp_fit
                && attempt >= 2
                && let Some((fit, at)) = fit_cp.take_if(|(_, at)| *at < cp_at)
            {
                if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                    eprintln!(
                        "    [checkpoint] the {} save lost twice: back to {}'s",
                        cp.scene, spine[at].id
                    );
                }
                cp = fit;
                cp_at = at;
                cp_fit = true;
            }
            session = match reload(&inp, &cp, &opts) {
                Ok(s) => s,
                Err(e) => {
                    stall = Some(format!("{} -> {}: {e}", spine[i].id, to.id));
                    break 'spine;
                }
            };
            // The reload restores the save's own Field Move (the options
            // words travel with it, SC `+0x58C`); a player who switched to Run
            // switches again on a save from before the switch.
            if run_tried {
                pad_field_move_run(&mut session);
            }
            if !(attempt == 1 && prepare.is_some()) {
                // Harness (deal): the time a reload spends on screens the
                // hand does not play, as another hand of the stream.
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
            // A save from behind this stretch: the stretches between it
            // and here are played again, each as itself.
            if cp_at < i {
                retries.insert(i, (attempt, prepare, walk_fails));
                next = cp_at;
                continue 'spine;
            }
        }
    }

    PLAYS_ON.with(|p| p.set(false));
    BOSS_LEAD_IN.with(|b| b.set(false));
    STORY_WAYPOINTS.with(|w| w.borrow_mut().clear());
    STRETCH_FRAMES.with(|f| f.set(PAD_SEGMENT_FRAMES));
    TARGET_LEVELS.with(|t| t.borrow_mut().clear());
    STRETCH_TO.with(|s| s.borrow_mut().clear());
    PLAYED_AHEAD.with(|a| *a.borrow_mut() = None);
    CHESTS_AHEAD.with(|c| c.borrow_mut().clear());
    reset_errands();
    reset_duel_dress();
    let denom = spine.len() - 1 - start;
    eprintln!();
    eprintln!(
        "[headline] one continuous run from {} reaches {reached_count}/{denom} milestones on pad input, {reloads} reload(s) of its own saves ({boss_reloads} at boss doors)",
        spine[start].id
    );
    if let Some(s) = &stall {
        eprintln!("[stall] {s}");
    }
    for (k, n) in take_shortcuts() {
        *shortcut_total.entry(k).or_insert(0) += n;
    }
    eprintln!("{}", shortcut_line(&shortcut_total));
    let (steps, aimed) = super::fight_turn::AIM_TALLY.with(std::cell::Cell::take);
    {
        let w = &session.host.world;
        eprintln!(
            "[point card] held {}, bank {}, toast edges {}",
            w.point_card_held(),
            w.minigames.point_card,
            super::pad_shopping::POINT_TOASTS.with(std::cell::Cell::take)
        );
    }
    let (struck, missed) = super::fight_turn::AIM_DISPATCH.with(std::cell::Cell::take);
    eprintln!(
        "[aim] {steps} frame(s) asking the target cursor to step, {aimed} commit(s) aimed off the opening foe; {struck} strike(s) dispatched, {missed} at another foe than the one confirmed"
    );
    eprintln!();
    if from_id.is_some() || until.is_some() {
        return;
    }
    // A dealt hand is scored against its own row of the seed set when the
    // baseline has one, and against the default stream's score otherwise.
    let baseline = read_baseline();
    let seed_key = rng_seed_override().map(|s| format!("seed_{s}"));
    eprintln!("# paste into scripts/replays/full_game_baseline.toml when the score rises:");
    let want = match &seed_key {
        Some(key) => {
            eprintln!("[continuous_seeds]");
            eprintln!("{key} = {reached_count}");
            baseline.continuous_seeds.get(key).copied()
        }
        None => {
            eprintln!("[continuous]");
            eprintln!("reached = {reached_count}");
            None
        }
    }
    .or_else(|| baseline.continuous.get("reached").copied())
    .unwrap_or(0);
    assert!(
        reached_count >= want,
        "continuous run reached {reached_count} milestone(s), baseline {want}: {}",
        stall.as_deref().unwrap_or("no stall recorded")
    );
}

/// The deal is a function of the stream's seed, the pad and the code's
/// meaning, and of nothing else: two processes of one binary, run side by
/// side, play the opening stretches to the same frame and rand state at
/// every milestone. Each child draws its own `HashMap` keys and heap and
/// stack addresses, and the two contend for the same cores, so a hand or
/// engine path that followed hash order, an address, the wall clock or a
/// thread's timing shows up here as a fingerprint that differs.
///
/// A build that only moves code (a dead function, a line shift, an inlining
/// change) deals the same hand too; that takes two builds, so it is the
/// procedure in `docs/tooling/full-game-ladder.md#what-re-deals-a-seed`, not a test.
#[test]
fn the_opening_deals_alike_in_two_processes() {
    const CHILD: &str = "LEGAIA_FGL_DEAL_CHILD";
    if std::env::var_os(CHILD).is_some() || inputs().is_none() {
        return;
    }
    let exe = std::env::current_exe().expect("test executable");
    let spawn = || {
        let mut cmd = std::process::Command::new(&exe);
        cmd.args([
            "full_game_ladder::continuous::part_c_continuous_run",
            "--exact",
            "--nocapture",
            "--test-threads",
            "1",
        ])
        .env(CHILD, "1")
        .env("LEGAIA_FGL_CONTINUOUS_UNTIL", "drake_castle")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
        for var in [
            "LEGAIA_FGL_RNG_SEED",
            "LEGAIA_FGL_CONTINUOUS_FROM",
            "LEGAIA_FGL_CONTINUOUS_RESUME",
            "LEGAIA_FGL_CHECKPOINT_DIR",
        ] {
            cmd.env_remove(var);
        }
        cmd.spawn().expect("spawn the continuous run")
    };
    let children = [spawn(), spawn()];
    let deals: Vec<Vec<String>> = children
        .into_iter()
        .map(|child| {
            let out = child
                .wait_with_output()
                .expect("wait for the continuous run");
            String::from_utf8_lossy(&out.stderr)
                .lines()
                .filter(|l| l.starts_with("[run "))
                .map(|l| {
                    // Drop the wall-clock field, `12.3s, `.
                    match (l.find(" ("), l.find("s, attempt")) {
                        (Some(a), Some(b)) if a < b => format!("{} ({}", &l[..a], &l[b + 3..]),
                        _ => l.to_string(),
                    }
                })
                .collect()
        })
        .collect();
    assert!(
        deals[0].len() >= 4,
        "the opening run reached too few milestones to compare: {:?}",
        deals[0]
    );
    assert_eq!(
        deals[0], deals[1],
        "two processes of one binary dealt the opening differently"
    );
}
