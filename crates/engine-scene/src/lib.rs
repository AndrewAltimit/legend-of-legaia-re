//! The disc-facing half of the engine: resolving a CDNAME scene to its PROT
//! entries and loading / decoding what it is built from, with no simulation
//! state involved.
//!
//! Nothing here names `World` or the scene host that ticks a loaded scene;
//! both live in `legaia-engine-core`, which depends on this crate. Doc
//! comments name items of a dependent crate in plain code spans, since
//! rustdoc cannot resolve a link into a dependent crate. See the crate
//! README for the module map.

pub mod battle_overlay_readers;
pub mod cd_dma;
pub mod clut_walk_anim;
pub mod coplanar_draws;
pub mod cutscene_caption;
pub mod expansion_map;
pub mod field_env;
pub mod field_ground;
pub mod field_occlusion;
pub mod field_view_window;
pub mod fog_ground;
pub mod man_census;
pub mod mesh_memo;
pub mod model_bank;
pub mod npc_catalog;
pub mod overlay_loader;
pub mod scene;
pub mod scene_arrivals;
pub mod scene_assembly;
pub mod scene_assets;
pub mod scene_bundle;
pub mod scene_resources;
pub mod stream_file;
pub mod vfs;

use std::sync::Mutex;

/// Lock a mutex, tolerating poison. A poisoned mutex means some thread
/// panicked while holding the lock; every mutex in this crate guards either a
/// read-through cache (a partially-populated map is still correct - the
/// missing entries are simply recomputed) or a read-only disc / archive
/// reader (byte reads are idempotent). In all those cases the guarded data is
/// still usable, so recover the guard via [`std::sync::PoisonError::into_inner`]
/// rather than cascading a second panic into an unrelated caller. Used by the
/// asset cache in [`vfs`] and the PROT index caches in [`scene`].
pub(crate) fn lock_poison_tolerant<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod lock_poison_tolerant_tests {
    use super::lock_poison_tolerant;
    use std::sync::{Arc, Mutex};

    #[test]
    fn recovers_a_poisoned_mutex_instead_of_panicking() {
        let m = Arc::new(Mutex::new(vec![1u8, 2, 3]));
        // Poison the mutex: panic while holding the lock on another thread.
        let m2 = Arc::clone(&m);
        let _ = std::thread::spawn(move || {
            let _g = m2.lock().unwrap();
            panic!("intentional poison");
        })
        .join();
        assert!(m.lock().is_err(), "mutex should be poisoned for this test");
        // The tolerant lock recovers the guard and the data is intact.
        let g = lock_poison_tolerant(&m);
        assert_eq!(&*g, &[1u8, 2, 3]);
    }
}

// `tests/*.rs` compile into this crate's unit-test target rather than an
// integration binary of their own: an integration binary links the non-test
// library, so a `cargo test` would build the crate twice (once as the
// library, once in test mode for the `src/` unit tests) with the two on one
// critical path. The alias lets those files keep naming the crate as
// `legaia_engine_scene::`, the public paths a downstream crate sees.
#[cfg(test)]
extern crate self as legaia_engine_scene;
#[cfg(test)]
#[path = "../tests/integration.rs"]
mod integration;
