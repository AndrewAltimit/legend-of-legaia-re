//! `LegaiaArts` WASM bindings for site/arts.html - the interactive Tactical
//! Arts viewer: click an art, watch the character perform it.
//!
//! The load path is the retail battle loader's own asset chain, the same one
//! the play-window's `--player-battle` drives natively:
//!
//! * **mesh** - assembled per character from the player battle file's
//!   equipment-id sections (extraction PROT `863 + char`,
//!   `legaia_asset::battle_char_assembly::assemble_character`), relocated into
//!   runtime VRAM band 0 (`relocate_tsb_cba`, the `FUN_800513F0`
//!   registration-time TSB/CBA pass);
//! * **texture** - the equipped sections' texture pools + record[0] image
//!   blocks at the pinned `FUN_80052FA0` placement
//!   (`character_texture_uploads`), with the character's decoded battle
//!   palette overlaid on the CLUT rows the mesh samples
//!   (`legaia_asset::battle_char_palette`);
//! * **idle** - the character's own record[0] action-slot-0 keyframe stream
//!   (`idle_battle_animation`), the loop retail holds between commands;
//! * **arts** - the art-animation bank at record[0] `+0x58`
//!   (`art_animation_bank`), each record's keyframe stream resolved through
//!   the character's `readef.DAT` `"ME"` archive (PROT 894, slots
//!   `3*char + 1` / `3*char + 2` - `art_me_archive` / `art_animation`,
//!   the `FUN_8004AD80` staged-anim materialization).
//!
//! Every animation is pre-expanded per assembled TMD object
//! (`expand_animation_for_objects` over the assembly's `anm_bones`), so the
//! JS pose loop can apply channel `i` to object `i` directly - the same flat
//! `[tx, ty, tz, rx, ry, rz]` per (frame, part) layout the monsters page
//! consumes. Everything decodes from the visitor's own disc in the browser;
//! no Sony bytes ship with the site.

use super::*;

use legaia_asset::battle_char_assembly as bca;
use legaia_asset::monster_archive::MonsterAnimation;

/// Log a decode degradation. Browser console on wasm; stderr natively (the
/// disc-gated tests drive this module natively).
fn arts_log(s: &str) {
    #[cfg(target_arch = "wasm32")]
    console_log(s);
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("{s}");
}

/// Player battle files (`data\battle\PLAYER1..4`) start at extraction PROT
/// entry 863 (Vahn), one per character slot.
const PLAYER_FILE_BASE: u32 = 863;
/// `readef.DAT` (extraction PROT 894) - the battle side-band file carrying
/// the per-character art `"ME"` stream archives.
const READEF_PROT_INDEX: u32 = 894;
/// Character labels in player-file order.
const CHARACTER_LABELS: [&str; 4] = ["Vahn", "Noa", "Gala", "Terra"];

/// One art-bank record and (when it decoded) its expanded keyframe clip.
struct ArtSlot {
    anim_id: u8,
    name: String,
    combo: Vec<u8>,
    rate: u8,
    uses_base_archive: bool,
    /// Clip expanded per assembled object (channel `i` drives object `i`).
    anim: Option<MonsterAnimation>,
    /// Present when the record's stream failed to resolve/decode.
    why: Option<String>,
}

/// One character's fully-decoded view bundle, cached on the host so the
/// per-buffer accessors don't re-run the assembly.
struct LoadedCharacter {
    cslot: usize,
    /// Assembled TMD object count = pose rig width (`anm_bones.len()`).
    part_count: usize,
    mesh: legaia_tmd::mesh::VramMesh,
    object_ids: Vec<u32>,
    vram: Vec<u8>,
    /// Idle loop (record[0] slot 0), expanded per object.
    idle: Option<MonsterAnimation>,
    arts: Vec<ArtSlot>,
}

/// The site's Tactical-Arts animation host: a disc, plus one character's
/// assembled battle mesh + art-clip bank at a time.
#[wasm_bindgen]
pub struct LegaiaArts {
    /// Extracted `PROT.DAT` bytes.
    prot: Vec<u8>,
    /// PROT TOC.
    entries: Vec<disc::EntryMeta>,
    /// The character currently on the canvas.
    current: Option<LoadedCharacter>,
}

impl Default for LegaiaArts {
    fn default() -> Self {
        Self::new()
    }
}

