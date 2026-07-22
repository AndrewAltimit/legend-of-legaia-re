//! Custom-overlay vertical slices + the in-shop Trade vendor (seru_overlay injections).

use super::*;

/// Outcome of injecting the custom-overlay vertical slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlaySliceReport {
    /// PROT entry (pochi slot) the custom overlay was written into.
    pub pochi_index: usize,
    /// Absolute disc LBA baked into the loader stub.
    pub lba: u32,
    /// Sectors the stub loads.
    pub sectors: u16,
}

/// Find a pochi-filler PROT slot whose on-disc footprint can hold `need_bytes`
/// (the "pochi" magic head marks reserved dev filler - safe to overwrite). Picks
/// the largest such slot (most headroom, deterministic by max-footprint then
/// lowest index). `None` if none qualifies.
fn find_pochi_host(patcher: &DiscPatcher, need_bytes: usize) -> Option<usize> {
    let mut best: Option<(u64, usize)> = None;
    for idx in 0..patcher.entry_count() {
        let Some(fp) = patcher.entry_footprint(idx) else {
            continue;
        };
        if (fp as usize) < need_bytes {
            continue;
        }
        let Ok(head) = patcher.read_entry(idx) else {
            continue;
        };
        if head.len() >= 5 && &head[0..5] == b"pochi" {
            let key = (fp, idx);
            if best.is_none_or(|(bf, bi)| fp > bf || (fp == bf && idx < bi)) {
                best = Some(key);
            }
        }
    }
    best.map(|(_, idx)| idx)
}

/// Inject the **custom-overlay vertical slice** (see [`crate::seru_overlay`]):
/// proves the retail custom-overlay load path end to end, triggered by **opening
/// a shop**. Overwrites a pochi slot with a tiny sentinel-writing overlay, bakes
/// a gap loader stub with that slot's real disc LBA, and detours the field-VM
/// op-0x49 arm edge (overlay 0897) into the stub. The stub gates on the sub-op
/// (only a merchant, sub-op `0`), FlushCaches, runs the overlay (which writes
/// [`crate::seru_overlay::SENTINEL`] to [`crate::seru_overlay::SENTINEL_ADDR`]),
/// then resumes the field VM - so the load fires when the player opens a vendor.
/// No Sony bytes.
pub fn inject_overlay_slice(patcher: &mut DiscPatcher) -> Result<OverlaySliceReport> {
    inject_overlay_slice_opts(patcher, true)
}

/// As [`inject_overlay_slice`], but `gated` selects whether the op-`0x49` stub's
/// sub-op gate is live (see [`crate::seru_overlay::assemble_shop_loader_stub_gated`]).
/// `gated = false` is the diagnostic build that fires on every op-`0x49` arm.
pub fn inject_overlay_slice_opts(
    patcher: &mut DiscPatcher,
    gated: bool,
) -> Result<OverlaySliceReport> {
    use crate::seru_overlay as ov;

    let overlay = ov::words_to_bytes(&ov::assemble_sentinel_overlay());
    let sectors = ov::sectors_for(overlay.len());

    // 1. Pick + overwrite a pochi host slot with the overlay.
    let pochi_index = find_pochi_host(patcher, overlay.len())
        .ok_or_else(|| anyhow::anyhow!("no pochi-filler slot large enough for the overlay"))?;
    let lba = patcher
        .entry_disc_lba(pochi_index)
        .ok_or_else(|| anyhow::anyhow!("pochi slot {pochi_index} has no disc LBA"))?;
    patcher
        .patch_prot_entry(pochi_index, 0, &overlay)
        .with_context(|| format!("write overlay into pochi slot {pochi_index}"))?;

    // 2. Bake the shop-gated loader stub into the preserved SCUS rodata gap.
    let scus = patcher
        .read_named_file(SCUS_NAME)
        .context("read SCUS_942.54 for overlay-slice stub")?;
    let stub = ov::words_to_bytes(&ov::assemble_shop_loader_stub_gated(lba, sectors, gated));
    let stub_off = legaia_asset::item_names::file_offset_for_va(&scus, ov::STUB_VA)
        .ok_or_else(|| anyhow::anyhow!("can't resolve stub VA {:#x} in SCUS", ov::STUB_VA))?;
    if scus
        .get(stub_off..stub_off + stub.len())
        .is_none_or(|r| r.iter().any(|&b| b != 0))
    {
        anyhow::bail!("stub region {:#x} is not all-zero dead space", ov::STUB_VA);
    }
    patcher
        .patch_named_file(SCUS_NAME, stub_off as u64, &stub)
        .context("write overlay-slice loader stub")?;

    // 3. Detour the field-VM op-0x49 arm edge (overlay 0897, raw - maps linearly
    //    from its base). Guard the displaced pair matches the recognized build.
    let overlay_entry = patcher
        .read_entry(ov::SHOP_OVERLAY_PROT_INDEX)
        .with_context(|| format!("read field overlay PROT {}", ov::SHOP_OVERLAY_PROT_INDEX))?;
    let hook_off = (ov::SHOP_HOOK_VA - ov::SHOP_OVERLAY_BASE) as usize;
    let at_hook: Vec<u32> = overlay_entry
        .get(hook_off..hook_off + 8)
        .ok_or_else(|| anyhow::anyhow!("hook offset past end of overlay entry"))?
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    if at_hook[..] != ov::SHOP_DISPLACED[..] {
        anyhow::bail!(
            "op-0x49 hook site does not match the recognized US build; refusing to patch"
        );
    }
    let detour: Vec<u8> = ov::detour_words()
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    patcher
        .patch_prot_entry(ov::SHOP_OVERLAY_PROT_INDEX, hook_off as u64, &detour)
        .context("write op-0x49 shop detour into the field overlay")?;

    Ok(OverlaySliceReport {
        pochi_index,
        lba,
        sectors,
    })
}

