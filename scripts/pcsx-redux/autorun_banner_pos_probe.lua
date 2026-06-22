-- autorun_banner_pos_probe.lua
--
-- Calibrate the screen position for the "+35% DMG!" cast text so it lands inside
-- the empty top HUD box (the user wants it there, not mid-screen). The banner is
-- drawn by FUN_80036888 with X = a3 and Y = caller's 0x10(sp) (the per-line pen
-- start). The banner-widget call has a1 = 0x801C (names use a1 = 0). This probe,
-- for the banner widget only: reads the current (X, Y), then forces a candidate
-- (X, Y) + the custom string, and screenshots so we can dial in the top box.
--
-- Vars: LEGAIA_BANNER_X / LEGAIA_BANNER_Y (decimal). Default aims at the top box.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local DRAW = 0x80036888
local STR  = 0x80078200
local NEWX = probe.getenv_num("LEGAIA_BANNER_X", 0x85)
local NEWY = probe.getenv_num("LEGAIA_BANNER_Y", 0x10)
local CUSTOM = {}
for _, c in ipairs({ string.byte("+35% DMG!", 1, -1) }) do CUSTOM[#CUSTOM + 1] = c end
CUSTOM[#CUSTOM + 1] = 0x00

local installed, logged, swapped, shot = false, 0, false, 0

local function take_fb(stem)
    local ok, ss = pcall(function() return PCSX.GPU.takeShot and PCSX.GPU.takeShot() or PCSX.GPU.takeScreenShot() end)
    if not ok or ss == nil then return end
    local bpp = (tonumber(ss.bpp) or 0) > 16 and 24 or 16
    local fh = io.open(probe.out_path(stem .. ".raw"), "wb")
    if fh then fh:write(tostring(ss.data)); fh:close() end
    local mh = io.open(probe.out_path(stem .. ".meta"), "w")
    if mh then mh:write(string.format("width=%d\nheight=%d\nbpp=%d\n",
        tonumber(ss.width), tonumber(ss.height), bpp)); mh:close() end
end

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate7"),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 52),
    on_arm = function()
        probe.arm_breakpoint(DRAW, "Exec", 4, "draw", function()
            local r = PCSX.getRegisters()
            local a1 = (tonumber(r.GPR.n.a1) or 0) % 0x100000000
            if a1 == 0 then return end                        -- name widget; leave it
            local sp = (tonumber(r.GPR.n.sp) or 0) % 0x100000000
            local oldx = (tonumber(r.GPR.n.a3) or 0) % 0x100000000
            local oldy = probe.read_u32(sp + 0x10) or 0
            if logged < 4 then
                PCSX.log(string.format("[pos] banner draw: a3(X)=%d sp=0x%08X Y@0x10(sp)=%d -> new (%d,%d)",
                    oldx, sp, oldy, NEWX, NEWY))
                logged = logged + 1
            end
            r.GPR.n.a0 = STR
            r.GPR.n.a3 = NEWX
            probe.write_u16(sp + 0x10, NEWY)      -- Y (low half; high half stays 0)
            probe.write_u16(sp + 0x12, 0)
            swapped = true
        end)
        return {}
    end,
    on_capture = function(ctx, elapsed)
        if not installed and elapsed >= 2 then
            for i, b in ipairs(CUSTOM) do probe.write_u8(STR + i - 1, b) end
            installed = true
        end
        -- Shoot the vsync AFTER a banner override (the new prim is on screen).
        if swapped then
            take_fb("banner_pos_" .. shot)
            PCSX.log(string.format("[pos] shot %d at frame %d", shot, elapsed))
            shot = shot + 1
            swapped = false
            if shot >= 4 then ctx.request_quit = true end
        end
        if elapsed >= 50 then ctx.request_quit = true end
    end,
    on_summary = function() PCSX.log("[pos] done") end,
})
