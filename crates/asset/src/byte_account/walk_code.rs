//! The overlay-code walker and the claim passes it runs over a code image.
//! Split out of `byte_account.rs`.

use super::*;

// --- overlay code ----------------------------------------------------------

pub(super) fn walk_overlay_code(buf: &[u8], sink: &mut Sink, opts: &AccountOptions) {
    let Some(idx) = opts.prot_index else {
        sink.note("overlay walker needs --prot-index");
        return;
    };
    let Some(rec) = crate::static_overlay::overlay_map()
        .overlays
        .iter()
        .find(|r| r.prot_index == idx)
    else {
        sink.note(format!("no static-overlays.toml row for PROT {idx}"));
        return;
    };
    let Some(dir) = opts.funcs_dir.as_ref() else {
        sink.note("overlay walker needs --funcs <ghidra/scripts/funcs>");
        return;
    };
    let dumps = match read_dump_extents(dir) {
        Ok(d) => d,
        Err(e) => {
            sink.note(format!("reading {}: {e}", dir.display()));
            return;
        }
    };
    let base = rec.base_va;
    let hi = base as u64 + buf.len() as u64;
    let label_ok = |l: &Option<String>| match l {
        Some(l) => {
            l == &rec.label || l.starts_with(&format!("{idx:04}")) || l.starts_with(&rec.label)
        }
        None => false,
    };
    let (mut confirmed, mut refuted, mut ambiguous, mut credited_by_label) = (0, 0, 0, 0);
    let mut fill_extents = 0usize;
    let mut data_headed = 0usize;
    let mut uncorroborated_labels = 0usize;
    let mut by_label: Vec<(usize, usize, u32)> = Vec::new();
    for d in &dumps {
        if (d.entry_va as u64) < base as u64 || (d.entry_va as u64) >= hi {
            continue;
        }
        let start = (d.entry_va - base) as usize;
        let end = (start + d.bytes as usize).min(buf.len());
        // A dump over an image's zero region is in the corpus (one is 4646
        // printed `nop`s) and its extent is fill, not code, in whatever image
        // it is checked against - including the one its filename names. Never
        // credit an all-zero extent as `code`: the shape classifier will call
        // the run `zero_pad`, which is what it is.
        if buf
            .get(start..end)
            .is_some_and(|w| w.iter().all(|&b| b == 0))
        {
            fill_extents += 1;
            continue;
        }
        // A dump whose opening window is the `$zero`-absolute data signature
        // is a table decoded as opcodes - pointer words read as `lb ra,
        // 0xNNNN(zero)` - and re-encoding it to this image's bytes confirms
        // only that the bytes are these bytes. The attribution sweep already
        // calls such an extent `data`; crediting it as code here claimed PROT
        // 0898's rodata (two switch tables and a string pool) as
        // `FUN_801cf5d0`.
        if zero_absolute_head(&buf[start..end]) || no_instruction_signature(&buf[start..end]) {
            data_headed += 1;
            continue;
        }
        match attribute(d, buf, base) {
            Attribution::Confirmed => {
                confirmed += 1;
                sink.claim(start, end, OWNER_CODE, format!("FUN_{:08x}", d.entry_va));
            }
            Attribution::Refuted => refuted += 1,
            // A label is no evidence for an extent that OPENS on fill. No
            // routine begins with eight `nop`s, so such a dump is a frontier
            // walk that started in padding and ran on into whatever follows -
            // in PROT 0972 five spawn records, which the label had credited as
            // `FUN_801d84b4` code. The attribution sweep already calls these
            // windows `zero_window`. (A byte-confirmed extent keeps its credit:
            // the confirmation is about the words after the fill.)
            Attribution::Unverifiable
                if buf
                    .get(start..start + ZERO_HEAD_BYTES)
                    .is_some_and(|w| w.iter().all(|&b| b == 0)) =>
            {
                fill_extents += 1;
            }
            Attribution::Unverifiable => {
                if label_ok(&d.label) {
                    by_label.push((start, end, d.entry_va));
                } else {
                    ambiguous += 1;
                }
            }
        }
    }
    // A label-credited extent is the one claim here that rests on a filename,
    // and a filename says where a dump was TAKEN, not which image the bytes
    // are. In an image where some other extent re-encodes to this image's own
    // words the label is corroborated by those; in an image where NOTHING
    // confirms, it is the whole of the evidence - and that is exactly the case
    // where the dump program's base was wrong, so the extents land at arbitrary
    // offsets in a file that never held them. Credit the label only alongside a
    // byte confirmation.
    if confirmed > 0 {
        for (start, end, entry_va) in by_label {
            credited_by_label += 1;
            sink.claim(
                start,
                end,
                OWNER_CODE,
                format!("FUN_{entry_va:08x} (by label)"),
            );
        }
    } else {
        uncorroborated_labels = by_label.len();
    }
    sink.ambiguous_dumps = ambiguous + uncorroborated_labels;
    sink.refuted_dumps = refuted;
    sink.note(format!(
        "base {:#010x} ({}); {confirmed} extents confirmed by bytes, \
         {credited_by_label} credited by filename label, {ambiguous} unverifiable, \
         {refuted} refuted (aliased sibling), {fill_extents} land on or open on fill, \
         {data_headed} open on the data signature",
        base, rec.label
    ));
    if uncorroborated_labels > 0 {
        sink.note(format!(
            "{uncorroborated_labels} label-matching extent(s) left uncredited: \
             no dump in this image confirms by bytes, so a filename is the whole \
             of their evidence"
        ));
    }
    claim_uninitialised_data(buf, sink, base);
    claim_pinned_overlay_assets(buf, sink, idx);
    claim_formed_strings(buf, sink, base);
    claim_switch_tables(buf, sink, base, opts);
    claim_accessed_globals(buf, sink, base, opts);
    claim_spawn_records(buf, sink, base, opts);
    claim_widget_scripts(buf, sink, base, opts);
    claim_loop_bounded_arrays(buf, sink, base, opts);
    {
        let own_end = inherited_tail_start(buf, opts).unwrap_or(buf.len());
        let code = code_intervals(sink);
        let in_code = |off: usize| {
            let i = code.partition_point(|&(s, _)| s <= off);
            i > 0 && off < code[i - 1].1
        };
        arrays::claim_pointer_bump_arrays(buf, sink, base, own_end, &in_code);
        arrays::claim_indexed_arrays(buf, sink, base, own_end, &in_code);
    }
}

