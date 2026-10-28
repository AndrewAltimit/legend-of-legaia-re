-- autorun_stack_highwater.lua
--
-- Main-stack high-water mark by canary fill. The BIOS enters the EXE with
-- sp = 0x801FFFF0 (the header's s_addr), but the startup code at
-- 0x80026C74 reloads it as 0x80200000 (the __ramsize word at 0x80026CD8
-- ORed with 0x80000000), so the stack grows down from the top of RAM into
-- the free band above the largest slot-B image (0x801FA9D8); depths here
-- are measured from 0x80200000. The patcher parks code and data in that
-- band (docs/reference/memory-map.md "Free RAM and stack").
-- This probe answers how deep the stack really goes on a given path:
--
--   1. Canary. Shortly after the state loads, every word of
--      [0x801FA9D8, sp - 64) is overwritten with CANARY. Words below the
--      live sp are dead: the PsyQ interrupt callbacks run on their own
--      stack (ResetCallback seeds the hook's jmp_buf sp slot 0x80079814 with
--      0x8007A7F0), so nothing asynchronous pushes onto the main stack.
--      Every SCAN_EVERY vsyncs the band is rescanned from the bottom; the
--      lowest non-canary word is the high-water mark so far. When the live
--      sp sits higher than the filled top, the fill is extended upward (the
--      words between are dead too), so a fill taken at a deep moment does
--      not cap what the run can see.
--   2. Frontier watch (interpreter core only, LEGAIA_STACK_FRONTIER=1).
--      FRONTIER_WORDS width-4 Write breakpoints are armed just below the
--      current mark. A store that lands there is a new deepest point: the
--      probe records pc / ra / sp, unwinds the frame chain heuristically
--      (prologue `addiu sp,sp,-N` + `sw ra,K(sp)`, each return address
--      validated by a jal / jalr at ra-8), lists every validated return
--      address on the stack, then re-arms lower on the next vsync.
--      LEGAIA_STACK_WATCH=0xADDR arms the watch at [ADDR, ADDR+64) from the
--      start instead, to name the path reaching a mark a previous run found.
--   3. Pad. LEGAIA_STACK_PAD = "none" (default) | "noise" (seeded random
--      buttons, LEGAIA_STACK_SEED, LEGAIA_STACK_NOISE_SKIP = buttons left
--      out, e.g. "START,SELECT") | a schedule "BTN:from:to,BTN:from:to,..."
--      in capture vsyncs.
--   4. Formation (LEGAIA_STACK_FORMATION="108,3"): at capture vsync
--      LEGAIA_STACK_FORMATION_AT (default 60) the encounter cells
--      0x8007BD0C..0F are written and game_mode set to 8, the install
--      FUN_801DA51C performs on a rolled encounter, so a field state loads
--      a chosen battle.
--   5. Wipe (LEGAIA_STACK_KILL=1): while battle main runs (mode 0x15) each
--      party actor's HP (actor+0x14C via the 0x801C9370 table) above 1 is
--      clamped to 1, so the next enemy hit wipes the party and the run
--      follows retail's wipe path back to the CARD continue screen.
--   6. Cold boot (LEGAIA_STACK_COLD=1, interpreter core): no state load;
--      the canary is laid at the EXE entry point 0x80026C28 itself, so boot
--      and title are measured from their first instruction.
--
-- Outputs (LEGAIA_OUT_DIR):
--   stack_hw.csv        tick,sp_now,fill_top,lowest_dirty,dirty_in_256,depth,mode,scene
--   stack_summary.txt   final mark, depth, margins, validated ra residue
--   stack_frontier.txt  frontier hits with call chains (frontier mode)
--
-- Launch (fast canary-only sweep):
--   LEGAIA_FRAMES=3600 LEGAIA_STACK_PAD=noise \
--   bash scripts/pcsx-redux/run_probe.sh --fast --sstate <state> \
--     --iso <retail.bin> --lua scripts/pcsx-redux/autorun_stack_highwater.lua
-- Launch (deep-path naming): drop --fast, add LEGAIA_STACK_FRONTIER=1.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local mem = require("probe.mem")
local pad = require("probe.pad")

local BAND_LO     = 0x801FA9D8
local STACK_TOP   = 0x80200000
local CANARY      = 0xDEADBEEF
local SAFETY      = 64
local SCAN_EVERY  = probe.getenv_num("LEGAIA_STACK_SCAN_EVERY", 30)
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 3600)
local FILL_AT     = probe.getenv_num("LEGAIA_STACK_FILL_AT", 2)
local PAD_MODE    = probe.getenv("LEGAIA_STACK_PAD", "none")
local SEED        = probe.getenv_num("LEGAIA_STACK_SEED", 1)
local FRONTIER    = probe.getenv("LEGAIA_STACK_FRONTIER", "0") == "1"
local WATCH_AT    = tonumber(probe.getenv("LEGAIA_STACK_WATCH", "") or "")
local COLD        = probe.getenv("LEGAIA_STACK_COLD", "0") == "1"
local EXE_ENTRY   = 0x80026C28
local FORMATION   = probe.getenv("LEGAIA_STACK_FORMATION", "")
local FORMATION_AT = probe.getenv_num("LEGAIA_STACK_FORMATION_AT", 60)
local FORMATION_CELL = 0x8007BD0C
local KILL        = probe.getenv("LEGAIA_STACK_KILL", "0") == "1"
local ACTOR_TABLE = 0x801C9370
local FRONTIER_WORDS = 32
local MAX_FRONTIER_HITS = probe.getenv_num("LEGAIA_STACK_MAX_HITS", 24)
local ARENA_END   = 0x801FDEB4  -- Super Arts Pack block end (crates/patcher)
local OLD_MARK    = 0x801FE420  -- residue mark of the save-state library

