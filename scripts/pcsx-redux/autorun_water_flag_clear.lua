-- autorun_water_flag_clear.lua - who clears the Rogue's Tower water flags?
--
-- Raises system flags 0x1D (green water, mask 0x04) and 0x1E (blue water,
-- mask 0x02) in byte 0x8008575B, forces the next step to roll a random
-- battle (step counter 0x8007B5FC = 1), walks, wins the fight (monster
-- HP held at 1, Cross mashed), and logs every write to the
-- flag word 0x80085758..0x8008575B with the writer's pc / ra, plus the byte
-- and game mode every 30 vsyncs. A first-N call-context dump goes to the
-- .detail.txt sidecar.
--
-- Default state: rugi_field_card_boot (Rogue's Tower, rate-20 region).
--
--   LEGAIA_SSTATE=saves/library/pcsx-redux/081f939e....sstate \
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_water_flag_clear.lua \
--   LEGAIA_OUT=/tmp/water.csv LEGAIA_FRAMES=1800 \
--       bash scripts/pcsx-redux/run_probe.sh
--
-- LEGAIA_WATER_MASK overrides the bits raised (default 0x06).

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local FLAG_WORD   = 0x80085758
local WATER_BYTE  = 0x8008575B
local STEP_CTR    = 0x8007B5FC
local GAME_MODE   = 0x8007B83C
local ACTORS      = 0x801C9370  -- live battle actor table, seats 0..7

local out   = probe.getenv("LEGAIA_OUT", "/tmp/water_flag_clear.csv")
local mask  = probe.getenv_num("LEGAIA_WATER_MASK", 0x06)
local csv   = probe.csv_open(out, "tick,kind,pc,ra,byte_before,mode")
local detail_path = out:gsub("%.csv$", "") .. ".detail.txt"
local tick  = 0
local details = 0
local dirs  = { probe.pad.BTN.UP, probe.pad.BTN.RIGHT, probe.pad.BTN.DOWN, probe.pad.BTN.LEFT }
local held  = nil
local poked = false

local function hold(btn)
    if held then probe.pad.release(held) end
    held = btn
    if btn then probe.pad.force(btn) end
end

probe.run({
    sstate         = probe.getenv("LEGAIA_SSTATE", ""),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 1800),
    snapshot_path  = out:gsub("%.csv$", "") .. ".hits.txt",
    on_arm = function()
        local d = { addr = FLAG_WORD, name = "flag_word", hits_ref = { n = 0 } }
        probe.arm_breakpoint(FLAG_WORD, "Write", 4, d.name, function()
            d.hits_ref.n = d.hits_ref.n + 1
            local r = PCSX.getRegisters()
            csv:row("%d,write,0x%08X,0x%08X,0x%02X,0x%02X", tick,
                tonumber(r.pc), tonumber(r.GPR.n.ra),
                probe.read_u8(WATER_BYTE), probe.read_u8(GAME_MODE))
            if details < 12 then
                details = details + 1
                probe.append_call_context(detail_path,
                    probe.capture_call_context(string.format("write #%d tick %d", details, tick)))
            end
        end)
        hold(dirs[1])
        return { d }
    end,
    on_capture = function(ctx, elapsed)
        tick = elapsed
        local mode = probe.read_u8(GAME_MODE)
        -- Poke once the loaded state is running in the field: a poke made
        -- in on_arm is overwritten by the state load.
        if not poked and mode == 0x03 and elapsed >= 10 then
            poked = true
            local b = probe.read_u8(WATER_BYTE)
            probe.write_u8(WATER_BYTE, bit.bor(b, mask))
            probe.write_u32(STEP_CTR, 1)
            csv:row("%d,poke,0,0,0x%02X,0x%02X", elapsed, b, mode)
        end
        if mode == 0x03 then
            if elapsed % 45 == 0 then hold(dirs[math.floor(elapsed / 45) % 4 + 1]) end
        elseif mode == 0x15 then
            -- Drive the fight to a win: monsters to 1 HP, mash Cross.
            for seat = 3, 7 do
                local a = probe.read_u32(ACTORS + seat * 4)
                if probe.in_ram(a, 0x200) and probe.read_u16(a + 0x14C) > 1 then
                    probe.write_u16(a + 0x14C, 1)
                end
            end
            if elapsed % 8 == 0 then hold(probe.pad.BTN.CROSS)
            elseif elapsed % 8 == 4 then hold(nil) end
        elseif held then
            hold(nil)
        end
        if elapsed % 30 == 0 then
            csv:row("%d,sample,0,0,0x%02X,0x%02X", elapsed, probe.read_u8(WATER_BYTE), mode)
        end
    end,
    on_done = function() hold(nil); csv:close() end,
})
