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

use legaia_art::arts_voice::ArtsVoiceTable;
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
/// The Tactical-Arts VOICE banks - the per-character arts **shout** files, as
/// selected by the retail arts-voice cue `FUN_8004C140` (the RE'd linkage, not
/// a guess): when the staged-anim materialiser runs a party art, it fires
/// `FUN_8003D53C(clip_slot = (char)*2 + 1, channel, dur)`, and clip slot `i` =
/// file `XA<i+1>.XA`. So Vahn = `XA2.XA` (slot 1), Noa = `XA4.XA` (slot 3),
/// Gala = `XA6.XA` (slot 5) - all 16-channel short-mono shout banks; Terra has
/// no arts. Capture-verified live: Vahn's Tri-Somersault fires
/// `FUN_8003D53C(0x01=XA2, ...)`, Noa's Miracle fires `(0x03=XA4, ...)`.
///
/// The per-art **channel** is not fixed: retail picks a random member of the
/// art's candidate pool (keyed by the art's action constant) from the SCUS
/// tables parsed by [`legaia_art::arts_voice`]. This page maps each art to a
/// stable member of its real pool. `XA30.XA` is the ordinary directional-attack
/// grunt (different cue); `XA3`/`XA5` are the stereo Miracle/summon fanfares
/// (the `FUN_8004FCC8` jingle path), not the per-art shout. See
/// `docs/subsystems/battle-action.md`.
const VOICE_XA_FILE: [Option<&str>; 4] = [Some("XA2.XA"), Some("XA4.XA"), Some("XA6.XA"), None];
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
    /// The arts-voice XA channel this art plays (a stable member of the art's
    /// real `FUN_8004C140` candidate pool, keyed on the record's `anim_id` =
    /// action constant). `None` when the disc has no voice bank or the art has
    /// no arts-voice entry (retail plays it silent).
    voice_channel: Option<u8>,
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
    /// The character's decoded arts-voice channels (every decoded channel of
    /// its [`VOICE_XA_FILE`], keyed by `DecodedXa::ch_no`), each trimmed of its
    /// trailing silence. Empty on a raw `PROT.DAT` load (no XA files to read),
    /// for Terra, or when the file didn't demux. An art plays the channel in
    /// its [`ArtSlot::voice_channel`].
    voice: Vec<audio::DecodedXa>,
}

impl LoadedCharacter {
    /// The decoded voice clip for XA channel `ch`, if it demuxed.
    fn voice_clip(&self, ch: u8) -> Option<&audio::DecodedXa> {
        self.voice.iter().find(|v| v.ch_no == ch)
    }
}

