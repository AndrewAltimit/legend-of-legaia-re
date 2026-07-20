//! SCUS core helpers: the actor node pool and the pre-increment u16 list
//! append.
//!
//! PORT: FUN_800203EC, FUN_80020424, FUN_80020454, FUN_800204A4, FUN_8001FA68
//!
//! Five small leaf routines in `SCUS_942.54`. Four of them form the
//! per-scene **actor node pool**: a LIFO free-stack over a fixed array of
//! fixed-stride nodes, plus the intrusive list primitives that link a
//! popped node onto a list head. The fifth is an unrelated list-append
//! helper the sprite path uses.
//!
//! Every claim below is read out of the instruction stream, cross-checked
//! against `extracted/SCUS_942.54` at `0x800 + va - 0x80010000`. The
//! decompiled C in the reference dumps is not the source.
//!
//! ## Clean-room boundary
//!
//! No `SCUS_942.54` bytes live in this crate. The reference dumps
//! (`ghidra/scripts/funcs/800203ec.txt`, `80020424.txt`, `80020454.txt`,
//! `800204a4.txt`, `8001fa68.txt`) are the *spec*.
//!
//! REF: FUN_80020DE0, FUN_8002519C, FUN_8003F3FC
//!
//! # NOT WIRED
//!
//! [`ActorNodePool`] has no caller in the engine. The port's actor storage
//! is a `Vec`-backed pool with generational slots, not a retail-shaped
//! free-stack, so nothing in `engine-core` allocates through this type
//! today. It is ported because the retail *allocation order* is
//! observable: the free-stack hands out the highest node index first and
//! descends, and a freed node returns to the top of the stack, so a
//! spawn/despawn sequence reproduces a specific node ordering. That
//! ordering feeds actor iteration order, which the recomp differential
//! oracle compares per frame. Wiring it means replacing the engine's actor
//! pool wholesale - out of scope here.
//!
//! [`list_append_u16`] is likewise uncalled: its retail caller
//! `FUN_8003F3FC` (the sprite placement/clip routine) is not ported.

// ---------------------------------------------------------------------------
// Retail memory layout
// ---------------------------------------------------------------------------

/// Number of nodes the pool holds.
///
/// `FUN_800203EC` opens `addiu a0, zero, 0x8e` and loops while `a0 >= 0`
/// (`bgez`), so the body runs for `a0 = 0x8e ..= 0` - 143 iterations, one
/// stack slot each.
pub const POOL_NODES: usize = 0x8E + 1;

/// Byte stride between consecutive nodes.
///
/// The init loop walks the node pointer down by `addiu v1, v1, -0xd8` per
/// slot.
pub const NODE_STRIDE: u32 = 0xD8;

/// Retail VA of node 0.
///
/// `FUN_800203EC` seeds `v1 = (0x80080000 - 0x3a54) + 0x77d0 = 0x80083D7C`
/// and writes it into the *highest* stack slot, then decrements by
/// [`NODE_STRIDE`] per slot. Node 0 therefore sits at
/// `0x80083D7C - 0x8E * 0xD8`.
pub const POOL_BASE_VA: u32 = 0x80083D7C - (0x8E * NODE_STRIDE);

/// Retail VA of the free-stack array (slot 0).
///
/// `FUN_80020424` addresses a slot as `0x8007C348 + idx*4 + 0x28`.
pub const STACK_BASE_VA: u32 = 0x8007C348 + 0x28;

/// Retail VA of the free-stack pointer, `_DAT_8007C348`.
///
/// Holds the index of the *topmost occupied* slot, not a count: init
/// stores `0x8E`, and a pop reads slot `sp` before decrementing.
pub const STACK_POINTER_VA: u32 = 0x8007C348;

/// Retail VA of node `index`, for cross-checking a model against a live
/// RAM capture.
pub fn node_va(index: usize) -> u32 {
    POOL_BASE_VA + (index as u32) * NODE_STRIDE
}

// ---------------------------------------------------------------------------
// Node model
// ---------------------------------------------------------------------------

