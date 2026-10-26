-- autorun_battle_heap_ledger.lua
--
-- Attributed allocation ledger of the battle heap over one battle load.
-- From a field save state it installs a formation the way FUN_801DA51C
-- states 1/2 do on a rolled encounter (cells 0x8007BD0C..0F, game_mode = 8;
-- the same install as autorun_delilas_battle_load.lua), then logs every
-- heap event until battle main has settled:
--
--   alloc   FUN_8002B468(pool a0, size a1) - entry (size, $ra) and both
--           success returns 0x8002B53C / 0x8002B57C (the returned pointer
--           is in v0 at the jr) plus the failure return 0x8002B484. When
--           $ra is 0x800178AC the call came through the malloc wrapper
--           FUN_80017888, whose own return address sits at sp+0x18.
--   free    FUN_8002B584(ptr a0) - entry; the node's size is ptr[-4].
--           Through the wrapper FUN_80017B94 ($ra 0x80017BDC) the real
--           caller is at sp+0x14.
--   drain   FUN_8002A880(pool a0) - frees a whole pool's allocated ring;
--           every node on that ring is logged as freed before it runs.
--   init    FUN_8002B3D4 - a heap re-initialisation (logged, not expected).
--
-- Each caller is named by its return address and by the function entry the
-- probe finds scanning back from it for `addiu sp,sp,-N` (27BDxxxx, negative
-- immediate). After every event the free ring is walked, so the log carries
-- the exact free total at each step. At battle-main entry and after the
-- settle the allocated rings of every pool and the free ring are dumped.
--
-- Launch:
--   LEGAIA_SSTATE=<field state> LEGAIA_IDS=108,3 LEGAIA_FRAMES=2400 \
--   bash scripts/pcsx-redux/run_probe.sh --lua scripts/pcsx-redux/autorun_battle_heap_ledger.lua \
--     --iso <scratch copy of the retail disc> --out-dir <dir>
--
-- Breakpoints fire only under the interpreter + debugger core (no --fast).
-- One and two frames further out, `outer` / `outer2` are the caller's own
-- return address and its caller's, each read from the slot the function's
-- prologue `sw ra,X(sp)` names.
-- Output: heap_ledger.csv  vsync,mode,event,pool,size,ptr,ra,ra_fn,caller,caller_fn,free_total,free_nodes,note,outer,outer_fn,outer2,outer2_fn
-- Offline: scripts/asset-investigation/battle-heap-ledger.py nets it out.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local GAME_MODE      = 0x8007B83C
local FORMATION_CELL = 0x8007BD0C
local HEAP_DESC_PTR  = 0x8007BB58 -- gp+0x840 -> {top, pool_count}
local BATTLE_MAIN    = 0x15

local ALLOC_ENTRY  = 0x8002B468
local ALLOC_RET_A  = 0x8002B53C -- split a node: v0 = new node + 0xC
local ALLOC_RET_B  = 0x8002B57C -- take a whole node: v0 = node + 0xC
local ALLOC_FAIL   = 0x8002B484
local FREE_ENTRY   = 0x8002B584
local DRAIN_ENTRY  = 0x8002A880
local INIT_ENTRY   = 0x8002B3D4
local MALLOC_RA    = 0x800178AC -- return into FUN_80017888
local FREE_RA      = 0x80017BDC -- return into FUN_80017B94

local SSTATE   = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 2400)
local FORCE_AT = probe.getenv_num("LEGAIA_FORCE_AT", 60)
local SETTLE   = probe.getenv_num("LEGAIA_SETTLE", 240)
local IDS_RAW  = probe.getenv("LEGAIA_IDS", "108,3")

