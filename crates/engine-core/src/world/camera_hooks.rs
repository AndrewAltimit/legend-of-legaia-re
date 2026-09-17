//! The field VM's **camera-zone arms** - the four field-script sites that
//! change or re-apply the field camera's parameter block, plus the
//! walk-region attribute refresh that sits next to them in the same
//! sub-dispatcher.
//!
//! # Why the port needs a queue
//!
//! The retail arms call straight into the field overlay's camera routines,
//! which write the camera globals. In the port those globals live on
//! [`crate::camera::Camera`], which each host owns (the native window's
//! `BootSession`, the browser play page's `PlayCamera`), while the field
//! VM's [`legaia_engine_vm::field::FieldHost`] is implemented on [`World`].
//! So an arm cannot reach the camera directly: it records a
//! [`CameraZoneRequest`] on the world, and
//! [`crate::camera::Camera::tick`] drains the queue on the next frame -
//! which is one shared drain point for both hosts rather than a hook each
//! of them would have to remember to call.
//!
//! # What the arms are
//!
//! All four live in `FUN_801DE840`'s `0x4C` `MENU_CTRL` dispatcher and were
//! read out of the disc's own jump tables (`0x801CEEB8` for outer nibble 3,
//! `0x801CEF88` for outer nibble C, both in PROT entry `0897`):
//!
//! | Op | Arm | Retail body |
//! |---|---|---|
//! | `[4C 38]` | `0x801E1048` | `FUN_801DE3E0` at the player's tile |
//! | `[4C 39]` | `0x801E1078` | query + `FUN_80019278` footing + fall into the sub-`E` tail |
//! | `[4C 3E]` | `0x801E10BC` | `FUN_801DB8EC` snap + `FUN_801DAA50` focus clamp |
//! | `[4C C4 x z]` | `0x801E2878` | `FUN_801DE3E0` at the operand tile |
//! | `[4C 3D]` | `0x801E10F8` | `FUN_800180EC` - attribute refresh, **not** a camera load |
//!
//! Retail has no other camera-zone query outside the player-seat path
//! (`0x801D1FE8..0x801D2014`, which runs the `[4C 39]` sequence in code
//! when the player is seated or warped) and the per-frame path at
//! `0x801D17FC..0x801D1830`, which is gated on scratchpad flag bit `22`
//! (`_DAT_1F800394 & 0x400000`, [`crate::world::ZONE_REQUERY_FLAG`]).
//!
//! REF: FUN_801DE840, FUN_801DE3E0, FUN_801DB8EC, FUN_800180EC

use super::World;

/// One queued camera-zone arm, drained by [`crate::camera::Camera::tick`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraZoneRequest {
    /// `[4C 38]` - query + load the camera parameter block from the record
    /// covering the player's tile. The ease walks the globals to the new
    /// block's composed pose.
    QueryAtPlayer,
    /// `[4C C4 x z]` - the same query at a tile the script names.
    QueryAtTile { x: u8, z: u8 },
    /// `[4C 39]` - query at the player's tile, re-conform the player's
    /// footing to the floor there, then snap the camera and clamp the focus.
    QueryConformAndSnap,
    /// `[4C 3E]` - snap the camera from the resident block and clamp the
    /// focus. No query.
    SnapAndClamp,
    /// `[4C 3D]` - re-latch the walk-region attribute box at the player's
    /// tile (`FUN_800180EC`). Camera-adjacent because the box is what the
    /// composer's position sweeps span.
    RefreshAttributes,
}

impl World {
    /// Queue one camera-zone arm. The queue is bounded: a script that
    /// spams an arm within one frame collapses to the requests the camera
    /// can act on, and the cap keeps a runaway record from growing the
    /// world without limit.
    pub fn push_camera_zone_request(&mut self, req: CameraZoneRequest) {
        const CAP: usize = 16;
        if self.camera.zone_requests.len() < CAP {
            self.camera.zone_requests.push(req);
        }
    }

    /// Take everything the field VM queued this frame.
    pub fn take_camera_zone_requests(&mut self) -> Vec<CameraZoneRequest> {
        std::mem::take(&mut self.camera.zone_requests)
    }

    /// Whether retail's **per-frame** camera-zone re-query is enabled right
    /// now: scratchpad flag bit `22` of `_DAT_1F800394`
    /// ([`World::flags`]`.story_flags`, the port's mirror of that word).
    ///
    /// The field per-frame update at `0x801D17DC..0x801D1840` (the routine
    /// whose tail builds the view through `FUN_800172C0`) tests
    /// `_DAT_1F800394 & 0x400000` before calling `FUN_801DE234` +
    /// `FUN_801DE3E0` at the player's tile; with the bit clear it only eases
    /// (`FUN_801DB510`) and clamps (`FUN_801DAA50`). The bit is not in the
    /// per-mode seed (that copies a `u16`), so it starts clear on every mode
    /// entry and only a field script raises it - op `0x2E` / `0x2F` with
    /// operand `0x16`. Fifteen of the disc's 124 CDNAME scenes touch it.
    ///
    /// REF: FUN_801D1344
    pub fn camera_zone_requery_per_frame(&self) -> bool {
        self.flags.story_flags & crate::world::ZONE_REQUERY_FLAG != 0
    }
}
