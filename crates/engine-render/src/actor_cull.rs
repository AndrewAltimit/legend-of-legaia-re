//! Camera-frame actor projector + visibility cull.
//!
//! PORT: FUN_801CF754 - dialog-overlay camera-frame projector: walks the
//! linked actor list, computes each actor's screen-space `(X, Y)` (either
//! from its tile descriptor or directly from its local position), culls to a
//! strict `+/-0x180` window around the camera and appends survivors to the
//! visible table capped at `0x20` entries.
//!
//! Source: `ghidra/scripts/funcs/overlay_dialog_801cf754.txt`; the retail
//! caller context is the dialog-overlay actor frame handler
//! (`docs/reference/functions.md`, dialog-overlay actor-frame helpers).
//!
//! Retail data flow, mirrored here over typed inputs instead of PSX
//! pointers:
//!
//! 1. Camera position is cached from `cam+0x14` / `cam+0x18` (i16) into
//!    scratchpad (`_DAT_1F800020/24`) and the visible-count global
//!    (`_DAT_8007B6B8`) is zeroed.
//! 2. Each actor with any of state bits `0x3` set (actor `+0x10`) is
//!    skipped outright.
//! 3. Actors **without** flag bits `0x0102_0000` resolve position through
//!    the per-scene tile-descriptor table at `_DAT_1F8003EC + slot*0x20`
//!    (slot = actor `+0x60`):
//!    `x = i8(desc[+0x06])*128 + i8(desc[+0x0e])*16` (and the same shape for
//!    `y` from `desc[+0x07]` / `desc[+0x0f]`) - a coarse-cell `<<7` plus
//!    fine `<<4` packing of two separate signed bytes. When actor flag word
//!    `+0x52` has bit `8` set, a mirror adjust applies:
//!    `x -= i16(desc[+0x00])`, `y += i16(desc[+0x04])`. The actor's own
//!    local offset (`+0x14` / `+0x18`, i16) is added on top.
//!    Actors **with** any of bits `0x0102_0000` use `+0x14` / `+0x18`
//!    directly as the position.
//! 4. Cull: keep only `cam - 0x180 < v < cam + 0x180` on both axes -
//!    strictly exclusive at both bounds (retail `slt` pairs).
//! 5. Survivors append to the table at `DAT_801C93C8`; when the count
//!    reaches `0x20` the walk stops entirely.

/// Half-width of the visibility window around the camera, in world units.
/// Retail `+/-0x180` (`addiu v0, v1, -0x180` / `+0x180` pairs).
pub const CULL_WINDOW: i32 = 0x180;

/// Maximum number of visible actors collected per frame (retail `li v1,0x20`
/// early-out).
pub const VISIBLE_ACTOR_CAP: usize = 0x20;

/// Actor state bits (actor `+0x10`) that exclude it from the visible walk
/// entirely (`andi v0, v1, 0x3`).
pub const SKIP_STATE_MASK: u32 = 0x3;

/// Actor state bits (actor `+0x10`) that bypass the tile-descriptor lookup
/// and use the actor's local position directly (`lui v0,0x102; and`).
pub const DIRECT_POS_MASK: u32 = 0x0102_0000;

/// Actor flag bit (flag word `+0x52`) that applies the tile descriptor's
/// mirror adjust (`andi v0, v0, 0x8`).
pub const MIRROR_FLAG: u16 = 0x8;

/// Camera position as cached by the retail prologue (i16 at `cam+0x14` /
/// `cam+0x18`, stored to scratchpad `_DAT_1F800020/24`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CullCamera {
    pub x: i16,
    pub y: i16,
}

/// The slice of the retail actor record the projector reads.
#[derive(Debug, Clone, Copy, Default)]
pub struct CullActor {
    /// Actor state/flag word (retail `+0x10`); see [`SKIP_STATE_MASK`] and
    /// [`DIRECT_POS_MASK`].
    pub state_flags: u32,
    /// Local X offset (retail `+0x14`, i16). For [`DIRECT_POS_MASK`] actors
    /// this IS the position; otherwise it offsets the tile-derived position.
    pub local_x: i16,
    /// Local Y offset (retail `+0x18`, i16).
    pub local_y: i16,
    /// Secondary flag word (retail `+0x52`); bit [`MIRROR_FLAG`] selects the
    /// mirror adjust.
    pub sprite_flags: u16,
    /// Tile-descriptor slot (retail `+0x60`, u16) into the `0x20`-byte-stride
    /// table at `_DAT_1F8003EC`.
    pub tile_slot: u16,
}

