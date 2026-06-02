-- autorun_door_dispatch_trace.lua
--
-- Pin HOW the field VM reaches a 0x3F named-scene-change ("door") op, to
-- decide whether the per-scene destination table can be safely resized
-- (variable-length randomization). The 0x3F ops live in a data blob the
-- controller indexes -- this probe captures the runtime control flow INTO
-- the op so we learn the indexing mechanism:
--   * if the op is reached by a jump within the SAME VM context (one a0
--     bytecode base, pc walks/jumps to the entry), the entry is addressed
--     by a jump in clean code (resize -> fix the jump target);
--   * if the op's context is freshly spawned with pc preset at the entry,
--     a dispatch table set that pc (resize -> relocate that table).
--
-- Scenario: drake_castle_to_worldmap (a Drake Castle field scene; a brief
-- Up press walks into the exit and warps to the Drake Kingdom world map).
-- We hold UP to trigger the transition during the capture window.
--
-- BP: FUN_801de840 per-op dispatch (a0=bytecode ptr, a1=pc, a2=ctx).
-- For each hit we read op=[a0+a1]&0x7F and keep a short per-ctx pc ring.
-- On op==0x3F we snapshot: frame, ctx, bytecode base, entry pc, the index
-- field, the inline name, and the last 16 pcs in that ctx.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 200)
local HOLD_FRAMES = probe.getenv_num("LEGAIA_HOLD", 90)
local OUT_PATH    = probe.out_path("door_dispatch_trace.txt")
local VM_CAP      = probe.getenv_num("LEGAIA_VM_CAP", 4000000)

local function read_name(addr, len)
    if len <= 0 or len > 16 then return "?" end
    local s = {}
    for i = 0, len - 1 do
        local b = probe.read_u8(addr + i) or 0
        s[#s + 1] = (b >= 0x20 and b <= 0x7e) and string.char(b) or "."
    end
    return table.concat(s)
end

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    hold_button    = probe.BTN.UP,
    hold_frames    = HOLD_FRAMES,
    snapshot_path  = OUT_PATH:gsub("%.txt$", ".summary.txt"),

    on_arm = function(ctx)
        ctx.frame = 0
        ctx.vm_n = 0
        ctx.ctxs = {}          -- a2 -> { hist = {ring of pc}, first_pc, first_a0, hits }
        ctx.hits = {}          -- captured 0x3F executions
        ctx.f = assert(io.open(OUT_PATH, "w"))
        ctx.f:write("# door dispatch trace: 0x3F scene-change executions\n")

        probe.arm_breakpoint(0x801de840, "Exec", 4, "vm_step", function()
            ctx.vm_n = ctx.vm_n + 1
            if ctx.vm_n > VM_CAP then return end
            local r  = PCSX.getRegisters()
            local a0 = tonumber(r.GPR.n.a0) or 0
            local a1 = tonumber(r.GPR.n.a1) or 0
            local a2 = tonumber(r.GPR.n.a2) or 0
            local rec = ctx.ctxs[a2]
            if not rec then
                rec = { hist = {}, first_pc = a1, first_a0 = a0, hits = 0,
                        first_frame = ctx.frame }
                ctx.ctxs[a2] = rec
            end
            rec.hits = rec.hits + 1
            local h = rec.hist
            h[#h + 1] = a1
            if #h > 18 then table.remove(h, 1) end

            if a0 < 0x80000000 then return end
            local op = probe.read_u8(a0 + a1) or 0
            local ext = (bit.band(op, 0x80) ~= 0)
            op = bit.band(op, 0x7F)
            if op == 0x3F then
                local opnd = a1 + (ext and 2 or 1)
                local index = probe.read_u16(a0 + opnd) or 0
                local name_len = probe.read_u8(a0 + opnd + 2) or 0
                local name = read_name(a0 + opnd + 3, name_len)
                ctx.hits[#ctx.hits + 1] = true
                ctx.f:write(string.format(
                    "\n=== 0x3F @ frame %d  ctx=0x%08X  a0=0x%08X  pc=0x%04X  index=%d  name='%s' ===\n",
                    ctx.frame, a2, a0, a1, index, name))
                ctx.f:write(string.format(
                    "  ctx first seen frame %d at pc=0x%04X (a0=0x%08X); this ctx total hits=%d\n",
                    rec.first_frame, rec.first_pc, rec.first_a0, rec.hits))
                local pcs = {}
                for _, p in ipairs(h) do pcs[#pcs + 1] = string.format("0x%04X", p) end
                ctx.f:write("  recent pc trail (this ctx): " .. table.concat(pcs, " -> ") .. "\n")

                -- Is a0 a partition-record base? Compare a0-man_base to the
                -- partition record-offset tables in the live MAN.
                local man_base = probe.read_u32(0x8007B898) or 0
                ctx.f:write(string.format("  MAN base _DAT_8007B898=0x%08X; a0-base=0x%X\n",
                    man_base, a0 - man_base))
                if man_base >= 0x80000000 then
                    -- header: counts @0x22/24/26, u24 @0x28, table @0x2B
                    local n0 = probe.read_u16(man_base + 0x22) or 0
                    local n1 = probe.read_u16(man_base + 0x24) or 0
                    local n2 = probe.read_u16(man_base + 0x26) or 0
                    local dro = 0x2B + 3 * (n0 + n1 + n2)
                    ctx.f:write(string.format("  MAN counts N0=%d N1=%d N2=%d  data_region=0x%X\n",
                        n0, n1, n2, dro))
                    local rel = a0 - man_base
                    -- dump every partition entry; flag the one equal to a0-base
                    -- (entry offset) or a0-base-dro (data-region-relative).
                    local cur = 0x2B
                    for part = 0, 2 do
                        local n = ({ n0, n1, n2 })[part + 1]
                        local hits_here = {}
                        for i = 0, n - 1 do
                            local off = (probe.read_u8(man_base + cur) or 0)
                                + (probe.read_u8(man_base + cur + 1) or 0) * 256
                                + (probe.read_u8(man_base + cur + 2) or 0) * 65536
                            local abs = dro + off
                            if abs == rel or off == rel or abs == a1 or off == (rel - 0) then
                                hits_here[#hits_here + 1] = string.format("rec%d off=0x%X(abs=0x%X)", i, off, abs)
                            end
                            cur = cur + 3
                        end
                        if #hits_here > 0 then
                            ctx.f:write(string.format("  PARTITION %d match: %s\n",
                                part, table.concat(hits_here, ", ")))
                        end
                    end
                    -- Also: dump the record prefix bytes at a0 (a0+0..a0+0x16).
                    local pre = {}
                    for i = 0, 0x16 do pre[#pre + 1] = string.format("%02X", probe.read_u8(a0 + i) or 0) end
                    ctx.f:write("  a0[0..0x16]: " .. table.concat(pre, " ") .. "\n")
                end
                ctx.f:flush()
            end
        end)
        PCSX.log("[door] armed FUN_801de840 dispatch BP; holding UP")
        return {}
    end,

    on_capture = function(ctx, elapsed)
        ctx.frame = elapsed
    end,

    on_done = function(ctx)
        ctx.f:write(string.format("\n# total VM ops: %d (cap %d), distinct ctxs: %d, 0x3F hits: %d\n",
            ctx.vm_n, VM_CAP, (function() local n = 0 for _ in pairs(ctx.ctxs) do n = n + 1 end return n end)(),
            #ctx.hits))
        ctx.f:close()
        PCSX.log(string.format("[door] done: %d VM ops, %d 0x3F executions -> %s",
            ctx.vm_n, #ctx.hits, OUT_PATH))
    end,
})
