# @category Legaia
# @runtime Jython
#
# Report every program in the project with the VA span of its memory blocks
# and its analyzed function count.
#
# The load base is the one fact that decides whether a dump taken from a
# program is usable at all: get it wrong and every printed address is off by a
# constant while the instruction text stays plausible (see
# docs/tooling/dump-corpus-integrity.md). `list_programs.py` names the imports;
# this one says where each of them thinks it lives, which is what a re-dump
# pass has to know before it picks a program to run against.
#
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process SCUS_942.54 -noanalysis \
#       -preScript /scripts/report_program_bases.py

from ghidra.util.task import ConsoleTaskMonitor

monitor = ConsoleTaskMonitor()
project = state.getProject()
data = project.getProjectData()


def walk(folder, out):
    for f in folder.getFiles():
        out.append(f)
    for sub in folder.getFolders():
        walk(sub, out)


files = []
walk(data.getRootFolder(), files)

print("PROGBASE\tname\tmin_va\tmax_va\tblocks\tfunctions")
for f in sorted(files, key=lambda x: x.getName()):
    if f.getContentType() != "Program":
        continue
    prog = None
    try:
        prog = f.getImmutableDomainObject(state, -1, monitor)
        mem = prog.getMemory()
        blocks = [b for b in mem.getBlocks()]
        if not blocks:
            print("PROGBASE\t{}\t-\t-\t0\t0".format(f.getName()))
            continue
        lo = min(b.getStart().getOffset() for b in blocks)
        hi = max(b.getEnd().getOffset() for b in blocks)
        nfun = prog.getFunctionManager().getFunctionCount()
        print("PROGBASE\t{}\t{:08x}\t{:08x}\t{}\t{}".format(
            f.getName(), lo, hi, len(blocks), nfun))
    except Exception as e:
        print("PROGBASE\t{}\tERR\t{}\t0\t0".format(f.getName(), e))
    finally:
        if prog is not None:
            prog.release(state)

print("done [report_program_bases]")
