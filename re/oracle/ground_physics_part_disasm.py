#!/usr/bin/env python3
"""Disassembly sweep for the GroundRef/PhysicsRef/Part native bodies (worker NativesGroundPhysics).

Linear disassembly from each native entry until a terminating ret/int3 padding
boundary; the text is written to re/out/boot/ground-physics-part/disasm.txt for
reading. Addresses come from crates/slr-boot/src/missing_rows.rs rows owned by
the GroundRef+PhysicsRef+Part slice.
"""
import json
from pathlib import Path
from oracle import Oracle

ROWS = {
    # Part (10 rows not owned by parts.rs)
    "Part.getSlotDamage": 0x468AB0,
    "Part.setSlotDamage": 0x468C50,
    "Part.setSlotPos": 0x468DF0,
    "Part.getMesh": 0x469CC0,
    "Part.setMesh": 0x469D90,
    "Part.getLogo": 0x46A0E0,
    "Part.getMass": 0x469290,
    "Part.setSfxLoopParams": 0x46A1B0,
    "Part.flap": 0x46A5A0,
    "Part.install_OK": 0x46A680,
    # GroundRef water/fog
    "GroundRef.setWater(FFF)": 0x4866C0,
    "GroundRef.setWater(V3V3FF)": 0x4867B0,
    "GroundRef.addWaterLimit": 0x486920,
    "GroundRef.setFog": 0x486A20,
    # GroundRef traffic
    "GroundRef.addTrafficCar": 0x484730,
    "GroundRef.remTrafficCar": 0x484A90,
    "GroundRef.notifyTrafficCar": 0x484AF0,
    "GroundRef.addTrafficN": 0x484050,
    "GroundRef.addTrafficP": 0x484420,
    "GroundRef.delTraffic": 0x484B50,
    "GroundRef.setTrafficCarBehaviour": 0x487EC0,
    "GroundRef.haltTrafficCross": 0x484B90,
    "GroundRef.haltTrafficPath": 0x4835E0,
    "GroundRef.getNearestCross": 0x483D00,
    "GroundRef.getStartDirection": 0x483400,
    "GroundRef.getRouteLength(V3V3)": 0x483750,
    "GroundRef.alignToRoad": 0x483E60,
    "GroundRef.getRouteLength()": 0x483C00,
    "GroundRef.getRoutePos": 0x483B30,
    "GroundRef.getRouteDist": 0x483C20,
    "GroundRef.setPedestrianDensityN": 0x484D30,
    "GroundRef.addPedestrianType": 0x484D90,
    "GroundRef.removePedestrianType": 0x484E20,
    "GroundRef.pedestrianDistance": 0x484C60,
    # PhysicsRef
    "PhysicsRef.create": 0x4807F0,
    "PhysicsRef.createBox": 0x4805B0,
    "PhysicsRef.createSphere": 0x4806F0,
    "PhysicsRef.setMatrix": 0x480920,
    "PhysicsRef.getPos": 0x480B00,
    "PhysicsRef.getOri": 0x480C10,
}


def sweep(o: Oracle, va: int, max_bytes: int = 4096) -> list[str]:
    code = o.read(va, max_bytes)
    lines = []
    for ins in o._cs.disasm(code, va):
        lines.append(f"  {ins.address:06x}  {ins.mnemonic} {ins.op_str}")
        if ins.mnemonic.startswith("ret"):
            # peek padding: stop when next byte is int3 or the block aligned past
            nxt = ins.address + ins.size
            pad = o.read(nxt, 1)
            if pad in (b"\xcc", b"\x90") or nxt % 16 == 0:
                break
    return lines


def run(out: Path) -> None:
    o = Oracle()
    out.parent.mkdir(parents=True, exist_ok=True)
    with out.open("w") as fh:
        for name, va in ROWS.items():
            fh.write(f"== {name} FUN_{va:08x}\n")
            for line in sweep(o, va):
                fh.write(line + "\n")
            fh.write("\n")
    print(f"wrote {out}")


if __name__ == "__main__":
    root = Path(__file__).resolve().parents[2]
    run(root / "re/out/boot/ground-physics-part/disasm.txt")
