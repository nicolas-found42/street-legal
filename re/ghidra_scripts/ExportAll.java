// @category SLR
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.listing.*;
import java.io.*;

public class ExportAll extends GhidraScript {
    @Override
    public void run() throws Exception {
        String outDir = getScriptArgs()[0];
        DecompInterface ifc = new DecompInterface();
        ifc.openProgram(currentProgram);
        PrintWriter all = new PrintWriter(new FileWriter(outDir + "/all_pseudo.c"));
        PrintWriter idx = new PrintWriter(new FileWriter(outDir + "/index.txt"));
        FunctionIterator it = currentProgram.getFunctionManager().getFunctions(true);
        int n = 0;
        while (it.hasNext()) {
            Function f = it.next();
            if (f.isExternal() || f.isThunk()) continue;
            DecompileResults r = ifc.decompileFunction(f, 60, monitor);
            String addr = f.getEntryPoint().toString();
            idx.println(addr + " " + f.getName() + " " + f.getBody().getNumAddresses());
            all.println("//==== " + addr + " " + f.getName());
            if (r.decompileCompleted()) all.println(r.getDecompiledFunction().getC());
            else all.println("// FAILED " + r.getErrorMessage());
            n++;
        }
        all.close(); idx.close();
        println("exported " + n);
    }
}