/// Arrays whose count the loop walking them states
/// ([`loop_bounded_arrays`]), claimed from the formed base for `count *
/// stride` bytes. The array must start outside code and end below the
/// inherited tail.
pub(super) fn claim_loop_bounded_arrays(
    buf: &[u8],
    sink: &mut Sink,
    base: u32,
    opts: &AccountOptions,
) {
    let own_end = inherited_tail_start(buf, opts).unwrap_or(buf.len());
    let code = code_intervals(sink);
    let in_code = |off: usize| {
        let i = code.partition_point(|&(s, _)| s <= off);
        i > 0 && off < code[i - 1].1
    };
    let mut n = 0usize;
    for a in loop_bounded_arrays(&buf[..own_end], base) {
        let Some(off) = a.base.checked_sub(base).map(|o| o as usize) else {
            continue;
        };
        let end = off + a.byte_len();
        if end > own_end || in_code(off) || !in_code((a.bound_site - base) as usize) {
            continue;
        }
        sink.claim(
            off,
            end,
            OWNER_RECORD,
            format!(
                "array of {} x {} B, count from the loop bound at {:#010x} (base formed at {:#010x})",
                a.count, a.stride, a.bound_site, a.form_site
            ),
        );
        n += 1;
    }
    if n > 0 {
        sink.note(format!(
            "{n} array(s) sized by the counted loop that walks them"
        ));
    }
}

/// The image's `code` claims as sorted, **merged** intervals.
///
/// Dump extents nest and overlap - a dump that opens mid-routine sits inside
/// the one that opens at its prologue - so a binary search over the raw claims
/// can land on the inner extent, see an offset past its end, and report code
/// as not-code. Merged, the last interval starting at or below an offset is
/// the only one that can contain it.
pub(super) fn code_intervals(sink: &Sink) -> Vec<(usize, usize)> {
    let mut raw: Vec<(usize, usize)> = sink
        .claims
        .iter()
        .filter(|c| c.owner == OWNER_CODE)
        .map(|c| (c.start, c.end))
        .collect();
    raw.sort_unstable();
    let mut out: Vec<(usize, usize)> = Vec::with_capacity(raw.len());
    for (s, e) in raw {
        match out.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => out.push((s, e)),
        }
    }
    out
}

/// The value `reg` holds on reaching the word at `from`, walking backwards,
/// when the image's own code formed it from a `lui` ([`lui_forms`]).
///
/// The last writer of `reg` must be the completing `addiu` of a plain form,
/// or a copy (`addu`/`or` with `$zero`) of a register that resolves the same
/// way - retail stages a record pointer in a saved register and hands it over
/// with `move a2,s3`. A load, or an `addiu` off a register no `lui` reached,
/// names nothing, and so does a call crossed on the way back while the walk
/// still tracks a caller-saved register - every argument register is one. A
/// saved register (`s0..s7`, `s8`) keeps its value across the call.
pub(super) fn reg_before(
    buf: &[u8],
    base: u32,
    from: usize,
    forms: &[LuiForm],
    reg: u32,
    skip_call_at: Option<usize>,
) -> Option<u32> {
    match reg_source(buf, base, from, forms, reg, skip_call_at) {
        Some(RegSource::Formed(v)) => Some(v),
        _ => None,
    }
}

/// Where a register's value at a word came from, walking backwards.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RegSource {
    /// A `lui`-formed address.
    Formed(u32),
    /// The routine's own argument register, unwritten since its prologue
    /// (`addiu sp,sp,-N`) at the given file offset.
    Arg { reg: u32, entry: usize },
}

/// [`reg_before`], also reporting an argument register the routine received
/// untouched - which makes the routine a wrapper that forwards that argument.
pub(super) fn reg_source(
    buf: &[u8],
    base: u32,
    from: usize,
    forms: &[LuiForm],
    mut reg: u32,
    skip_call_at: Option<usize>,
) -> Option<RegSource> {
    // Wide enough for a record pointer formed at the top of a routine's
    // argument set-up and handed over dozens of words later (PROT 0972 forms
    // 0x801D8D30 in `$a2` thirty-seven words above its spawn call).
    const WINDOW: usize = 64;
    let mut o = from;
    for _ in 0..WINDOW {
        let w = legaia_bytes::u32_le(buf, o)?;
        if w >> 16 == 0x27BD && w & 0x8000 != 0 {
            return (4..=7)
                .contains(&reg)
                .then_some(RegSource::Arg { reg, entry: o });
        }
        // Past the previous routine's return: the walk has left this one.
        if w == 0x03E0_0008 && o + 4 < from {
            return None;
        }
        // A crossed call clobbers the caller-saved registers only. Once the
        // walk has followed a copy into `s0..s7` / `s8` the value survives
        // the call by the ABI - PROT 0902 stages its spawn record in `s5`
        // above a `jal` and copies it to `$a2` inside the loop below.
        if Some(o) != skip_call_at
            && matches!(Flow::of(w, o, base), Flow::Call)
            && !matches!(reg, 16..=23 | 30)
        {
            return None;
        }
        if defines(w) == Some(reg) && Some(o) != skip_call_at {
            let (op, rs, rt) = (w >> 26, (w >> 21) & 0x1F, (w >> 16) & 0x1F);
            if op == 0x00 && matches!(w & 0x3F, 0x21 | 0x25) && (rs == 0) != (rt == 0) {
                reg = rs | rt;
            } else {
                let site = base.wrapping_add(o as u32);
                return (op == 0x09)
                    .then(|| {
                        forms
                            .iter()
                            .find(|f| f.site == site && f.op == 0x09 && f.index.is_none())
                            .map(|f| RegSource::Formed(f.target))
                    })
                    .flatten();
            }
        }
        o = o.checked_sub(4)?;
    }
    None
}