/// The fields the projector reads out of one `0x20`-byte tile descriptor at
/// `_DAT_1F8003EC + slot*0x20`.
#[derive(Debug, Clone, Copy, Default)]
pub struct TileDescriptor {
    /// i16 at `+0x00`; subtracted from X under the mirror adjust.
    pub mirror_dx: i16,
    /// i16 at `+0x04`; added to Y under the mirror adjust.
    pub mirror_dy: i16,
    /// Signed coarse cell X (i8 at `+0x06`), scaled by `<<7` (x128).
    pub coarse_x: i8,
    /// Signed coarse cell Y (i8 at `+0x07`), scaled by `<<7`.
    pub coarse_y: i8,
    /// Signed fine X (i8 at `+0x0e`), scaled by `<<4` (x16).
    pub fine_x: i8,
    /// Signed fine Y (i8 at `+0x0f`), scaled by `<<4`.
    pub fine_y: i8,
}

/// Resolve one actor's screen-space `(X, Y)` exactly as the retail walk does
/// (step 3 of the module doc). Returns `None` when the actor needs a tile
/// descriptor whose slot is outside `tile_descs` - retail would read
/// whatever RAM lives past the table; the port culls such actors instead.
pub fn project_actor(actor: &CullActor, tile_descs: &[TileDescriptor]) -> Option<(i32, i32)> {
    if actor.state_flags & DIRECT_POS_MASK != 0 {
        return Some((i32::from(actor.local_x), i32::from(actor.local_y)));
    }
    let desc = tile_descs.get(usize::from(actor.tile_slot))?;
    let mut x = (i32::from(desc.coarse_x) << 7) + (i32::from(desc.fine_x) << 4);
    let mut y = (i32::from(desc.coarse_y) << 7) + (i32::from(desc.fine_y) << 4);
    if actor.sprite_flags & MIRROR_FLAG != 0 {
        x -= i32::from(desc.mirror_dx);
        y += i32::from(desc.mirror_dy);
    }
    Some((x + i32::from(actor.local_x), y + i32::from(actor.local_y)))
}

/// Strictly-exclusive `+/-0x180` window test on both axes (step 4).
fn in_window(v: i32, cam: i32) -> bool {
    cam - CULL_WINDOW < v && v < cam + CULL_WINDOW
}

/// Walk `actors` in list order and collect the indices of the visible ones,
/// capped at [`VISIBLE_ACTOR_CAP`]; hitting the cap stops the walk entirely
/// (matching the retail early return, which never examines later actors).
pub fn build_visible_actor_list(
    camera: CullCamera,
    actors: &[CullActor],
    tile_descs: &[TileDescriptor],
) -> Vec<usize> {
    let cam_x = i32::from(camera.x);
    let cam_y = i32::from(camera.y);
    let mut visible = Vec::new();
    for (idx, actor) in actors.iter().enumerate() {
        if actor.state_flags & SKIP_STATE_MASK != 0 {
            continue;
        }
        let Some((x, y)) = project_actor(actor, tile_descs) else {
            continue;
        };
        if !in_window(x, cam_x) || !in_window(y, cam_y) {
            continue;
        }
        visible.push(idx);
        if visible.len() == VISIBLE_ACTOR_CAP {
            break;
        }
    }
    visible
}

#[cfg(test)]
mod tests {
    use super::*;

    fn direct_actor(x: i16, y: i16) -> CullActor {
        CullActor {
            state_flags: DIRECT_POS_MASK, // any of bits 0x102_0000 suffices
            local_x: x,
            local_y: y,
            ..Default::default()
        }
    }

