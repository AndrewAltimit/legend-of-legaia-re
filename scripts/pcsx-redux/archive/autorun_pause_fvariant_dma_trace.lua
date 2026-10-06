-- autorun_pause_fvariant_dma_trace.lua
--
-- Pin the pause-menu-path writer of the extraction-0874 s2 (player.lzs)
-- F-variant pixels (VRAM row 271, x=853/856/857, content = the disc words two
-- rows down at (x,273)). The libgpu LoadImage/MoveImage wrappers are silent
-- on the pause path (autorun_pause_vram_upload_trace.lua: zero calls after
-- game_mode 0x17), so the transfer must ride the GPU DMA chain or direct GP0
-- stores. This probe:
--
--   * Write-BPs DMA2 MADR (0x1F8010A0) to remember the last chain head + the
--     PC that staged it.
--   * Write-BPs DMA2 CHCR (0x1F8010A8): on every kick inside the vsync
--     window, walks the linked-list chain (or block buffer) in main RAM and
--     logs every GP0 A0h (CPU->VRAM) / 80h (VRAM->VRAM) packet whose dest y
--     lands in [256, 288) - wide net around row 271.
--   * Write-BPs GP0 (0x1F801810) and logs stores whose value is an A0/80
--     command or a coord word with y in [256, 288) - catches a programmed-IO
--     upload with the storing PC.
--
-- Pad input: LEGAIA_PAD_SCRIPT="60:SELECT,200:DOWN,250:CROSS" (one-shot
-- steps, autorun_pad_walk.lua grammar) and/or LEGAIA_MASH="CROSS:50:6"
-- (periodic pulse for dialog advance). The finding this probe closed: the
-- pause-menu path issues NO image transfers at all - the F-variant is a
-- parked wrap-scroll phase of the FUN_80021DF4 dispatch-4 texture-scroll
-- arm (see docs/formats/character-mesh.md, runtime scroll-cell residue).
--
--   LEGAIA_FRAMES=160 \
--       timeout --kill-after=30s 900s bash scripts/pcsx-redux/run_probe.sh \
--       --scenario field_walled_collision_pin \
--       --lua scripts/pcsx-redux/autorun_pause_fvariant_dma_trace.lua

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local mem = require("probe.mem")
local pad = require("probe.pad")

local SSTATE = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 160)
local OUT_DIR = probe.getenv("LEGAIA_OUT_DIR", "/tmp/pausevram3")
local SELECT_AT = probe.getenv_num("LEGAIA_SELECT_AT", 60)
local SELECT_HOLD = 8
-- Only walk chains / log GP0 inside this vsync window (menu init).
local WIN_LO = probe.getenv_num("LEGAIA_WIN_LO", 55)
local WIN_HI = probe.getenv_num("LEGAIA_WIN_HI", 150)

local GAME_MODE_VA = 0x8007b83c
local Y_LO, Y_HI = 256, 288

os.execute(string.format("mkdir -p %q", OUT_DIR))

local csv = probe.csv_open(OUT_DIR .. "/dma_trace.csv",
    "tick,kind,pc,packet_va,cmd,x1,y1,x2,y2,w,h")
local tick = 0
local hits = 0
local kicks = 0
local last_madr = 0
local last_madr_pc = 0
local gp0_pending = 0
local gp0_cmd = 0
local gp0_count = 0

local function reg_pc()
    local r = PCSX.getRegisters()
    return bit.band(tonumber(r.pc) or 0, 0xFFFFFFFF)
end

local REG_NAMES = { [0]="r0","at","v0","v1","a0","a1","a2","a3",
    "t0","t1","t2","t3","t4","t5","t6","t7",
    "s0","s1","s2","s3","s4","s5","s6","s7",
    "t8","t9","k0","k1","gp","sp","s8","ra" }

-- Written value of the trapping store: the Lua callback's r.pc is not
-- reliably the store's own address (hook ordering / delay slots), so decode
-- candidate instructions at pc and pc-4, keep the one that IS a sw whose
-- effective address matches the trapped register, and read its rt.
local function store_value(target)
    local r = PCSX.getRegisters()
    local pc = bit.band(tonumber(r.pc) or 0, 0xFFFFFFFF)
    for _, cand in ipairs({ pc, pc - 4, pc + 4 }) do
        local insn = mem.read_u32(cand)
        if insn ~= nil and bit.band(bit.rshift(insn, 26), 0x3F) == 0x2B then
            local base = REG_NAMES[bit.band(bit.rshift(insn, 21), 0x1F)]
            local rt = REG_NAMES[bit.band(bit.rshift(insn, 16), 0x1F)]
            local imm = bit.band(insn, 0xFFFF)
            if imm >= 0x8000 then imm = imm - 0x10000 end
            local bv = base == "r0" and 0
                or bit.band(tonumber(r.GPR.n[base]) or 0, 0xFFFFFFFF)
            local ea = bit.band(bv + imm, 0x1FFFFFFF)
            if ea == bit.band(target, 0x1FFFFFFF) then
                if rt == "r0" then return 0, pc end
                return bit.band(tonumber(r.GPR.n[rt]) or 0, 0xFFFFFFFF), pc
            end
        end
    end
    return nil, pc