/// The site's Tactical-Arts animation host: a disc, plus one character's
/// assembled battle mesh + art-clip bank at a time.
#[wasm_bindgen]
pub struct LegaiaArts {
    /// Extracted `PROT.DAT` bytes.
    prot: Vec<u8>,
    /// PROT TOC.
    entries: Vec<disc::EntryMeta>,
    /// Raw 2352-byte sectors of each distinct arts-voice file ([`VOICE_XA_FILE`]
    /// = `XA2.XA` / `XA4.XA` / `XA6.XA`), keyed by upper-case file name. The XA demuxer
    /// needs the CD-XA subheaders, which a 2048-byte ISO file view drops. Empty
    /// when the page was fed a raw `PROT.DAT` instead of a full disc image.
    voice_files: Vec<(String, Vec<u8>)>,
    /// The decoded arts-voice cue tables (`FUN_8004C140`, parsed from
    /// `SCUS_942.54`): art action constant -> candidate voice channel. `None`
    /// on a raw `PROT.DAT` load (no SCUS to read).
    voice_table: Option<ArtsVoiceTable>,
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

/// Frames of an art clip on which the page fires the strike cue.
///
/// **This is a fit, not a traced timing.** Retail times an art's sound from
/// the art record's *Hit Effect Cue* words (`[u16 frame][u16 kind]`, see
/// `docs/formats/art-data.md`) - a field whose offset inside the `0xD0`-stride
/// record is not pinned, and the move-power table's per-move `+0x0d` cue covers
/// enemy specials only (a party art's move id is unmapped - see
/// `docs/formats/move-power.md`). So the page derives the impact frames from
/// the clip itself: the local maxima of the rig's **extension** - the largest
/// per-part translation distance from the rest pose - which is where a swing,
/// kick or lunge reaches full reach. A multi-hit art therefore fires once per
/// swing, and a single-strike art once.
///
/// Frames are returned ascending, at most four (the art record carries four
/// power bytes, so retail lands at most four hits per art).
fn strike_frames(anim: &MonsterAnimation) -> Vec<u32> {
    if anim.frame_count < 3 || anim.part_count == 0 {
        return Vec::new();
    }
    let rest = &anim.frames[0];
    // Per-frame extension: how far the furthest-displaced part has travelled
    // from its rest position.
    let extension: Vec<f32> = anim
        .frames
        .iter()
        .map(|frame| {
            frame
                .iter()
                .zip(rest.iter())
                .map(|(p, r)| {
                    let dx = f32::from(p.tx) - f32::from(r.tx);
                    let dy = f32::from(p.ty) - f32::from(r.ty);
                    let dz = f32::from(p.tz) - f32::from(r.tz);
                    (dx * dx + dy * dy + dz * dz).sqrt()
                })
                .fold(0.0f32, f32::max)
        })
        .collect();
    let peak = extension.iter().copied().fold(0.0f32, f32::max);
    if peak <= 0.0 {
        return Vec::new();
    }
    // Only peaks in the top half of the clip's reach count as a strike; that
    // filters the wind-up and recovery wobble.
    let floor = peak * 0.5;
    let mut hits: Vec<u32> = (1..extension.len() - 1)
        .filter(|&f| {
            extension[f] >= floor
                && extension[f] >= extension[f - 1]
                && extension[f] > extension[f + 1]
        })
        .map(|f| f as u32)
        .collect();
    if hits.is_empty() {
        // Monotone clip (a lunge that ends at full reach): the final frame is
        // the impact.
        let best = extension
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(f, _)| f as u32)
            .unwrap_or(0);
        hits.push(best);
    }
    if hits.len() > 4 {
        // Keep the four biggest, back in frame order.
        hits.sort_by(|a, b| extension[*b as usize].total_cmp(&extension[*a as usize]));
        hits.truncate(4);
        hits.sort_unstable();
    }
    hits
}

/// Slice the raw 2352-byte sectors of the named XA file out of a full
/// Mode2/2352 disc image, so the arts-voice channels can demux after the
/// disc bytes are dropped.
fn extract_named_xa_raw(disc_bytes: &[u8], file_name: &str) -> Option<Vec<u8>> {
    let f = audio::enumerate_xa_files(disc_bytes)
        .into_iter()
        .find(|f| {
            let name = f.path.rsplit(['/', '\\']).next().unwrap_or(&f.path);
            let name = name.split(';').next().unwrap_or(name);
            name.eq_ignore_ascii_case(file_name)
        })?;
    // ISO9660 reports the Form-1 (2048-byte) size; the on-disc footprint is
    // the same sector count of raw 2352-byte sectors.
    let sectors = f.size.div_ceil(2048) as usize;
    let start = f.lba as usize * 2352;
    let end = start.checked_add(sectors * 2352)?.min(disc_bytes.len());
    (start < end).then(|| disc_bytes[start..end].to_vec())
}

/// Slice out every distinct arts-voice file referenced by [`VOICE_XA_FILE`]
/// (`XA2.XA`, `XA4.XA`, `XA6.XA`), keyed by upper-case name for [`decode_voice_bank`].
fn extract_voice_files(disc_bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    for name in VOICE_XA_FILE.into_iter().flatten() {
        let key = name.to_ascii_uppercase();
        if out.iter().any(|(k, _)| *k == key) {
            continue;
        }
        if let Some(raw) = extract_named_xa_raw(disc_bytes, name) {
            out.push((key, raw));
        }
    }
    out
}

