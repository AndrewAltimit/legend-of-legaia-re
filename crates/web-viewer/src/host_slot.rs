//! The play page's slot for its engine session.
//!
//! The page holds the native window's session type,
//! [`legaia_engine_session::BootSession`] over the page's audio output
//! ([`crate::play_sfx::PageSink`]), built over the scene host `load_disc`
//! assembles in memory. Every page module reaches the scene host through
//! [`HostSlot::host`] / [`HostSlot::host_mut`] on the field itself
//! (`self.scene_host.host_mut()`), which borrows that one field and so stays
//! disjoint from the runtime's other fields.

use legaia_engine_core::camera::Camera;
use legaia_engine_core::scene::SceneHost;
use legaia_engine_session::BootConfig;

/// The engine session the page runs, over the page's audio output.
pub(crate) type PageSession = legaia_engine_session::BootSession<crate::play_sfx::PageSink>;

/// The page's engine session, once a disc is loaded, and the engine camera.
///
/// The camera is the engine camera controller ([`crate::play_camera`]) the
/// native window's session owns: it routes the op-`0x45` Configure beats,
/// advances the mover, writes the follow focus back into the retail globals
/// and resets them on scene entry. It is the session's once one exists
/// (`BootSession::camera`, the one its tick drives); before a disc is loaded
/// the slot holds it, so the page's camera exports answer from the first
/// frame.
pub(crate) struct HostSlot {
    session: Option<PageSession>,
    camera: Camera,
}

impl HostSlot {
    /// An empty slot holding `camera` until a session takes it.
    pub(crate) fn new(camera: Camera) -> Self {
        Self {
            session: None,
            camera,
        }
    }

    /// The scene host, once a disc is loaded.
    pub(crate) fn host(&self) -> Option<&SceneHost> {
        self.session.as_ref().map(|s| &s.host)
    }

    /// The scene host, mutably.
    pub(crate) fn host_mut(&mut self) -> Option<&mut SceneHost> {
        self.session.as_mut().map(|s| &mut s.host)
    }

    /// The session, once a disc is loaded.
    #[allow(dead_code)]
    pub(crate) fn session(&self) -> Option<&PageSession> {
        self.session.as_ref()
    }

    /// The session, mutably.
    #[allow(dead_code)]
    pub(crate) fn session_mut(&mut self) -> Option<&mut PageSession> {
        self.session.as_mut()
    }

    /// The engine camera.
    pub(crate) fn camera(&self) -> &Camera {
        match self.session.as_ref() {
            Some(s) => &s.camera,
            None => &self.camera,
        }
    }

    /// The engine camera, mutably.
    pub(crate) fn camera_mut(&mut self) -> &mut Camera {
        match self.session.as_mut() {
            Some(s) => &mut s.camera,
            None => &mut self.camera,
        }
    }

    /// The scene host and the camera together, once a disc is loaded.
    pub(crate) fn host_cam(&self) -> Option<(&SceneHost, &Camera)> {
        self.session.as_ref().map(|s| (&s.host, &s.camera))
    }

    /// The scene host and the camera together, mutably.
    pub(crate) fn host_cam_mut(&mut self) -> Option<(&mut SceneHost, &mut Camera)> {
        self.session.as_mut().map(|s| (&mut s.host, &mut s.camera))
    }

    /// Build the page's session over `host` and the disc's `SCUS_942.54`
    /// (`None` on a PROT.DAT-only load), replacing any previous one.
    ///
    /// The session boots no scene (the page enters its first scene through
    /// its own picker) and opens no audio (the page's output exists only after
    /// a user gesture, and its director is the page's own). The page drains
    /// the world's per-tick queues, opens its pause menu and plays the field's
    /// CD-XA itself, so the session is told all three. Every cold entry on
    /// this page is a scene-picker entry, so the new-game defaults stand up
    /// the full Vahn / Noa / Gala party, as the native picker does.
    pub(crate) fn install(&mut self, host: SceneHost, scus: Option<&[u8]>) -> anyhow::Result<()> {
        let cfg = BootConfig {
            scene: String::new(),
            enable_audio: false,
        };
        let mut session = PageSession::from_host(host, scus, &cfg, || {
            anyhow::bail!("the page opens its audio output itself")
        })?;
        // The camera carries over: the page's framing knobs (yaw bias, follow
        // distance, the user's orbit / tilt / zoom) belong to the page, not
        // to the disc.
        if let Some(old) = self.session.take() {
            self.camera = old.camera.clone();
        }
        session.camera = std::mem::replace(&mut self.camera, Camera::new());
        session.set_host_drains_queues(true);
        session.set_host_owns_pause_menu(true);
        session.set_host_stages_field_xa(true);
        if let Some(defaults) = session.host.new_game_defaults.as_mut() {
            defaults.picker_party = true;
        }
        self.session = Some(session);
        Ok(())
    }
}
