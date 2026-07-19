//! Disc-gated oracle for the ambient motion VM's two facing ops (`0x04`,
//! `0x0D` of `FUN_80038158`), replayed against the **whole disc corpus** of
//! authored sites rather than hand-written fixtures.
//!
//! Corpus: every retail `scene_asset_table` bundle's MAN, tail-section 1
//! (`legaia_asset::man_motion`), every motion record, every flag-gated
//! variant of every record's stream. Each variant's bytecode is walked by
//! op width and every `0x04` / `0x0D` site collected, then replayed through
//! [`legaia_engine_vm::ambient_motion`] until it retires.
//!
//! What this pins - the properties that separate a correct port from a
//! plausible one:
//!
//! - **Every authored turn lands exactly on a compass point.** Both ops
//!   index the `0x80073F04` eight-entry LUT, so the endpoint is one of the
//!   eight 45-degree headings, written verbatim by the terminal arm. A port
//!   that steps to the endpoint instead of snapping leaves a rounding
//!   residue and fails here.
//! - **`0x04` runs for exactly `(b2 & 0x7F) + 1` ticks at any frame
//!   scalar.** Its cursor is `+1` per tick, not `_DAT_1F800393`-scaled -
//!   the trap, because the sibling VM's ramps *are* scaled. The oracle
//!   replays every site at three different scalars and asserts the tick
//!   count is scalar-invariant.
//! - **`0x0D` retires in `ceil(duration / speed) + 1` ticks** - its cursor
//!   *is* scalar-driven, in lockstep with the ramp scheduler it installs
//!   into, so op and ramp retire together.
//! - **The turn travels the authored direction, the whole way.** `b1 & 0x80`
//!   forces increasing/decreasing with no shortest-arc override, so the
//!   traced heading must be monotone in that direction modulo `0x1000` and
//!   the total swept arc must equal the mod-`0x1000` arc - not its
//!   complement.
//! - **Raw pre-unwrap headings are held mid-ramp.** Whenever the authored
//!   arc crosses the `0x1000` boundary the trace must contain a value
//!   outside `0..0xFFF`; masking per tick would hide it and diverge from
//!   the traced retail `+0x26`.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` or `extracted/PROT/` is missing
//! (CLAUDE.md disc-gated convention).

use legaia_asset::man_motion;
use legaia_asset::man_section;
use legaia_asset::scene_asset_table;
use legaia_engine_vm::ambient_motion::{
    AmbientFacingSite, AmbientMotion, AmbientTick, facing_sites,
};
use std::path::PathBuf;

/// One census row: a facing site plus where on the disc it came from.
struct Site {
    entry: String,
    record: usize,
    variant: usize,
    site: AmbientFacingSite,
}

fn extracted_prot() -> Option<PathBuf> {
    [
        "extracted/PROT",
        "../extracted/PROT",
        "../../extracted/PROT",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|p| p.is_dir())
}

/// Re-derive the disc-wide ambient-facing census. Mirrors the corpus walk in
/// `legaia-asset`'s `man_section_corpus` oracle: bundle -> MAN descriptor
/// (type byte `0x03`) -> LZS -> section parse -> motion records.
fn census() -> Option<Vec<Site>> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let prot = extracted_prot().or_else(|| {
        eprintln!("[skip] extracted/PROT/ missing - run `legaia-extract` first");
        None
    })?;

    let mut entries: Vec<PathBuf> = std::fs::read_dir(&prot)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("BIN"))
        .collect();
    entries.sort();

    let mut out = Vec::new();
    for path in &entries {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let Some(table) = scene_asset_table::detect(&bytes) else {
            continue;
        };
        let Some(desc) = table.descriptors.iter().find(|d| d.type_byte == 0x03) else {
            continue;
        };
        let start = desc.data_offset as usize;
        if start >= bytes.len() {
            continue;
        }
        let Ok((man, _)) = legaia_lzs::decompress_tracked(&bytes[start..], desc.size as usize)
        else {
            continue;
        };
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        harvest(&man, &name, &mut out);
    }

    // Second carrier family: the **streaming variant MANs** - raw (not LZS)
    // type-`0x03` chunks inside DATA_FIELD streaming entries. The bundle MAN
    // above is one carrier per scene; a scene's story-state variants arrive
    // here, and they carry motion streams of their own.
    for path in &entries {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let Ok(report) = legaia_asset::parse_streaming(&bytes, 4096) else {
            continue;
        };
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        for chunk in &report.chunks {
            if chunk.type_byte != 0x03 {
                continue;
            }
            let a = chunk.header_offset + 4;
            let Some(man) = bytes.get(a..a.saturating_add(chunk.size as usize)) else {
                continue;
            };
            harvest(man, &format!("{name}+{:#x}", chunk.header_offset), &mut out);
        }
    }
    Some(out)
}

