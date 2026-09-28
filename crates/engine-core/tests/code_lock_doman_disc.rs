//! Disc-gated: `doman`'s code lock runs as authored.
//!
//! The one shipped `49 02` (field-VM op `0x49` sub-op 2, handler slot `0x21`,
//! `FUN_801EED58`) is in `doman`'s streamed MAN, extraction `0401`. The test
//! finds it the way the field-op census does, checks the instruction that
//! follows it is a system-flag test on flag `9` (the flag the lock's verdict
//! writes), and then runs the disc's own bytes from the `49 02` through the
//! live field VM twice: once entering the operand's code, once entering a
//! wrong one. The script must unpark both times, flag `9` must follow the
//! verdict, and the two runs must leave the flag test on different branches.
//!
//! Only structural facts are asserted - offsets, flag ids and the fact that
//! the header string is non-empty, never the code or the text.
//! Skips and passes without `LEGAIA_DISC_BIN` or `extracted/`.

use legaia_asset::field_disasm::{self as fd, InsnInfo};
use legaia_engine_core::field_submode_code_lock::{
    CODE_LOCK_RECORD, CODE_LOCK_RESULT_FLAG, CODE_LOCK_SLOT,
};
use legaia_engine_core::scene::SceneHost;
use std::path::PathBuf;

/// `doman`'s streaming MAN carrier (extraction index).
const DOMAN_STREAM_ENTRY: u32 = 401;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn gate() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let d = extracted_dir();
    if d.is_none() {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    }
    d
}

/// The `49 02` site: the record slice (from the record's script start), the
/// pc of the `49 02` inside it, and the instruction bytes.
struct Site {
    body: Vec<u8>,
    pc: usize,
    instr: Vec<u8>,
}

fn find_site(host: &SceneHost) -> Site {
    let raw = host
        .index
        .entry_bytes(DOMAN_STREAM_ENTRY)
        .expect("read the doman stream entry");
    let report = legaia_asset::parse_streaming(&raw, 4096).expect("DATA_FIELD stream");
    let key = fd::OpKey {
        opcode: 0x49,
        sub: Some(0x02),
    };
    let mut hits = Vec::new();
    for chunk in report.chunks.iter().filter(|c| c.type_byte == 0x03) {
        let start = chunk.header_offset + 4;
        let Some(man) = raw.get(start..start + chunk.size as usize) else {
            continue;
        };
        let Ok(man_file) = legaia_asset::man_section::parse(man) else {
            continue;
        };
        for (_p, _r, s, pc0, len) in fd::man_script_spans(&man_file, man) {
            let body = &man[s..s + len];
            for pc in fd::clean_hit_offsets(body, pc0, key) {
                hits.push(Site {
                    body: body.to_vec(),
                    pc,
                    instr: Vec::new(),
                });
            }
        }
    }
    assert_eq!(hits.len(), 1, "exactly one clean 49 02 in the doman stream");
    let mut site = hits.pop().unwrap();
    let insn = fd::decode(&site.body, site.pc).expect("decode 49 02");
    assert!(matches!(insn.info, InsnInfo::StateResume { sub_op: 2, .. }));
    assert_eq!(insn.size, 7, "sub-op byte plus five code bytes");
    site.instr = site.body[site.pc..site.pc + insn.size].to_vec();
    // The next instruction tests the flag the lock writes.
    let next = fd::decode(&site.body, site.pc + insn.size).expect("decode the follower");
    assert!(
        matches!(
            next.info,
            InsnInfo::SystemFlag {
                kind: fd::FlagKind::Test,
                idx,
                ..
            } if i32::from(idx) == CODE_LOCK_RESULT_FLAG
        ),
        "a system-flag test on the verdict flag follows: {:?}",
        next.info
    );
    site
}

fn bit_for(sym: u8) -> u16 {
    match sym {
        0 => 0x40,
        1 => 0x20,
        2 => 0x80,
        _ => 0x10,
    }
}

/// Run the site from the `49 02` with `code` pressed; returns the field pc
/// once the park resolves and the flag-test has run.
fn run(host: &mut SceneHost, site: &Site, code: &[u8]) -> (bool, usize) {
    let w = &mut host.world;
    w.man_load_actor_reset();
    w.load_field_script_at(site.body.clone(), site.pc);
    let pack = |m: u16| legaia_engine_core::dev_menu::retail_packed(m);
    // Let the op arm and the slot's phase 0 install its window.
    for _ in 0..4 {
        w.set_pad(0);
        let _ = w.tick();
    }
    let s = &w.field_vm.submode_screen;
    assert!(s.is_open(), "49 02 opened a submode screen");
    assert_eq!(s.actor.state, CODE_LOCK_SLOT);
    assert!(s.installed_windows.contains(&CODE_LOCK_RECORD));
    assert_eq!(w.field_pc, site.pc, "the script is parked on the op");
    let header = host.code_lock_lines();
    assert!(!header.is_empty() && !header[0].text.is_empty());
    let w = &mut host.world;
    for &sym in code {
        w.set_pad(pack(bit_for(sym)));
        let _ = w.tick();
        w.set_pad(0);
        let _ = w.tick();
    }
    for _ in 0..200 {
        w.set_pad(0);
        let _ = w.tick();
        if w.field_pc > site.pc + site.instr.len() {
            break;
        }
    }
    (w.system_flag_test(CODE_LOCK_RESULT_FLAG as u16), w.field_pc)
}

#[test]
fn doman_code_lock_runs_as_authored() {
    let Some(extracted) = gate() else { return };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene("doman", 0).expect("enter doman");
    let site = find_site(&host);
    let code: Vec<u8> = site.instr[2..7].to_vec();
    assert!(code.iter().all(|&s| s < 4), "every code byte is a symbol");

    let (right_flag, right_pc) = run(&mut host, &site, &code);
    assert!(right_flag, "the operand's code sets flag 9");
    let wrong: Vec<u8> = code.iter().map(|&s| (s + 1) & 3).collect();
    let (wrong_flag, wrong_pc) = run(&mut host, &site, &wrong);
    assert!(!wrong_flag, "any other code clears flag 9");
    assert_ne!(right_pc, wrong_pc, "the flag test branches on the verdict");
    eprintln!(
        "[ok] doman 49 02 at pc 0x{:04X}: right -> 0x{right_pc:04X}, wrong -> 0x{wrong_pc:04X}",
        site.pc
    );
}