/// A node's four link words, as the four routines below use them.
///
/// Byte offsets within the `0xD8`-byte node, taken from the stores:
///
/// | Offset | Field | Written by |
/// |---|---|---|
/// | `+0x00` | [`next`](NodeLinks::next) | `FUN_80020424`, `FUN_80020454`, `FUN_800204A4` |
/// | `+0x04` | [`prev`](NodeLinks::prev) | `FUN_80020424`, `FUN_80020454`, `FUN_800204A4` |
/// | `+0x08` | [`owner`](NodeLinks::owner) | `FUN_80020454` |
/// | `+0x0C` | [`tail`](NodeLinks::tail) | `FUN_80020424`, `FUN_80020454`, `FUN_800204A4` |
///
/// The remaining `0xC8` bytes of the node are payload these five routines
/// never touch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NodeLinks {
    /// `+0x00`. Successor, or `None` for the retail null (`sw zero`).
    pub next: Option<usize>,
    /// `+0x04`. Predecessor. A list head points at itself.
    pub prev: Option<usize>,
    /// `+0x08`. The list head this node was appended to. Only
    /// `FUN_80020454` writes it and only `FUN_800204A4` reads it.
    pub owner: Option<usize>,
    /// `+0x0C`. On a list head, the last member (itself when empty).
    pub tail: Option<usize>,
}

/// The per-scene actor node pool: a LIFO free-stack over [`POOL_NODES`]
/// fixed-stride nodes.
#[derive(Debug, Clone)]
pub struct ActorNodePool {
    /// Free-stack contents. `stack[i]` is the node index the retail code
    /// stores at `STACK_BASE_VA + i*4`.
    stack: [usize; POOL_NODES],
    /// `_DAT_8007C348`. Index of the topmost occupied slot; `-1` means
    /// empty. Signed because `FUN_80020454` tests it with `bltz`.
    sp: i32,
    /// Link words for every node.
    nodes: [NodeLinks; POOL_NODES],
}

impl Default for ActorNodePool {
    fn default() -> Self {
        Self::new()
    }
}

impl ActorNodePool {
    /// Build the free-stack and set the stack pointer - `FUN_800203EC`.
    ///
    /// PORT: FUN_800203EC
    ///
    /// The retail loop writes the highest node VA into the highest slot
    /// and walks both down together, so `stack[i] == i` in node-index
    /// terms. It finishes with `_DAT_8007C348 = 0x8E`, i.e. every node
    /// free and the top of the stack holding the *last* node.
    ///
    /// The link words are **not** initialised here - the retail body only
    /// writes the stack array. A node's links are set when it is popped.
    pub fn new() -> Self {
        let mut stack = [0usize; POOL_NODES];
        for (i, slot) in stack.iter_mut().enumerate() {
            *slot = i;
        }
        Self {
            stack,
            sp: 0x8E,
            nodes: [NodeLinks::default(); POOL_NODES],
        }
    }

    /// Current free-stack pointer (`_DAT_8007C348`). `-1` = exhausted.
    pub fn stack_pointer(&self) -> i32 {
        self.sp
    }

    /// Number of nodes still on the free-stack.
    pub fn free_count(&self) -> usize {
        (self.sp + 1).max(0) as usize
    }

    /// Link words of `node`.
    pub fn links(&self, node: usize) -> NodeLinks {
        self.nodes[node]
    }

    /// Pop a node and initialise it as an empty list head -
    /// `FUN_80020424`.
    ///
    /// PORT: FUN_80020424
    ///
    /// Stores, in instruction order: `next = 0`, `prev = self`,
    /// `tail = self`. Note `next` is null while `prev` and `tail` are
    /// self-referential - the head is *not* a fully circular sentinel, and
    /// `owner` is left untouched.
    ///
    /// Retail performs **no** bounds check: it reads `stack[sp]` before
    /// testing anything and lets `sp` go negative, unlike its sibling
    /// [`alloc_and_append`](Self::alloc_and_append) which does `bltz`
    /// first. This port returns `None` on an exhausted stack rather than
    /// reproducing the out-of-bounds read.
    pub fn alloc_list_head(&mut self) -> Option<usize> {
        if self.sp < 0 {
            return None;
        }
        let node = self.stack[self.sp as usize];
        self.sp -= 1;
        self.nodes[node] = NodeLinks {
            next: None,
            prev: Some(node),
            owner: self.nodes[node].owner,
            tail: Some(node),
        };
        Some(node)
    }

