// @category SLR
// M3-01: create Ghidra functions at the native-method implementation entries that the
// original export left as LAB_xxxxxxxx (258 of 353 natives), plus any CALL targets reachable
// from them that still have no function, then decompile every function created here.
//
// Args: <outDir> <native_api.tsv> [extraEntryHex ... | vtables]
// `vtables` also creates a function at every code address that a .rdata/.data relocation site points to
// when that address lies outside every existing function (vtable slots, cfg handler tables).
// Writes (git-ignored, derived from the exe, never committed):
//   <outDir>/native_pseudo.c      "//==== <addr> <name>" blocks for the functions created here
//   <outDir>/native_created.txt   "<addr> <name> <size> <origin>" origin = native|callee|extra
// The program is NOT saved by this script; run headless with -overwrite or a project that
// the caller keeps (see docs/physics.md "Method").
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.app.cmd.disassemble.DisassembleCommand;
import ghidra.program.model.address.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.*;
import java.io.*;
import java.util.*;

public class CreateNativeFunctions extends GhidraScript {
    private final LinkedHashMap<Address, String> created = new LinkedHashMap<>();

    private boolean ensureFunction(Address a, String origin) throws Exception {
        if (currentProgram.getFunctionManager().getFunctionAt(a) != null) return false;
        if (getInstructionAt(a) == null) {
            disassemble(a);
        }
        if (getInstructionAt(a) == null) return false;
        Function f = createFunction(a, null);
        if (f == null) return false;
        created.put(a, origin);
        return true;
    }

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        String outDir = args[0];
        String tsv = args[1];
        List<Address> work = new ArrayList<>();
        try (BufferedReader br = new BufferedReader(new FileReader(tsv))) {
            String line;
            while ((line = br.readLine()) != null) {
                String[] p = line.split("\t");
                if (p.length < 4) continue;
                String impl = p[3].trim();
                int u = impl.lastIndexOf('_');
                if (u < 0) continue;
                String hex = impl.substring(u + 1);
                try {
                    Address a = toAddr(Long.parseLong(hex, 16));
                    if (ensureFunction(a, "native")) work.add(a);
                } catch (NumberFormatException e) { /* skip */ }
            }
        }
        for (int i = 2; i < args.length; i++) {
            if (args[i].equals("vtables")) {
                var mem = currentProgram.getMemory();
                var fm = currentProgram.getFunctionManager();
                var rit = currentProgram.getRelocationTable().getRelocations();
                List<Address> targets = new ArrayList<>();
                while (rit.hasNext()) {
                    var rel = rit.next();
                    Address site = rel.getAddress();
                    var blk = mem.getBlock(site);
                    if (blk == null) continue;
                    String bn = blk.getName();
                    if (!(bn.equals(".rdata") || bn.equals(".data"))) continue;
                    Address t;
                    try { t = toAddr(mem.getInt(site) & 0xffffffffL); } catch (Exception e) { continue; }
                    var tb = mem.getBlock(t);
                    if (tb == null || !tb.getName().equals(".text")) continue;
                    if (fm.getFunctionContaining(t) != null) continue;
                    targets.add(t);
                }
                for (Address t : targets) if (ensureFunction(t, "vtable")) work.add(t);
                continue;
            }
            Address a = toAddr(Long.parseLong(args[i].replace("0x", ""), 16));
            if (ensureFunction(a, "extra")) work.add(a);
        }
        // Callee closure: CALL targets inside created functions that have no function yet.
        for (int round = 0; round < 6 && !work.isEmpty(); round++) {
            List<Address> next = new ArrayList<>();
            for (Address fa : work) {
                Function f = currentProgram.getFunctionManager().getFunctionAt(fa);
                if (f == null) continue;
                InstructionIterator it = currentProgram.getListing().getInstructions(f.getBody(), true);
                while (it.hasNext()) {
                    Instruction ins = it.next();
                    if (!ins.getFlowType().isCall()) continue;
                    for (Address t : ins.getFlows()) {
                        if (t == null || !currentProgram.getMemory().contains(t)) continue;
                        if (ensureFunction(t, "callee")) next.add(t);
                    }
                }
            }
            work = next;
        }
        DecompInterface ifc = new DecompInterface();
        ifc.openProgram(currentProgram);
        PrintWriter all = new PrintWriter(new FileWriter(outDir + "/native_pseudo.c"));
        PrintWriter idx = new PrintWriter(new FileWriter(outDir + "/native_created.txt"));
        for (Map.Entry<Address, String> e : created.entrySet()) {
            Function f = currentProgram.getFunctionManager().getFunctionAt(e.getKey());
            if (f == null) continue;
            idx.println(f.getEntryPoint() + " " + f.getName() + " " + f.getBody().getNumAddresses() + " " + e.getValue());
            all.println("//==== " + f.getEntryPoint() + " " + f.getName() + " (" + e.getValue() + ")");
            DecompileResults r = ifc.decompileFunction(f, 60, monitor);
            if (r.decompileCompleted()) all.println(r.getDecompiledFunction().getC());
            else all.println("// FAILED " + r.getErrorMessage());
        }
        all.close();
        idx.close();
        println("created " + created.size());
    }
}
