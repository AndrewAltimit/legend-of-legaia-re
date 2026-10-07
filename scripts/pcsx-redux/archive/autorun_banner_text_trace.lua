-- autorun_banner_text_trace.lua
--
-- Piece 2 (+35% cast text): find the REAL "Burning Attack" banner draw. The
-- glyph renderer FUN_80036888(a0=string ptr, a1=x?, a2=?, a3=?) draws every
-- on-screen string. Breakpoint its entry on a mid-cast state and log, per call,
-- the string pointer + the first bytes at it (ASCII-ish; the font maps 0x20..0x7E
-- to glyphs) + the arg registers. The row whose bytes spell "Burning Attack" is
-- the banner; its pointer/source is what a +35% feature must redirect or augment.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local DRAW = 0x80036888
local OUT = probe.out_path("banner_text_trace.txt")
local f = assert(io.open(OUT, "w"))
local seen = {}

local function str_at(p, n)
    local hex, asc = {}, {}
    for i = 0, n - 1 do
        local b = probe.read_u8(p + i) or 0
        hex[#hex + 1] = string.format("%02X", b)
        asc[#asc + 1] = (b >= 0x20 and b < 0x7F) and string.char(b) or "."
        if b == 0 then break end
    end
    return table.concat(hex, " "), table.concat(asc)
end

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate7"),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 20),
    on_arm = function()
        probe.arm_breakpoint(DRAW, "Exec", 4, "draw", function()
            local r = PCSX.getRegisters()
            local a0 = (tonumber(r.GPR.n.a0) or 0) % 0x100000000
            local a1 = (tonumber(r.GPR.n.a1) or 0) % 0x100000000
            local a2 = (tonumber(r.GPR.n.a2) or 0) % 0x100000000
            local a3 = (tonumber(r.GPR.n.a3) or 0) % 0x100000000
            local ra = (tonumber(r.GPR.n.ra) or 0) % 0x100000000
            local key = string.format("%08X", a0)
            if seen[key] then return end
            seen[key] = true
            if a0 >= 0x80000000 and a0 < 0x80200000 then
                local hex, asc = str_at(a0, 20)
                local s4 = (tonumber(r.GPR.n.s4) or 0) % 0x100000000
                local w = {}
                if s4 >= 0x80000000 and s4 < 0x80200000 then
                    for o = 0x10, 0x24, 2 do
                        w[#w + 1] = string.format("+%02X=%04X", o, probe.read_u16(s4 + o) or 0)
                    end
                end
                f:write(string.format("a0=0x%08X a1=0x%08X a3=0x%08X ra=0x%08X s4=0x%08X | \"%s\"\n    %s\n",
                    a0, a1, a3, ra, s4, asc, table.concat(w, " ")))
                f:flush()
            end
        end)
        return {}
    end,
    on_capture = function(ctx, elapsed)
        if elapsed >= probe.getenv_num("LEGAIA_FRAMES", 20) - 2 then ctx.request_quit = true end
    end,
    on_summary = function() f:write("done\n"); f:close() end,
})