    /// Pop a node and append it to the list headed by `head` -
    /// `FUN_80020454`.
    ///
    /// PORT: FUN_80020454
    ///
    /// Returns `None` when the free-stack is empty; retail returns 0
    /// through the `bltz` early exit at `0x8002049C`, which is the one
    /// bounds check in the cluster.
    ///
    /// The link-up reads the head's tail first, then stores in this
    /// order: `tail.next = new`, `new.owner = head`, `new.next = 0`,
    /// `new.prev = tail`, `head.tail = new`. Because
    /// [`alloc_list_head`](Self::alloc_list_head) leaves `head.tail =
    /// head`, the first append writes `head.next = new`, so a head's
    /// `next` is the first member.
    pub fn alloc_and_append(&mut self, head: usize) -> Option<usize> {
        if self.sp < 0 {
            return None;
        }
        let node = self.stack[self.sp as usize];
        self.sp -= 1;
        let tail = self.nodes[head].tail;
        if let Some(t) = tail {
            self.nodes[t].next = Some(node);
        }
        self.nodes[node].owner = Some(head);
        self.nodes[node].next = None;
        self.nodes[node].prev = tail;
        self.nodes[head].tail = Some(node);
        Some(node)
    }

    /// Unlink `node` and push it back onto the free-stack -
    /// `FUN_800204A4`.
    ///
    /// PORT: FUN_800204A4
    ///
    /// The push happens **first**, before any link is read: retail
    /// increments `_DAT_8007C348` and stores the node pointer, then
    /// unlinks. There is no check that the node was allocated, and no
    /// check that `sp` stays inside the array - freeing more nodes than
    /// were allocated walks the stack past its end. This port returns
    /// `false` in that case instead.
    ///
    /// The unlink branches on `next`:
    ///
    /// - `next == 0` (node is the tail): `owner.tail = prev` and
    ///   `prev.next = 0`. This is the only read of `owner` in the
    ///   cluster.
    /// - otherwise: `next.prev = prev` and `prev.next = next`.
    ///
    /// Note the tail branch reaches the list head through `owner`, while
    /// the interior branch never needs it - which is why
    /// [`alloc_list_head`](Self::alloc_list_head) can leave `owner`
    /// unwritten on a head that is never itself freed.
    pub fn free(&mut self, node: usize) -> bool {
        let next_sp = self.sp + 1;
        if next_sp as usize >= POOL_NODES {
            return false;
        }
        self.sp = next_sp;
        self.stack[next_sp as usize] = node;

        let next = self.nodes[node].next;
        let prev = self.nodes[node].prev;
        match next {
            None => {
                if let Some(owner) = self.nodes[node].owner {
                    self.nodes[owner].tail = prev;
                }
                if let Some(p) = prev {
                    self.nodes[p].next = None;
                }
            }
            Some(n) => {
                self.nodes[n].prev = prev;
                if let Some(p) = prev {
                    self.nodes[p].next = Some(n);
                }
            }
        }
        true
    }