local ids = {}
for tok in string.gmatch(IDS_RAW, "[^,%s]+") do ids[#ids + 1] = tonumber(tok) end

local CSV = probe.csv_open(probe.out_path("heap_ledger.csv"),
    "vsync,mode,event,pool,size,ptr,ra,ra_fn,caller,caller_fn,free_total,free_nodes,note,outer,outer_fn,outer2,outer2_fn")

local function n32(v) return (tonumber(v) or 0) % 0x100000000 end
local function reg(r, name)
    local ok, v = pcall(function() return n32(r.GPR.n[name]) end)
    if ok then return v end
    return 0
end

-- Function entry of the code containing `addr`: the nearest preceding
-- `addiu sp,sp,-N`. Cached per address.
local fn_cache = {}
local function fn_entry(addr)
    if addr == 0 then return 0 end
    if fn_cache[addr] then return fn_cache[addr] end
    local a = addr - 8
    for _ = 1, 0x2000 do
        local w = probe.read_u32(a) or 0
        if bit.rshift(w, 16) == 0x27BD and bit.band(w, 0x8000) ~= 0 then
            fn_cache[addr] = a
            return a
        end
        a = a - 4
    end
    fn_cache[addr] = 0
    return 0
end

-- The return address one frame further out: `caller` returns into a
-- function whose entry is `fn` and whose frame starts at `frame_sp`; its
-- prologue's `sw ra,X(sp)` (AFBFxxxx) names the slot its own $ra sits in.
local function outer_ra(fn, frame_sp)
    if fn == 0 then return 0 end
    local w0 = probe.read_u32(fn) or 0
    local frame = 0x10000 - bit.band(w0, 0xFFFF)
    for i = 1, 40 do
        local w = probe.read_u32(fn + i * 4) or 0
        if bit.rshift(w, 16) == 0xAFBF then
            return probe.read_u32(frame_sp + bit.band(w, 0xFFFF)) or 0, frame_sp + frame
        end
    end
    return 0, 0
end

local function heap_top()
    local desc = probe.read_u32(HEAP_DESC_PTR) or 0
    if desc == 0 then return 0, 0 end
    return probe.read_u32(desc) or 0, probe.read_u32(desc + 4) or 0
end

local function free_walk(dump, tag)
    local top = heap_top()
    if top == 0 then return 0, 0, 0 end
    local sent = top + 0xC
    local node = probe.read_u32(sent + 4)
    local total, largest, n = 0, 0, 0
    while node ~= nil and node ~= sent and n < 512 do
        local size = probe.read_u32(node + 8) or 0
        total = total + size
        if size > largest then largest = size end
        n = n + 1
        if dump then
            CSV:row("0,0,%s-free,0,0x%X,0x%08X,0,0,0,0,0,0,", tag, size, node + 0xC)
        end
        node = probe.read_u32(node + 4)
    end
    return total, n, largest
end

local function alloc_ring(pool, cb)
    local top = heap_top()
    if top == 0 then return end
    local sent = top - pool * 0xC
    local node = probe.read_u32(sent + 4)
    local n = 0
    while node ~= nil and node ~= sent and n < 1024 do
        cb(node, probe.read_u32(node + 8) or 0)
        n = n + 1
        node = probe.read_u32(node + 4)
    end
end

local function dump_rings(tag)
    local _, pools = heap_top()
    for p = 0, pools - 1 do
        local sum, n = 0, 0
        alloc_ring(p, function(node, size)
            sum = sum + size
            n = n + 1
            CSV:row("0,0,%s-alloc,%d,0x%X,0x%08X,0,0,0,0,0,0,", tag, p, size, node + 0xC)
        end)
        CSV:row("0,0,%s-pool,%d,0x%X,0,0,0,0,0,0,%d,", tag, p, sum, n)
    end
    local total, n, largest = free_walk(true, tag)
    CSV:row("0,0,%s-freesum,0,0x%X,0,0,0,0,0,0x%X,%d,largest=0x%X", tag, total, total, n, largest)
end

local installed = false
local vs = 0
local mode_now = 0
local pending = nil

local function emit(ev, pool, size, ptr, ra, caller, note, outer, outer2)
    local total, n = free_walk(false)
    outer = outer or 0
    outer2 = outer2 or 0
    CSV:row("%d,0x%X,%s,%d,0x%X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%X,%d,%s,0x%08X,0x%08X,0x%08X,0x%08X",
        vs, mode_now, ev, pool, size, ptr, ra, fn_entry(ra), caller, fn_entry(caller),
        total, n, note or "", outer, fn_entry(outer), outer2, fn_entry(outer2))
end

local function on_alloc_ret(ok)
    if not installed or pending == nil then return end
    local r = PCSX.getRegisters()
    local p = pending
    pending = nil
    emit(ok and "alloc" or "alloc-fail", p.pool, p.size, ok and reg(r, "v0") or 0,
        p.ra, p.caller, nil, p.outer, p.outer2)
end

local main_seen, settle, dumped_main = false, 0, false

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function(ctx)
        probe.arm_breakpoint(ALLOC_ENTRY, "Exec", 4, "alloc", function()
            if not installed then return end
            local r = PCSX.getRegisters()
            local ra = reg(r, "ra")
            local caller = ra
            local sp = reg(r, "sp")
            local caller_sp = sp
            if ra == MALLOC_RA then
                caller = probe.read_u32(sp + 0x18) or 0
                caller_sp = sp + 0x20
            end
            local outer, outer_sp = outer_ra(fn_entry(caller), caller_sp)
            local outer2 = 0
            if outer ~= 0 then outer2 = outer_ra(fn_entry(outer), outer_sp) end
            pending = { pool = reg(r, "a0"), size = reg(r, "a1"), ra = ra, caller = caller,
                outer = outer, outer2 = outer2 }
        end)
        probe.arm_breakpoint(ALLOC_RET_A, "Exec", 4, "alloc_ret_a", function() on_alloc_ret(true) end)
        probe.arm_breakpoint(ALLOC_RET_B, "Exec", 4, "alloc_ret_b", function() on_alloc_ret(true) end)
        probe.arm_breakpoint(ALLOC_FAIL, "Exec", 4, "alloc_fail", function() on_alloc_ret(false) end)
        probe.arm_breakpoint(FREE_ENTRY, "Exec", 4, "free", function()
            if not installed then return end
            local r = PCSX.getRegisters()
            local ptr = reg(r, "a0")
            local ra = reg(r, "ra")
            local caller = ra
            if ra == FREE_RA then caller = probe.read_u32(reg(r, "sp") + 0x14) or 0 end
            emit("free", 0, probe.read_u32(ptr - 4) or 0, ptr, ra, caller)
        end)
        probe.arm_breakpoint(DRAIN_ENTRY, "Exec", 4, "drain", function()
            if not installed then return end
            local r = PCSX.getRegisters()
            local pool = reg(r, "a0")
            local ra = reg(r, "ra")
            alloc_ring(pool, function(node, size)
                emit("drain-free", pool, size, node + 0xC, ra, ra)
            end)
            emit("drain", pool, 0, 0, ra, ra)
        end)
        probe.arm_breakpoint(INIT_ENTRY, "Exec", 4, "init", function()
            if not installed then return end
            local r = PCSX.getRegisters()
            emit("heap-init", reg(r, "a0"), reg(r, "a2"), reg(r, "a1"), reg(r, "ra"), reg(r, "ra"))
        end)
        return {}
    end,
    on_capture = function(ctx, elapsed)
        vs = elapsed
        mode_now = probe.read_u16(GAME_MODE) or 0
        if elapsed == FORCE_AT then
            dump_rings("pre")
            for i = 0, 3 do probe.write_u8(FORMATION_CELL + i, ids[i + 1] or 0) end
            probe.write_u16(GAME_MODE, 8)
            installed = true
            CSV:row("%d,0x%X,install,0,0,0,0,0,0,0,0,0,ids=%s", elapsed, mode_now, IDS_RAW)
            return
        end
        if not installed then return end
        if mode_now == BATTLE_MAIN then
            if not dumped_main then
                dumped_main = true
                dump_rings("main")
            end
            settle = settle + 1
            if settle == SETTLE then
                dump_rings("settled")
                CSV:close()
                ctx.request_quit = true
            end
        end
    end,
    on_done = function()
        pcall(function() CSV:close() end)
    end,
})
