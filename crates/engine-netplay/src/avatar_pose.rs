//! The pose a remote avatar is drawn in.
//!
//! An avatar carries its clip (`anim`: `0` idle, `1` walk) and frame, not
//! bones. Every player starts from the same party, so a host poses a remote
//! avatar with its **own** leader's locomotion clips at the reported frame
//! and draws it with its own leader's mesh: a remote player always looks
//! like the local party leader. Drawing each player's own leader needs the
//! other characters' rigs resident, which the field scene does not load.

use legaia_anm::PoseFrame;
use legaia_engine_core::world::World;
use legaia_netplay::protocol::AvatarState;

/// `a`'s pose off the local leader's clips; `None` before the field has a
/// player animation.
pub fn avatar_pose(world: &World, a: &AvatarState) -> Option<PoseFrame> {
    let anim = world.locomotion.player_anim.as_ref()?;
    let mut clip = if a.anim == 1 {
        anim.walk.clone()
    } else {
        anim.idle.clone()
    };
    let frames = clip.frame_count().max(1) as u32;
    clip.set_cursor((u32::from(a.anim_frame) % frames) * 16, false);
    Some(clip.current_pose())
}
