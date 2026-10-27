#!/usr/bin/env python3
"""Original part `install` command on genuine boot state.

Replays the captured 426800 entry for the first stock part installation
(`Part.addPart` -> `command("install 0 X 0 car 0")`). Everything executes as
original x86 except the VM boundaries a Rust port reaches through its script
host: class lookup 411600, object construction 407da0, field lookup 408800,
script invocation 425a90 and event delivery 425c60. Their arguments are recorded in order.

    python re/oracle/part_install_replay.py [snapshot]
"""
import struct
import sys

from snapshot_replay import VM_CELL, Replay, write

VM_CLASS = 0x30000100
# Original functions executed by this graph outside the stubbed VM.
GRAPH = [int(a, 16) for a in '''
426800 4265c0 475ec0 546070 473b10 473790 426470 4720a0 471cf0 470910 474600
48aba0 48f260 429c80 544fe0 545120 472e80 4707d0 470720 48aea0 54ebc0 54e4a0
436880 48af20 477560 54e2d0 54e9c0 4a3a20 54fb50 54eda0 4986f0 537790 4988a0
498b10 474170 46a250 474000 4a7990 49a760 49a810 49a840 54fa70 49a050 54e1b0
54eab0 54e8a0 54e510 54e3b0 498f50 45db90 45dbc0 4907e0 4a5930 4ac2b0 4ac6e0
54e1d0 4368f0 4a5950 4a7a80 470e20 48aaf0 4290f0 471620 471de0 470ff0 458c00
447b30 45faf0 43f2e0 419860 5447d0 429060 429010 4293a0 429390 551240 551220
'''.split()]
SCRIPT_END = 0x4F


def main():
    positional = [a for a in sys.argv[1:] if not a.startswith("--")]
    name = positional[0] if positional else "install-part-queue"
    o = Replay(name)
    cells = []

    def vm_class(oc, args):
        oc.record("vm_class", table=oc.reg("ecx"), name=oc.read_cstr(args[0]).decode("latin1"))
        return VM_CLASS

    def vm_object(oc, args):
        oc.record("vm_object", object=oc.reg("ecx"), vm_class=args[0], flags=args[1])
        return 0

    def vm_field(oc, args):
        cell = VM_CELL + 16 * len(cells)
        cells.append(cell)
        oc.record("vm_field", object=oc.reg("ecx"), name=oc.read_cstr(args[0]).decode("latin1"), cell=cell)
        return cell

    def script(oc, args):
        # 425a90(envelope, name, result type, (type, value)*, 0x4f).
        esp = oc.reg("esp")
        word = lambda i: struct.unpack("<I", bytes(oc.uc.mem_read(esp + 4 + 4 * i, 4)))[0]
        words = [word(2)]
        i = 3
        while True:
            if word(i) == SCRIPT_END:
                words.append(SCRIPT_END)
                break
            words += [word(i), word(i + 1)]
            i += 2
            if i > 16:
                raise RuntimeError("unterminated script argument list")
        oc.record(
            "script",
            instance=word(0),
            method=oc.read_cstr(word(1)).decode("latin1"),
            args=words,
            cells={hex(c): oc.read_u32(c + 8) for c in cells},
        )
        return None

    def event(oc, args):
        oc.record(
            "event",
            reference=args[0],
            source=args[1],
            event=args[2],
            argument=args[3],
            envelope=args[4],
            name=args[5],
        )
        return None

    def unexpected(address):
        def stop(oc, args):
            raise RuntimeError(f"undeclared VM boundary {address:#x}")

        return stop

    o.hook(0x411600, vm_class, "thiscall", 1)
    o.hook(0x407DA0, vm_object, "thiscall", 2)
    o.hook(0x408800, vm_field, "thiscall", 1)
    o.hook(0x425A90, script, "cdecl", 0)
    o.hook(0x425C60, event, "cdecl", 6)
    for address in (0x42A690, 0x4044E0, 0x42AB50):
        o.hook(address, unexpected(address), "cdecl", 0)
    if "--checkpoints" in sys.argv:
        o.checkpoint(GRAPH)
    eax = o.resume()
    payload = {
        "snapshot": name,
        "function": o.meta["function"],
        "eax": eax,
        "events": o.events,
        "heap_ptr": o.heap_ptr,
        "diff": o.diff(),
        "instructions": o.icount,
    }
    path = write(name, "install", payload)
    if o.checkpoints is not None:
        print(o.write_checkpoints("install"), len(o.checkpoints))
    print(path, len(payload["events"]), len(payload["diff"]), o.icount)


if __name__ == "__main__":
    main()
