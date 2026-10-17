# 0003: Procedural tracks and parametric engines for the first slice

Map pieces (SCX v3, `.scy/.scm/.spl`) and the part-script engine model (TUFA VM) are not decoded.
To keep the loop playable and honest, tracks are procedural (drag strip, circuit) and engines
are parametric (torque peak, rpm, redline per stock block, sized from the part names).
Replacing these with decoded data is the main fidelity upgrade path.
