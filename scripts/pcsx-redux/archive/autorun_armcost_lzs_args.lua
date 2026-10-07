-- Capture the LZS-decode call (entry 0x8001a6a4, ra=0x80051760) that produces the
-- arm-cost section: its src (compressed input) and dst (output) pointers, so the
-- cost byte 0x800E3860 maps to an offset within the decompressed section and the
-- src maps back to the player battle file.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 2400)
local function tou32(v) v=tonumber(v) or 0 if v<0 then v=v+0x100000000 end return v end
local function u8(a) return probe.read_u8(a) or -1 end
local GMODE=0x8007B83C
local seen=0
probe.run({
    sstate=SSTATE_PATH, capture_frames=FRAMES,
    on_arm=function()
        PCSX.log("== LZS-decode args for arm-cost section ==")
        probe.arm_breakpoint(0x8001A6A4, "Exec", 4, "lzs_entry", function()
            local r=PCSX.getRegisters(); local n=r.GPR and r.GPR.n
            if not n then return end
            local ra=tou32(n.ra)
            if ra ~= 0x80051760 then return end
            seen=seen+1
            if seen<=6 then
                PCSX.log(string.format("[lzs] a0=0x%08X a1=0x%08X a2=0x%08X ra=0x%08X gmode=0x%02X",
                    tou32(n.a0), tou32(n.a1), tou32(n.a2), ra, u8(GMODE)))
            end
        end)
        return {}
    end,
    on_capture=function(c,e)
        local gm=u8(GMODE)
        if e>10 and gm==0x03 then
            local seg=math.floor(e/45)%4
            local btn=({probe.BTN.UP,probe.BTN.DOWN,probe.BTN.LEFT,probe.BTN.RIGHT})[seg+1]
            probe.pad_force(btn)
            for _,b in ipairs({probe.BTN.UP,probe.BTN.DOWN,probe.BTN.LEFT,probe.BTN.RIGHT}) do if b~=btn then probe.pad_release(b) end end
        end
        if e%200==0 then PCSX.log(string.format("[diag t%d] gmode=0x%02X lzs_calls=%d", e, gm, seen)) end
    end,
})
