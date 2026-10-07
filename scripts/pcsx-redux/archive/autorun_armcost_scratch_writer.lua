-- Write-watch the SCRATCH-buffer arm-cost byte 0x800E3860 (the source FUN_800557B8
-- copies into the runtime struct) to learn whether it is LZS-decompressed verbatim
-- from the player file (=> raw file data) or written by a splice computation.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 2400)
local function tou32(v) v=tonumber(v) or 0 if v<0 then v=v+0x100000000 end return v end
local function u8(a) return probe.read_u8(a) or -1 end
local GMODE=0x8007B83C
local SRC=0x800E3860
local writes={}
local nwrite=0
probe.run({
    sstate=SSTATE_PATH, capture_frames=FRAMES,
    on_arm=function()
        PCSX.log(string.format("== scratch arm-cost writer == watch 0x%08X", SRC))
        probe.arm_breakpoint(SRC, "Write", 1, "scratch_cost", function()
            local r=PCSX.getRegisters(); local n=r.GPR and r.GPR.n
            local pc=tou32(r.pc); local ra=n and tou32(n.ra) or 0
            nwrite=nwrite+1
            if writes[pc]==nil then
                writes[pc]={ra=ra, val=u8(SRC), gm=u8(GMODE)}
                PCSX.log(string.format("[WRITER] pc=0x%08X ra=0x%08X val=0x%02X gmode=0x%02X",
                    pc, ra, u8(SRC), u8(GMODE)))
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
        if e%200==0 then PCSX.log(string.format("[diag t%d] gmode=0x%02X writes=%d val=0x%02X", e, gm, nwrite, u8(SRC))) end
    end,
})
