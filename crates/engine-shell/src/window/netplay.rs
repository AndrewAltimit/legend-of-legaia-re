//! Netplay in the play window: `--host [PORT]` (an in-process relay this
//! window joins) and `--join ADDR --room CODE`, the room screen key, chat
//! typing, and the other players' avatars in the field.
//!
//! The room itself is `legaia_engine_netplay::room::NetPlay`, the same
//! controller the browser play page drives; this module only owns the
//! sockets, the keys and the draws.

use std::time::Duration;

use super::*;
use legaia_engine_netplay::room::{NetPlay, RoomKey, pack_identity};
use legaia_netplay::protocol::RoomCode;
use legaia_netplay::relay::server::{RelayHandle, RelayServer};
use legaia_netplay::transport::stream::StreamTransport;
use legaia_render_kernels::screen_prim::ScreenPrim;

/// How long a connect may take before the launch gives up.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// The `--host` / `--join` / `--room` / `--name` operands.
#[derive(Debug, Clone, Default)]
pub struct NetLaunch {
    /// `--host [PORT]`: run a relay on this port (`0.0.0.0`) and create a
    /// room on it.
    pub host: Option<u16>,
    /// `--join ADDR`: a relay's `host:port`.
    pub join: Option<String>,
    /// `--room CODE`: the room to join.
    pub room: Option<String>,
    /// `--name NAME`: the name the room shows.
    pub name: Option<String>,
    /// `--multiplayer`: open the Multiplayer screen (host or join) once the
    /// field is up - the launcher's Play online button.
    pub menu: bool,
}

impl NetLaunch {
    pub fn is_set(&self) -> bool {
        self.host.is_some() || self.join.is_some()
    }
}

/// The Multiplayer screen before a room: the menu and last tick's pad.
pub(in crate::window) struct NetMenu {
    pub menu: legaia_engine_netplay::connect::ConnectMenu,
    prev_pad: u16,
}

impl NetMenu {
    /// The screen seeded with `name` (`--name`), else the name it was last
    /// used with, and the address it last joined (`remembered`).
    pub fn new(name: Option<&str>, remembered: &NetRemembered) -> Self {
        let mut menu = legaia_engine_netplay::connect::ConnectMenu::new(
            true,
            name.unwrap_or(&remembered.name),
        );
        menu.address = remembered.address.clone();
        Self {
            menu,
            prev_pad: u16::MAX,
        }
    }
}

/// The file [`NetRemembered`] lives in, in the launcher's settings folder
/// ([`crate::launcher::config_dir`]): its own file, so a window writing it
/// never races the launcher's write of the settings file.
const NET_REMEMBERED_FILE: &str = "legaia-netplay.toml";

/// What the Multiplayer screen remembers between runs: the address last
/// joined and the name last played under. The browser play page's Online
/// panel remembers the same two (and, like it, not the room code, which is
/// a new one for every room).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(in crate::window) struct NetRemembered {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub address: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
}

impl NetRemembered {
    /// The remembered file's path, if a settings folder is known.
    pub fn path() -> Option<std::path::PathBuf> {
        crate::launcher::config_dir().map(|d| d.join(NET_REMEMBERED_FILE))
    }

    /// What `path` holds; nothing when it is missing or unreadable (a
    /// remembered address is a convenience, never a reason to fail).
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Write to `path`, whole or not at all.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        }
        let text = toml::to_string(self).context("serialise the remembered room")?;
        legaia_app_theme::settings_file::write_atomic(path, text.as_bytes())
            .with_context(|| format!("write {}", path.display()))
    }

    /// `self` after the screen asked for `action`: a join's address and
    /// either action's name replace the remembered ones. The default name
    /// (an empty field) is not a name the player chose.
    pub fn after(&self, menu: &legaia_engine_netplay::connect::ConnectMenu, joined: bool) -> Self {
        Self {
            address: if joined {
                menu.address.trim().to_string()
            } else {
                self.address.clone()
            },
            name: menu.name.trim().to_string(),
        }
    }
}

/// The window's room connection, and the relay when this window hosts it.
pub(in crate::window) struct NetHost {
    pub net: NetPlay,
    /// Dropped with the window: every room closes and the port is freed.
    pub _relay: Option<RelayHandle>,
    /// The scene the prop catalog was last built for.
    props_scene: Option<u16>,
    /// The room code was logged.
    logged_room: bool,
}

