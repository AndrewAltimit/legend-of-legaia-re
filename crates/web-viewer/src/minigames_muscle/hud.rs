//! The retail HUD: sheets, banners, course ladder and hub quads.
//! Split out of `minigames_muscle.rs`.

use super::*;

// ------------------------------------------------------------- retail HUD
//
// The dome presents as a standard battle, and its whole on-screen chrome is
// disc data reachable without a save: the chip/plate art + D-pad live in a
// boot-time TIM in the unindexed pre-`init_data` gap of PROT.DAT (pixels at
// VRAM (896, 256), CLUT bank packed into row 511 as 16 sub-palettes), the
// chip-label font is the gap's 256x256 ASCII TIM at (896, 0) (drawn through
// the menu-glyph atlas's CLUT bank on row 510, sub-palette 13), the small
// digits are the menu-glyph atlas itself at (960, 256), and the arts-banner
// words / big damage numerals / red cross-out X are the third TIM of the
// battle-effect bank `etim` (extraction 0870) at (448, 0) with CLUT row 476.
// The dome-hub art (the "Welcome to the Muscle Dome!" cursive, INTERVAL /
// ROUND headings, hub digit strip) is the two-TIM LZS payload of the dome's
// own data file (extraction 1220, `other6.lzs` slot 0), whose on-screen
// geometry is the PROT 0977 overlay's sprite descriptor table
// (`legaia_engine_ui::other_game_hud::parse_sprite_table`).
//
// Every piece rect below is capture-pinned: a live PCSX-Redux Muscle Dome
// battle (the `minigame_muscle_dome_pcsx` scenario driven into the match)
// was snapshotted at the command cluster, an enemy art, and a player HYPER
// ARTS!! playback, and the GP0 packet stream + VRAM were read out of the
// savestates (`scripts/pcsx-redux/autorun_muscle_hud_capture.lua`). The
// sprite geometry (screen anchors, glide endpoints, widths) additionally
// lives in the SCUS-static screen-element placement table at `0x80076C10`
// (24-byte stride; 80 of its records are the dome's HUD), exported raw below.
// That base carries one table under several historical names - see
// `docs/reference/memory-map.md`.

/// `PROT.DAT` file offset of the battle-chrome widget TIM (plates, D-pad,
/// AP-plate art, HP/MP badges) in the unindexed pre-`init_data` gap.
/// Uploads to VRAM (896, 256) 256x192; CLUT bank -> row 511.
pub(super) const HUD_WIDGET_TIM_OFFSET: usize = 0x18E0;

/// `PROT.DAT` gap offset of the 256x256 ASCII battle font TIM -> (896, 0).
pub(super) const HUD_FONT_TIM_OFFSET: usize = 0x7F40;

/// `PROT.DAT` gap offset of the menu-glyph atlas TIM -> (960, 256); its
/// CLUT bank packs into VRAM row 510 (16 sub-palettes).
pub(super) const HUD_MENU_ATLAS_TIM_OFFSET: usize = 0x11218;

/// PROT entry of the battle-effect TIM bank (`etim`, `befect_data`).
pub(super) const HUD_ETIM_PROT_INDEX: u32 = 870;

/// Offset of `etim`'s third TIM - the arts-banner / damage-numeral / red-X
/// page -> VRAM (448, 0), CLUT row 476.
pub(super) const HUD_BANNER_TIM_OFFSET: usize = 0x10450;

/// PROT entry (extraction space) of the dome data container
/// (`other6.lzs` slot 0): LZS section 0 carries the two hub-page TIMs.
pub(super) const HUD_HUB_CONTAINER_PROT_INDEX: u32 =
    legaia_asset::muscle_dome::HUB_CONTAINER_PROT_INDEX;

/// HUD sheet id of the ringside still (`int.tim` / `int2.tim`, extraction
/// 1221 / 1222, 320x256 BGR555); its `palette` is the still variant.
pub(crate) const HUD_STILL_SHEET: u32 = 7;

/// `PROT.DAT` gap offset of the small pad-button-glyph TIM (the four
/// button circles + the R1/R2/L1/L2 labels): image -> VRAM (928, 352)
/// (page (896,256) local texels (128,96)..(192,128)), own 16-entry CLUT ->
/// (304, 511). The arts-input caption's green Triangle circle is its local
/// rect (48, 0, 16, 16) - the recomp GP0 capture of the input screen draws
/// it as `uv (176,96) clut (304,511)` on the widget page.
pub(super) const HUD_BUTTON_TIM_OFFSET: usize = 0x7B00;

