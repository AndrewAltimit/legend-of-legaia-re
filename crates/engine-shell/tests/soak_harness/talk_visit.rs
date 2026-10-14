//! The talk-visit run mode: stand the player in front of each thing in the
//! scene the action button opens - a talk target, an examined prop - in
//! turn and press it, so every conversation and prop record a scene carries
//! runs under the detectors.

use super::*;

/// `"town01+talk"` -> `("town01", true)`: the run walks up to one of the
/// scene's action-button targets every [`TALK_VISIT_EVERY`] free field
/// frames. Such a run does not arm the random encounter roll: a New Game
/// party loses a dungeon's first fight, and the run is there for the
/// records.
pub(super) fn split_talk(label: &str) -> (&str, bool) {
    match label.strip_suffix("+talk") {
        Some(scene) => (scene, true),
        None => (label, false),
    }
}

/// Frames between two talk visits in a `+talk` run.
pub(super) const TALK_VISIT_EVERY: u64 = 300;

/// The interact probe's eight compass points
/// (`legaia_engine_core::world::config::FIELD_FACING_PROBES`): sector ->
/// `(dx, dz)`, probe point `(x + dx, z - dz)`.
const FACING_PROBES: [(i16, i16); 8] = [
    (0, 64),
    (-64, 64),
    (-64, 0),
    (-64, -64),
    (0, -64),
    (64, -64),
    (64, 0),
    (64, 64),
];

/// Smallest open-floor component (in 64-unit sub-cells) a talk seat may sit
/// in. A scene's floor splits into several components that its own doors
/// join (a town's interiors share the map with its streets), so the seat is
/// not held to the component the player stands in; a pocket this small is a
/// gap between props rather than ground a player walks on.
const MIN_SEAT_COMPONENT: usize = 48;

/// What the action button opens.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(super) enum TalkTarget {
    /// A placement the probe answers with: an NPC, a minigame door.
    Actor(u8),
    /// An interact-gated prop (a spring, a cupboard, a switch) by its anchor.
    Prop((u8, u8)),
}

/// Every action-button target of the scene with where it stands: the NPC
/// anchors and the interaction-carrying walk-touch placements in slot
/// order, then the solid interact-gated props that carry a record.
fn talk_targets(w: &legaia_engine_core::world::World) -> Vec<(TalkTarget, (i16, i16))> {
    let mut actors: BTreeMap<u8, (i16, i16)> = BTreeMap::new();
    for (&slot, &pos) in &w.npcs.positions {
        actors.insert(slot, pos);
    }
    for (&slot, &(pos, _)) in &w.props.walk_touch {
        if w.npcs.dialog_prologue.contains_key(&slot) {
            actors.entry(slot).or_insert(pos);
        }
    }
    let mut props: BTreeMap<(u8, u8), (i16, i16)> = BTreeMap::new();
    for c in &w.props.colliders {
        if let Some(anchor) = c.anchor
            && c.solid
            && c.interact
            && w.props.bank.props.contains_key(&anchor)
        {
            let (x, z) = if c.moving_box { c.live } else { c.center };
            props.entry(anchor).or_insert((x as i16, z as i16));
        }
    }
    actors
        .into_iter()
        .map(|(s, p)| (TalkTarget::Actor(s), p))
        .chain(props.into_iter().map(|(a, p)| (TalkTarget::Prop(a), p)))
        .collect()
}

/// Stand the player where the action button opens target `n` (modulo the
/// target count) and face it. Only a seat on the authored walk-visible
/// floor is taken, so a target parked in the off-map hide box or standing
/// outside the walls is skipped rather than reached by a seat no play
/// produces. Returns the target seated at.
///
/// An actor is faced from 64 units, where the probe point lands on its
/// anchor. A prop's contact box is wider than the reach (±80 against 64),
/// so it is faced from 128: the probe point is then 64 inside the box and
/// the player stands outside it.
pub(super) fn seat_at_talk_target(session: &mut NativeSession, n: usize) -> Option<TalkTarget> {
    let w = &mut session.host.world;
    if w.mode != SceneMode::Field || w.script_context_engages_player() {
        return None;
    }
    let targets = talk_targets(w);
    if targets.is_empty() {
        return None;
    }
    let (target, (ax, az)) = targets[n % targets.len()];
    let reach: i16 = match target {
        TalkTarget::Actor(_) => 1,
        TalkTarget::Prop(_) => 2,
    };
    let player = w.player_actor_slot? as usize;
    w.actors.get(player)?;
    // Cardinal sides first: a player talks face on far more than diagonally.
    for sector in [4usize, 0, 2, 6, 1, 3, 5, 7] {
        let (dx, dz) = FACING_PROBES[sector];
        let (sx, sz) = (ax.saturating_sub(dx * reach), az.saturating_add(dz * reach));
        if w.field_walk_component_size(sx, sz) < MIN_SEAT_COMPONENT {
            continue;
        }
        let sy = w.sample_field_floor_height(sx as i32, sz as i32) as i16;
        let m = &mut w.actors[player].move_state;
        let (ox, oy, oz, of) = (m.world_x, m.world_y, m.world_z, m.render_26);
        m.world_x = sx;
        m.world_y = sy;
        m.world_z = sz;
        m.render_26 = (((sector as i16) << 9) + 0x800) & 0x0FFF;
        // The button's own order: an actor the probe answers with wins, and
        // a prop is asked only when none does.
        let hit = match w.field_interact_probe_slot() {
            Some(slot) => Some(TalkTarget::Actor(slot)),
            None => w.field_interact_prop_anchor().map(TalkTarget::Prop),
        };
        if hit == Some(target) {
            return Some(target);
        }
        // Something else answers from this side: put the player back.
        let m = &mut w.actors[player].move_state;
        (m.world_x, m.world_y, m.world_z, m.render_26) = (ox, oy, oz, of);
    }
    None
}
