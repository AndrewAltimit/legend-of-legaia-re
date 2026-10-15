//! `OwnedDialogPanel` page loading: seeding the box, decoding rows and lines, and rebuilding a page after a substitution changes.

use super::*;

impl OwnedDialogPanel {
    /// Seed the typewriter on the pager page whose first `0x1F` lead is at
    /// `lead`, in a fresh window: PC on row 0's glyphs, and the picker
    /// attached only when the page's own post-page dispatch is a menu-open
    /// byte (the faithful "is this box a menu?" test - the picker open byte
    /// sits at [`legaia_mes::DialogBox::dispatch_at`], directly after the
    /// page's *last* row, so a 2-row prompt still finds its menu). Falls back
    /// to the ungrouped single-segment walk when `lead` isn't a `0x1F` byte.
    pub(super) fn seed_box_at_lead(&mut self, lead: usize) {
        self.pending_box_advance = false;
        self.press_latch = false;
        self.menu_pending = false;
        self.window = None;
        self.page_rows.clear();
        match legaia_mes::pack_page(&self.bytes, lead) {
            Some(bx) => self.load_page(bx, None),
            None => {
                self.pc = lead + 1;
                self.picker = Self::picker_following_segment(&self.bytes, lead + 1);
                self.current_box = None;
            }
        }
    }

    /// Make `bx` the current page: decode its rows and either open a fresh
    /// window on it (`keep_rows = None`) or queue a page turn onto it
    /// (`Some(true)` for the `0x24` continuation that keeps the window,
    /// `Some(false)` for a fresh box).
    pub(super) fn load_page(&mut self, bx: legaia_mes::DialogBox, keep_rows: Option<bool>) {
        self.pc = bx.lines[0].start;
        self.picker = if matches!(bx.dispatch, legaia_mes::Dispatch::Picker(_)) {
            legaia_mes::scan_pickers(&self.bytes)
                .into_iter()
                .find(|p| p.open == bx.dispatch_at)
        } else {
            None
        };
        self.decoded_subs = self.subs_key();
        let rows = Arc::new(self.decode_page(&bx));
        let n = rows.len();
        match (self.window.as_mut(), keep_rows) {
            (Some(w), Some(keep)) => w.turn_page(n, keep),
            _ => {
                self.page_rows.clear();
                self.window = Some(crate::dialog_window::RowWindow::open(n));
            }
        }
        if let Some(w) = self.window.as_mut() {
            w.capacity = self.window_rows;
            w.instant |= std::mem::take(&mut self.window_instant);
        }
        let serial = self.window.as_ref().map_or(0, |w| w.page());
        self.page_rows.retain(|(s, _)| s + 1 >= serial);
        self.page_rows.push((serial, rows));
        self.current_box = Some(bx);
        self.rebuild_page();
    }

    /// Decode every line of `bx` (glyphs, their reveal units, the line's
    /// count), carrying the pen colour from line to line.
    fn decode_page(&self, bx: &legaia_mes::DialogBox) -> Vec<DecodedRow> {
        bx.lines
            .iter()
            .map(|l| {
                let lead = l.start.saturating_sub(1);
                DecodedRow {
                    glyphs: self.decode_line(l.start),
                    count: self.line_count(lead),
                    lead,
                }
            })
            .collect()
    }

    /// The glyphs of the line whose glyph run starts at `start`, each with the
    /// reveal units before it: one unit per glyph, per spliced substitution
    /// glyph and per `0xCE` escape, none for a `0xCF` pair (the draw's count,
    /// `FUN_80036044`). The line ends at its terminator, or at a mid-line
    /// `0x80..=0x9F` byte the pager's `(b & 0x7F) < 0x20` test also stops on.
    fn decode_line(&self, start: usize) -> Vec<(PanelGlyph, u32)> {
        let clut = self.current_clut;
        let glyph = |byte: u8| PanelGlyph { byte, clut };
        let mut out = Vec::new();
        let mut units = 0u32;
        let mut interp = Interpreter::new_at(&self.bytes, start);
        loop {
            match interp.next_event() {
                Some(MesEvent::EndOfMessage(_)) | Some(MesEvent::Control(_)) | None => break,
                Some(MesEvent::Glyph(g)) | Some(MesEvent::WideGlyph(_, g)) => {
                    out.push((glyph(g), units));
                    units += 1;
                }
                Some(MesEvent::SkipTwo(arg)) => out.push((glyph(arg), units)),
                Some(MesEvent::Substitute { kind, arg }) => {
                    if let Some(name) = self
                        .substitutions
                        .as_ref()
                        .and_then(|subs| subs.get(&(substitute_kind_key(kind), arg)))
                    {
                        for &b in name {
                            out.push((glyph(b), units));
                            units += 1;
                        }
                    }
                }
                // A number escape prints the script counter the host
                // resolved at open; the whole number is one reveal unit, like
                // the escape it replaces (`FUN_80036044` counts the `0xCE`
                // pair once). Any other `0xCE` escape is one typewriter unit
                // and, when the font has its sprite, one drawn icon
                // (`FUN_80036888`): keep the pair in the page so the layout
                // can place it.
                Some(MesEvent::Spacing(arg)) => {
                    if let Some(digits) = self
                        .substitutions
                        .as_ref()
                        .and_then(|subs| subs.get(&(SCRIPT_COUNTER_KEY, arg)))
                    {
                        for &b in digits {
                            out.push((glyph(b), units));
                        }
                    } else {
                        out.push((glyph(0xCE), units));
                        out.push((glyph(arg), units));
                    }
                    units += 1;
                }
                Some(MesEvent::Truncated(_)) => {}
            }
        }
        out
    }

