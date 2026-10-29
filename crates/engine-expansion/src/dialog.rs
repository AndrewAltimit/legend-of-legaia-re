//! Pack dialogue on the faithful dialogue path.
//!
//! A pack line ([`legaia_expansion_format::dialog::DialogLine`]) compiles to
//! the inline interaction-script bytes the field VM's dialogue runner reads
//! for an NPC talk (`World::start_inline_dialogue`), so a pack line types,
//! pages and opens its menu through the same pager as a retail one:
//!
//! ```text
//! page    = row+                       row = 0x1F <encoded markup> 0x00
//! text    = page (0x24 page)*          0x24: next page, same box
//! line    = text 0x21                  no menu: the talk ends
//!         | text <open> j0..jN-1 label*N branch*N
//!   open  = 0x27 / 0x28 / 0x29         two / three / four options
//!   jK    = i16 LE, relative to jK's own offset, to branch K
//!   label = 0x1F <encoded label> 0x00  immediate-labels form
//!   branch= [text] 0x21                the option's reply, then the end
//! ```
//!
//! The bytecode carries no field-VM flag op: a choice's pack-flag effects are
//! applied by the `World` after the talk ends, from the choice the runner
//! recorded ([`DialogRuntime`]), so a pack line can never write a retail
//! story flag.

use std::sync::Arc;

use legaia_expansion_format::dialog::{
    DialogLine, MAX_CHOICES, MIN_CHOICES, MarkupIssue, encode_markup, pages,
};

/// Why a line does not compile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogError {
    /// No line with that id.
    Unknown(String),
    /// The line, or a choice's reply, has no text where text is required.
    Empty,
    /// A menu with fewer than two or more than four options.
    ChoiceCount(usize),
    /// Text that has no bytes in a dialog segment: `(where, issues)`.
    Markup(String, Vec<MarkupIssue>),
}

fn push_row(out: &mut Vec<u8>, row: &str, at: &str) -> Result<(), DialogError> {
    let bytes = encode_markup(row).map_err(|e| DialogError::Markup(at.to_string(), e))?;
    out.push(0x1F);
    out.extend_from_slice(&bytes);
    out.push(0x00);
    Ok(())
}

fn push_text(out: &mut Vec<u8>, text: &str, at: &str) -> Result<bool, DialogError> {
    let pages = pages(text);
    for (i, page) in pages.iter().enumerate() {
        if i > 0 {
            out.push(0x24);
        }
        for row in page {
            push_row(out, row, at)?;
        }
    }
    Ok(!pages.is_empty())
}

/// Compile `line` to inline interaction-script bytes (see the module doc).
pub fn compile_line(line: &DialogLine) -> Result<Vec<u8>, DialogError> {
    let mut out = Vec::new();
    if !push_text(&mut out, &line.text, "text")? {
        return Err(DialogError::Empty);
    }
    if line.choices.is_empty() {
        out.push(0x21);
        return Ok(out);
    }
    let n = line.choices.len();
    if !(MIN_CHOICES..=MAX_CHOICES).contains(&n) {
        return Err(DialogError::ChoiceCount(n));
    }
    out.push(0x25 + n as u8); // 0x27 / 0x28 / 0x29
    let table = out.len();
    out.resize(table + n * 2, 0);
    for (k, c) in line.choices.iter().enumerate() {
        push_row(&mut out, &c.label, &format!("choices[{k}].label"))?;
    }
    for (k, c) in line.choices.iter().enumerate() {
        let entry = table + k * 2;
        let rel = i16::try_from(out.len() - entry).map_err(|_| DialogError::Empty)?;
        out[entry..entry + 2].copy_from_slice(&rel.to_le_bytes());
        if let Some(reply) = &c.reply {
            push_text(&mut out, reply, &format!("choices[{k}].reply"))?;
        }
        out.push(0x21);
    }
    Ok(out)
}

/// The pack line on screen, if any.
#[derive(Debug, Clone, Default)]
pub struct DialogRuntime {
    pub pending: Option<PendingLine>,
}

/// A pack line the `World` started.
#[derive(Debug, Clone)]
pub struct PendingLine {
    /// The line id.
    pub line: String,
    /// The compiled bytes the runner was started on; identifies the run.
    pub bytes: Arc<Vec<u8>>,
    /// The option the player committed, once they have.
    pub choice: Option<usize>,
}

