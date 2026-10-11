//! The [`FieldHost`] engine-callback surface for the field VM.
//!
//! The VM is generic over one bound, [`FieldHost`], which is nothing but the
//! sum of the supertraits below. Each supertrait is the part of the surface
//! one owner of host state answers for, so a host reads as a list of what it
//! must own rather than as one flat method table:
//!
//! | Supertrait | Host state it speaks for |
//! |---|---|
//! | [`FieldFlagsHost`] | flag banks, script registers, frame clock, `rand` |
//! | [`FieldSceneHost`] | scene changes, scripted encounters, mode handoffs |
//! | [`FieldTerrainHost`] | tile flags, region tables, floor heights, footing |
//! | [`FieldCameraHost`] | camera registers, camera zones, the view window |
//! | [`FieldCameraMotionHost`] | camera glide, vertical-offset ease, zoom, shake |
//! | [`FieldScreenHost`] | fades, tints, screen widgets, VRAM edits, lights |
//! | [`FieldAudioHost`] | music and sound-effect cues |
//! | [`FieldPartyInventoryHost`] | roster, character records, bag, gold, coins |
//! | [`FieldDialogMenuHost`] | dialogue, balloons, the op `0x49` menu request |
//! | [`FieldActorHost`] | the stepped actor's motion, heading, model, arcs |
//! | [`FieldActorSpawnHost`] | the actor list: spawn, clone, allocate, resolve |
//! | [`FieldActorFxHost`] | lights, tint, reflections, emitters on an actor |
//! | [`FieldPlayerHost`] | the player object behind channel `0xF8` |
//! | [`FieldHaltHost`] | the halt-acquire park and its release |
//!
//! Only [`FieldFlagsHost`] has required methods (the global flag word and the
//! frame delta); every other method carries a default body, so a host
//! implements the supertraits it has state for and leaves the rest as empty
//! `impl` blocks. The one bound between supertraits is
//! `FieldCameraMotionHost: FieldFlagsHost`, for the default
//! [`FieldCameraMotionHost::camera_offset_mode`] that reads the global flag
//! word.

mod actor;
mod actor_fx;
mod actor_spawn;
mod audio;
mod camera;
mod camera_motion;
mod dialog_menu;
mod flags;
mod halt;
mod party_inventory;
mod player;
mod scene;
mod screen;
mod terrain;

pub use actor::FieldActorHost;
pub use actor_fx::FieldActorFxHost;
pub use actor_spawn::FieldActorSpawnHost;
pub use audio::FieldAudioHost;
pub use camera::FieldCameraHost;
pub use camera_motion::FieldCameraMotionHost;
pub use dialog_menu::FieldDialogMenuHost;
pub use flags::FieldFlagsHost;
pub use halt::FieldHaltHost;
pub use party_inventory::FieldPartyInventoryHost;
pub use player::FieldPlayerHost;
pub use scene::FieldSceneHost;
pub use screen::FieldScreenHost;
pub use terrain::FieldTerrainHost;

/// Engine-side callbacks the field VM dispatches into: the umbrella over the
/// per-owner supertraits, and the bound every VM entry point takes.
///
/// It declares no method of its own and is implemented for every type that
/// implements all the supertraits, so a host never writes
/// `impl FieldHost for T`.
///
/// A method for a newly ported opcode lands on the supertrait whose state it
/// touches, with a default body, so existing hosts compile unchanged and
/// engines override what they care about.
pub trait FieldHost:
    FieldFlagsHost
    + FieldSceneHost
    + FieldTerrainHost
    + FieldCameraHost
    + FieldCameraMotionHost
    + FieldScreenHost
    + FieldAudioHost
    + FieldPartyInventoryHost
    + FieldDialogMenuHost
    + FieldActorHost
    + FieldActorSpawnHost
    + FieldActorFxHost
    + FieldPlayerHost
    + FieldHaltHost
{
}

impl<T> FieldHost for T where
    T: FieldFlagsHost
        + FieldSceneHost
        + FieldTerrainHost
        + FieldCameraHost
        + FieldCameraMotionHost
        + FieldScreenHost
        + FieldAudioHost
        + FieldPartyInventoryHost
        + FieldDialogMenuHost
        + FieldActorHost
        + FieldActorSpawnHost
        + FieldActorFxHost
        + FieldPlayerHost
        + FieldHaltHost
{
}
