-- autorun_delilas_dome_course.lua
--
-- Drive the REAL Muscle Dome course flow on a patched Delilas disc, from a
-- plain field save: poke the arena warp (`_DAT_8007BA34 = 5`, game mode
-- 0x18 - the koin1 `3E 69` op's effect), install the SCUS-side cave
-- routines + hooks in RAM (the save state predates the patch; the overlay
-- half rides the --iso disc), and replace the seed routine with a poked
-- always-seed stub so the course starts without the story-flag machinery.
-- Then mash Cross through the hub and watch the round-0 battle come up.
--
-- Reproduces what the field-staged battle_load probe cannot: the dome's own
-- pre-menu round startup, where live testing freezes. Instruments:
--  - mode-transition CSV rows (a freeze stops vsync -> rows stop),
--  - malloc entry/failure breakpoints (OOM class),
--  - an exec breakpoint on the general exception vector 0x80000080
--    (wild-pointer class - fires even when vsync is dead).
--
-- Launch:
--   LEGAIA_POKES="..." LEGAIA_FRAMES=3600 \
--   bash scripts/pcsx-redux/run_probe.sh --iso patched.bin \
--     --scenario field_walled_collision_pin \
--     --lua scripts/pcsx-redux/autorun_delilas_dome_course.lua
--
-- Output: delilas_dome_course.csv  tick,mode,note

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad = require("probe.pad")

local GAME_MODE   = 0x8007B83C
local WARP_SUB    = 0x8007BA34
local COURSE_WORD = 0x8007BAC0
local FORMATION   = 0x8007BD0C
local HEAP_DESC   = 0x8007BB58
local MALLOC_ENTRY = 0x80017888
local MALLOC_FAIL  = 0x800178B8
local EXC_VECTOR   = 0x80000080

local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 3600)
-- Optional: force a second enemy seat (formation cell 1) right after the
-- installer runs (mode 0x14), making any course's round a 1v2. Used to
-- discriminate "two-enemy dome round" from "slim clones" as the park cause.
local SECOND = probe.getenv_num("LEGAIA_SECOND_SEAT", 0)
local FIRST = probe.getenv_num("LEGAIA_FIRST_SEAT", 0)
local FORCE_AT = probe.getenv_num("LEGAIA_FORCE_AT", 120)
local POKES_RAW = probe.getenv("LEGAIA_POKES", "")

-- "addr:word" pokes a u32; "addr:val:b" pokes a single byte.
local pokes = {}
for pair in string.gmatch(POKES_RAW, "[^,%s]+") do
    local a, v, w = string.match(pair, "([^:]+):([^:]+):?(.*)")
    if a and v then
        pokes[#pokes + 1] = { addr = tonumber(a), val = tonumber(v), byte = (w == "b") }
    end
end

local CSV = probe.csv_open(probe.out_path("delilas_dome_course.csv"),
    "tick,mode,note")

local function reg(r, name)
    local ok, v = pcall(function() return tonumber(r.GPR.n[name]) % 0x100000000 end)
    if ok then return v end
    return 0
end

local function heap_report(tag)
    local desc = probe.read_u32(HEAP_DESC)
    if desc == nil or desc == 0 then return end
    local top = probe.read_u32(desc) or 0
    local sent = top + 0xC
    local node = probe.read_u32(top + 0x10)
    local total, n = 0, 0
    while node ~= nil and node ~= sent and n < 64 do
        total = total + (probe.read_u32(node + 8) or 0)
        n = n + 1
        node = probe.read_u32(node + 4)
    end
    CSV:row("0,0,%s free=0x%X nodes=%d", tag, total, n)
end

local installed = false
local last_mode = -1
local exc_logged = 0

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE", ""),
    capture_frames = FRAMES,
    on_arm = function(ctx)
        probe.arm_breakpoint(MALLOC_ENTRY, "Exec", 4, "malloc", function()
            if not installed then return end
            local r = PCSX.getRegisters()
            CSV:row("0,0,malloc size=0x%X ra=0x%08X", reg(r, "a1"), reg(r, "ra"))
        end)
        probe.arm_breakpoint(MALLOC_FAIL, "Exec", 4, "malloc_fail", function()
            local r = PCSX.getRegisters()
            CSV:row("0,0,MALLOC FAILED size=0x%X", reg(r, "s1"))
            heap_report("at-fail")
        end)
        -- The general exception vector: interrupts land here constantly, so
        -- only log hits whose cause is NOT an interrupt (ExcCode != 0).
        probe.arm_breakpoint(EXC_VECTOR, "Exec", 4, "exception", function()
            if exc_logged >= 16 then return end
            local r = PCSX.getRegisters()
            local ok, cause = pcall(function() return tonumber(r.CP0.n.cause) % 0x100000000 end)
            local ok2, epc = pcall(function() return tonumber(r.CP0.n.epc) % 0x100000000 end)
            local code = ok and math.floor(cause / 4) % 32 or -1
            if code == 0 then return end -- interrupt: normal traffic
            exc_logged = exc_logged + 1
            CSV:row("0,0,EXCEPTION code=%d epc=0x%08X ra=0x%08X", code,
                ok2 and epc or 0, reg(r, "ra"))
        end)
        return {}
    end,
    on_capture = function(ctx, elapsed)
        local mode = probe.read_u16(GAME_MODE) or -1
        if elapsed == FORCE_AT then
            for _, p in ipairs(pokes) do
                if p.byte then
                    probe.write_u8(p.addr, p.val)
                else
                    probe.write_u32(p.addr, p.val)
                end
            end
            CSV:row("%d,0x%X,pokes installed (%d)", elapsed, mode, #pokes)
            probe.write_u32(WARP_SUB, 5)
            probe.write_u32(COURSE_WORD, 0)
            probe.write_u16(GAME_MODE, 0x18)
            installed = true
            CSV:row("%d,0x%X,warp to arena forced", elapsed, mode)
            return
        end
        if mode ~= last_mode then
            if mode == 0x14 and SECOND ~= 0 then
                probe.write_u8(FORMATION + 1, SECOND)
                CSV:row("%d,0x%X,second seat forced = %d", elapsed, mode, SECOND)
            end
            if mode == 0x14 and FIRST ~= 0 then
                probe.write_u8(FORMATION, FIRST)
                CSV:row("%d,0x%X,first seat forced = %d", elapsed, mode, FIRST)
            end
            CSV:row("%d,0x%X,mode-change word=0x%X cells=%02X %02X %02X %02X",
                elapsed, mode, probe.read_u32(COURSE_WORD) or 0,
                probe.read_u8(FORMATION) or 0, probe.read_u8(FORMATION + 1) or 0,
                probe.read_u8(FORMATION + 2) or 0, probe.read_u8(FORMATION + 3) or 0)
            last_mode = mode
        end
        -- Mash Cross through hub prompts / the battle intro.
        if installed then
            local phase = elapsed % 40
            if phase == 0 then
                pad.force(pad.BTN.CROSS)
            elseif phase == 6 then
                pad.release(pad.BTN.CROSS)
            end
            if elapsed % 600 == 0 then
                CSV:row("%d,0x%X,tick word=0x%X", elapsed, mode,
                    probe.read_u32(COURSE_WORD) or 0)
                heap_report("tick")
            end
        end
    end,
    on_done = function()
        CSV:row("0,0x%X,done (last mode)", last_mode)
        heap_report("done")
        -- Save a state so the framebuffer can be inspected offline (did the
        -- run actually reach the command menu?).
        pcall(function()
            local sstate = require("probe.sstate")
            sstate.save(probe.out_path("dome_course_end.sstate"))
        end)
        CSV:close()
    end,
})
