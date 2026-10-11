//! The ladder's rungs, tick budgets, marks and `score_scene`: one scene's verdict.

use super::*;

// ---------------------------------------------------------------------------
// Rungs
// ---------------------------------------------------------------------------

pub(super) const RUNGS: [&str; 6] = ["assets", "man", "enter", "script", "walk", "exit"];

/// Ticks a scene's entry script gets to settle before it is called parked.
/// Comfortable headroom over the longest first-visit record the sweep meets
/// (izumi's spring choreography needs ~1500 sim ticks; those scenes report a
/// scripted departure instead and never reach the cap).
pub(super) const SETTLE_TICKS: usize = 2800;

/// Ticks each of the four pad directions is held in the walk probe.
pub(super) const WALK_TICKS: usize = 60;

/// Ticks an exit gets to fire after the player is seated on its trigger tile.
///
/// A door record is not a handful of ops. Retail's exit records open with a
/// fade, a camera beat and several authored `0x4A WAIT_FRAMES` holds before
/// their trailing `0x3F`: `urudre1` `P2[2]` alone waits 240 + 60 + 60 frames.
/// The old 24-tick budget could not reach a single one of those tails, so
/// "the exit did not fire" measured the budget rather than the disc. The
/// budget is only *spent* when a record is actually running -
/// [`run_to_transition`] returns as soon as the world goes idle - so a tile
/// that spawns nothing still costs a handful of ticks.
pub(super) const EXIT_TICKS: usize = 2_400;

/// The same budget for the deep records Part E drives: `urudre3` `P2[0]` needs
/// ~14.9k ticks to reach its `0x3F` and `jouine` `P2[16]` ~8.6k to reach its
/// FMV hand-off (both are boss-cutscene-length records whose exit is the tail).
pub(super) const DEEP_EXIT_TICKS: usize = 24_000;

/// Consecutive idle ticks (no cutscene timeline, no helper context, no
/// dialogue, no FMV) that end a post-step wait early. Without it every quiet
/// tile would cost the full budget.
pub(super) const EXIT_IDLE_TICKS: usize = 4;

/// Exit sites tried per scene before the rung is called failed. Sites are
/// tried in `.MAP` trigger order and the first success wins.
///
/// Four was a sample, not a bound: `uru` carries several `urudre*` doors
/// ahead of the `map03` door Part E finds at trigger position 64 of its 118,
/// so the rung reported "nothing fired" about doors it never tried. A site
/// that fires ends the loop, and a quiet tile returns in a handful of ticks
/// ([`run_to_transition`] leaves as soon as the world goes idle), so the
/// ceiling costs nothing on the scenes whose first door works.
pub(super) const EXIT_SITES_TRIED: usize = 16;

/// Walk-on tiles swept per scene in Part E. Sized to cover **every** gate-1
/// tile the five carry - the largest list is `urudre2`'s 186, and `uru`'s exit
/// band sits at positions 63..66 of its own 118, so the old 48-tile cap
/// stopped short of the very door the part exists to find.
pub(super) const MAX_TILES_SWEPT: usize = 256;

/// Records executed per partition in Part E2, and the frames each gets. The
/// frame budget matches [`EXIT_TICKS`]: a record whose scene change is its
/// tail cannot be reached from a 180-frame run.
pub(super) const MAX_RECORDS_RUN: usize = 48;
pub(super) const RECORD_RUN_TICKS: usize = EXIT_TICKS;

/// Records per scene in Part E2 that may be re-run on the deep budget - the
/// ones still executing when the ordinary budget expired.
///
/// Sized to cover every partition-2 record a door scene carries, not a
/// sample of them: `jouine` has 18 and the one that reaches its exit is
/// `P2[16]`, so a budget of six was spent on its predecessors and the record
/// the part exists to find never got the deep run. A record that goes idle
/// inside the ordinary budget never spends one of these, so the ceiling costs
/// nothing on the quiet records.
pub(super) const DEEP_RERUNS_PER_SCENE: usize = 24;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mark {
    /// Cleared.
    Pass,
    /// Attempted and failed - the note says how.
    Fail,
    /// Not attempted: an earlier rung failed.
    Skip,
    /// Not applicable - the scene's script leaves before the rung can run.
    Na,
}

