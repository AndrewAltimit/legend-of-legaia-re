-- autorun_w1a_koru_strip.lua
--
-- Koru's timed-fight HUD strip (`Turns Left / HP Left`), captured.
--
-- The strip's draw sites in the battle overlay (PROT 0898) all open on the
-- same gate: the first formation cell `0x8007BD0C` must hold monster `0xB6`
-- (Koru). This probe installs that formation from a plain field state the
-- way a rolled encounter does (cells `0x8007BD0C..0F`, then master mode 8 -
-- the `autorun_delilas_battle_load.lua` install), drives the command flow
-- with a CROSS / UP mash once battle main (`0x15`) is up, and records what
-- the strip reads from: the battle turn counter `ctx[+0x28A]` (`ctx` =
-- `*0x8007BD24`), the round driver's phase byte `ctx[+6]`, and the two
-- digit globals `DAT_801F6958` (turns left) / `DAT_801F6959` (HP percent).
-- A checkpoint is written every LEGAIA_CKPT_EVERY battle vsyncs and on
-- every turn-counter change, so the frame each one holds can be read off
-- the state's VRAM (`extract_vram_from_sstate.py`).
--
-- SYNTHETIC: the formation is installed, not rolled or scripted. The strip
-- gate reads only the formation cell, so the draw it exercises is Koru's;
-- the arena, BGM and party are whatever the loaded state's scene gives.
--
-- Env: LEGAIA_SSTATE, LEGAIA_FORCE_AT (field vsyncs before the install,
-- default 120), LEGAIA_IDS (default "182"), LEGAIA_CKPT_EVERY (default
-- 300), LEGAIA_CKPT_MAX (default 30), LEGAIA_MAX_TICKS (default 9000),
-- LEGAIA_OUT_DIR. Output: koru_strip.csv + k_*.rawsstate. Poll-only: runs
-- under `--fast`.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local mem   = require("probe.mem")
local pad   = require("probe.pad")

local GAME_MODE      = 0x8007B83C
local FORMATION_CELL = 0x8007BD0C
local BATTLE_CTX_PTR = 0x8007BD24
local TURNS_LEFT     = 0x801F6958
local HP_PERCENT     = 0x801F6959

local SSTATE    = probe.getenv("LEGAIA_SSTATE", "")
local FORCE_AT  = probe.getenv_num("LEGAIA_FORCE_AT", 120)
local CK_EVERY  = probe.getenv_num("LEGAIA_CKPT_EVERY", 300)
local CK_MAX    = probe.getenv_num("LEGAIA_CKPT_MAX", 30)
local MAX_TICKS = probe.getenv_num("LEGAIA_MAX_TICKS", 9000)
local OUT_DIR   = probe.getenv("LEGAIA_OUT_DIR", "captures/w1a_koru_strip")
local ids = {}
for tok in string.gmatch(probe.getenv("LEGAIA_IDS", "182"), "[^,%s]+") do ids[#ids + 1] = tonumber(tok) end

os.execute(string.format("mkdir -p %q", OUT_DIR))
local CSV = probe.csv_open(probe.out_path("koru_strip.csv"),
    "vsync,mode,cell0,turn,phase,turns_left,hp_pct,note")

local vsync, loaded, field, installed, battle, ckpts, done = 0, nil, 0, false, 0, 0, false
local last_turn, last_phase, last_mode = nil, nil, nil

local function u8(a) return mem.read_u8(a) or 0 end
local function checkpoint(tag)
    if ckpts >= CK_MAX then return end
    ckpts = ckpts + 1
    pcall(function()
        local w = PCSX.createSaveState()
        local fh = Support.File.open(string.format("%s/k_%05d_%s.rawsstate", OUT_DIR, vsync, tag), "CREATE")
        fh:writeMoveSlice(w); fh:close()
    end)
end

local function on_vsync()
    if done then return end
    vsync = vsync + 1
    if loaded == nil then
        if vsync >= 60 then
            if SSTATE ~= "" and not probe.load_save_state(SSTATE) then
                PCSX.log("[koru_strip] load failed"); done = true; PCSX.quit(1); return
            end
            loaded = vsync
        end
        return
    end
    local md = u8(GAME_MODE)
    if not installed then
        if md == 0x03 then field = field + 1 end
        if field >= FORCE_AT then
            for i = 0, 3 do mem.write_u8(FORMATION_CELL + i, ids[i + 1] or 0) end
            mem.write_u16(GAME_MODE, 8)
            installed = true
            CSV:row("%d,0x%02X,%d,-,-,-,-,installed (synthetic)", vsync, md, u8(FORMATION_CELL))
        end
        return
    end
    local ctx = (mem.read_u32(BATTLE_CTX_PTR) or 0) % 0x100000000
    local turn, phase = -1, -1
    if md == 0x15 and ctx >= 0x80000000 and ctx < 0x80200000 then
        turn = u8(ctx + 0x28A); phase = u8(ctx + 6)
    end
    local note = ""
    if md ~= last_mode then note = "mode" end
    if turn ~= last_turn then note = note .. " turn" end
    if phase ~= last_phase then note = note .. " phase" end
    if note ~= "" then
        CSV:row("%d,0x%02X,%d,%d,%d,%d,%d,%s", vsync, md, u8(FORMATION_CELL), turn, phase,
            u8(TURNS_LEFT), u8(HP_PERCENT), note)
        CSV.fh:flush()
    end
    if md == 0x15 then
        battle = battle + 1
        if last_turn ~= nil and turn ~= last_turn then checkpoint("turn" .. turn) end
        if battle % CK_EVERY == 0 then checkpoint("b" .. battle) end
        local ph = battle % 40
        if ph == 0 then pad.force(pad.BTN.CROSS)
        elseif ph == 6 then pad.release(pad.BTN.CROSS)
        elseif ph == 20 then pad.force(pad.BTN.UP)
        elseif ph == 26 then pad.release(pad.BTN.UP) end
    end
    last_mode, last_turn, last_phase = md, turn, phase
    if vsync >= MAX_TICKS then done = true; CSV:close(); PCSX.quit(0) end
end

PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] =
    PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