local GAME_MODE_VA  = 0x8007B83C
local SCENE_NAME_VA = 0x8007050C

local csv = probe.csv_open(probe.out_path("stack_hw.csv"),
    "tick,sp_now,fill_top,lowest_dirty,dirty_in_256,depth,mode,scene")
local frontier_path = probe.out_path("stack_frontier.txt")
local summary_path  = probe.out_path("stack_summary.txt")

local function u32(v) return bit.band(v or 0, 0xFFFFFFFF) % 0x100000000 end
local function hex(v) return string.format("0x%08X", u32(v)) end

local function reg_sp()
    local r = PCSX.getRegisters()
    return u32(tonumber(r.GPR.n.sp))
end

local function scene_name()
    local b = mem.read_bytes(SCENE_NAME_VA, 8)
    if b == nil then return "?" end
    local s = tostring(b):gsub("%z.*$", "")
    s = s:gsub("[^%w_]", "")
    return s
end

---------------------------------------------------------------- canary
local fill_top = nil       -- exclusive top of the canary band
local lowest   = nil       -- lowest non-canary word seen so far
local min_sp   = nil       -- lowest main-range sp sampled at a vsync
local last_raw_sp = 0

local function in_main_stack(sp)
    return sp > BAND_LO + 0x100 and sp <= STACK_TOP
end

