//! Branch coverage of the retail routines a test runs: which conditional
//! branches each routine holds, and which directions the run took.
//!
//! Compiled in by the `branch-cov` feature, and off unless
//! `PSX_RETAIL_EXEC_BRANCHES` names a file. When it does, every
//! routine the [`ledger`](crate::ledger) records gets a static walk of its
//! control flow from the entry, over memory as the call found it: fall-through,
//! both arms of each conditional branch, and `j` targets inside the routine:
//! within the extent already reached, or further on with no other routine's
//! frame setup (`addiu sp,sp,-N`) between (a `j` behind the entry or past one
//! is a tail call). Calls
//! are not followed, so a routine's branches are its own. A `jr` through a
//! register other than `ra` is a jump table the walk cannot resolve, so a
//! routine holding one takes every conditional branch in the whole extent the
//! walk reached instead (the compiler keeps the tables in `.rdata` and the
//! arms inside the routine, ahead of its epilogue).
//!
//! While the run executes, each conditional branch notes whether it was taken
//! or fell through, keyed by its address and instruction word (so two overlays
//! at one VA do not mix). When a test's thread ends its table is appended to
//! the file, one line per routine and one per branch:
//!
//! ```text
//! R  test  entry  image-hash  branches  jump-tables  truncated
//! B  test  entry  image-hash  branch-pc  mask      (1 = taken, 2 = fell through)
//! ```
//!
//! No instruction bytes are written, only addresses and a hash.
//! `scripts/ci/branch-coverage.py` aggregates the file across tests.

use crate::Memory;
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::Write;
use std::sync::{Mutex, OnceLock};

/// A walk that reaches this many instructions stops and marks itself
/// truncated.
const WALK_LIMIT: usize = 40_000;

fn file() -> Option<&'static Mutex<std::fs::File>> {
    static FILE: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();
    FILE.get_or_init(|| {
        let path = std::env::var_os("PSX_RETAIL_EXEC_BRANCHES")?;
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()
            .map(Mutex::new)
    })
    .as_ref()
}

/// Whether branch coverage is being recorded: built with the `branch-cov`
/// feature and the variable set. Without the feature this is a constant
/// `false`, so the interpreter's loop carries no trace of the recorder.
pub fn enabled() -> bool {
    cfg!(feature = "branch-cov") && file().is_some()
}

/// Whether `w` is a conditional branch that can go either way: not `b`
/// (`beq r, r`), `blez $0` or `bgez $0`, which always branch.
pub(crate) fn is_conditional(w: u32) -> bool {
    let op = w >> 26;
    let rs = (w >> 21) & 31;
    let rt = (w >> 16) & 31;
    match op {
        4 => rs != rt,
        5 | 7 => true,
        6 => rs != 0,
        1 => match rt {
            0 | 0x10 => true,
            1 | 0x11 => rs != 0,
            _ => false,
        },
        _ => false,
    }
}

struct Routine {
    branches: Vec<(u32, u32)>,
    jump_tables: u32,
    truncated: bool,
}

#[derive(Default)]
struct Table {
    routines: HashMap<(u32, u64), Routine>,
    hits: HashMap<(u32, u32), u8>,
}

impl Drop for Table {
    fn drop(&mut self) {
        if self.routines.is_empty() {
            return;
        }
        let Some(f) = file() else { return };
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("?");
        let mut out = String::new();
        for ((entry, hash), r) in &self.routines {
            out.push_str(&format!(
                "R\t{name}\t{entry:08x}\t{hash:016x}\t{}\t{}\t{}\n",
                r.branches.len(),
                r.jump_tables,
                u8::from(r.truncated)
            ));
            for key in &r.branches {
                let mask = self.hits.get(key).copied().unwrap_or(0);
                out.push_str(&format!(
                    "B\t{name}\t{entry:08x}\t{hash:016x}\t{:08x}\t{mask}\n",
                    key.0
                ));
            }
        }
        if let Ok(mut f) = f.lock() {
            let _ = f.write_all(out.as_bytes());
        }
    }
}