/// The arts command-input piece rects (recomp GP0 packet capture of a live
/// dome input screen + Triangle arts list; every rect and palette index is
/// byte-read out of the captured SPRT/FT4/shaded-quad words -
/// `docs/subsystems/minigame-muscle-dome.md` "Arts command input"). Split
/// out of [`LegaiaMinigames::muscle_hud_json`]'s `json!` so the macro stays
/// under the recursion limit.
///
/// Every rect here comes from the shared
/// [`legaia_engine_ui::arts_input`] module (which the battle hosts draw
/// through directly): retail runs one input screen for the dome's Attack
/// and for the battle Arts command, so the dome page must not carry its
/// own copy of the numbers. The page samples the widget page in **texel**
/// space, hence [`ArtsInputAtlasRects::SHEET`] rather than the hosts'
/// baked-atlas variant.
pub(super) fn arts_input_pieces() -> serde_json::Value {
    use legaia_asset::title_pak as tp;
    use legaia_engine_ui::arts_input::{ArtsInputAtlasRects, ChipDirection};
    let r = ArtsInputAtlasRects::SHEET;
    let rect = |t: (u32, u32, u32, u32)| serde_json::json!([t.0, t.1, t.2, t.3]);
    serde_json::json!({
        "cmd_chip": {"body": rect(r.chip_body), "cap_l": rect(r.chip_cap_l),
                      "cap_r": rect(r.chip_cap_r),
                      "pal": tp::OVERLAY_SYSTEM_UI_ARTS_CHIP_CLUT_ROW},
        "cmd_label": {"u": r.label_u, "w": r.label_w, "h": r.label_h,
                       "pal": tp::OVERLAY_SYSTEM_UI_ARTS_LABEL_CLUT_ROW,
                       "v": {"high": ChipDirection::High.label_v(),
                             "left": ChipDirection::Left.label_v(),
                             "right": ChipDirection::Right.label_v(),
                             "low": ChipDirection::Low.label_v(),
                             "arms": tp::OVERLAY_SYSTEM_UI_ARTS_LABEL_V_ARMS,
                             "raseru": tp::OVERLAY_SYSTEM_UI_ARTS_LABEL_V_RASERU}},
        "chip_diamond_l": {"r": rect(r.diamond_l),
                            "pal": tp::OVERLAY_SYSTEM_UI_ARTS_LABEL_CLUT_ROW},
        "chip_diamond_r": {"r": rect(r.diamond_r),
                            "pal": tp::OVERLAY_SYSTEM_UI_ARTS_LABEL_CLUT_ROW},
        "pennant_cap_l": {"r": rect(r.pennant_cap_l),
                           "pal": tp::OVERLAY_SYSTEM_UI_ARTS_LABEL_CLUT_ROW},
        "pennant_cap_r": {"r": rect(r.pennant_cap_r),
                           "pal": tp::OVERLAY_SYSTEM_UI_ARTS_LABEL_CLUT_ROW},
        "bar_end_l": {"r": rect(r.bar_end_l),
                       "pal": tp::OVERLAY_SYSTEM_UI_ARTS_CHIP_CLUT_ROW},
        "bar_body": {"r": rect(r.bar_body),
                      "pal": tp::OVERLAY_SYSTEM_UI_ARTS_CHIP_CLUT_ROW},
        "bar_arrow": {"r": rect(r.bar_arrow),
                       "pal": tp::OVERLAY_SYSTEM_UI_ARTS_CHIP_CLUT_ROW},
        // The Triangle arts-list window is the ordinary system-UI panel
        // 9-slice under a per-window vertical gouraud - the same tiles the
        // pause menu frames its windows with.
        "list_win": {"interior": [128,0,32,32],
                      "edge_top": rect(tp::OVERLAY_SYSTEM_UI_PANEL_TOP),
                      "edge_bottom": rect(tp::OVERLAY_SYSTEM_UI_PANEL_BOT),
                      "edge_l": [160,4,4,24], "edge_r": [188,4,4,24],
                      "corner_tl": rect(tp::OVERLAY_SYSTEM_UI_PANEL_TL),
                      "corner_tr": rect(tp::OVERLAY_SYSTEM_UI_PANEL_TR),
                      "corner_bl": rect(tp::OVERLAY_SYSTEM_UI_PANEL_BL),
                      "corner_br": rect(tp::OVERLAY_SYSTEM_UI_PANEL_BR),
                      "pal": tp::OVERLAY_SYSTEM_UI_PANEL_CLUT_ROW,
                      "grad": [0x40, 0x88]},
        "arts_arrows": {"v": 208, "w": 12, "h": 12, "pal": 15,
                         "u": {"up": 208, "down": 220, "right": 232,
                               "left": 244}},
        "arts_text_pal": 15,
        "tri_button": {"r": [48,0,16,16]},
        "ap_input_fill": {"rect": [ai_fill().0, ai_fill().1, ai_fill().2, ai_fill().3],
                           "rgb": [rgb(tp::OVERLAY_SYSTEM_UI_GAUGE_FILL_GOLD_RGB),
                                   rgb(tp::OVERLAY_SYSTEM_UI_GAUGE_FILL_DARK_RGB)]},
    })
}

/// The AP plate's gouraud-fill span, from the shared module.
pub(super) fn ai_fill() -> (i32, i32, i32, i32) {
    use legaia_engine_ui::arts_input as ai;
    (ai::AP_FILL_X, ai::AP_FILL_Y, ai::AP_FILL_W, ai::AP_FILL_H)
}

pub(super) fn rgb(c: (u8, u8, u8)) -> serde_json::Value {
    serde_json::json!([c.0, c.1, c.2])
}

/// SCUS VA of the screen-element placement table (24-byte stride); the
/// dome's HUD is the first 80 records.
pub(super) const HUD_ELEMENT_TABLE_VA: u32 = 0x8007_6C10;

/// Element records in the table.
pub(super) const HUD_ELEMENT_COUNT: usize = 80;

/// Sub-palette (of the menu-glyph atlas CLUT bank) the battle font and the
/// small digits draw through (capture: SPRT clut word `0x7F8D` = row 510,
/// x 208 = bank sub-palette 13).
pub(super) const HUD_TEXT_SUB_PALETTE: usize = 13;

