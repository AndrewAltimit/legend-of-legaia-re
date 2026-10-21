//! The shared GTE camera kernels: the camera build and projection
//! (`psx_camera`, `projection`), the op-`0x45` cutscene glide
//! (`camera_mover`), the ten live camera globals as one value
//! (`retail_cam`), the battle camera's tween step table and shake jitter
//! (`battle_camera`), the camera-relative effect-actor spawn and its glide
//! tick (`camera_rel_actor`, `camera_rel_glide`), and the field follow
//! camera's eye-back distance preset (`camera_distance`).
//!
//! The field camera, the battle camera script, the Baka Fighter duel and the
//! draw kernels all name these, so the crate sits below each of them; its
//! only engine dependency is `legaia-engine-system` (the world `rand`
//! stream the shake draws from). See the crate README for the module map.

#![forbid(unsafe_code)]

pub mod battle_camera;
pub mod camera_distance;
pub mod camera_mover;
pub mod camera_rel_actor;
pub mod camera_rel_glide;
pub mod projection;
pub mod psx_camera;
pub mod retail_cam;
