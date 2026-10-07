-- autorun_weapon_specialty_reader.lua
--
-- Pin the arts-bar weapon-specialty reader: which function reads the active
-- character's equipped-weapon byte (char+0x198) when the in-battle Arts command
-- bar sizes the "Arms" input (off-class / Astral Sword = double width; favored
-- class = single). The equipped weapon lives only in the field character record
-- (no battle-actor copy); live records are 0x80084708 + idx*0x414, weapon byte
-- at +0x198:
--   Vahn(0) 0x800848A0   Noa(1) 0x80084CB4   Gala(2) 0x800850C8
--
-- The width is computed when the Arts input is (re)entered, not while sitting at
-- the bar, so a plain read-watch at rest never fires. This probe read-watches
-- all three weapon bytes AND injects a rotating input sequence (cancel / confirm
-- / directions) to force the bar to rebuild, capturing the reader pc + $ra the
-- moment the weapon is read. $ra names the builder; the code around the reader
-- pc indexes the per-weapon class table the randomizer needs.
--
--   bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_weapon_specialty_reader.lua \
--     --scenario arts_bar_offclass_gala_nail --frames 900

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate2")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 900)

local WEAPON_BYTES = {
    { name = "Vahn", addr = 0x800848A0 },
    { name = "Noa",  addr = 0x80084CB4 },
    { name = "Gala", addr = 0x800850C8 },
}

local function tou32(v) v = tonumber(v) or 0 if v < 0 then v = v + 0x100000000 end return v end

local hits = 0
local armed = false

local function arm_reads()
    for _, w in ipairs(WEAPON_BYTES) do
        probe.arm_breakpoint(w.addr, "Read", 1, "wpn_" .. w.name, function()
            local r = PCSX.getRegisters()
            local pc = tou32(r.pc)
            local ra = tou32(r.GPR and r.GPR.n and r.GPR.n.ra or 0)
            hits = hits + 1
            PCSX.log(string.format(
                "[WPN-READ #%d] %s weapon @0x%08X read by pc=0x%08X ra=0x%08X (val=0x%02X)",
                hits, w.name, w.addr, pc, ra, probe.read_u8(w.addr) or 0))
        end)
    end
    PCSX.log("[probe] armed Read-watch on 3 weapon bytes (+0x198)")
end

-- Deliberate cancel -> re-select-Attack loop. From the combo-input screen,
-- repeated cancels back out to the top-level battle command menu, then a confirm
-- re-selects Attack and re-opens the combo bar - the moment the per-Arms-input
-- width is (re)decided from the weapon. We try both common cancel buttons
-- (Circle / Triangle) since the US mapping isn't pinned. One press = force for
-- HOLD frames then release for GAP frames.
-- Phase A (cancel x3) then Phase B (confirm x2), looped.
local HOLD, GAP = 5, 9
local CYCLE = HOLD + GAP            -- frames per single press
-- Build an explicit press timeline (button per press-slot).
local TIMELINE = {}
local function add_presses(btn, n) for _ = 1, n do TIMELINE[#TIMELINE + 1] = btn end end
local function build_round()
    add_presses(probe.BTN.CIRCLE, 2)
    add_presses(probe.BTN.TRIANGLE, 1)
    add_presses(probe.BTN.CROSS, 2)   -- re-select Attack
end
for _ = 1, 12 do build_round() end    -- enough rounds to span the window
local held = nil

probe.run({
    sstate = SSTATE_PATH,
    capture_frames = FRAMES,
    on_arm = function(c)
        PCSX.log("== weapon-specialty arts-bar reader trace ==")
        arm_reads()
        armed = true
        return {}
    end,
    on_capture = function(c, elapsed)
        -- Drive the deliberate cancel/confirm timeline after a short settle.
        if elapsed > 15 then
            local slot = math.floor((elapsed - 16) / CYCLE) + 1
            local phase = (elapsed - 16) % CYCLE
            local btn = TIMELINE[slot]
            if btn ~= nil then
                if phase == 0 then
                    if held ~= nil then probe.pad_release(held) end
                    probe.pad_force(btn); held = btn
                elseif phase == HOLD then
                    if held ~= nil then probe.pad_release(held); held = nil end
                end
            end
        end
        if elapsed % 90 == 0 then
            PCSX.log(string.format("[diag t%d] weapon reads so far=%d", elapsed, hits))
        end
        -- Enough samples to identify the reader + caller; stop early.
        if hits >= 8 and elapsed > 30 then c.request_quit = true end
    end,
})
