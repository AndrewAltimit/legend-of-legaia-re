use super::*;

/// Iterator that linearly walks `bytecode` starting at `start_pc`, yielding
/// one `Result<Insn, (usize, DisasmError)>` per source-encoded instruction.
///
/// On error the iterator advances by one byte and continues, so a single
/// truncated / unknown instruction doesn't kill the whole walk.
pub struct LinearWalker<'a> {
    bytecode: &'a [u8],
    pc: usize,
}

impl<'a> LinearWalker<'a> {
    pub fn new(bytecode: &'a [u8], start_pc: usize) -> Self {
        Self {
            bytecode,
            pc: start_pc,
        }
    }
}

impl Iterator for LinearWalker<'_> {
    type Item = Result<Insn, (usize, DisasmError)>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pc >= self.bytecode.len() {
            return None;
        }
        match decode(self.bytecode, self.pc) {
            Ok(insn) => {
                let next = self.pc + insn.size.max(1);
                self.pc = next;
                Some(Ok(insn))
            }
            Err(err) => {
                let pc = self.pc;
                self.pc += 1;
                Some(Err((pc, err)))
            }
        }
    }
}
