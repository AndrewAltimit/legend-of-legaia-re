// Dump disassembly for FUN_801ED710 (field overlay 0897) as a native GhidraScript.
// @category Legaia
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.address.AddressSetView;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.listing.InstructionIterator;
import ghidra.program.model.listing.Listing;
import java.io.FileWriter;
import java.io.PrintWriter;

public class DumpEd710 extends GhidraScript {
    @Override
    public void run() throws Exception {
        String progName = currentProgram.getName();
        Address addr = currentProgram.getAddressFactory().getAddress("801ed710");
        Function func = getFunctionContaining(addr);
        if (func == null) {
            func = getFunctionAt(addr);
        }
        if (func == null) {
            println("[skip] no function at 801ed710 in " + progName);
            return;
        }
        AddressSetView body = func.getBody();
        Listing listing = currentProgram.getListing();
        String label = progName.replace(".bin", "").replace(".", "_");
        String outPath = "/scripts/funcs/" + label + "_801ed710.txt";
        PrintWriter fh = new PrintWriter(new FileWriter(outPath));
        try {
            long nInstr = 0;
            InstructionIterator it0 = listing.getInstructions(body, true);
            while (it0.hasNext()) { it0.next(); nInstr++; }
            fh.println("== " + func.getName() + " 801ed710 (entry=" + func.getEntryPoint()
                    + ") [" + progName + "] ==");
            fh.println("size=" + body.getNumAddresses() + " bytes, " + nInstr + " instructions");
            fh.println();
            fh.println("--- DISASSEMBLY ---");
            InstructionIterator it = listing.getInstructions(body, true);
            while (it.hasNext()) {
                Instruction ins = it.next();
                fh.println(ins.getAddress() + "  " + ins.toString());
            }
        } finally {
            fh.close();
        }
        println("wrote " + outPath);
    }
}