-- Four SCUS routines run on a scratchpad stack (FUN_8001D140 and the
-- switch inside FUN_80026CEC at 0x1F8002BC / 0x1F8002A4, FUN_80021DF4 at
-- 0x1F8002A4, FUN_8002B28C at 0x1F8002DC): each stores the main sp in the
-- slot word and sets sp = slot - 4. A vsync that lands inside one sees a
-- scratchpad sp; the live main sp is then the word in the active slot.
-- The scratchpad stacks overlap (0x1F8002BC's grows down past 0x1F8002A4),
-- so the probe takes the lowest main-range word of the three: the active
-- slot is never under its own frames, and a lower stale word only makes
-- the fill smaller, never live.
local SCRATCH_SLOTS = { 0x1F8002A4, 0x1F8002BC, 0x1F8002DC }
local function main_sp(sp)
    if in_main_stack(sp) then return sp end
    if sp >= 0x1F800000 and sp < 0x1F800400 then
        local best = nil
        for _, slot in ipairs(SCRATCH_SLOTS) do
            local saved = u32(mem.read_scratch_u32(slot))
            if in_main_stack(saved) and (best == nil or saved < best) then best = saved end
        end
        return best
    end
    return nil
end

local function fill(lo, hi)
    local n = 0
    local a = lo
    while a < hi do
        mem.write_u32(a, CANARY)
        a = a + 4
        n = n + 1
    end
    return n
end

-- Returns lowest dirty address below fill_top (or fill_top when clean),
-- and the dirty-word count in the 256 bytes from it (isolated stray writes
-- show up as a count of 1..2 far below the real frontier).
local function scan()
    if fill_top == nil then return nil, 0 end
    local len = fill_top - BAND_LO
    local buf = mem.read_bytes(BAND_LO, len)
    if buf == nil then return nil, 0 end
    local s = tostring(buf)
    local p = ffi.cast("const uint32_t*", s)
    local nw = math.floor(len / 4)
    local first = nil
    for i = 0, nw - 1 do
        if p[i] ~= CANARY then first = i; break end
    end
    if first == nil then return fill_top, 0 end
    local cnt = 0
    for i = first, math.min(nw - 1, first + 63) do
        if p[i] ~= CANARY then cnt = cnt + 1 end
    end
    return BAND_LO + first * 4, cnt
end

---------------------------------------------------------------- pad
local rng = SEED
local function rand(n)
    rng = (rng * 1103515245 + 12345) % 2147483648
    return math.floor(rng / 65536) % n
end

local NOISE_BTNS = { "UP", "DOWN", "LEFT", "RIGHT", "CROSS", "CIRCLE",
    "TRIANGLE", "SQUARE", "L1", "R1", "L2", "R2", "START", "SELECT" }
do
    local skip = {}
    for b in string.gmatch(probe.getenv("LEGAIA_STACK_NOISE_SKIP", ""), "[%w]+") do
        skip[b] = true
    end
    local kept = {}
    for _, b in ipairs(NOISE_BTNS) do if not skip[b] then kept[#kept + 1] = b end end
    NOISE_BTNS = kept
end
local held = {}        -- btn name -> release tick
local schedule = {}
if PAD_MODE ~= "none" and PAD_MODE ~= "noise" then
    for name, a, b in string.gmatch(PAD_MODE, "(%u+%d?):(%d+):(%d+)") do
        schedule[#schedule + 1] = { btn = name, from = tonumber(a), to = tonumber(b) }
    end
end

local function pad_tick(t)
    if PAD_MODE == "noise" then
        for b, rel in pairs(held) do
            if t >= rel then pad.release(pad.BTN[b]); held[b] = nil end
        end
        if rand(4) == 0 then
            local b = NOISE_BTNS[rand(#NOISE_BTNS) + 1]
            if not held[b] then
                -- START/SELECT taps stay short so the pause screen toggles
                local dur = (b == "START" or b == "SELECT") and 2 or (2 + rand(14))
                pad.force(pad.BTN[b]); held[b] = t + dur
            end
        end
    else
        for _, e in ipairs(schedule) do
            if t == e.from then pad.force(pad.BTN[e.btn]) end
            if t == e.to then pad.release(pad.BTN[e.btn]) end
        end
    end
end

local function pad_release_all()
    for b, _ in pairs(held) do pad.release(pad.BTN[b]) end
    for _, e in ipairs(schedule) do pad.release(pad.BTN[e.btn]) end
end

---------------------------------------------------------------- unwinder
local function is_code(a)
    a = u32(a)
    return (a >= 0x80010000 and a < 0x8007C000) or (a >= 0x801CE818 and a < BAND_LO)
end

local function call_before(ra)
    if not is_code(ra) or ra % 4 ~= 0 then return false end
    local w = mem.read_u32(ra - 8)
    if w == nil then return false end
    local op = bit.rshift(w, 26)
    if op == 3 then return true end                       -- jal
    if op == 0 and bit.band(w, 0x3F) == 9 then return true end  -- jalr
    return false
end

-- Nearest `addiu sp,sp,-N` at or before pc: returns prologue addr, N.
local function find_prologue(pc)
    local a = pc - (pc % 4)
    for _ = 1, 4096 do
        local w = mem.read_u32(a)
        if w == nil then return nil end
        if bit.rshift(w, 16) == 0x27BD then
            local imm = bit.band(w, 0xFFFF)
            if imm >= 0x8000 then return a, 0x10000 - imm end
        end
        a = a - 4
    end
    return nil
end

-- `sw ra,K(sp)` between prologue and pc (exclusive): K or nil.
local function find_ra_slot(pro, pc)
    local a = pro
    for _ = 1, 64 do
        if a >= pc then return nil end
        local w = mem.read_u32(a)
        if w ~= nil and bit.rshift(w, 16) == 0xAFBF then
            local k = bit.band(w, 0xFFFF)
            if k < 0x8000 then return k end
        end
        a = a + 4
    end
    return nil
end

local function unwind(pc, sp, ra)
    local chain = { string.format("pc %s sp %s", hex(pc), hex(sp)) }
    for depth = 1, 48 do
        if not is_code(pc) then
            chain[#chain + 1] = string.format("  (pc %s outside game code; ra reg %s)", hex(pc), hex(ra))
            break
        end
        local pro, size = find_prologue(pc)
        if pro == nil then chain[#chain + 1] = "  (no prologue)"; break end
        local k = find_ra_slot(pro, pc)
        local caller
        if k ~= nil then caller = mem.read_u32(sp + k)
        elseif depth == 1 then caller = ra
        else chain[#chain + 1] = "  (no ra slot)"; break end
        chain[#chain + 1] = string.format("  #%d fn~%s frame %d (sp %s) -> ret %s%s",
            depth, hex(pro), size, hex(sp), hex(caller),
            call_before(caller) and "" or " (unvalidated)")
        if not call_before(caller) then break end
        pc = caller
        sp = sp + size
        if sp >= STACK_TOP then break end
    end
    return chain
end

local function ra_residue(lo, hi)
    local out = {}
    local a = lo - (lo % 4)
    while a < hi do
        local w = mem.read_u32(a)
        if w ~= nil and w ~= CANARY and call_before(w) then
            out[#out + 1] = string.format("  [%s] %s", hex(a), hex(w))
        end
        a = a + 4
    end
    return out
end

---------------------------------------------------------------- frontier
local fbps = {}
local frontier_lo = nil
local frontier_pending = false
local frontier_hits = 0
local seen_pc = {}
local ffh = nil

local function frontier_disarm()
    for _, b in ipairs(fbps) do pcall(function() b:remove() end) end
    fbps = {}
end

local tick_now = 0
local function on_frontier_hit(addr_hint)
    if frontier_pending or frontier_hits >= MAX_FRONTIER_HITS then return end
    local r = PCSX.getRegisters()
    local pc = u32(tonumber(r.pc))
    local sp = u32(tonumber(r.GPR.n.sp))
    local ra = u32(tonumber(r.GPR.n.ra))
    frontier_pending = true
    if seen_pc[pc] and WATCH_AT == nil then return end
    seen_pc[pc] = true
    frontier_hits = frontier_hits + 1
    ffh = ffh or io.open(frontier_path, "w")
    ffh:write(string.format("== hit %d tick %d mode 0x%X scene %s watch %s sp %s depth %d ra %s\n",
        frontier_hits, tick_now, mem.read_u16(GAME_MODE_VA) or 0, scene_name(),
        hex(addr_hint), hex(sp), STACK_TOP - sp, hex(ra)))
    for _, l in ipairs(unwind(pc, sp, ra)) do ffh:write(l .. "\n") end
    ffh:write("  validated return addresses on the stack:\n")
    for _, l in ipairs(ra_residue(sp, math.min(STACK_TOP, sp + 0x1000))) do ffh:write(l .. "\n") end
    ffh:flush()
end

local function frontier_arm(lo)
    frontier_disarm()
    lo = lo - (lo % 4)
    frontier_lo = lo
    for i = 0, FRONTIER_WORDS - 1 do
        local a = lo + i * 4
        fbps[#fbps + 1] = PCSX.addBreakpoint(a, "Write", 4, "stack_frontier",
            function() pcall(on_frontier_hit, a) end)
    end
    frontier_pending = false
end

---------------------------------------------------------------- driver
local fh_sum = nil
local function record(tick)
    local lo, cnt = scan()
    if lo ~= nil and (lowest == nil or lo < lowest) then lowest = lo end
    local sp = reg_sp()
    csv:row("%d,%s,%s,%s,%d,%d,0x%X,%s", tick, hex(sp), hex(fill_top or 0),
        hex(lo or 0), cnt, lo and (STACK_TOP - lo) or 0,
        mem.read_u16(GAME_MODE_VA) or 0, scene_name())
    return lo
end

local function lay_canary(sp)
    fill_top = sp - SAFETY
    fill_top = fill_top - (fill_top % 4)
    local n = fill(BAND_LO, fill_top)
    PCSX.log(string.format("[stack] canary laid %s..%s (%d words), sp %s",
        hex(BAND_LO), hex(fill_top), n, hex(sp)))
    if WATCH_AT ~= nil then frontier_arm(WATCH_AT); frontier_lo = WATCH_AT end
end

if COLD then
    require("probe.sstate").load = function(_)
        PCSX.log("[stack] cold boot; sstate load skipped")
        return true
    end
end

probe.run({
    sstate = COLD and "unused" or probe.getenv("LEGAIA_SSTATE", "unused"),
    capture_frames = FRAMES,
    boot_delay = COLD and 1 or 60,
    on_arm = function(ctx)
        if COLD then
            local entry_bp
            entry_bp = PCSX.addBreakpoint(EXE_ENTRY, "Exec", 4, "stack_exe_entry",
                function()
                    if fill_top == nil then
                        pcall(lay_canary, reg_sp())
                    end
                end)
            STACK_ENTRY_BP = entry_bp
        end
        return {}
    end,
    on_capture = function(ctx, t)
        tick_now = t
        pad_tick(t)
        if FORMATION ~= "" and t == FORMATION_AT then
            local ids = {}
            for v in string.gmatch(FORMATION, "%d+") do ids[#ids + 1] = tonumber(v) end
            for i = 0, 3 do mem.write_u8(FORMATION_CELL + i, ids[i + 1] or 0) end
            mem.write_u16(GAME_MODE_VA, 8)
            PCSX.log("[stack] formation installed: " .. FORMATION)
        end
        if KILL and mem.read_u16(GAME_MODE_VA) == 0x15 then
            for i = 0, 2 do
                local ptr = mem.read_u32(ACTOR_TABLE + i * 4)
                if ptr ~= nil and mem.in_ram(ptr + 0x14C, 2) then
                    local hp = mem.read_u16(ptr + 0x14C) or 0
                    if hp > 1 then
                        mem.write_u16(ptr + 0x14C, 1)
                        PCSX.log(string.format("[stack] tick %d: party slot %d HP %d -> 1", t, i, hp))
                    end
                end
            end
        end
        last_raw_sp = reg_sp()
        local sp = main_sp(last_raw_sp)
        if sp ~= nil and (min_sp == nil or sp < min_sp) then min_sp = sp end
        if fill_top == nil then
            if not COLD and t >= FILL_AT and sp ~= nil then lay_canary(sp) end
            return
        end
        if frontier_pending then
            -- a store landed below the mark: rescan and push the watch lower
            local lo = record(t)
            if WATCH_AT == nil and lo ~= nil then frontier_arm(lo - FRONTIER_WORDS * 4)
            else frontier_pending = false end
        end
        if t % SCAN_EVERY == 0 then
            local lo = record(t)
            -- extend the canary up to a live sp that sits above the filled top
            if sp ~= nil and sp - SAFETY > fill_top and lo == fill_top then
                local new_top = sp - SAFETY
                new_top = new_top - (new_top % 4)
                fill(fill_top, new_top)
                fill_top = new_top
            end
            if FRONTIER and WATCH_AT == nil and lo ~= nil
                and (frontier_lo == nil or lo < frontier_lo + FRONTIER_WORDS * 4) then
                frontier_arm(lo - FRONTIER_WORDS * 4)
            end
        end
    end,
    on_done = function(ctx)
        pad_release_all()
        frontier_disarm()
        local lo = record(FRAMES)
        if lo ~= nil and (lowest == nil or lo < lowest) then lowest = lo end
        fh_sum = io.open(summary_path, "w")
        local function w(fmt, ...) fh_sum:write(string.format(fmt, ...) .. "\n") end
        w("sstate        %s", COLD and "(cold boot)" or tostring(ctx.sstate))
        w("pad           %s seed %d", PAD_MODE, SEED)
        w("frames        %d", FRAMES)
        w("fill_top      %s", hex(fill_top or 0))
        if lowest ~= nil then
            w("high_water    %s", hex(lowest))
            w("depth         %d", STACK_TOP - lowest)
            w("margin_arena  %d", lowest - ARENA_END)
            w("vs_0x801FE420 %d", lowest - OLD_MARK)
            w("capped        %s", (lowest >= (fill_top or 0)) and "yes (mark is the fill top: an upper bound)" or "no")
        else
            w("high_water    (canary never laid; last raw sp %s)", hex(last_raw_sp))
        end
        w("min_sp_sample %s", hex(min_sp or 0))
        w("frontier_hits %d", frontier_hits)
        w("final mode 0x%X scene %s", mem.read_u16(GAME_MODE_VA) or 0, scene_name())
        if lowest ~= nil then
            w("validated return addresses in the 1 KB above the mark:")
            for _, l in ipairs(ra_residue(lowest, math.min(STACK_TOP, lowest + 0x400))) do w("%s", l) end
        end
        fh_sum:close()
        csv:close()
        if ffh then ffh:close() end
        PCSX.log(string.format("[stack] DONE high_water=%s depth=%d margin=%d",
            hex(lowest or 0), lowest and (STACK_TOP - lowest) or -1,
            lowest and (lowest - ARENA_END) or -1))
    end,
})
