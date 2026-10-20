"""Passive observation of the original interpreter, resource creation and main loop.

Only Unicorn code hooks on single addresses; nothing here writes a register,
memory or the program counter, and no original function is replaced. Observed
facts are written to (a) the SLRLOG call log as native-call records and (b) a
JSON sidecar. Addresses are build 606 (`StreetLegal_Redline.bak.exe`):

* 0x420ff0            `Thread::run` entry
* 0x42007e/8c/a2/b7   `call edi` sites that invoke a registered native with one
                      stack argument, a pointer to `CallInfo` (docs/tufa-executor.md)
* 0x4200be            the common join; EBP holds the native's result word
                      (float results are the f32 bits)
* 0x4255e0            call-by-name into a script (wrapper 0x425a90)
* 0x53a5e0            resource instance creation (Config / GameInit)
* 0x427980            boot function (ECX = system context)
* 0x418d10/0x426fd0/0x427160/0x4291e0/0x4265c0   main-loop calls
"""

from __future__ import annotations

import json
import struct

from unicorn import UC_HOOK_CODE
from unicorn import x86_const as X

import calllog
from resource_history import ResourceHistory
from menu_history import MenuHistory
from package_history import PackageHistory

THREAD_RUN = 0x420FF0
NATIVE_PRE_SITES = (0x42007E, 0x42008C, 0x4200A2, 0x4200B7)
NATIVE_JOIN = 0x4200BE
CALL_BY_NAME = 0x4255E0
CREATE_INSTANCE = 0x53A5E0
BOOT = 0x427980
INIT_LOOP = 0x428960
LOOP_CALLS = {
    0x418D10: ("FUN_00418d10", "thiscall", 1),
    0x426FD0: ("FUN_00426fd0", "thiscall", 1),
    0x427160: ("FUN_00427160", "fastcall", 0),
    0x4291E0: ("FUN_004291e0", "cdecl", 2),
    0x4265C0: ("FUN_004265c0", "cdecl", 5),
    0x4277F0: ("FUN_004277f0", "cdecl", 0),
    0x428450: ("FUN_00428450", "cdecl", 0),
}
# Engine classes the port's observe() skips; kept for the filtered natives log.
PORT_SKIPPED_CLASSES = ("java.render.", "java.sound.")
PORT_SKIPPED_EXACT = ("java.io.Input", "java.io.MouseCursor", "java.io.Controller")

NODE_DISPATCH = 0x4210C2  # movsx eax, word [ecx] in Thread::run; ECX = node, EBX = thread
FRAME_CONSTRUCTOR = 0x407B10  # thiscall(frame, a1, a2, a3 = body entry node, ...)
CALL_METHOD = 0x41FBC0  # thiscall(thread, call record, ...)
CONDITIONAL_OPWORDS = (0x1015, 0x1016, 0x0004, 0x0005)
LVALUE_VTABLE = 0x5E7344
VALUE_VTABLE = 0x5E734C
CONFIG_GETTERS = {
    0x426100: ("FUN_00426100", "int"),
    0x426170: ("FUN_00426170", "int"),
    0x4261F0: ("FUN_004261f0", "float"),
    0x426270: ("FUN_00426270", "string"),
}
CONTEXT_POINTER = 0x63641C
PHYSICS_POINTER_SLOT = 0x617650
GLOBAL_WORDS = {
    "DAT_0063c534": 0x63C534,
    "DAT_0063c538": 0x63C538,
    "DAT_0063c530": 0x63C530,
    "DAT_0062f270": 0x62F270,
    "DAT_0063c520": 0x63C520,
    "_DAT_0060c8c4": 0x60C8C4,
    "DAT_0060c8cc": 0x60C8CC,
}
CONTEXT_OFFSETS = (0xE4, 0xF0, 0xF4, 0x100, 0x104, 0x108, 0x110)
PHYSICS_OFFSETS = (0x00, 0x04, 0x08, 0x20, 0x104)
MAX_NATIVE_DETAIL = 200000
MAX_LOOP_DETAIL = 4000


def _ext_to_float(raw):
    """x87 80-bit extended (mantissa, sign|exponent) to a Python float."""
    import math

    mantissa, sign_exponent = struct.unpack("<QH", raw)
    sign = -1.0 if sign_exponent & 0x8000 else 1.0
    exponent = sign_exponent & 0x7FFF
    if exponent == 0 and mantissa == 0:
        return sign * 0.0
    if exponent == 0x7FFF:
        return sign * float("inf") if (mantissa << 1) & ((1 << 64) - 1) == 0 else float("nan")
    return sign * math.ldexp(mantissa, exponent - 16383 - 63)


