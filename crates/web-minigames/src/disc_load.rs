//! `LegaiaMinigames::load_disc`: read every minigame's overlay tables, art
//! packs and sound banks out of the visitor's disc image.

use super::*;

#[wasm_bindgen]
impl LegaiaMinigames {
    /// Load a full Mode2/2352 disc image (or a raw `PROT.DAT`), parse the TOC,
    /// and pre-decode every minigame table that resolves. Returns a JSON status
    /// object naming which games came up:
    ///
    /// ```json
    /// { "entries": 1290,
    ///   "dance":  { "ok": true, "rows": 3, "beats": 32 },
    ///   "baka":   { "ok": true, "fighters": 17 },
    ///   "slot":   { "ok": true, "payouts": [.., ..] } }
    /// ```
    ///
    /// A game whose overlay or table doesn't resolve reports `{"ok":false}` with
    /// a reason rather than throwing - a regional / modded disc can still play
    /// the others.
    pub fn load_disc(&mut self, bytes: Vec<u8>) -> Result<String, JsValue> {
        // The Baka announcer lines are CD-XA, outside PROT.DAT: decode them
        // now, before the raw image is dropped.
        self.baka_xa = if legaia_iso::image::is_mode2_2352_disc(&bytes) {
            baka_presentation::stage_baka_announcer(&bytes)
        } else {
            legaia_engine_audio::XaClipBank::new()
        };
        let (prot, scus) = if legaia_iso::image::is_mode2_2352_disc(&bytes) {
            // Keep the executable too: the Muscle Dome reads the new-game
            // party template / growth curves / spell names out of it, and
            // the fishing point-exchange names its prize rows from the SCUS
            // item table. A raw PROT.DAT load plays with ids / fallbacks.
            let scus = legaia_iso::image::extract_scus(&bytes);
            let prot = legaia_iso::image::extract_prot_dat(&bytes).ok_or_else(|| {
                JsValue::from_str("minigames: PROT.DAT not found in this disc image")
            })?;
            (prot, scus)
        } else {
            (bytes, None)
        };
        self.item_names = scus
            .as_ref()
            .and_then(|scus| legaia_game_tables::item_names::ItemNameTable::from_scus(scus));
        self.scus = scus;
        let entries = legaia_prot::toc_index::parse_prot_toc(&prot)
            .ok_or_else(|| JsValue::from_str("minigames: PROT.DAT TOC parse failed"))?;
        #[cfg(target_arch = "wasm32")]
        console_log(&format!(
            "Minigames: PROT.DAT loaded ({} entries)",
            entries.len()
        ));
        self.prot = prot;
        self.entries = entries;
        self.dance = None;
        self.baka = None;
        self.baka_winnings = 0;
        self.baka_surface = Default::default();
        self.slot = None;
        self.fishing_species = None;
        self.fishing_overlay = None;
        self.fishing_pond = None;
        self.fishing_spawn = None;
        self.fishing_cadence = None;
        self.fishing_exchange = None;
        self.fishing_banners = Default::default();
        self.fishing_sprites = None;
        self.fishing_hud_last.clear();
        self.fishing_captions = None;
        self.fishing_prizes = Default::default();
        self.fishing_scene = None;
        self.muscle = None;
        self.muscle_tables = None;

        // --- dance step chart (PROT 0980) + presentation (PROT 1230 art,
        //     the overlay's widget table, PROT 1228/1231 SFX) ---
        self.dance_pres = self.load_dance_presentation();
        // The dance cast: Noa's field mesh (global pool slot 1) + the
        // dance-hall scene module's dedicated dancer NPCs, with the scene's
        // choreography ANM bundle (PROT 1229) - decoded here so the page can
        // render the floor with the retail dancers actually dancing.
        self.dance_bodies = self.load_dance_bodies();
        let dance_json = match self.dance_chart() {
            Some(c) => format!(
                r#"{{"ok":true,"rows":{},"beats":{},"art":{},"body":{},"sfx":{}}}"#,
                c.rows.len(),
                legaia_minigame_formats::dance_chart::BEATS_PER_ROW,
                self.dance_pres.is_some(),
                self.dance_bodies.is_some(),
                self.dance_pres
                    .as_ref()
                    .and_then(|p| p.sfx.as_ref())
                    .map(|b| b.cues().len())
                    .unwrap_or(0),
            ),
            None => format!(
                r#"{{"ok":false,"why":{}}}"#,
                jstr("dance overlay (PROT 0980) or its step chart did not decode")
            ),
        };

        // --- baka roster + action tables (PROT 0976) ---
        self.baka_tables = overlay_image(
            &self.prot,
            &self.entries,
            legaia_minigame_formats::baka_opponents::BAKA_OVERLAY_PROT_INDEX as u32,
        )
        .and_then(|img| {
            let opponents = legaia_minigame_formats::baka_opponents::parse(&img)?;
            let actions = legaia_minigame_formats::baka_opponents::parse_actions(&img)?;
            Some((opponents, actions))
        });
        // The roster carries each fighter's name in the 32 bytes ahead of the
        // stat block the table parser starts at.
        self.baka_names = overlay_image(
            &self.prot,
            &self.entries,
            legaia_minigame_formats::baka_opponents::BAKA_OVERLAY_PROT_INDEX as u32,
        )
        .and_then(|img| minigame_art::baka_roster_names(&img).ok());
        let baka_json = match &self.baka_tables {
            Some((o, _)) => format!(
                r#"{{"ok":true,"fighters":{},"named":{}}}"#,
                o.len(),
                self.baka_names.is_some()
            ),
            None => format!(
                r#"{{"ok":false,"why":{}}}"#,
                jstr("Baka Fighter overlay (PROT 0976) or its roster tables did not decode")
            ),
        };

        // --- slot payout table (PROT 0975) ---
        self.slot_payouts = overlay_image(
            &self.prot,
            &self.entries,
            legaia_minigame_formats::slot_payout::SLOT_OVERLAY_PROT_INDEX as u32,
        )
        .and_then(|img| legaia_minigame_formats::slot_payout::parse(&img));

        // --- slot art pack (PROT 1200) + HUD descriptors (raw PROT 0975) ---
        // The overlay init `FUN_801CEC94` loads the art from raw TOC 0x4B2; the
        // HUD widget table is rodata inside the overlay entry itself.
        self.slot_art = entry_bytes(
            &self.prot,
            &self.entries,
            minigame_art::SLOT_ART_PROT_INDEX as u32,
        )
        .and_then(|raw| minigame_art::parse_art_pack(raw).ok());
        self.slot_hud = entry_bytes(
            &self.prot,
            &self.entries,
            legaia_minigame_formats::slot_payout::SLOT_OVERLAY_PROT_INDEX as u32,
        )
        .and_then(|raw| minigame_art::parse_slot_hud(raw).ok());

        // --- slot 3D scene graph (PROT 0975 rodata + art page 3) ---
        // The machine is a 3D scene, not a sprite collage: its paylines,
        // medallions, lamps, pedestals and marquee are GTE-projected quads whose
        // model-space positions are four contiguous tables in the overlay's own
        // rodata. The dot-matrix marquee's message bank is cut out of art page 3.
        self.slot_scene = (|| {
            let overlay = entry_bytes(
                &self.prot,
                &self.entries,
                legaia_minigame_formats::slot_payout::SLOT_OVERLAY_PROT_INDEX as u32,
            )?;
            let art = self.slot_art.as_ref()?;
            let (idx, w, _h) = minigame_art::slot_page_indices(art, slot_scene::DOT_PAGE).ok()?;
            slot_scene::parse_scene(overlay, &idx, w).ok()
        })();

        // --- the machine's resident set for the shared screen-prim builder ---
        self.slot_cabinet = legaia_engine_ui::slot_cabinet::SlotCabinetAssets::load(|i| {
            entry_bytes(&self.prot, &self.entries, i as u32).map(<[u8]>::to_vec)
        })
        .ok()
        .map(|a| {
            // The resident system-UI sheet the submenu box's border samples.
            match self
                .prot
                .get(legaia_ui_formats::title_pak::OVERLAY_SYSTEM_UI_TIM_OFFSET..)
            {
                Some(head) => a.with_system_ui(head),
                None => a,
            }
        });

        // The dialog font the machine's rules pages print in.
        let off = legaia_font::FONT_TIM_PROT_DAT_OFFSET as usize;
        self.slot_font = self
            .prot
            .get(
                off..off
                    .saturating_add(legaia_font::FONT_TIM_LEN)
                    .min(self.prot.len()),
            )
            .zip(self.scus.as_deref())
            .and_then(|(tim, scus)| {
                let font = legaia_font::Font::from_disc_tim_and_scus(tim, scus).ok()?;
                let head = legaia_font::escape_icons::ICON_PROT_DAT_OFFSET as usize;
                Some(match self.prot.get(head..) {
                    Some(h) => font.with_escape_icons_from_disc(h, scus),
                    None => font,
                })
            });

        // --- slot SFX cue bank (descriptors PROT 1199 + samples PROT 1198) ---
        // The reel-stop click, payout tick and reach sting are all runtime-bank
        // cues (id >= 0x200), so they need the overlay's own efect.dat block.
        self.slot_sfx = match (
            entry_bytes(
                &self.prot,
                &self.entries,
                minigame_sfx::SLOT_SFX_BANK_PROT_INDEX as u32,
            ),
            entry_bytes(
                &self.prot,
                &self.entries,
                minigame_sfx::SLOT_SFX_VAB_PROT_INDEX as u32,
            ),
        ) {
            (Some(bank), Some(vab)) => SfxCueBank::new(bank, vab).ok(),
            _ => None,
        };

        let slot_json = match &self.slot_payouts {
            Some(t) => format!(
                r#"{{"ok":true,"payouts":[{}],"art":{},"sfx":{}}}"#,
                t.payouts
                    .iter()
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
                    .join(","),
                self.slot_art.is_some() && self.slot_hud.is_some(),
                self.slot_sfx.as_ref().map(|b| b.cues().len()).unwrap_or(0),
            ),
            None => format!(
                r#"{{"ok":false,"why":{}}}"#,
                jstr("slot-machine overlay (PROT 0975) or its payout table did not decode")
            ),
        };

        // --- fishing species table (PROT 0972) + muscle-dome hand (PROT 0898) ---
        // Decoded into the cached fields the child modules read; each returns a
        // status object so a disc that can't feed one still plays the others.
        let fishing_json = self.load_fishing_tables();
        // The fishing venue's 3D scene (`other1`) + the angler body - the
        // presentation layer behind the pond session.
        self.fishing_scene = self.load_fishing_scene();
        let muscle_json = self.load_muscle_tables();

        Ok(format!(
            r#"{{"entries":{},"dance":{dance_json},"baka":{baka_json},"slot":{slot_json},"fishing":{fishing_json},"muscle":{muscle_json}}}"#,
            self.entries.len()
        ))
    }
}
