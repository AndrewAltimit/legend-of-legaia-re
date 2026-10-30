//! Prop hunt in the world: the field scene the room stands in is the
//! arena, its own placed props are the disguises and the decoys, and every
//! player walks their own world.
//!
//! The rules are the prop-hunt session's (`legaia_engine_modes::prop_hunt`),
//! run by the host alone: positions come from the real field - the host's
//! own player and each guest's avatar report - through
//! `ActiveMode::tick_with_positions`, so the session stays the one
//! authority on roles, timers and tags, and nothing but its view travels
//! back (the view stream, [`crate::view_stream`]). Each world keeps its own
//! collision, camera and movement model (action movement too, when its
//! toggle is on).
//!
//! The catalog is the scene's placed objects small enough to hide as: one
//! prop kind per environment-pack slot, sized from the mesh. A slot is the
//! scene's own index space (`legaia_engine_scene::field_env::EnvDraw::env_slot`),
//! the same on every peer that loaded the scene from the same disc, so a
//! prop names its mesh as `slot<N>` and each host draws its own upload of it.

use legaia_engine_modes::fixed::{FX_ONE, Vec2};
use legaia_expansion_format::modes::{AiTier, DecoyPlacement, PropDef, PropHuntArena};

/// The mode id a field prop hunt's start order names.
pub const FIELD_HUNT_ID: &str = "field-prop-hunt";
/// World units per session unit: the session's reach and speed constants
/// read at this scale as a field tile's half-width.
pub const SCALE: i32 = 4;
/// Smallest and largest prop, as a horizontal half-extent in world units:
/// a hider can be a barrel or a cart, not a house.
pub const MIN_PROP_RADIUS: i32 = 24;
pub const MAX_PROP_RADIUS: i32 = 200;
/// Prop kinds a match offers.
pub const MAX_KINDS: usize = 8;
/// Decoys a match places.
pub const MAX_DECOYS: usize = 48;
/// How far past the props and players the arena reaches, world units.
const MARGIN: i32 = 512;

/// One placed object of the scene that can be hidden as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldProp {
    /// Environment-pack slot: the mesh.
    pub slot: usize,
    /// World position (retail frame, `+Y` down).
    pub pos: [i32; 3],
    /// Horizontal half-extent, world units.
    pub radius: i32,
}

/// A mesh's horizontal half-extent about its origin, world units.
pub fn tmd_radius(tmd: &legaia_tmd::Tmd) -> i32 {
    tmd.objects
        .iter()
        .flat_map(|o| o.vertices.iter())
        .map(|v| i32::from(v.x).abs().max(i32::from(v.z).abs()))
        .max()
        .unwrap_or(0)
}

/// The catalog from the scene's placed objects (`slot`, position, mesh):
/// the ones sized like props.
pub fn prop_catalog<'a>(
    placed: impl IntoIterator<Item = (usize, [i32; 3], &'a legaia_tmd::Tmd)>,
) -> Vec<FieldProp> {
    placed
        .into_iter()
        .filter_map(|(slot, pos, tmd)| {
            let radius = tmd_radius(tmd);
            (MIN_PROP_RADIUS..=MAX_PROP_RADIUS)
                .contains(&radius)
                .then_some(FieldProp { slot, pos, radius })
        })
        .collect()
}

/// A field position in session units.
pub fn to_session(pos: [i32; 3]) -> Vec2 {
    Vec2::new(pos[0] * FX_ONE / SCALE, pos[2] * FX_ONE / SCALE)
}

/// A session position back in world units, at height `y`.
pub fn to_world(p: Vec2, y: i32) -> [i32; 3] {
    [p.x * SCALE / FX_ONE, y, p.y * SCALE / FX_ONE]
}

/// The prop id a slot plays as.
pub fn prop_id(slot: usize) -> String {
    format!("slot{slot}")
}

/// The slot a prop id names.
pub fn prop_slot(id: &str) -> Option<usize> {
    id.strip_prefix("slot")?.parse().ok()
}

/// The arena for `players` players over `props`, framed around the props
/// and the players standing at `around`. `None` when the scene has no prop
/// or fewer than two players play.
pub fn arena(props: &[FieldProp], around: &[[i32; 3]], players: u8) -> Option<PropHuntArena> {
    if props.is_empty() || players < 2 {
        return None;
    }
    // The most common slots are the kinds, so a hider has company.
    let mut counts: std::collections::BTreeMap<usize, (usize, i32)> = Default::default();
    for p in props {
        let e = counts.entry(p.slot).or_insert((0, 0));
        e.0 += 1;
        e.1 = e.1.max(p.radius);
    }
    let mut kinds: Vec<(usize, usize, i32)> =
        counts.into_iter().map(|(s, (n, r))| (s, n, r)).collect();
    kinds.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    kinds.truncate(MAX_KINDS);
    let decoys: Vec<&FieldProp> = props
        .iter()
        .filter(|p| kinds.iter().any(|k| k.0 == p.slot))
        .take(MAX_DECOYS)
        .collect();
    let (mut x0, mut z0, mut x1, mut z1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for p in decoys.iter().map(|d| d.pos).chain(around.iter().copied()) {
        x0 = x0.min(p[0]);
        z0 = z0.min(p[2]);
        x1 = x1.max(p[0]);
        z1 = z1.max(p[2]);
    }
    let unit = |v: i32| v / SCALE;
    Some(PropHuntArena {
        id: FIELD_HUNT_ID.to_string(),
        name: "Prop hunt".to_string(),
        location: None,
        props: kinds
            .iter()
            .map(|&(slot, _, r)| PropDef {
                id: prop_id(slot),
                mesh: None,
                radius: (r / SCALE).max(1) as u32,
            })
            .collect(),
        decoys: decoys
            .iter()
            .map(|d| DecoyPlacement {
                prop: prop_id(d.slot),
                x: unit(d.pos[0]),
                y: unit(d.pos[2]),
            })
            .collect(),
        bounds: [
            unit(x0 - MARGIN),
            unit(z0 - MARGIN),
            unit(x1 + MARGIN),
            unit(z1 + MARGIN),
        ],
        blockers: Vec::new(),
        rounds: 2,
        hunters: 1,
        players,
        hide_frames: 20 * 60,
        round_frames: 120 * 60,
        hunter_health: 100,
        miss_penalty: 20,
        ai_tier: AiTier::Regular,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prop(slot: usize, x: i32, z: i32) -> FieldProp {
        FieldProp {
            slot,
            pos: [x, 0, z],
            radius: 64,
        }
    }

    #[test]
    fn the_arena_frames_the_props_and_players() {
        let props = [prop(3, 0, 0), prop(3, 1024, 0), prop(7, 0, 2048)];
        let a = arena(&props, &[[4096, 0, 4096]], 2).unwrap();
        assert_eq!(a.props.len(), 2);
        assert_eq!(a.props[0].id, "slot3", "the commonest slot leads");
        assert_eq!(a.decoys.len(), 3);
        assert_eq!(a.bounds, [-128, -128, 1152, 1152]);
        assert_eq!(prop_slot(&a.props[1].id), Some(7));
        assert!(arena(&[], &[], 2).is_none());
        assert!(arena(&props, &[], 1).is_none());
    }

    #[test]
    fn positions_round_trip_through_session_units() {
        let w = [1200, -64, -3000];
        assert_eq!(to_world(to_session(w), -64), w);
    }
}