/// Every value a call at `jal_off` can be handed in argument register `reg`:
/// the one formed on the fall-through path ([`reg_before`] from the call's
/// delay slot), and one per `j` that lands on the call (or a few words above
/// it, with nothing in between writing `reg`), resolved from that `j`'s own
/// delay slot. The second shape is a `switch` whose arms each load `$a2` in
/// the delay slot of a jump to one shared `jal`.
pub(super) fn arg_values_at_call(
    buf: &[u8],
    base: u32,
    jal_off: usize,
    forms: &[LuiForm],
    jumps_to: &std::collections::BTreeMap<usize, Vec<usize>>,
    reg: u32,
) -> Vec<u32> {
    let mut out: Vec<u32> = reg_before(buf, base, jal_off + 4, forms, reg, Some(jal_off))
        .into_iter()
        .collect();
    let mut t = jal_off;
    for _ in 0..4 {
        for &j in jumps_to.get(&t).map(Vec::as_slice).unwrap_or(&[]) {
            out.extend(reg_before(buf, base, j + 4, forms, reg, Some(j)));
        }
        let Some(prev) = t.checked_sub(4) else { break };
        let w = legaia_bytes::u32_le(buf, prev).unwrap_or(0);
        if defines(w) == Some(reg) || !matches!(Flow::of(w, prev, base), Flow::None) {
            break;
        }
        t = prev;
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// How far a record handed to a callee extends.
#[derive(Clone, Copy)]
pub(super) enum ArgExtent {
    /// `[i16 model_sel][u16 reserved][move-VM bytecode]`: to the program's
    /// terminator ([`crate::slot_b_module::move_program_end`]).
    MoveRecord,
    /// A fixed size the callee's own field reads pin.
    Fixed(usize),
    /// An array whose element size the callee's loop fixes and whose element
    /// count the caller hands over as an immediate in `count_reg`.
    Counted { count_reg: u32, stride: usize },
}

/// The immediate `reg` holds on reaching the word at `from`, walking
/// backwards: the last writer must be `addiu reg,$zero,imm` (`li`), reached
/// through any chain of register copies. A call crossed on the way back, the
/// routine's prologue, or any other writer names nothing.
pub(super) fn imm_before(
    buf: &[u8],
    base: u32,
    from: usize,
    mut reg: u32,
    skip_call_at: usize,
) -> Option<i32> {
    const WINDOW: usize = 64;
    let mut o = from;
    for _ in 0..WINDOW {
        let w = legaia_bytes::u32_le(buf, o)?;
        if w >> 16 == 0x27BD && w & 0x8000 != 0 {
            return None;
        }
        if o != skip_call_at {
            if matches!(Flow::of(w, o, base), Flow::Call) {
                return None;
            }
            if defines(w) == Some(reg) {
                let (op, rs, rt) = (w >> 26, (w >> 21) & 0x1F, (w >> 16) & 0x1F);
                if op == 0x09 && rs == 0 {
                    return Some(i32::from((w & 0xFFFF) as i16));
                }
                if op == 0x00 && matches!(w & 0x3F, 0x21 | 0x25) && (rs == 0) != (rt == 0) {
                    reg = rs | rt;
                } else {
                    return None;
                }
            }
        }
        o = o.checked_sub(4)?;
    }
    None
}

/// The callees whose pointer argument names a record of a known extent.
///
/// * `FUN_80021B04` (spawn) and `FUN_80050ED4` (its pool wrapper) take a
///   spawn record in `$a2` - the shape [`crate::slot_b_module`] claims across
///   the slot-B band.
/// * `FUN_80020DE0` (actor allocator) takes a static actor template in `$a0`:
///   24 bytes, `+0x00..+0x14`, fixed by the allocator's own field copies
///   (`docs/reference/functions/runtime-libs.md`, static actor templates).
/// * `FUN_80024C88` (positioned actor spawn) takes the same template in `$a1`
///   and hands it to the allocator unchanged (`move a0,a1` at `0x80024C94`,
///   then `jal 0x80020DE0`); its own reads are the three position halfwords
///   of `$a0`, a stack vector.
/// * `FUN_8001C93C` (debug value-monitor list drawer) takes `$a0` rows of
///   `0x28` bytes at `$a1`: its loop runs `$a0` times and every arm advances
///   the row pointer by `addiu s0,s0,0x28`; a row is `[i16 kind][i16 x][i16
///   y][..][u32 value ptr @ +0x08][label @ +0x0E][u32 name table @ +0x24]`
///   (`see ghidra/scripts/funcs/8001c93c.txt`).
pub(super) const ARG_RECORD_CALLEES: [(u32, u32, ArgExtent, &str); 5] = [
    (0x8002_1B04, 6, ArgExtent::MoveRecord, "spawn record"),
    (0x8005_0ED4, 6, ArgExtent::MoveRecord, "spawn record"),
    (0x8002_0DE0, 4, ArgExtent::Fixed(0x18), "actor template"),
    (0x8002_4C88, 5, ArgExtent::Fixed(0x18), "actor template"),
    (
        0x8001_C93C,
        5,
        ArgExtent::Counted {
            count_reg: 4,
            stride: 0x28,
        },
        "value-monitor row list",
    ),
];

/// Records the image's own code hands to a callee in
/// [`ARG_RECORD_CALLEES`], claimed from the consumer's pointer-forming
/// instruction to the extent the callee fixes.
///
/// Both ends are evidence rather than shape. The start is the argument the
/// call is handed ([`arg_at_call`]); the end is where a spawn record's program
/// stops (`HALT`, an armed idle loop, or a `WAIT` that never retires) or the
/// template size. A program walk that runs unterminated claims nothing, and a
/// spawn record is cut at the next consumer-formed start of the same kind so
/// two records never overlap. The call site must lie inside a `code` claim and
/// below the inherited tail, and the record must start outside code, like
/// every other data claim in this walker.
pub(super) fn claim_spawn_records(buf: &[u8], sink: &mut Sink, base: u32, opts: &AccountOptions) {
    let own_end = inherited_tail_start(buf, opts).unwrap_or(buf.len());
    let code = code_intervals(sink);
    let in_code = |off: usize| {
        let i = code.partition_point(|&(s, _)| s <= off);
        i > 0 && off < code[i - 1].1
    };
    let jal = |t: u32| 0x0C00_0000 | ((t & 0x0FFF_FFFF) >> 2);
    let forms = lui_forms(&buf[..own_end], base);
    let mut jumps_to: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    for o in (0..own_end.saturating_sub(3)).step_by(4) {
        let w = legaia_bytes::u32_le(buf, o).unwrap_or(0);
        if let Flow::Jump(t) = Flow::of(w, o, base)
            && w >> 26 == 0x02
        {
            jumps_to.entry(t).or_default().push(o);
        }
    }
    // An image-local routine that hands its own argument straight to one of
    // the callees is that callee for this purpose: PROT 0980 stages every
    // dancer effect through `FUN_801D3FD0`, which moves `$a3` into `$a2` and
    // calls the spawn. Found once, from the callees' own call sites.
    let mut record_starts: Vec<usize> = Vec::new();
    let mut callees: Vec<(u32, u32, ArgExtent, &str)> = ARG_RECORD_CALLEES.to_vec();
    for (callee, reg, extent, what) in ARG_RECORD_CALLEES {
        // A counted callee's count travels in a second register a forwarding
        // wrapper would have to be read for too; none is needed on the disc.
        if matches!(extent, ArgExtent::Counted { .. }) {
            continue;
        }
        let target = jal(callee);
        for o in (0..own_end.saturating_sub(3)).step_by(4) {
            if legaia_bytes::u32_le(buf, o) != Some(target) || !in_code(o) {
                continue;
            }
            if let Some(RegSource::Arg { reg: arg, entry }) =
                reg_source(&buf[..own_end], base, o + 4, &forms, reg, Some(o))
            {
                let va = base.wrapping_add(entry as u32);
                if !callees.iter().any(|c| c.0 == va) {
                    callees.push((va, arg, extent, what));
                }
            }
        }
    }
    for (callee, reg, extent, what) in callees {
        let target = jal(callee);
        let mut starts: Vec<usize> = Vec::new();
        // Per start, the element count every call handing it over agrees on
        // (`Counted` only); a start two calls disagree about claims nothing.
        let mut counts: std::collections::BTreeMap<usize, Option<usize>> = Default::default();
        let mut sites = 0usize;
        let mut o = 0usize;
        while o + 4 <= own_end {
            if legaia_bytes::u32_le(buf, o) == Some(target) && in_code(o) {
                sites += 1;
                let count = match extent {
                    ArgExtent::Counted { count_reg, .. } => {
                        imm_before(&buf[..own_end], base, o + 4, count_reg, o)
                            .and_then(|n| usize::try_from(n).ok())
                            .filter(|&n| n > 0)
                    }
                    _ => None,
                };
                for t in arg_values_at_call(&buf[..own_end], base, o, &forms, &jumps_to, reg) {
                    if let Some(off) = t.checked_sub(base).map(|x| x as usize)
                        && off + 4 < own_end
                        && !in_code(off)
                    {
                        starts.push(off);
                        if matches!(extent, ArgExtent::Counted { .. }) {
                            let e = counts.entry(off).or_insert(count);
                            if *e != count {
                                *e = None;
                            }
                        }
                    }
                }
            }
            o += 4;
        }
        if sites == 0 {
            continue;
        }
        starts.sort_unstable();
        starts.dedup();
        let (mut n, mut bytes) = (0usize, 0usize);
        for (i, &s) in starts.iter().enumerate() {
            let end = match extent {
                ArgExtent::MoveRecord => {
                    let Some(e) =
                        crate::slot_b_module::move_program_end(&buf[..own_end], s + 4).bounded()
                    else {
                        continue;
                    };
                    starts.get(i + 1).map_or(e, |&n| e.min(n))
                }
                ArgExtent::Fixed(len) => s + len,
                ArgExtent::Counted { stride, .. } => {
                    let Some(n) = counts.get(&s).copied().flatten() else {
                        continue;
                    };
                    let end = s + n * stride;
                    // The whole array must be data this image owns: an extent
                    // that runs into code or past the own-content end is a
                    // count read off the wrong register, not a table.
                    if end > own_end || (s..end).step_by(4).any(&in_code) {
                        continue;
                    }
                    end
                }
            }
            .min(own_end);
            let head = i16::from_le_bytes([buf[s], buf[s + 1]]);
            sink.claim(
                s,
                end,
                OWNER_RECORD,
                format!("{what}, first halfword {head} (argument of a FUN_{callee:08x} call)"),
            );
            n += 1;
            bytes += end - s;
        }
        sink.note(format!(
            "{sites} FUN_{callee:08x} call(s); {} distinct {what} start(s) formed in the \
             argument, {n} claimed ({bytes} bytes)",
            starts.len()
        ));
        if matches!(extent, ArgExtent::MoveRecord) {
            record_starts.extend(starts);
        }
    }
    claim_chained_spawn_records(&buf[..own_end], sink, &record_starts, &in_code);
}

/// A window-program interpreter an overlay image carries, and the program
/// shape it walks.
#[derive(Clone, Copy)]
pub(super) enum WindowProgram {
    /// PROT 0899's `FUN_801D6628`: 4-byte `[opcode][window][u16 operand]`
    /// instructions ending on a zero opcode, validated by
    /// [`crate::widget_script::parse_at`].
    Menu,
    /// PROT 0897's `FUN_801E9B3C`: 8-byte instructions, `[i16 opcode][i16
    /// window][u32 operand]`, walked `addiu s5,s5,8` until the opcode
    /// halfword reads zero (`lh v0,(s5)` / `bnez` at `0x801E9D90`); the window
    /// halfword indexes the overlay's own 28-byte descriptor table at
    /// `0x801F2B98` (`FUN_801E9B3C`'s `((w << 3) - w) << 2`).
    Field,
}

/// `(PROT entry, interpreter VA, program kind)` for the window-program VMs.
/// The interpreter is image-local - at any other image the same `jal` word
/// names whatever that image holds at the VA - so each row applies to its own
/// entry alone.
///
/// PROT 0896, the foreign build's options / status image, carries its own copy
/// of the menu interpreter at `FUN_801D896C`: it walks `[u8 op][u8 window][u16
/// operand]` words with `addiu s4,s4,4` until the opcode byte reads zero
/// (`lbu v0,(s4)` / `bnez` at `0x801D8BAC`) and dispatches `op - 1` through an
/// `sltiu 0xd` jump table - the menu program format, opcode bound included.
///
/// The second column is the link base the interpreter VA assumes; an image
/// accounted at any other base names nothing through it.
pub(super) const WINDOW_PROGRAM_VMS: [(u32, u32, u32, WindowProgram); 3] = [
    (899, 0x801C_E818, 0x801D_6628, WindowProgram::Menu),
    (897, 0x801C_E818, 0x801E_9B3C, WindowProgram::Field),
    (896, 0x801D_4DF0, 0x801D_896C, WindowProgram::Menu),
];

/// Window programs an overlay hands its window VM
/// ([`WINDOW_PROGRAM_VMS`]).
///
/// Each program pointer is the `$a0` a call is handed, resolved like a spawn
/// record's (`arg_values_at_call`: the fall-through value from the delay slot
/// back, plus one per `switch` arm that loads `$a0` in the delay slot of a `j`
/// to the shared call - most of the pause menu's programs are staged that way,
/// which [`crate::widget_script::scan`]'s eight-word window does not follow),
/// and the extent is the program through its terminator
/// (`docs/formats/window-script.md`).
pub(super) fn claim_widget_scripts(buf: &[u8], sink: &mut Sink, base: u32, opts: &AccountOptions) {
    let Some(&(_, link_base, vm, kind)) = WINDOW_PROGRAM_VMS
        .iter()
        .find(|r| Some(r.0) == opts.prot_index)
    else {
        return;
    };
    if base != link_base {
        return;
    }
    let own_end = inherited_tail_start(buf, opts).unwrap_or(buf.len());
    let code = code_intervals(sink);
    let in_code = |off: usize| {
        let i = code.partition_point(|&(s, _)| s <= off);
        i > 0 && off < code[i - 1].1
    };
    let forms = lui_forms(&buf[..own_end], base);
    let mut jumps_to: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    for o in (0..own_end.saturating_sub(3)).step_by(4) {
        let w = legaia_bytes::u32_le(buf, o).unwrap_or(0);
        if let Flow::Jump(t) = Flow::of(w, o, base)
            && w >> 26 == 0x02
        {
            jumps_to.entry(t).or_default().push(o);
        }
    }
    let target = 0x0C00_0000 | ((vm & 0x0FFF_FFFF) >> 2);
    let mut vas: std::collections::BTreeSet<u32> = Default::default();
    let mut sites = 0usize;
    for o in (0..own_end.saturating_sub(3)).step_by(4) {
        if legaia_bytes::u32_le(buf, o) == Some(target) && in_code(o) {
            sites += 1;
            vas.extend(arg_values_at_call(
                &buf[..own_end],
                base,
                o,
                &forms,
                &jumps_to,
                4,
            ));
        }
    }
    let starts: Vec<usize> = vas
        .iter()
        .filter_map(|&va| va.checked_sub(base).map(|o| o as usize))
        .filter(|&o| o < own_end)
        .collect();
    let (mut n, mut bytes) = (0usize, 0usize);
    for (i, &off) in starts.iter().enumerate() {
        let (end, insns) = match kind {
            WindowProgram::Menu => {
                // `parse_at` addresses the image at the menu overlay's base;
                // hand it the file offset in that frame so an image linked
                // elsewhere (PROT 0896) parses the same bytes.
                let at = crate::menu_windows::MENU_OVERLAY_BASE_VA + off as u32;
                let Ok(script) = crate::widget_script::parse_at(&buf[..own_end], at) else {
                    continue;
                };
                (off + script.byte_len(), script.insns.len())
            }
            WindowProgram::Field => {
                let mut p = off;
                let mut k = 0usize;
                while let Some(op) = buf.get(p..p + 2) {
                    if op == [0, 0] || k > crate::widget_script::MAX_SCRIPT_INSNS {
                        break;
                    }
                    p += 8;
                    k += 1;
                }
                if k > crate::widget_script::MAX_SCRIPT_INSNS || p + 8 > own_end {
                    continue;
                }
                (p + 8, k)
            }
        };
        let end = starts.get(i + 1).map_or(end, |&nx| end.min(nx));
        if in_code(off) || end > own_end || end <= off {
            continue;
        }
        sink.claim(
            off,
            end,
            OWNER_SCRIPT,
            format!("window program, {insns} instruction(s) (argument of a FUN_{vm:08x} call)"),
        );
        n += 1;
        bytes += end - off;
    }
    if sites > 0 {
        sink.note(format!(
            "{sites} FUN_{vm:08x} call(s); {n} window program(s) claimed ({bytes} bytes)"
        ));
    }
}

/// Spawn records the consumer's pointers do not name but whose position both
/// ends of a `[header][program]` chain pin.
///
/// Retail lays a module's spawn records back to back, and hands only some of
/// them to a spawn call directly; the rest are reached by a route the static
/// walk does not see. A run the pointer-credited records leave unclaimed is
/// read as a chain - each record's end is where its move-VM program stops
/// ([`crate::slot_b_module::move_program_end`]), and the next record opens
/// there - and the chain is claimed only when **both** of its ends are
/// structural: it starts at a pointer-credited record's end (or at the first
/// word above code or another claim, below a credited record), and it lands
/// exactly on a pointer-credited record's start, or on eight zero bytes of
/// padding. A chain that dies on a program that does not terminate, or lands
/// anywhere else, claims nothing. PROT 0972's records from `0x801D89E8` chain
/// from the credited `0x801D899C` exactly onto the credited `0x801D8CDC`;
/// PROT 0895's from `0x801F37D0` onto `0x801F3918`.
pub(super) fn claim_chained_spawn_records(
    buf: &[u8],
    sink: &mut Sink,
    starts: &[usize],
    in_code: &dyn Fn(usize) -> bool,
) {
    const MAX_CHAIN: usize = 256;
    let mut anchors: Vec<usize> = starts.to_vec();
    anchors.sort_unstable();
    anchors.dedup();
    if anchors.is_empty() {
        return;
    }
    let claimed: Vec<(usize, usize)> = {
        let mut v: Vec<(usize, usize)> = sink.claims.iter().map(|c| (c.start, c.end)).collect();
        v.sort_unstable();
        let mut out: Vec<(usize, usize)> = Vec::new();
        for (s, e) in v {
            match out.last_mut() {
                Some(last) if s <= last.1 => last.1 = last.1.max(e),
                _ => out.push((s, e)),
            }
        }
        out
    };
    let is_claimed = |off: usize| {
        let i = claimed.partition_point(|&(s, _)| s <= off);
        i > 0 && off < claimed[i - 1].1
    };
    let zero8 = |p: usize| buf.get(p..p + 8).is_some_and(|w| w.iter().all(|&b| b == 0));
    // Chain from `p`; `Some(records)` when it lands on an anchor or padding.
    let chain = |mut p: usize, stop_at: Option<usize>| -> Option<Vec<(usize, usize)>> {
        let mut out = Vec::new();
        for _ in 0..MAX_CHAIN {
            if Some(p) == stop_at || (stop_at.is_none() && anchors.binary_search(&p).is_ok()) {
                return Some(out);
            }
            if stop_at.is_none() && zero8(p) {
                return Some(out);
            }
            if p + 4 > buf.len() || in_code(p) || (stop_at.is_none() && is_claimed(p)) {
                return None;
            }
            let sel = i16::from_le_bytes([buf[p], buf[p + 1]]);
            if !crate::slot_b_module::dispatchable_model_sel(sel) {
                return None;
            }
            let q = crate::slot_b_module::move_program_end(buf, p + 4).bounded()?;
            if q <= p || (p + 1..q).any(in_code) {
                return None;
            }
            out.push((p, q));
            if let Some(s) = stop_at
                && q > s
            {
                return None;
            }
            p = q;
        }
        None
    };
    let mut found: Vec<(usize, usize)> = Vec::new();
    for &a in &anchors {
        // Forward: from this record's own program end.
        if let Some(e) = crate::slot_b_module::move_program_end(buf, a + 4).bounded()
            && e > a
            && !is_claimed(e)
            && let Some(recs) = chain(e, None)
        {
            found.extend(recs);
        }
        // Backward: the unclaimed run just below this record, chained onto it.
        if a >= 4 && !is_claimed(a - 4) {
            let mut g = a - 4;
            while g >= 4 && !is_claimed(g - 4) && !in_code(g - 4) && a - (g - 4) <= 0x2000 {
                g -= 4;
            }
            // Skip word padding up to the first non-zero word.
            while g < a && buf.get(g..g + 4).is_some_and(|w| w.iter().all(|&b| b == 0)) {
                g += 4;
            }
            if g < a
                && let Some(recs) = chain(g, Some(a))
            {
                found.extend(recs);
            }
        }
    }
    found.sort_unstable();
    found.dedup();
    let bytes: usize = found.iter().map(|&(s, e)| e - s).sum();
    for &(s, e) in &found {
        let head = i16::from_le_bytes([buf[s], buf[s + 1]]);
        sink.claim(
            s,
            e,
            OWNER_RECORD,
            format!(
                "spawn record, first halfword {head}, chained between pointer-credited records \
                 (both ends pinned)"
            ),
        );
    }
    if !found.is_empty() {
        sink.note(format!(
            "{} spawn record(s) chained between pointer-credited ones ({bytes} bytes)",
            found.len()
        ));
    }
}

/// Claim every scalar the image's own code loads or stores directly.
///
/// An overlay's initialised data segment has no header, no count and no
/// stride - it is whatever globals the linker laid out. What pins one of them
/// is its consumer: a `lui` pair whose second instruction is a load or store
/// forms the address *and* states the width it reads. So each such access
/// claims exactly `[target, target + width)`, and nothing between two
/// accessed words is claimed on their account.
///
/// Three guards. The forming site must lie inside a `code` claim (a dumped
/// function of this image), so a `lui`-shaped word in data forms nothing; the
/// site and the target must both lie below the inherited tail, so a donor's
/// code claims nothing here; and the target must be aligned to its width, as
/// every R3000 load and store requires.
pub(super) fn claim_accessed_globals(
    buf: &[u8],
    sink: &mut Sink,
    base: u32,
    opts: &AccountOptions,
) {
    let own_end = inherited_tail_start(buf, opts).unwrap_or(buf.len());
    let code = code_intervals(sink);
    let in_code = |off: usize| {
        let i = code.partition_point(|&(s, _)| s <= off);
        i > 0 && off < code[i - 1].1
    };
    let mut seen: std::collections::BTreeMap<(usize, u32), usize> =
        std::collections::BTreeMap::new();
    for (site, target, width) in accessed_addresses(&buf[..own_end], base) {
        let site_off = (site - base) as usize;
        if !in_code(site_off) || target < base || target % width != 0 {
            continue;
        }
        let off = (target - base) as usize;
        if off + width as usize > own_end {
            continue;
        }
        *seen.entry((off, width)).or_default() += 1;
    }
    let n = seen.len();
    for ((off, width), sites) in seen {
        sink.claim(
            off,
            off + width as usize,
            OWNER_GLOBAL,
            format!("{width}-byte global, loaded / stored directly at {sites} site(s)"),
        );
    }
    if n > 0 {
        sink.note(format!(
            "{n} data-segment global(s) sized by the load / store that reads them"
        ));
    }
}

/// Every `switch` jump table this image's own code dispatches through, each
/// read off its dispatch: base from the `lui` pair, extent from the `sltiu`
/// bound ([`crate::switch_tables`]). Cut at the inherited tail, so a dispatch
/// in the donor's residue claims nothing here.
pub(super) fn claim_switch_tables(buf: &[u8], sink: &mut Sink, base: u32, opts: &AccountOptions) {
    let own_end = inherited_tail_start(buf, opts).unwrap_or(buf.len());
    let tables = crate::switch_tables::find(buf, base, own_end);
    for t in &tables {
        let off = (t.va - base) as usize;
        sink.claim(
            off,
            off + t.byte_len(),
            OWNER_TOC,
            format!("switch table, {} arms (jr {:#010x})", t.arms, t.jr),
        );
    }
    if !tables.is_empty() {
        sink.note(format!(
            "{} switch table(s) read off their dispatch (lui base, sltiu bound)",
            tables.len()
        ));
    }
}

/// Leading zero bytes that disqualify a dump extent as a routine: eight
/// `nop`s. A compiled routine opens on its frame or its first real
/// instruction, never on a run of fill.
pub(super) const ZERO_HEAD_BYTES: usize = 32;

/// Does this window open on the `$zero`-absolute data signature - at least
/// half of its first 24 words a load or store off `$zero`? Real code reaches
/// statics through `gp` or a `lui` pair, so a run of `lb rN,0xNNNN(zero)` is a
/// table of `0x80`-high words decoded as instructions. The Rust side of
/// `looks_like_data` in `scripts/ghidra-analysis/attribute-dump-extents.py`.
pub fn zero_absolute_head(window: &[u8]) -> bool {
    let words: Vec<u32> = window
        .as_chunks::<4>()
        .0
        .iter()
        .take(24)
        .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
        .collect();
    if words.is_empty() {
        return false;
    }
    let hits = words
        .iter()
        .filter(|&&w| matches!(w >> 26, 0x20..=0x26 | 0x28..=0x2B | 0x2E) && (w >> 21) & 0x1F == 0)
        .count();
    hits * 2 >= words.len()
}

/// Does this extent read as data decoded as opcodes by what its non-`nop`
/// words **do**? A compiler never writes `$zero` except with the canonical
/// `nop`, and never emits a word that decodes to no R3000 instruction; at
/// least half the extent's non-zero words doing one or the other is a table,
/// not a routine. It catches the shape [`zero_absolute_head`] cannot - a run
/// of small halfwords or a fill-headed window shorter than
/// [`ZERO_HEAD_BYTES`] (the Baka Fighter image's `FUN_801daa50` label: seven
/// `nop`s then `mfhi zero`).
pub fn no_instruction_signature(window: &[u8]) -> bool {
    let words: Vec<u32> = window
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
        .filter(|&w| w != 0)
        .collect();
    if words.is_empty() {
        return false;
    }
    let odd = words
        .iter()
        .filter(|&&w| writes_gpr_zero(w) || !is_r3000_word(w))
        .count();
    odd >= 1 && odd * 2 >= words.len()
}

/// Whether a non-zero word is an ALU op, load or coprocessor move whose
/// destination general register is `$zero`. (A GTE load `lwc2 $0` names a
/// GTE register, not a general one, and is not counted.)
pub(super) fn writes_gpr_zero(w: u32) -> bool {
    let (op, rs, rt, rd) = (
        w >> 26,
        (w >> 21) & 0x1F,
        (w >> 16) & 0x1F,
        (w >> 11) & 0x1F,
    );
    match op {
        0x00 => {
            w != 0
                && rd == 0
                && matches!(
                    w & 0x3F,
                    0x00 | 0x02..=0x04 | 0x06 | 0x07 | 0x09 | 0x10 | 0x12 | 0x20..=0x27 | 0x2A | 0x2B
                )
        }
        0x08..=0x0F | 0x20..=0x26 => rt == 0,
        0x10 | 0x12 => matches!(rs, 0x00 | 0x02) && rt == 0,
        _ => false,
    }
}

/// Whether a word decodes to an R3000 instruction a PSX compiler emits (the
/// integer set plus `COP0` / `COP2` and `lwc2` / `swc2`).
pub(super) fn is_r3000_word(w: u32) -> bool {
    match w >> 26 {
        0x00 => matches!(
            w & 0x3F,
            0x00 | 0x02..=0x04 | 0x06..=0x09 | 0x0C | 0x0D | 0x10..=0x13 | 0x18..=0x1B | 0x20..=0x27 | 0x2A | 0x2B
        ),
        0x01 => matches!((w >> 16) & 0x1F, 0x00 | 0x01 | 0x10 | 0x11),
        0x02..=0x10 | 0x12 => true,
        0x20..=0x26 | 0x28..=0x2B | 0x2E | 0x32 | 0x3A => true,
        _ => false,
    }
}

/// Shortest share of printable ASCII (`0x20..=0x7E`) a NUL-terminated run
/// must reach to be read as a string - three quarters, so the dialog escape
/// bytes some labels carry do not disqualify them.
pub(super) const FORMED_STRING_PRINTABLE_NUM: usize = 3;
pub(super) const FORMED_STRING_PRINTABLE_DEN: usize = 4;

/// End (one past the NUL) of the C string at `off`, or `None` when the bytes
/// there are not one.
pub(super) fn cstring_end(buf: &[u8], off: usize) -> Option<usize> {
    let tail = buf.get(off..)?;
    let len = tail.iter().position(|&b| b == 0)?;
    if len == 0 {
        return None;
    }
    // The dialog font's `0xCE` escape (`docs/formats/dialog-font.md`) and the
    // index byte after it are text too: a label that opens on a glyph escape
    // is still a label. So is a Shift-JIS pair - PROT 0896 is a Japanese
    // build, and its labels are `[count][SJIS pairs][NUL]`.
    // A Shift-JIS reading is only offered to a run with no control byte in it
    // other than a leading count: `8C 8D 8E 8F 1A ...` pairs up as two SJIS
    // characters, but a `0x1A` mid-run says it is a byte table.
    let body = &tail[..len];
    let sjis_ok = body.iter().skip(1).all(|&b| b >= 0x20);
    let mut printable = 0usize;
    let mut i = 0usize;
    while i < len {
        let sjis = sjis_ok
            && matches!(body[i], 0x81..=0x9F | 0xE0..=0xEF)
            && body
                .get(i + 1)
                .is_some_and(|&t| matches!(t, 0x40..=0x7E | 0x80..=0xFC));
        if (body[i] == 0xCE && i + 1 < len) || sjis {
            printable += 2;
            i += 2;
            continue;
        }
        if (0x20..0x7F).contains(&body[i]) {
            printable += 1;
        }
        i += 1;
    }
    (printable * FORMED_STRING_PRINTABLE_DEN >= len * FORMED_STRING_PRINTABLE_NUM)
        .then_some(off + len + 1)
}

/// Strings - and tables of pointers to strings - whose address the image's own
/// code forms with a `lui` pair.
///
/// An overlay's rodata string pool has no header and no count; what bounds a
/// string is its own NUL, and what makes a byte run a *string of this image*
/// rather than text-shaped data is that this image's code computes its address
/// ([`formed_addresses`], the same pointer-forming test the uninitialised-data
/// claim rests on). One level of indirection is followed: where the formed
/// address holds a run of two or more in-image words that each point at a
/// string (or at an empty / one-byte one), the words are a pointer table and
/// are claimed with the strings they name. A target already inside a claim (code, a pinned table, the inherited
/// tail) is left to that claim.
pub(super) fn claim_formed_strings(buf: &[u8], sink: &mut Sink, base: u32) {
    let in_claim =
        |sink: &Sink, off: usize| sink.claims.iter().any(|c| c.start <= off && off < c.end);
    let to_off = |va: u32| -> Option<usize> {
        let o = va.checked_sub(base)? as usize;
        (o < buf.len()).then_some(o)
    };
    // A pair issued from the inherited tail is the donor's code forming the
    // donor's addresses; it names nothing of this image.
    let tail: Vec<(usize, usize)> = sink
        .claims
        .iter()
        .filter(|c| c.owner == OWNER_INHERITED_TAIL)
        .map(|c| (c.start, c.end))
        .collect();
    let mut targets: Vec<u32> = formed_addresses(buf, base)
        .into_iter()
        .filter(|&(site, _)| {
            let s = site.wrapping_sub(base) as usize;
            !tail.iter().any(|&(a, b)| a <= s && s < b)
        })
        .map(|(_, t)| t)
        .collect();
    targets.sort_unstable();
    targets.dedup();
    let (mut strings, mut tables) = (0usize, 0usize);
    for t in targets {
        let Some(off) = to_off(t) else { continue };
        if in_claim(sink, off) {
            continue;
        }
        if let Some(end) = cstring_end(buf, off) {
            sink.claim(
                off,
                end,
                OWNER_STRING,
                format!("string, address formed by this image ({t:#010x})"),
            );
            strings += 1;
            continue;
        }
        if off % 4 != 0 {
            continue;
        }
        let mut k = off;
        let mut named: Vec<(usize, usize)> = Vec::new();
        while let Some(w) = legaia_bytes::u32_le(buf, k) {
            let Some(s) = to_off(w) else { break };
            // Inside a table an entry may be empty or one glyph byte (the
            // options screen's button-glyph choice): the neighbours already
            // say what the table is, so the printable test is not asked of a
            // string too short to carry it.
            let short = buf
                .get(s..s + 2)
                .and_then(|b| b.iter().position(|&x| x == 0));
            let Some(e) = cstring_end(buf, s).or(short.map(|n| s + n + 1)) else {
                break;
            };
            named.push((s, e));
            k += 4;
        }
        if named.len() >= 2 {
            sink.claim(
                off,
                k,
                OWNER_TOC,
                format!(
                    "string pointer table, {} words (formed at {t:#010x})",
                    named.len()
                ),
            );
            for (s, e) in named {
                if !in_claim(sink, s) {
                    sink.claim(s, e, OWNER_STRING, "string named by a formed pointer table");
                }
            }
            tables += 1;
        }
    }
    if strings + tables > 0 {
        sink.note(format!(
            "{strings} string(s) and {tables} string-pointer table(s) at addresses this image's own code forms"
        ));
    }
}

/// Link base of the image being accounted, from its `static-overlays.toml` row.
/// Falls back to the slot-B base, which is the only base a slot-B walk is ever
/// selected for.
pub(super) fn base_for(opts: &AccountOptions) -> u32 {
    opts.prot_index
        .and_then(|i| crate::static_overlay::overlay_map().by_prot_index(i))
        .map(|r| r.base_va)
        .unwrap_or(crate::slot_b_module::SLOT_B_LINK_BASE)
}

/// File offset at which this image stops being its own content, when it is a
/// mapped overlay and the sibling entries can be read. See
/// [`crate::inherited_tail`].
pub(super) fn inherited_tail_start(buf: &[u8], opts: &AccountOptions) -> Option<usize> {
    let idx = opts.prot_index?;
    let dir = opts.prot_dir.as_ref()?;
    let t = crate::inherited_tail::tails_cached(dir)
        .get(&idx)
        .cloned()?;
    (t.image_bytes == buf.len() && t.start < buf.len()).then_some(t.start)
}

/// Claim the run at which this overlay image stops being its own content.
///
/// The packer wrote every overlay into a buffer it did not clear, so a module
/// shorter than the buffer flushes its own bytes and then the previous, longer
/// module's residue - inside the entry, at the file offsets that module
/// occupies. Those bytes are that module's, so no parser of THIS entry can ever
/// consume them: counting them as residue puts work on the worklist that no
/// work can close, and gives the run the shape of un-dumped code.
/// `scripts/ci/disc-coverage.py` has cut tails out of its denominator since the
/// rule was found; this is the byte account's side of the same cut, and
/// [`crate::inherited_tail`] is the shared measurement.
///
/// The claim is made before any walker runs, so a walker that reaches into the
/// tail (a slot-B record chain walking on into the donor's residue) loses no
/// claim of its own - claims merge - while the residue classifier no longer
/// sees the run.
pub(super) fn claim_inherited_tail(buf: &[u8], sink: &mut Sink, opts: &AccountOptions) {
    let Some(idx) = opts.prot_index else {
        return;
    };
    if crate::static_overlay::overlay_map()
        .by_prot_index(idx)
        .is_none()
    {
        return;
    }
    let Some(dir) = opts.prot_dir.as_ref() else {
        sink.note(
            "inherited-tail cut unavailable: an overlay's tail is a comparison \
             against its sibling entries, and no --prot-dir was given",
        );
        return;
    };
    let tails = crate::inherited_tail::tails_cached(dir);
    // The map carries both legs of the rule - sibling comparison and the
    // packer's buffer - so a tail whose donor is not an overlay (PROT 0898's
    // and 0895's are PROT 0894's bytes) is in it too.
    let Some(t) = tails.get(&idx) else {
        return;
    };
    // A nested pass (a decoded LZS payload) carries the outer entry's index but
    // not its bytes, and a file offset measured on the image means nothing in
    // it.
    if t.image_bytes != buf.len() || t.start >= buf.len() {
        return;
    }
    sink.claim(
        t.start,
        buf.len(),
        OWNER_INHERITED_TAIL,
        if t.donor_label.is_empty() {
            format!(
                "PROT {:04}'s bytes at the same file offset (packer buffer)",
                t.donor_prot_index
            )
        } else {
            format!(
                "PROT {:04} ({})'s bytes at the same file offset",
                t.donor_prot_index, t.donor_label
            )
        },
    );
}

/// Claim the last-sector slack above a parser's measured content end when the
/// packer's buffer reproduces it byte for byte.
///
/// The mapped overlays get the same cut from [`claim_inherited_tail`], where
/// the own-content end is itself a measurement the cut feeds back into. Every
/// other entry has a parser whose claims already *are* the content end - a
/// scene bundle's last descriptor stops where `legaia_lzs::decompress_tracked`
/// stopped consuming - so the slack above the highest claim is tested whole
/// against [`crate::inherited_tail::buffer_run`]: the nearest earlier entry
/// reaching each offset must hold the same byte there. All or nothing; a run
/// with one byte the prediction does not reproduce stays residue, and an
/// all-zero run stays the `zero_pad` it already is.
///
/// This is the whole of the `scene_asset_table` class's residue: every bundle
/// on the disc ends its last LZS stream inside its last sector, and the bytes
/// above are an earlier entry's, at the same file offsets.
pub(super) fn claim_buffer_slack(buf: &[u8], sink: &mut Sink, opts: &AccountOptions) {
    let (Some(idx), Some(dir)) = (opts.prot_index, opts.prot_dir.as_ref()) else {
        return;
    };
    if crate::static_overlay::overlay_map()
        .by_prot_index(idx)
        .is_some()
    {
        return;
    }
    let Some(end) = sink.claims.iter().map(|c| c.end).max() else {
        return;
    };
    if end >= buf.len() || buf[end..].iter().all(|&b| b == 0) {
        return;
    }
    let Some(pieces) = crate::inherited_tail::buffer_run(dir, idx, buf, end) else {
        return;
    };
    for p in pieces {
        let detail = match p.donor {
            Some(d) => format!("PROT {d:04}'s bytes at the same file offset (packer buffer)"),
            None => "zero - no earlier entry reached this offset (packer buffer)".to_string(),
        };
        sink.claim(p.start, p.end, OWNER_INHERITED_TAIL, detail);
    }
}
