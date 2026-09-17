-- autorun_scene_name_hijack.lua
--
-- Load a field scene the save library has no state for, through retail's own
-- loader, and screenshot it.
--
-- Some scenes are unreachable from every catalogued save - `juui1` has no
-- library state at all - so no capture shows what retail draws there. The
-- field VM's op `0x3F` (named scene change, the "door") carries its
-- destination **inline in the bytecode**: `[u16 index][u8 name_len][name
-- bytes]`. This probe breaks on the VM's per-op dispatcher `FUN_801DE840`,
-- waits for a `0x3F`, and overwrites those name bytes in the live MAN buffer
-- before the op consumes them. Everything downstream - the bundle load, the
-- scene entry script, the camera seat, the renderer - is retail's.
--
-- The replacement must be the SAME LENGTH as the name it replaces, because the
-- name is inline in the bytecode: a shorter or longer one would need
-- `name_len` changed and would shift every following instruction. The probe
-- refuses a length mismatch and reports it rather than corrupting the script.
-- `LEGAIA_HIJACK` empty makes the run a read-only trace that just reports the
-- doors it saw, which is how to find a same-length donor.
--
-- The seat index is left alone, so the player arrives at whatever seat that
-- index names in the destination - which may be nothing sensible. This is a
-- capture of what the scene LOOKS like, not of a reachable route into it.
--
-- **Not every scene exit is an op `0x3F`.** The Drake Castle exit is not: over
-- 600 vsyncs of that state the VM dispatcher is entered 1072 times, the scene
-- word goes `dolk` -> `map01`, and not one of those ops is a `0x3F`. That exit
-- is a walk-on trigger from the scene's `.PCH` sidecar
-- (docs/formats/scene-v12-table.md), which reaches the loader without a script
-- op. `LEGAIA_HIJACK_SCENE_WORD=1` covers that case from the other end: it
-- watches the resident scene-name word `0x8007050C` and overwrites it with the
-- hijack name for `LEGAIA_HIJACK_HOLD` vsyncs after whatever wrote it, so the
-- load that follows resolves the replacement name. Same length rule.
--
-- Env vars:
--   LEGAIA_SSTATE      save state (run_probe.sh --scenario <label>)
--   LEGAIA_FRAMES      capture vsyncs (default 1200)
--   LEGAIA_HOLD_BTN    direction held to reach the door (default UP)
--   LEGAIA_HOLD_START  first vsync of the first hold (default 60)
--   LEGAIA_HOLD_LEN    vsyncs held per attempt (default 90)
--   LEGAIA_HOLD_PERIOD vsyncs between attempts (default 300)
--   LEGAIA_HIJACK      destination scene name (empty = read-only trace)
--   LEGAIA_HIJACK_SEAT seat index to write alongside it (-1 = keep the door's)
--   LEGAIA_HIJACK_SCENE_WORD  1 = overwrite the resident scene word instead of
--                      the op-`0x3F` inline name (for a walk-on-trigger exit)
--   LEGAIA_HIJACK_HOLD vsyncs to keep rewriting the word (default 240)
--   LEGAIA_SHOT_EVERY  screenshot period in vsyncs after the swap (default 120)
--   LEGAIA_OUT_DIR     output directory
--
-- Outputs: scene_name_hijack.log, door_%02d.txt, shot_%05d.screen[.meta]

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE      = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 1200)
local HOLD_BTN    = probe.getenv("LEGAIA_HOLD_BTN", "UP")
local HOLD_START  = probe.getenv_num("LEGAIA_HOLD_START", 60)
local HOLD_LEN    = probe.getenv_num("LEGAIA_HOLD_LEN", 90)
local HOLD_PERIOD = probe.getenv_num("LEGAIA_HOLD_PERIOD", 300)
local HIJACK      = probe.getenv("LEGAIA_HIJACK", "")
local SHOT_EVERY  = probe.getenv_num("LEGAIA_SHOT_EVERY", 120)
local WORD_MODE   = probe.getenv_num("LEGAIA_HIJACK_SCENE_WORD", 0)
local SEAT        = probe.getenv_num("LEGAIA_HIJACK_SEAT", -1)
local WORD_HOLD   = probe.getenv_num("LEGAIA_HIJACK_HOLD", 240)

local OUT_LOG = probe.out_path("scene_name_hijack.log")

-- ---------------------------------------------------------------- addresses
local VM_DISPATCH = 0x801DE840   -- FUN_801DE840(a0 = bytecode, a1 = pc, a2 = ctx)
local SCENE_NAME  = 0x8007050C
local GAME_MODE   = 0x8007B83C
local PLAYER_P    = 0x8007C364   -- pointer to the player actor record
local FIELDBUF_P  = 0x1F8003EC   -- scratchpad: the scene's field buffer
local GRID_OFF    = 0x4000       -- walkability grid inside that buffer
local TILE        = 128

