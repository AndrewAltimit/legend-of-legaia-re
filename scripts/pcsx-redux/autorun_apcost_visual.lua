-- Inject one AP cost into all four command records (+0x74) of the active
-- character on the pre-build arts-input state, let the gauge build, and
-- screenshot the Arts bar. LEGAIA_COST picks the cost.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local COST = probe.getenv_num("LEGAIA_COST", 30)
local SHOTS = { 55, 130 }
local SEQ = { {5,'CROSS'}, {30,'CIRCLE'}, {62,'RIGHT'}, {76,'UP'}, {90,'LEFT'}, {104,'DOWN'} }
local function u8(a) return probe.read_u8(a) or -1 end
local function u32(a) return probe.read_u32(a) or 0 end
local function shot(name)
    local ok, ss = pcall(function() return PCSX.GPU.takeScreenShot() end)
    if ok and ss then
        local bpp = (tonumber(ss.bpp) or 0) > 16 and 24 or 16
        local h = io.open(probe.out_path(name .. ".raw"), "wb"); h:write(tostring(ss.data)); h:close()
        local m = io.open(probe.out_path(name .. ".meta"), "w")
        m:write(string.format("width=%d\nheight=%d\nbpp=%d\n", tonumber(ss.width), tonumber(ss.height), bpp)); m:close()
    else PCSX.log("[shot] failed: " .. tostring(ss)) end
end
local function gauge()
    local s = ""
    for i = 0, 3 do
        local b = 0x80076C10 + 0x288 + i * 0x18
        s = s .. string.format("[x=%d y=%d w=%d h=%d] ", probe.read_u16 and probe.read_u16(b+2) or -1,
            probe.read_u16 and probe.read_u16(b+0xa) or -1, probe.read_u16 and probe.read_u16(b+6) or -1,
            probe.read_u16 and probe.read_u16(b+8) or -1)
    end
    return s
end
probe.run({
    sstate = SSTATE_PATH, capture_frames = 140,
    on_arm = function() return {} end,
    on_capture = function(ctx, elapsed)
        if elapsed == 1 then
            local active = u8(u32(0x8007BD24) + 0x13)
            local base = u32(0x801C9360 + active * 4)
            local line = string.format("== cost inject %d == active=%d base=0x%08X", COST, active, base)
            for i = 0, 3 do
                local cmd = u8(0x801F4B8C + i)
                local p = u32(base + cmd * 4)
                if p ~= 0 then
                    line = line .. string.format(" cmd%02X:%d->", cmd, u8(p + 0x74))
                    probe.write_u8(p + 0x74, COST)
                    line = line .. string.format("%d", u8(p + 0x74))
                end
            end
            PCSX.log(line)
        end
        for _, b in ipairs({probe.BTN.UP,probe.BTN.DOWN,probe.BTN.LEFT,probe.BTN.RIGHT,probe.BTN.CROSS,probe.BTN.CIRCLE}) do probe.pad_release(b) end
        for _, e in ipairs(SEQ) do if elapsed >= e[1] and elapsed < e[1] + 4 then probe.pad_force(probe.BTN[e[2]]) end end
        if elapsed == 54 or elapsed == 129 then
            local ctx0 = u32(0x8007BD24)
            PCSX.log(string.format("[gauge t%d] built costs=%d,%d,%d,%d gmode=0x%02X %s", elapsed,
                u8(ctx0+0x14), u8(ctx0+0x15), u8(ctx0+0x16), u8(ctx0+0x17), u8(0x8007B83C), gauge()))
        end
        for _, t in ipairs(SHOTS) do if elapsed == t then shot(string.format("cost%02d_t%d", COST, t)) end end
        if elapsed == SHOTS[#SHOTS] + 1 then ctx.request_quit = true end
    end,
})
