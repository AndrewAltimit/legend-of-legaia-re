-- autorun_tetsu_spar_order.lua
--
-- Retail check of the Tetsu spar order in Rim Elm (town01): from a state in
-- or before the spar, play the fight out (CROSS every other eight vsyncs, as
-- autorun_fight_pace.lua does) and the talk that resumes after it (CROSS
-- every 20 vsyncs), logging the story flags the hunters' return hangs on:
--   0x20B  the spar was fought          (byte 0x80085799 bit 0x10)
--   0x20C  the spar counts              (byte 0x80085799 bit 0x08)
--   0x227  the measuring is done        (byte 0x8008579C bit 0x01)
--   0x22E  the hunters returned         (byte 0x8008579D bit 0x02)
-- One CSV row every 60 vsyncs and on every game-mode change, plus a raw save
-- state when the run ends (`LEGAIA_CKPT` names it).
--
-- The spar is the battle tutorial, whose four lessons (attacks, items,
-- Spirit, the Hyper Art drill) a CROSS-only hand never finishes. With
-- LEGAIA_TUTORIAL_DONE=1 the probe writes lesson 4 ("all taught, close the
-- fight on the next tick") into the battle context's lesson byte
-- `ctx[+0x28A]` once (`*0x8007BD24`; the clamp at `0x801F7390`). That poke
-- ends the fight the way finishing the lessons does and touches no story
-- flag, so the flags the talk then writes are retail's.
--
-- Env: LEGAIA_SSTATE, LEGAIA_OUT_DIR, LEGAIA_FRAMES (default 6000),
--      LEGAIA_CKPT (default tetsu_spar_order.rawsstate in the out dir),
--      LEGAIA_TUTORIAL_DONE.
-- Output: flags.csv (vsync,mode,b799,b79c,b79d,f20b,f20c,f227,f22e).

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local GAME_MODE = 0x8007b83c
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 6000)
local CKPT = probe.getenv("LEGAIA_CKPT", probe.out_path("tetsu_spar_order.rawsstate"))

local csv = probe.csv_open(probe.out_path("flags.csv"),
    "vsync,mode,b799,b79c,b79d,f20b,f20c,f227,f22e")
local TUTORIAL_DONE = probe.getenv("LEGAIA_TUTORIAL_DONE", "") == "1"
local CTX = 0x8007bd24
local last_mode = -1
local saved = false
local poked = false

local function bitset(b, m) return bit.band(b, m) ~= 0 and 1 or 0 end

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE", ""),
    capture_frames = FRAMES,
    on_arm = function(_) return {} end,
    on_capture = function(_, vsync)
        local mode = probe.read_u8(GAME_MODE) or 0
        local b799 = probe.read_u8(0x80085799) or 0
        local b79c = probe.read_u8(0x8008579c) or 0
        local b79d = probe.read_u8(0x8008579d) or 0
        if mode ~= last_mode or vsync % 60 == 0 then
            csv:row("%d,%d,%d,%d,%d,%d,%d,%d,%d", vsync, mode, b799, b79c, b79d,
                bitset(b799, 0x10), bitset(b799, 0x08), bitset(b79c, 0x01), bitset(b79d, 0x02))
            last_mode = mode
        end
        if TUTORIAL_DONE and not poked and mode == 0x15 and vsync > 120 then
            local ctx = probe.read_u32(CTX) or 0
            if ctx >= 0x80000000 and ctx < 0x80200000 then
                probe.write_u8(ctx + 0x28A, 4)
                poked = true
                PCSX.log(string.format("[tetsu_spar_order] lesson byte at %08X set to 4 (vsync %d)", ctx + 0x28A, vsync))
            end
        end
        probe.pad_release(probe.BTN.CROSS)
        if mode == 0x15 then
            if vsync % 16 < 4 then probe.pad_force(probe.BTN.CROSS) end
        elseif vsync % 20 < 3 then
            probe.pad_force(probe.BTN.CROSS)
        end
        if not saved and vsync >= FRAMES - 2 then
            saved = true
            probe.sstate.save(CKPT)
        end
    end,
})