def f32(word):
    return struct.unpack("<f", struct.pack("<I", word & 0xFFFFFFFF))[0]


def signed(word):
    word &= 0xFFFFFFFF
    return word - (1 << 32) if word >= 1 << 31 else word


class Observer:
    def __init__(self, host, natives_path=None, raw_path=None, all_path=None, steps_path=None,
                 max_steps=3_000_000):
        self.host = host
        self.steps = open(steps_path, "w") if steps_path else None
        self.max_steps = max_steps
        self.step_count = 0
        self.frame_entries = {}
        self.frame_descriptors = {}
        self.last_call_descriptor = None
        self.class_names = {}
        self.uc = host.uc
        self.all_log = calllog.Writer(all_path) if all_path else None
        self.config_reads = []
        self.viewports = []
        self.natives_log = calllog.Writer(natives_path) if natives_path else None
        self.raw_log = calllog.Writer(raw_path) if raw_path else None
        self.native_pending = []
        self.object_ids = {}
        self.stats = {"thread_run": 0, "native_calls": 0, "call_by_name": 0}
        self.native_counts = {}
        self.thread_runs = []
        self.natives = []
        self.by_name = []
        self.creations = []
        self.snapshots = []
        self.loop = []
        self.loop_counts = {}
        self.boot_context = None
        self.watch_pending = {}
        self.return_hooked = set()
        self.first_script_icount = None
        self.first_loop_icount = None
        self.loop_iterations = 0
        self.install()
        self.resource_history = ResourceHistory(self)
        self.menu_history = MenuHistory(self)
        self.package_history = PackageHistory(self)

    # ------------------------------------------------------------- plumbing
    def hook(self, address, callback):
        self.uc.hook_add(UC_HOOK_CODE, callback, None, address, address)

    def reg(self, name):
        return self.uc.reg_read(getattr(X, "UC_X86_REG_" + name.upper()))

    def u32(self, address):
        return struct.unpack("<I", bytes(self.uc.mem_read(address, 4)))[0]

    def try_u32(self, address):
        try:
            return self.u32(address)
        except Exception:
            return None

    def cstr(self, address, limit=256):
        out = bytearray()
        try:
            while len(out) < limit:
                byte = bytes(self.uc.mem_read(address + len(out), 1))
                if byte == b"\0":
                    break
                out += byte
        except Exception:
            return None
        return out.decode("latin-1")

    def install(self):
        self.hook(THREAD_RUN, self.on_thread_run)
        for site in NATIVE_PRE_SITES:
            self.hook(site, self.on_native_pre)
        self.hook(NATIVE_JOIN, self.on_native_join)
        self.hook(CALL_BY_NAME, self.on_call_by_name)
        self.hook(CREATE_INSTANCE, self.on_create_instance)
        self.hook(BOOT, self.on_boot)
        for address in LOOP_CALLS:
            self.hook(address, self.on_loop_call)
        # Return hooks are installed statically at the instruction after every
        # direct `call` of the watched functions (adding hooks mid-run is not
        # reliable in Unicorn).
        if self.steps is not None:
            self.hook(NODE_DISPATCH, self.on_node)
            self.hook(FRAME_CONSTRUCTOR, self.on_frame)
            self.hook(CALL_METHOD, self.on_call_method)
        for address in CONFIG_GETTERS:
            self.hook(address, self.on_config_getter)
        for target in (CREATE_INSTANCE, BOOT, *CONFIG_GETTERS):
            for site in self.call_sites(target):
                if site not in self.return_hooked:
                    self.return_hooked.add(site)
                    self.hook(site, self.on_watch_return)

    def call_sites(self, target):
        """Addresses following each `call rel32` to `target` in the executable's .text."""
        start, end = self.host.text_range
        code = bytes(self.uc.mem_read(start, end - start))
        sites = []
        index = code.find(b"\xe8")
        while index != -1 and index + 5 <= len(code):
            displacement = struct.unpack_from("<i", code, index + 1)[0]
            if (start + index + 5 + displacement) & 0xFFFFFFFF == target:
                sites.append(start + index + 5)
            index = code.find(b"\xe8", index + 1)
        return sites

    def snapshot(self, label):
        context = self.try_u32(CONTEXT_POINTER)
        entry = {"label": label, "icount": self.host.icount, "context_pointer": context}
        if context:
            entry["context"] = {
                hex(o): self.try_u32(context + o) for o in CONTEXT_OFFSETS
            }
            entry["context"]["+0x108_float"] = (
                f32(entry["context"]["0x108"]) if entry["context"]["0x108"] is not None else None
            )
            entry["context"]["+0x110_float"] = (
                f32(entry["context"]["0x110"]) if entry["context"]["0x110"] is not None else None
            )
        slot = self.try_u32(PHYSICS_POINTER_SLOT)
        entry["physics_slot_0x617650"] = slot
        if slot:
            words = {hex(o): self.try_u32(slot + o) for o in PHYSICS_OFFSETS}
            entry["physics_object_words"] = words
            entry["physics_object_floats"] = {
                k: (f32(v) if v is not None else None) for k, v in words.items()
            }
        entry["globals"] = {name: self.try_u32(address) for name, address in GLOBAL_WORDS.items()}
        entry["globals_float"] = {
            name: (f32(v) if v is not None else None) for name, v in entry["globals"].items()
        }
        self.snapshots.append(entry)

    # ----------------------------------------------------------- Thread::run
    def on_thread_run(self, uc, address, size, user):
        self.stats["thread_run"] += 1
        if self.first_script_icount is None:
            self.first_script_icount = self.host.icount
            self.snapshot("first_thread_run")
        if len(self.thread_runs) < 200:
            esp = self.reg("esp")
            self.thread_runs.append(
                {
                    "icount": self.host.icount,
                    "thread": self.reg("ecx"),
                    "budget_bits": self.u32(esp + 4),
                    "budget": f32(self.u32(esp + 4)),
                    "caller": self.u32(esp),
                }
            )

    # --------------------------------------------------------- native calls
    def on_native_pre(self, uc, address, size, user):
        esp = self.reg("esp")
        info = self.u32(esp)
        jvm, thread, container, record = (self.u32(info + 4 * i) for i in range(4))
        cls = self.cstr(self.u32(record + 0xC))
        name = self.cstr(self.u32(record + 0x10))
        sig_record = self.u32(record + 0x14)
        descriptor = self.cstr(self.u32(sig_record + 4)) if sig_record else None
        data, count = self.u32(container + 4), self.u32(container + 8)
        cells = []
        for index in range(count - 2, -1, -1):  # first declared parameter first
            cell = self.u32(data + 4 * index)
            type_text = self.u32(cell + 4)
            kind = self.cstr(type_text, 64) if type_text else None
            cells.append({"kind": kind, "payload": self.u32(cell + 8), "cell": cell})
        returns = descriptor.rsplit(")", 1)[1] if descriptor and ")" in descriptor else None
        self.native_pending.append(
            {
                "key": esp + 4,
                "class": cls,
                "name": name,
                "descriptor": descriptor,
                "return": returns,
                "args": cells,
                "icount": self.host.icount,
                "thread": thread,
                "receiver_slot": self.u32(data + 4 * (count - 1)) if count else None,
            }
        )

    def object_id(self, pointer):
        if pointer == 0:
            return 0
        if pointer not in self.object_ids:
            self.object_ids[pointer] = len(self.object_ids) + 1
        return self.object_ids[pointer]

    def on_native_join(self, uc, address, size, user):
        esp = self.reg("esp")
        if not self.native_pending or self.native_pending[-1]["key"] != esp:
            return  # native not called on this path
        call = self.native_pending.pop()
        result_word = self.reg("ebp")
        self.stats["native_calls"] += 1
        key = f'{call["class"]}.{call["name"]}'
        self.native_counts[key] = self.native_counts.get(key, 0) + 1
        void = call["return"] == "V"
        port_args = []
        raw_args = []
        for cell in call["args"]:
            kind = (cell["kind"] or "?")[:1]
            if kind == "F":
                port_args.append(calllog.Float32(bits=cell["payload"]))
                raw_args.append(calllog.Float32(bits=cell["payload"]))
            elif kind in ("L", "["):
                port_args.append(self.object_id(cell["payload"]))
                raw_args.append(signed(cell["payload"]))
            else:
                port_args.append(signed(cell["payload"]))
                raw_args.append(signed(cell["payload"]))
        if void:
            port_result = raw_result = None
        elif call["return"] == "F":
            port_result = raw_result = calllog.Float32(bits=result_word)
        elif call["return"] in ("L", "["):
            port_result = self.object_id(result_word)
            raw_result = signed(result_word)
        else:
            port_result = raw_result = signed(result_word)
        entry = calllog.native_call(call["class"], call["name"], port_args, port_result, void=void)
        raw_entry = calllog.native_call(call["class"], call["name"], raw_args, raw_result, void=void)
        host = self.host
        if host.calllog is not None:
            host.calllog.record(entry)
        if self.all_log is not None:
            self.all_log.record(entry)
        if call["class"] == "java.render.Viewport" and call["name"] == "create":
            receiver_cell = call["receiver_slot"]
            receiver = self.try_u32(receiver_cell + 8) if receiver_cell else None
            self.viewports.append(
                {
                    "icount": call["icount"],
                    "args": [(c["kind"], c["payload"]) for c in call["args"]],
                    "result_word": result_word,
                    "receiver_object": receiver,
                    "result_object_words": self.words_at(result_word, 0x40),
                    "receiver_object_words": self.words_at(receiver, 0x40),
                    "result_object_floats_0x18_0x24": self.floats_at(result_word + 0x18, 4),
                    "receiver_object_floats_0x18_0x24": self.floats_at((receiver or 0) + 0x18, 4),
                }
            )
        skipped = call["class"].startswith(PORT_SKIPPED_CLASSES) or call["class"] in PORT_SKIPPED_EXACT
        if self.natives_log is not None and not skipped:
            self.natives_log.record(entry)
        if self.raw_log is not None and not skipped:
            self.raw_log.record(raw_entry)
        if len(self.natives) < MAX_NATIVE_DETAIL:
            self.natives.append(
                {
                    "i": self.stats["native_calls"],
                    "icount": call["icount"],
                    "thread": call["thread"],
                    "class": call["class"],
                    "name": call["name"],
                    "descriptor": call["descriptor"],
                    "args": [
                        {"kind": c["kind"], "payload": c["payload"]} for c in call["args"]
                    ],
                    "result": result_word if not void else None,
                    "receiver_slot": call["receiver_slot"],
                }
            )

    def words_at(self, address, nbytes):
        if not address or address < 0x1000:
            return None
        return [self.try_u32(address + o) for o in range(0, nbytes, 4)]

    def floats_at(self, address, count):
        words = self.words_at(address, 4 * count)
        return None if words is None else [f32(w) if w is not None else None for w in words]

    # ------------------------------------------------------- node-level trace
    def on_call_method(self, uc, address, size, user):
        esp = self.reg("esp")
        record = self.try_u32(esp + 4)
        method = self.try_u32(record + 8) if record else None
        signature = self.try_u32(method + 0x18) if method else None
        text = self.try_u32(signature + 4) if signature else None
        self.last_call_descriptor = self.cstr(text) if text and text > 0x1000 else None

    def on_frame(self, uc, address, size, user):
        frame = self.reg("ecx")
        entry = self.u32(self.reg("esp") + 12)  # a3: body entry node
        self.frame_entries[frame] = entry
        self.frame_descriptors[frame] = self.last_call_descriptor
        self.last_call_descriptor = None

    def class_name(self, class_object):
        name = self.class_names.get(class_object)
        if name is None:
            record = self.try_u32(class_object + 4) if class_object else None
            pointer = self.try_u32(record + 0x30) if record else None
            name = self.cstr(pointer) if pointer and pointer > 0x1000 else "?"
            self.class_names[class_object] = name
        return name

    def value_cell(self, cell):
        """Decode one operand-stack cell: (type char, payload, raw words)."""
        if not cell:
            return {"null_cell": True}
        raw = [self.try_u32(cell + 4 * i) for i in range(4)]
        vtable = raw[0]
        target, ty_pointer, payload = cell, raw[1], raw[2]
        kind = "value"
        inner_words = None
        if vtable == LVALUE_VTABLE:
            kind = "lvalue"
            for candidate in (raw[1], raw[3]):
                words = (
                    [self.try_u32(candidate + 4 * i) for i in range(4)]
                    if candidate and candidate > 0x1000
                    else None
                )
                if words and words[0] == VALUE_VTABLE:
                    inner_words = words
                    ty_pointer, payload = words[1], words[2]
                    break
            else:
                ty_pointer, payload = raw[1], raw[2]
        text = self.cstr(ty_pointer, 4) if ty_pointer and ty_pointer > 0x1000 else None
        return {
            "k": kind,
            "ty": text[:1] if text else None,
            "p": payload,
            "raw": raw,
            "inner": inner_words,
        }

    def on_node(self, uc, address, size, user):
        if self.step_count >= self.max_steps:
            self.step_count += 1
            return
        thread = self.reg("ebx")
        node = self.reg("ecx")
        frame = self.u32(thread + 0x20)
        word = bytes(self.uc.mem_read(node, 8))
        opword, _line, imm = struct.unpack("<HHI", word)
        class_object = self.u32(frame + 0x34)
        name_pointer = self.u32(frame + 0x38)
        entry = self.frame_entries.get(frame)
        record = {
            "s": self.step_count,
            "ic": self.host.icount,
            "t": thread,
            "f": frame,
            "c": self.class_name(class_object) if class_object else None,
            "m": self.cstr(name_pointer) if name_pointer > 0x1000 else None,
            "g": self.frame_descriptors.get(frame),
            "n": (node - entry) // 8 if entry and node >= entry else None,
            "o": opword,
            "i": imm,
        }
        if opword in CONDITIONAL_OPWORDS:
            container = frame + 0x1C
            count = self.u32(container + 8)
            data = self.u32(container + 4)
            record["v"] = self.value_cell(self.u32(data + 4 * (count - 1))) if count else None
            record["sd"] = count
        self.steps.write(json.dumps(record, separators=(",", ":")) + "\n")
        self.step_count += 1

    # -------------------------------------------------------- config getters
    def on_config_getter(self, uc, address, size, user):
        label, kind = CONFIG_GETTERS[address]
        esp = self.reg("esp")
        name_pointer = self.u32(esp + 4)
        record = {
            "icount": self.host.icount,
            "getter": label,
            "kind": kind,
            "name": self.cstr(name_pointer) if name_pointer > 0x1000 else name_pointer,
        }
        self.config_reads.append(record)
        ret = self.u32(esp)
        self.watch_pending.setdefault(ret, []).append(("config", len(self.config_reads) - 1))

    # -------------------------------------------------------- call-by-name
    def on_call_by_name(self, uc, address, size, user):
        self.stats["call_by_name"] += 1
        esp = self.reg("esp")
        node, name_pointer, flag, kind1, argp = (self.u32(esp + 4 * (i + 1)) for i in range(5))
        name = self.cstr(name_pointer) if name_pointer > 0x1000 else None
        values = []
        kind, cursor = kind1, argp
        for _ in range(16):
            if kind == 0x4F:
                break
            if kind == 3:  # float passed as a double
                low, high = self.u32(cursor), self.u32(cursor + 4)
                value = struct.unpack("<d", struct.pack("<II", low, high))[0]
                values.append((kind, calllog.Float32(value)))
                cursor += 8
            else:
                values.append((kind, signed(self.u32(cursor))))
                cursor += 4
            kind = self.u32(cursor)
            cursor += 4
        node_id = self.try_u32(node + 0x50) if node else None
        rec_args = [signed(node_id or 0), signed(flag)]
        for kind_code, value in values:
            rec_args.extend([signed(kind_code), value])
        entry = calllog.native_call("engine.FUN_004255e0", name or "?", rec_args, None, void=True)
        if self.host.calllog is not None:
            self.host.calllog.record(entry)
        if len(self.by_name) < 50000:
            self.by_name.append(
                {
                    "icount": self.host.icount,
                    "node": node,
                    "node_resource_id": node_id,
                    "method": name,
                    "wait_flag": flag,
                    "args": [
                        {"kind": k, "value": (float(v) if isinstance(v, float) else v)}
                        for k, v in values
                    ],
                }
            )

    # ------------------------------------------------------ resource creation
    def on_create_instance(self, uc, address, size, user):
        esp = self.reg("esp")
        args = [self.u32(esp + 4 * (i + 1)) for i in range(4)]
        record = {
            "icount": self.host.icount,
            "return_address": self.u32(esp),
            "args": args,
            "name": self.cstr(args[3]) if args[3] > 0x1000 else None,
            "game_root_handle_words": [self.try_u32(args[0] + o) for o in (0, 4, 8, 0xC)],
            "gametype_ref_words": [self.try_u32(args[1] + o) for o in (0, 4, 8, 0xC)],
        }
        self.creations.append(record)
        ret = record["return_address"]
        self.watch_pending.setdefault(ret, []).append(("create", len(self.creations) - 1))

    def on_watch_return(self, uc, address, size, user):
        stack = self.watch_pending.get(address)
        if not stack:
            return
        kind, index = stack.pop()
        if kind == "create":
            node = self.reg("eax")
            record = self.creations[index]
            record["returned_node"] = node
            if node:
                record["node_words"] = {
                    hex(o): self.try_u32(node + o) for o in (0x40, 0x44, 0x48, 0x4C, 0x50, 0x54)
                }
                record["resource_instance_id"] = self.try_u32(node + 0x50)
            record["return_icount"] = self.host.icount
            context = self.try_u32(CONTEXT_POINTER)
            if context:
                record["context_after"] = {
                    hex(o): self.try_u32(context + o) for o in (0xF0, 0xF4, 0x100, 0x104)
                }
        elif kind == "config":
            record = self.config_reads[index]
            eax = self.reg("eax")
            record["eax"] = eax
            if record["kind"] == "string":
                record["value"] = self.cstr(eax) if eax > 0x1000 else None
            elif record["kind"] == "float":
                top = (self.reg("fpsw") >> 11) & 7
                mantissa, exponent = self.uc.reg_read(X.UC_X86_REG_FP0 + top)
                record["value"] = _ext_to_float(struct.pack("<QH", mantissa, exponent))
            else:
                record["value"] = signed(eax)
        elif kind == "boot":
            context = self.boot_context
            self.snapshot("after_boot_FUN_00427980")
            self.boot = {
                "context": context,
                "words": {hex(o): self.try_u32(context + o) for o in CONTEXT_OFFSETS},
                "return_icount": self.host.icount,
            }

    def on_boot(self, uc, address, size, user):
        self.boot_context = self.reg("ecx")
        ret = self.u32(self.reg("esp"))
        self.watch_pending.setdefault(ret, []).append(("boot", 0))

    # -------------------------------------------------------------- main loop
    def on_loop_call(self, uc, address, size, user):
        name, convention, nstack = LOOP_CALLS[address]
        count = self.loop_counts.get(name, 0)
        self.loop_counts[name] = count + 1
        if address == 0x418D10:
            self.loop_iterations += 1
            self.menu_history.sample(self.reg("ecx"), self.loop_iterations)
            if self.first_loop_icount is None:
                self.first_loop_icount = self.host.icount
            if self.loop_iterations in (1, 2, 3, 10, 100):
                self.snapshot(f"main_loop_scheduler_call_{self.loop_iterations}")
        if len(self.loop) >= MAX_LOOP_DETAIL:
            return
        esp = self.reg("esp")
        args = [self.u32(esp + 4 * (i + 1)) for i in range(nstack)]
        entry = {"icount": self.host.icount, "fn": name, "iteration": self.loop_iterations}
        if convention in ("thiscall", "fastcall"):
            entry["ecx"] = self.reg("ecx")
        if name in ("FUN_00418d10", "FUN_00426fd0"):
            entry["float_arg"] = f32(args[0])
            entry["arg_bits"] = args[0]
        elif name == "FUN_004291e0":
            entry["float_arg"] = f32(args[0])
            entry["args"] = args
        elif args:
            entry["args"] = args
        self.loop.append(entry)

    def finish(self):
        if self.natives_log is not None:
            self.natives_log.close()
        if self.raw_log is not None:
            self.raw_log.close()
        if self.all_log is not None:
            self.all_log.close()
        if self.steps is not None:
            self.steps.close()

    def sidecar(self):
        return {
            "observer": "passive-unicorn-code-hooks",
            "alters_execution": False,
            "stats": self.stats,
            "first_thread_run_icount": self.first_script_icount,
            "first_main_loop_scheduler_call_icount": self.first_loop_icount,
            "main_loop_iterations_seen": self.loop_iterations,
            "thread_run_first_entries": self.thread_runs,
            "native_counts": dict(sorted(self.native_counts.items())),
            "resource_creations": self.creations,
            "resource_history": self.resource_history.sidecar(),
            "menu_history": self.menu_history.sidecar(),
            "package_history": self.package_history.sidecar(),
            "boot_function": getattr(self, "boot", None),
            "snapshots": self.snapshots,
            "main_loop_calls": self.loop,
            "main_loop_call_counts": self.loop_counts,
            "call_by_name": self.by_name,
            "config_reads": self.config_reads,
            "viewport_creates": self.viewports,
            "native_calls": self.natives,
            "script_steps": {"written": min(self.step_count, self.max_steps), "total_nodes": self.step_count},
            "native_calls_truncated": self.stats["native_calls"] > len(self.natives),
        }