/// Read one PROT entry's raw on-disc bytes (the web `parse_prot_toc` already
/// honours the extended footprint window).
fn entry_bytes<'a>(
    prot: &'a [u8],
    entries: &[disc::EntryMeta],
    prot_index: u32,
) -> Option<&'a [u8]> {
    let meta = entries.iter().find(|e| e.index == prot_index)?;
    let off = meta.byte_offset as usize;
    let end = off.checked_add(meta.size_bytes as usize)?;
    prot.get(off..end.min(prot.len()))
}

/// Flatten a clip to the site animators' pose layout: six `i32` per part per
/// frame, `[tx, ty, tz, rx, ry, rz]`, frame `f` / part `p` / component `c` at
/// `(f * part_count + p) * 6 + c`. Rotations are unsigned 12-bit angles
/// (`4096` = a full turn).
fn flatten_pose_frames(anim: &MonsterAnimation) -> Vec<i32> {
    let mut out = Vec::with_capacity(anim.frame_count * anim.part_count * 6);
    for frame in &anim.frames {
        for p in frame {
            out.extend_from_slice(&[
                p.tx as i32,
                p.ty as i32,
                p.tz as i32,
                p.rx as i32,
                p.ry as i32,
                p.rz as i32,
            ]);
        }
    }
    out
}

/// Build one character's full bundle off the PROT bytes. `Err(reason)` names
/// the first stage that failed; per-art stream failures degrade to
/// `ArtSlot::why` instead (the page falls that art back to the idle pose).
fn build_character(
    prot: &[u8],
    entries: &[disc::EntryMeta],
    cslot: usize,
) -> Result<LoadedCharacter, String> {
    let prot_index = PLAYER_FILE_BASE + cslot as u32;
    let raw = entry_bytes(prot, entries, prot_index)
        .ok_or_else(|| format!("player file (PROT {prot_index}) not present"))?;
    let pack =
        legaia_asset::battle_data_pack::parse(raw).map_err(|e| format!("player-file pack: {e}"))?;
    // All-default (unequipped) sections: the arts showcase has no roster, and
    // every section id 0 is the section's default variant.
    let equipped = [0u8; 5];
    let mut asm = bca::assemble_character(raw, &pack, &equipped)
        .map_err(|e| format!("battle-mesh assembly: {e}"))?;
    // Band 0: texpages (512, 256)/(576, 256), CLUT row 481.
    bca::relocate_tsb_cba(&mut asm.tmd, 0).map_err(|e| format!("TSB/CBA relocation: {e}"))?;
    let tmd = legaia_tmd::parse(&asm.tmd).map_err(|e| format!("assembled TMD parse: {e}"))?;
    let (mesh, object_ids) = legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids(&tmd, &asm.tmd);
    if mesh.indices.is_empty() {
        return Err("assembled mesh has no textured primitives".to_string());
    }

    // ---- VRAM: band-0 pixels + the character's decoded battle palette ----
    let mut vram = legaia_tim::Vram::new();
    match bca::character_texture_uploads(raw, &pack, &equipped, 0) {
        Ok(uploads) => {
            for u in &uploads {
                vram.write_block(u.fb_x(), u.fb_y(), u.rect.w, u.rect.h, &u.pixels);
                if !u.clut.is_empty() {
                    vram.write_clut_row(u.clut_x, u.clut_row(), &u.clut_bytes());
                }
            }
        }
        Err(e) => arts_log(&format!("arts: char {cslot} texture-pool decode: {e}")),
    }
    // CLUT rows / columns the relocated mesh actually samples.
    let mut rows: Vec<u16> = mesh.cba_tsb.iter().map(|c| (c[0] >> 6) & 0x1FF).collect();
    rows.sort_unstable();
    rows.dedup();
    let mut cols: Vec<u16> = mesh.cba_tsb.iter().map(|c| (c[0] & 0x3F) * 16).collect();
    cols.sort_unstable();
    cols.dedup();
    // Vahn (863) = the byte-exact fixed-stride record parse; the others
    // (864..866, incl. Terra) = the equipment-robust collector.
    let pal = if cslot == 0 {
        legaia_asset::battle_char_palette::find_record0(raw)
            .and_then(|rec0| legaia_asset::battle_char_palette::parse_record(raw, rec0).ok())
    } else {
        legaia_asset::battle_char_palette::collect_palette(raw, 0, &cols).ok()
    };
    if let Some(pal) = pal {
        for &row in &rows {
            for band in &pal.bands {
                let bytes: Vec<u8> = band
                    .vram_words()
                    .iter()
                    .flat_map(|w| w.to_le_bytes())
                    .collect();
                vram.write_clut_row(band.base, row, &bytes);
            }
        }
    }

    // ---- Idle loop (rest pose source), expanded per object ----
    let idle = match bca::idle_battle_animation(raw) {
        Ok(Some(anim)) => Some(bca::expand_animation_for_objects(&anim, &asm.anm_bones)),
        Ok(None) => None,
        Err(e) => {
            arts_log(&format!("arts: char {cslot} idle-stream decode: {e}"));
            None
        }
    };

    // ---- Art-animation bank through the readef "ME" archives ----
    let mut arts = Vec::new();
    match bca::decode_record0(raw).and_then(|r0| bca::art_animation_bank(&r0)) {
        Ok(records) => {
            let readef = entry_bytes(prot, entries, READEF_PROT_INDEX);
            let main = readef.and_then(|r| bca::art_me_archive(r, cslot, false).ok());
            let base = readef.and_then(|r| bca::art_me_archive(r, cslot, true).ok());
            for rec in &records {
                let archive = if rec.uses_base_archive() {
                    base.as_ref()
                } else {
                    main.as_ref()
                };
                let (anim, why) = match archive {
                    Some(a) => match bca::art_animation(rec, a) {
                        Ok(anim) => (
                            Some(bca::expand_animation_for_objects(&anim, &asm.anm_bones)),
                            None,
                        ),
                        Err(e) => (None, Some(format!("{e:#}"))),
                    },
                    None => (None, Some("ME archive did not decode".to_string())),
                };
                arts.push(ArtSlot {
                    anim_id: rec.anim_id,
                    name: rec.name.clone(),
                    combo: rec.combo.clone(),
                    rate: rec.rate,
                    uses_base_archive: rec.uses_base_archive(),
                    anim,
                    why,
                });
            }
        }
        Err(e) => return Err(format!("art-animation bank: {e}")),
    }

    Ok(LoadedCharacter {
        cslot,
        part_count: asm.anm_bones.len(),
        mesh,
        object_ids,
        vram: vram.as_bytes().to_vec(),
        idle,
        arts,
    })
}

