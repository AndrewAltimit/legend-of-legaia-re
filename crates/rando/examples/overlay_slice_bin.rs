//! Produce a patched disc carrying the custom-overlay vertical slice, for
//! emulator validation of the retail load->exec->return path.
//!
//! ```text
//! cargo run -p legaia-rando --example overlay_slice_bin -- <input.bin> <output.bin>
//! ```
//!
//! Then boot `<output.bin>` and win one battle (the slice rides the battle-reward
//! hook). The overlay should stream in from its pochi slot, run, and write the
//! sentinel `0x5E2D7ADE` to RAM `0x8007AF20` (`legaia_rando::seru_overlay::{SENTINEL,
//! SENTINEL_ADDR}`). Check that cell with a debugger / cheat / save-state read; if
//! it holds the sentinel and the game kept running, the mechanism works on hardware.

use anyhow::{Context, Result};
use legaia_rando::apply;
use legaia_rando::disc::DiscPatcher;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let input = args
        .next()
        .context("usage: overlay_slice_bin <input.bin> <output.bin>")?;
    let output = args
        .next()
        .context("usage: overlay_slice_bin <input.bin> <output.bin>")?;

    // Build-mode selection (env):
    //   LEGAIA_SLICE_WARP=1     -> mode-24 warp host (the chosen robust path):
    //     op-0x49 mirrors the op-0x3E minigame warp; FUN_80025980 is redirected to
    //     load our overlay to slot A and return to field via FUN_80026018. Clean
    //     teardown+reload (no resume-in-place freeze).
    //   LEGAIA_SLICE_DEADMODE=1 -> dead-dev-mode resume-in-place [DEAD END: froze;
    //     mode switch wipes the menu actor op-0x49 awaits. Kept as a reference.]
    //   LEGAIA_SLICE_UNGATED=1  -> raw-stub diagnostic, fires on EVERY op-0x49 arm.
    //   (default)               -> raw-stub gated to shop sub-op 0.
    use legaia_rando::seru_overlay as ov;
    // PICKER = the shop-picker quiet-frame trigger (the real shop integration
    // path): mode-24 warp armed from FUN_801d4868 on SQUARE. Always draws.
    let picker = matches!(std::env::var("LEGAIA_SLICE_PICKER").as_deref(), Ok("1"));
    let draw = picker || matches!(std::env::var("LEGAIA_SLICE_DRAW").as_deref(), Ok("1"));
    let warp = draw || matches!(std::env::var("LEGAIA_SLICE_WARP").as_deref(), Ok("1"));
    let dead_mode = matches!(std::env::var("LEGAIA_SLICE_DEADMODE").as_deref(), Ok("1"));
    let gated = !matches!(std::env::var("LEGAIA_SLICE_UNGATED").as_deref(), Ok("1"));

    // TRADEROW = the trigger-agnostic native "Trade" row only (clamp 3->4 + draw +
    // highlight); selecting it is a no-op. Independently testable styling pass.
    let traderow = matches!(std::env::var("LEGAIA_SLICE_TRADEROW").as_deref(), Ok("1"));
    // TRADE = full flow: native row + Trade-confirm -> mode-24 warp -> trade screen.
    let trade = matches!(std::env::var("LEGAIA_SLICE_TRADE").as_deref(), Ok("1"));

    let image = std::fs::read(&input).with_context(|| format!("read {input}"))?;
    let mut patcher = DiscPatcher::open(image).context("open disc image")?;

    if trade {
        // Seed for the precomputed vendor schedule (override with LEGAIA_SLICE_SEED).
        let seed = std::env::var("LEGAIA_SLICE_SEED")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0xC0FFEE);
        apply::inject_trade_full(&mut patcher, seed).context("inject trade flow")?;
        std::fs::write(&output, patcher.image()).with_context(|| format!("write {output}"))?;
        let mut sidecar = String::new();
        let mut emit = |va: u32, words: &[u32]| {
            for (i, w) in words.iter().enumerate() {
                sidecar += &format!("{:08X} {:08X}\n", va + (i as u32) * 4, w);
            }
        };
        emit(ov::CLAMP_VA, &[ov::CLAMP_NEW]);
        emit(ov::ROW2_STR_LOAD_VA, &ov::row2_str_load_new());
        emit(ov::ROW4_DETOUR_VA, &ov::row4_detour_words());
        emit(
            ov::ROW4_STUB_VA,
            &ov::assemble_row4_draw_stub_str(ov::QUIT_STR_VA),
        );
        emit(ov::TRADE_DISPATCH_VA, &ov::trade_dispatch_detour_words());
        emit(
            ov::TRADE_DISPATCH_STUB_VA,
            &ov::assemble_trade_dispatch_stub(),
        );
        emit(ov::ENTRY_VA, &ov::trade_entry_detour_words());
        emit(ov::ENTRY_STUB_VA, &ov::assemble_trade_entry_stub());
        emit(ov::TRADE_HANDLER_VA, &ov::assemble_trade_handler());
        // The precomputed bucket schedule as resident data words (128 bytes).
        let table = legaia_asset::seru_trade::bucket_table_to_bytes(
            &legaia_asset::seru_trade::bucket_offers(
                seed,
                legaia_asset::seru_trade::BUCKET_COUNT,
                &legaia_asset::seru_trade::default_pool(),
            ),
        );
        let table_words: Vec<u32> = table
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect();
        emit(ov::BUCKET_TABLE_VA, &table_words);
        let sidecar_path = format!("{output}.rampatch");
        std::fs::write(&sidecar_path, &sidecar).with_context(|| format!("write {sidecar_path}"))?;
        println!("  ram-patch sidecar -> {sidecar_path}");
        println!(
            "trade flow -> {output}\n  host: IN-SHOP TRADE (Buy/Sell/Trade/Quit; confirm Trade -> sub-mode 3 -> SERU TRADE screen in mode 0x17; X -> back to picker)\n  dispatch {:#010X}; entry {:#010X}; handler {:#010X} (PROT {})",
            ov::TRADE_DISPATCH_VA,
            ov::ENTRY_VA,
            ov::TRADE_HANDLER_VA,
            ov::PICKER_MENU_PROT_INDEX,
        );
        return Ok(());
    }

    if traderow {
        apply::inject_native_trade_row(&mut patcher).context("inject native trade row")?;
        std::fs::write(&output, patcher.image()).with_context(|| format!("write {output}"))?;
        let mut sidecar = String::new();
        let mut emit = |va: u32, words: &[u32]| {
            for (i, w) in words.iter().enumerate() {
                sidecar += &format!("{:08X} {:08X}\n", va + (i as u32) * 4, w);
            }
        };
        emit(ov::CLAMP_VA, &[ov::CLAMP_NEW]);
        emit(ov::ROW4_DETOUR_VA, &ov::row4_detour_words());
        emit(ov::ROW4_STUB_VA, &ov::assemble_row4_draw_stub());
        let sidecar_path = format!("{output}.rampatch");
        std::fs::write(&sidecar_path, &sidecar).with_context(|| format!("write {sidecar_path}"))?;
        println!("  ram-patch sidecar -> {sidecar_path}");
        println!(
            "native trade row -> {output}\n  host: NATIVE TRADE ROW (clamp 3->4 + in-body draw/highlight; select = no-op)\n  clamp {:#010X} (PROT {}); renderer detour {:#010X}; stub {:#010X}; label \"@Trade\" {:#010X}",
            ov::CLAMP_VA,
            ov::PICKER_MENU_PROT_INDEX,
            ov::ROW4_DETOUR_VA,
            ov::ROW4_STUB_VA,
            ov::TRADE_STR_VA,
        );
        return Ok(());
    }

    let report = if picker {
        apply::inject_overlay_slice_picker(&mut patcher).context("inject picker slice")?
    } else if warp {
        apply::inject_overlay_slice_warp_opts(&mut patcher, draw).context("inject warp slice")?
    } else if dead_mode {
        apply::inject_overlay_slice_dead_mode(&mut patcher).context("inject dead-mode slice")?
    } else {
        apply::inject_overlay_slice_opts(&mut patcher, gated).context("inject overlay slice")?
    };

    std::fs::write(&output, patcher.image()).with_context(|| format!("write {output}"))?;

    // Emit a RAM-patch sidecar (`<output>.rampatch`): the patched words at their
    // RAM addresses (single source of truth = the Rust assemblers). For the raw
    // stub it covers the detour + stub; for dead-mode it covers the detour + the
    // two gap routines (the mode-table handler patch is SCUS-resident, applied at
    // cold boot, so it is not in this RAM sidecar).
    let mut sidecar = String::new();
    let mut sidecar_words = 0usize;
    let mut emit = |va: u32, words: &[u32]| {
        for (i, w) in words.iter().enumerate() {
            sidecar += &format!("{:08X} {:08X}\n", va + (i as u32) * 4, w);
            sidecar_words += 1;
        }
    };
    if picker {
        emit(ov::PICKER_RENDER_VA, &ov::picker_detour_words());
        emit(
            ov::PICKER_TRIGGER_VA,
            &ov::assemble_picker_trade_detour_stub(ov::WARP_SUBID),
        );
        emit(
            ov::WARP_REDIRECT_VA,
            &ov::assemble_warp_init_redirect_opts(report.lba, report.sectors, false),
        );
        emit(ov::WARP_INIT_DETOUR_VA, &ov::warp_init_detour_words());
    } else {
        emit(ov::SHOP_HOOK_VA, &ov::detour_words());
    }
    if warp && !picker {
        emit(
            ov::WARP_TRIGGER_VA,
            &ov::assemble_warp_trigger_stub_opts(ov::WARP_SUBID, !draw, draw),
        );
        emit(
            ov::WARP_REDIRECT_VA,
            &ov::assemble_warp_init_redirect_opts(report.lba, report.sectors, !draw),
        );
        // The FUN_80025980 load-site detour is SCUS-resident code (applied at cold
        // boot), included here so a probe can mirror it if needed.
        emit(ov::WARP_INIT_DETOUR_VA, &ov::warp_init_detour_words());
    } else if dead_mode {
        emit(ov::TRIGGER_VA, &ov::assemble_mode_request_trigger());
        emit(
            ov::MODE_INIT_VA,
            &ov::assemble_mode_init_loader_stub(report.lba, report.sectors),
        );
    } else {
        emit(
            ov::STUB_VA,
            &ov::assemble_shop_loader_stub_gated(report.lba, report.sectors, gated),
        );
    }
    let sidecar_path = format!("{output}.rampatch");
    std::fs::write(&sidecar_path, &sidecar).with_context(|| format!("write {sidecar_path}"))?;

    let host = if picker {
        "PICKER WARP + DRAW (shop FUN_801d4868 SQUARE -> mode-24 warp -> trade screen)"
    } else if draw {
        "MODE-24 WARP + DRAW (slot-A overlay draws via mode 13, returns to field)"
    } else if warp {
        "MODE-24 WARP (Fork A: new sub-id; clean teardown+reload; minigames intact)"
    } else if dead_mode {
        "DEAD-MODE (resume-in-place; DEAD END - froze)"
    } else if gated {
        "raw stub, gated (shop sub-op 0 only)"
    } else {
        "raw stub, UNGATED (every op-0x49 arm)"
    };
    println!("  ram-patch sidecar -> {sidecar_path} ({sidecar_words} words)");
    println!(
        "overlay slice -> {output}\n  host: {host}\n  pochi host PROT entry: {}\n  baked disc LBA: {} ({} sector(s))\n  sentinel {:#010X} -> RAM {:#010X}",
        report.pochi_index,
        report.lba,
        report.sectors,
        ov::SENTINEL,
        ov::SENTINEL_ADDR,
    );
    if picker {
        println!(
            "  picker: SQUARE at FUN_801d4868 ({:#010X}, PROT {}) -> sub-id {} -> slot A {:#010X}; FUN_80025980 redirect {:#010X}; return {:#010X}",
            ov::PICKER_RENDER_VA,
            ov::PICKER_MENU_PROT_INDEX,
            ov::WARP_SUBID,
            ov::SLOT_A_BASE,
            ov::WARP_REDIRECT_VA,
            ov::MODE24_RETURN_FN,
        );
    }
    if warp && !picker {
        println!(
            "  warp: sub-id {} -> slot A {:#010X}; FUN_80025980 @ {:#010X} redirected -> {:#010X}; return via FUN_80026018 {:#010X}",
            ov::WARP_SUBID,
            ov::SLOT_A_BASE,
            ov::WARP_INIT_DETOUR_VA,
            ov::WARP_REDIRECT_VA,
            ov::MODE24_RETURN_FN,
        );
    }
    if dead_mode {
        println!(
            "  dead mode: index {} (was handler {:#010X}) -> loader {:#010X}",
            ov::DEAD_MODE_INDEX,
            ov::DEAD_MODE_HANDLER_ORIG,
            ov::MODE_INIT_VA,
        );
    }
    Ok(())
}