-- ------------------------------------------------------------------ helpers
local lines = {}
local function logf(fmt, ...)
    local s = string.format(fmt, ...)
    lines[#lines + 1] = s
    PCSX.log("[hijack] " .. s)
end

local function read_name(addr, len)
    if len <= 0 or len > 16 then return "?" end
    local s = {}
    for i = 0, len - 1 do
        local b = probe.read_u8(addr + i) or 0
        s[#s + 1] = (b >= 0x20 and b <= 0x7E) and string.char(b) or "."
    end
    return table.concat(s)
end

local function scene_word()
    local out = {}
    for i = 0, 7 do
        local b = probe.read_u8(SCENE_NAME + i)
        if b == nil or b < 0x20 or b >= 0x7F then break end
        out[#out + 1] = string.char(b)
    end
    return table.concat(out)
end

local function s16(v)
    v = bit.band(tonumber(v) or 0, 0xFFFF)
    if v >= 0x8000 then v = v - 0x10000 end
    return v
end

-- Is the scene the loader actually staged drawable and standable, or is the
-- player seated outside it? A black frame alone cannot tell those apart. The
-- player's tile against the per-scene walkability grid can: the grid is
-- `*(_DAT_1F8003EC) + 0x4000`, one byte of four sub-cell wall bits per
-- 128-unit tile (docs/subsystems/field-locomotion.md).
local function footing()
    local pp = probe.read_u32(PLAYER_P)
    if pp == nil or not probe.in_ram(pp) then return nil end
    local x, z = s16(probe.read_u16(pp + 0x14) or 0), s16(probe.read_u16(pp + 0x18) or 0)
    local base = probe.read_scratch_u32(FIELDBUF_P)
    local cell, open_tiles = nil, 0
    if base ~= nil and base >= 0x80000000 then
        local col, rowi = math.floor(x / TILE), math.floor(z / TILE)
        if col >= 0 and col < 0x80 and rowi >= 0 and rowi < 0x80 then
            cell = probe.read_u8(base + GRID_OFF + rowi * 0x80 + col)
        end
        for i = 0, 0x3FFF, 7 do
            local b = probe.read_u8(base + GRID_OFF + i) or 0xFF
            if b ~= 0xFF then open_tiles = open_tiles + 1 end
        end
    end
    return x, z, cell, open_tiles
end

local function shot(name)
    local ok, ss = pcall(function() return PCSX.GPU.takeScreenShot() end)
    if not (ok and ss) then return end
    local bpp = (tonumber(ss.bpp) or 0) > 16 and 24 or 16
    local h = io.open(probe.out_path(name .. ".screen"), "wb")
    if not h then return end
    h:write(tostring(ss.data)); h:close()
    local m = io.open(probe.out_path(name .. ".screen.meta"), "w")
    if m then
        m:write(string.format("width=%d\nheight=%d\nbpp=%d\n",
            tonumber(ss.width), tonumber(ss.height), bpp))
        m:close()
    end
end

-- ------------------------------------------------------------------- state
local g_elapsed = 0
local doors = 0
local swapped = false
local swap_vsync = -1
local hold_held = nil
local last_scene = nil
local vm_hits = 0

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,

    on_arm = function(ctx)
        probe.write_manifest("autorun_scene_name_hijack.lua", {
            sstate = SSTATE, frames = FRAMES, hold_btn = HOLD_BTN,
            hold_start = HOLD_START, hold_len = HOLD_LEN,
            hold_period = HOLD_PERIOD, hijack = HIJACK,
            shot_every = SHOT_EVERY,
        })
        local d = { addr = VM_DISPATCH, hits_ref = { n = 0 }, name = "field VM op dispatch" }
        probe.arm_breakpoint(VM_DISPATCH, "Exec", 4, "hijack_vm", function()
            vm_hits = vm_hits + 1
            d.hits_ref.n = vm_hits
            local r = PCSX.getRegisters()
            -- `tonumber`, NOT `n32`: `bit.band` yields a SIGNED 32-bit integer
            -- in LuaJIT, so a KSEG0 pointer comes back negative and every
            -- `a0 < 0x80000000` guard then rejects it. The first run of this
            -- probe did exactly that and reported zero doors while the VM
            -- dispatcher was entered 1072 times.
            local a0 = tonumber(r.GPR.n.a0) or 0
            local a1 = tonumber(r.GPR.n.a1) or 0
            if a0 < 0x80000000 then return end
            local raw = probe.read_u8(a0 + a1) or 0
            local ext = bit.band(raw, 0x80) ~= 0
            if bit.band(raw, 0x7F) ~= 0x3F then return end
            local opnd = a1 + (ext and 2 or 1)
            local index = probe.read_u16(a0 + opnd) or 0
            local nlen = probe.read_u8(a0 + opnd + 2) or 0
            local nbase = a0 + opnd + 3
            local name = read_name(nbase, nlen)
            doors = doors + 1
            logf("door %d at vsync %d: index=%d name_len=%d name='%s' (bytecode 0x%08X pc 0x%04X)",
                 doors, g_elapsed, index, nlen, name, a0, a1)
            if HIJACK == "" or swapped then return end
            if #HIJACK ~= nlen then
                logf("  NOT swapped: '%s' is %d bytes and the inline name is %d - "
                     .. "a different length would need name_len changed and would "
                     .. "shift every following instruction", HIJACK, #HIJACK, nlen)
                return
            end
            for i = 1, nlen do
                probe.write_u8(nbase + i - 1, string.byte(HIJACK, i))
            end
            if SEAT >= 0 then
                -- The seat index belongs to the door's ORIGINAL destination.
                -- Left alone it seats the player outside the replacement
                -- scene, which draws nothing at all - measured: a hijack into
                -- a scene the game draws brightly renders as black as one into
                -- a scene under test, so a frame taken that way says nothing
                -- about the scene. `u16`, inline, same length either way.
                probe.write_u16(a0 + opnd, SEAT)
            end
            swapped = true
            swap_vsync = g_elapsed
            logf("  SWAPPED -> '%s' seat %s (read back '%s' index %d)",
                 HIJACK, SEAT >= 0 and tostring(SEAT) or "unchanged",
                 read_name(nbase, nlen), probe.read_u16(a0 + opnd) or 0)
        end)
        return { d }
    end,

    on_capture = function(ctx, elapsed)
        g_elapsed = elapsed
        if elapsed == 2 then
            logf("start: scene=%s mode=0x%02X hijack='%s'",
                 scene_word(), probe.read_u8(GAME_MODE) or 0, HIJACK)
        end
        local sc = scene_word()
        if sc ~= last_scene then
            logf("scene word -> %s at vsync %d (mode 0x%02X)", sc, elapsed,
                 probe.read_u8(GAME_MODE) or 0)
            if WORD_MODE == 1 and HIJACK ~= "" and not swapped
               and last_scene ~= nil and sc ~= HIJACK then
                if #sc ~= #HIJACK then
                    logf("  NOT swapped: the word is '%s' (%d bytes) and '%s' is %d",
                         sc, #sc, HIJACK, #HIJACK)
                else
                    swapped = true
                    swap_vsync = elapsed
                    logf("  SWAPPING the scene word -> '%s' for %d vsyncs",
                         HIJACK, WORD_HOLD)
                end
            end
            last_scene = sc
        end
        if WORD_MODE == 1 and swapped and elapsed - swap_vsync <= WORD_HOLD then
            for i = 1, #HIJACK do
                probe.write_u8(SCENE_NAME + i - 1, string.byte(HIJACK, i))
            end
            probe.write_u8(SCENE_NAME + #HIJACK, 0)
            last_scene = HIJACK
        end
        -- Screenshots start once a swap has landed (or, on a read-only run,
        -- once the hold window opens) so the disk does not fill with the
        -- source scene.
        local from = swapped and swap_vsync or HOLD_START
        if elapsed > from and SHOT_EVERY > 0 and (elapsed - from) % SHOT_EVERY == 0 then
            pcall(function() shot(string.format("shot_%05d", elapsed)) end)
        end
        if swapped and elapsed > swap_vsync and (elapsed - swap_vsync) % 120 == 0 then
            local x, z, cell, open_tiles = footing()
            if x ~= nil then
                logf("vsync %d: player (%d,%d) tile (%d,%d) grid cell %s; "
                     .. "sampled open tiles %d/2341",
                     elapsed, x, z, math.floor(x / TILE), math.floor(z / TILE),
                     cell and string.format("0x%02X", cell) or "unreadable",
                     open_tiles)
            end
        end
        -- After a swap the hold keeps running, so the player walks: a scene
        -- that stages a grid lets him move, and a black frame over a moving
        -- player is a dark scene rather than an empty one.
        if HOLD_BTN ~= "NONE" then
            local btn = probe.BTN[HOLD_BTN]
            if btn ~= nil and elapsed >= HOLD_START then
                local phase = (elapsed - HOLD_START) % HOLD_PERIOD
                local want = phase < HOLD_LEN
                if want and hold_held == nil then
                    probe.pad_force(btn); hold_held = btn
                elseif not want and hold_held ~= nil then
                    probe.pad_release(hold_held); hold_held = nil
                end
            end
        end
    end,

    on_summary = function(ctx, descs)
        if hold_held then probe.pad_release(hold_held); hold_held = nil end
        logf("VM dispatch hits: %d; doors seen: %d; swapped: %s (vsync %d)",
             vm_hits, doors, tostring(swapped), swap_vsync)
        logf("end: scene=%s mode=0x%02X", scene_word(), probe.read_u8(GAME_MODE) or 0)
        local fh = io.open(OUT_LOG, "w")
        if fh then fh:write(table.concat(lines, "\n")); fh:write("\n"); fh:close() end
    end,
})
