-- autorun_drawn_pose_film.lua
--
-- Film which pose and display trio each battle pass draws, beside the
-- double buffer's flip and the damage numeral's own reads.
--
-- A battle capture's screen is a pass or two older than its RAM, and three
-- residuals of the image comparison (the hit-spark numeral's anchor, Zeto's
-- Big Wave idle, Horn's creature blades) turn on which pass's pose a frame
-- drew. This probe logs, in execution order:
--
--   flip  <vsync> idx <gp+0x434>            frame-end flip (0x800171E4)
--   tick  <vsync> node <n> slot <s> cur <c> anim tick FUN_80047430 entry
--   dec   <vsync> node <n> slot <s> cur <c> entry <e>
--                                           pose decode FUN_8004998C entry
--   trio  <vsync> actor <a> pos x y z trio x y z
--                                           the decode's trio store done
--                                           (0x8004A5FC)
--   num   <vsync> actor <a> trio x y z timer <t>
--                                           the numeral FUN_801DF6B8 reads
--                                           the struck actor's trio (0x801DF758)
--   numxy <vsync> x0 y0 x1 y1 x2 y2 x3 y3    the projected square (0x801DF858)
--   vs    <vsync> step <s> <slot>:<actor>:<clip>:<staged>:<rate>:<ring cur 0..3> ...
--                                           one line a vsync
--
-- plus `shot_<vsync>.raw` per vsync from LEGAIA_SHOT_FROM. Breakpoints:
-- run with the interpreter (no --fast).
--
-- Env:
--   LEGAIA_SSTATE    save state (or --scenario on the runner)
--   LEGAIA_FRAMES    vsyncs to film (default 240)
--   LEGAIA_SHOT_FROM first vsync to screenshot (default 0; -1 = none)
--   LEGAIA_POKE32    optional `addr=value` poked once after load (or at the
--                    inject, with LEGAIA_SPELL)
--   LEGAIA_SPELL     optional action id to force into seat 0's queue at its
--                    action state 0x0A (the cast-sequence probe's inject:
--                    +0x1DE = 2, +0x1DF = spell, +0x1DD = 3, MP 999), tapping
--                    CROSS until it lands; lines are written from the inject
--
-- Usage:
--   bash scripts/pcsx-redux/run_probe.sh --iso <scratch copy of the disc> \
--       --scenario rim_elm_gimard_victory \
--       --lua scripts/pcsx-redux/autorun_drawn_pose_film.lua --frames 240

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local mem = require("probe.mem")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 240)
local SHOT_FROM = probe.getenv_num("LEGAIA_SHOT_FROM", 0)
local POKE = probe.getenv("LEGAIA_POKE32", "")
local SPELL = probe.getenv_num("LEGAIA_SPELL", -1)
local injected = SPELL < 0

local ACTORS = 0x801C9370
local CTX_PTR = 0x8007BD24
local DB_INDEX = 0x8007B74C
local STEP = 0x1F800393

local out = io.open(probe.out_path("film.txt"), "w")
local vsync = -1

local function s16(v)
    if v >= 0x8000 then return v - 0x10000 end
    return v
end
local function rd16(a) return s16(probe.read_u16(a) or 0) end
local function reg(r, name)
    return bit.band(tonumber(r.GPR.n[name]) or 0, 0xFFFFFFFF)
end
local function emit(s)
    if injected then out:write(s .. "\n") end
end

local function screenshot(stem)
    local ok, ss = pcall(PCSX.GPU.takeScreenShot)
    if not ok or ss == nil then return end
    local fh = io.open(probe.out_path(stem .. ".raw"), "wb")
    if fh == nil then return end
    fh:write(tostring(ss.data)); fh:close()
    local mh = io.open(probe.out_path(stem .. ".raw.meta"), "w")
    if mh then
        mh:write(string.format("width=%d\nheight=%d\nbpp=%d\n",
            ss.width or 320, ss.height or 240,
            (ss.bpp == "BPP_24") and 24 or 16))
        mh:close()
    end
