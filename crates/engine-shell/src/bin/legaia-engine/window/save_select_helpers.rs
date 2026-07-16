//! Extracted from `window.rs` (mechanical split; behavior-preserving).
//!
//! Save-select / load-screen helpers: the save-directory scanner, the
//! per-slot info-view builder, the info-panel slide-in offset, and the
//! owned-string flavour of the renderer's `SlotInfoView`.

use super::*;

/// Walk `save_dir` and build per-slot `SlotSnapshot` entries from any
/// LGSF v1 / v2 files found there. Empty slots produce
/// `SlotSnapshot::empty(slot)`. Up to 8 slots are scanned (the retail
/// PSX memory card supports 15 blocks; engines wishing to scan more can
/// drive their own scanner and feed the result into `SaveSelectSession`).
/// Pluck the lead-character roster index out of a [`SlotSnapshot`] for
/// the load-screen portrait grid. The snapshot already exposes the
/// leader's char_id (scan_save_dir picks it from the parsed
/// [`legaia_save::SaveFile`]); this thin helper exists so render-time
/// call sites read clearly.
pub(crate) fn slot_leader_char_id(snap: &legaia_engine_core::save_select::SlotSnapshot) -> u8 {
    snap.leader_char_id
}

/// Build a per-frame [`legaia_engine_render::SlotInfoView`] for the
/// info panel shown at the bottom of the slot-preview screen.
/// Returns `None` for empty slots (the info panel renders only when
/// a save is present).
pub(crate) fn build_slot_info_view(
    slots: &[legaia_engine_core::save_select::SlotSnapshot],
    cursor_slot: u8,
) -> Option<SlotInfoOwned> {
    let snap = slots.get(cursor_slot as usize)?;
    if !snap.present {
        return None;
    }
    Some(SlotInfoOwned {
        slot_no: snap.slot.saturating_add(1),
        location: snap.location.clone(),
        play_time: snap.play_time_string(),
        leader_name: snap.leader_name.clone(),
        leader_level: snap.party_lv,
        leader_hp: snap.leader_hp,
        leader_mp: snap.leader_mp,
        leader_char_id: snap.leader_char_id,
    })
}

/// Compute the slide-in y-offset (delta from parked y) for the
/// bottom info panel. Mirrors retail FUN_801E08D8's inline
/// `local_34 = (anim_t * -0x100) / 0xFFF >> 12 + 0x18A`: the panel
/// slides from `INFO_PANEL_OFFSCREEN_Y = 394` (off-screen below) up
/// to `INFO_PANEL_PARKED_Y = 138` (parked under load chrome) as
/// `info_panel_slide_anim_t` ramps 0 → 4096. Returns the delta from
/// parked y, so 0 = fully landed.
pub(crate) fn info_panel_slide_offset(
    session: &legaia_engine_core::save_select::SaveSelectSession,
) -> i32 {
    let (_, y) = legaia_engine_core::save_select::interpolate_anim(
        (0, legaia_engine_core::save_select::INFO_PANEL_OFFSCREEN_Y),
        (0, legaia_engine_core::save_select::INFO_PANEL_PARKED_Y),
        session.info_panel_slide_anim_t(),
    );
    y - legaia_engine_core::save_select::INFO_PANEL_PARKED_Y
}

/// Owned-string flavour of [`legaia_engine_render::SlotInfoView`] used
/// to keep the strings alive across the render call. The borrowed
/// view referenced by the renderer is taken via [`Self::as_view`].
pub(crate) struct SlotInfoOwned {
    slot_no: u8,
    location: String,
    play_time: String,
    leader_name: String,
    leader_level: u8,
    leader_hp: (u16, u16),
    leader_mp: (u16, u16),
    leader_char_id: u8,
}

impl SlotInfoOwned {
    pub(crate) fn as_view(&self) -> legaia_engine_render::SlotInfoView<'_> {
        legaia_engine_render::SlotInfoView {
            slot_no: self.slot_no,
            location: &self.location,
            play_time: &self.play_time,
            leader_name: &self.leader_name,
            leader_level: self.leader_level,
            leader_hp: self.leader_hp,
            leader_mp: self.leader_mp,
            leader_char_id: self.leader_char_id,
        }
    }
}

pub(crate) fn scan_save_dir(save_dir: &Path) -> Vec<legaia_engine_core::save_select::SlotSnapshot> {
    use legaia_engine_core::menu_runtime::SAVE_EXT;
    use legaia_engine_core::save_select::{SlotContent, SlotSnapshot};
    // Scan up to 15 slots (one per retail PSX memory-card block) so
    // the load-screen 5×3 grid can render every potential slot.
    const MAX_SLOTS: u8 = 15;
    let mut out = Vec::with_capacity(MAX_SLOTS as usize);
    for slot in 0..MAX_SLOTS {
        // Saves are written by the field menu via `MenuRuntime` as
        // `<dir>/slot_NN.<SAVE_EXT>` (zero-padded slot, see
        // `menu_runtime::slot_path`). The title-screen and
        // save-select scanners must use the same shape; an earlier
        // mismatch (`slot_N.lgsf`) made every save invisible at boot,
        // greying out Continue even with valid saves on disk.
        let path = save_dir.join(format!("slot_{slot:02}.{SAVE_EXT}"));
        let snap = match std::fs::read(&path).ok().and_then(|b| {
            legaia_save::SaveFile::parse(&b)
                .ok()
                .map(|sf| (b.len(), sf))
        }) {
            Some((_, sf)) => {
                // Prefer the record's retail displayed-level byte (+0x130);
                // fall back to inferring from the cumulative XP word (+0x0)
                // against the retail base curve.
                let leader = sf.party.members.first();
                let lv = leader
                    .map(|r| match r.magic_rank() {
                        l @ 1..=99 => l,
                        _ => legaia_save::level_for_cumulative_xp(r.cumulative_xp()),
                    })
                    .unwrap_or(1);
                let leader_hp = leader
                    .map(|r| {
                        let v = r.hp_mp_sp();
                        (v.hp_cur, v.hp_max)
                    })
                    .unwrap_or((0, 0));
                let leader_mp = leader
                    .map(|r| {
                        let v = r.hp_mp_sp();
                        (v.mp_cur, v.mp_max)
                    })
                    .unwrap_or((0, 0));
                // Retail saves serialise the scene name into the SC
                // block (`+0x200..0x208`, ASCII null-padded). Our LGSF
                // saves don't carry that field yet, so default to the
                // most-common starting kingdom; engines that capture
                // it can override.
                let _ = sf.ext_v2.active_party.is_empty(); // kept-for-future-use
                let location = "Drake Kingdom".to_string();
                SlotSnapshot {
                    slot,
                    present: true,
                    content: SlotContent::LegaiaSave,
                    label: format!("Slot {slot}"),
                    play_time_seconds: sf.ext_v2.play_time_seconds,
                    party_lv: lv,
                    location,
                    money: sf.ext.money.max(0) as u32,
                    // Lead char is always Vahn (char_id=0) in retail
                    // Legaia - Vahn is the protagonist and slot 0 of
                    // the SC character record array.
                    leader_char_id: 0,
                    leader_name: "Vahn".to_string(),
                    leader_hp,
                    leader_mp,
                }
            }
            None => SlotSnapshot::empty(slot),
        };
        out.push(snap);
    }
    out
}