end

local function log_packet(kind, pc, va, cmd, words)
    -- words = array of u32 packet words after cmd word
    local function xy(w) return bit.band(w, 0x3FF), bit.band(bit.rshift(w, 16), 0x1FF) end
    if cmd == 0xA0 then
        local x, y = xy(words[1] or 0)
        local w2 = words[2] or 0
        local w, h = bit.band(w2, 0xFFFF), bit.band(bit.rshift(w2, 16), 0xFFFF)
        -- plausibility: nonzero rect, in-range
        if w > 0 and w <= 1024 and h > 0 and h <= 512 then
            hits = hits + 1
            csv:row("%d,%s,0x%08X,0x%08X,0xA0,%d,%d,,,%d,%d",
                tick, kind, pc, va, x, y, w, h)
        end
    elseif cmd == 0x80 then
        local sx, sy = xy(words[1] or 0)
        local dx, dy = xy(words[2] or 0)
        local w3 = words[3] or 0
        local w, h = bit.band(w3, 0xFFFF), bit.band(bit.rshift(w3, 16), 0xFFFF)
        if w > 0 and w <= 1024 and h > 0 and h <= 512 then
            hits = hits + 1
            csv:row("%d,%s,0x%08X,0x%08X,0x80,%d,%d,%d,%d,%d,%d",
                tick, kind, pc, va, sx, sy, dx, dy, w, h)
        end
    end
end

-- Scan a payload word run for A0/80 packets + draw-env packets (E3
-- draw-area TL, E4 draw-area BR, E5 draw offset) that reach into the upper
-- VRAM half (y >= 256) where the texture bands live.
local function scan_words(kind, pc, base_va, count)
    local i = 0
    while i < count do
        local w = mem.read_u32(base_va + i * 4)
        if w == nil then return end
        local cmd = bit.band(bit.rshift(w, 24), 0xFF)
        if cmd == 0xA0 then
            log_packet(kind, pc, base_va + i * 4, 0xA0, {
                mem.read_u32(base_va + (i + 1) * 4),
                mem.read_u32(base_va + (i + 2) * 4) })
        elseif cmd == 0x80 then
            log_packet(kind, pc, base_va + i * 4, 0x80, {
                mem.read_u32(base_va + (i + 1) * 4),
                mem.read_u32(base_va + (i + 2) * 4),
                mem.read_u32(base_va + (i + 3) * 4) })
        elseif cmd == 0xE3 or cmd == 0xE4 then
            local x = bit.band(w, 0x3FF)
            local y = bit.band(bit.rshift(w, 10), 0x1FF)
            if y >= 256 then
                hits = hits + 1
                csv:row("%d,%s,0x%08X,0x%08X,0x%02X,%d,%d,,,,",
                    tick, kind, pc, base_va + i * 4, cmd, x, y)
            end
        elseif cmd == 0xE5 then
            local x = bit.band(w, 0x7FF)
            local y = bit.band(bit.rshift(w, 11), 0x7FF)
            -- signed 11-bit
            if x >= 0x400 then x = x - 0x800 end
            if y >= 0x400 then y = y - 0x800 end
            if y >= 256 then
                hits = hits + 1
                csv:row("%d,%s,0x%08X,0x%08X,0xE5,%d,%d,,,,",
                    tick, kind, pc, base_va + i * 4, x, y)
            end
        end
        i = i + 1
    end
end

local function walk_chain(pc, madr)
    local node = bit.bor(bit.band(madr, 0x1FFFFF), 0x80000000)
    local steps = 0
    while steps < 200000 do
        local hdr = mem.read_u32(node)
        if hdr == nil then return end
        local n = bit.band(hdr, 0xFFFFFF)
        local size = bit.band(bit.rshift(hdr, 24), 0xFF)
        if size > 0 then
            scan_words("chain", pc, node + 4, size)
        end
        if n == 0xFFFFFF then return end
        node = bit.bor(bit.band(n, 0x1FFFFF), 0x80000000)
        steps = steps + 1
    end
end

local last_mode = nil
local released = false

local mash = nil
do
    local text = probe.getenv("LEGAIA_MASH", "")
    local name, period, hold = string.match(text, "(%a+):(%d+):(%d+)")
    if name ~= nil and pad.BTN[string.upper(name)] ~= nil then
        mash = { btn = pad.BTN[string.upper(name)],
                 period = tonumber(period), hold = tonumber(hold) }
    end
end

