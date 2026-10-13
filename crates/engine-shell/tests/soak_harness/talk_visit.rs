//! The talk-visit run mode: stand the player in front of each of the scene's
//! talk targets in turn and press the action button, so every conversation
//! record a scene carries runs under the detectors.

use super::*;

/// `"town01+talk"` -> `("town01", true)`: the run walks up to one of the
/// scene's talk targets every [`TALK_VISIT_EVERY`] free field frames.
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

/// The slots the action button can open a conversation on: the NPC anchors
/// and the interaction-carrying walk-touch placements (minigame doors), in
/// slot order.
fn talk_targets(w: &legaia_engine_core::world::World) -> Vec<(u8, (i16, i16))> {
    let mut out: BTreeMap<u8, (i16, i16)> = BTreeMap::new();
    for (&slot, &pos) in &w.npcs.positions {
        out.insert(slot, pos);
    }
    for (&slot, &(pos, _)) in &w.props.walk_touch {
        if w.npcs.dialog_prologue.contains_key(&slot) {
            out.entry(slot).or_insert(pos);
        }
    }
    out.into_iter().collect()
}

/// Smallest open-floor component (in 64-unit sub-cells) a talk seat may sit
/// in. A scene's floor splits into several components that its own doors
/// join (a town's interiors share the map with its streets), so the seat is
/// not held to the component the player stands in; a pocket this small is a
/// gap between props rather than ground a player walks on.
const MIN_SEAT_COMPONENT: usize = 48;

/// Stand the player where the interact probe hits talk target `n` (modulo
/// the target count) and face it. Only a seat on the authored walk-visible
/// floor is taken, so a target parked in the off-map hide box or standing
/// outside the walls is skipped rather than reached by a seat no play
/// produces. Returns the slot seated at.
pub(super) fn seat_at_talk_target(session: &mut NativeSession, n: usize) -> Option<u8> {
    let w = &mut session.host.world;
    if w.mode != SceneMode::Field || w.script_context_engages_player() {
        return None;
    }
    let targets = talk_targets(w);
    if targets.is_empty() {
        return None;
    }
    let (slot, (ax, az)) = targets[n % targets.len()];
    let player = w.player_actor_slot? as usize;
    w.actors.get(player)?;
    // Cardinal sides first: a player talks face on far more than diagonally.
    for sector in [4usize, 0, 2, 6, 1, 3, 5, 7] {
        let (dx, dz) = FACING_PROBES[sector];
        let (sx, sz) = (ax.saturating_sub(dx), az.saturating_add(dz));
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
        if w.field_interact_probe_slot() == Some(slot) {
            return Some(slot);
        }
        // Another actor's box answers from this side: put the player back.
        let m = &mut w.actors[player].move_state;
        (m.world_x, m.world_y, m.world_z, m.render_26) = (ox, oy, oz, of);
    }
    None
}
