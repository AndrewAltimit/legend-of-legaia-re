//! Leaving a scene: the exit diagnosis, running to a transition, and trying each exit site.

use super::*;

/// Why a scene produced no exit site, in terms of the two disc structures the
/// join needs: the `.MAP` walk-on triggers and the partition-2 records that
/// carry a `0x3F`.
///
/// "No exit site" has three very different causes and the join cannot tell
/// them apart on its own: the scene has no `0x3F` anywhere; it has one but no
/// walk-on band reaches the record that carries it (the door is an
/// interactive actor or a `0x3E` warp instead); or it has both and they do
/// not join. This names which.
pub(super) fn exit_diagnosis(host: &SceneHost, name: &str, mf: &ManFile, man: &[u8]) -> String {
    let p2_count = mf.header.partition_counts[2].max(0) as usize;
    // Which partition-2 records carry a `0x3F` at all? Probe each with a
    // synthetic gate-1 trigger on its own tile, reusing the real decoder.
    let synthetic: Vec<legaia_engine_field::field_regions::TileTrigger> = (0..p2_count.min(256))
        .map(|r| legaia_engine_field::field_regions::TileTrigger {
            tile_x: (r % 128) as u8,
            tile_z: (r / 128) as u8,
            record: r as u8,
            gate: 1,
        })
        .collect();
    let carriers: BTreeSet<u8> = overworld_portal_sites(mf, man, &synthetic)
        .iter()
        .map(|s| s.record)
        .collect();

    let (g1, g0) = match Scene::load(&host.index, name)
        .ok()
        .and_then(|s| s.field_tile_triggers(&host.index).ok())
    {
        Some((primary, fallback)) => {
            let all: Vec<_> = primary.into_iter().chain(fallback).collect();
            let g1: BTreeSet<u8> = all
                .iter()
                .filter(|t| t.gate == 1)
                .map(|t| t.record)
                .collect();
            let g0 = all.iter().filter(|t| t.gate == 0).count();
            (g1, g0)
        }
        None => (BTreeSet::new(), 0),
    };

    if carriers.is_empty() {
        return format!(
            "no partition-2 record carries a 0x3F ({p2_count} P2 records, \
             {} gate-1 / {g0} gate-0 .MAP triggers) - the scene's exits, if any, \
             are partition-1 or 0x3E-carried",
            g1.len()
        );
    }
    format!(
        "P2 records carrying a 0x3F: {carriers:?}, but the {} gate-1 .MAP trigger(s) \
         name records {g1:?} - the door is not on a walk-on band",
        g1.len()
    )
}

/// One exit attempt's result.
pub(super) struct ExitTry {
    pub(super) claimed: String,
    pub(super) entered: Option<String>,
    /// Did the arrival land on the tile the record's `0x3F` names?
    pub(super) on_entry_tile: bool,
}

/// How a scene ended after a walk-on trigger fired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Fired {
    /// A `0x3F` named scene change landed in `scene`.
    Scene(String),
    /// The record's tail is the FMV hand-off op (`4C E2 <id>`), not a `0x3F`.
    /// `scene` is where [`SceneHost::apply_pending_fmv_handoff`] put control
    /// once the movie finished - the headless stand-in for playback.
    Fmv { id: i16, scene: Option<String> },
}

impl Fired {
    /// The scene control ended up in, whichever tail fired.
    pub(super) fn scene(&self) -> Option<&str> {
        match self {
            Self::Scene(s) => Some(s.as_str()),
            Self::Fmv { scene, .. } => scene.as_deref(),
        }
    }
}

/// `true` when nothing is running: no cutscene timeline, no spawned helper
/// context, no dialogue and no FMV. A tile that fired nothing leaves the world
/// here on the tick after the step.
pub(super) fn world_idle(host: &SceneHost) -> bool {
    // A scene change parked behind the streaming actor's countdown is a
    // transition in flight, not a quiet tile.
    host.world.scene_transition_hold.is_none()
        && !host.world.cutscene_timeline_active()
        && host.world.field_vm.helper_contexts.is_empty()
        && !host.world.dialogue_owns_input()
        && host.world.active_fmv().is_none()
}

