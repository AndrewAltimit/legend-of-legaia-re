use super::*;

/// Render an instruction as a single text line.
///
/// Format (similar to MIPS objdump):
///
/// ```text
///   0x000A  4C E2 03 00 00 00       FmvTrigger fmv_id=3 (MV4.STR)
/// ```
pub fn format_instruction(insn: &Insn, bytecode: &[u8]) -> String {
    let mut bytes_str = String::new();
    let end = (insn.pc + insn.size).min(bytecode.len());
    for (i, b) in bytecode[insn.pc..end].iter().enumerate() {
        if i > 0 {
            bytes_str.push(' ');
        }
        bytes_str.push_str(&format!("{:02X}", b));
    }
    let mnemonic = render_mnemonic(insn);
    format!("  0x{:04X}  {:24}  {}", insn.pc, bytes_str, mnemonic)
}

fn render_mnemonic(insn: &Insn) -> String {
    use InsnInfo::*;
    let ext = if let Some(t) = insn.extended {
        format!("[ext target=0x{:02X}] ", t)
    } else {
        String::new()
    };
    let body = match &insn.info {
        Nop => "Nop".into(),
        ExecMove { move_id } => format!("ExecMove move_id={move_id}"),
        MoveTo { xb, zb } => format!("MoveTo xb=0x{xb:02X} zb=0x{zb:02X}"),
        JmpRel { delta, target } => {
            format!("JmpRel delta=0x{delta:04X} -> 0x{target:04X}")
        }
        LFlag { kind, bit } => format!("LFlag.{kind:?} bit={bit}"),
        GFlag { kind, bit } => format!("GFlag.{kind:?} bit={bit}"),
        CFlag { kind, bit } => format!("CFlag.{kind:?} bit={bit}"),
        Bgm { text_id, sub_op } => format!("Bgm text_id={text_id} sub={sub_op:#x}"),
        Yield { kind } => format!("Yield ({kind:?})"),
        CamCfg { op0, op1 } => format!("CamCfg op0=0x{op0:02X} op1=0x{op1:02X}"),
        GiveItem { item_id } => format!("GiveItem item_id={item_id}"),
        AddMoney { signed_24 } => format!("AddMoney delta={signed_24}"),
        SetItemCount { slot, count } => format!("SetItemCount slot={slot} count={count}"),
        PartyAdd { char_id } => format!("PartyAdd char_id={char_id}"),
        PartyRemove { char_id } => format!("PartyRemove char_id={char_id}"),
        CondJmp {
            mode,
            op1,
            delta,
            target,
        } => format!("CondJmp mode={mode} op1=0x{op1:02X} delta=0x{delta:04X} -> 0x{target:04X}"),
        WarpOrInteract { op0, op1, is_warp } => {
            if *is_warp {
                format!("Warp map_id={}", op0 - 100)
            } else {
                format!("Interact op0=0x{op0:02X} op1=0x{op1:02X}")
            }
        }
        RenderCfg { long, op0, .. } => format!(
            "RenderCfg {} op0=0x{:02X}",
            if *long { "long" } else { "short" },
            op0
        ),
        SceneRegisterWrite { b0, b1, b2 } => {
            format!("SceneRegisterWrite [{b0}, {b1}, {b2}]")
        }
        Counter { op0 } => format!("Counter op=0x{op0:02X}"),
        Animate { count, base_id } => format!("Animate count={count} base_id={base_id}"),
        SceneFade { word0, word1 } => {
            format!("SceneFade word0=0x{word0:04X} word1=0x{word1:04X}")
        }
        Camera { op0, kind } => format!("Camera op0=0x{op0:02X} {kind:?}"),
        BBoxTest {
            x_min,
            z_min,
            x_max,
            z_max,
            skip_target,
            ..
        } => format!("BBoxTest [{x_min},{z_min}..{x_max},{z_max}] skip-> 0x{skip_target:04X}"),
        SceneChange {
            index,
            name_len,
            entry_x,
            entry_z,
            ..
        } => format!(
            "SceneChange index={index} name_len={name_len} entry=(0x{entry_x:02X},0x{entry_z:02X})"
        ),
        DataBlock { len } => format!("DataBlock len={len}"),
        WaitFrames { target } => format!("WaitFrames target={target}"),
        InventoryCmp {
            page,
            mode_byte,
            kind,
        } => format!("InventoryCmp page={page} mode=0x{mode_byte:02X} {kind:?}"),
        StateResume { sub_op, kind } => format!("StateResume sub={sub_op:#x} {kind:?}"),
        Effect { op0, kind } => format!("Effect op0=0x{op0:02X} {kind:?}"),
        ActorCtrl { sub_op, kind } => format!("ActorCtrl sub={sub_op:#x} {kind:?}"),
        MenuCtrl { op0, kind } => match kind {
            MenuCtrlKind::FmvTrigger { fmv_id } => {
                let name = fmv_filename(*fmv_id);
                format!("FmvTrigger fmv_id={fmv_id} ({name})")
            }
            other => format!("MenuCtrl op0=0x{op0:02X} {other:?}"),
        },
        SystemFlag {
            kind, idx, target, ..
        } => match target {
            Some(t) => format!("SysFlag.{kind:?} idx=0x{idx:04X} -> 0x{t:04X}"),
            None => format!("SysFlag.{kind:?} idx=0x{idx:04X}"),
        },
        Byte { value } => format!(".byte 0x{value:02X}"),
    };
    format!("{ext}{body}")
}

