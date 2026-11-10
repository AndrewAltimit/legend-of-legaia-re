//! The actor node pool of `SCUS_942.54` against
//! `legaia_engine_vm::actor_node_pool::ActorNodePool`, as a stream of mixed
//! operations: the free-stack build `FUN_800203EC`, the list-head pop
//! `FUN_80020424`, the pop-and-append `FUN_80020454` and the unlink-and-push
//! `FUN_800204A4`. After every operation the stack pointer, the node handed
//! out and every node's four link words have to match.

use crate::common::{Rng, scus_cpu};
use legaia_engine_vm::actor_node_pool::{
    ActorNodePool, POOL_BASE_VA, POOL_NODES, STACK_POINTER_VA, node_va,
};
use psx_retail_exec::{Cpu, Stop, V0};

const INIT: u32 = 0x8002_03EC;
const ALLOC_HEAD: u32 = 0x8002_0424;
const ALLOC_APPEND: u32 = 0x8002_0454;
const FREE: u32 = 0x8002_04A4;

fn call(cpu: &mut Cpu, entry: u32, args: &[u32]) -> u32 {
    cpu.regs.r[V0] = 0xDEAD_BEEF;
    assert_eq!(cpu.call(entry, args), Stop::Returned, "{entry:#x} {args:?}");
    cpu.regs.r[V0]
}

fn va(node: Option<usize>) -> u32 {
    node.map_or(0, node_va)
}

#[test]
fn the_node_pool_behaves_as_retail_does() {
    differential!(scus(INIT), scus(ALLOC_HEAD), scus(ALLOC_APPEND), scus(FREE));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4E4F_4445);
    let (mut appends, mut tail_frees, mut inner_frees, mut exhausted) = (0, 0, 0, 0);
    for session in 0..60 {
        // Clear link words: the port's `None` is retail's null.
        for i in 0..POOL_NODES {
            for k in 0..4 {
                cpu.mem.wr32(node_va(i) + k * 4, 0);
            }
        }
        call(&mut cpu, INIT, &[]);
        let mut port = ActorNodePool::new();
        assert_eq!(cpu.mem.rd32(STACK_POINTER_VA) as i32, port.stack_pointer());
        let mut heads: Vec<usize> = Vec::new();
        let mut members: Vec<usize> = Vec::new();
        for op in 0..600 {
            let what = rng.below(10);
            let label = if heads.is_empty() || what == 0 {
                if port.stack_pointer() < 0 {
                    // Retail reads below the stack here; the port refuses.
                    continue;
                }
                let r = call(&mut cpu, ALLOC_HEAD, &[]);
                let p = port.alloc_list_head().expect("stack not empty");
                assert_eq!(r, node_va(p), "session {session} op {op} head");
                heads.push(p);
                "head"
            } else if what < 6 || members.is_empty() {
                let head = heads[rng.below(heads.len() as u32) as usize];
                let r = call(&mut cpu, ALLOC_APPEND, &[node_va(head)]);
                let p = port.alloc_and_append(head);
                assert_eq!(r, va(p), "session {session} op {op} append");
                match p {
                    Some(n) => {
                        members.push(n);
                        appends += 1;
                    }
                    None => exhausted += 1,
                }
                "append"
            } else {
                let i = rng.below(members.len() as u32) as usize;
                let n = members.swap_remove(i);
                if port.links(n).next.is_none() {
                    tail_frees += 1;
                } else {
                    inner_frees += 1;
                }
                call(&mut cpu, FREE, &[node_va(n)]);
                assert!(port.free(n));
                "free"
            };
            assert_eq!(
                cpu.mem.rd32(STACK_POINTER_VA) as i32,
                port.stack_pointer(),
                "session {session} op {op} {label} sp"
            );
            for i in 0..POOL_NODES {
                let l = port.links(i);
                let at = POOL_BASE_VA + i as u32 * 0xD8;
                let retail = [
                    cpu.mem.rd32(at),
                    cpu.mem.rd32(at + 4),
                    cpu.mem.rd32(at + 8),
                    cpu.mem.rd32(at + 12),
                ];
                assert_eq!(
                    retail,
                    [va(l.next), va(l.prev), va(l.owner), va(l.tail)],
                    "session {session} op {op} {label} node {i}"
                );
            }
        }
    }
    assert!(
        appends > 5000 && tail_frees > 500 && inner_frees > 500 && exhausted > 0,
        "{appends} {tail_frees} {inner_frees} {exhausted}"
    );
}
