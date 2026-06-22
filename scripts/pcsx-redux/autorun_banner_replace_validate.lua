-- autorun_banner_replace_validate.lua
--
-- Piece 2 (+35% cast text): validate replacing the spell-name banner with a
-- custom "+35% DMG!" string at draw time. The banner is drawn by
-- FUN_80036888(a0=string, a1=colour/style, a3=x); the banner widget has
-- a1 = 0x801C (the move-banner style) while the caster/target NAME widgets have
-- a1 = 0. So: at FUN_80036888 entry, if a1 != 0, point a0 at a custom MES string
-- we stash in a free gap. If the centered banner shows "+35% DMG!" (and the
-- names stay put), the at-draw a0 swap is the correct design.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local DRAW = 0x80036888
local STR  = 0x80078200          -- free SCUS gap (BANNER_STR_VA); scratch here
-- Plain ASCII (the real banner string has no colour escape); terminator 0.
local CUSTOM = {}
for _, c in ipairs({ string.byte("+35% DMG!", 1, -1) }) do CUSTOM[#CUSTOM + 1] = c end
CUSTOM[#CUSTOM + 1] = 0x00

local installed = false
local nlog = 0
local swapped = false
local gframe = 0
local shot = 0

local function take_fb(stem)
    local ok, ss = pcall(function() return PCSX.GPU.takeScreenShot() end)
    if not ok or ss == nil then return end
    local bpp = (tonumber(ss.bpp) or 0) > 16 and 24 or 16
    local fh = io.open(probe.out_path(stem .. ".raw"), "wb")
    if fh then fh:write(tostring(ss.data)); fh:close() end
    local mh = io.open(probe.out_path(stem .. ".meta"), "w")
    if mh then mh:write(string.format("width=%d\nheight=%d\nbpp=%d\n",
        tonumber(ss.width), tonumber(ss.height), bpp)); mh:close() end
    PCSX.log("[banner] wrote " .. stem)
end

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate7"),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 16),
    on_arm = function()
        probe.arm_breakpoint(DRAW, "Exec", 4, "draw", function()
            local r = PCSX.getRegisters()
            local a0 = (tonumber(r.GPR.n.a0) or 0) % 0x100000000
            local a1 = (tonumber(r.GPR.n.a1) or 0) % 0x100000000
            if a1 ~= 0 then
                -- banner widget: redirect the string pointer to our custom text
                r.GPR.n.a0 = STR
                swapped = true
                if nlog < 12 then
                    PCSX.log(string.format("[banner] f=%d SWAP a0 0x%08X->0x%08X (a1=0x%08X)",
                        gframe, a0, STR, a1))
                    nlog = nlog + 1
                end
            end
        end)
        return {}
    end,
    on_capture = function(ctx, elapsed)
        gframe = elapsed
        if not installed and elapsed >= 2 then
            for i, b in ipairs(CUSTOM) do probe.write_u8(STR + i - 1, b) end
            PCSX.log(string.format("[banner] custom string (%d bytes) at 0x%08X", #CUSTOM, STR))
            installed = true
        end
        -- Shoot the frame right after a swap (when the new banner is on screen).
        if swapped and shot < 3 then
            take_fb("banner_replace_" .. shot)
            shot = shot + 1
            swapped = false
        end
        if elapsed >= 50 then take_fb("banner_replace_final"); ctx.request_quit = true end
    end,
    on_summary = function() PCSX.log("[banner] done") end,
})
