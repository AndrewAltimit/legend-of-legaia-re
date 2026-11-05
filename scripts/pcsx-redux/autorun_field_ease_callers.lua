-- autorun_field_ease_callers.lua
--
-- Who runs the field follow ease, and on what stationary-test words? An exec
-- breakpoint at FUN_801DB510 and its compose call log the caller, the gates
-- (0x8007B606, 0x1F800394) and the player's position pairs, under a pad
-- schedule. Needs the interpreter (no --fast).
--
-- Env: LEGAIA_SSTATE, LEGAIA_FRAMES, LEGAIA_OUT_DIR, LEGAIA_PAD_SCRIPT (A-B:BTN).
-- Output: ease_callers.csv.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 120)
local SCRIPT = probe.getenv("LEGAIA_PAD_SCRIPT", "")
local spans = {}
for item in string.gmatch(SCRIPT, "[^,]+") do
    local a, b, name = string.match(item, "^%s*(%d+)-(%d+):(%a+)%s*$")
    local btn = name and probe.BTN[string.upper(name)]
    if a and btn then spans[#spans + 1] = { from = tonumber(a), to = tonumber(b), btn = btn } end
end
local held, vs = {}, 0
local fh = assert(io.open(probe.out_path("ease_callers.csv"), "w"))
fh:write("vsync,event,ra,b606,b607,story,x,y,z,px,py,pz,flags10\n")

local function log(ev)
    local r = PCSX.getRegisters()
    local p = probe.read_u32(0x8007C364)
    if not probe.in_ram(p) then return end
    fh:write(string.format("%d,%s,%08X,%02X,%02X,%08X,%d,%d,%d,%d,%d,%d,%08X\n", vs, ev,
        tonumber(r.GPR.n.ra), probe.read_u8(0x8007B606), probe.read_u8(0x8007B607),
        probe.read_scratch_u32(0x394),
        probe.read_u16(p + 0x14), probe.read_u16(p + 0x16), probe.read_u16(p + 0x18),
        probe.read_u16(p + 0x1C), probe.read_u16(p + 0x1E), probe.read_u16(p + 0x20),
        probe.read_u32(p + 0x10)))
    fh:flush()
end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function()
        probe.arm_breakpoint(0x801DB510, "Exec", 4, "ease", function() log("ease") end)
        probe.arm_breakpoint(0x801DB5B4, "Exec", 4, "compose", function() log("compose") end)
        probe.arm_breakpoint(0x801DB820, "Exec", 4, "pin", function() log("pin") end)
        probe.arm_breakpoint(0x801D01B0, "Exec", 4, "walk", function() log("walk") end)
        probe.arm_breakpoint(0x80089120, "Write", 4, "focus_write", function()
            local r = PCSX.getRegisters()
            log(string.format("focus_w_pc_%08X", tonumber(r.pc)))
        end)
        return {}
    end,
    on_capture = function(_, el)
        vs = el
        local want = {}
        for _, s in ipairs(spans) do
            if el >= s.from and el < s.to then want[s.btn] = true end
        end
        for btn, _ in pairs(held) do
            if not want[btn] then probe.pad_release(btn); held[btn] = nil end
        end
        for btn, _ in pairs(want) do
            if not held[btn] then probe.pad_force(btn); held[btn] = true end
        end
    end,
})
