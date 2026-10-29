//! What one PROT entry holds: the sub-assets the previews and exporters work
//! on, found by the format crates' own scanners (`tim_scan`, `tmd_scan`,
//! `scene_tmd_stream`, `legaia_vab::find_vabs`, `parse_streaming`,
//! `legaia_mes`). Nothing here decodes a format itself.

use std::sync::Arc;

use legaia_container_formats::{StreamChunk, parse_streaming, tim_scan, tmd_scan};
use legaia_disc_audit::categorize::{self, Class};
use legaia_translate::build::TextCodec;

/// Where a sub-asset's bytes live: straight in the entry, or inside one of
/// its LZS-decompressed sections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Raw,
    /// Index into the scan's section list ([`Analysis::tim_sections`] for a
    /// TIM, [`Analysis::tmd_sections`] for a TMD).
    Lzs(usize),
}

impl Origin {
    pub fn describe(self) -> String {
        match self {
            Origin::Raw => "raw".into(),
            Origin::Lzs(i) => format!("lzs#{i}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TimRef {
    pub origin: Origin,
    pub offset: usize,
    pub width: u32,
    pub height: u32,
    pub bpp: u32,
    pub palettes: usize,
}

#[derive(Debug, Clone)]
pub struct TmdRef {
    pub origin: Origin,
    pub offset: usize,
    pub len: usize,
    pub objects: u32,
    pub verts: u32,
    pub prims: u32,
    /// Found by the scene-mesh stream detector (a battle backdrop shell).
    pub scene_stream: bool,
}

#[derive(Debug, Clone)]
pub struct VabRef {
    pub offset: usize,
    pub report: legaia_vab::VabReport,
    /// Spans resolved through the DATA_FIELD body chunk (`parse_in_stream`)
    /// rather than walked straight on from the size table.
    pub in_stream: bool,
}

#[derive(Debug, Clone)]
pub struct SeqRef {
    pub offset: usize,
    pub ppqn: u16,
    pub bpm: f32,
}

#[derive(Debug, Clone)]
pub struct MesRef {
    /// `None` = the whole entry; `Some(i)` = streaming chunk `i`.
    pub chunk: Option<usize>,
    pub offset: usize,
    pub len: usize,
    pub format: &'static str,
    /// One rendered text per message (Compact layout) or per text-bearing
    /// record (Records layout).
    pub messages: Vec<String>,
    pub records: usize,
}

/// Everything the UI needs about one entry.
pub struct Analysis {
    pub pos: usize,
    pub index: u32,
    pub bytes: Arc<Vec<u8>>,
    pub class: Class,
    pub tims: Vec<TimRef>,
    pub tim_sections: Vec<Vec<u8>>,
    pub tmds: Vec<TmdRef>,
    pub tmd_sections: Vec<Vec<u8>>,
    pub vabs: Vec<VabRef>,
    pub seqs: Vec<SeqRef>,
    pub mes: Vec<MesRef>,
    /// Dialog lines found by the translation pipeline's scanners: the
    /// scene-bundle MAN script (`from_man`) and raw dialog carriers.
    pub dialog: Vec<DialogLine>,
    pub chunks: Vec<StreamChunk>,
}

/// One dialog line, in the language-pack markup (`|` = line break,
/// `{c1:00}` = a name substitution; see `legaia_translate::markup`).
#[derive(Debug, Clone)]
pub struct DialogLine {
    /// `true`: offset is in the decompressed scene MAN; `false`: in the entry.
    pub from_man: bool,
    pub offset: usize,
    pub text: String,
}

impl Analysis {
    pub fn tim_bytes(&self, t: &TimRef) -> &[u8] {
        let base: &[u8] = match t.origin {
            Origin::Raw => &self.bytes,
            Origin::Lzs(i) => self.tim_sections.get(i).map(Vec::as_slice).unwrap_or(&[]),
        };
        base.get(t.offset..).unwrap_or(&[])
    }

    pub fn tmd_bytes(&self, t: &TmdRef) -> &[u8] {
        let base: &[u8] = match t.origin {
            Origin::Raw => &self.bytes,
            Origin::Lzs(i) => self.tmd_sections.get(i).map(Vec::as_slice).unwrap_or(&[]),
        };
        base.get(t.offset..t.offset + t.len)
            .or_else(|| base.get(t.offset..))
            .unwrap_or(&[])
    }

    /// Every TIM in the entry uploaded into a VRAM image - the texture
    /// source the TMD preview samples, as the site's viewer builds it.
    pub fn vram(&self) -> legaia_tim::Vram {
        let mut vram = legaia_tim::Vram::new();
        for t in &self.tims {
            if let Ok(tim) = legaia_tim::parse(self.tim_bytes(t)) {
                vram.upload_tim(&tim);
            }
        }
        vram
    }
}

/// Analyse one entry's bytes. `codec` is the disc build's dialog codec
/// ([`crate::Library::text_codec`]).
pub fn analyze(pos: usize, index: u32, bytes: Vec<u8>, codec: TextCodec) -> Analysis {
    let high = matches!(codec, TextCodec::Latin { allow_high: true });
    let buf = bytes.as_slice();
    let class = categorize::classify(buf).class;

    let tim_scan = tim_scan::scan_entry(buf);
    let tims = tim_scan
        .hits
        .iter()
        .map(|(src, h)| {
            let origin = match src {
                tim_scan::Source::Raw => Origin::Raw,
                tim_scan::Source::Lzs(i) => Origin::Lzs(*i),
            };
            TimRef {
                origin,
                offset: h.offset,
                width: h.width,
                height: h.height,
                bpp: h.bpp,
                palettes: 0,
            }
        })
        .collect::<Vec<_>>();

    let tmd_scan = tmd_scan::scan_entry(buf);
    let mut tmds: Vec<TmdRef> = Vec::new();
    if class == Class::SceneTmdStream
        && let Some(s) = legaia_scene_formats::scene_tmd_stream::detect(buf)
    {
        let r = s.tmd_range();
        if let Ok(tmd) = legaia_tmd::parse(&buf[r.clone()]) {
            let st = tmd.stats();
            tmds.push(TmdRef {
                origin: Origin::Raw,
                offset: r.start,
                len: r.end - r.start,
                objects: tmd.header.nobj,
                verts: st.total_vertices as u32,
                prims: st.total_primitives as u32,
                scene_stream: true,
            });
        }
    }
    for (src, h) in &tmd_scan.hits {
        let origin = match src {
            tmd_scan::Source::Raw => Origin::Raw,
            tmd_scan::Source::Lzs(i) => Origin::Lzs(*i),
        };
        if tmds
            .iter()
            .any(|t| t.origin == origin && t.offset == h.offset)
        {
            continue;
        }
        tmds.push(TmdRef {
            origin,
            offset: h.offset,
            len: h.byte_len,
            objects: h.n_obj,
            verts: h.total_verts,
            prims: h.total_prims,
            scene_stream: false,
        });
    }

    let mut vabs = Vec::new();
    for off in legaia_vab::find_vabs(buf) {
        let streamed = off
            .checked_sub(4)
            .and_then(|s| legaia_vab::parse_in_stream(buf, s).ok());
        let (report, in_stream) = match streamed {
            Some(r) => (r, true),
            None => match legaia_vab::parse(buf, off) {
                Ok(r) => (r, false),
                Err(_) => continue,
            },
        };
        vabs.push(VabRef {
            offset: off,
            report,
            in_stream,
        });
    }

    let mut seqs = Vec::new();
    let mut from = 0;
    while let Some(rel) = buf[from..].windows(4).position(|w| w == b"pQES") {
        let off = from + rel;
        if let Ok(h) = legaia_seq::parse_header(&buf[off..]) {
            seqs.push(SeqRef {
                offset: off,
                ppqn: h.ppqn,
                bpm: h.bpm(),
            });
        }
        from = off + 4;
    }

    let chunks = parse_streaming(buf, 4096)
        .ok()
        .filter(|r| r.terminated && r.all_known_types && !r.chunks.is_empty())
        .map(|r| r.chunks)
        .unwrap_or_default();

    let mut mes = Vec::new();
    // The Records layout has no magic, so a whole-entry match is trusted only
    // for the Compact layout; a MES-typed chunk is trusted either way.
    if let Some(m) =
        mes_at(buf, None, 0, codec).filter(|m| m.format == legaia_mes::Format::Compact.name())
    {
        mes.push(m);
    }
    for (i, c) in chunks.iter().enumerate() {
        if c.type_byte != 0x04 {
            continue;
        }
        let start = c.header_offset + 4;
        let end = (start + c.size as usize).min(buf.len());
        if let Some(m) = mes_at(&buf[start..end], Some(i), start, codec) {
            mes.push(m);
        }
    }

    // Field dialog lives in the scene MAN script, not in MES blobs (scene
    // bundles carry an empty MES stub); read it the way `legaia-patcher
    // translate export` does, with the build's own codec.
    let mut dialog: Vec<DialogLine> = Vec::new();
    if let Some(man) = legaia_translate::export::SceneManText::locate(buf) {
        dialog.extend(
            legaia_translate::export::scene_man_lines(&man, codec)
                .into_iter()
                .map(|l| DialogLine {
                    from_man: true,
                    offset: l.off,
                    text: render_cells(&l.source, high),
                }),
        );
    }
    dialog.extend(
        legaia_translate::export::raw_lines(buf, codec)
            .into_iter()
            .map(|l| DialogLine {
                from_man: false,
                offset: l.off,
                text: render_cells(&l.source, high),
            }),
    );

    let mut a = Analysis {
        pos,
        index,
        bytes: Arc::new(bytes.clone()),
        class,
        tims,
        tim_sections: tim_scan.lzs_sections,
        tmds,
        tmd_sections: tmd_scan.lzs_sections,
        vabs,
        seqs,
        mes,
        dialog,
        chunks,
    };
    let counts: Vec<usize> = a
        .tims
        .iter()
        .map(|t| {
            legaia_tim::parse(a.tim_bytes(t))
                .map(|tim| tim.palette_count())
                .unwrap_or(0)
        })
        .collect();
    for (t, n) in a.tims.iter_mut().zip(counts) {
        t.palettes = n;
    }
    a
}

/// A MES blob at the start of `buf`, with its messages rendered.
///
/// A Compact blob is read message by message through its offset table
/// (`legaia_mes::interp`). A Records blob has no per-message table (its
/// record structure is unknown, `docs/formats/mes.md`), so its text is the
/// `0x1F`-led dialog segments inside each record, found by the translation
/// scanner (`legaia_translate::segments::scan_ext`) and shown one message per
/// record.
fn mes_at(buf: &[u8], chunk: Option<usize>, offset: usize, codec: TextCodec) -> Option<MesRef> {
    let blob = legaia_mes::parse(buf).ok()?;
    let high = matches!(codec, TextCodec::Latin { allow_high: true });
    let messages = match &blob.records {
        Some(records) => records_text(buf, records, high),
        None => legaia_mes::interp::extract_all_messages(buf)
            .map(|all| all.iter().map(|ev| render_message(ev, high)).collect())
            .unwrap_or_default(),
    };
    Some(MesRef {
        chunk,
        offset,
        len: blob.size.min(buf.len()),
        format: blob.format.name(),
        records: blob.records.as_ref().map(Vec::len).unwrap_or(0),
        messages,
    })
}

/// The text of a Records-layout blob: per record (marker to next marker),
/// its dialog segments in the language-pack markup, joined by line breaks.
/// Records without text are skipped.
fn records_text(buf: &[u8], records: &[legaia_mes::RecordBoundary], high: bool) -> Vec<String> {
    let mut out = Vec::new();
    for (i, r) in records.iter().enumerate() {
        let end = records.get(i + 1).map_or(buf.len(), |n| n.offset);
        let Some(rec) = buf.get(r.offset..end) else {
            continue;
        };
        let lines: Vec<String> = legaia_translate::segments::scan_ext(rec, high)
            .into_iter()
            .map(|s| {
                let text = legaia_translate::markup::decode(&rec[s.text_off..s.text_off + s.len]);
                render_cells(&text, high).replace('|', "\n")
            })
            .collect();
        if !lines.is_empty() {
            out.push(format!("[record @0x{:X}]\n{}", r.offset, lines.join("\n")));
        }
    }
    out
}

/// Replace the `{xx}` escapes of the accented Latin cells with the letters
/// they draw (`{82}` -> `é`), using the one cell table every translation
/// tool reads (`legaia_font::latin`). Only on a build whose font draws those
/// cells as letters (`high`); the USA font keeps other symbols there, so
/// its escapes stay as they are. Two-byte control tokens (`{c1:00}`) are
/// untouched.
pub fn render_cells(markup: &str, high: bool) -> String {
    if !high || !markup.contains('{') {
        return markup.to_string();
    }
    let chars: Vec<char> = markup.chars().collect();
    let mut out = String::with_capacity(markup.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '{'
            && let [h1, h2, '}', ..] = &chars[i + 1..]
            && let (Some(a), Some(b)) = (h1.to_digit(16), h2.to_digit(16))
            && let Some(ch) = cell_char(((a << 4) | b) as u8)
        {
            out.push(ch);
            i += 4;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// The letter an accented Latin cell draws, if the layout names one.
fn cell_char(byte: u8) -> Option<char> {
    if byte < 0x80 || legaia_translate::markup::is_two_byte_op(byte) {
        return None;
    }
    legaia_font::latin::cell_for_byte(byte).and_then(|c| c.ch)
}

/// Render one message's events as readable text: `0x20..=0x7E` glyph cells
/// are ASCII (the dialog font's layout, `docs/formats/dialog-font.md`),
/// `|` is the in-game line break, the accented Latin cells are their letters
/// on a build that draws them (`high`), and everything else is a bracketed
/// tag.
pub fn render_message(events: &[legaia_mes::interp::MesEvent], high: bool) -> String {
    use legaia_mes::interp::{MesEvent, SubstituteKind};
    let mut out = String::new();
    for ev in events {
        match ev {
            MesEvent::Glyph(b'|') => out.push('\n'),
            MesEvent::Glyph(g @ 0x20..=0x7E) => out.push(*g as char),
            MesEvent::Glyph(g) => match cell_char(*g).filter(|_| high) {
                Some(ch) => out.push(ch),
                None => out.push_str(&format!("{{{g:02x}}}")),
            },
            MesEvent::WideGlyph(op, arg) => out.push_str(&format!("{{{op:02x}:{arg:02x}}}")),
            MesEvent::Substitute { kind, arg } => {
                let tag = match kind {
                    SubstituteKind::CharacterName => "char",
                    SubstituteKind::ItemName => "item",
                    SubstituteKind::MagicName => "magic",
                    SubstituteKind::SpellName => "spell",
                    SubstituteKind::QuestName => "quest",
                };
                out.push_str(&format!("[{tag} {arg}]"));
            }
            MesEvent::Spacing(_) => out.push(' '),
            MesEvent::SkipTwo(g) => out.push_str(&format!("{{{g:02x}}}")),
            MesEvent::Control(b) => out.push_str(&format!("[ctl {b:02x}]")),
            MesEvent::EndOfMessage(_) => {}
            MesEvent::Truncated(op) => out.push_str(&format!("[trunc {op:02x}]")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accent_cells_render_only_on_a_build_that_draws_them() {
        let m = "r{82}colte {c1:00}|f{88}te";
        assert_eq!(render_cells(m, true), "récolte {c1:00}|fête");
        assert_eq!(render_cells(m, false), m);
        // A control token's operand is never mistaken for a cell.
        assert_eq!(render_cells("{cf:82}", true), "{cf:82}");
    }

    #[test]
    fn records_blob_text_comes_from_its_segments() {
        let mut blob = Vec::new();
        for text in [
            &b"Hello there, friend."[..],
            b"The second r\x82cord is here.",
        ] {
            blob.extend_from_slice(&[0x44, 0x78, 0, 0]);
            blob.push(0x1F);
            blob.extend_from_slice(text);
            blob.push(0x00);
        }
        blob.extend_from_slice(&[0x44, 0x78, 0x44, 0x78, 0x44, 0x78]);
        let codec = TextCodec::Latin { allow_high: true };
        let m = mes_at(&blob, Some(0), 0, codec).expect("records blob");
        assert_eq!(m.format, "records");
        assert_eq!(m.messages.len(), 2, "{:?}", m.messages);
        assert!(m.messages[0].ends_with("Hello there, friend."));
        assert!(m.messages[1].ends_with("The second récord is here."));
    }
}