    /// Walk a list head's members in link order.
    ///
    /// Not a retail routine - a convenience for tests and for engines
    /// inspecting a pool built through the ported primitives.
    pub fn members(&self, head: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut cur = self.nodes[head].next;
        while let Some(n) = cur {
            out.push(n);
            cur = self.nodes[n].next;
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Pre-increment u16 list append
// ---------------------------------------------------------------------------

/// Append `value` to a u16 list whose element count sits in a separate
/// `i16` - `FUN_8001FA68`.
///
/// PORT: FUN_8001FA68
///
/// The eight-instruction body is:
///
/// ```text
/// lh   v0, 0(a0)     ; count
/// addiu v0, v0, 1    ; pre-increment
/// sh   v0, 0(a0)     ; store it back
/// sll  v0, v0, 1     ; count * 2
/// addu v0, v0, a1
/// sh   a3, 0(v0)     ; entries[new_count] = value
/// ```
///
/// Two things the C rendering does not show:
///
/// - The store indexes at the **new** count, so with a count starting at
///   `0` the first append lands in `entries[1]` and slot `0` is never
///   written. Callers that want a dense list start the count at `-1`.
/// - `a2` is never read. The one traced caller, `FUN_8003F3FC` at
///   `0x8003F7FC`, loads `a2` from `lh 0x2(a0)` - the list's capacity -
///   immediately before the `jal`, and the callee ignores it. Retail
///   therefore performs **no** bounds check on this append. Ghidra's
///   inferred signature drops the argument entirely, which is the
///   "dropped register arguments" artifact; the `jal` and its
///   surroundings are what settle it.
///
/// This port cannot reproduce the unchecked store, so it returns the
/// written index, or `None` when the new count falls outside `entries`.
/// In that case the count is still incremented, matching retail's
/// unconditional `sh v0, 0(a0)`.
pub fn list_append_u16(count: &mut i16, entries: &mut [u16], value: u16) -> Option<usize> {
    *count = count.wrapping_add(1);
    let idx = *count;
    if idx < 0 {
        return None;
    }
    let idx = idx as usize;
    match entries.get_mut(idx) {
        Some(slot) => {
            *slot = value;
            Some(idx)
        }
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_builds_a_full_descending_free_stack() {
        let pool = ActorNodePool::new();
        assert_eq!(pool.stack_pointer(), 0x8E);
        assert_eq!(pool.free_count(), POOL_NODES);
        assert_eq!(POOL_NODES, 143);
    }

    #[test]
    fn retail_node_addresses_match_the_init_loop_seed() {
        // FUN_800203EC seeds the top slot with 0x80083D7C and steps down
        // by 0xD8. Node 0x8E must land back on the seed, node 0 on the
        // pool base.
        assert_eq!(node_va(0x8E), 0x80083D7C);
        assert_eq!(node_va(0), POOL_BASE_VA);
        assert_eq!(node_va(1) - node_va(0), NODE_STRIDE);
        // The free-stack array sits at 0x8007C348 + 0x28.
        assert_eq!(STACK_BASE_VA, 0x8007C370);
        assert_eq!(STACK_POINTER_VA, 0x8007C348);
    }

    #[test]
    fn allocation_hands_out_the_highest_node_first() {
        let mut pool = ActorNodePool::new();
        assert_eq!(pool.alloc_list_head(), Some(0x8E));
        assert_eq!(pool.alloc_list_head(), Some(0x8D));
        assert_eq!(pool.stack_pointer(), 0x8C);
    }

    #[test]
    fn list_head_init_stores_null_next_and_self_prev_and_tail() {
        let mut pool = ActorNodePool::new();
        let head = pool.alloc_list_head().unwrap();
        let l = pool.links(head);
        assert_eq!(l.next, None);
        assert_eq!(l.prev, Some(head));
        assert_eq!(l.tail, Some(head));
        assert_eq!(l.owner, None, "FUN_80020424 never writes +0x08");
    }

    #[test]
    fn first_append_becomes_the_heads_next() {
        let mut pool = ActorNodePool::new();
        let head = pool.alloc_list_head().unwrap();
        let a = pool.alloc_and_append(head).unwrap();
        assert_eq!(pool.links(head).next, Some(a));
        assert_eq!(pool.links(head).tail, Some(a));
        assert_eq!(pool.links(a).prev, Some(head));
        assert_eq!(pool.links(a).next, None);
        assert_eq!(pool.links(a).owner, Some(head));
        assert_eq!(pool.members(head), vec![a]);
    }

    #[test]
    fn appends_keep_link_order_and_tail() {
        let mut pool = ActorNodePool::new();
        let head = pool.alloc_list_head().unwrap();
        let a = pool.alloc_and_append(head).unwrap();
        let b = pool.alloc_and_append(head).unwrap();
        let c = pool.alloc_and_append(head).unwrap();
        assert_eq!(pool.members(head), vec![a, b, c]);
        assert_eq!(pool.links(head).tail, Some(c));
        assert_eq!(pool.links(b).prev, Some(a));
        assert_eq!(pool.links(c).prev, Some(b));
    }

    #[test]
    fn freeing_an_interior_node_splices_it_out() {
        let mut pool = ActorNodePool::new();
        let head = pool.alloc_list_head().unwrap();
        let a = pool.alloc_and_append(head).unwrap();
        let b = pool.alloc_and_append(head).unwrap();
        let c = pool.alloc_and_append(head).unwrap();
        assert!(pool.free(b));
        assert_eq!(pool.members(head), vec![a, c]);
        assert_eq!(pool.links(c).prev, Some(a));
        // The tail is untouched: the interior branch never reads owner.
        assert_eq!(pool.links(head).tail, Some(c));
    }

    #[test]
    fn freeing_the_tail_walks_back_through_owner() {
        let mut pool = ActorNodePool::new();
        let head = pool.alloc_list_head().unwrap();
        let a = pool.alloc_and_append(head).unwrap();
        let b = pool.alloc_and_append(head).unwrap();
        assert!(pool.free(b));
        assert_eq!(pool.members(head), vec![a]);
        assert_eq!(pool.links(head).tail, Some(a));
        assert_eq!(pool.links(a).next, None);
    }

    #[test]
    fn emptying_a_list_restores_the_head_to_its_initial_shape() {
        // The self-consistency check that validates the whole model: an
        // append followed by a free must land back on exactly the state
        // FUN_80020424 leaves behind.
        let mut pool = ActorNodePool::new();
        let head = pool.alloc_list_head().unwrap();
        let initial = pool.links(head);
        let a = pool.alloc_and_append(head).unwrap();
        assert!(pool.free(a));
        assert_eq!(pool.links(head), initial);
        assert_eq!(pool.members(head), Vec::<usize>::new());
    }

    #[test]
    fn free_returns_the_node_to_the_top_of_the_stack() {
        let mut pool = ActorNodePool::new();
        let head = pool.alloc_list_head().unwrap();
        let a = pool.alloc_and_append(head).unwrap();
        let sp_before = pool.stack_pointer();
        assert!(pool.free(a));
        assert_eq!(pool.stack_pointer(), sp_before + 1);
        // LIFO: the very next append gets the node back.
        assert_eq!(pool.alloc_and_append(head), Some(a));
    }

    #[test]
    fn exhausting_the_pool_returns_none_from_the_checked_allocator() {
        let mut pool = ActorNodePool::new();
        let head = pool.alloc_list_head().unwrap();
        // 143 nodes, one spent on the head.
        for _ in 0..(POOL_NODES - 1) {
            assert!(pool.alloc_and_append(head).is_some());
        }
        assert_eq!(pool.stack_pointer(), -1);
        assert_eq!(
            pool.alloc_and_append(head),
            None,
            "FUN_80020454's bltz early exit"
        );
    }

    #[test]
    fn u16_append_pre_increments_so_slot_zero_stays_unwritten() {
        let mut entries = [0u16; 4];
        let mut count: i16 = 0;
        assert_eq!(list_append_u16(&mut count, &mut entries, 0xAAAA), Some(1));
        assert_eq!(count, 1);
        assert_eq!(entries, [0, 0xAAAA, 0, 0]);
    }

    #[test]
    fn u16_append_from_minus_one_fills_densely() {
        let mut entries = [0u16; 3];
        let mut count: i16 = -1;
        assert_eq!(list_append_u16(&mut count, &mut entries, 1), Some(0));
        assert_eq!(list_append_u16(&mut count, &mut entries, 2), Some(1));
        assert_eq!(list_append_u16(&mut count, &mut entries, 3), Some(2));
        assert_eq!(entries, [1, 2, 3]);
        assert_eq!(count, 2);
    }

    #[test]
    fn u16_append_still_increments_the_count_when_it_overruns() {
        // Retail's `sh v0, 0(a0)` is unconditional - the count advances
        // whether or not the store lands in range.
        let mut entries = [0u16; 2];
        let mut count: i16 = 1;
        assert_eq!(list_append_u16(&mut count, &mut entries, 9), None);
        assert_eq!(count, 2);
        assert_eq!(entries, [0, 0]);
    }
}