impl NetHost {
    /// A window joined to room `code` over an already-open `link`: the
    /// headless driver's way into a room, over an in-memory link.
    pub(in crate::window) fn joined(
        link: Box<dyn legaia_netplay::transport::Transport>,
        code: RoomCode,
    ) -> Self {
        Self {
            net: NetPlay::join(link, "Window", code, None),
            _relay: None,
            props_scene: None,
            logged_room: false,
        }
    }
}

/// The address others on this machine's network reach it at: the local
/// interface a route out would use (a UDP `connect` sends nothing), or
/// loopback when there is none.
fn lan_ip() -> String {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("192.0.2.1:9")?;
            s.local_addr()
        })
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string())
}

/// Open the launch's room, if it names one.
pub(in crate::window) fn start_netplay(
    launch: &NetLaunch,
    world: &legaia_engine_core::world::World,
) -> Result<Option<NetHost>> {
    if !launch.is_set() {
        return Ok(None);
    }
    let name = launch.name.clone().unwrap_or_else(|| "Player".to_string());
    let pack = pack_identity(world.expansion.pack.as_deref());
    if let Some(port) = launch.host {
        let relay = RelayServer::spawn(("0.0.0.0", port))
            .with_context(|| format!("--host: bind port {port}"))?;
        let port = relay.local_addr().port();
        let link = StreamTransport::connect_tcp(("127.0.0.1", port), CONNECT_TIMEOUT)
            .map_err(|e| anyhow::anyhow!("--host: join the local relay: {e}"))?;
        let mut net = NetPlay::host(Box::new(link), &name, pack);
        let address = format!("{}:{port}", lan_ip());
        log::info!("netplay: hosting on port {port}; others join with --join {address}");
        net.address = Some(address);
        return Ok(Some(NetHost {
            net,
            _relay: Some(relay),
            props_scene: None,
            logged_room: false,
        }));
    }
    let addr = launch.join.as_deref().expect("is_set");
    let code = launch
        .room
        .as_deref()
        .and_then(RoomCode::parse)
        .context("--join needs --room CODE (six letters and digits)")?;
    let link = StreamTransport::connect_tcp(addr, CONNECT_TIMEOUT)
        .map_err(|e| anyhow::anyhow!("--join {addr}: {e}"))?;
    let mut net = NetPlay::join(Box::new(link), &name, code, pack);
    net.address = Some(addr.to_string());
    log::info!("netplay: joining room {code} at {addr}");
    Ok(Some(NetHost {
        net,
        _relay: None,
        props_scene: None,
        logged_room: false,
    }))
}

impl PlayWindowApp {
    /// This tick's room frame, before the world's, on `pad` (zero on a tick
    /// the field does not own). The return is the pad bits the field may
    /// see: none while a multiplayer screen holds the pad, movement only in
    /// a field prop hunt (`NetPlay::world_pad_filter`).
    pub(in crate::window) fn step_netplay(&mut self, pad: u16) -> u16 {
        if self.netplay.is_none() {
            return if self.step_netplay_menu(pad) {
                0
            } else {
                u16::MAX
            };
        }
        let scene = legaia_engine_netplay::room::scene_id(&self.session.host);
        if self
            .netplay
            .as_ref()
            .is_some_and(|n| n.props_scene != Some(scene))
        {
            let props = self.scene_prop_catalog();
            log::info!("netplay: {} props to hide as in this scene", props.len());
            if let Some(nh) = self.netplay.as_mut() {
                nh.props_scene = Some(scene);
                nh.net.set_scene_props(props);
            }
        }
        let Some(nh) = self.netplay.as_mut() else {
            return u16::MAX;
        };
        nh.net.frame_host(&mut self.session.host, pad);
        if !nh.logged_room
            && let Some(code) = nh.net.room
        {
            nh.logged_room = true;
            log::info!("netplay: room {code}");
        }
        nh.net.world_pad_filter()
    }