/// As [`inject_overlay_slice`], but hosts the overlay via the **dead dev-mode**
/// path (option 1): the op-0x49 detour only REQUESTS a repurposed dev game-mode
/// ([`ov::DEAD_MODE_INDEX`]), and that mode's INIT handler (our gap loader) does
/// the CD load in the safe between-frames context - avoiding the mid-tick
/// reentrancy that froze the raw stub. Edits: the pochi overlay, two gap
/// routines (trigger + mode-INIT loader), the op-0x49 detour, and the dead
/// mode's mode-table handler word (guarded against an unexpected build). All 7
/// minigames stay intact. No Sony bytes.
pub fn inject_overlay_slice_dead_mode(patcher: &mut DiscPatcher) -> Result<OverlaySliceReport> {
    use crate::seru_overlay as ov;

    let overlay = ov::words_to_bytes(&ov::assemble_sentinel_overlay());
    let sectors = ov::sectors_for(overlay.len());

    // 1. Pochi host + overlay.
    let pochi_index = find_pochi_host(patcher, overlay.len())
        .ok_or_else(|| anyhow::anyhow!("no pochi-filler slot large enough for the overlay"))?;
    let lba = patcher
        .entry_disc_lba(pochi_index)
        .ok_or_else(|| anyhow::anyhow!("pochi slot {pochi_index} has no disc LBA"))?;
    patcher
        .patch_prot_entry(pochi_index, 0, &overlay)
        .with_context(|| format!("write overlay into pochi slot {pochi_index}"))?;

    // 2. Gap routines: trigger @ TRIGGER_VA, mode-INIT loader @ MODE_INIT_VA.
    let scus = patcher
        .read_named_file(SCUS_NAME)
        .context("read SCUS_942.54 for dead-mode slice")?;
    let trigger = ov::words_to_bytes(&ov::assemble_mode_request_trigger());
    let loader = ov::words_to_bytes(&ov::assemble_mode_init_loader_stub(lba, sectors));
    let resolve_gap = |va: u32, len: usize| -> Result<usize> {
        let off = legaia_asset::item_names::file_offset_for_va(&scus, va)
            .ok_or_else(|| anyhow::anyhow!("can't resolve gap VA {va:#x} in SCUS"))?;
        if scus
            .get(off..off + len)
            .is_none_or(|r| r.iter().any(|&b| b != 0))
        {
            anyhow::bail!("gap region {va:#x} is not all-zero dead space");
        }
        Ok(off)
    };
    let trig_off = resolve_gap(ov::TRIGGER_VA, trigger.len())?;
    let load_off = resolve_gap(ov::MODE_INIT_VA, loader.len())?;
    patcher
        .patch_named_file(SCUS_NAME, trig_off as u64, &trigger)
        .context("write dead-mode trigger")?;
    patcher
        .patch_named_file(SCUS_NAME, load_off as u64, &loader)
        .context("write dead-mode mode-INIT loader")?;

    // 3. Detour the op-0x49 arm edge (overlay 0897) into the trigger.
    let overlay_entry = patcher
        .read_entry(ov::SHOP_OVERLAY_PROT_INDEX)
        .with_context(|| format!("read field overlay PROT {}", ov::SHOP_OVERLAY_PROT_INDEX))?;
    let hook_off = (ov::SHOP_HOOK_VA - ov::SHOP_OVERLAY_BASE) as usize;
    let at_hook: Vec<u32> = overlay_entry
        .get(hook_off..hook_off + 8)
        .ok_or_else(|| anyhow::anyhow!("hook offset past end of overlay entry"))?
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    if at_hook[..] != ov::SHOP_DISPLACED[..] {
        anyhow::bail!(
            "op-0x49 hook site does not match the recognized US build; refusing to patch"
        );
    }
    let detour: Vec<u8> = ov::detour_words()
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    patcher
        .patch_prot_entry(ov::SHOP_OVERLAY_PROT_INDEX, hook_off as u64, &detour)
        .context("write op-0x49 dead-mode detour into the field overlay")?;

    // 4. Repurpose the dead mode's mode-table handler -> our loader (guarded).
    let handler_va = ov::dead_mode_handler_va();
    let handler_off = legaia_asset::item_names::file_offset_for_va(&scus, handler_va)
        .ok_or_else(|| anyhow::anyhow!("can't resolve mode-table handler VA {handler_va:#x}"))?;
    let cur = scus
        .get(handler_off..handler_off + 4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .ok_or_else(|| anyhow::anyhow!("mode-table handler offset out of range"))?;
    if cur != ov::DEAD_MODE_HANDLER_ORIG {
        anyhow::bail!(
            "dead-mode handler is {cur:#010x}, expected {:#010x}; refusing to patch",
            ov::DEAD_MODE_HANDLER_ORIG
        );
    }
    patcher
        .patch_named_file(
            SCUS_NAME,
            handler_off as u64,
            &ov::MODE_INIT_VA.to_le_bytes(),
        )
        .context("repurpose dead-mode mode-table handler")?;

    Ok(OverlaySliceReport {
        pochi_index,
        lba,
        sectors,
    })
}

/// As [`inject_overlay_slice`], but hosts the overlay via the **mode-24 warp**
/// (Fork A, new sub-id): the op-0x49 shop detour mirrors the op-0x3E minigame
/// warp (request mode 24 + our sub-id), and `FUN_80025980`'s per-sub-id
/// overlay-load call is detoured so that, for our sub-id, it baked-LBA-loads our
/// pochi overlay to slot A and runs it, then returns to the field via the
/// mode-24 return warp (`FUN_80026018` -> mode 2 scene reload). This is the
/// game's own clean teardown+reload path, avoiding the resume-in-place freezes.
/// Edits: the pochi overlay, two gap routines (warp trigger + FUN_80025980
/// redirect), the op-0x49 detour, and the `FUN_80025980` load-site detour (both
/// recognized-build guarded). All 7 minigames stay intact. No Sony bytes.
pub fn inject_overlay_slice_warp(patcher: &mut DiscPatcher) -> Result<OverlaySliceReport> {
    inject_overlay_slice_warp_opts(patcher, false)
}

/// As [`inject_overlay_slice_warp`], but `draw` selects the payload: `false` =
/// the sentinel slice (load proof, immediate field reload); `true` = the draw-side
/// overlay ([`crate::seru_overlay::assemble_draw_overlay`]) that hands off to mode
/// 13 and renders on screen each frame before returning to the field.
pub fn inject_overlay_slice_warp_opts(
    patcher: &mut DiscPatcher,
    draw: bool,
) -> Result<OverlaySliceReport> {
    use crate::seru_overlay as ov;

    let overlay = if draw {
        ov::words_to_bytes(&ov::assemble_draw_overlay())
    } else {
        ov::words_to_bytes(&ov::assemble_sentinel_overlay())
    };
    let sectors = ov::sectors_for(overlay.len());

    // 1. Pochi host + overlay.
    let pochi_index = find_pochi_host(patcher, overlay.len())
        .ok_or_else(|| anyhow::anyhow!("no pochi-filler slot large enough for the overlay"))?;
    let lba = patcher
        .entry_disc_lba(pochi_index)
        .ok_or_else(|| anyhow::anyhow!("pochi slot {pochi_index} has no disc LBA"))?;
    patcher
        .patch_prot_entry(pochi_index, 0, &overlay)
        .with_context(|| format!("write overlay into pochi slot {pochi_index}"))?;

    // 2. Gap routines: warp trigger + FUN_80025980 overlay-load redirect.
    let scus = patcher
        .read_named_file(SCUS_NAME)
        .context("read SCUS_942.54 for warp slice")?;
    // Fire-once guard is UNRELIABLE: its flag cell (0x8007AF28) sits in the gap
    // tail the game reuses at runtime, so it can read "already fired" before our
    // first warp and skip it. The DRAW build avoids it entirely - its overlay holds
    // the draw mode indefinitely, so the warp never returns to re-trigger (no loop
    // even without fire-once). The sentinel build keeps fire-once only because its
    // round-trip returns; that path needs a reliable flag location later.
    // Draw build = the real feature: gate to shop sub-op 0 (fires at a merchant,
    // mid-game) and skip fire-once (shops don't auto-retrigger). Sentinel build:
    // ungated + fire-once for the name-entry mechanism test.
    let trigger = ov::words_to_bytes(&ov::assemble_warp_trigger_stub_opts(
        ov::WARP_SUBID,
        !draw,
        draw,
    ));
    // Draw payload's INIT requests the persistent draw mode itself, so the redirect
    // must NOT call the return-warp (which would reload the field immediately).
    let redirect = ov::words_to_bytes(&ov::assemble_warp_init_redirect_opts(lba, sectors, !draw));
    let resolve_gap = |va: u32, len: usize| -> Result<usize> {
        let off = legaia_asset::item_names::file_offset_for_va(&scus, va)
            .ok_or_else(|| anyhow::anyhow!("can't resolve gap VA {va:#x} in SCUS"))?;
        if scus
            .get(off..off + len)
            .is_none_or(|r| r.iter().any(|&b| b != 0))
        {
            anyhow::bail!("gap region {va:#x} is not all-zero dead space");
        }
        Ok(off)
    };
    let trig_off = resolve_gap(ov::WARP_TRIGGER_VA, trigger.len())?;
    let redir_off = resolve_gap(ov::WARP_REDIRECT_VA, redirect.len())?;
    patcher
        .patch_named_file(SCUS_NAME, trig_off as u64, &trigger)
        .context("write warp trigger")?;
    patcher
        .patch_named_file(SCUS_NAME, redir_off as u64, &redirect)
        .context("write FUN_80025980 redirect")?;

    // 3. op-0x49 arm edge -> warp trigger.
    let overlay_entry = patcher
        .read_entry(ov::SHOP_OVERLAY_PROT_INDEX)
        .with_context(|| format!("read field overlay PROT {}", ov::SHOP_OVERLAY_PROT_INDEX))?;
    let hook_off = (ov::SHOP_HOOK_VA - ov::SHOP_OVERLAY_BASE) as usize;
    let at_hook: Vec<u32> = overlay_entry
        .get(hook_off..hook_off + 8)
        .ok_or_else(|| anyhow::anyhow!("hook offset past end of overlay entry"))?
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    if at_hook[..] != ov::SHOP_DISPLACED[..] {
        anyhow::bail!(
            "op-0x49 hook site does not match the recognized US build; refusing to patch"
        );
    }
    let detour: Vec<u8> = ov::detour_words()
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    patcher
        .patch_prot_entry(ov::SHOP_OVERLAY_PROT_INDEX, hook_off as u64, &detour)
        .context("write op-0x49 warp detour into the field overlay")?;

    // 4. FUN_80025980 overlay-load site -> redirect (guarded).
    let init_off = legaia_asset::item_names::file_offset_for_va(&scus, ov::WARP_INIT_DETOUR_VA)
        .ok_or_else(|| anyhow::anyhow!("can't resolve FUN_80025980 detour VA"))?;
    let at_init: Vec<u32> = scus
        .get(init_off..init_off + 8)
        .ok_or_else(|| anyhow::anyhow!("FUN_80025980 detour offset out of range"))?
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    if at_init[..] != ov::WARP_INIT_DISPLACED[..] {
        anyhow::bail!(
            "FUN_80025980 load site does not match the recognized US build; refusing to patch"
        );
    }
    let init_detour: Vec<u8> = ov::warp_init_detour_words()
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    patcher
        .patch_named_file(SCUS_NAME, init_off as u64, &init_detour)
        .context("write FUN_80025980 overlay-load redirect detour")?;

    Ok(OverlaySliceReport {
        pochi_index,
        lba,
        sectors,
    })
}

/// Inject the draw-side trade overlay triggered from the **shop picker**
/// (`FUN_801d4868`, overlay 0899) instead of the op-0x49 arm.
///
/// The op-0x49 arm trigger loses a same-frame race to the shop's menu-actor mode
/// 0x17. This routes the mode-24 warp through the picker renderer, which runs
/// every frame the settled Buy/Sell/Quit choice is on screen -- a quiet frame
/// with no competing transition. SQUARE arms the warp (a button the picker
/// ignores), so this is a decisive test of whether a mode-0x18 request issued
/// from a settled shop frame sticks. The load+run+return machinery (pochi
/// overlay, FUN_80025980 redirect, mode-24 return) is identical to the warp draw
/// build; only the trigger site differs.
pub fn inject_overlay_slice_picker(patcher: &mut DiscPatcher) -> Result<OverlaySliceReport> {
    use crate::seru_overlay as ov;

    let overlay = ov::words_to_bytes(&ov::assemble_draw_overlay());
    let sectors = ov::sectors_for(overlay.len());

    // 1. Pochi host + overlay.
    let pochi_index = find_pochi_host(patcher, overlay.len())
        .ok_or_else(|| anyhow::anyhow!("no pochi-filler slot large enough for the overlay"))?;
    let lba = patcher
        .entry_disc_lba(pochi_index)
        .ok_or_else(|| anyhow::anyhow!("pochi slot {pochi_index} has no disc LBA"))?;
    patcher
        .patch_prot_entry(pochi_index, 0, &overlay)
        .with_context(|| format!("write overlay into pochi slot {pochi_index}"))?;

    // 2. Gap routines: picker trigger (reuses the op-0x49 trigger slot) +
    //    FUN_80025980 overlay-load redirect. Draw payload holds the draw mode
    //    itself, so the redirect must NOT call the return-warp.
    let scus = patcher
        .read_named_file(SCUS_NAME)
        .context("read SCUS_942.54 for picker slice")?;
    let trigger = ov::words_to_bytes(&ov::assemble_picker_trade_detour_stub(ov::WARP_SUBID));
    let redirect = ov::words_to_bytes(&ov::assemble_warp_init_redirect_opts(lba, sectors, false));
    let resolve_gap = |va: u32, len: usize| -> Result<usize> {
        let off = legaia_asset::item_names::file_offset_for_va(&scus, va)
            .ok_or_else(|| anyhow::anyhow!("can't resolve gap VA {va:#x} in SCUS"))?;
        if scus
            .get(off..off + len)
            .is_none_or(|r| r.iter().any(|&b| b != 0))
        {
            anyhow::bail!("gap region {va:#x} is not all-zero dead space");
        }
        Ok(off)
    };
    let trig_off = resolve_gap(ov::PICKER_TRIGGER_VA, trigger.len())?;
    let redir_off = resolve_gap(ov::WARP_REDIRECT_VA, redirect.len())?;
    patcher
        .patch_named_file(SCUS_NAME, trig_off as u64, &trigger)
        .context("write picker trigger")?;
    patcher
        .patch_named_file(SCUS_NAME, redir_off as u64, &redirect)
        .context("write FUN_80025980 redirect")?;

    // 3. Picker renderer prologue -> picker trigger (overlay 0899, slot A, raw -
    //    maps linearly from base).
    let menu_entry = patcher
        .read_entry(ov::PICKER_MENU_PROT_INDEX)
        .with_context(|| format!("read menu overlay PROT {}", ov::PICKER_MENU_PROT_INDEX))?;
    let hook_off = (ov::PICKER_RENDER_VA - ov::SLOT_A_BASE) as usize;
    let at_hook: Vec<u32> = menu_entry
        .get(hook_off..hook_off + 8)
        .ok_or_else(|| anyhow::anyhow!("picker hook offset past end of menu overlay entry"))?
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    if at_hook[..] != ov::PICKER_DISPLACED[..] {
        anyhow::bail!("picker hook site does not match the recognized US build; refusing to patch");
    }
    let detour: Vec<u8> = ov::picker_detour_words()
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    patcher
        .patch_prot_entry(ov::PICKER_MENU_PROT_INDEX, hook_off as u64, &detour)
        .context("write picker trade detour into the menu overlay")?;

    // 4. FUN_80025980 overlay-load site -> redirect (guarded). SCUS-resident.
    let init_off = legaia_asset::item_names::file_offset_for_va(&scus, ov::WARP_INIT_DETOUR_VA)
        .ok_or_else(|| anyhow::anyhow!("can't resolve FUN_80025980 detour VA"))?;
    let at_init: Vec<u32> = scus
        .get(init_off..init_off + 8)
        .ok_or_else(|| anyhow::anyhow!("FUN_80025980 detour offset out of range"))?
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    if at_init[..] != ov::WARP_INIT_DISPLACED[..] {
        anyhow::bail!(
            "FUN_80025980 load site does not match the recognized US build; refusing to patch"
        );
    }
    let init_detour: Vec<u8> = ov::warp_init_detour_words()
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    patcher
        .patch_named_file(SCUS_NAME, init_off as u64, &init_detour)
        .context("write FUN_80025980 overlay-load redirect detour")?;

    Ok(OverlaySliceReport {
        pochi_index,
        lba,
        sectors,
    })
}

/// Add a native fourth "Trade" row to the shop Buy/Sell/Quit picker (overlay
/// 0899), trigger-agnostic: the cursor clamp is bumped 3 -> 4 and the renderer
/// draws/highlights the row in the game's own style. Selecting it is a clean
/// no-op (index-3 confirm falls through to the dispatcher's normal exit) until the
/// trade action is wired in. No pochi overlay, no warp infra -- only two PROT-0899
/// edits plus a draw stub + label in the SCUS gap.
pub fn inject_native_trade_row(patcher: &mut DiscPatcher) -> Result<()> {
    use crate::seru_overlay as ov;

    // 1. Cursor clamp 3 -> 4 in the dispatcher FUN_801dafd4 (overlay 0899).
    let menu_entry = patcher
        .read_entry(ov::PICKER_MENU_PROT_INDEX)
        .with_context(|| format!("read menu overlay PROT {}", ov::PICKER_MENU_PROT_INDEX))?;
    let read_word = |buf: &[u8], off: usize| -> Result<u32> {
        Ok(u32::from_le_bytes(
            buf.get(off..off + 4)
                .ok_or_else(|| anyhow::anyhow!("offset {off:#x} past end of menu overlay"))?
                .try_into()
                .unwrap(),
        ))
    };
    let clamp_off = (ov::CLAMP_VA - ov::SLOT_A_BASE) as usize;
    if read_word(&menu_entry, clamp_off)? != ov::CLAMP_OLD {
        anyhow::bail!(
            "picker clamp site does not match the recognized US build; refusing to patch"
        );
    }
    patcher
        .patch_prot_entry(
            ov::PICKER_MENU_PROT_INDEX,
            clamp_off as u64,
            &ov::CLAMP_NEW.to_le_bytes(),
        )
        .context("write picker cursor clamp 3->4")?;

    // 1b. Grow the picker box one row taller (sprite-def height for id 0x2a) so
    //     the 4th row sits inside the frame.
    let box_h_off = (ov::BOX_H_VA - ov::SLOT_A_BASE) as usize;
    let cur_h = u16::from_le_bytes(
        menu_entry
            .get(box_h_off..box_h_off + 2)
            .ok_or_else(|| anyhow::anyhow!("box height offset past end of menu overlay"))?
            .try_into()
            .unwrap(),
    );
    if cur_h != ov::BOX_H_OLD {
        anyhow::bail!(
            "picker box height does not match the recognized US build; refusing to patch"
        );
    }
    patcher
        .patch_prot_entry(
            ov::PICKER_MENU_PROT_INDEX,
            box_h_off as u64,
            &ov::BOX_H_NEW.to_le_bytes(),
        )
        .context("write picker box height (3->4 rows)")?;

    // 2. Renderer FUN_801d4868 in-body detour -> row-4 draw stub (overlay 0899).
    let row4_off = (ov::ROW4_DETOUR_VA - ov::SLOT_A_BASE) as usize;
    let at_row4: Vec<u32> = menu_entry
        .get(row4_off..row4_off + 8)
        .ok_or_else(|| anyhow::anyhow!("row-4 hook offset past end of menu overlay entry"))?
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    if at_row4[..] != ov::ROW4_DISPLACED[..] {
        anyhow::bail!(
            "renderer epilogue does not match the recognized US build; refusing to patch"
        );
    }
    let row4_detour: Vec<u8> = ov::row4_detour_words()
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    patcher
        .patch_prot_entry(ov::PICKER_MENU_PROT_INDEX, row4_off as u64, &row4_detour)
        .context("write renderer row-4 detour")?;

    // 3. Draw stub + "@Trade" label into 0899's run-C dead region (same host the
    //    full trade build uses; keeps everything off the contended SCUS gap).
    let stub = ov::words_to_bytes(&ov::assemble_row4_draw_stub());
    let write0899 = |p: &mut DiscPatcher, va: u32, bytes: &[u8], what: &str| -> Result<()> {
        let off = (va - ov::SLOT_A_BASE) as usize;
        if menu_entry
            .get(off..off + bytes.len())
            .is_none_or(|r| r.iter().any(|&b| b != 0))
        {
            anyhow::bail!("0899 region {va:#x} ({what}) is not all-zero dead space");
        }
        p.patch_prot_entry(ov::PICKER_MENU_PROT_INDEX, off as u64, bytes)
            .with_context(|| format!("write {what} into menu overlay 0899"))
    };
    write0899(patcher, ov::ROW4_STUB_VA, &stub, "row-4 draw stub")?;
    write0899(patcher, ov::TRADE_STR_VA, ov::TRADE_STR, "@Trade label")?;

    Ok(())
}

/// Full in-shop Trade vendor: Buy/Sell/**Trade**/Quit, and confirming Trade enters
/// a picker SUB-MODE (no warp) that draws the trade screen inside mode 0x17 and
/// returns to the picker on exit. Pure overlay-0899 + SCUS-gap edits; the shop is
/// never torn down. All PROT-0899 sites are guarded against the recognized US build.
pub fn inject_trade_full(patcher: &mut DiscPatcher, seed: u64) -> Result<()> {
    use crate::seru_overlay as ov;
    use legaia_asset::seru_trade as st;

    let base = ov::SLOT_A_BASE;
    let menu = patcher
        .read_entry(ov::PICKER_MENU_PROT_INDEX)
        .with_context(|| format!("read menu overlay PROT {}", ov::PICKER_MENU_PROT_INDEX))?;
    let word = |va: u32| -> Result<u32> {
        let o = (va - base) as usize;
        Ok(u32::from_le_bytes(
            menu.get(o..o + 4)
                .ok_or_else(|| anyhow::anyhow!("VA {va:#x} past end of menu overlay"))?
                .try_into()
                .unwrap(),
        ))
    };
    let words2 = |va: u32| -> Result<[u32; 2]> { Ok([word(va)?, word(va + 4)?]) };

    // --- Guard every overlay-0899 site against the recognized build ---
    if word(ov::CLAMP_VA)? != ov::CLAMP_OLD {
        anyhow::bail!("picker clamp site mismatch; refusing to patch");
    }
    let box_off = (ov::BOX_H_VA - base) as usize;
    // Guarded like every other overlay-0899 probe in this function: a
    // shorter-than-expected PROT 0899 (foreign build / bad dump) must bail
    // with "refusing to patch", not index-panic.
    let box_h = menu
        .get(box_off..box_off + 2)
        .ok_or_else(|| anyhow::anyhow!("picker box-height VA past end of menu overlay"))?;
    if u16::from_le_bytes([box_h[0], box_h[1]]) != ov::BOX_H_OLD {
        anyhow::bail!("picker box-height site mismatch; refusing to patch");
    }
    if words2(ov::ROW2_STR_LOAD_VA)? != ov::ROW2_STR_LOAD_OLD {
        anyhow::bail!("row-2 string-load site mismatch; refusing to patch");
    }
    if words2(ov::ROW4_DETOUR_VA)? != ov::ROW4_DISPLACED {
        anyhow::bail!("renderer row-4 site mismatch; refusing to patch");
    }
    if words2(ov::TRADE_DISPATCH_VA)? != ov::TRADE_DISPATCH_DISPLACED {
        anyhow::bail!("dispatch site mismatch; refusing to patch");
    }
    if words2(ov::ENTRY_VA)? != ov::ENTRY_DISPLACED {
        anyhow::bail!("FUN_801dafd4 entry site mismatch; refusing to patch");
    }

    // --- Apply the overlay-0899 edits ---
    let le2 = |w: [u32; 2]| -> Vec<u8> { w.iter().flat_map(|x| x.to_le_bytes()).collect() };
    let prot = |p: &mut DiscPatcher, va: u32, bytes: &[u8], what: &str| -> Result<()> {
        p.patch_prot_entry(ov::PICKER_MENU_PROT_INDEX, (va - base) as u64, bytes)
            .with_context(|| format!("write {what}"))
    };
    prot(
        patcher,
        ov::CLAMP_VA,
        &ov::CLAMP_NEW.to_le_bytes(),
        "cursor clamp 3->4",
    )?;
    prot(
        patcher,
        ov::BOX_H_VA,
        &ov::BOX_H_NEW.to_le_bytes(),
        "box height (4 rows)",
    )?;
    prot(
        patcher,
        ov::ROW2_STR_LOAD_VA,
        &le2(ov::row2_str_load_new()),
        "row-2 string swap (-> @Trade)",
    )?;
    prot(
        patcher,
        ov::ROW4_DETOUR_VA,
        &le2(ov::row4_detour_words()),
        "renderer row-4 detour",
    )?;
    prot(
        patcher,
        ov::TRADE_DISPATCH_VA,
        &le2(ov::trade_dispatch_detour_words()),
        "Trade dispatch detour",
    )?;
    prot(
        patcher,
        ov::ENTRY_VA,
        &le2(ov::trade_entry_detour_words()),
        "FUN_801dafd4 entry detour",
    )?;

    // --- All seru-trade code + data lives in 0899's resident run-C dead region
    // (reference-free, all-zero, reloaded with the overlay). Nothing touches the SCUS
    // rodata gap, so seru trading is compatible with every gap-based feature
    // (bonus-equipment drops, flee-EXP, the Seru-Bell name). ---
    let row4 = ov::words_to_bytes(&ov::assemble_row4_draw_stub_str(ov::QUIT_STR_VA));
    let entry = ov::words_to_bytes(&ov::assemble_trade_entry_stub());
    let disp = ov::words_to_bytes(&ov::assemble_trade_dispatch_stub());
    let handler = ov::words_to_bytes(&ov::assemble_trade_handler());
    // The precomputed vendor schedule the handler indexes by play-time bucket: one
    // `[want, give, give_level]` per bucket, derived deterministically from `seed`.
    let bucket_table = st::bucket_table_to_bytes(&st::bucket_offers(
        seed,
        st::BUCKET_COUNT,
        &st::default_pool(),
    ));
    let blobs: [(u32, &[u8], &str); 10] = [
        (ov::TRADE_HANDLER_VA, &handler, "trade handler"),
        (ov::ENTRY_STUB_VA, &entry, "entry stub"),
        (ov::TRADE_DISPATCH_STUB_VA, &disp, "dispatch stub"),
        (ov::ROW4_STUB_VA, &row4, "row-4 draw stub"),
        (ov::TRADE_STR_VA, ov::TRADE_STR, "@Trade label"),
        (ov::TITLE_STR_VA, ov::TITLE_STR, "title string"),
        (
            ov::CONFIRM_PROMPT_STR_VA,
            ov::CONFIRM_PROMPT_STR,
            "confirm prompt",
        ),
        (ov::CONFIRM_YES_STR_VA, ov::CONFIRM_YES_STR, "confirm Yes"),
        (ov::CONFIRM_NO_STR_VA, ov::CONFIRM_NO_STR, "confirm No"),
        (ov::BUCKET_TABLE_VA, &bucket_table, "bucket schedule"),
    ];
    for (va, bytes, what) in blobs {
        if va < base || va + bytes.len() as u32 > ov::TRADE_HANDLER_END {
            anyhow::bail!("0899 blob {what} ({va:#x}) outside the run-C region");
        }
        let off = (va - base) as usize;
        if menu
            .get(off..off + bytes.len())
            .is_none_or(|r| r.iter().any(|&b| b != 0))
        {
            anyhow::bail!("0899 region {va:#x} ({what}) is not all-zero dead space");
        }
        patcher
            .patch_prot_entry(ov::HANDLER_OVL_PROT_INDEX, off as u64, bytes)
            .with_context(|| format!("write {what} into menu overlay 0899"))?;
    }
    Ok(())
}
