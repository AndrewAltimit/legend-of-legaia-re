//! The field actor visibility cull `FUN_801D79E8` (PROT 0897), the
//! pre-update `FUN_8003BC08` calls on every live actor (`jal` at
//! `0x8003BC34`) before its height arm tests `+0x10 & 2`.
//!
//! The routine rewrites bit `1` of `+0x10` on every call:
//!
//! ```text
//! if _DAT_8007BAF4 != 0:  flags &= ~2; return          // 0x801D79FC..0x801D7A14
//! tx = (X + _DAT_80089118) >> 7                         // focus-relative tile
//! tz = (Z + _DAT_80089120) >> 7                         // (the focus pair is stored negated)
//! culled = X <  box[0] << 7 || X >= box[2] << 7         // 0x1F800384 / 86, world tiles
//!       || Z <  box[1] << 7 || Z >= box[3] << 7         // 0x1F800385 / 87
//!       || tx >= win[2] + r  || tx <= win[0] - r        // 0x1F8003EA / E8, signed
//!       || tz >= win[3] + r  || tz <= win[1] - r - 2    // 0x1F8003EB / E9, signed
//! culled ? flags |= 2 (0x801D7B08..0x801D7B18)
//!        : flags &= ~2, and +0x16 = FUN_80019278(actor) when +0x52 & 0x40
//! ```
//!
//! `r` is the actor's signed `+0x58`. The two tests measure different
//! things: the region box is the walk-region attribute box in **world**
//! tiles, the window the camera's visible tile window **around the focus**.
//! The near-Z edge alone carries an extra two tiles.
//!
//! A retail capture (`scripts/pcsx-redux/autorun_field_actor_cull.lua` on
//! `first_town_interactive`, 420 vsyncs with a held DOWN) logged every call's
//! inputs and the exit it took; all 600 distinct rows (547 culled, 53
//! visible) reproduce through [`field_actor_culled`]. Every town01 placement
//! (`+0x50` `0x25..0x58`) read `+0x58 = 0`.

use super::World;

pub use crate::camera::FieldCullView;

/// `FUN_801D79E8`'s decision: `true` = bit `1` set (the actor is outside the
/// region box or the widened window), `false` = cleared.
///
/// `map_view_fade` is `_DAT_8007BAF4 != 0`, which forces every actor visible.
///
/// PORT: FUN_801D79E8
pub fn field_actor_culled(
    x: i16,
    z: i16,
    radius: i16,
    map_view_fade: bool,
    view: &FieldCullView,
) -> bool {
    if map_view_fade {
        return false;
    }
    let (x, z) = (i32::from(x), i32::from(z));
    let tx = (x + view.focus_stored[0]) >> 7;
    let tz = (z + view.focus_stored[1]) >> 7;
    let b = view.attr_box.map(|v| i32::from(v) << 7);
    let w = view.window.map(i32::from);
    let r = i32::from(radius);
    x < b[0]
        || x >= b[2]
        || z < b[1]
        || z >= b[3]
        || tx >= w[2] + r
        || tx <= w[0] - r
        || tz >= w[3] + r
        || tz <= w[1] - r - 2
}

impl World {
    /// Whether the placement NPC standing at `(x, z)` is culled this tick,
    /// against the camera view a host last published
    /// ([`crate::world::FieldNpcState::cull_view`]). `false` while no view has
    /// been published: with no camera there is nothing to cull against.
    pub fn field_npc_culled(&self, x: i16, z: i16) -> bool {
        // Placement radius: `+0x58` read 0 on every captured placement.
        self.field_actor_culled_at(x, z, 0)
    }