    /// The Multiplayer screen's tick: its pad edges, then the host or join it
    /// asks for. `true` while it is open (it holds the pad).
    fn step_netplay_menu(&mut self, pad: u16) -> bool {
        use legaia_engine_netplay::connect::ConnectAction;
        let Some(m) = self.netplay_menu.as_mut() else {
            return false;
        };
        let pressed = pad & !m.prev_pad;
        m.prev_pad = pad;
        let action = m.menu.step(pressed);
        let joined = matches!(action, Some(ConnectAction::Join { .. }));
        let asked = action
            .as_ref()
            .is_some_and(|a| !matches!(a, ConnectAction::Close))
            .then(|| m.menu.clone());
        let launch = match action {
            None => return true,
            Some(ConnectAction::Close) => {
                self.netplay_menu = None;
                return true;
            }
            Some(ConnectAction::Host { port, name }) => NetLaunch {
                host: Some(port),
                name: Some(name),
                ..NetLaunch::default()
            },
            Some(ConnectAction::Join { addr, room, name }) => NetLaunch {
                join: Some(addr),
                room: Some(room.to_string()),
                name: Some(name),
                ..NetLaunch::default()
            },
        };
        // Remembered on the ask, not on the connect: an address that did
        // not answer is the one to correct next time.
        if let Some(menu) = asked {
            self.remember_room(&menu, joined);
        }
        match start_netplay(&launch, &self.session.host.world) {
            Ok(nh) => {
                self.netplay = nh;
                self.netplay_menu = None;
            }
            Err(e) => {
                if let Some(m) = self.netplay_menu.as_mut() {
                    m.menu.notice = Some(format!("{e:#}"));
                }
            }
        }
        true
    }

    /// What the Multiplayer screen opens with: the remembered address and
    /// name, or nothing in a scripted capture (which reads no player file).
    pub(in crate::window) fn remembered_room(&self) -> NetRemembered {
        if self.scripted_run() {
            return NetRemembered::default();
        }
        NetRemembered::path()
            .map(|p| NetRemembered::load(&p))
            .unwrap_or_default()
    }

    /// Keep what `menu` just hosted or joined with for the next run.
    fn remember_room(&self, menu: &legaia_engine_netplay::connect::ConnectMenu, joined: bool) {
        if self.scripted_run() {
            return;
        }
        let Some(path) = NetRemembered::path() else {
            return;
        };
        let next = NetRemembered::load(&path).after(menu, joined);
        if let Err(e) = next.save(&path) {
            log::warn!("netplay: the room was not remembered: {e:#}");
        }
    }

    /// `Tab`: open or close the room screen (and, online, the backquote
    /// key does the same instead of the offline mode picker). `T` on the
    /// room screen starts a chat line. `true` when the key was taken.
    pub(in crate::window) fn netplay_key(&mut self, code: KeyCode, state: ElementState) -> bool {
        if state != ElementState::Pressed || self.boot_ui.is_active() {
            return false;
        }
        let pad_bound = self
            .mapping
            .pad_button_for_key(super::launch_options::keycode_to_name(code))
            .is_some();
        let Some(nh) = self.netplay.as_mut() else {
            // Offline, Tab opens (or closes) the Multiplayer screen.
            if code == KeyCode::Tab {
                self.netplay_menu = match self.netplay_menu.take() {
                    Some(_) => None,
                    None => Some(NetMenu::new(None, &self.remembered_room())),
                };
                return true;
            }
            return false;
        };
        match nh.net.room_key(&format!("{code:?}"), pad_bound) {
            Some(RoomKey::Reaction(i)) => nh.net.emote(i),
            Some(RoomKey::ToggleScreen) => nh.net.toggle_screen(),
            Some(RoomKey::BeginChat) => nh.net.begin_chat(),
            None => return false,
        }
        true
    }

    /// A key while a chat line is being typed: Enter sends, Escape cancels,
    /// Backspace deletes, anything with text types it. `true` when taken
    /// (every press is, so a typed letter never reaches a pad binding).
    pub(in crate::window) fn netplay_text_key(
        &mut self,
        code: KeyCode,
        state: ElementState,
        text: Option<&str>,
    ) -> bool {
        if let Some(m) = self.netplay_menu.as_mut()
            && m.menu.editing.is_some()
        {
            if state == ElementState::Pressed {
                match code {
                    KeyCode::Enter | KeyCode::NumpadEnter => m.menu.commit(),
                    KeyCode::Escape => m.menu.cancel_edit(),
                    KeyCode::Backspace => m.menu.backspace(),
                    _ => {
                        if let Some(t) = text {
                            m.menu.type_text(t);
                        }
                    }
                }
            }
            return true;
        }
        let Some(nh) = self.netplay.as_mut() else {
            return false;
        };
        if nh.net.draft.is_none() {
            return false;
        }
        if state != ElementState::Pressed {
            return true;
        }
        match code {
            KeyCode::Enter | KeyCode::NumpadEnter => nh.net.submit_chat(),
            KeyCode::Escape => nh.net.cancel_chat(),
            KeyCode::Backspace => nh.net.chat_backspace(),
            _ => {
                if let Some(t) = text {
                    nh.net.type_chat(t);
                }
            }
        }
        true
    }