impl LegaiaMinigames {
    /// Parse a plain TIM out of the raw `PROT.DAT` image at `offset`
    /// (the boot-gap system TIMs are not PROT entries).
    pub(super) fn hud_gap_tim(&self, offset: usize) -> Option<legaia_tim::Tim> {
        legaia_tim::parse(self.prot.get(offset..)?).ok()
    }

    /// The `etim` banner TIM (third TIM of PROT 0870).
    pub(super) fn hud_banner_tim(&self) -> Option<legaia_tim::Tim> {
        let entry = entry_bytes(&self.prot, &self.entries, HUD_ETIM_PROT_INDEX)?;
        legaia_tim::parse(entry.get(HUD_BANNER_TIM_OFFSET..)?).ok()
    }

    /// The two dome-hub page TIMs out of the extraction-1220 LZS container
    /// (section 0 = `[u32 tag][u32 count][u32 size]` + TIM at `0xC` + TIM
    /// immediately after).
    pub(super) fn hud_hub_tims(&self) -> Option<(legaia_tim::Tim, legaia_tim::Tim)> {
        // The shared decoder applies the arena's upload STP, so the CLUT
        // words here are the VRAM ones the variant passes blend against.
        let entry = entry_bytes(&self.prot, &self.entries, HUD_HUB_CONTAINER_PROT_INDEX)?;
        legaia_asset::muscle_dome::hub_page_tims(entry)
    }

    /// Decode a 4bpp TIM through 16-colour palette `pal` to RGBA8
    /// (texel index 0 = transparent, everything else opaque).
    pub(super) fn hud_tim_rgba(tim: &legaia_tim::Tim, pal: &[u16]) -> Vec<u8> {
        let w = tim.pixel_width();
        let h = tim.pixel_height();
        let mut out = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let byte = tim.image.data[y * (w / 2) + x / 2];
                let idx = if x % 2 == 0 { byte & 0xF } else { byte >> 4 } as usize;
                if idx == 0 {
                    continue;
                }
                let c = legaia_tim::bgr555_to_rgba8(*pal.get(idx).unwrap_or(&0));
                let o = (y * w + x) * 4;
                out[o..o + 3].copy_from_slice(&c[..3]);
                out[o + 3] = 255;
            }
        }
        out
    }

    /// The SCUS battle HUD element table rows, or empty without a SCUS.
    pub(super) fn hud_elements_json(&self) -> Vec<serde_json::Value> {
        let Some(scus) = self.scus.as_deref() else {
            return Vec::new();
        };
        if scus.len() < 0x20 || &scus[0..8] != b"PS-X EXE" {
            return Vec::new();
        }
        let t_addr = u32::from_le_bytes(scus[0x18..0x1C].try_into().unwrap());
        let Some(off) = (HUD_ELEMENT_TABLE_VA.checked_sub(t_addr))
            .map(|d| d as usize + 0x800)
            .filter(|o| o + HUD_ELEMENT_COUNT * 24 <= scus.len())
        else {
            return Vec::new();
        };
        let i16_at = |p: usize| i16::from_le_bytes(scus[p..p + 2].try_into().unwrap());
        (0..HUD_ELEMENT_COUNT)
            .map(|i| {
                let r = off + i * 24;
                serde_json::json!({
                    "id": i,
                    "spr": [scus[r], scus[r + 1]],
                    "a": [i16_at(r + 2), i16_at(r + 4)],
                    "w": i16_at(r + 6), "h": i16_at(r + 8),
                    "b": [i16_at(r + 10), i16_at(r + 12)],
                    "style": [scus[r + 0xE], scus[r + 0xF]],
                    "kind": scus[r + 0x10],
                })
            })
            .collect()
    }

    /// Per-character advance widths of the ASCII battle font. The pen
    /// advance retail uses equals the glyph's occupied texel width for every
    /// character measured in the captured chip-label runs (`Begin`, `Carl`,
    /// `Attack`, `Item`, `Spirit`, `Meta`, `Run`, `Ironman`, `Fire Blow`,
    /// `Auto`, `Command`) except three - `i`, `m`, `M` advance one texel
    /// wider - and the space advances 5. A retail width *table* was not
    /// found statically (SCUS + battle overlay byte-scanned), so this is
    /// texel-derived with the capture-measured exceptions baked in.
    pub(super) fn hud_font_advances(&self) -> Vec<u8> {
        let Some(tim) = self.hud_gap_tim(HUD_FONT_TIM_OFFSET) else {
            return Vec::new();
        };
        let w = tim.pixel_width();
        let mut out = Vec::with_capacity(96);
        for ch in 0..96usize {
            let (cx, cy) = ((ch % 16) * 16, (ch / 16) * 16);
            let mut maxc = 0usize;
            for y in 0..16 {
                for x in 0..16 {
                    let (px, py) = (cx + x, cy + y);
                    let byte = tim.image.data[py * (w / 2) + px / 2];
                    let idx = if px % 2 == 0 { byte & 0xF } else { byte >> 4 };
                    if idx != 0 && x + 1 > maxc {
                        maxc = x + 1;
                    }
                }
            }
            let adv = match (ch + 0x20) as u8 {
                b' ' => 5,
                b'i' | b'm' | b'M' => (maxc + 1).min(16),
                _ if maxc == 0 => 5,
                _ => maxc.min(16),
            };
            out.push(adv as u8);
        }
        out
    }
}

