//! The dancer bodies' exports: cast JSON, per-dancer mesh buffers, clip
//! dimensions and pose frames, and the body VRAM.

use super::*;

#[wasm_bindgen]
impl LegaiaMinigames {
    /// Whether the dance's art pack + widget table decoded off this disc.
    /// When `false` the page falls back to its own glyphs - and says so.
    pub fn dance_art_ready(&self) -> bool {
        self.dance_pres.is_some()
    }

    // --------------------------------------------------------- dancer bodies
    //
    // The dance overlay draws no mesh of its own; its spawner (FUN_801d0190)
    // pulls each dancer kind from a baked descriptor table. Noa (the human
    // dancer, centre of the retail floor) is her real field-view model - the
    // same mesh the site's play / field view walks - and the AI dancers are
    // the dance-hall scene module's dedicated dancer NPCs (`other7` scene TMD
    // pool; face-strip rigs 2 and 3 in qualifier mode). The page poses them
    // off the scene's 60-record choreography ANM bundle (PROT 1229): the
    // dance-groove loop synced to the beat clock plus the judge-triggered
    // move clips. This is the browser twin of the Baka Fighter 3D render
    // (`baka_presentation.rs`): same VramMesh accessors, same per-(frame, bone)
    // pose format, so `site/js/minigame-dance.js` drives the shared
    // `TmdRenderer` exactly as `minigame-baka.js` does.

    /// Whether the dance cast (Noa + the dancer NPCs) and the choreography
    /// bundle decoded off this disc.
    pub fn dance_body_ready(&self) -> bool {
        self.dance_bodies.is_some()
    }

    /// Number of dancer bodies (3 on the qualifier floor: left / centre /
    /// right).
    pub fn dance_body_count(&self) -> u32 {
        self.dance_bodies
            .as_ref()
            .map(|b| b.dancers.len() as u32)
            .unwrap_or(0)
    }

    /// Display index of the human dancer (Noa - the centre of the retail
    /// qualifier floor).
    pub fn dance_body_human_index(&self) -> u32 {
        self.dance_bodies
            .as_ref()
            .map(|b| b.human as u32)
            .unwrap_or(0)
    }