    /// Whether the Multiplayer screen covers the stage: the menu before a
    /// room, or the room screen or a party card (not the roaming panel). The
    /// window's own readouts - the field party's, the start-up key line -
    /// step aside under it.
    pub(in crate::window) fn multiplayer_screen_up(&self) -> bool {
        self.netplay_menu.is_some()
            || self
                .netplay
                .as_ref()
                .is_some_and(|nh| legaia_engine_netplay::screens::screen_owns_frame(&nh.net))
    }

    /// The room screen / roaming panel's prims, in stage pixels.
    pub(in crate::window) fn netplay_prims(&self) -> Vec<ScreenPrim> {
        match (&self.netplay, &self.netplay_menu) {
            (Some(nh), _) => {
                legaia_engine_netplay::screens::net_overlay_prims(&nh.net, &self.session.host.world)
            }
            (None, Some(m)) => legaia_engine_netplay::screens::connect_overlay_prims(&m.menu),
            (None, None) => Vec::new(),
        }
    }

    /// The room screen / roaming panel's labels, in stage pixels.
    pub(in crate::window) fn netplay_stage_texts(&self) -> Vec<legaia_engine_ui::TextDraw> {
        match (&self.netplay, &self.netplay_menu) {
            (Some(nh), _) => legaia_engine_netplay::screens::net_overlay_stage_texts(
                &nh.net,
                &self.session.host.world,
                &self.font,
            ),
            (None, Some(m)) => {
                legaia_engine_netplay::screens::connect_overlay_stage_texts(&m.menu, &self.font)
            }
            (None, None) => Vec::new(),
        }
    }

    /// The prop slot this window's player hides as, in a field prop hunt.
    pub(in crate::window) fn netplay_self_disguise(&self) -> Option<usize> {
        self.netplay.as_ref()?.net.self_disguise()
    }

    /// A field prop hunt's hiders drawn as their props: each disguised
    /// remote avatar, and this player when it hides, as the env-pack slot's
    /// uploaded mesh at the avatar's position and heading.
    pub(in crate::window) fn remote_prop_draws(&self) -> Vec<(usize, Mat4)> {
        let Some(nh) = &self.netplay else {
            return Vec::new();
        };
        let world = &self.session.host.world;
        if world.mode != SceneMode::Field {
            return Vec::new();
        }
        let mesh_of = |slot: usize| self.field_pack_meshes.get(slot).copied().flatten();
        let model = |pos: [i32; 3], facing: u16| {
            let yaw = std::f32::consts::PI + f32::from(facing) / 4096.0 * std::f32::consts::TAU;
            Mat4::from_translation(Vec3::new(pos[0] as f32, pos[1] as f32, pos[2] as f32))
                * Mat4::from_rotation_y(yaw)
        };
        let mut out: Vec<(usize, Mat4)> = nh
            .net
            .remote_avatars()
            .iter()
            .filter_map(|a| {
                Some((
                    mesh_of(nh.net.disguise_of(a.player)?)?,
                    model(a.pos, a.facing),
                ))
            })
            .collect();
        if let (Some(slot), Some(p)) = (
            nh.net.self_disguise(),
            world
                .player_actor_slot
                .and_then(|s| world.actors.get(usize::from(s))),
        ) && let Some(mi) = mesh_of(slot)
        {
            let m = &p.move_state;
            out.push((
                mi,
                model(
                    [
                        i32::from(m.world_x),
                        i32::from(m.world_y),
                        i32::from(m.world_z),
                    ],
                    m.render_26 as u16,
                ),
            ));
        }
        out
    }

    /// The scene's prop catalog for a field prop hunt
    /// (`legaia_engine_netplay::field_hunt::prop_catalog`): the placed
    /// objects, each named by its env-pack slot and sized from its mesh.
    fn scene_prop_catalog(&self) -> Vec<legaia_engine_netplay::field_hunt::FieldProp> {
        let slot_of = |mi: usize| self.field_pack_meshes.iter().position(|m| *m == Some(mi));
        let placed = self.field_placement_draws.iter().filter_map(|(mi, model)| {
            let slot = slot_of(*mi)?;
            let (tmd, _) = self.scene_tmd_data.get(*mi)?;
            let t = model.w_axis;
            Some((slot, [t.x as i32, t.y as i32, t.z as i32], tmd))
        });
        legaia_engine_netplay::field_hunt::prop_catalog(placed)
    }