#[wasm_bindgen]
impl LegaiaMinigames {
    /// The live contest's **victory banner**, composed the way retail
    /// composes it and resolved to real strings.
    ///
    /// Retail assembles three pieces into the battle context's text buffer
    /// (`ctx + 0x1F9`): the winning fighter's lead-in line out of the
    /// victory-message pointer table at `0x801F4DFC` indexed `char_id - 1`,
    /// the reward spell's name from the shared spell-name table, and a fixed
    /// suffix at `0x801F4C28`. The index half is
    /// [`MuscleDomeSession::reward_banner`](legaia_engine_core::muscle_dome::MuscleDomeSession::reward_banner);
    /// this resolves the two overlay strings off the as-loaded PROT 0898
    /// image and the spell name off `SCUS_942.54`.
    ///
    /// `{"ok":true,"lead_in_index":n,"lead_in":"…","spell_id":n,
    /// "spell":"…","suffix":"…","text":"…"}` - `text` is the three joined in
    /// retail's order. `ok` is false with no live contest.
    pub fn muscle_reward_banner_json(&self) -> String {
        let Some(contest) = self.muscle.as_ref() else {
            return r#"{"ok":false}"#.to_string();
        };
        // Retail's `DAT_8007BD10[slot]` is the 1-based character id.
        let char_id = contest.char_slot as u8 + 1;
        let banner = contest.session.reward_banner(char_id);
        let loaded = overlay_image(
            &self.prot,
            &self.entries,
            md::MUSCLE_OVERLAY_PROT_INDEX as u32,
        );
        let lead_in = loaded
            .as_deref()
            .and_then(|img| {
                overlay_string(
                    img,
                    overlay_u32(
                        img,
                        md::VICTORY_MSG_TABLE_VA + banner.lead_in_index as u32 * 4,
                    )?,
                )
            })
            .unwrap_or_default();
        let suffix = loaded
            .as_deref()
            .and_then(|img| {
                overlay_string(
                    img,
                    legaia_engine_vm::battle_cast_dispatch::BANNER_SUFFIX_VA,
                )
            })
            .unwrap_or_default();
        let spell = self.muscle_spell_name(banner.spell_id as u8);
        serde_json::json!({
            "ok": true,
            "lead_in_index": banner.lead_in_index,
            "lead_in": lead_in,
            "spell_id": banner.spell_id,
            "spell": spell,
            "suffix": if banner.suffix { suffix.clone() } else { String::new() },
            "text": format!("{lead_in}{spell}{}", if banner.suffix { suffix } else { String::new() }),
        })
        .to_string()
    }

    /// The arena's **course ladder**, straight off the disc.
    ///
    /// PROT 0977 carries a 3-entry course descriptor table (`0x801D1A08`,
    /// `{ i32 rounds; ptr first }`) over a run of 29
    /// `{ u32 label_va; u32 monster_id }` round records (`0x801D1920`), and
    /// `FUN_801D1510` stores the round's `monster_id` into formation slot 0
    /// at `0x8007BD0C` - so the arena's opponent is an ordinary battle
    /// monster with an ordinary PROT 867 record.
    ///
    /// Rows: `{ "course": c, "rounds": [{ "round": 1-based, "id": monster
    /// id, "name": archive name, "hp": archive HP, "score": the score cell
    /// clearing it adds }] }`. `name`/`hp` are null when the archive slot
    /// does not decode.
    pub fn muscle_course_ladder_json(&self) -> String {
        use legaia_engine_core::muscle_dome as md;
        let Some(raw) = entry_bytes(&self.prot, &self.entries, 977) else {
            return "[]".to_string();
        };
        let Some(ladder) = md::parse_course_ladder(raw) else {
            return "[]".to_string();
        };
        let archive = self.monster_archive_entry();
        let rows: Vec<serde_json::Value> = ladder
            .iter()
            .enumerate()
            .map(|(c, course)| {
                let rounds: Vec<serde_json::Value> = course
                    .rounds
                    .iter()
                    .enumerate()
                    .map(|(r, round)| {
                        let rec = archive.and_then(|a| {
                            monster_archive::record(a, round.monster_id as u16)
                                .ok()
                                .flatten()
                        });
                        serde_json::json!({
                            "round": r + 1,
                            "id": round.monster_id,
                            "name": rec.as_ref().map(|m| m.name.clone()),
                            "hp": rec.as_ref().map(|m| m.hp),
                            "score": md::course_score_cell(raw, c, r as u32 + 1),
                        })
                    })
                    .collect();
                serde_json::json!({ "course": c, "rounds": rounds })
            })
            .collect();
        serde_json::Value::Array(rows).to_string()
    }

    /// The hub's **first visit** at `tick` ticks in (no pad input), as
    /// retail-placed rows: `{ ok, arm, done, rows }`.
    ///
    /// The arms are `legaia_engine_core::muscle_ringside::FirstVisitHub` -
    /// the intro strip, the brick wall rising under it, the course-title
    /// zoom, the course card (`FUN_801D042C`, course `course`), the wall
    /// draining and the ROUND card (`round` is the displayed number) - and
    /// the frame is composed by
    /// `legaia_engine_ui::ringside_backdrop::first_visit_hub_draw`, the
    /// kernel both play hosts draw it with. `rows` are in paint order and in
    /// [`Self::muscle_hub_quads_json`]'s row shape, plus one
    /// `{ shade: true, x, y, dw, dh, top, bottom }` row for the backdrop
    /// shade between the wall and the screens. `arm` is the arm name
    /// (`intro`, `title`, `card`, `drain`, `round`, `done`).
    pub fn muscle_first_visit_json(&self, tick: i32, course: i32, round: i32) -> String {
        use legaia_engine_core::muscle_ringside::FirstVisitHub;
        // The walk is deterministic and bounded; replay it to `tick`.
        let mut hub = FirstVisitHub::new();
        for _ in 0..tick.clamp(0, 4096) {
            if hub.done() {
                break;
            }
            hub.tick(1, 0);
        }
        self.first_visit_rows_json(&hub, course, round)
    }

