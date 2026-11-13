-- autorun_story_forward.lua
--
-- Play a story beat forward from a retail save state with the pad alone, and
-- keep checkpoints of the scenes it passes through. For beats whose chain runs
-- through cutscenes and scene changes the player never controls (a record that
-- changes the scene, an entry script that spawns a cutscene record), where
-- autorun_door_hop.lua's per-scene save waits for control that never comes.
--
-- Input: Cross every LEGAIA_CROSS_EVERY (20) vsyncs outside a fight (a box to
-- page, a banner to clear; LEGAIA_MASH="CROSS,CIRCLE" takes the listed buttons
-- in turn, for a box that Circle closes); in a fight (game mode 0x14 / 0x15, ctx =
-- *0x8007BD24) the round prompt and the commit check take Cross, the command
-- ring takes Attack and the arts entry types LEGAIA_ARTS (default "121"), as
-- autorun_battle_harvest.lua's LEGAIA_PARTY_ARTS drives it. No RAM is written,
-- unless LEGAIA_FINISH=<round> is set: from that round's prompt on, every
-- monster's HP is written to 1 so the party's attacks end a fight it cannot win
-- with this drive (a poke: the state's scenario must name it).
--
-- LEGAIA_PRESSES: `;` list of `<vsync>:<BUTTON>:<hold>`, a button held from
-- that vsync of the run (a walk to the next trigger), ahead of the Cross drive.
--
-- Checkpoints (<prefix>_<scene>_e<vsyncs since the scene entered>_v<vsync>):
--   LEGAIA_CKPT_AT     comma list of vsyncs after each scene's entry (default
--                      "150,600,1200,1800,2400,3000")
--   LEGAIA_CKPT_SCENES comma list of scenes to checkpoint ("" = every scene)
--   LEGAIA_KEEP_SAVE   1 = also save the vsync sound-flag bit 2 (0x8007B750,
--                      op 0x35 sub-op 6, "music not change") rises, named
--                      <prefix>_<scene>_keep_v<vsync>
-- audio.csv, one row per vsync: mode, scene, the track word 0x8007BAC8, the
-- requested index 0x8007BAB8, the sound-flag word 0x8007B750, the loaded
-- index barrier 0x8007BA9C and the field-BGM source 0x8007052C (open, seq).
--
-- Stop: LEGAIA_STOP_SCENE entered (not the scene the run starts in) and LEGAIA_STOP_AFTER (600) vsyncs run in
-- it, or LEGAIA_FRAMES (12000).
--
-- Env: LEGAIA_SSTATE, LEGAIA_OUT_DIR, LEGAIA_PREFIX (default "story").
-- No breakpoints, --fast / --turbo are fine.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 12000)
local PREFIX = probe.getenv("LEGAIA_PREFIX", "story")
local CROSS_EVERY = probe.getenv_num("LEGAIA_CROSS_EVERY", 20)
local KEEP_SAVE = probe.getenv_num("LEGAIA_KEEP_SAVE", 0) == 1
local STOP_SCENE = probe.getenv("LEGAIA_STOP_SCENE", "")
local STOP_AFTER = probe.getenv_num("LEGAIA_STOP_AFTER", 600)

