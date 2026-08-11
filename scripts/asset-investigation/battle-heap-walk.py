#!/usr/bin/env python3
"""Walk Legaia's custom 2-pool best-fit heap in a main-RAM image.

Heap structures (FUN_8002b3d4 init / FUN_8002b468 alloc, SCUS_942.54):
  gp = 0x8007B318; descriptor ptr at gp+0x840 = 0x8007BB58.
  desc[0] = TOP (= arena_end - 0x18), desc[1] = pool_count.
  Free ring: sentinel at TOP+0xC, first node at *(TOP+0x10);
             node = [owner, link, size], payload = node+0xC.
  Pool p allocated ring: sentinel at TOP - p*0xC, head at *(sentinel+4),
             walk via node[1] back to sentinel.
  Alloc counter gp+0x488 = 0x8007B7A0; malloc-err accum gp+0x510 = 0x8007B828.
Usage: heap_walk.py <ram.bin> [--base 0x80000000] [label]
"""
import struct, sys

DESC_PTR_VA = 0x8007BB58
ERR_VA = 0x8007B828
COUNT_VA = 0x8007B7A0
HEAP_BASE_VAR = 0x8007B414
RECORD_TABLE = 0x801C9348  # per-enemy record ptr table (5 slots)
FORMATION = 0x8007BD0C


def main():
    path = sys.argv[1]
    base = 0x80000000
    label = sys.argv[2] if len(sys.argv) > 2 else path
    ram = open(path, "rb").read()

    def u32(va):
        off = va - base
        if 0 <= off <= len(ram) - 4:
            return struct.unpack_from("<I", ram, off)[0]
        return None

    desc = u32(DESC_PTR_VA)
    print(f"== {label} ==")
    print(f"heap desc ptr @0x{DESC_PTR_VA:08X} = 0x{desc:08X}")
    print(f"heap base var @0x{HEAP_BASE_VAR:08X} = 0x{u32(HEAP_BASE_VAR):08X}")
    top = u32(desc)
    pools = u32(desc + 4)
    print(f"TOP = 0x{top:08X}  pool_count = {pools}")
    arena_end = top + 0x18
    arena_base = desc
    print(f"arena = 0x{arena_base:08X}..0x{arena_end:08X}  ({(arena_end-arena_base)/1024:.1f} KB)")
    print(f"alloc count = {u32(COUNT_VA)}   malloc-err accum = 0x{u32(ERR_VA):X}")

    def in_heap(va):
        return va is not None and arena_base <= va < arena_end

    # Free ring
    sent = top + 0xC
    node = u32(top + 0x10)
    free_total, free_max, nfree = 0, 0, 0
    seen = set()
    print("free ring:")
    while node != sent and in_heap(node) and node not in seen and nfree < 200:
        seen.add(node)
        size = u32(node + 8)
        print(f"  free @0x{node:08X}  size 0x{size:X} ({size/1024:.1f} KB)")
        free_total += size
        free_max = max(free_max, size)
        nfree += 1
        node = u32(node + 4)
    print(f"  -> {nfree} free nodes, total {free_total/1024:.1f} KB, largest {free_max/1024:.1f} KB")

    # Allocated rings per pool
    for p in range(pools):
        sentinel = top - p * 0xC
        node = u32(sentinel + 4)
        seen = set()
        total, n = 0, 0
        rows = []
        while node != sentinel and in_heap(node) and node not in seen and n < 500:
            seen.add(node)
            size = u32(node + 8)
            rows.append((node + 0xC, size))
            total += size
            n += 1
            node = u32(node + 4)
        print(f"pool {p}: {n} allocations, {total/1024:.1f} KB")
        for payload, size in sorted(rows, key=lambda r: -r[1])[:20]:
            print(f"  alloc payload @0x{payload:08X}  size 0x{size:X} ({size/1024:.1f} KB)")

    # Battle context
    print("formation cells:", [(u32(FORMATION) or 0) & 0xFF,
                               ((u32(FORMATION) or 0) >> 8) & 0xFF,
                               ((u32(FORMATION) or 0) >> 16) & 0xFF,
                               ((u32(FORMATION) or 0) >> 24) & 0xFF])
    recs = [u32(RECORD_TABLE + i * 4) for i in range(5)]
    print("record table 0x801C9348:", [f"0x{r:08X}" if r else "0" for r in recs])


main()