    /// Start a **live** first visit: the page steps it once per tick with
    /// [`Self::muscle_first_visit_step`] instead of replaying a no-input walk,
    /// so a press ends the two card holds early and the announcer lines play
    /// - what both play hosts do through `World::tick_muscle_hub`.
    pub fn muscle_first_visit_reset(&mut self) {
        self.muscle_hub = Some(legaia_engine_core::muscle_ringside::FirstVisitHub::new());
    }

    /// Step the live first visit one tick. `pressed` is whether the player
    /// pressed a button this tick (the page has no retail pad word; any press
    /// stands for the hold arms' `0xF4` mask). Starts the CD-XA line the tick
    /// started, through the announcer lane. Returns the frame as
    /// [`Self::muscle_first_visit_json`] does; `{"ok":false}` with no live hub.
    pub fn muscle_first_visit_step(&mut self, pressed: bool, course: i32, round: i32) -> String {
        use legaia_engine_core::muscle_dome::HUB_SKIP_PAD_MASK;
        let Some(mut hub) = self.muscle_hub.take() else {
            return r#"{"ok":false}"#.to_string();
        };
        if !hub.done() {
            hub.tick(1, if pressed { HUB_SKIP_PAD_MASK } else { 0 });
        }
        if let Some(cue) = hub.take_xa() {
            self.muscle_hub_xa_fired = self.muscle_hub_xa_fired.wrapping_add(1);
            self.play_baka_xa(legaia_engine_core::baka_fighter_chrome::XaCue {
                clip: cue.clip,
                chan: cue.channel,
                dur: cue.duration_sectors,
            });
        }
        let out = self.first_visit_rows_json(&hub, course, round);
        self.muscle_hub = Some(hub);
        out
    }

    /// How many announcer lines the live first visit has started.
    pub fn muscle_hub_xa_fired(&self) -> u32 {
        self.muscle_hub_xa_fired
    }
}

impl LegaiaMinigames {
    /// The first-visit frame of `hub` as retail-placed rows (see
    /// [`Self::muscle_first_visit_json`]).
    fn first_visit_rows_json(
        &self,
        hub: &legaia_engine_core::muscle_ringside::FirstVisitHub,
        course: i32,
        round: i32,
    ) -> String {
        use legaia_engine_core::muscle_ringside::FirstVisitArm as A;
        use legaia_engine_ui::other_game_hud as hud;
        use legaia_engine_ui::ringside_backdrop as rb;
        let Some(raw) = entry_bytes(&self.prot, &self.entries, 977) else {
            return r#"{"ok":false}"#.to_string();
        };
        let mut table = hud::parse_sprite_table(raw);
        if table.is_empty() {
            return r#"{"ok":false}"#.to_string();
        }
        let f = hub.frame();
        let levels = rb::FirstVisitLevels {
            backdrop: f.backdrop,
            intro: f.intro,
            title_scale: f.title_scale,
            course_card: f.course_card,
            round_card: f.round_card,
        };
        let d = rb::first_visit_hub_draw(&mut table, &levels, course, round);
        let stp = self
            .hud_hub_tims()
            .map(|(t0, t1)| rb::HubPaletteStp::from_tims(&t0, &t1))
            .unwrap_or_default();
        let quad_row = |q: &hud::HudQuad| {
            serde_json::json!({
                "sheet": if q.tpage & 0x10 != 0 { 5 } else { 4 },
                "pal": q.clut & 0x3F,
                "u": q.uv[0].0, "v": q.uv[0].1,
                "w": q.uv[1].0 as i32 - q.uv[0].0 as i32 + 1,
                "h": q.uv[2].1 as i32 - q.uv[0].1 as i32 + 1,
                "x": q.xy[0].0, "y": q.xy[0].1,
                "dw": q.xy[1].0 as i32 - q.xy[0].0 as i32 + 1,
                "dh": q.xy[2].1 as i32 - q.xy[0].1 as i32 + 1,
                "semi": q.semi_transparent,
                "abr": stp.quad_abr(q),
            })
        };
        let mut rows: Vec<serde_json::Value> = d.tiles.iter().map(quad_row).collect();
        if let Some(sh) = d.shade {
            rows.push(serde_json::json!({
                "shade": true,
                "x": sh.xy[0].0, "y": sh.xy[0].1,
                "dw": sh.xy[1].0 as i32 - sh.xy[0].0 as i32,
                "dh": sh.xy[2].1 as i32 - sh.xy[0].1 as i32,
                "top": sh.rgb[0][0],
                "bottom": sh.rgb[2][0],
            }));
        }
        rows.extend(d.hud.iter().map(quad_row));
        let arm = match hub.arm() {
            A::IntroIn | A::IntroHold | A::IntroOut => "intro",
            A::TitleZoom => "title",
            A::CardIn | A::CardHold => "card",
            A::Drain | A::Return => "drain",
            A::RoundIn | A::RoundOut => "round",
            A::Done => "done",
        };
        serde_json::json!({ "ok": true, "arm": arm, "done": hub.done(), "rows": rows }).to_string()
    }
}