thread_local! {
    static TABLE: RefCell<Table> = RefCell::new(Table::default());
}

fn hash(body: &[u8]) -> u64 {
    body.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Note the routine at `entry` (whose first bytes are `body`), walking its
/// control flow the first time this thread sees it.
pub(crate) fn note_routine(mem: &mut Memory, entry: u32, body: &[u8]) {
    let key = (entry, hash(body));
    if TABLE.with(|t| t.borrow().routines.contains_key(&key)) {
        return;
    }
    let bad = mem.bad.take();
    let routine = walk(mem, entry);
    mem.bad = bad;
    TABLE.with(|t| t.borrow_mut().routines.insert(key, routine));
}

/// Fold one call's branch outcomes into this thread's table.
pub(crate) fn merge_hits(hits: &mut HashMap<(u32, u32), u8>) {
    if hits.is_empty() {
        return;
    }
    TABLE.with(|t| {
        let mut t = t.borrow_mut();
        for (k, m) in hits.drain() {
            *t.hits.entry(k).or_default() |= m;
        }
    });
}

/// `addiu sp,sp,-N`: a routine opening its stack frame.
fn is_frame_setup(w: u32) -> bool {
    w & 0xFFFF_8000 == 0x27BD_8000
}

/// Whether `target` lies after `entry` with no other routine's frame setup in
/// between. The routine's own may sit a few instructions past its entry (the
/// compiler schedules loads ahead of it); the walk has visited that one.
fn ahead_in_routine(mem: &mut Memory, seen: &HashSet<u32>, entry: u32, target: u32) -> bool {
    if target <= entry || (target - entry) as usize > WALK_LIMIT * 4 {
        return false;
    }
    let mut pc = entry.wrapping_add(4);
    while pc <= target {
        let w = mem.rd32(pc);
        if mem.bad.take().is_some() || (is_frame_setup(w) && !seen.contains(&pc)) {
            return false;
        }
        pc = pc.wrapping_add(4);
    }
    true
}

fn walk(mem: &mut Memory, entry: u32) -> Routine {
    let mut seen: HashSet<u32> = HashSet::new();
    let mut branches: BTreeSet<(u32, u32)> = BTreeSet::new();
    let mut jumps: BTreeSet<u32> = BTreeSet::new();
    let mut jump_tables = 0;
    let mut truncated = false;
    let mut hi = entry;
    let mut work = vec![entry];
    loop {
        while let Some(start) = work.pop() {
            let mut pc = start;
            // Instructions left before this path ends: set to 1 when a jump's
            // delay slot is the last thing it runs.
            let mut left: Option<u32> = None;
            loop {
                if left == Some(0) || !seen.insert(pc) {
                    break;
                }
                if seen.len() > WALK_LIMIT {
                    truncated = true;
                    break;
                }
                hi = hi.max(pc);
                let w = mem.rd32(pc);
                if mem.bad.take().is_some() {
                    break;
                }
                let op = w >> 26;
                let btarget = pc
                    .wrapping_add(4)
                    .wrapping_add(((w & 0xFFFF) as u16 as i16 as i32 as u32) << 2);
                let jtarget = (pc.wrapping_add(4) & 0xF000_0000) | ((w & 0x03FF_FFFF) << 2);
                if let Some(n) = left.as_mut() {
                    *n -= 1;
                } else if is_conditional(w) {
                    branches.insert((pc, w));
                    // `bltzal` / `bgezal` branch to a callee, not inside.
                    if !(op == 1 && (w >> 16) & 0x10 != 0) {
                        work.push(btarget);
                    }
                } else if (op == 4 || op == 6 || op == 1) && (w >> 16) & 0x10 == 0 {
                    // An always-taken branch: the delay slot, then the target.
                    work.push(btarget);
                    left = Some(1);
                } else if op == 2 {
                    jumps.insert(jtarget);
                    left = Some(1);
                } else if op == 0 && w & 0x3F == 0x08 {
                    if (w >> 21) & 31 != 31 {
                        jump_tables += 1;
                    }
                    left = Some(1);
                }
                pc = pc.wrapping_add(4);
            }
            if truncated {
                break;
            }
        }
        if truncated {
            break;
        }
        // A `j` inside the reached extent is a label in this routine (the
        // shared-epilogue idiom), and so is one further on with no other
        // routine's frame setup between (a loop rotated to test at the
        // bottom); one behind the entry or past a frame setup is a tail call.
        let inside: Vec<u32> = jumps
            .iter()
            .copied()
            .filter(|&t| {
                !seen.contains(&t)
                    && ((entry..=hi).contains(&t) || ahead_in_routine(mem, &seen, entry, t))
            })
            .collect();
        if inside.is_empty() {
            break;
        }
        work.extend(inside);
    }
    // A jump table's arms are unreachable to the walk but sit inside the
    // routine (the tables themselves are in `.rdata`): sweep the whole extent
    // the walk reached, entry to epilogue, for them. The epilogue is the first
    // `jr ra` whose delay slot reaches that extent: when only the table
    // reaches the code after its `jr`, the walk stops short of the arms. A
    // `jr` that is a tail call through a function pointer has no epilogue
    // after it, so the sweep also ends at the next routine's frame setup
    // (`addiu sp,sp,-N`) past the extent.
    if jump_tables > 0 && !truncated {
        let mut pc = entry;
        loop {
            let w = mem.rd32(pc);
            if mem.bad.take().is_some() || pc.wrapping_sub(entry) as usize > WALK_LIMIT * 4 {
                break;
            }
            if pc > hi && is_frame_setup(w) {
                break;
            }
            if is_conditional(w) {
                branches.insert((pc, w));
            }
            if pc.wrapping_add(4) >= hi && w == 0x03E0_0008 {
                break;
            }
            pc = pc.wrapping_add(4);
        }
    }
    Routine {
        branches: branches.into_iter().collect(),
        jump_tables,
        truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn routine(mem: &mut Memory, base: u32, code: &[u32]) {
        for (i, w) in code.iter().enumerate() {
            mem.wr32(base + i as u32 * 4, *w);
        }
    }

    fn pcs(r: &Routine) -> Vec<u32> {
        r.branches.iter().map(|b| b.0).collect()
    }

    #[test]
    fn always_taken_forms_are_not_conditional() {
        assert!(is_conditional(0x1080_0003)); // beq a0,zero
        assert!(!is_conditional(0x1000_0003)); // b
        assert!(!is_conditional(0x1800_0003)); // blez zero
        assert!(!is_conditional(0x0401_0003)); // bgez zero
        assert!(is_conditional(0x1CA0_0001)); // bgtz a1
    }

    /// Both arms, a `j` back inside the routine, the epilogue; dead code past
    /// `jr ra` is not the routine's.
    #[test]
    fn the_walk_takes_the_routines_own_branches() {
        let mut mem = Memory::default();
        let base = 0x8001_0000;
        routine(
            &mut mem,
            base,
            &[
                0x1080_0003, // 00 beq a0,zero,0x10
                0x0000_0000, // 04
                0x0800_4006, // 08 j 0x80010018
                0x0000_0000, // 0C
                0x1CA0_0001, // 10 bgtz a1,0x18
                0x0000_0000, // 14
                0x03E0_0008, // 18 jr ra
                0x0000_0000, // 1C
                0x14C0_0000, // 20 bne a2,zero (past the epilogue)
            ],
        );
        let r = walk(&mut mem, base);
        assert_eq!(pcs(&r), [base, base + 0x10]);
        assert_eq!(r.jump_tables, 0);
    }

    /// A loop rotated to test at the bottom: the `j` to the test lands past
    /// everything reached so far, and is followed. A `j` past the next
    /// routine's frame setup is a tail call and is not.
    #[test]
    fn a_forward_j_stays_in_the_routine_until_a_frame_setup() {
        let mut mem = Memory::default();
        let base = 0x8001_0000;
        routine(
            &mut mem,
            base,
            &[
                0x8C82_0000, // 00 lw v0,0(a0)
                0x0800_4006, // 04 j 0x80010018 (the loop test)
                0x27BD_FFE8, // 08 addiu sp,sp,-0x18 (its own, in the slot)
                0x2484_FFFF, // 0C addiu a0,a0,-1 (the loop body)
                0x0000_0000, // 10
                0x0000_0000, // 14
                0x1480_FFFC, // 18 bne a0,zero,0x0C
                0x0000_0000, // 1C
                0x0800_400B, // 20 j 0x8001002C (tail call)
                0x0000_0000, // 24
                0x27BD_FFE8, // 28 addiu sp,sp,-0x18 (the next routine)
                0x14C0_0000, // 2C bne a2,zero
                0x03E0_0008, // 30 jr ra
                0x0000_0000, // 34
            ],
        );
        let r = walk(&mut mem, base);
        assert_eq!(pcs(&r), [base + 0x18]);
    }

    /// Case arms only the table reaches are swept up to the epilogue.
    #[test]
    fn a_jump_tables_arms_are_counted() {
        let mut mem = Memory::default();
        let base = 0x8001_0000;
        routine(
            &mut mem,
            base,
            &[
                0x0040_0008, // 00 jr v0
                0x0000_0000, // 04
                0x1480_0001, // 08 bne a0,zero,0x10
                0x0000_0000, // 0C
                0x03E0_0008, // 10 jr ra
                0x0000_0000, // 14
                0x14C0_0000, // 18 bne a2,zero (past the epilogue)
            ],
        );
        let r = walk(&mut mem, base);
        assert_eq!(pcs(&r), [base + 8]);
        assert_eq!(r.jump_tables, 1);
    }

    /// A `jr` through a function pointer has no epilogue after it: the
    /// sweep stops at the next routine's frame setup.
    #[test]
    fn a_pointer_tail_call_does_not_sweep_the_next_routine() {
        let mut mem = Memory::default();
        let base = 0x8001_0000;
        routine(
            &mut mem,
            base,
            &[
                0x1080_0001, // 00 beq a0,zero,0x08
                0x0000_0000, // 04
                0x0040_0008, // 08 jr v0
                0x0000_0000, // 0C
                0x27BD_FFE8, // 10 addiu sp,sp,-0x18 (the next routine)
                0x14C0_0000, // 14 bne a2,zero
                0x03E0_0008, // 18 jr ra
                0x0000_0000, // 1C
            ],
        );
        let r = walk(&mut mem, base);
        assert_eq!(pcs(&r), [base]);
    }

    /// The epilogue's delay slot is the extent the walk reached, so the
    /// sweep ends on the `jr ra` one word before it.
    #[test]
    fn the_sweep_ends_at_the_epilogue_the_walk_reached() {
        let mut mem = Memory::default();
        let base = 0x8001_0000;
        routine(
            &mut mem,
            base,
            &[
                0x2C82_0004, // 00 sltiu v0,a0,4
                0x1040_0003, // 04 beq v0,zero,0x14 (default)
                0x0000_0000, // 08
                0x0040_0008, // 0C jr v0
                0x0000_0000, // 10
                0x03E0_0008, // 14 jr ra
                0x0000_0000, // 18
                0x14C0_0000, // 1C bne a2,zero (a frameless neighbour)
            ],
        );
        let r = walk(&mut mem, base);
        assert_eq!(pcs(&r), [base + 4]);
    }
}
