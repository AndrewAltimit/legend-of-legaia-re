-- autorun_field_pack_projection.lua
--
-- Capture the on-disc -> RAM projection performed by the scene asset
-- loader (FUN_8001F7C0). Documented at docs/formats/field-pack.md, the
-- loader transforms the on-disc preamble into a runtime structure that
-- mixes GP0-shaped primitive packets, the asset descriptor table, and
-- the asset region. A single post-load save state only shows the FINAL
-- runtime layout, so the disc-byte -> RAM-cell projection is invisible.
-- This probe captures the WINDOWS WE CAN'T SEE FROM A SAVE STATE: the
-- exact RAM region after the loader returns, plus the loader's
-- arguments at entry (which scene name was requested, into which
-- buffer pointer).
--
-- Output:
--   - <out>.entry.txt:    loader entry context (a0..a3, sp, ra, scene
--                         name table contents).
--   - <out>.post.bin:     1 MiB main-RAM dump (or a slice; default is
--                         the field-pack window plus 8 KB of slack on
--                         either side) captured immediately after the
--                         loader returns.
--   - <out>.post.txt:     a small text summary: recovered base, scene
--                         name pool slots, scratchpad heap pointer,
--                         GP0-packet header at base+0x60.
--
-- The user runs this with:
--   1. A pre-transition save state (game paused right before stepping
--      onto the warp tile).
--   2. The save's scene-input setup arranged so that resuming the state
--      and tapping a direction triggers the transition into the target
--      scene.
--
-- A separate Python tool (scripts/mednafen/diff_field_pack_projection.py)
-- then diffs the captured RAM against the on-disc PROT bytes for the
-- target scene's field-pack entry, surfacing the "0x4C 0xE2" instances
-- and any other relocation residue.
--
-- Run:
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_field_pack_projection.lua \
--       LEGAIA_SSTATE=/path/to/pre_warp.sstate \
--       LEGAIA_OUT=/tmp/fp_proj \
--       LEGAIA_FRAMES=1200 \
--       ./scripts/pcsx-redux/run_world_map_probe.sh

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 1200)
local OUT_BASE    = probe.getenv("LEGAIA_OUT", "fp_proj")

-- Loader address (per docs/formats/field-pack.md and
-- capture_observations::field_pack_load::SCENE_ASSET_LOADER_ADDR).
local LOADER_ADDR     = probe.getenv_num("LEGAIA_LOADER", 0x8001F7C0)
local SCENE_NAME_ADDR = probe.getenv_num("LEGAIA_SCENE_NAME", 0x80084548)
local LOAD_DEST_PTR   = probe.getenv_num("LEGAIA_LOAD_DEST_PTR", 0x8007B8D0)
local EFFECT_OFFSET   = 0x12800
local FIELD_PACK_LEN  = EFFECT_OFFSET  -- region between base and effect data

local ENTRY_PATH      = OUT_BASE .. ".entry.txt"
local POST_TXT_PATH   = OUT_BASE .. ".post.txt"
local POST_BIN_PATH   = OUT_BASE .. ".post.bin"

local state = {
    capture_count = 0,
    armed_return  = false,
    return_bp     = nil,
}

local function read_cstring(addr, max)
    local bytes = probe.read_bytes(addr, max)
    if not bytes then return "(unreadable)" end
    local s = tostring(bytes)
    local nul = s:find("\0", 1, true)
    if nul then s = s:sub(1, nul - 1) end
    return s
end