#[wasm_bindgen]
impl LegaiaMinigames {
    /// The re-entered hub's **ringside still** under the INTERVAL screen,
    /// `tick` ticks into that screen: `{ ok, quads: [...] }` with two rows on
    /// sheet `7` (`pal` = the still variant, `0` = extraction 1221 / `int.tim`,
    /// `1` = 1222 / `int2.tim`) and `bright` the packet colour (`0x80` =
    /// neutral modulation). `quads` is empty while the level is zero.
    ///
    /// The level is retail's `*(0x801D1A7C)` across hub arms `0x0A..0x0C`
    /// ([`legaia_engine_core::muscle_ringside::HubBackdrop`]), replayed to
    /// `tick` beside the INTERVAL envelope and the tally roll the same way
    /// the play hosts' `HubTimers` steps them; once the screen has drained it
    /// holds at the level the backdrop climbs back to (this page's own ROUND
    /// banner follows on its key, not on retail's arm `0x15`). The variant is
    /// the loader's pick over the fighter's HP as the leg ended
    /// ([`legaia_engine_core::muscle_ringside::still_prot_index`]: below half
    /// of `hp_max` selects `int2.tim`). The quads are
    /// [`legaia_engine_ui::ringside_backdrop::ringside_still_quads`]
    /// resolved onto the sheet through their texture pages
    /// (`StillDraw::from_quad`).
    pub fn muscle_interval_still_json(&self, tick: i32, hp_cur: u32, hp_max: u32) -> String {
        use legaia_engine_core::muscle_dome as md;
        use legaia_engine_core::muscle_ringside::{BackdropStage, HubBackdrop};
        use legaia_engine_ui::ringside_backdrop as rb;
        let Some(run) = self.muscle_run.as_ref() else {
            return r#"{"ok":false}"#.to_string();
        };
        let still = legaia_engine_core::muscle_ringside::still_prot_index(
            hp_cur.min(0xFFFF) as u16,
            hp_max.min(0xFFFF) as u16,
        );
        let variant = still.saturating_sub(legaia_asset::ringside_still::PROT_INDEX_DEFAULT);
        let roll =
            md::HUB_TALLY_ROLL_LEAD_TICKS + *md::HUB_TALLY_CUE_STAGGER.last().unwrap_or(&0) as i32;
        let mut interval = Some(md::HubScreen::interval(roll));
        let (mut ramp, _) = run.tally_roll();
        let mut backdrop = HubBackdrop::reentry(still);
        let volume_word = legaia_engine_core::new_game::GAME_STATE_COLD_RESET.voice_volume as u32;
        for _ in 0..tick.max(0) {
            if backdrop.stage() == BackdropStage::Card {
                break;
            }
            let lane0_full = ramp.fade[0] >= legaia_engine_core::other_game_overlay::LANE_FADE_FULL;
            backdrop.tick(1, 0, interval.map(|i| i.stage()), lane0_full);
            if let Some(i) = interval.as_mut() {
                i.tick(1, 0);
                ramp.tick(1, false, volume_word);
                if i.done() {
                    interval = None;
                }
            }
        }
        let level = backdrop.level();
        let rows: Vec<serde_json::Value> = if level > 0 {
            rb::ringside_still_quads(level)
                .iter()
                .filter_map(|q| {
                    let d = rb::StillDraw::from_quad(q)?;
                    Some(serde_json::json!({
                        "sheet": HUD_STILL_SHEET,
                        "pal": variant,
                        "u": d.src.0, "v": d.src.1, "w": d.src.2, "h": d.src.3,
                        "x": d.dst.0, "y": d.dst.1, "dw": d.dst.2, "dh": d.dst.3,
                        "bright": d.level,
                    }))
                })
                .collect()
        } else {
            Vec::new()
        };
        serde_json::json!({ "ok": true, "level": level, "variant": variant, "quads": rows })
            .to_string()
    }

