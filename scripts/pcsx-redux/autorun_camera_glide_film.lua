-- autorun_camera_glide_film.lua
--
-- Per-vsync film of the field camera globals and the op-0x45 camera mover
-- (`FUN_801DC0BC`): the ten globals the view builder `FUN_800172C0` reads
-- (pitch / yaw / roll `0x8007B790..94`, the eye trio `0x800840B8/BC/C0`, the
-- focus trio `0x80089118/1C/20`, `H` `0x8007B6F4`), the frame step
-- `DAT_1F800393`, and the live mover node's progress `+0x9C`, duration
-- `+0x9E` and curve `+0x50`. On the first vsync the mover node is found it
-- also dumps the node's first 0xA0 bytes and the 40-byte (start, end) pair
-- block every pointer word in it names, so a glide's staging can be read
-- off retail. Answers "does the port's glide stand where retail's stands on
-- every game frame" for a scene-entry glide (`izumi`'s arrival).
--
--   timeout --kill-after=30s 600s bash scripts/pcsx-redux/run_probe.sh \
--       --headless --lua scripts/pcsx-redux/autorun_camera_glide_film.lua \
--       --scenario izumi_arrival_revisit --frames 240
--
-- Env: LEGAIA_SSTATE, LEGAIA_FRAMES (default 240), LEGAIA_OUT_DIR.
-- Output: camera_glide_film.csv (one row a vsync), camera_glide_mover.txt.
-- Pure RAM observation, no pad input.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 240)
local MOVER_TICK = 0x801DC0BC

local function u8(a) return probe.mem.read_u8(a) or 0 end
local function u16(a) return probe.mem.read_u16(a) or 0 end
local function u32(a) return probe.mem.read_u32(a) or 0 end
local function s16(a)
    local v = u16(a)
    if v >= 0x8000 then v = v - 0x10000 end
    return v
end
local function s32(a)
    local v = u32(a)
    if v >= 0x80000000 then v = v - 0x100000000 end
    return v
end

local function label()
    local s = ""
    for i = 0, 7 do
        local c = u8(0x8007050C + i)
        if c == 0 then break end
        s = s .. string.char(c)
    end
    return s
end

local function find_mover()
    local seen = {}
    for k = 0, 8 do
        local p = u32(0x8007C34C + 4 * k)
        local n = 0
        while probe.in_ram(p) and not seen[p] and n < 512 do
            seen[p] = true
            if u32(p + 0x0C) == MOVER_TICK and bit.band(u32(p + 0x10), 8) == 0 then
                return p
            end
            p = u32(p)
            n = n + 1
        end
    end
    return nil
end

local CSV = io.open(probe.out_path("camera_glide_film.csv"), "w")
CSV:write("vsync,label,step393,pitch,yaw,roll,eye_x,eye_y,eye_z,focus_x,focus_y,focus_z,h,"
    .. "mover,mover_t,mover_d,mover_curve\n")
local DUMP = io.open(probe.out_path("camera_glide_mover.txt"), "w")
local dumped = {}

local function dump(node, e)
    if dumped[node] then return end
    dumped[node] = true
    DUMP:write(string.format("vsync %d mover node 0x%08X\n", e, node))
    for off = 0, 0x9C, 4 do
        DUMP:write(string.format("  +0x%02X 0x%08X\n", off, u32(node + off)))
    end
    for off = 0, 0x9C, 4 do
        local q = u32(node + off)
        if probe.in_ram(q) and q ~= node then
            DUMP:write(string.format("  block at +0x%02X -> 0x%08X:", off, q))
            for i = 0, 9 do
                DUMP:write(string.format(" (%d,%d)", s16(q + 4 * i), s16(q + 4 * i + 2)))
            end
            DUMP:write("\n")
        end
    end
    DUMP:flush()
end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function()
        probe.env.write_manifest("autorun_camera_glide_film.lua",
            { sstate = SSTATE, frames = FRAMES })
        return {}
    end,
    on_capture = function(_, e)
        local m = find_mover()
        local mt, md, mc = -1, -1, -1
        if m then
            mt, md, mc = s16(m + 0x9C), s16(m + 0x9E), s16(m + 0x50)
            dump(m, e)
        end
        CSV:write(string.format("%d,%s,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%s,%d,%d,%d\n",
            e, label(), probe.mem.read_scratch_u8(0x1F800393) or -1,
            s16(0x8007B790), s16(0x8007B792), s16(0x8007B794),
            s32(0x800840B8), s32(0x800840BC), s32(0x800840C0),
            s32(0x80089118), s32(0x8008911C), s32(0x80089120),
            s32(0x8007B6F4),
            m and string.format("0x%08X", m) or "-", mt, md, mc))
        if e % 60 == 0 then CSV:flush() end
    end,
    on_done = function()
        CSV:close()
        DUMP:close()
        PCSX.log("[camera_glide_film] done")
    end,
})