    #[test]
    fn window_cull_is_strict_at_both_bounds() {
        let cam = CullCamera { x: 0, y: 0 };
        let actors = [
            direct_actor(0x17f, 0),  // inside (kept)
            direct_actor(0x180, 0),  // exactly +window (culled)
            direct_actor(-0x17f, 0), // inside (kept)
            direct_actor(-0x180, 0), // exactly -window (culled)
            direct_actor(0, 0x180),  // Y bound (culled)
            direct_actor(0, -0x180), // Y bound (culled)
            direct_actor(0, 0x17f),  // Y inside (kept)
        ];
        assert_eq!(
            build_visible_actor_list(cam, &actors, &[]),
            vec![0, 2, 6],
            "+/-0x180 must be exclusive on both axes"
        );
    }

    #[test]
    fn window_is_camera_relative() {
        let cam = CullCamera { x: 1000, y: -500 };
        let actors = [
            direct_actor(1000 + 0x17f, -500), // kept
            direct_actor(1000 + 0x180, -500), // culled
            direct_actor(1000, -500 - 0x17f), // kept
        ];
        assert_eq!(build_visible_actor_list(cam, &actors, &[]), vec![0, 2]);
    }

    #[test]
    fn state_bits_0x3_skip_the_actor() {
        let cam = CullCamera::default();
        let mut actors = [direct_actor(0, 0); 4];
        actors[0].state_flags |= 0x1;
        actors[1].state_flags |= 0x2;
        actors[2].state_flags |= 0x3;
        // actors[3] keeps only DIRECT_POS_MASK: not skipped.
        assert_eq!(build_visible_actor_list(cam, &actors, &[]), vec![3]);
    }

    #[test]
    fn either_direct_pos_bit_bypasses_tile_lookup() {
        for bit in [0x0100_0000u32, 0x0002_0000u32] {
            let actor = CullActor {
                state_flags: bit,
                local_x: 5,
                local_y: -7,
                tile_slot: 99, // would be out of range for the tile path
                ..Default::default()
            };
            assert_eq!(project_actor(&actor, &[]), Some((5, -7)));
        }
    }

    #[test]
    fn tile_packing_is_coarse_shl7_plus_fine_shl4() {
        let descs = [TileDescriptor {
            coarse_x: 3,
            coarse_y: -2,
            fine_x: 5,
            fine_y: -1,
            ..Default::default()
        }];
        let actor = CullActor {
            local_x: 10,
            local_y: -20,
            tile_slot: 0,
            ..Default::default()
        };
        // x = 3*128 + 5*16 + 10 = 474; y = -2*128 + -1*16 - 20 = -292.
        assert_eq!(project_actor(&actor, &descs), Some((474, -292)));
    }

    #[test]
    fn mirror_flag_applies_descriptor_adjust() {
        let descs = [TileDescriptor {
            mirror_dx: 100,
            mirror_dy: 40,
            coarse_x: 1,
            coarse_y: 1,
            fine_x: 0,
            fine_y: 0,
        }];
        let mut actor = CullActor {
            tile_slot: 0,
            ..Default::default()
        };
        assert_eq!(project_actor(&actor, &descs), Some((128, 128)));
        actor.sprite_flags = MIRROR_FLAG;
        // x -= mirror_dx, y += mirror_dy.
        assert_eq!(project_actor(&actor, &descs), Some((28, 168)));
    }

    #[test]
    fn out_of_range_tile_slot_is_culled() {
        let actor = CullActor {
            tile_slot: 4,
            ..Default::default()
        };
        assert_eq!(project_actor(&actor, &[]), None);
        assert!(build_visible_actor_list(CullCamera::default(), &[actor], &[]).is_empty());
    }

    #[test]
    fn visible_table_caps_at_0x20_and_stops_the_walk() {
        let cam = CullCamera::default();
        let actors = vec![direct_actor(0, 0); 40];
        let visible = build_visible_actor_list(cam, &actors, &[]);
        assert_eq!(visible.len(), VISIBLE_ACTOR_CAP);
        assert_eq!(visible, (0..0x20).collect::<Vec<_>>());
    }
}