/// Decode one MAN's tail-section-1 motion streams and collect every facing
/// site in every gated variant of every record.
fn harvest(man: &[u8], label: &str, out: &mut Vec<Site>) {
    let Ok(man_file) = man_section::parse(man) else {
        return;
    };
    for (ri, rec) in man_motion::motion_records(man, &man_file)
        .iter()
        .enumerate()
    {
        for (vi, var) in man_motion::stream_variants(man, rec).iter().enumerate() {
            let (a, b) = (var.code_offset, var.code_end.min(man.len()));
            if a >= b {
                continue;
            }
            for site in facing_sites(&man[a..b]) {
                out.push(Site {
                    entry: label.to_string(),
                    record: ri,
                    variant: vi,
                    site,
                });
            }
        }
    }
}

/// Replay a site and return `(tick_count, raw_trace)`.
fn replay(site: &AmbientFacingSite, start: u16, speed: u8) -> (usize, Vec<u16>) {
    let code: Vec<u8> = if site.op == 0x04 {
        vec![
            0x04,
            site.lut_index | if site.decreasing { 0x80 } else { 0 },
            (site.duration as u8) & 0x7F,
        ]
    } else {
        let d = (site.duration as i16).to_le_bytes();
        vec![
            0x0D,
            site.lut_index | if site.decreasing { 0x80 } else { 0 },
            d[0],
            d[1],
        ]
    };
    let mut vm = AmbientMotion::new(7, start);
    let mut trace = Vec::new();
    let mut ticks = 0usize;
    // Generous cap: the longest authored `0x04` budget is 0x7F, and a
    // `0x0D` retires in duration/speed ticks.
    let cap = 4096usize;
    while ticks < cap {
        let r = vm.tick(&code, speed);
        ticks += 1;
        trace.push(vm.heading);
        if usize::from(vm.pc) >= code.len() {
            break;
        }
        assert_eq!(r, AmbientTick::Yield, "a mid-leg tick must yield");
    }
    (ticks, trace)
}

/// Total arc swept by a trace in the given direction, summed modulo
/// `0x1000` - the invariant that catches a port that quietly takes the
/// short way round when the op forced the long one.
fn swept_arc(start: u16, trace: &[u16], decreasing: bool) -> u32 {
    let mut total = 0u32;
    let mut prev = start & 0x0FFF;
    for h in trace {
        let cur = h & 0x0FFF;
        let step = if decreasing {
            (i32::from(prev) - i32::from(cur) + 0x1000) & 0xFFF
        } else {
            (i32::from(cur) - i32::from(prev) + 0x1000) & 0xFFF
        };
        total += step as u32;
        prev = cur;
    }
    total
}