    /// One PROT 0977 **hub screen** as retail-placed quads.
    ///
    /// `screen`: 0 = intro card, 1 = course-title art, 2 = INTERVAL
    /// heading, 3 = ROUND banner (`round` is the displayed number),
    /// 4 = the six-row score tally. `brightness` is the emitter's colour
    /// scale (`0x100` = the record's own colour).
    ///
    /// Every row's screen rect comes out of the retail emitters
    /// ([`legaia_engine_ui::other_game_hud::hub_screen_quads`]) fed the draw
    /// list recovered from that entry's own call sites, so the page places
    /// nothing itself: `x`/`y`/`dw`/`dh` are the quad's, `u`/`v`/`w`/`h`
    /// its texels, `sheet` 4/5 the hub page and `pal` the row sub-palette.
    pub fn muscle_hub_quads_json(&self, screen: u32, round: i32, brightness: i32) -> String {
        use legaia_engine_ui::other_game_hud as hud;
        let Some(raw) = entry_bytes(&self.prot, &self.entries, 977) else {
            return r#"{"ok":false}"#.to_string();
        };
        let mut table = hud::parse_sprite_table(raw);
        if table.is_empty() {
            return r#"{"ok":false}"#.to_string();
        }
        let quads = match screen {
            0 => hud::hub_screen_quads(&mut table, hud::HUB_INTRO_CARD, brightness),
            1 => hud::title_art_quads(&mut table, hud::TITLE_ART_ZOOM_END),
            2 => hud::hub_screen_quads(&mut table, hud::HUB_INTERVAL_HEADING, brightness),
            3 => hud::hub_screen_quads(&mut table, &hud::round_banner_draws(round), brightness),
            // The six rows are the roll's, not the settled totals: the three
            // recovery lanes counting down, the HP they count into, the score
            // lane counting down and the coin tally counting up. On this arm
            // `round` is the screen's own tick, and the roll is replayed from
            // it the way the envelope is - the kernel is
            // `other_game_overlay::ScoreTallyRamp`, which the native window
            // steps one frame at a time off the same armed state.
            _ => {
                let Some(run) = self.muscle_run.as_ref() else {
                    return r#"{"ok":false}"#.to_string();
                };
                let volume_word =
                    legaia_engine_core::new_game::GAME_STATE_COLD_RESET.voice_volume as u32;
                let (mut ramp, mut tally) = run.tally_roll();
                for _ in 0..round.max(0) {
                    let step = ramp.tick(1, false, volume_word);
                    tally += step.tally_gain;
                    if !step.rolling {
                        break;
                    }
                }
                hud::score_tally_quads(
                    &mut table,
                    ramp.row_values(tally),
                    ramp.row_brightness(brightness),
                )
            }
        };
        let stp = self
            .hud_hub_tims()
            .map(|(t0, t1)| legaia_engine_ui::ringside_backdrop::HubPaletteStp::from_tims(&t0, &t1))
            .unwrap_or_default();
        let rows: Vec<serde_json::Value> = quads
            .iter()
            .map(|q| {
                serde_json::json!({
                    // tpage bit 4 is the VRAM Y base; the emitter's page
                    // byte lands in the ABR field, so it never disturbs it.
                    "sheet": if q.tpage & 0x10 != 0 { 5 } else { 4 },
                    "pal": q.clut & 0x3F,
                    "u": q.uv[0].0, "v": q.uv[0].1,
                    "w": q.uv[1].0 as i32 - q.uv[0].0 as i32 + 1,
                    "h": q.uv[2].1 as i32 - q.uv[0].1 as i32 + 1,
                    "x": q.xy[0].0, "y": q.xy[0].1,
                    "dw": q.xy[1].0 as i32 - q.xy[0].0 as i32 + 1,
                    "dh": q.xy[2].1 as i32 - q.xy[0].1 as i32 + 1,
                    "semi": q.semi_transparent,
                    "abr": stp.quad_abr(q),
                    // The Gouraud colour, top then bottom (`texel * c / 128`)
                    // - the brightness the screen fades by.
                    "rgb": [q.rgb[0], q.rgb[2]],
                })
            })
            .collect();
        serde_json::json!({ "ok": true, "quads": rows }).to_string()
    }

    /// One HUD sprite sheet decoded to RGBA8 (row-major, texel index 0
    /// transparent). `source`: 0 = battle-chrome widget page (own CLUT-bank
    /// sub-palette `palette`), 1 = ASCII battle font (through the
    /// menu-glyph atlas bank sub-palette `palette`), 2 = menu-glyph atlas,
    /// 3 = `etim` banner page (CLUT-row sub-palette), 4/5 = dome hub pages
    /// (320,0)/(320,256), 6 = the pad-button-glyph TIM (own CLUT), 7 = the
    /// ringside still (`palette` = the variant, [`HUD_STILL_SHEET`]). Empty
    /// when the source doesn't decode on this image. Sheet dimensions ride
    /// in [`Self::muscle_hud_json`].
    pub fn muscle_hud_sheet_rgba(&self, source: u32, palette: u32) -> Vec<u8> {
        if source == HUD_STILL_SHEET {
            return entry_bytes(
                &self.prot,
                &self.entries,
                legaia_asset::ringside_still::PROT_INDEX_DEFAULT + palette.min(1),
            )
            .and_then(legaia_engine_ui::ringside_backdrop::still_sheet_rgba)
            .unwrap_or_default();
        }
        let pal_idx = palette as usize;
        let (tim, pal_tim) = match source {
            0 => {
                let t = self.hud_gap_tim(HUD_WIDGET_TIM_OFFSET);
                (t.clone(), t)
            }
            1 => (
                self.hud_gap_tim(HUD_FONT_TIM_OFFSET),
                self.hud_gap_tim(HUD_MENU_ATLAS_TIM_OFFSET),
            ),
            2 => {
                let t = self.hud_gap_tim(HUD_MENU_ATLAS_TIM_OFFSET);
                (t.clone(), t)
            }
            3 => {
                let t = self.hud_banner_tim();
                (t.clone(), t)
            }
            4 => {
                let t = self.hud_hub_tims().map(|(a, _)| a);
                (t.clone(), t)
            }
            5 => {
                let t = self.hud_hub_tims().map(|(_, b)| b);
                (t.clone(), t)
            }
            6 => {
                let t = self.hud_gap_tim(HUD_BUTTON_TIM_OFFSET);
                (t.clone(), t)
            }
            _ => (None, None),
        };
        let (Some(tim), Some(pal_tim)) = (tim, pal_tim) else {
            return Vec::new();
        };
        let Some(pal) = pal_tim
            .clut
            .as_ref()
            .and_then(|c| c.entries.get(pal_idx * 16..pal_idx * 16 + 16))
        else {
            return Vec::new();
        };
        Self::hud_tim_rgba(&tim, pal)
    }

