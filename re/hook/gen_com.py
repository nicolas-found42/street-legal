#!/usr/bin/env python3
"""gen_com.py: generate com_tables.h from the mingw-w64 d3d9.h / dsound.h headers.

For every COM interface the hook layer wraps, emit the ordered method list
(vtable slot order), each method's argument classes and the stdcall argument
count, so hook_com.c can patch vtable slots in place with a logging thunk.

Argument classes (how a value is recorded in the SLRLOG record):
  K_INT     any <=32-bit integer/enum/bool by value        -> int
  K_FLT     float by value                                  -> float bits
  K_STRUCT  `const T *` / LPCxxx input struct               -> bytes(sizeof T), bool false if NULL
  K_OBJ     COM interface pointer by value                  -> int object id (0 = NULL)
  K_OUTOBJ  `I **` out pointer that yields a new interface  -> bool non-null (id logged on the next use)
  K_PTR     any other pointer / handle                      -> bool non-null
  K_GUID    REFIID/REFGUID                                  -> bytes(16)

Usage: gen_com.py [include-dir] > com_tables.h
"""
import re
import sys
import glob
import os

INC = sys.argv[1] if len(sys.argv) > 1 else glob.glob(
    "/opt/homebrew/Cellar/mingw-w64/*/toolchain-i686/i686-w64-mingw32/include")[0]

D3D9_IFACES = [
    "IDirect3D9", "IDirect3DDevice9", "IDirect3DSwapChain9", "IDirect3DSurface9",
    "IDirect3DVertexBuffer9", "IDirect3DIndexBuffer9", "IDirect3DTexture9",
    "IDirect3DCubeTexture9", "IDirect3DVolumeTexture9", "IDirect3DVertexDeclaration9",
    "IDirect3DVertexShader9", "IDirect3DPixelShader9", "IDirect3DStateBlock9",
    "IDirect3DQuery9", "IDirect3DVolume9",
]
DS_IFACES = [
    "IDirectSound", "IDirectSoundBuffer", "IDirectSound3DBuffer",
    "IDirectSound3DListener", "IDirectSoundNotify",
]
# typedef names (LPxxx) that name a wrapped interface pointer
LP_TO_IFACE = {
    "LPDIRECTSOUND": "IDirectSound",
    "LPDIRECTSOUNDBUFFER": "IDirectSoundBuffer",
    "LPDIRECTSOUND3DBUFFER": "IDirectSound3DBuffer",
    "LPDIRECTSOUND3DLISTENER": "IDirectSound3DListener",
    "LPDIRECTSOUNDNOTIFY": "IDirectSoundNotify",
}
HANDLE_TYPES = {"HWND", "HDC", "HANDLE", "HMONITOR", "HMODULE", "HINSTANCE"}
# by (interface, method, argname) -> C expression for the byte length of a struct array
LEN_OVERRIDE = {
    ("IDirect3DDevice9", "Clear", "pRects"): "args[0] * sizeof(D3DRECT)",
}
# Non-const struct pointers that are real inputs (logged as the struct the game passed).
FORCE_STRUCT = {
    ("IDirect3D9", "CreateDevice", "pPresentationParameters"): "sizeof(D3DPRESENT_PARAMETERS)",
    ("IDirect3DDevice9", "Reset", "pPresentationParameters"): "sizeof(D3DPRESENT_PARAMETERS)",
    ("IDirect3DDevice9", "CreateAdditionalSwapChain", "pPresentationParameters"): "sizeof(D3DPRESENT_PARAMETERS)",
}
# Methods never logged and never wrapped (reference counting noise).
SKIP = {"AddRef", "Release"}


def strip_comments(s):
    s = re.sub(r"/\*.*?\*/", "", s, flags=re.S)
    return re.sub(r"//[^\n]*", "", s)


def split_args(s):
    out, depth, cur = [], 0, ""
    for ch in s:
        if ch in "(<":
            depth += 1
        elif ch in ")>":
            depth -= 1
        if ch == "," and depth == 0:
            out.append(cur.strip())
            cur = ""
        else:
            cur += ch
    if cur.strip():
        out.append(cur.strip())
    return out