end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function()
        probe.env.write_manifest("autorun_drawn_pose_film.lua",
            { sstate = SSTATE, frames = FRAMES, poke = POKE })
        local pa, pv = POKE:match("^(0x%x+)=(%d+)$")
        if pa ~= nil and SPELL < 0 then
            mem.write_u32(tonumber(pa), tonumber(pv))
        end
        probe.arm_breakpoint(0x800171E4, "Exec", 4, "flip", function()
            emit(string.format("flip %d idx %d", vsync, probe.read_u16(DB_INDEX) or 0))
        end)
        probe.arm_breakpoint(0x80047430, "Exec", 4, "tick", function()
            local r = PCSX.getRegisters()
            local n = reg(r, "a0")
            emit(string.format("tick %d node %08X slot %d cur %d", vsync, n,
                rd16(n + 0x5A), probe.read_u16(n + 0x68) or 0))
        end)
        probe.arm_breakpoint(0x8004998C, "Exec", 4, "dec", function()
            local r = PCSX.getRegisters()
            local n = reg(r, "a0")
            emit(string.format("dec %d node %08X slot %d cur %d entry %08X", vsync, n,
                rd16(n + 0x5A), probe.read_u16(n + 0x68) or 0, probe.read_u32(n + 0x4C) or 0))
        end)
        probe.arm_breakpoint(0x8004A5FC, "Exec", 4, "trio", function()
            local r = PCSX.getRegisters()
            local a = reg(r, "s3")
            emit(string.format("trio %d actor %08X pos %d %d %d trio %d %d %d", vsync, a,
                rd16(a + 0x34), rd16(a + 0x36), rd16(a + 0x38),
                rd16(a + 0x3C), rd16(a + 0x3E), rd16(a + 0x40)))
        end)
        probe.arm_breakpoint(0x801DF758, "Exec", 4, "num", function()
            local r = PCSX.getRegisters()
            local a = reg(r, "v0")
            local e = reg(r, "a2")
            emit(string.format("num %d actor %08X trio %d %d %d timer %d", vsync, a,
                rd16(a + 0x3C), rd16(a + 0x3E), rd16(a + 0x40),
                probe.read_u32(e + 0x85C) or 0))
        end)
        probe.arm_breakpoint(0x801DF858, "Exec", 4, "numxy", function()
            local r = PCSX.getRegisters()
            local p = reg(r, "s1")
            emit(string.format("numxy %d %d %d %d %d %d %d %d %d", vsync,
                rd16(p + 0x8), rd16(p + 0xA), rd16(p + 0x10), rd16(p + 0x12),
                rd16(p + 0x18), rd16(p + 0x1A), rd16(p + 0x20), rd16(p + 0x22)))
        end)
        return {}
    end,
    on_capture = function(_, el)
        vsync = el
        if not injected then
            probe.pad_release(probe.BTN.CROSS)
            local cx = probe.read_u32(CTX_PTR)
            local a0 = probe.read_u32(ACTORS)
            if cx ~= nil and probe.in_ram(cx) and a0 ~= nil and probe.in_ram(a0) then
                if probe.read_u8(cx + 7) == 0x0A and probe.read_u8(cx + 0x13) == 0 then
                    mem.write_u16(a0 + 0x150, 999)
                    mem.write_u8(a0 + 0x1DE, 2)
                    mem.write_u8(a0 + 0x1DF, SPELL)
                    mem.write_u8(a0 + 0x1DD, 3)
                    local pa, pv = POKE:match("^(0x%x+)=(%d+)$")
                    if pa ~= nil then mem.write_u32(tonumber(pa), tonumber(pv)) end
                    injected = true
                    PCSX.log(string.format("[film] inject 0x%02X at vsync %d", SPELL, el))
                elseif el % 60 >= 30 and el % 60 < 34 then
                    probe.pad_force(probe.BTN.CROSS)
                end
            end
            return
        end
        local cx = probe.read_u32(CTX_PTR) or 0
        local st7 = probe.in_ram(cx) and probe.read_u8(cx + 7) or 0
        local parts = { string.format("vs %d step %d st7 %d", el,
            mem.read_scratch_u8(STEP) or 0, st7) }
        for s = 0, 7 do
            local a = probe.read_u32(ACTORS + s * 4)
            if a ~= nil and probe.in_ram(a) then
                parts[#parts + 1] = string.format("%d:%08X:%d:%d:%d:%d,%d,%d,%d", s, a,
                    probe.read_u8(a + 0x1D9), probe.read_u8(a + 0x1DA),
                    probe.read_u8(a + 0x21D),
                    probe.read_u16(a + 0x17A), probe.read_u16(a + 0x17C),
                    probe.read_u16(a + 0x17E), probe.read_u16(a + 0x180))
            end
        end
        emit(table.concat(parts, " "))
        out:flush()
        if SHOT_FROM >= 0 and el >= SHOT_FROM then
            screenshot(string.format("shot_%05d", el))
        end
    end,
    on_done = function()
        out:close()
    end,
})
