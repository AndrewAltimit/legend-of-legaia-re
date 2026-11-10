//! The interpreter's GTE against the GTE register files captured in the
//! PCSX-Redux state library. A state's coprocessor holds the inputs and the
//! outputs of the commands it last ran; where those outputs are an RTPS / RTPT
//! of the vertices still in `V0..V2`, or an NCLIP / AVSZ of the FIFOs, re-running
//! the command from the state's own inputs has to land on the captured outputs,
//! the perspective divide and its saturations included.
//!
//! The register file is the emulator's GTE, not the game's, so this checks the
//! coprocessor rather than any routine. Skips when the library is absent.

use psx_retail_exec::gte::Gte;
use std::path::PathBuf;

fn varint(b: &[u8], mut o: usize) -> Option<(u64, usize)> {
    let mut v = 0u64;
    let mut s = 0;
    loop {
        let c = *b.get(o)?;
        o += 1;
        v |= u64::from(c & 0x7F) << s;
        if c < 0x80 {
            return Some((v, o));
        }
        s += 7;
        if s > 63 {
            return None;
        }
    }
}

/// `(field, start, len)` of each length-delimited field of a message.
fn delimited(b: &[u8], start: usize, end: usize) -> Option<Vec<(u64, usize, usize)>> {
    let mut out = Vec::new();
    let mut o = start;
    while o < end {
        let (tag, o2) = varint(b, o)?;
        o = match tag & 7 {
            0 => varint(b, o2)?.1,
            1 => o2 + 8,
            5 => o2 + 4,
            2 => {
                let (len, o3) = varint(b, o2)?;
                out.push((tag >> 3, o3, len as usize));
                o3 + len as usize
            }
            _ => return None,
        };
    }
    (o == end).then_some(out)
}

fn packed32(b: &[u8], start: usize, len: usize) -> Option<[u32; 32]> {
    let mut out = [0u32; 32];
    let mut o = start;
    let mut n = 0;
    while o < start + len {
        let (v, o2) = varint(b, o)?;
        *out.get_mut(n)? = v as u32;
        n += 1;
        o = o2;
    }
    (n == 32).then_some(out)
}

/// The GTE data / control files of a `.sstate`: PCSX-Redux's registers
/// message is top-level field 4, holding `cp2d` (3) and `cp2c` (4) packed.
fn gte_of(path: &PathBuf) -> Option<Gte> {
    let raw = std::fs::read(path).ok()?;
    let p = psx_pcsx_redux::sstate_payload(&raw).ok()?;
    let top = delimited(&p, 0, p.len())?;
    let &(_, rs, rl) = top.iter().find(|(f, _, _)| *f == 4)?;
    let regs = delimited(&p, rs, rs + rl)?;
    let field = |n: u64| regs.iter().find(|(f, _, _)| *f == n).copied();
    let (_, ds, dl) = field(3)?;
    let (_, cs, cl) = field(4)?;
    Some(Gte {
        data: packed32(&p, ds, dl)?,
        ctrl: packed32(&p, cs, cl)?,
    })
}

const RTPS: u32 = 0x0018_0001;
const RTPT: u32 = 0x0028_0030;
const NCLIP: u32 = 0x0140_0006;
const AVSZ3: u32 = 0x0158_002D;
const AVSZ4: u32 = 0x0168_002E;

#[test]
fn gte_reproduces_the_captured_register_files() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../saves/library/pcsx-redux");
    let Ok(rd) = std::fs::read_dir(&dir) else {
        eprintln!("no PCSX-Redux state library; skipping");
        return;
    };
    let mut paths: Vec<_> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "sstate"))
        .collect();
    paths.sort();
    // Every state costs a gunzip of its whole RAM; a deterministic spread of
    // the library keeps the run short.
    let step = (paths.len() / 160).max(1);
    let (mut seen, mut rtp, mut rtpt, mut nclip, mut avsz) = (0, 0, 0, 0, 0);
    let mut bad = Vec::new();
    for path in paths.iter().step_by(step) {
        let Some(cap) = gte_of(path) else { continue };
        seen += 1;
        let name = path.file_name().unwrap().to_string_lossy().to_string();

        // A lone RTPS of any vertex slot (an RTPT's last vertex is V2's). A
        // transform match (MAC1..3, IR1..3 and the pushed SZ3, not all zero)
        // says that command ran last; its divide and projection must match.
        for n in 0..3 {
            let mut g = cap.clone();
            g.data[0] = cap.data[2 * n];
            g.data[1] = cap.data[2 * n + 1];
            g.execute(RTPS);
            let transform = (9..=11)
                .chain(25..=27)
                .all(|r| g.read_data(r) == cap.read_data(r))
                && g.read_data(19) == cap.read_data(19)
                && cap.read_data(27) != 0;
            if !transform {
                continue;
            }
            if g.read_data(14) == cap.read_data(14) {
                rtp += 1;
                // An RTPT leaves all three; count it when the slots agree.
                let mut t = cap.clone();
                t.execute(RTPT);
                if n == 2
                    && (12..=14)
                        .chain(17..=19)
                        .all(|r| t.read_data(r) == cap.read_data(r))
                {
                    rtpt += 1;
                }
            } else {
                bad.push(format!(
                    "{name} v{n}: sxy2 {:08x} vs {:08x}, sz3 {:x}, h {:x}, ir {:x?}",
                    g.data[14],
                    cap.data[14],
                    cap.data[19],
                    cap.ctrl[26],
                    [cap.data[9], cap.data[10]],
                ));
            }
            break;
        }
        let mut g = cap.clone();
        g.execute(NCLIP);
        if g.read_data(24) == cap.read_data(24) {
            nclip += 1;
        }
        for cmd in [AVSZ3, AVSZ4] {
            let mut g = cap.clone();
            g.execute(cmd);
            if g.read_data(24) == cap.read_data(24) && g.read_data(7) == cap.read_data(7) {
                avsz += 1;
            }
        }
    }
    eprintln!(
        "{seen} states: {rtp} RTPS ({rtpt} as RTPT), {nclip} NCLIP, {avsz} AVSZ register files reproduced"
    );
    assert!(bad.is_empty(), "projection differs:\n{}", bad.join("\n"));
    if seen > 20 {
        assert!(
            rtp > 0 && rtpt > 0,
            "no state held a reproducible RTPS / RTPT"
        );
        assert!(
            nclip > 0 && avsz > 0,
            "no state held a reproducible NCLIP / AVSZ"
        );
    }
}