impl DialogRuntime {
    /// Record that `line`'s compiled `bytes` are running.
    pub fn begin(&mut self, line: &str, bytes: Arc<Vec<u8>>) {
        self.pending = Some(PendingLine {
            line: line.to_string(),
            bytes,
            choice: None,
        });
    }

    /// The runner on `bytes` committed `choice`. Ignored unless `bytes` is
    /// the pending line's run.
    pub fn observe_choice(&mut self, bytes: &Arc<Vec<u8>>, choice: usize) {
        if let Some(p) = self.pending.as_mut()
            && Arc::ptr_eq(&p.bytes, bytes)
        {
            p.choice = Some(choice);
        }
    }

    /// Whether the run on `bytes` is the pending line's.
    pub fn is_pending_run(&self, bytes: &Arc<Vec<u8>>) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|p| Arc::ptr_eq(&p.bytes, bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_expansion_format::dialog::DialogChoice;

    fn line(text: &str) -> DialogLine {
        DialogLine {
            text: text.into(),
            ..Default::default()
        }
    }

    #[test]
    fn a_plain_line_is_rows_pages_and_an_end() {
        let b = compile_line(&line("ab\ncd\n\nef")).unwrap();
        assert_eq!(
            b,
            [
                &[0x1F, b'a', b'b', 0x00, 0x1F, b'c', b'd', 0x00, 0x24][..],
                &[0x1F, b'e', b'f', 0x00, 0x21]
            ]
            .concat()
        );
    }

    #[test]
    fn a_menu_jumps_each_option_to_its_branch() {
        let mut l = line("Go?");
        l.choices = vec![
            DialogChoice {
                label: "Yes".into(),
                reply: Some("OK".into()),
                ..Default::default()
            },
            DialogChoice {
                label: "No".into(),
                ..Default::default()
            },
        ];
        let b = compile_line(&l).unwrap();
        // prompt
        assert_eq!(&b[..6], &[0x1F, b'G', b'o', b'?', 0x00, 0x27]);
        let table = 6;
        let target = |k: usize| {
            let e = table + k * 2;
            (e as isize + i16::from_le_bytes([b[e], b[e + 1]]) as isize) as usize
        };
        // labels follow the table
        assert_eq!(&b[10..15], &[0x1F, b'Y', b'e', b's', 0x00]);
        assert_eq!(&b[15..19], &[0x1F, b'N', b'o', 0x00]);
        assert_eq!(
            &b[target(0)..target(0) + 5],
            &[0x1F, b'O', b'K', 0x00, 0x21]
        );
        assert_eq!(b[target(1)], 0x21);
        assert_eq!(target(1), b.len() - 1);
    }

    #[test]
    fn the_open_byte_follows_the_option_count() {
        for (n, open) in [(2, 0x27), (3, 0x28), (4, 0x29)] {
            let mut l = line("?");
            l.choices = vec![DialogChoice::default(); n];
            l.choices.iter_mut().for_each(|c| c.label = "x".into());
            assert_eq!(compile_line(&l).unwrap()[3], open);
        }
        let mut l = line("?");
        l.choices = vec![DialogChoice::default(); 5];
        assert_eq!(compile_line(&l), Err(DialogError::ChoiceCount(5)));
    }

    #[test]
    fn bad_text_is_reported_where_it_is() {
        assert_eq!(compile_line(&line("\n\n")), Err(DialogError::Empty));
        match compile_line(&line("caf\u{00E9}")) {
            Err(DialogError::Markup(at, issues)) => {
                assert_eq!(at, "text");
                assert_eq!(issues.len(), 1);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_choice_is_recorded_only_for_its_own_run() {
        let mut rt = DialogRuntime::default();
        let mine = Arc::new(vec![0x21]);
        let other = Arc::new(vec![0x21]);
        rt.begin("l", mine.clone());
        rt.observe_choice(&other, 1);
        assert_eq!(rt.pending.as_ref().unwrap().choice, None);
        rt.observe_choice(&mine, 1);
        assert_eq!(rt.pending.as_ref().unwrap().choice, Some(1));
        assert!(rt.is_pending_run(&mine) && !rt.is_pending_run(&other));
    }
}