    fn subs_key(&self) -> Option<usize> {
        self.substitutions
            .as_ref()
            .map(|a| Arc::as_ptr(a) as *const () as usize)
    }

    /// Re-decode the held pages if [`Self::substitutions`] changed since
    /// they were decoded (a name splices in, and its units count).
    pub(super) fn refresh_decode(&mut self) {
        let key = self.subs_key();
        if key == self.decoded_subs {
            return;
        }
        self.decoded_subs = key;
        let pages = std::mem::take(&mut self.page_rows);
        self.page_rows = pages
            .into_iter()
            .map(|(serial, rows)| {
                let fresh = rows
                    .iter()
                    .map(|r| DecodedRow {
                        glyphs: self.decode_line(r.lead + 1),
                        count: self.line_count(r.lead),
                        lead: r.lead,
                    })
                    .collect();
                (serial, Arc::new(fresh))
            })
            .collect();
        self.rebuild_page();
    }

    /// Rebuild [`Self::page`] from the window: every row whole except the
    /// typing one, which shows the glyphs the reveal counter has passed;
    /// rows joined by the `0x7C` newline glyph.
    pub(super) fn rebuild_page(&mut self) {
        let Some(w) = self.window.as_ref() else {
            return;
        };
        let typing = w.typing_row();
        let mut page = Vec::new();
        for (i, r) in w.rows.iter().enumerate() {
            if i > 0 {
                page.push(PanelGlyph {
                    byte: legaia_font::NEWLINE,
                    clut: self.current_clut,
                });
            }
            let Some(row) = self
                .page_rows
                .iter()
                .find(|(s, _)| *s == r.page)
                .and_then(|(_, rows)| rows.get(r.line))
            else {
                continue;
            };
            let cap = match typing {
                Some((t, units)) if t == *r => units,
                _ => u32::MAX,
            };
            page.extend(
                row.glyphs
                    .iter()
                    .filter(|(_, before)| *before < cap)
                    .map(|(g, _)| *g),
            );
        }
        self.page = page;
    }

    /// PCs of the current box's `0x1F` row leads. Runners that keep a
    /// visited/wrap map mark these so a conversation tail that jumps back onto
    /// row 2/3 of an already-shown box still reads as a wrap.
    pub fn row_leads(&self) -> Vec<usize> {
        self.current_box
            .as_ref()
            .map(|b| b.lines.iter().map(|l| l.start - 1).collect())
            .unwrap_or_default()
    }

    /// End index of the `0x1F` text segment whose glyph run starts at `from`
    /// (where the standard MES interpreter halts).
    fn segment_end(bytes: &[u8], from: usize) -> usize {
        let mut interp = Interpreter::new_at(bytes, from);
        loop {
            match interp.next_event() {
                Some(MesEvent::EndOfMessage(_)) | None => break interp.pc(),
                _ => {}
            }
        }
    }

    /// The option-picker (if any) that sits immediately after the text segment
    /// whose glyph run starts at `seg_start` - i.e. whose open byte equals the
    /// segment's end index. This is the faithful "is this box a menu?" test:
    /// the picker open byte directly follows the box's last typed line.
    fn picker_following_segment(bytes: &[u8], seg_start: usize) -> Option<legaia_mes::Picker> {
        let end = Self::segment_end(bytes, seg_start);
        legaia_mes::scan_pickers(bytes)
            .into_iter()
            .find(|p| p.open == end)
    }
}