#[test]
fn disc_ambient_facing_sites_replay_onto_their_compass_endpoints() {
    let Some(sites) = census() else { return };
    assert!(
        !sites.is_empty(),
        "corpus is empty - the oracle would be vacuous"
    );

    let n04 = sites.iter().filter(|s| s.site.op == 0x04).count();
    let n0d = sites.iter().filter(|s| s.site.op == 0x0D).count();
    eprintln!(
        "[ambient facing census] {} sites ({} x 0x04 ramp, {} x 0x0D tween) \
         across {} MAN-carrying PROT entries",
        sites.len(),
        n04,
        n0d,
        {
            let mut e: Vec<&str> = sites.iter().map(|s| s.entry.as_str()).collect();
            e.sort_unstable();
            e.dedup();
            e.len()
        }
    );

    // Start headings chosen to exercise both wrap directions and a
    // non-compass start (nothing guarantees an NPC is compass-aligned when
    // an ambient leg begins - the interact face-the-player write lands on
    // arbitrary bearings).
    const STARTS: [u16; 4] = [0x000, 0x321, 0x800, 0xFF0];

    let mut wrap_crossings = 0usize;
    for s in &sites {
        let site = &s.site;
        let where_ = format!(
            "{} rec{} var{} +{:#x} op{:#04x}",
            s.entry, s.record, s.variant, site.offset, site.op
        );
        for start in STARTS {
            for speed in [1u8, 2, 4] {
                let (ticks, trace) = replay(site, start, speed);
                let last = *trace.last().expect("at least one tick");

                // 1. Endpoint is the compass entry, verbatim and in range.
                assert_eq!(
                    last,
                    site.target(),
                    "{where_}: start {start:#05X} speed {speed} did not land on \
                     its compass endpoint"
                );

                // 2. Cadence.
                if site.op == 0x04 {
                    assert_eq!(
                        ticks,
                        site.duration.max(0) as usize + 1,
                        "{where_}: 0x04 cursor must be unit-per-tick \
                         (scalar-invariant), speed {speed}"
                    );
                } else {
                    let d = site.duration.max(0) as usize;
                    let sp = usize::from(speed);
                    let expect = d.div_ceil(sp) + 1;
                    assert_eq!(
                        ticks, expect,
                        "{where_}: 0x0D cursor must advance by the frame \
                         scalar, speed {speed}"
                    );
                }

                // 3. Direction: the swept arc equals the authored arc, not
                //    its complement. (Skip when start == target: no motion.)
                let arc = if site.decreasing {
                    (i32::from(start & 0xFFF) - i32::from(site.target()) + 0x1000) & 0xFFF
                } else {
                    (i32::from(site.target()) - i32::from(start & 0xFFF) + 0x1000) & 0xFFF
                } as u32;
                if arc != 0 {
                    assert_eq!(
                        swept_arc(start, &trace, site.decreasing),
                        arc,
                        "{where_}: swept the wrong way / wrong distance, \
                         start {start:#05X} speed {speed}"
                    );
                }

                // 4. Raw pre-unwrap hold whenever the arc crosses the
                //    0x1000 boundary.
                let crosses = if site.decreasing {
                    site.target() > (start & 0xFFF)
                } else {
                    site.target() < (start & 0xFFF)
                };
                if crosses && arc != 0 {
                    wrap_crossings += 1;
                    if site.op == 0x04 && site.duration >= 2 {
                        // `0x04` steps the heading itself, so the raw
                        // out-of-range values are in the trace.
                        assert!(
                            trace[..trace.len() - 1].iter().any(|h| *h > 0x0FFF),
                            "{where_}: a wrap-crossing 0x04 leg must hold raw \
                             >0xFFF headings mid-ramp (start {start:#05X} \
                             speed {speed}): {trace:04X?}"
                        );
                    } else if site.op == 0x0D {
                        // `0x0D` parks the raw value at install time and the
                        // scheduler interpolates on it, so observe the VM
                        // step alone (a short enough tween can retire before
                        // any intermediate value is ever stored).
                        let code = [
                            0x0Du8,
                            site.lut_index | if site.decreasing { 0x80 } else { 0 },
                            (site.duration as i16).to_le_bytes()[0],
                            (site.duration as i16).to_le_bytes()[1],
                        ];
                        let mut vm = AmbientMotion::new(7, start);
                        vm.step_ops(&code, speed);
                        assert!(
                            vm.heading > 0x0FFF,
                            "{where_}: the 0x0D pre-unwrap must park the \
                             heading outside 0..0xFFF (start {start:#05X}), \
                             got {:#06X}",
                            vm.heading
                        );
                    }
                }
            }
        }
    }
    assert!(
        wrap_crossings > 0,
        "no wrap-crossing leg in the corpus - the raw-hold assertion is vacuous"
    );
}

#[test]
fn disc_ambient_facing_operands_stay_inside_the_decoded_space() {
    let Some(sites) = census() else { return };
    assert!(!sites.is_empty());
    for s in &sites {
        // The LUT index is masked `& 7` by both ops, so the decode can never
        // escape the eight real compass entries into the adjacent SCUS data
        // the sibling VM's `& 0xF` index can reach.
        assert!(s.site.lut_index <= 7);
        assert_eq!(
            s.site.target() & 0x1FF,
            0,
            "endpoints are multiples of 0x200"
        );
        assert!(s.site.target() <= 0xE00);
        // `0x04`'s budget is a 7-bit field; `0x0D`'s is a signed 16-bit
        // frame count and a negative one would retire instantly.
        if s.site.op == 0x04 {
            assert!((0..=0x7F).contains(&s.site.duration));
        }
    }
}