def parse_iface(text, name):
    m = re.search(r"DECLARE_INTERFACE(?:_IID)?_\(\s*%s\b[^{]*\{(.*?)\n\};" % re.escape(name), text, re.S)
    if not m:
        raise SystemExit("interface %s not found" % name)
    body = strip_comments(m.group(1))
    methods = []
    # STDMETHOD(Name)(THIS_ args) PURE;   STDMETHOD_(ret,Name)(THIS ...) PURE;
    for mm in re.finditer(
            r"STDMETHOD(?:_\(\s*([^,]+?)\s*,\s*(\w+)\s*\)|\(\s*(\w+)\s*\))\s*\((.*?)\)\s*PURE\s*;",
            body, re.S):
        ret = mm.group(1) or "HRESULT"
        mname = mm.group(2) or mm.group(3)
        argtxt = re.sub(r"\s+", " ", mm.group(4)).strip()
        argtxt = re.sub(r"^THIS_?\s*", "", argtxt)
        args = split_args(argtxt) if argtxt else []
        methods.append((mname, ret.strip(), args))
    return methods


def classify(iface, mname, arg, known_ifaces):
    """Return (kind, size_expr or None, objtype or None, argname)."""
    arg = re.sub(r"\bstruct\s+", "", arg).strip()
    nm = re.search(r"(\w+)\s*$", arg)
    name = nm.group(1) if nm else "a"
    typ = arg[: nm.start()].strip() if nm else arg
    # arrays like `DWORD x[4]` do not occur in these headers
    t = re.sub(r"\s+", "", typ)
    if t in ("REFIID", "REFGUID"):
        return "K_GUID", None, None, name
    if t == "float":
        return "K_FLT", None, None, name
    if t in HANDLE_TYPES:
        return "K_PTR", None, None, name
    if t in LP_TO_IFACE and LP_TO_IFACE[t] in known_ifaces:
        return "K_OBJ", None, None, name
    m = re.match(r"LPLP(\w+)$", t)
    if m and ("LP" + m.group(1)) in LP_TO_IFACE:
        return "K_OUTOBJ", None, LP_TO_IFACE["LP" + m.group(1)], name
    m = re.match(r"(?:const)?(I\w+)\*\*$", t)
    if m:
        if m.group(1) in known_ifaces:
            return "K_OUTOBJ", None, m.group(1), name
        return "K_PTR", None, None, name
    m = re.match(r"(?:const)?(I\w+)\*$", t)
    if m and m.group(1) in known_ifaces:
        return "K_OBJ", None, None, name
    if (iface, mname, name) in FORCE_STRUCT:
        return "K_STRUCT", FORCE_STRUCT[(iface, mname, name)], None, name
    if "*" in t:
        # input structs: `const T *` (not void/char) or LPCxxx typedefs
        if t.startswith("const") and not re.search(r"void\*|char\*|WCHAR\*|CHAR\*", t) and "**" not in t:
            expr = LEN_OVERRIDE.get((iface, mname, name))
            return "K_STRUCT", expr or ("sizeof(*(%s)0)" % typ), None, name
        if re.match(r"LPC[A-Z0-9]+$", t) and t not in ("LPCVOID", "LPCSTR", "LPCWSTR"):
            return "K_STRUCT", "sizeof(*(%s)0)" % typ, None, name
        return "K_PTR", None, None, name
    if re.match(r"LPC[A-Z0-9]+$", t) and t not in ("LPCVOID", "LPCSTR", "LPCWSTR", "LPCGUID"):
        return "K_STRUCT", "sizeof(*(%s)0)" % typ, None, name
    if t.startswith("LP") or t in ("PVOID", "LPVOID"):
        return "K_PTR", None, None, name
    return "K_INT", None, None, name