#[wasm_bindgen]
impl LegaiaArts {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        #[cfg(target_arch = "wasm32")]
        console_error_panic_hook::set_once();
        Self {
            prot: Vec::new(),
            entries: Vec::new(),
            current: None,
        }
    }

    /// Load a full Mode2/2352 disc image (or a raw `PROT.DAT`) and parse the
    /// TOC. Returns `{"entries": N}` JSON; errors throw.
    pub fn load_disc(&mut self, bytes: Vec<u8>) -> Result<String, JsValue> {
        let prot = if disc::is_mode2_2352_disc(&bytes) {
            disc::extract_prot_dat(&bytes)
                .ok_or_else(|| JsValue::from_str("arts: PROT.DAT not found in this disc image"))?
        } else {
            bytes
        };
        let entries = disc::parse_prot_toc(&prot)
            .ok_or_else(|| JsValue::from_str("arts: PROT.DAT TOC parse failed"))?;
        #[cfg(target_arch = "wasm32")]
        console_log(&format!(
            "Arts: PROT.DAT loaded ({} entries)",
            entries.len()
        ));
        self.prot = prot;
        self.entries = entries;
        self.current = None;
        Ok(format!("{{\"entries\":{}}}", self.entries.len()))
    }

    /// Assemble character `cslot` (0=Vahn, 1=Noa, 2=Gala, 3=Terra) and decode
    /// its art-clip bank. Returns a JSON summary the page keys everything on:
    ///
    /// ```json
    /// { "ok": true, "character": "Vahn", "part_count": 17,
    ///   "idle": { "frames": 24, "rate": 2 },
    ///   "arts": [ { "index": 0, "anim_id": 16, "name": "", "combo": [3,3],
    ///               "rate": 4, "base": true, "ok": true, "frames": 20,
    ///               "why": null }, ... ] }
    /// ```
    ///
    /// `name` is the record's inline HUD art-name string (empty on the
    /// un-named base records); `combo` the arts-matcher direction bytes
    /// (`1=L 2=R 3=D 4=U`) - the page matches its curated art cards against
    /// both. `{"ok":false,"why":...}` when the character doesn't assemble.
    pub fn set_character(&mut self, cslot: u32) -> String {
        let cslot = cslot as usize;
        if cslot >= CHARACTER_LABELS.len() {
            self.current = None;
            return r#"{"ok":false,"why":"character slot out of range"}"#.to_string();
        }
        match build_character(&self.prot, &self.entries, cslot) {
            Ok(c) => {
                let arts: Vec<serde_json::Value> = c
                    .arts
                    .iter()
                    .enumerate()
                    .map(|(i, a)| {
                        serde_json::json!({
                            "index": i,
                            "anim_id": a.anim_id,
                            "name": a.name,
                            "combo": a.combo,
                            "rate": a.rate,
                            "base": a.uses_base_archive,
                            "ok": a.anim.is_some(),
                            "frames": a.anim.as_ref().map(|x| x.frame_count).unwrap_or(0),
                            "why": a.why,
                        })
                    })
                    .collect();
                let json = serde_json::json!({
                    "ok": true,
                    "character": CHARACTER_LABELS[c.cslot],
                    "part_count": c.part_count,
                    "idle": c.idle.as_ref().map(|i| serde_json::json!({
                        "frames": i.frame_count,
                        "rate": i.rate,
                    })),
                    "arts": arts,
                })
                .to_string();
                self.current = Some(c);
                json
            }
            Err(why) => {
                self.current = None;
                serde_json::json!({ "ok": false, "why": why }).to_string()
            }
        }
    }

    /// Per-vertex positions of the current character's assembled battle mesh
    /// (flat `f32`, 3 per vertex). Empty until [`Self::set_character`].
    pub fn mesh_positions(&self) -> Vec<f32> {
        let Some(c) = &self.current else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(c.mesh.positions.len() * 3);
        for p in &c.mesh.positions {
            out.extend_from_slice(&[p[0], p[1], p[2]]);
        }
        out
    }

    /// Per-vertex `[u, v]` integer texel coords, parallel to the positions.
    pub fn mesh_uvs(&self) -> Vec<i32> {
        let Some(c) = &self.current else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(c.mesh.uvs.len() * 2);
        for uv in &c.mesh.uvs {
            out.extend_from_slice(&[uv[0] as i32, uv[1] as i32]);
        }
        out
    }

    /// Per-vertex `[cba, tsb]`, parallel to the positions.
    pub fn mesh_cba_tsb(&self) -> Vec<u32> {
        let Some(c) = &self.current else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(c.mesh.cba_tsb.len() * 2);
        for ct in &c.mesh.cba_tsb {
            out.extend_from_slice(&[ct[0] as u32, ct[1] as u32]);
        }
        out
    }

    /// Triangle indices (`u32`, multiple of 3).
    pub fn mesh_indices(&self) -> Vec<u32> {
        self.current
            .as_ref()
            .map(|c| c.mesh.indices.clone())
            .unwrap_or_default()
    }

    /// Per-vertex TMD object index (the bone a vertex hangs from), parallel
    /// to the positions.
    pub fn mesh_object_ids(&self) -> Vec<u32> {
        self.current
            .as_ref()
            .map(|c| c.object_ids.clone())
            .unwrap_or_default()
    }

    /// Bounding sphere `[cx, cy, cz, r]` (vertex centroid + max distance),
    /// so the page can frame the model before the first pose lands.
    pub fn mesh_bounds(&self) -> Vec<f32> {
        let Some(c) = &self.current else {
            return vec![0.0; 4];
        };
        if c.mesh.positions.is_empty() {
            return vec![0.0; 4];
        }
        centroid_bounds(&c.mesh.positions)
    }

    /// The 1 MB PSX VRAM for the current character: band-0 texture pixels at
    /// the pinned retail placement + the character's decoded battle palette.
    pub fn vram_bytes(&self) -> Vec<u8> {
        self.current
            .as_ref()
            .map(|c| c.vram.clone())
            .unwrap_or_default()
    }

    /// The idle loop's pose frames (see [`flatten_pose_frames`] layout).
    /// Empty when the character has no decodable idle stream.
    pub fn idle_pose_frames(&self) -> Vec<i32> {
        self.current
            .as_ref()
            .and_then(|c| c.idle.as_ref())
            .map(flatten_pose_frames)
            .unwrap_or_default()
    }

    /// Art clip `index`'s pose frames (the position in `set_character`'s
    /// `arts` array). Empty when the index is out of range or that record's
    /// stream did not decode - the page falls back to the idle pose.
    pub fn art_pose_frames(&self, index: u32) -> Vec<i32> {
        self.current
            .as_ref()
            .and_then(|c| c.arts.get(index as usize))
            .and_then(|a| a.anim.as_ref())
            .map(flatten_pose_frames)
            .unwrap_or_default()
    }
}