/// Recover the destination scene name of a [`InsnInfo::SceneChange`] (`0x3F`)
/// instruction from the bytecode it was decoded against.
///
/// The name is a `name_len`-byte slice at `insn_start + header + 3` (header is
/// 2 for the `0x80` cross-context form, 1 otherwise). Returns `None` when `insn`
/// is not a `SceneChange`, the slice runs past `bytecode`, or the bytes aren't a
/// clean ASCII scene label - the same desync guard the `0x3E` warp gate uses:
/// the linear walk hits literal `?` (`0x3F`) bytes inside message text, so a
/// caller must reject names that aren't lowercase-ASCII-ish CDNAME labels.
/// Genuine destinations are short (`town01`, `dolk`, `rikuroa`, …).
pub fn scene_change_name(bytecode: &[u8], insn: &Insn) -> Option<String> {
    let InsnInfo::SceneChange { name_len, .. } = insn.info else {
        return None;
    };
    let header = if insn.extended.is_some() { 2 } else { 1 };
    let start = insn.pc + header + 3;
    let raw = bytecode.get(start..start + name_len as usize)?;
    clean_scene_name(raw)
}

/// The clean-CDNAME-label gate shared by [`scene_change_name`] and the field-VM
/// `0x3F` executor. A genuine destination name is short, non-empty, and a
/// lowercase-ASCII / digit CDNAME label (`town01`, `dolk`, `rikuroa`, …).
/// Rejects anything else - the desync guard for a literal `?` (`0x3F`) landing
/// inside message text, which would otherwise decode a bogus "name". Returns the
/// owned name on success.
pub fn clean_scene_name(raw: &[u8]) -> Option<String> {
    if raw.is_empty()
        || raw.len() > 12
        || !raw
            .iter()
            .all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit())
    {
        return None;
    }
    Some(String::from_utf8_lossy(raw).into_owned())
}

/// Map a retail FMV index to its filename via the runtime FMV-state
/// table at `0x801D0A6C`. The retail mapping skips `MV2.STR` and
/// `MV5.STR` (disc-resident but not referenced by any FMV slot) and
/// reaches them via `MV3.STR` segments instead. Slots `5..=11`
/// reference cut paths.
pub fn fmv_filename(fmv_id: i16) -> &'static str {
    match fmv_id {
        0 => "MV1.STR",
        1 => "MV3.STR",
        2 => "MV3.STR", // second segment of MV3 (different start sector)
        3 => "MV4.STR",
        4 => "MV6.STR",
        5 => "(cut: MOV15.STR)",
        6..=11 => "(cut: MOV.STR)",
        _ => "(unknown)",
    }
}

/// Convenience: scan a script body for every `0x4C 0xE2` FMV trigger and
/// return the decoded `(pc, fmv_id)` pairs. Useful for the per-scene MV
/// index lift in the cutscene-table workflow.
pub fn find_fmv_triggers(bytecode: &[u8]) -> Vec<(usize, i16)> {
    let mut out = Vec::new();
    for r in LinearWalker::new(bytecode, 0) {
        if let Ok(insn) = r
            && let InsnInfo::MenuCtrl {
                kind: MenuCtrlKind::FmvTrigger { fmv_id },
                ..
            } = insn.info
        {
            out.push((insn.pc, fmv_id));
        }
    }
    out
}