/// Tick until the scene changes, an FMV is triggered, or the world has been
/// idle for [`EXIT_IDLE_TICKS`] consecutive ticks - whichever comes first,
/// bounded by `budget`.
///
/// The pad pulses Cross because a door record is often a conversation: the
/// Uru Mais dream rooms page several screens of text before their tail. That
/// makes the budget a bound on a *running* record rather than a wall clock -
/// a quiet tile returns in a handful of ticks.
///
/// An FMV trigger is completed here rather than left pending: a headless world
/// has no MDEC playback, so `World::finish_cutscene` stands in for the movie
/// and [`SceneHost::apply_pending_fmv_handoff`] performs retail's post-play
/// control transfer (`FUN_801CEA3C`). That is the whole exit mechanism for
/// `jouine`, whose record carries no `0x3F` at all.
pub(super) fn run_to_transition(host: &mut SceneHost, budget: usize) -> Option<Fired> {
    let mut idle = 0usize;
    for frame in 0..budget {
        host.world.set_pad(if frame % 3 == 2 {
            PadButton::Cross.mask()
        } else {
            0
        });
        match host.tick() {
            Ok(SceneTickEvent::SceneEntered { name }) => return Some(Fired::Scene(name)),
            Ok(_) => {}
            Err(_) => return None,
        }
        if let Some(id) = host.world.active_fmv() {
            host.world.finish_cutscene();
            let scene = match host.apply_pending_fmv_handoff() {
                Some(legaia_engine_scene_host::scene_host::FmvHandoffOutcome::Entered {
                    scene,
                    ..
                }) => Some(scene.to_string()),
                _ => None,
            };
            return Some(Fired::Fmv { id, scene });
        }
        if host.world.mode == SceneMode::Battle {
            return None;
        }
        if world_idle(host) {
            idle += 1;
            if idle >= EXIT_IDLE_TICKS {
                break;
            }
        } else {
            idle = 0;
        }
    }
    host.world.set_pad(0);
    None
}

/// Try the scene's decoded exit sites until one fires. Each attempt re-enters
/// the scene fresh, because a fired exit has already left it.
pub(super) fn try_exits(
    host: &mut SceneHost,
    name: &str,
    mf: &ManFile,
    man: &[u8],
    base: &FlagBaseline,
) -> Vec<ExitTry> {
    let Ok(scene) = Scene::load(&host.index, name) else {
        return Vec::new();
    };
    let Ok((primary, fallback)) = scene.field_tile_triggers(&host.index) else {
        return Vec::new();
    };
    let mut triggers = primary;
    triggers.extend(fallback);
    let sites = overworld_portal_sites(mf, man, &triggers);

    let mut out = Vec::new();
    for site in sites.iter().take(EXIT_SITES_TRIED) {
        if !enter(host, name, base) {
            break;
        }
        // Let the entry script run first; seating onto a trigger while a
        // cutscene owns the frame tests nothing, and a script that departs or
        // parks has already taken the scene away.
        if !matches!(settle(host), Settle::Released) {
            break;
        }
        step_onto_tile(host, site.overworld_x, site.overworld_z);
        // Two budgets, the same shape Part E2 uses: every site gets the
        // ordinary one, and only a site whose record is STILL RUNNING when it
        // expires earns the deep one. The door records that need it are the
        // boss-cutscene-length tails (`urudre3` `P2[0]` alone is ~14.9k
        // ticks); a tile that spawned nothing has already returned.
        let mut fired_by = run_to_transition(host, EXIT_TICKS);
        if fired_by.is_none() && !world_idle(host) {
            fired_by = run_to_transition(host, DEEP_EXIT_TICKS);
        }
        let entered = fired_by.and_then(|f| f.scene().map(str::to_string));
        let on_entry_tile = entered.is_some() && {
            let (x, z) = player_xz(host);
            let (tx, tz) = tile_of(x, z);
            tx == i16::from(site.entry_x & 0x7F) && tz == i16::from(site.entry_z & 0x7F)
        };
        let fired = entered.is_some();
        out.push(ExitTry {
            claimed: site.scene_name.clone(),
            entered,
            on_entry_tile,
        });
        if fired {
            break;
        }
    }
    out
}