    /// [`field_actor_culled`] for any field actor at `(x, z)` with cull
    /// radius `radius` (`+0x58`), against the camera view a host last
    /// published; `false` while none is. A `.MAP` placed object's actor
    /// carries its record's `+0x1E` byte there (`FUN_80020F88`,
    /// `0x80020FC0..0x80020FC8`).
    pub fn field_actor_culled_at(&self, x: i16, z: i16, radius: i16) -> bool {
        let Some(view) = self.npcs.cull_view.as_ref() else {
            return false;
        };
        let fade = self
            .world_map
            .ctrl
            .as_ref()
            .is_some_and(|c| c.entry_fade.ramp != 0);
        field_actor_culled(x, z, radius, fade, view)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rows from the retail capture: `(x, z, r, focus, box, window, culled)`.
    #[allow(clippy::type_complexity)]
    const CAPTURED: &[(i16, i16, i16, [i32; 2], [u8; 4], [i8; 4], bool)] = &[
        // Visible: the placements around the player.
        (
            4160,
            11968,
            0,
            [-4160, -11840],
            [20, 77, 44, 108],
            [-8, -6, 8, 12],
            false,
        ),
        (
            4224,
            12032,
            0,
            [-4160, -11840],
            [20, 77, 44, 108],
            [-8, -6, 8, 12],
            false,
        ),
        (
            4288,
            11712,
            0,
            [-4160, -11808],
            [20, 77, 44, 108],
            [-8, -6, 8, 12],
            false,
        ),
        (
            3776,
            11840,
            0,
            [-4160, -11824],
            [20, 77, 44, 108],
            [-8, -6, 8, 12],
            false,
        ),
        (
            4160,
            11968,
            8,
            [-4160, -11840],
            [20, 77, 44, 108],
            [-8, -6, 8, 12],
            false,
        ),
        // Culled on the near-Z window edge alone (inside the region box).
        (
            4032,
            10048,
            1,
            [-4160, -11840],
            [20, 77, 44, 108],
            [-8, -6, 8, 12],
            true,
        ),
        (
            4160,
            10048,
            1,
            [-4160, -11840],
            [20, 77, 44, 108],
            [-8, -6, 8, 12],
            true,
        ),
        // Culled outside the region box.
        (
            16320,
            16320,
            0,
            [-4160, -11840],
            [20, 77, 44, 108],
            [-8, -6, 8, 12],
            true,
        ),
        (
            3264,
            4032,
            0,
            [-4160, -11840],
            [20, 77, 44, 108],
            [-8, -6, 8, 12],
            true,
        ),
        (
            2816,
            960,
            10,
            [-4160, -11840],
            [20, 77, 44, 108],
            [-8, -6, 8, 12],
            true,
        ),
    ];

    #[test]
    fn the_kernel_reproduces_the_captured_decisions() {
        for &(x, z, r, focus_stored, attr_box, window, culled) in CAPTURED {
            let view = FieldCullView {
                focus_stored,
                attr_box,
                window,
            };
            assert_eq!(
                field_actor_culled(x, z, r, false, &view),
                culled,
                "({x}, {z}) r {r}"
            );
        }
    }

    #[test]
    fn the_map_view_fade_forces_every_actor_visible() {
        let view = FieldCullView {
            focus_stored: [-4160, -11840],
            attr_box: [20, 77, 44, 108],
            window: [-8, -6, 8, 12],
        };
        assert!(field_actor_culled(16320, 16320, 0, false, &view));
        assert!(!field_actor_culled(16320, 16320, 0, true, &view));
    }

    #[test]
    fn the_near_z_edge_carries_two_extra_tiles() {
        // Focus on world (8192, 8192), a window of +-4 tiles, a box that
        // covers the whole map.
        const O: i16 = 8192;
        let view = FieldCullView {
            focus_stored: [-8192, -8192],
            attr_box: [0, 0, 0x7F, 0x7F],
            window: [-4, -4, 4, 4],
        };
        let culled = |dx: i16, dz: i16, r: i16| field_actor_culled(O + dx, O + dz, r, false, &view);
        // X: tiles -3..=3 are in, -4 and 4 are out.
        assert!(!culled(-3 * 128, 0, 0));
        assert!(culled(-4 * 128 + 127, 0, 0));
        assert!(!culled(3 * 128 + 127, 0, 0));
        assert!(culled(4 * 128, 0, 0));
        // Z: tiles -5..=3 are in (the -2), -6 and 4 are out.
        assert!(!culled(0, -5 * 128, 0));
        assert!(culled(0, -6 * 128 + 127, 0));
        assert!(culled(0, 4 * 128, 0));
        // The radius widens every edge.
        assert!(!culled(4 * 128, 0, 1));
        // The region box is in world tiles, independent of the focus.
        let boxed = FieldCullView {
            attr_box: [64, 0, 0x7F, 0x7F],
            ..view
        };
        assert!(field_actor_culled(O - 1, O, 0, false, &boxed));
        assert!(!field_actor_culled(O, O, 0, false, &boxed));
    }
}
