//! Netplay on the play page: join a room over a browser `WebSocket`, the
//! room screen, chat and emotes, and the other players' avatars.
//!
//! The room is `legaia_engine_netplay::room::NetPlay`, the controller the
//! native window drives; its screens come out of
//! `legaia_engine_netplay::screens` into the page's screen-prim pass and
//! text layer, the composition the native window draws. A browser cannot
//! listen on a socket, so the page only joins: a room is hosted by a native
//! play window (`--host`) or a standalone `legaia-relay`.

use crate::runtime::LegaiaRuntime;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_netplay::room::{NetPlay, pack_identity};
use legaia_netplay::protocol::RoomCode;
use legaia_render_kernels::screen_prim::ScreenPrim;
use wasm_bindgen::prelude::*;

impl LegaiaRuntime {
    /// This tick's room frame, before the world's, on `pad` (zero on a tick
    /// the field does not own). The return is the pad bits the field may
    /// see (`NetPlay::world_pad_filter`): none under the room screen,
    /// movement only in a field prop hunt.
    pub(crate) fn step_netplay(&mut self, pad: u16) -> u16 {
        if self.netplay.is_none() {
            return u16::MAX;
        }
        let scene = self.scene_host.host().map_or(
            legaia_engine_netplay::roam::NO_SCENE,
            legaia_engine_netplay::room::scene_id,
        );
        if self.netplay_props_scene != Some(scene) {
            self.netplay_props_scene = Some(scene);
            let props = self.scene_prop_catalog();
            if let Some(net) = self.netplay.as_mut() {
                net.set_scene_props(props);
            }
        }
        let (Some(net), Some(host)) = (self.netplay.as_mut(), self.scene_host.host_mut()) else {
            return u16::MAX;
        };
        net.frame_host(host, pad);
        net.world_pad_filter()
    }

    /// The scene's prop catalog for a field prop hunt
    /// (`legaia_engine_netplay::field_hunt::prop_catalog`): the placed
    /// objects, each named by its env-pack slot - the native window's twin.
    fn scene_prop_catalog(&self) -> Vec<legaia_engine_netplay::field_hunt::FieldProp> {
        let (Some(f), Some(res)) = (self.field.as_ref(), self.res()) else {
            return Vec::new();
        };
        let placed = f.placements.iter().filter_map(|d| {
            let tmd = &res.tmds.get(*f.env_tmds.get(d.env_slot)?)?.tmd;
            Some((d.env_slot, [d.world_x, d.world_y, d.world_z], tmd))
        });
        legaia_engine_netplay::field_hunt::prop_catalog(placed)
    }

    /// The room screen / roaming panel's prims, in stage pixels.
    pub(crate) fn netplay_prims(&self) -> Vec<ScreenPrim> {
        match (&self.netplay, self.scene_host.host()) {
            (Some(net), Some(h)) => {
                legaia_engine_netplay::screens::net_overlay_prims(net, &h.world)
            }
            _ => Vec::new(),
        }
    }