    /// The retail-HUD description the page renders from: sheet dimensions,
    /// the capture-pinned piece rects (chips, plates, D-pad, red X, AP
    /// plate, HP/MP badges, arts-banner strips, damage numerals), the
    /// SCUS element-table rows (screen anchors + glide endpoints), the
    /// PROT 0977 hub sprite records, and the font advance table. `ok` is
    /// false when the chrome TIMs don't decode on this image.
    pub fn muscle_hud_json(&self) -> String {
        let widget = self.hud_gap_tim(HUD_WIDGET_TIM_OFFSET);
        let font = self.hud_gap_tim(HUD_FONT_TIM_OFFSET);
        let atlas = self.hud_gap_tim(HUD_MENU_ATLAS_TIM_OFFSET);
        let banner = self.hud_banner_tim();
        let button = self.hud_gap_tim(HUD_BUTTON_TIM_OFFSET);
        if widget.is_none() || font.is_none() || atlas.is_none() {
            return r#"{"ok":false}"#.to_string();
        }
        let dims = |t: &Option<legaia_tim::Tim>| {
            t.as_ref()
                .map(|t| serde_json::json!([t.pixel_width(), t.pixel_height()]))
                .unwrap_or(serde_json::Value::Null)
        };
        let hub = self.hud_hub_tims();
        let hub_sprites: Vec<serde_json::Value> = entry_bytes(&self.prot, &self.entries, 977)
            .map(legaia_engine_ui::other_game_hud::parse_sprite_table)
            .unwrap_or_default()
            .iter()
            .enumerate()
            .map(|(i, s)| {
                serde_json::json!({
                    "i": i,
                    // tpage 0x0005 -> hub page 0 (VRAM (320,0)),
                    // 0x0015 -> hub page 1 ((320,256)).
                    "sheet": if s.tpage & 0x10 != 0 { 5 } else { 4 },
                    // CLUT word: bits 0..5 = x/16 (the row sub-palette).
                    "pal": s.clut & 0x3F,
                    "uv": [s.u0, s.v0], "wh": [s.w, s.h],
                    "semi": s.semi_transparent,
                })
            })
            .collect();
        serde_json::json!({
            "ok": true,
            "sheets": {
                "widget": dims(&widget), "font": dims(&font),
                "atlas": dims(&atlas), "banner": dims(&banner),
                "hub0": dims(&hub.as_ref().map(|(a, _)| a.clone())),
                "hub1": dims(&hub.as_ref().map(|(_, b)| b.clone())),
                "button": dims(&button),
                "still": [
                    legaia_asset::ringside_still::WIDTH,
                    legaia_asset::ringside_still::HEIGHT,
                ],
            },
            // Capture-pinned piece rects: [u, v, w, h] on the named sheet,
            // "pal" = the sub-palette observed in the live packets.
            "pieces": {
                "plate_blue": {"cap_l": [208,0,8,20], "body": [192,0,16,20],
                                "cap_r": [216,0,8,20], "pal": 4},
                "plate_gold": {"cap_l": [208,64,8,20], "body": [192,64,16,20],
                                "cap_r": [216,64,8,20], "pal": 12},
                "dpad": {"r": [0,112,16,16], "pal": 7},
                "slash": {"r": [96,64,8,16], "pal": 5},
                "hp_badge": {"r": [208,86,16,10], "pal": 1},
                "mp_badge": {"r": [224,86,16,10], "pal": 1},
                "ap_label": {"r": [128,64,24,16], "pal": 4},
                "ap_trough": {"r": [128,80,56,16], "pal": 4},
                "ap_end": {"r": [176,64,16,16], "pal": 4},
                "ap_cap": {"r": [184,80,8,16], "pal": 4},
                // The baked "100" numeral tile that fills the plate's end
                // box at a full gauge (`OVERLAY_SYSTEM_UI_GAUGE_100`) - the
                // 6px digit strip has no 3-digit seat, so the sheet carries
                // this one. It is NOT the meter fill: the sheet has no fill
                // tile at all, because retail draws the fill as an
                // untextured gouraud pair (`ap_input_fill` below).
                "gauge_100": {"r": [64,136,16,6], "pal": 1},
                "red_x": {"r": [0,96,64,16], "pal": 4},
                "digit24_v": 64,
                "word_super": {"r": [3,152,105,24], "pal": 3},
                "word_hyper": {"r": [3,176,105,24], "pal": 3},
                "word_arts": {"r": [115,176,97,24], "pal": 3},
                "word_miracle": {"r": [0,200,127,24], "pal": 3},
                "word_new": {"r": [132,200,64,24], "pal": 3},
                "word_damage": {"r": [0,224,52,14], "pal": 3},
                "word_hit": {"r": [0,240,32,16], "pal": 3},
                "word_total": {"r": [32,240,48,16], "pal": 3},
                "atlas_digits": {"v": 208, "x0": 0, "cell": 8, "h": 12, "pal": HUD_TEXT_SUB_PALETTE},
                "font_pal": HUD_TEXT_SUB_PALETTE,
            },
            "arts_input": arts_input_pieces(),
            "elements": self.hud_elements_json(),
            "hub": hub_sprites,
            "advance": self.hud_font_advances(),
        })
        .to_string()
    }
}