/// Trim a decoded voice clip to its audible span: `decode_xa_in_memory`
/// concatenates the channel's whole run of audio sectors, and the arts-voice
/// channels carry a single short shout followed by up to ~2 s of digital
/// silence. Drop everything past the last sample above 3 % of the clip's peak
/// (plus a short tail pad), so the shout plays tight without the trailing gap.
/// (Unlike the retail-timed XA30 grunt, XA2/XA4 have no `dur` read span to
/// window to, so the trim is derived from the audio itself.)
fn trim_trailing_silence(pcm: &mut Vec<i16>) {
    let Some(peak) = pcm.iter().map(|s| s.unsigned_abs()).max() else {
        return;
    };
    if peak == 0 {
        return;
    }
    let thr = (u32::from(peak) * 3 / 100) as u16;
    let last = pcm
        .iter()
        .rposition(|s| s.unsigned_abs() > thr)
        .unwrap_or(0);
    // ~64 ms of pad after the last audible sample (37 800 Hz mono).
    let keep = (last + 2400).min(pcm.len());
    pcm.truncate(keep);
}

/// Demux + decode every channel of character `cslot`'s arts-voice file
/// ([`VOICE_XA_FILE`]) out of the pre-sliced voice files. Each clip is trimmed
/// of its trailing silence and keyed by its own `ch_no`, so an art can address
/// the exact channel its `FUN_8004C140` pool selects.
fn decode_voice_bank(voice_files: &[(String, Vec<u8>)], cslot: usize) -> Vec<audio::DecodedXa> {
    let Some(Some(file_name)) = VOICE_XA_FILE.get(cslot) else {
        return Vec::new();
    };
    let key = file_name.to_ascii_uppercase();
    let Some((_, raw)) = voice_files.iter().find(|(k, _)| *k == key) else {
        return Vec::new();
    };
    let sectors = (raw.len() / 2352) as u32;
    let mut decoded: Vec<audio::DecodedXa> = audio::decode_xa_in_memory(raw, 0, sectors * 2048)
        .into_iter()
        .filter(|s| !s.pcm.is_empty())
        .collect();
    decoded.sort_by_key(|s| s.ch_no);
    for s in &mut decoded {
        trim_trailing_silence(&mut s.pcm);
    }
    decoded
}

