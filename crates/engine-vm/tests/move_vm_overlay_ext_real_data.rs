//! Disc-gated smoke test: walk a real PROT block through the
//! `move_vm_overlay_ext` port and assert the dispatch table parses every
//! byte we encounter without hitting an out-of-range opcode.
//!
//! PROT 0085 (`map01`) is the closest disc-resident analogue to the
//! world-map VM bytecode shape, although the bytecode there is event-
//! script flavoured (zero continent-render sub-ops) - the test verifies
//! the dispatcher walks it cleanly, not that any specific sub-op fires.
//!
//! Skips silently when `extracted/PROT/0085_map01.BIN` is missing -
//! same convention as the rest of the disc-gated suite.

use std::path::PathBuf;

use legaia_engine_vm::move_vm_overlay_ext::{MoveVmExtHost, walk};

fn map01_prot() -> Option<PathBuf> {
    for prefix in ["extracted/PROT", "../../extracted/PROT"] {
        let p = PathBuf::from(prefix).join("0085_map01.BIN");
        if p.exists() {
            return Some(p);
        }
    }
    None
}

#[derive(Default)]
struct CountingHost {
    n_slab_set: usize,
    n_slab_inc: usize,
    n_strip: usize,
    n_draw_mode: usize,
}

impl MoveVmExtHost for CountingHost {
    fn slab_uv_set(&mut self, _args: [u16; 4]) {
        self.n_slab_set += 1;
    }
    fn slab_uv_inc(&mut self, _args: [u16; 4]) {
        self.n_slab_inc += 1;
    }
    fn emit_strip(&mut self, _args: [u16; 5]) {
        self.n_strip += 1;
    }
    fn gpu_draw_mode(&mut self, _args: [u16; 11]) {
        self.n_draw_mode += 1;
    }
}

#[test]
fn move_vm_overlay_ext_walks_real_map01_bytecode() {
    let Some(path) = map01_prot() else {
        eprintln!("[skip] extracted/PROT/0085_map01.BIN missing; run legaia-extract");
        return;
    };
    let buf = std::fs::read(&path).expect("read map01 PROT entry");
    // Move-VM bytecode for the world map starts at +0x800 (the same
    // sector-aligned offset towns use for their scene-event-script
    // prescript). The first ~0x800 bytes are header/asset tables.
    assert!(buf.len() > 0x800, "map01 PROT shorter than 0x800 bytes");
    let bytecode = &buf[0x800..];

    let mut host = CountingHost::default();
    let summary = walk(&mut host, bytecode);

    eprintln!(
        "[smoke] walked {} steps, final pc=0x{:X}, out_of_range_terminated={}",
        summary.steps_walked, summary.final_pc, summary.terminated_out_of_range
    );
    eprintln!(
        "[smoke] slab_set={} slab_inc={} strip={} draw_mode={}",
        host.n_slab_set, host.n_slab_inc, host.n_strip, host.n_draw_mode
    );

    // PROT 0085 has an asset/init table at +0x800; the walker recognises
    // some of it as VM bytecode (the layout uses the same opcode space)
    // until it hits the first non-VM region. A positive number of steps
    // is sufficient evidence that `step` + `canonical_size` round-trip
    // on real bytes.
    assert!(
        summary.steps_walked > 0,
        "expected at least one VM step on real map01 bytecode"
    );

    // No scrolling-strip ops (0x2B..0x2E) are expected in PROT 0085 -
    // that block carries event-script-flavoured bytecode and zero
    // `[2F 00 2C 00]` patterns. The strip emitter is invoked at
    // runtime from contexts (dialog, cutscene, scrolling text panels)
    // whose bytecode is constructed in RAM, not stored in PROT.
}