-- Pad walk: LEGAIA_PAD_SCRIPT="60:SELECT,200:DOWN,230:DOWN,260:CROSS" (same
-- grammar as autorun_pad_walk.lua; hold defaults to 8). Default = open the
-- pause menu only.
local pad_script = {}
do
    local text = probe.getenv("LEGAIA_PAD_SCRIPT",
        string.format("%d:SELECT", SELECT_AT))
    for chunk in string.gmatch(text, "[^,]+") do
        local parts = {}
        for p in string.gmatch(chunk, "[^:]+") do parts[#parts + 1] = p end
        local at = tonumber(parts[1])
        local name = parts[2] and string.upper(parts[2])
        local hold = tonumber(parts[3] or "8")
        if at ~= nil and name ~= nil and pad.BTN[name] ~= nil then
            pad_script[#pad_script + 1] =
                { at = at, btn = pad.BTN[name], hold = hold, name = name }
        end
    end
end

probe.run({
    sstate         = SSTATE,
    capture_frames = FRAMES,
    out_path       = OUT_DIR .. "/dma_trace.csv",

    on_arm = function()
        probe.arm_breakpoint(0x1F8010A0, "Write", 4, "dma2_madr", function()
            local v, pc = store_value(0x1F8010A0)
            if v ~= nil then
                last_madr = v
                last_madr_pc = pc
            end
        end)
        -- Programmed-IO GP0 stores: a CPU-side LoadImage/MoveImage sends the
        -- A0/80 header + coords straight to 0x1F801810. Tiny state machine:
        -- when an A0/80 command word passes, log it and the following coord
        -- words with the storing PC.
        probe.arm_breakpoint(0x1F801810, "Write", 4, "gp0_pio", function()
            local v, pc = store_value(0x1F801810)
            if v == nil then return end
            gp0_count = gp0_count + 1
            if gp0_pending > 0 then
                gp0_pending = gp0_pending - 1
                hits = hits + 1
                csv:row("%d,gp0arg,0x%08X,,0x%02X,%d,%d,,,,",
                    tick, pc, gp0_cmd, bit.band(v, 0x3FF),
                    bit.band(bit.rshift(v, 16), 0x1FF))
                return
            end
            local cmd = bit.band(bit.rshift(v, 24), 0xFF)
            if cmd == 0xA0 or cmd == 0x80 then
                gp0_cmd = cmd
                gp0_pending = (cmd == 0xA0) and 2 or 3
                hits = hits + 1
                csv:row("%d,gp0cmd,0x%08X,,0x%02X,,,,,,", tick, pc, cmd)
            end
        end)
        probe.arm_breakpoint(0x1F8010A8, "Write", 4, "dma2_chcr", function()
            if tick < WIN_LO or tick > WIN_HI then return end
            local v, pc = store_value(0x1F8010A8)
            kicks = kicks + 1
            if kicks <= 5 then
                PCSX.log(string.format(
                    "[fvdma] tick=%d chcr pc=0x%08X val=%s madr=0x%08X (madr_pc=0x%08X)",
                    tick, pc, v and string.format("0x%08X", v) or "?",
                    last_madr, last_madr_pc))
            end
            local mode = v and bit.band(bit.rshift(v, 9), 3) or 2
            if mode == 2 then
                walk_chain(pc, last_madr)
            else
                -- block/request mode: scan the head of the buffer for an
                -- inline A0/80 header (LoadImage-style staging).
                local base = bit.bor(bit.band(last_madr, 0x1FFFFF), 0x80000000)
                scan_words("block", pc, base, 32)
            end
        end)
        return {}
    end,

    on_capture = function(_ctx, elapsed)
        tick = elapsed
        local mode = probe.read_u8(GAME_MODE_VA)
        local sub = probe.read_u32(0x801E46A4)
        local key = string.format("%02x/%08x", mode or 0xFF, sub or 0)
        if key ~= last_mode then
            PCSX.log(string.format("[fvdma] vsync=%d game_mode=0x%02x submenu=0x%x",
                tick, mode or 0xFF, sub or 0))
            last_mode = key
        end
        for _, s in ipairs(pad_script) do
            if tick == s.at then
                pad.force(s.btn)
                PCSX.log(string.format("[fvdma] vsync=%d press %s", tick, s.name))
            elseif tick == s.at + s.hold then
                pad.release(s.btn)
            end
        end
        -- LEGAIA_MASH="CROSS:45:6" pulses a button on a period (dialog
        -- advance) without spelling out hundreds of script steps.
        if mash ~= nil then
            local ph = tick % mash.period
            if ph == 0 then
                pad.force(mash.btn)
            elseif ph == mash.hold then
                pad.release(mash.btn)
            end
        end
    end,

    on_done = function()
        for _, s in ipairs(pad_script) do pad.release(s.btn) end
        -- Park a raw state at the end (Use list open) so the VRAM band can
        -- be compared offline against the pre-walk state.
        local sstate = require("probe.sstate")
        pcall(function() sstate.save(OUT_DIR .. "/end_state.sstate") end)
        csv:close()
        PCSX.log(string.format("=== fvariant_dma_trace: %d hit(s), %d gp0 store(s) ===",
            hits, gp0_count))
    end,
})
