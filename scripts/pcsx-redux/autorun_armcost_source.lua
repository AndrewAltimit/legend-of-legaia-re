-- At the arm-cost writer 0x80055810 (FUN_800557B8 verbatim copy), capture the
-- SOURCE address (a1) and dump the source vs dest command-struct bytes, to learn
-- where the cost originates (loaded player-file region vs a computed scratch
-- buffer). Filtered to Gala's struct (dest a0 in 0x80191144..0x80191210).
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 2400)
local function tou32(v) v=tonumber(v) or 0 if v<0 then v=v+0x100000000 end return v end
local function u8(a) return probe.read_u8(a) or -1 end
local function u32(a) return probe.read_u32(a) or 0 end
local GMODE=0x8007B83C
local hits=0
local function hexdump(addr, n)
    local s=""
    for i=0,n-1 do s=s..string.format("%02x ", u8(addr+i)) end
    return s
end
probe.run({
    sstate=SSTATE_PATH, capture_frames=FRAMES,
    on_arm=function()
        PCSX.log("== arm-cost SOURCE capture (exec 0x80055810) ==")
        probe.arm_breakpoint(0x80055810, "Exec", 4, "copy_store", function()
            local r=PCSX.getRegisters()
            local n=r.GPR and r.GPR.n
            if not n then return end
            local a0=tou32(n.a0); local a1=tou32(n.a1)
            -- only the word landing on Gala cmd0C+0x74 = 0x801911B8
            if a0 ~= 0x801911B8 then return end
            hits=hits+1
            if hits<=2 then
                PCSX.log(string.format("[copy] a0(dest)=0x%08X a1(src)=0x%08X gmode=0x%02X", a0, a1, u8(GMODE)))
                PCSX.log("  src struct (a1-0x74 .. +0x20): "..hexdump(a1-0x74, 0x28))
                PCSX.log("  src @cost (a1): "..hexdump(a1, 0x10))
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
        if e%200==0 then PCSX.log(string.format("[diag t%d] gmode=0x%02X hits=%d", e, gm, hits)) end
    end,
})