    /// The live run's floor cast as dancer kinds, in spawn-slot order - the
    /// engine's `DanceGame::dancer_kind` over `dancer_count`. One entry on a
    /// how-to run, three on the qualifier / finals, six in free play; empty
    /// with no run. The page draws a body only while its kind is in here, so
    /// the floor shows the mode's own cast.
    pub fn dance_run_kinds(&self) -> Vec<u32> {
        self.dance
            .as_ref()
            .map(|g| {
                (0..g.dancer_count())
                    .map(|i| g.dancer_kind(i) as u32)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Dancer `dancer`'s kind descriptor index (0 = Noa, 1 = Mary, 2/3 = the
    /// competitor dancers, 4 = the Disco King) - also the face-stamp rig id
    /// for kinds 0..=3. `255` when out of range.
    pub fn dance_body_kind(&self, dancer: u32) -> u32 {
        self.dance_body(dancer)
            .map(|d| d.kind as u32)
            .unwrap_or(255)
    }

    /// The decoded cast + choreography map, so the page drives retail clips
    /// rather than invented ones:
    ///
    /// ```json
    /// { "human": 1,
    ///   "dancers": [
    ///     { "kind": 2, "model": 62, "x": 5952, "z": 13440,
    ///       "clips": [ { "id": 0, "record": 32, "frames": 20, "rate": 8,
    ///                    "party_bank": false }, ... ] }, ... ],
    ///   "moves": { "miss_square": 2, "miss_circle": 3,
    ///              "seq_square": [4, 6, 8], "seq_circle": [5, 7, 9],
    ///              "beat": [10, 11, 12] } }
    /// ```
    ///
    /// Clip ids: `0` = idle (pre-game), `1` = the dance-groove loop, `2 + k` =
    /// judge-triggered move pair `k` (`FUN_801d1af4`'s return, in pair units).
    /// The `moves` map gives the clip id per judge event on each difficulty
    /// lane. `"[]"`-empty when the cast didn't decode.
    pub fn dance_cast_json(&self) -> String {
        let Some(b) = self.dance_bodies.as_ref() else {
            return "null".to_string();
        };
        let dancers = b
            .dancers
            .iter()
            .enumerate()
            .map(|(di, d)| {
                let clips = (0..DANCE_CLIPS as u32)
                    .map(|c| {
                        let clip = self.dance_clip(di as u32, c);
                        let (record, frames, rate, trans) = clip
                            .map(|cl| {
                                let rec = cl.record_index();
                                let frames = rec
                                    .and_then(|r| b.anm.record_lenient(r).ok())
                                    .map(|r| r.frame_count)
                                    .unwrap_or(0);
                                (
                                    rec.map(|r| r as i32).unwrap_or(-1),
                                    frames,
                                    cl.rate,
                                    cl.party_bank,
                                )
                            })
                            .unwrap_or((-1, 0, 0, false));
                        format!(
                            r#"{{"id":{c},"record":{record},"frames":{frames},"rate":{rate},"party_bank":{trans}}}"#
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                let model = b.cast.kinds.get(d.kind).map(|k| k.model).unwrap_or(0);
                format!(
                    r#"{{"kind":{},"model":{},"x":{},"z":{},"clips":[{}]}}"#,
                    d.kind, model, d.spawn.0, d.spawn.1, clips
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let seq = |circle: bool| {
            (0..3)
                .map(|lane| (2 + dance_cast::move_sequence_pair(lane, circle)).to_string())
                .collect::<Vec<_>>()
                .join(",")
        };
        let beat = (0..3)
            .map(|lane| (2 + dance_cast::move_beat_pair(lane)).to_string())
            .collect::<Vec<_>>()
            .join(",");
        format!(
            concat!(
                r#"{{"human":{},"dancers":[{}],"moves":{{"miss_square":{},"miss_circle":{},"#,
                r#""seq_square":[{}],"seq_circle":[{}],"beat":[{}]}}}}"#
            ),
            b.human,
            dancers,
            2 + dance_cast::MOVE_MISS_SQUARE,
            2 + dance_cast::MOVE_MISS_CIRCLE,
            seq(false),
            seq(true),
            beat,
        )
    }

    /// Per-vertex positions of dancer `dancer`'s body (object-local; the pose
    /// assembles them). Empty when the bodies didn't decode.
    pub fn dance_body_positions(&self, dancer: u32) -> Vec<f32> {
        let Some(d) = self.dance_body(dancer) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(d.mesh.positions.len() * 3);
        for p in &d.mesh.positions {
            out.extend_from_slice(&[p[0], p[1], p[2]]);
        }
        out
    }

    /// Per-vertex `[u, v]` texel coords, parallel to the positions.
    pub fn dance_body_uvs(&self, dancer: u32) -> Vec<i32> {
        let Some(d) = self.dance_body(dancer) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(d.mesh.uvs.len() * 2);
        for uv in &d.mesh.uvs {
            out.extend_from_slice(&[uv[0] as i32, uv[1] as i32]);
        }
        out
    }

    /// Per-vertex `[cba, tsb]`, parallel to the positions.
    pub fn dance_body_cba_tsb(&self, dancer: u32) -> Vec<u32> {
        let Some(d) = self.dance_body(dancer) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(d.mesh.cba_tsb.len() * 2);
        for ct in &d.mesh.cba_tsb {
            out.extend_from_slice(&[ct[0] as u32, ct[1] as u32]);
        }
        out
    }

    /// Triangle indices for dancer `dancer`'s body.
    pub fn dance_body_indices(&self, dancer: u32) -> Vec<u32> {
        self.dance_body(dancer)
            .map(|d| d.mesh.indices.clone())
            .unwrap_or_default()
    }

    /// Per-vertex TMD object index (the bone a vertex hangs from), parallel to
    /// the positions - the animator keys `R . v + T` on this.
    pub fn dance_body_object_ids(&self, dancer: u32) -> Vec<u32> {
        self.dance_body(dancer)
            .map(|d| d.object_ids.clone())
            .unwrap_or_default()
    }

    /// Per-vertex `[r, g, b, textured_flag]` for the hybrid textured / flat
    /// shader path (the field body mixes textured skin with flat body prims).
    pub fn dance_body_flat_rgba(&self, dancer: u32) -> Vec<u8> {
        self.dance_body(dancer)
            .map(|d| d.flat.clone())
            .unwrap_or_default()
    }

    /// TMD object count (pose rig width) of dancer `dancer`'s body.
    pub fn dance_body_part_count(&self, dancer: u32) -> u32 {
        self.dance_body(dancer)
            .map(|d| d.part_count as u32)
            .unwrap_or(0)
    }

    /// `[bone_count, frame_count]` of dancer `dancer`'s clip slot `clip`
    /// (0 = idle, 1 = the dance loop, `2 + k` = move pair `k`). Lenient on
    /// the record-size invariant: several choreography records carry frame
    /// data past the header count that the retail cursor never plays.
    pub fn dance_body_anim_dims(&self, dancer: u32, clip: u32) -> Vec<u32> {
        let Some((bundle, record)) = self.dance_anim_record(dancer, clip) else {
            return vec![0, 0];
        };
        match bundle.record_lenient(record) {
            Ok(r) => vec![r.bone_count as u32, r.frame_count as u32],
            Err(_) => vec![0, 0],
        }
    }

    /// Dancer `dancer`'s clip slot `clip` decoded to absolute per-(frame,
    /// bone) `[tx, ty, tz, rx, ry, rz]` (PSX 4096-unit angles), padded to
    /// `target_part_count` parts - the same pose stream the site's mesh
    /// animator consumes (identical shape to `baka_anim_pose_frames`).
    pub fn dance_body_pose_frames(
        &self,
        dancer: u32,
        clip: u32,
        target_part_count: u32,
    ) -> Vec<i32> {
        let Some((bundle, record)) = self.dance_anim_record(dancer, clip) else {
            return Vec::new();
        };
        let Ok(rec) = bundle.record_lenient(record) else {
            return Vec::new();
        };
        let bones = rec.bone_count as usize;
        let frames = rec.frame_count as usize;
        let parts = (target_part_count as usize).max(bones);
        let mut out = Vec::with_capacity(frames * parts * 6);
        for f in 0..frames {
            for p in 0..parts {
                if p < bones {
                    let Some(t) = bundle.bone_transform(record, f, p) else {
                        return Vec::new();
                    };
                    out.extend_from_slice(&[t.t_x, t.t_y, t.t_z, t.r_x, t.r_y, t.r_z]);
                } else {
                    out.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
                }
            }
        }
        out
    }

    /// The 1 MB PSX VRAM the dancer bodies sample: the dance-hall scene's
    /// full TIM upload (the dancer NPC atlases + their row-480/481 CLUTs)
    /// merged with the PROT 0874 §2 field-character textures (Noa's atlas,
    /// row-478 CLUTs). Empty when the cast didn't decode.
    pub fn dance_body_vram(&self) -> Vec<u8> {
        self.dance_bodies
            .as_ref()
            .map(|b| b.vram.clone())
            .unwrap_or_default()
    }

    /// Does [`Self::dance_body_vram`] carry the HUD's own texture page and
    /// CLUT strip? `false` means every HUD quad would sample texels that
    /// belong to something else, so a host draws the placeholder instead.
    pub fn dance_hud_art_staged(&self) -> bool {
        self.dance_bodies.as_ref().is_some_and(|b| b.hud_staged)
    }
}
