//! The Load / Save sub-screen's builder. The other sub-screens are the shared
//! projection (`legaia_engine_screens::pause_screens`).

use super::*;

impl LegaiaRuntime {
    /// Load / Save sub-screen: the real retail save-select chrome, driven off
    /// the memory-card rack - the native window's `save_select_overlay`
    /// twin, with the pill row bound to the rack's card ports and the preview
    /// grid to the selected card's blocks.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_save_select(
        &self,
        assets: &PlayMenuAssets,
        s: &SaveSelectSession,
        menu: &PlayMenu,
        sprites: &mut Vec<SpriteDraw>,
        texts: &mut Vec<TextDraw>,
        origin: (i32, i32),
        scale: u32,
    ) -> Option<(usize, u8)> {
        // The screen is the shared composition over the engine's overlay
        // sequence (`SaveScreenFlow::overlay_model` ->
        // `save_select_overlay_draws`), the native window's calls too. Its
        // text half draws with or without the chrome atlas: this page used
        // to return before every phase overlay when the atlas was absent,
        // leaving the title and nothing else.
        let m = menu.save_flow.overlay_model(s)?;
        let rows: Vec<ui::SaveSelectRow<'_>> = s
            .slots()
            .iter()
            .map(|slot| ui::SaveSelectRow {
                label: &slot.label,
                present: slot.present,
                party_lv: slot.party_lv,
                play_time_seconds: slot.play_time_seconds,
                money: slot.money,
                location: &slot.location,
            })
            .collect();
        let cells: Vec<SlotGridCell> = m
            .preview
            .iter()
            .flat_map(|p| p.cells.iter())
            .map(|c| SlotGridCell {
                present: c.present,
                portrait_char_id: c.portrait_char_id,
            })
            .collect();
        let preview = m.preview.as_ref().map(|p| ui::SaveSelectPreviewView {
            cells: &cells,
            cell: p.cell,
            info: p.info.map(|b| SlotInfoView {
                slot_no: b.slot.saturating_add(1),
                location: &b.location,
                play_time: &p.play_time,
                leader_name: &b.leader_name,
                leader_level: b.party_lv,
                leader_hp: b.leader_hp,
                leader_mp: b.leader_mp,
                leader_char_id: b.leader_char_id,
            }),
            caption: p.caption,
            panel_y_offset: p.panel_y_offset,
        });
        let view = ui::SaveSelectOverlayView {
            title: m.title,
            rows: &rows,
            cursor: m.cursor,
            single_pill: m.single_pill,
            pills: &m.pills,
            pill_cursor: m.pill_cursor,
            slide_t: m.slide_t,
            info_t: m.info_t,
            now_checking: m.now_checking,
            banner: m.banner.map(|b| ui::CardBannerView {
                lines: b.lines,
                note: b.note,
                work: b.work,
                slide_t: b.slide_t,
                progress_t: b.progress_t,
            }),
            preview,
            confirm: m.confirm,
        };
        let out = ui::save_select_overlay_draws(
            &assets.font,
            assets.chrome.as_ref().map(|(_, rects)| rects),
            &view,
            origin,
            scale,
        );
        let darken = out.darken.map(|d| (sprites.len() + d.sprite, d.level));
        sprites.extend(out.sprites);
        texts.extend(out.texts);
        darken
    }
}