    /// The room screen / roaming panel's labels, in stage pixels.
    pub(crate) fn netplay_stage_texts(
        &self,
        font: &legaia_font::Font,
    ) -> Vec<legaia_engine_ui::TextDraw> {
        match (&self.netplay, self.scene_host.host()) {
            (Some(net), Some(h)) => {
                legaia_engine_netplay::screens::net_overlay_stage_texts(net, &h.world, font)
            }
            _ => Vec::new(),
        }
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Join room `room` on the relay at `url` (`ws://host:port/`; a page
    /// served over `https://` needs `wss://`) as `name`. Returns `""` once
    /// the socket is opening, else why not. The room screen opens; the page
    /// draws it through the overlay it already draws.
    pub fn play_net_join(&mut self, url: &str, room: &str, name: &str) -> String {
        let Some(code) = RoomCode::parse(room) else {
            return "A room code is six letters and digits.".to_string();
        };
        let Some(host) = self.scene_host.host() else {
            return "Load a scene first.".to_string();
        };
        let pack = pack_identity(host.world.expansion.pack.as_deref());
        let name = if name.trim().is_empty() {
            "Player"
        } else {
            name.trim()
        };
        match open_socket(url) {
            Ok(link) => {
                let mut net = NetPlay::join(link, name, code, pack);
                net.address = Some(url.to_string());
                self.netplay = Some(net);
                self.netplay_props_scene = None;
                String::new()
            }
            Err(e) => e,
        }
    }

    /// Leave the room (a networked mode in play ends for this player).
    pub fn play_net_leave(&mut self) {
        if let (Some(net), Some(host)) = (self.netplay.as_mut(), self.scene_host.host_mut()) {
            net.leave(&mut host.world);
        }
        self.netplay = None;
    }

    /// Open or close the room screen (the native window's Tab).
    pub fn play_net_toggle_screen(&mut self) {
        if let Some(net) = self.netplay.as_mut() {
            net.toggle_screen();
        }
    }

    /// Send a chat line.
    pub fn play_net_say(&mut self, text: &str) {
        if let Some(net) = self.netplay.as_mut()
            && !text.trim().is_empty()
        {
            net.say(text);
        }
    }

    /// Send emote `i` (`waves`, `cheers`, `bows`, `dances`, `laughs`).
    pub fn play_net_emote(&mut self, i: u32) {
        if let Some(net) = self.netplay.as_mut() {
            net.emote(i as usize);
        }
    }

    /// The room as
    /// `{"joined", "room", "connected", "in_mode", "screen", "notice", "chat": [...], "players": [{"name", "seat", "ready", "host", "you"}]}`;
    /// `{"joined": false}` outside a room.
    pub fn play_net_state_json(&self) -> String {
        let Some(net) = &self.netplay else {
            return r#"{"joined":false}"#.to_string();
        };
        let players: Vec<_> = net
            .player_rows()
            .into_iter()
            .map(|p| {
                serde_json::json!({
                    "name": p.name, "seat": p.seat, "ready": p.ready, "host": p.host, "you": p.you,
                })
            })
            .collect();
        serde_json::json!({
            "joined": true,
            "room": net.room.map(|c| c.to_string()),
            "connected": !net.closed,
            "in_mode": net.in_mode(),
            "screen": net.screen_open,
            "notice": net.notice,
            "chat": net.chat,
            "players": players,
        })
        .to_string()
    }

    /// The env-pack slot this player hides as in a field prop hunt (the
    /// page draws that mesh in place of the lead), `-1` when not hiding.
    pub fn play_net_self_disguise(&self) -> i32 {
        self.netplay
            .as_ref()
            .and_then(NetPlay::self_disguise)
            .map_or(-1, |s| s as i32)
    }

    /// The other players in this scene: `[x, y, z, facing, slot]` per
    /// avatar (retail frame, `+Y` down; `facing` the 12-bit heading, as
    /// [`Self::player_transform`]; `slot` the env-pack mesh a field prop
    /// hunt's hider is drawn as, `-1` for the lead's mesh). Empty outside a
    /// room or the field.
    pub fn play_net_avatars(&self) -> Vec<f32> {
        let (Some(net), Some(h)) = (&self.netplay, self.scene_host.host()) else {
            return Vec::new();
        };
        if h.world.mode != SceneMode::Field {
            return Vec::new();
        }
        net.remote_avatars()
            .iter()
            .flat_map(|a| {
                [
                    a.pos[0] as f32,
                    a.pos[1] as f32,
                    a.pos[2] as f32,
                    f32::from(a.facing),
                    net.disguise_of(a.player).map_or(-1.0, |s| s as f32),
                ]
            })
            .collect()
    }

    /// Avatar `i`'s vertices: the lead's mesh ([`Self::player_mesh_positions`]'s
    /// rig, so the same index / UV / texture streams) posed at the avatar's
    /// reported clip frame, at the player's render scale.
    pub fn play_net_avatar_positions(&mut self, i: u32) -> Vec<f32> {
        let bones = (|| {
            let net = self.netplay.as_ref()?;
            let h = self.scene_host.host()?;
            let a = *net.remote_avatars().get(i as usize)?;
            let pose = legaia_engine_netplay::avatar_pose::avatar_pose(&h.world, &a)?;
            Some((pose.bone_outputs, h.world.player_render_scale()))
        })();
        let Some((bones, scale)) = bones else {
            return Vec::new();
        };
        let Some(rig) = self.player.as_ref() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        crate::play::pose_into(&mut out, &rig.base.positions, &rig.object_ids, &bones);
        if scale != 1.0 {
            for v in &mut out {
                *v *= scale;
            }
        }
        out
    }
}

/// A browser `WebSocket` to the relay.
#[cfg(target_arch = "wasm32")]
fn open_socket(url: &str) -> Result<Box<dyn legaia_netplay::transport::Transport>, String> {
    legaia_netplay::transport::web::WebSocketTransport::connect(url)
        .map(|t| Box::new(t) as Box<dyn legaia_netplay::transport::Transport>)
        .map_err(|e| format!("Could not open {url}: {e}"))
}

/// A native build of the page (its unit tests) has no browser socket.
#[cfg(not(target_arch = "wasm32"))]
fn open_socket(_url: &str) -> Result<Box<dyn legaia_netplay::transport::Transport>, String> {
    Err("Joining a room needs the browser build.".to_string())
}
