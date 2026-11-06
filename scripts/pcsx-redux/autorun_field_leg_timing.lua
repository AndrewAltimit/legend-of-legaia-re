-- autorun_field_leg_timing.lua
--
-- When a scripted walk leg lands, when its halt bit clears and when the
-- script parked on it moves on: one row per field actor per vsync, with the
-- walk kernel's inputs (the halt bit +0x10 & 0x400, the parked pointer
-- +0x94, the leg cursor +0x54), the actor's own script cursor (+0x90 base,
-- +0x9E pc) and the actor-tick counter 0x801C6ED0 that FUN_8003BC08 bumps on
-- every call, which marks the game frames. Poll-only, safe under --fast.
--
-- Env: LEGAIA_SSTATE, LEGAIA_FRAMES, LEGAIA_OUT_DIR,
--      LEGAIA_CROSS_EVERY  tap Cross for 4 vsyncs every N vsyncs (0 = never),
--                          to page the dialogue boxes a cutscene waits on.
-- Output: leg_timing.csv.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 900)
local CROSS_EVERY = probe.getenv_num("LEGAIA_CROSS_EVERY", 0)

local PLAYER_PTR = 0x8007C364
local TICKS = 0x801C6ED0
local FIELD_TICK = 0x8003BC08

local function s16(a)
    local v = probe.read_u16(a)
    if v >= 0x8000 then v = v - 0x10000 end
    return v
end

local fh = assert(io.open(probe.out_path("leg_timing.csv"), "w"))
fh:write("vsync,ticks,step,actor,flat,player,x,z,heading,flags,park,cursor,base,pc,op\n")

local function op_bytes(a)
    if not probe.in_ram(a) then return "" end
    local t = {}
    for i = 0, 5 do t[#t + 1] = string.format("%02X", probe.read_u8(a + i)) end
    return table.concat(t)
end

local function row(el, n, is_player)
    local park = probe.read_u32(n + 0x94)
    fh:write(string.format("%d,%d,%d,%08X,%d,%d,%d,%d,%d,%08X,%08X,%d,%08X,%d,%s\n",
        el, probe.read_u32(TICKS), probe.read_scratch_u8(0x1F800393), n,
        probe.read_u16(n + 0x50), is_player and 1 or 0,
        s16(n + 0x14), s16(n + 0x18), s16(n + 0x26),
        probe.read_u32(n + 0x10), probe.read_u32(n + 0x94), s16(n + 0x54),
        probe.read_u32(n + 0x90), s16(n + 0x9E), op_bytes(park)))
end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function()
        probe.env.write_manifest("autorun_field_leg_timing.lua",
            { sstate = SSTATE, frames = FRAMES })
        return {}
    end,
    on_capture = function(_, el)
        if CROSS_EVERY > 0 then
            if el % CROSS_EVERY == 0 then probe.pad_force(probe.BTN.CROSS) end
            if el % CROSS_EVERY == 4 then probe.pad_release(probe.BTN.CROSS) end
        end
        local p = probe.read_u32(PLAYER_PTR)
        if p ~= nil and probe.in_ram(p) then row(el, p, true) end
        local seen = {}
        for k = 0, 8 do
            local n, count = probe.read_u32(0x8007C34C + 4 * k), 0
            while probe.in_ram(n) and count < 512 and not seen[n] do
                seen[n] = true
                count = count + 1
                if n ~= p and probe.read_u32(n + 0x0C) == FIELD_TICK then row(el, n, false) end
                n = probe.read_u32(n)
            end
        end
        fh:flush()
    end,
})
