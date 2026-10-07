-- Catch the battle-LOAD writer of the arm-cost field. The per-command cost
-- DAT_801c9360[char][0x0C]+0x74 is computed once when battle data loads (it is
-- not rewritten on arts re-entry). Gala's field resolves to a deterministic
-- address (0x801911B8 with the Vahn/Noa/Gala party). Walk the karisto overworld
-- into a random encounter and write-watch that byte through the field->battle
-- transition; the writer pc + the code around it is the off-class penalty logic.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 2400)
local GMODE = 0x8007B83C
local FIELD = 0x801911B8        -- Gala arm cost (cmd0C +0x74), deterministic load addr
local function tou32(v) v=tonumber(v) or 0 if v<0 then v=v+0x100000000 end return v end
local function u8(a) return probe.read_u8(a) or -1 end
local function u32(a) return probe.read_u32(a) or 0 end
local writes, nwrite, last_gm = {}, 0, -1

probe.run({
    sstate = SSTATE_PATH, capture_frames = FRAMES,
    on_arm = function()
        PCSX.log(string.format("== armcost LOAD writer == watch 0x%08X gmode=0x%02X", FIELD, u8(GMODE)))
        probe.arm_breakpoint(FIELD, "Write", 1, "armcost_load", function()
            local r = PCSX.getRegisters()
            local pc = tou32(r.pc); local ra = tou32(r.GPR and r.GPR.n and r.GPR.n.ra or 0)
            nwrite = nwrite + 1
            local key = pc
            if writes[key] == nil then
                writes[key] = {ra=ra, gm=u8(GMODE)}
                PCSX.log(string.format("[WRITER] pc=0x%08X ra=0x%08X newval=0x%02X gmode=0x%02X",
                    pc, ra, u8(FIELD), u8(GMODE)))
            end
        end)
        return {}
    end,
    on_capture = function(c, elapsed)
        local gm = u8(GMODE)
        if gm ~= last_gm then
            PCSX.log(string.format("[gmode] 0x%02X->0x%02X t%d writes=%d", last_gm<0 and 0 or last_gm, gm, elapsed, nwrite))
            last_gm = gm
        end
        -- walk to trip an encounter
        if elapsed > 10 and gm == 0x03 then
            local seg = math.floor(elapsed/45) % 4
            local btn = ({probe.BTN.UP,probe.BTN.DOWN,probe.BTN.LEFT,probe.BTN.RIGHT})[seg+1]
            probe.pad_force(btn)
            for _,b in ipairs({probe.BTN.UP,probe.BTN.DOWN,probe.BTN.LEFT,probe.BTN.RIGHT}) do if b~=btn then probe.pad_release(b) end end
        end
        if elapsed % 150 == 0 then
            PCSX.log(string.format("[diag t%d] gmode=0x%02X writes=%d field@0x%08X=0x%02X DAT9360[2]=0x%08X",
                elapsed, gm, nwrite, FIELD, u8(FIELD), u32(0x801C9368)))
        end
    end,
})