/// Build one character's full bundle off the PROT bytes. `Err(reason)` names
/// the first stage that failed; per-art stream failures degrade to
/// `ArtSlot::why` instead (the page falls that art back to the idle pose).
fn build_character(
    prot: &[u8],
    entries: &[disc::EntryMeta],
    voice_files: &[(String, Vec<u8>)],
    voice_table: Option<&ArtsVoiceTable>,
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
                let voice_channel = voice_table.and_then(|t| t.pick_channel(cslot, rec.anim_id));
                arts.push(ArtSlot {
                    anim_id: rec.anim_id,
                    name: rec.name.clone(),
                    combo: rec.combo.clone(),
                    rate: rec.rate,
                    uses_base_archive: rec.uses_base_archive(),
                    anim,
                    why,
                    voice_channel,
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
        voice: decode_voice_bank(voice_files, cslot),
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
            voice_files: Vec::new(),
            voice_table: None,
            current: None,
        }
    }

    /// Load a full Mode2/2352 disc image (or a raw `PROT.DAT`) and parse the
    /// TOC. Returns `{"entries": N}` JSON; errors throw. On a full disc the
    /// arts-voice banks ([`VOICE_XA_FILE`] = `XA2.XA` / `XA4.XA` / `XA6.XA`) are sliced out
    /// alongside `PROT.DAT`; a raw `PROT.DAT` load simply has no voice audio.
    pub fn load_disc(&mut self, bytes: Vec<u8>) -> Result<String, JsValue> {
        let mut voice_files = Vec::new();
        let mut voice_table = None;
        let prot = if disc::is_mode2_2352_disc(&bytes) {
            voice_files = extract_voice_files(&bytes);
            // Arts-voice cue tables (art action constant -> voice channel) live
            // in SCUS_942.54; parse them so each art maps to its real channel.
            voice_table = disc::extract_scus(&bytes)
                .as_deref()
                .and_then(ArtsVoiceTable::parse_from_scus);
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
        self.voice_files = voice_files;
        self.voice_table = voice_table;
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
        match build_character(
            &self.prot,
            &self.entries,
            &self.voice_files,
            self.voice_table.as_ref(),
            cslot,
        ) {
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
                            // The arts-voice XA channel this art plays (its
                            // real FUN_8004C140 pool member), or null.
                            "voice_channel": a.voice_channel,
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
                    // The character's arts-voice bank (null on a raw PROT.DAT
                    // load, for Terra, or if the file didn't demux): its XA
                    // file + every decoded channel's metadata, keyed by the
                    // real XA `channel`. Each art's channel is in its `arts[]`
                    // entry's `voice_channel` (see `art_voice_pcm_i16`).
                    "voice": (!c.voice.is_empty()).then(|| serde_json::json!({
                        "file": VOICE_XA_FILE[c.cslot],
                        "count": c.voice.len(),
                        "channels": c.voice.iter().map(|v| serde_json::json!({
                            "channel": v.ch_no,
                            "rate": v.sample_rate,
                            "stereo": v.stereo,
                            "samples": v.pcm.len(),
                        })).collect::<Vec<_>>(),
                    })),
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

    /// Frames of art clip `index` on which the page should fire the strike
    /// sound cue ([`Self::art_strike_cue`]), ascending. See [`strike_frames`]
    /// for what they are and why they are a fit rather than a traced timing.
    /// Empty when the clip didn't decode.
    pub fn art_strike_frames(&self, index: u32) -> Vec<u32> {
        self.current
            .as_ref()
            .and_then(|c| c.arts.get(index as usize))
            .and_then(|a| a.anim.as_ref())
            .map(strike_frames)
            .unwrap_or_default()
    }

    /// The SFX cue id an art strike fires: the art record's documented generic
    /// "play sound" Hit Effect Cue kind. Resolve it to audio through
    /// [`crate::sfx_view::LegaiaSfx`].
    pub fn art_strike_cue(&self) -> u32 {
        crate::sfx_view::CUE_ART_STRIKE as u32
    }

    /// The arts-voice PCM for the art at bank index `art_index`: mono i16 at
    /// the rate reported in `set_character`'s `voice.channels[..].rate`
    /// (37 800 Hz). The clip is the XA channel the art's `FUN_8004C140`
    /// candidate pool selects (the art's `voice_channel`), trimmed of its
    /// trailing silence. Empty when the character has no voice bank (raw
    /// `PROT.DAT` load, Terra, demux failure) or the art has no voice entry.
    /// Also exposed by-channel via [`Self::voice_channel_pcm_i16`].
    pub fn art_voice_pcm_i16(&self, art_index: u32) -> Vec<i16> {
        self.current
            .as_ref()
            .and_then(|c| {
                let a = c.arts.get(art_index as usize)?;
                let ch = a.voice_channel?;
                Some(c.voice_clip(ch)?.pcm.clone())
            })
            .unwrap_or_default()
    }

    /// The arts-voice PCM of the current character's XA channel `channel`,
    /// regardless of any art mapping. Lets the page (and the listening aid)
    /// address a specific voice clip directly. Empty when out of range or the
    /// character has no voice bank.
    pub fn voice_channel_pcm_i16(&self, channel: u32) -> Vec<i16> {
        self.current
            .as_ref()
            .and_then(|c| c.voice_clip(channel as u8))
            .map(|v| v.pcm.clone())
            .unwrap_or_default()
    }
}
