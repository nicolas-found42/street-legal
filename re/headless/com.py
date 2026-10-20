"""Generic COM vtable host for declared platform scenarios.

Vtable layouts come from `com_tables.py` (generated from MinGW-w64 headers).
Every method is a 32-bit stdcall thunk in the emulator's import thunk window;
the object is `[vtable pointer, ...]` in the emulator heap. A method without an
explicit model handler stops by name (`unsupported_com_method`) before any
result is supplied. IUnknown AddRef/Release are COM reference counting, not a
scenario assumption; QueryInterface is only answered for the object's own
interface unless its model handles more.
"""

from __future__ import annotations

from com_tables import INTERFACES

E_NOINTERFACE = 0x80004002
S_OK = 0
THUNK_END = 0x7F010000


# Struct sizes for `const T *` input structures, as the hook layer's sizeof(T).
STRUCT_SIZES = {
    "D3DBOX": 24,
    "D3DCLIPSTATUS9": 8,
    "D3DGAMMARAMP": 768,
    "D3DLIGHT9": 104,
    "D3DMATERIAL9": 68,
    "D3DMATRIX": 64,
    "D3DRECT": 16,
    "RECT": 16,
    "POINT": 8,
    "D3DVIEWPORT9": 24,
    "D3DVERTEXELEMENT9": 8,
    "DWORD": 4,
    "float": 4,
    "int": 4,
    "WINBOOL": 4,
    "PALETTEENTRY": 4,
    "RGNDATA": 36,
    "D3DPRESENT_PARAMETERS": 56,
}
HANDLE_TYPES = {"HWND", "HDC", "HANDLE", "HMONITOR", "HMODULE", "HINSTANCE"}
# Non-const struct pointers that are real inputs; variable-length arrays.
FORCE_STRUCT = {
    ("IDirect3D9", "CreateDevice", "pPresentationParameters"),
    ("IDirect3DDevice9", "Reset", "pPresentationParameters"),
    ("IDirect3DDevice9", "CreateAdditionalSwapChain", "pPresentationParameters"),
}
LOG_SKIPPED_METHODS = {"AddRef", "Release"}


def classify(interface, method, ctype, name):
    """Argument class, mirroring re/hook/gen_com.py (K_INT/K_FLT/...)."""
    t = ctype.replace(" ", "").replace("struct", "")
    if t in ("REFIID", "REFGUID"):
        return "guid", None
    if t == "float":
        return "float", None
    if t in HANDLE_TYPES:
        return "ptr", None
    if t.endswith("**"):
        return ("outobj" if t.startswith("I") else "ptr"), None
    base = t[5:] if t.startswith("const") else t
    if base.startswith("I") and base.endswith("*") and base[1:2].isupper():
        return "obj", None
    if (interface, method, name) in FORCE_STRUCT:
        return "struct", STRUCT_SIZES["D3DPRESENT_PARAMETERS"]
    if "*" in t:
        if t.startswith("const") and not any(s in t for s in ("void*", "char*", "WCHAR*", "CHAR*")):
            struct_name = base.rstrip("*")
            return "struct", STRUCT_SIZES.get(struct_name, struct_name)
        return "ptr", None
    return "int", None


class ComObject:
    __slots__ = ("pointer", "interface", "model", "refcount", "state", "released", "id")

    def __init__(self, pointer, interface, model, state, ident):
        self.pointer = pointer
        self.interface = interface
        self.model = model
        self.refcount = 1
        self.state = state
        self.released = False
        self.id = ident


class ComHost:
    def __init__(self, host, boundary, budget=65536):
        self.host = host
        self.boundary = boundary
        self.objects = {}
        self.thunks = {}
        self.vtables = {}
        self.budget = budget
        self.sink = None
        self.counts = {}
        self.next_id = 1

    def vtable(self, interface):
        if interface in self.vtables:
            return self.vtables[interface]
        if interface not in INTERFACES:
            raise self.boundary("unknown_com_interface", interface=interface)
        addresses = []
        for slot, (method, params) in enumerate(INTERFACES[interface]):
            address = self.host.allocate_thunk(
                "com." + interface + "!" + interface + "." + method
            )
            self.thunks[address] = (interface, slot, method, params)
            addresses.append(address)
        table = self.host.alloc(4 * len(addresses))
        self.host.write_u32s(table, addresses)
        self.vtables[interface] = table
        return table

    def new_object(self, interface, model, state=None):
        if len(self.objects) >= self.budget:
            raise self.boundary("com_object_budget", limit=self.budget)
        table = self.vtable(interface)
        pointer = self.host.alloc(16)
        self.host.write_u32s(pointer, [table, 0, 0, 0])
        obj = ComObject(pointer, interface, model, {} if state is None else state, self.next_id)
        self.next_id += 1
        self.objects[pointer] = obj
        return obj

    def log_values(self, interface, method, params, words):
        """Call-log values for one call, in the hook layer's argument classes."""
        from calllog import Float32

        values = []
        for (ctype, name), word in zip(params, words):
            kind, size = classify(interface, method, ctype, name)
            if method == "Clear" and name == "rects":
                size = 16 * words[0]
            if kind == "int":
                values.append(word)
            elif kind == "float":
                values.append(Float32(bits=word))
            elif kind == "obj":
                target = self.objects.get(word)
                values.append(target.id if target is not None else 0)
            elif kind in ("struct", "guid"):
                size = 16 if kind == "guid" else size
                if not word:
                    values.append(False)
                elif not isinstance(size, int):
                    raise self.boundary("unsized_struct_argument", type=size, method=method)
                else:
                    try:
                        values.append(self.host.read(word, size))
                    except Exception:
                        values.append(True)
            else:
                values.append(word != 0)
        return values

    def invoke(self, address):
        """Run the model handler for the thunk at `address`; returns (event info)."""
        interface, slot, method, params = self.thunks[address]
        host = self.host
        esp = host.reg("esp")
        return_site = host.read_u32(esp)
        nargs = 1 + len(params)
        args = host.read_u32s(esp + 4, nargs)
        obj = self.objects.get(args[0])
        if obj is None or obj.interface != interface:
            raise self.boundary(
                "invalid_com_this",
                interface=interface,
                method=method,
                this=args[0],
                return_site=return_site,
            )
        if obj.released and method not in ("AddRef", "Release"):
            raise self.boundary(
                "released_com_object", interface=interface, method=method, this=args[0]
            )
        handler = getattr(obj.model, method, None)
        key = interface + "." + method
        self.counts[key] = self.counts.get(key, 0) + 1
        if self.sink is not None and method not in LOG_SKIPPED_METHODS:
            self.sink(obj, method, self.log_values(interface, method, params, args[1:]))
        if handler is not None:
            result = handler(obj, args[1:])
        elif method == "AddRef":
            obj.refcount += 1
            result = obj.refcount
        elif method == "Release":
            if obj.refcount == 0:
                raise self.boundary("com_release_underflow", interface=interface)
            obj.refcount -= 1
            result = obj.refcount
            if obj.refcount == 0:
                obj.released = True
                destroy = getattr(obj.model, "destroy", None)
                if destroy is not None:
                    destroy(obj)
        else:
            raise self.boundary(
                "unsupported_com_method",
                interface=interface,
                slot=slot,
                method=method,
                args=args,
                return_site=return_site,
                method_result_supplied=False,
            )
        # stdcall: callee pops return address plus `nargs` words
        host.set_reg("esp", esp + 4 + 4 * nargs)
        if result is not None:  # void methods leave EAX untouched
            host.set_reg("eax", result)
        host.set_reg("eip", return_site)
        return key, return_site, args, result
