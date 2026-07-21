-- autorun_status_word_writer.lua
--
-- Battle-actor status-word writer hunt: who stores the actor `+0x16E`
-- flags word (bit 4 = counts-as-defeated / Stone, bit 0x400 = the
-- still-unattributed status bit in docs/subsystems/battle.md)? From a
-- battle save state, arm a width-correct write-watch over `+0x16C..+0x170`
-- of every live actor slot (the 8-pointer table at 0x801C9370) and log
-- every store's pc/ra plus the pre/post word - the queen-bee auto-battle
-- inflicts party statuses with no input, so the setter fires unattended.
--
-- Lua BPs are DEAD under --fast; run the default -interpreter -debugger
-- tier and ALWAYS wrap in `timeout`.
--
-- Launch:
--   LEGAIA_FRAMES=5400 timeout --kill-after=30s 1500s \
--   bash scripts/pcsx-redux/run_probe.sh \
--     --scenario rim_elm_queen_bee_battle \
--     --lua scripts/pcsx-redux/autorun_status_word_writer.lua
--
-- Output: status_word_writer.csv  tick,slot,pc,ra,pre,now,mode,count

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe  = require("probe")
local mem    = require("probe.mem")
local step   = require("probe.step")

local GAME_MODE   = 0x8007B83C
local ACTOR_TABLE = 0x801C9370
local A_STATUS    = 0x16E
local BATTLE_MODE = 0x15

local SSTATE = probe.getenv("LEGAIA_SSTATE",
    "saves/library/pcsx-redux/3d22fa5fd53d47cd22999a7b377ec8ece057fdb5ca164357be0f96a65147ddf3.sstate")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 5400)
local ARM_DELAY = probe.getenv_num("LEGAIA_ARM_DELAY", 60)

local CSV = probe.csv_open(probe.out_path("status_word_writer.csv"),
    "tick,slot,pc,ra,pre,now,mode,count")

local vsync = 0
local armed_battle = false
local battle_at = nil
local pending = {}
local counts = {}
local watched = {}

local function log(s) PCSX.log("[statw] " .. s) end
local function u16(a) return mem.read_u16(a) or 0 end

local function arm_actor(slot, ptr)
    local addr = ptr + A_STATUS
    watched[slot] = addr
    step.find_writer(addr, 2, {
        unit = 2, read_len = 2, label = "st" .. slot, max = 8192,
        on_write = function(rg)
            pending[#pending + 1] = {
                slot = slot, pc = rg.pc, ra = rg.ra, pre = u16(addr),
                addr = addr,
            }
        end,
    })
    log(string.format("armed slot %d status word at 0x%08X (now 0x%04X)",
        slot, addr, u16(addr)))
end

probe.run({
    sstate         = SSTATE,
    capture_frames = FRAMES,
    on_arm = function()
        probe.write_manifest("autorun_status_word_writer.lua",
            { sstate = SSTATE, frames = tostring(FRAMES) })
        return {}
    end,
    on_capture = function(ctx, elapsed)
        vsync = elapsed
        for i = 1, #pending do
            local ev = pending[i]
            local now = u16(ev.addr)
            local key = string.format("%d|%08X|%04X", ev.slot, ev.pc, now)
            local n = (counts[key] or 0) + 1
            counts[key] = n
            if n <= 6 then
                CSV:row("%d,%d,0x%08X,0x%08X,0x%04X,0x%04X,0x%02X,%d",
                    vsync, ev.slot, ev.pc, ev.ra, ev.pre, now,
                    mem.read_u8(GAME_MODE) or 0, n)
            end
        end
        pending = {}

        local m = u16(GAME_MODE)
        if m == BATTLE_MODE and battle_at == nil then battle_at = elapsed end
        if not armed_battle and battle_at and elapsed >= battle_at + ARM_DELAY then
            armed_battle = true
            for slot = 0, 7 do
                local ptr = probe.read_u32(ACTOR_TABLE + slot * 4) or 0
                if ptr ~= 0 and mem.in_ram(ptr) then arm_actor(slot, ptr) end
            end
        end
    end,
    on_done = function()
        CSV:close()
        log("done")
    end,
})
