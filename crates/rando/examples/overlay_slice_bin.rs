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

    let image = std::fs::read(&input).with_context(|| format!("read {input}"))?;
    let mut patcher = DiscPatcher::open(image).context("open disc image")?;
    let report = apply::inject_overlay_slice(&mut patcher).context("inject overlay slice")?;

    std::fs::write(&output, patcher.image()).with_context(|| format!("write {output}"))?;
    println!(
        "overlay slice -> {output}\n  pochi host PROT entry: {}\n  baked disc LBA: {} ({} sector(s))\n  sentinel {:#010X} -> RAM {:#010X} when the battle-reward hook fires",
        report.pochi_index,
        report.lba,
        report.sectors,
        legaia_rando::seru_overlay::SENTINEL,
        legaia_rando::seru_overlay::SENTINEL_ADDR,
    );
    Ok(())
}