impl Mark {
    pub(super) fn glyph(self) -> char {
        match self {
            Self::Pass => '*',
            Self::Fail => 'X',
            Self::Skip => '.',
            Self::Na => '~',
        }
    }
}

pub(super) struct Verdict {
    pub(super) scene: String,
    pub(super) marks: [Mark; 6],
    /// Rungs cleared in order before the first non-`Pass`.
    pub(super) score: usize,
    /// True when the ladder stopped because the scene's own script left it,
    /// not because the engine could not do the rung.
    pub(super) departed: bool,
    /// Whether the exit that fired also landed on the tile the record names.
    /// `None` when no exit fired.
    pub(super) landed_on_entry_tile: Option<bool>,
    pub(super) note: String,
}

/// Run one scene up the ladder.
#[allow(clippy::too_many_lines)]
pub(super) fn score_scene(host: &mut SceneHost, name: &str) -> Verdict {
    let base = FlagBaseline::snapshot(host);
    let mut marks = [Mark::Skip; 6];
    let mut departed = false;
    let mut landed_on_entry_tile = None;

    let finish = |marks: [Mark; 6], note: String, departed: bool, landed: Option<bool>| {
        // Only rungs actually demonstrated count. An `~` (not applicable,
        // because the scene's own script left before the rung could run) is
        // deliberately NOT credited - crediting it would let a scene that
        // never walked and never exited score the same as one that did.
        let score = marks.iter().take_while(|m| **m == Mark::Pass).count();
        Verdict {
            scene: name.to_string(),
            marks,
            score,
            departed,
            landed_on_entry_tile: landed,
            note,
        }
    };

    // -- Rung 1: assets ----------------------------------------------------
    let scene = match Scene::load(&host.index, name) {
        Ok(s) => s,
        Err(e) => {
            marks[0] = Mark::Fail;
            return finish(
                marks,
                format!("Scene::load: {e}"),
                departed,
                landed_on_entry_tile,
            );
        }
    };
    let man = match scene.field_man_payload(&host.index) {
        Ok(Some(m)) => m,
        Ok(None) => {
            marks[0] = Mark::Fail;
            return finish(
                marks,
                "no MAN resolves".into(),
                departed,
                landed_on_entry_tile,
            );
        }
        Err(e) => {
            marks[0] = Mark::Fail;
            return finish(
                marks,
                format!("field_man_payload: {e}"),
                departed,
                landed_on_entry_tile,
            );
        }
    };
    marks[0] = Mark::Pass;

    // -- Rung 2: man -------------------------------------------------------
    let mf = match parse_man(&man) {
        Ok(mf) => mf,
        Err(e) => {
            marks[1] = Mark::Fail;
            return finish(
                marks,
                format!("parse MAN: {e}"),
                departed,
                landed_on_entry_tile,
            );
        }
    };
    let dests = scene_destinations(&mf, &man);
    marks[1] = Mark::Pass;

    // -- Rung 3: enter -----------------------------------------------------
    if !enter(host, name, &base) {
        marks[2] = Mark::Fail;
        return finish(
            marks,
            format!("entered as {:?}, expected Field/WorldMap", host.world.mode),
            departed,
            landed_on_entry_tile,
        );
    }
    marks[2] = Mark::Pass;

    // -- Rung 4: script ----------------------------------------------------
    match settle(host) {
        Settle::Released => marks[3] = Mark::Pass,
        Settle::Departed(to) => {
            marks[3] = Mark::Pass;
            marks[4] = Mark::Na;
            marks[5] = Mark::Na;
            departed = true;
            return finish(
                marks,
                format!("entry script departs -> {to}"),
                departed,
                landed_on_entry_tile,
            );
        }
        Settle::Parked { held_by, at } => {
            marks[3] = Mark::Fail;
            let where_ = at.map_or_else(
                || "no live field bytecode".to_string(),
                |(pc, op)| format!("pc=0x{pc:04X} op=0x{op:02X}"),
            );
            return finish(
                marks,
                format!(
                    "script parked after {SETTLE_TICKS} ticks ({held_by} holds input, {where_})"
                ),
                departed,
                landed_on_entry_tile,
            );
        }
    }

    // -- Rung 5: walk ------------------------------------------------------
    // The driven probe, then the same budget of released pad from the same
    // settled state, as a CONTROL. Without it "the player walked" and "a
    // still-running record dragged the player" are the same measurement -
    // and they are not the same claim. `izumi`'s spring choreography moves
    // the player thirty tiles with the pad released.
    let driven = probe(host, &CARDINALS.map(PadButton::mask), WALK_TICKS);
    let neutral = if enter(host, name, &base) && matches!(settle(host), Settle::Released) {
        probe(host, &[0], WALK_TICKS * CARDINALS.len())
    } else {
        Probe {
            tiles: 0,
            left: None,
            encounter: false,
            start_tile: driven.start_tile,
        }
    };
    let pad_did_it =
        driven.tiles > neutral.tiles || (driven.left.is_some() && neutral.left.is_none());
    let drift = if neutral.tiles > 0 || neutral.left.is_some() {
        format!(
            " (released-pad control drifted {} tile(s){})",
            neutral.tiles,
            neutral
                .left
                .as_ref()
                .map(|n| format!(" and left for {n}"))
                .unwrap_or_default()
        )
    } else {
        String::new()
    };
    if !pad_did_it {
        marks[4] = Mark::Fail;
        // Which failure is it? "The engine cannot walk here" and "this
        // scene's first-visit script owns the player" look identical from
        // one measurement. Re-enter WITHOUT restoring the flag banks - the
        // first-visit records latched during the two probes above, so this
        // entry is a revisit - and say which. The rung stays failed either
        // way: it is scored on the first visit, which is the visit a player
        // actually makes first.
        let revisit = if enter_raw(host, name) && matches!(settle(host), Settle::Released) {
            let r = probe(host, &CARDINALS.map(PadButton::mask), WALK_TICKS);
            format!("; on a revisit the pad moves {} tile(s)", r.tiles)
        } else {
            "; the revisit did not settle either".to_string()
        };
        return finish(
            marks,
            format!(
                "no pad direction beat the released-pad control off tile {:?}: \
                 driven {} tile(s) vs control {} tile(s){revisit}",
                driven.start_tile, driven.tiles, neutral.tiles
            ),
            departed,
            landed_on_entry_tile,
        );
    }
    marks[4] = Mark::Pass;
    let mut note = match (&driven.left, driven.encounter) {
        (Some(to), _) => format!("walk-on trigger fired -> {to}{drift}"),
        (None, true) => format!("walked {} tile(s), then an encounter{drift}", driven.tiles),
        (None, false) => format!("walked {} tile(s){drift}", driven.tiles),
    };

    // -- Rung 6: exit ------------------------------------------------------
    let tries = try_exits(host, name, &mf, &man, &base);
    if tries.is_empty() {
        marks[5] = Mark::Fail;
        let dest_names: Vec<&str> = dests.iter().map(|d| d.scene_name.as_str()).collect();
        return finish(
            marks,
            format!(
                "{note}; no exit site: {}. MAN lists {dest_names:?}",
                exit_diagnosis(host, name, &mf, &man)
            ),
            departed,
            landed_on_entry_tile,
        );
    }
    if let Some(hit) = tries
        .iter()
        .find(|t| t.entered.as_deref() == Some(t.claimed.as_str()))
    {
        marks[5] = Mark::Pass;
        landed_on_entry_tile = Some(hit.on_entry_tile);
        note = format!(
            "{note}; exit -> {}{}",
            hit.claimed,
            if hit.on_entry_tile {
                ""
            } else {
                " (arrival tile != record's entry tile)"
            }
        );
    } else {
        marks[5] = Mark::Fail;
        let detail: Vec<String> = tries
            .iter()
            .map(|t| {
                format!(
                    "{} -> {}",
                    t.claimed,
                    t.entered.as_deref().unwrap_or("(nothing fired)")
                )
            })
            .collect();
        note = format!("{note}; exits tried: {}", detail.join(", "));
    }
    finish(marks, note, departed, landed_on_entry_tile)
}