local CKPT_AT = {}
for n in string.gmatch(probe.getenv("LEGAIA_CKPT_AT", "150,600,1200,1800,2400,3000"), "%d+") do
    CKPT_AT[#CKPT_AT + 1] = tonumber(n)
end
local CKPT_SCENES = {}
local any_scene = true
for s in string.gmatch(probe.getenv("LEGAIA_CKPT_SCENES", ""), "[%w_]+") do
    CKPT_SCENES[s] = true
    any_scene = false
end

local GAME_MODE_VA = 0x8007b83c
local SCENE_VA = 0x8007050c
local CTX_PTR = 0x8007bd24
local SOUND_FLAGS = 0x8007b750
local BTN = probe.BTN

local log_lines = {}
local function logf(fmt, ...)
    local s = string.format(fmt, ...)
    log_lines[#log_lines + 1] = s
    PCSX.log("[story] " .. s)
end

local function ok_ptr(p) return p ~= nil and p >= 0x80000000 and p < 0x80200000 end

local function scene_name()
    local out = {}
    for i = 0, 7 do
        local b = probe.read_u8(SCENE_VA + i)
        if b == nil or b < 0x20 or b >= 0x7f then break end
        out[#out + 1] = string.char(b)
    end
    return table.concat(out)
end

local function save_checkpoint(name, vsync)
    local ok, err = pcall(function()
        local w = PCSX.createSaveState()
        local fh = Support.File.open(probe.out_path(name .. ".rawsstate"), "TRUNCATE")
        fh:writeMoveSlice(w)
        fh:close()
    end)
    logf("CKPT %s vsync=%d ok=%s %s", name, vsync, tostring(ok), tostring(err or ""))
end

local held, held_until = nil, 0
local function press(btn, vsync, hold)
    if held ~= nil then probe.pad.release(held) end
    probe.pad.force(btn)
    held, held_until = btn, vsync + (hold or 4)
end

local audio = nil
local last_mode, last_scene, last_flags = nil, nil, nil
local scene_since = 0
local done_ckpt = {}
local next_press = 0
local last_flow, art_i = nil, 1
local FINISH = probe.getenv_num("LEGAIA_FINISH", 0)
local rounds = 0
local PRESSES = {}
for at, b, hold in string.gmatch(probe.getenv("LEGAIA_PRESSES", ""), "(%d+):(%u+):(%d+)") do
    PRESSES[#PRESSES + 1] = { at = tonumber(at), btn = probe.BTN[b], hold = tonumber(hold) }
end
local press_i = 1
local MASH = {}
for b in string.gmatch(probe.getenv("LEGAIA_MASH", "CROSS"), "%u+") do MASH[#MASH + 1] = probe.BTN[b] end
local mash_i = 0
local DEBUG = probe.getenv_num("LEGAIA_DEBUG", 0) == 1
local ARTS = probe.getenv("LEGAIA_ARTS", "121")
local stop_at = nil

probe.run({
    sstate = SSTATE_PATH,
    capture_frames = FRAMES,

    on_arm = function(_)
        logf("sstate=%s frames=%d", SSTATE_PATH, FRAMES)
        return {}
    end,

    on_capture = function(ctx, vsync)
        if held ~= nil and vsync >= held_until then
            probe.pad.release(held)
            held = nil
        end
        local mode = probe.read_u8(GAME_MODE_VA) or 0
        local label = scene_name()
        local flags = probe.read_u32(SOUND_FLAGS) or 0
        audio = audio or probe.csv_open(probe.out_path("audio.csv"),
            "v,mode,scene,word,req,flags,barrier,f_open,f_seq")
        audio:row("%d,%d,%s,%d,%d,%d,%d,%d,%d", vsync, mode, label,
            probe.read_u32(0x8007BAC8) or 0, probe.read_u32(0x8007BAB8) or 0, flags,
            probe.read_u32(0x8007BA9C) or 0, probe.read_u16(0x8007052C + 8) or 0,
            probe.read_u16(0x8007052C + 0xA) or 0)

        if mode ~= last_mode or label ~= last_scene then
            logf("vsync=%d game_mode=0x%02x scene=%q word=%d flags=0x%x", vsync, mode, label,
                probe.read_u32(0x8007BAC8) or 0, flags)
            if label ~= last_scene then
                scene_since = vsync
                if label == STOP_SCENE and stop_at == nil and last_scene ~= nil then stop_at = vsync + STOP_AFTER end
            end
            last_mode, last_scene = mode, label
        end
        if KEEP_SAVE and last_flags ~= nil and bit.band(flags, 4) ~= 0 and bit.band(last_flags, 4) == 0 then
            logf("KEEP bit 2 raised vsync=%d scene=%q", vsync, label)
            save_checkpoint(string.format("%s_%s_keep_v%d", PREFIX, label, vsync), vsync)
        end
        last_flags = flags
        if any_scene or CKPT_SCENES[label] then
            for _, at in ipairs(CKPT_AT) do
                local key = label .. "/" .. at
                if not done_ckpt[key] and vsync - scene_since >= at then
                    done_ckpt[key] = true
                    save_checkpoint(string.format("%s_%s_e%d_v%d", PREFIX, label, at, vsync), vsync)
                end
            end
        end
        if stop_at ~= nil and vsync >= stop_at then
            logf("stop: %d vsyncs in %q", STOP_AFTER, label)
            ctx.request_quit = true
            return
        end
        local p = PRESSES[press_i]
        if p ~= nil and vsync >= p.at then
            press_i = press_i + 1
            logf("PRESS %s for %d at vsync=%d", tostring(p.btn), p.hold, vsync)
            press(p.btn, vsync, p.hold)
            next_press = vsync + p.hold + 4
            return
        end
        if vsync < next_press or held ~= nil then return end

        if mode == 0x14 or mode == 0x15 then
            local cp = probe.read_u32(CTX_PTR)
            if not ok_ptr(cp) then return end
            local flow = probe.read_u8(cp + 6) or 0
            if flow == 0xFF then return end
            -- Attack on the ring, the arts entry takes LEGAIA_ARTS (art-data
            -- direction tokens: 1 Left, 2 Right, 3 Down, 4 Up), then Cross.
            if flow ~= last_flow then
                art_i = 1
                if flow == 0x1E then
                    rounds = rounds + 1
                    if FINISH > 0 and rounds >= FINISH then
                        for seat = 3, 7 do
                            local a = probe.read_u32(0x801C9370 + seat * 4)
                            if ok_ptr(a) and (probe.read_u16(a + 0x14C) or 0) > 1 then
                                probe.write_u16(a + 0x14C, 1)
                                logf("FINISH seat %d hp -> 1 (round %d)", seat, rounds)
                            end
                        end
                    end
                end
                if DEBUG then logf("flow 0x%02x seat=%d vsync=%d", flow, probe.read_u8(cp + 0x13) or 0, vsync) end
            end
            last_flow = flow
            if flow == 0x28 then
                press(BTN.LEFT, vsync, 4)
                next_press = vsync + 16
            elseif flow == 0x78 then
                press(BTN.RIGHT, vsync, 4)
                next_press = vsync + 16
            elseif flow == 0x50 and art_i <= #ARTS then
                local c = tonumber(string.sub(ARTS, art_i, art_i))
                press(({ BTN.LEFT, BTN.RIGHT, BTN.DOWN, BTN.UP })[c] or BTN.CROSS, vsync, 4)
                art_i = art_i + 1
                next_press = vsync + 14
            else
                press(BTN.CROSS, vsync, 4)
                next_press = vsync + 20
            end
            return
        end
        mash_i = mash_i % #MASH + 1
        press(MASH[mash_i], vsync, 4)
        next_press = vsync + CROSS_EVERY
    end,

    on_done = function(_, _)
        if held ~= nil then probe.pad.release(held) end
        local fh = io.open(probe.out_path(PREFIX .. ".log"), "w")
        if fh then fh:write(table.concat(log_lines, "\n"), "\n"); fh:close() end
    end,
})
