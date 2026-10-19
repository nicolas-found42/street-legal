// @category SLR
// M3-01 read-only queries on the analysed project (run with -process -noanalysis, see docs/physics.md).
//   QueryExe.java mnemonic FLDCW FNSTCW ...   -> every instruction with one of these mnemonics, plus containing function
//   QueryExe.java xrefs 0x4a5190 ...          -> all references (code+data) to each address, plus containing function
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.*;
import java.util.*;

public class QueryExe extends GhidraScript {
    private String fn(Address a) {
        Function f = currentProgram.getFunctionManager().getFunctionContaining(a);
        return f == null ? "-" : f.getEntryPoint() + "(" + f.getName() + ")";
    }

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        String mode = args[0];
        if (mode.equals("mnemonic")) {
            Set<String> want = new HashSet<>();
            for (int i = 1; i < args.length; i++) want.add(args[i].toUpperCase());
            InstructionIterator it = currentProgram.getListing().getInstructions(true);
            while (it.hasNext()) {
                Instruction ins = it.next();
                if (want.contains(ins.getMnemonicString().toUpperCase()))
                    println("Q " + ins.getAddress() + " " + ins + " in " + fn(ins.getAddress()));
            }
        } else if (mode.equals("xrefs")) {
            for (int i = 1; i < args.length; i++) {
                Address a = toAddr(Long.parseLong(args[i].replace("0x", ""), 16));
                for (Reference r : getReferencesTo(a))
                    println("Q " + a + " <- " + r.getFromAddress() + " " + r.getReferenceType() + " in " + fn(r.getFromAddress()));
            }
        }
    }
}
