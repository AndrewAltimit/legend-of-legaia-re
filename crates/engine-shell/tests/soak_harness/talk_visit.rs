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

/// How far a seat's walk-out flood must get: one tile. The `confined`
/// detector's own 48 units is too short a yardstick here, because a seat the
/// harness picks can stand inside an NPC's body standoff: `son`'s nook seat
/// faces slot 29 from 64 units, the walk can leave it 82 units westward, and
/// from there the standoff never lets the player back - the pocket the
/// detector then measures is 44 units across.
const SEAT_WALK_OUT: i32 = 128;

/// Whether a player seated at `(x, z)` could walk out of the spot under the
/// locomotion's own per-axis probes (walls through `field_dir_blocked`, NPC
/// and prop bodies through `field_actor_dir_blocked`): a flood over 2-unit
/// steps, in 8-unit strides, that gets a whole tile ([`SEAT_WALK_OUT`]) from
/// the seat.
///
/// A seat the walk could never reach is a harness artifact, not a place a
/// player stands: `son`'s camp has a wall nook at (74..75, 68) closed on its
/// open sides by two NPCs who never move (slots 25 and 29), and seating there
/// to face either of them left the player `confined` with no input able to
/// walk the 48 units out. The wall-only floor component cannot see it - the
/// nook is ordinary floor until the bodies are counted.
fn seat_walks_out(w: &legaia_engine_core::world::World, x: i16, z: i16) -> bool {
    const STRIDE: i16 = 8;
    // `(dx, dz)` per probe direction: 0 = Z-, 1 = X-, 2 = Z+, 3 = X+.
    const DIRS: [(i16, i16); 4] = [(0, -1), (-1, 0), (0, 1), (1, 0)];
    let mut seen = BTreeSet::new();
    let mut queue = std::collections::VecDeque::from([(x, z)]);
    seen.insert((x, z));
    while let Some((cx, cz)) = queue.pop_front() {
        let span = SEAT_WALK_OUT;
        if (i32::from(cx) - i32::from(x)).abs() >= span
            || (i32::from(cz) - i32::from(z)).abs() >= span
        {
            return true;
        }
        for (dir, &(dx, dz)) in DIRS.iter().enumerate() {
            let (mut px, mut pz) = (cx, cz);
            let mut open = true;
            for _ in 0..STRIDE / 2 {
                if w.field_dir_blocked(px, pz, dir) || w.field_actor_dir_blocked(px, pz, dir) {
                    open = false;
                    break;
                }
                px = px.saturating_add(dx * 2);
                pz = pz.saturating_add(dz * 2);
            }
            if open && seen.insert((px, pz)) {
                queue.push_back((px, pz));
            }
        }
    }
    false
}

/// Whether a player could have walked **into** a seat at `(x, z)` from a
/// tile away: the backward flood of [`seat_walks_out`], whose step from `q`
/// to `q + stride * dir` is taken only when the forward probes at `q` let
/// the player make it.
///
/// The body probes look one way: they test points 64 ahead of the player,
/// so a pocket can let the player out and never back in. `dolk`'s monk
/// formation stands its rows 128 apart, and a seat in a gap between two of
/// them (facing one from 64) walks out along the row while every approach
/// is shut by the probe reaching the next monk; one diagonal step from the
/// seat then left the player boxed on all four sides, a spot no walk enters.
fn seat_walks_in(w: &legaia_engine_core::world::World, x: i16, z: i16) -> bool {
    const STRIDE: i16 = 8;
    const DIRS: [(i16, i16); 4] = [(0, -1), (-1, 0), (0, 1), (1, 0)];
    let mut seen = BTreeSet::new();
    let mut queue = std::collections::VecDeque::from([(x, z)]);
    seen.insert((x, z));
    while let Some((cx, cz)) = queue.pop_front() {
        if (i32::from(cx) - i32::from(x)).abs() >= SEAT_WALK_OUT
            || (i32::from(cz) - i32::from(z)).abs() >= SEAT_WALK_OUT
        {
            return true;
        }
        for (dir, &(dx, dz)) in DIRS.iter().enumerate() {
            // The predecessor one stride back along `dir`, and whether its
            // forward walk in `dir` reaches this point.
            let (qx, qz) = (
                cx.saturating_sub(dx * STRIDE),
                cz.saturating_sub(dz * STRIDE),
            );
            let (mut px, mut pz) = (qx, qz);
            let mut open = true;
            for _ in 0..STRIDE / 2 {
                if w.field_dir_blocked(px, pz, dir) || w.field_actor_dir_blocked(px, pz, dir) {
                    open = false;
                    break;
                }
                px = px.saturating_add(dx * 2);
                pz = pz.saturating_add(dz * 2);
            }
            if open && seen.insert((qx, qz)) {
                queue.push_back((qx, qz));
            }
        }
    }
    false
}

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
/// floor that a walk could both leave and enter is taken, so a target parked
/// in the off-map hide box, in a pocket or outside the walls is skipped
/// rather than reached by a seat no play produces. Returns the target seated
/// at.
///
/// An actor is faced from 112 units where it can be, else 96, else 64. The
/// walk stops a face-on approach 104 short of a standing NPC (its body
/// probe reaches 64 ahead into a ±40 box), so a 64-unit seat - where the
/// probe point lands on the anchor - is mostly one no walk reaches; from 112
/// the probe point is 48 short of the anchor, well inside the ±72 the
/// action button tests. A prop's contact box is wider than the reach (±80
/// against 64), so it is faced from 128: the probe point is then 64 inside
/// the box and the player stands outside it.
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
    // Seat distances in quarters of the 64-unit probe reach, farthest first.
    let reaches: &[i16] = match target {
        TalkTarget::Actor(_) => &[7, 6, 4],
        TalkTarget::Prop(_) => &[8],
    };
    let player = w.player_actor_slot? as usize;
    w.actors.get(player)?;
    // Cardinal sides first: a player talks face on far more than diagonally.
    let seats = reaches
        .iter()
        .flat_map(|&q| [4usize, 0, 2, 6, 1, 3, 5, 7].map(|sector| (q, sector)));
    for (q, sector) in seats {
        let (dx, dz) = FACING_PROBES[sector];
        let (sx, sz) = (ax.saturating_sub(dx * q / 4), az.saturating_add(dz * q / 4));
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
        if hit == Some(target) && seat_walks_out(w, sx, sz) && seat_walks_in(w, sx, sz) {
            return Some(target);
        }
        // Something else answers from this side: put the player back.
        let m = &mut w.actors[player].move_state;
        (m.world_x, m.world_y, m.world_z, m.render_26) = (ox, oy, oz, of);
    }
    None
}