local function dump_post_load()
    state.capture_count = state.capture_count + 1
    local off = (LOAD_DEST_PTR - 0x80000000)
    local base_plus_eff = probe.read_u32(LOAD_DEST_PTR) or 0
    local base = (base_plus_eff >= EFFECT_OFFSET)
        and (base_plus_eff - EFFECT_OFFSET) or 0
    PCSX.log(string.format(
        "[fp] post-load capture #%d: load_dest=0x%08X base=0x%08X",
        state.capture_count, base_plus_eff, base))

    local lines = {}
    lines[#lines + 1] = string.format(
        "post-load capture #%d", state.capture_count)
    lines[#lines + 1] = string.format(
        "load_dest_ptr (_DAT_%08X) = 0x%08X",
        LOAD_DEST_PTR, base_plus_eff)
    lines[#lines + 1] = string.format(
        "field-pack base                 = 0x%08X", base)
    lines[#lines + 1] = string.format(
        "scratchpad heap ptr (1F8003EC)  = 0x%08X",
        probe.read_scratch_u32(0x1F8003EC))
    lines[#lines + 1] = string.format(
        "scene name table (0x%08X):", SCENE_NAME_ADDR)
    -- Scene-name table is 16 bytes per slot; first two slots hold the
    -- new + previous scene names (per FUN_8001FD44).
    lines[#lines + 1] = string.format("  +0x00: %q",
        read_cstring(SCENE_NAME_ADDR, 12))
    lines[#lines + 1] = string.format("  +0x10: %q",
        read_cstring(SCENE_NAME_ADDR + 0x10, 12))

    -- Hex preview of the GP0-packet header at base + 0x60 (where
    -- on-disc slot 0 sits). The runtime stores GPU primitive packets
    -- here, NOT the on-disc record bytes.
    if base ~= 0 then
        local preview = probe.read_bytes(base + 0x60, 64)
        if preview then
            lines[#lines + 1] = string.format(
                "GP0 packet preview at base+0x60 (0x%08X):",
                base + 0x60)
            local s = tostring(preview)
            for row = 0, 3 do
                local words = {}
                for w = 0, 3 do
                    local o = row * 16 + w * 4 + 1
                    if o + 3 <= #s then
                        local b0 = s:byte(o)
                        local b1 = s:byte(o + 1)
                        local b2 = s:byte(o + 2)
                        local b3 = s:byte(o + 3)
                        words[#words + 1] = string.format("%08X",
                            b0 + b1 * 0x100 + b2 * 0x10000 + b3 * 0x1000000)
                    end
                end
                lines[#lines + 1] = "  " .. table.concat(words, " ")
            end
        end
    end

    local f = io.open(POST_TXT_PATH, "a")
    if f then
        f:write(table.concat(lines, "\n"))
        f:write("\n\n")
        f:close()
    end

    -- Dump the field-pack RAM window so the offline diff tool can
    -- align it against the on-disc PROT bytes. We dump
    -- [base .. base + FIELD_PACK_LEN] plus 8 KB of slack on either
    -- side. The capture index is appended to the path so multi-shot
    -- runs don't overwrite each other.
    if base ~= 0 then
        local bin_path = string.format("%s.%02d.bin",
            POST_BIN_PATH:gsub("%.bin$", ""), state.capture_count - 1)
        local slack    = 0x2000
        local lo       = math.max(0x80000000, base - slack)
        local hi       = math.min(0x80200000, base + FIELD_PACK_LEN + slack)
        local len      = hi - lo
        local bytes    = probe.read_bytes(lo, len)
        if bytes then
            local out = io.open(bin_path, "wb")
            if out then
                out:write(tostring(bytes))
                out:close()
                PCSX.log(string.format(
                    "[fp] dumped %d bytes from 0x%08X to %s",
                    len, lo, bin_path))
                local meta = io.open(bin_path .. ".meta", "w")
                if meta then
                    meta:write(string.format(
                        "lo=0x%08X\nhi=0x%08X\nbase=0x%08X\n" ..
                        "scene_slot0=%q\nscene_slot1=%q\n",
                        lo, hi, base,
                        read_cstring(SCENE_NAME_ADDR, 12),
                        read_cstring(SCENE_NAME_ADDR + 0x10, 12)))
                    meta:close()
                end
            else
                PCSX.log("[fp] FATAL: cannot open " .. bin_path)
            end
        else
            PCSX.log("[fp] read_bytes failed for the field-pack window")
        end
    end
end

local desc = {
    addr     = LOADER_ADDR,
    name     = "loader_entry",
    hits_ref = { n = 0 },
}

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    out_path       = OUT_BASE,
    snapshot_path  = OUT_BASE .. ".hits.txt",

    on_arm = function(_)
        -- Truncate output files so multi-shot runs are clean.
        for _, p in ipairs({ ENTRY_PATH, POST_TXT_PATH }) do
            local fh = io.open(p, "w")
            if fh then
                fh:write(string.format(
                    "# field-pack projection probe; loader=0x%08X; sstate=%s\n\n",
                    LOADER_ADDR, SSTATE_PATH))
                fh:close()
            end
        end

        probe.arm_breakpoint(LOADER_ADDR, "Exec", 4, "loader_entry",
            function()
                desc.hits_ref.n = desc.hits_ref.n + 1
                local r  = PCSX.getRegisters()
                local pc = tonumber(r.pc) or 0
                local a0 = tonumber(r.GPR.n.a0) or 0
                local a1 = tonumber(r.GPR.n.a1) or 0
                local a2 = tonumber(r.GPR.n.a2) or 0
                local a3 = tonumber(r.GPR.n.a3) or 0
                local ra = tonumber(r.GPR.n.ra) or 0
                local sp = tonumber(r.GPR.n.sp) or 0
                local scene_idx = (a2 ~= 0) and (probe.read_u32(a2) or 0) or 0

                local entry_lines = {
                    string.format("== loader_entry hit %d ==", desc.hits_ref.n),
                    string.format("pc=0x%08X  ra=0x%08X  sp=0x%08X",
                        pc, ra, sp),
                    string.format("a0 (buf_ptr)        = 0x%08X", a0),
                    string.format("a1 (scene_name_tbl) = 0x%08X", a1),
                    string.format("a2 (scene_idx_ptr)  = 0x%08X (-> 0x%08X)",
                        a2, scene_idx),
                    string.format("a3                  = 0x%08X", a3),
                    string.format("scene_name slot0    = %q",
                        read_cstring(SCENE_NAME_ADDR, 12)),
                    string.format("scene_name slot1    = %q",
                        read_cstring(SCENE_NAME_ADDR + 0x10, 12)),
                }
                local fh = io.open(ENTRY_PATH, "a")
                if fh then
                    fh:write(table.concat(entry_lines, "\n") .. "\n\n")
                    fh:close()
                end
                PCSX.log(string.format(
                    "[fp] loader entry %d: a0=0x%08X scene=%q ra=0x%08X",
                    desc.hits_ref.n, a0,
                    read_cstring(SCENE_NAME_ADDR, 12), ra))

                -- One-shot Exec bp at ra so we capture immediately
                -- after the loader returns. PCSX.addBreakpoint returns
                -- a handle with `:enable() / :disable() / :remove()`;
                -- we remove it from inside the callback so the dump
                -- runs once per loader call.
                if ra ~= 0 and not state.armed_return then
                    state.armed_return = true
                    state.return_bp    = probe.arm_breakpoint(ra, "Exec", 4,
                        "loader_return", function()
                            dump_post_load()
                            state.armed_return = false
                            if state.return_bp then
                                pcall(function()
                                    state.return_bp:remove()
                                end)
                                state.return_bp = nil
                            end
                        end)
                end
            end)

        return { desc }
    end,

    on_done = function(_, _descs)
        PCSX.log(string.format(
            "[fp] capture done; %d loader calls, %d post-load dumps",
            desc.hits_ref.n, state.capture_count))
        PCSX.log("[fp]   entry log:  " .. ENTRY_PATH)
        PCSX.log("[fp]   post text:  " .. POST_TXT_PATH)
        PCSX.log("[fp]   post bins:  " .. OUT_BASE .. ".post.NN.bin")
    end,
})