    /// The other players in this scene, each the local leader's mesh posed
    /// at its reported clip frame, with its model matrix (the field branch
    /// of `actor_model`).
    pub(in crate::window) fn build_remote_avatar_draws(
        &self,
        r: &legaia_engine_render::Renderer,
    ) -> Vec<(UploadedVramMesh, Option<UploadedColorMesh>, Mat4)> {
        let Some(nh) = &self.netplay else {
            return Vec::new();
        };
        let world = &self.session.host.world;
        if world.mode != SceneMode::Field {
            return Vec::new();
        }
        let Some(tmd_idx) = world
            .player_actor_slot
            .and_then(|s| world.actors.get(usize::from(s)))
            .and_then(|a| a.tmd_binding)
        else {
            return Vec::new();
        };
        let Some((tmd, raw)) = self.scene_tmd_data.get(tmd_idx) else {
            return Vec::new();
        };
        let scale = world.player_render_scale();
        let mut out = Vec::new();
        for a in nh.net.remote_avatars() {
            if nh.net.disguise_of(a.player).is_some() {
                continue;
            }
            let Some(pose) = legaia_engine_netplay::avatar_pose::avatar_pose(world, &a) else {
                continue;
            };
            let vmesh = legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot(tmd, raw, &pose.bone_outputs);
            if vmesh.indices.is_empty() {
                continue;
            }
            let Ok(mesh) = r.upload_vram_mesh(
                &vmesh.positions,
                &vmesh.uvs,
                &vmesh.cba_tsb,
                &vmesh.normals,
                &vmesh.colors,
                &vmesh.indices,
            ) else {
                continue;
            };
            let cmesh = legaia_tmd::mesh::tmd_to_color_mesh_posed_rot(tmd, raw, &pose.bone_outputs);
            let color = (!cmesh.is_empty())
                .then(|| {
                    r.upload_color_mesh_blended(
                        &cmesh.positions,
                        &cmesh.colors,
                        &cmesh.indices,
                        &cmesh.blend,
                    )
                    .ok()
                })
                .flatten();
            let pos = Vec3::new(a.pos[0] as f32, a.pos[1] as f32, a.pos[2] as f32);
            let yaw = std::f32::consts::PI + f32::from(a.facing) / 4096.0 * std::f32::consts::TAU;
            let model = Mat4::from_translation(pos)
                * Mat4::from_rotation_y(yaw)
                * Mat4::from_scale(Vec3::splat(scale));
            out.push((mesh, color, model));
        }
        out
    }
}

#[cfg(test)]
mod remembered_tests {
    use super::*;
    use legaia_engine_netplay::connect::ConnectMenu;

    #[test]
    fn a_join_is_remembered_and_seeds_the_next_screen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join(NET_REMEMBERED_FILE);
        assert_eq!(NetRemembered::load(&path), NetRemembered::default());

        let mut menu = ConnectMenu::new(true, "");
        menu.address = " 192.168.1.20:7777 ".into();
        menu.name = "Noa".into();
        let kept = NetRemembered::default().after(&menu, true);
        kept.save(&path).unwrap();

        let back = NetRemembered::load(&path);
        assert_eq!(back.address, "192.168.1.20:7777");
        assert_eq!(back.name, "Noa");
        let next = NetMenu::new(None, &back);
        assert_eq!(next.menu.address, "192.168.1.20:7777");
        assert_eq!(next.menu.name, "Noa");
        assert!(next.menu.room.is_empty(), "a room code is never remembered");
    }

    #[test]
    fn hosting_keeps_the_address_last_joined() {
        let had = NetRemembered {
            address: "example.org:7777".into(),
            name: "Vahn".into(),
        };
        let mut menu = ConnectMenu::new(true, "Gala");
        menu.address = "typed-but-not-joined".into();
        let kept = had.after(&menu, false);
        assert_eq!(kept.address, "example.org:7777");
        assert_eq!(kept.name, "Gala");
    }

    #[test]
    fn a_launch_name_wins_over_the_remembered_one() {
        let had = NetRemembered {
            address: "example.org:7777".into(),
            name: "Vahn".into(),
        };
        let menu = NetMenu::new(Some("Guest"), &had).menu;
        assert_eq!(menu.name, "Guest");
        assert_eq!(menu.address, "example.org:7777");
    }

    #[test]
    fn a_damaged_file_reads_as_nothing_remembered() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(NET_REMEMBERED_FILE);
        std::fs::write(&path, "address = [not toml").unwrap();
        assert_eq!(NetRemembered::load(&path), NetRemembered::default());
    }
}