D3D_KIND_DRAW = ("Draw", "Clear")
D3D_KIND_PRESENT = ("Present", "PresentEx")
D3D_KIND_FMT = ("FVF", "VertexDeclaration")
D3D_KIND_RES = ("Create", "Lock", "Unlock", "StreamSource", "Indices", "SetTexture", "StretchRect",
                "Update", "PreLoad", "RenderTarget", "DepthStencil", "GetBackBuffer", "ColorFill",
                "Release", "SetPrivateData", "GenerateMipSubLevels", "SetAutoGenTextureFilter",
                "LockRect", "UnlockRect", "LockBox", "UnlockBox", "GetSurfaceLevel")


def d3d_kind(mname):
    if mname.startswith(D3D_KIND_DRAW):
        return 1
    if mname in D3D_KIND_PRESENT:
        return 4
    if any(k in mname for k in D3D_KIND_FMT):
        return 3
    if any(mname.startswith(k) or k in mname for k in D3D_KIND_RES):
        return 2
    return 0


def ds_kind(iface, mname):
    if iface in ("IDirectSound3DBuffer", "IDirectSound3DListener"):
        return 1
    return 0


def emit(iface, methods, known, kindfn, out):
    arr = []
    for idx, (mname, ret, args) in enumerate(methods):
        cls = [classify(iface, mname, a, known) for a in args]
        argc = len(args) + 1  # + This
        parts = []
        for (k, sz, ot, an) in cls:
            parts.append("{%s, %s, %s}" % (k, "0" if sz is None else "1", ('"%s"' % ot) if ot else "0"))
        # struct sizes are computed at table-build time in a helper function per method
        if len(cls) > 12:
            raise SystemExit("too many args: %s.%s" % (iface, mname))
        arr.append((idx, mname, argc, cls, ret))
    out.append("/* %s: %d slots */" % (iface, len(arr)))
    # size functions: one per method with struct args
    for (idx, mname, argc, cls, ret) in arr:
        if any(c[0] == "K_STRUCT" for c in cls):
            out.append("static void sz_%s_%s(const uint32_t *args, uint32_t *sz) {" % (iface, mname))
            out.append("    (void)args;")
            for i, c in enumerate(cls):
                if c[0] == "K_STRUCT":
                    out.append("    sz[%d] = (uint32_t)(%s);" % (i, c[1]))
            out.append("}")
    out.append("static const slr_com_method mt_%s[] = {" % iface)
    for (idx, mname, argc, cls, ret) in arr:
        skip = "1" if mname in SKIP else "0"
        szf = ("sz_%s_%s" % (iface, mname)) if any(c[0] == "K_STRUCT" for c in cls) else "0"
        floatret = "1" if ret.strip() in ("float", "FLOAT") else "0"
        kinds = ", ".join(c[0] for c in cls) or "K_INT"
        objs = ", ".join(('"%s"' % c[2]) if c[2] else "0" for c in cls) or "0"
        out.append('    {"%s", %d, %d, %s, %s, %d, {%s}, {%s}},'
                   % (mname, argc, len(cls), skip, szf, kindfn(iface, mname), kinds, objs))
    out.append("};")
    out.append("")


def main():
    d3d = strip_comments_keep(open(os.path.join(INC, "d3d9.h")).read())
    ds = strip_comments_keep(open(os.path.join(INC, "dsound.h")).read())
    out = ["/* GENERATED by re/hook/gen_com.py from the mingw-w64 headers; do not edit. */", ""]
    d3d_known = set(D3D9_IFACES)
    ds_known = set(DS_IFACES)
    tables = []
    for n in D3D9_IFACES:
        emit(n, parse_iface(d3d, n), d3d_known, lambda i, m: d3d_kind(m), out)
        tables.append((n, "D3D9"))
    for n in DS_IFACES:
        emit(n, parse_iface(ds, n), ds_known, ds_kind, out)
        tables.append((n, "DS"))
    out.append("static const slr_com_iface g_com_ifaces[] = {")
    for n, fam in tables:
        out.append('    {"%s", mt_%s, (unsigned)(sizeof mt_%s / sizeof mt_%s[0]), FAM_%s, &IID_%s},'
                   % (n, n, n, n, fam, n))
    out.append("};")
    print("\n".join(out))


def strip_comments_keep(s):
    return s


if __name__ == "__main__":
    main()
