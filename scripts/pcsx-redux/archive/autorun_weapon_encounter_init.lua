-- autorun_weapon_encounter_init.lua
--
-- Catch the battle-LOAD weapon-specialty lookup. The per-Arms-input favored
-- width is computed once when a battle loads (actor setup), from each party
-- member's equipped weapon id (char+0x198) -- not per turn, not at equip time.
-- So: from an overworld state, read-watch the three party weapon bytes, WALK to
-- trigger a random encounter, and flag any reader pc fired around the field->
-- battle (game_mode -> 0x15) transition. That reader is the favored builder; the
-- code around it indexes the per-weapon class table the randomizer needs.
--
--   bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_weapon_encounter_init.lua \
--     --scenario karisto_sol_pre_encounter --frames 2400

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 2400)

local GMODE = 0x8007B83C        -- game mode (battle = 0x15)
local WEAPON_BYTES = {
    { name = "Vahn", addr = 0x800848A0 },
    { name = "Noa",  addr = 0x80084CB4 },
    { name = "Gala", addr = 0x800850C8 },
}

local function tou32(v) v = tonumber(v) or 0 if v < 0 then v = v + 0x100000000 end return v end
local function gmode() return probe.read_u8(GMODE) or 0xFF end

-- All readers seen, keyed by pc -> {count, first_gmode, sample ra+val}.
local readers = {}
local total = 0

local function arm_reads()
    for _, w in ipairs(WEAPON_BYTES) do
        probe.arm_breakpoint(w.addr, "Read", 1, "wpn_" .. w.name, function()
            local r = PCSX.getRegisters()
            local pc = tou32(r.pc)
            local ra = tou32(r.GPR and r.GPR.n and r.GPR.n.ra or 0)
            total = total + 1
            local e = readers[pc]
            if e == nil then
                e = { count = 0, gm = gmode(), ra = ra, who = w.name,
                      val = probe.read_u8(w.addr) or 0 }
                readers[pc] = e
                PCSX.log(string.format(
                    "[READER new] pc=0x%08X ra=0x%08X %s wpn=0x%02X gmode=0x%02X",
                    pc, ra, w.name, e.val, e.gm))
            end
            e.count = e.count + 1
        end)
    end
    PCSX.log("[probe] armed Read-watch on 3 weapon bytes; walking to encounter")
end

local last_gm = -1
probe.run({
    sstate = SSTATE_PATH,
    capture_frames = FRAMES,
    on_arm = function(c)
        PCSX.log("== weapon battle-load (encounter) lookup trace ==")
        arm_reads()
        return {}
    end,
    on_capture = function(c, elapsed)
        local gm = gmode()
        if gm ~= last_gm then
            PCSX.log(string.format("[gmode] 0x%02X -> 0x%02X at t%d (readers so far=%d)",
                last_gm < 0 and 0 or last_gm, gm, elapsed, total))
            last_gm = gm
        end
        -- Walk: alternate Up / Down (and a little Left/Right) so we keep moving
        -- on the overworld and trip a random encounter rather than stick on a wall.
        if elapsed > 10 then
            local seg = math.floor(elapsed / 45) % 4
            local btn = ({ probe.BTN.UP, probe.BTN.DOWN, probe.BTN.LEFT, probe.BTN.RIGHT })[seg + 1]
            probe.pad_force(btn)
            -- release the other three
            for _, b in ipairs({ probe.BTN.UP, probe.BTN.DOWN, probe.BTN.LEFT, probe.BTN.RIGHT }) do
                if b ~= btn then probe.pad_release(b) end
            end
        end
        if elapsed % 150 == 0 then
            PCSX.log(string.format("[diag t%d] gmode=0x%02X total_reads=%d distinct_pcs=%d",
                elapsed, gm, total, (function() local n=0 for _ in pairs(readers) do n=n+1 end return n end)()))
        end
        -- Once in battle and we've collected a batch of reads, stop.
        if gm == 0x15 and total > 0 and elapsed % 300 == 0 and elapsed > 0 then
            -- keep going a bit to capture the full init, but cap.
        end
    end,
})
