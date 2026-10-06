//! The play page's slot for its scene host.
//!
//! Every page module reaches the scene host through [`HostSlot::host`] /
//! [`HostSlot::host_mut`] on the field itself (`self.scene_host.host_mut()`),
//! which borrows that one field and so stays disjoint from the runtime's
//! other fields. What the slot holds behind those two calls is its own
//! business: a bare `SceneHost` today, the engine session that owns one when
//! the page drives `legaia_engine_session::BootSession`.

use legaia_engine_core::scene::SceneHost;

/// The page's scene host, once a disc is loaded.
#[derive(Default)]
pub(crate) struct HostSlot(Option<SceneHost>);

impl HostSlot {
    /// The scene host, once a disc is loaded.
    pub(crate) fn host(&self) -> Option<&SceneHost> {
        self.0.as_ref()
    }

    /// The scene host, mutably.
    pub(crate) fn host_mut(&mut self) -> Option<&mut SceneHost> {
        self.0.as_mut()
    }

    /// Install a freshly-built host, replacing any previous one.
    pub(crate) fn set(&mut self, host: SceneHost) {
        self.0 = Some(host);
    }
}
