/* hook_com.c: D3D9 and DirectSound call logging by in-place vtable patching.
 *
 * After Direct3DCreate9 / DirectSoundCreate (and every later "create" call
 * that yields an interface pointer) the real object's vtable slots are
 * overwritten with generated thunks that log the call (method name + typed
 * argument values), run the real method with a faithful stdcall frame, and
 * then wrap any object the call produced. The vtable is patched in place, not
 * copied, because Wine's d3d9 asserts on vtable identity.
 *
 * Method tables (slot order, argument classes) come from the generated
 * com_tables.h (re/hook/gen_com.py, parsed from the mingw-w64 headers).
 *
 * Objects are identified in the log by a per-run sequential id (never by
 * address), so two runs of the same replay log identical bytes.
 */
#include "hook.h"
#include <d3d9.h>
#include <dsound.h>
#include <stdio.h>
#include <string.h>

enum { K_INT, K_FLT, K_STRUCT, K_OBJ, K_OUTOBJ, K_PTR, K_GUID };
enum { FAM_D3D9, FAM_DS };
#define MAXA 12

typedef struct {
    const char *name;
    uint8_t argc;   /* stdcall argument count including This */
    uint8_t nargs;  /* without This */
    uint8_t skip;
    void (*szf)(const uint32_t *args, uint32_t *sz);
    uint8_t rec_kind;
    uint8_t kinds[MAXA];
    const char *objs[MAXA];
} slr_com_method;

typedef struct {
    const char *name;
    const slr_com_method *m;
    unsigned n;
    int fam;
    const GUID *iid;
} slr_com_iface;

#include "com_tables.h"
#define N_IFACES (sizeof g_com_ifaces / sizeof g_com_ifaces[0])

typedef struct {
    unsigned iface, slot;
    void *orig;
} slr_com_desc;

/* ------------------------------------------------------- object table --- */
typedef struct {
    void *ptr;
    int32_t id;
    int iface;
} obj_entry;
#define OBJ_CAP 65536
static obj_entry g_objs[OBJ_CAP];
static int32_t g_next_id = 1;
static CRITICAL_SECTION g_com_cs;
static void **g_patched[256];
static unsigned g_patched_n;

static unsigned obj_hash(void *p) { return (unsigned)(((uintptr_t)p >> 3) * 2654435761u) & (OBJ_CAP - 1); }

static int32_t obj_lookup(void *p) {
    unsigned h, i;
    int32_t id = 0;
    if (!p) return 0;
    EnterCriticalSection(&g_com_cs);
    h = obj_hash(p);
    for (i = 0; i < OBJ_CAP; i++) {
        obj_entry *e = &g_objs[(h + i) & (OBJ_CAP - 1)];
        if (!e->ptr) break;
        if (e->ptr == p) {
            id = e->id;
            break;
        }
    }
    LeaveCriticalSection(&g_com_cs);
    return id ? id : -1;
}

static int32_t obj_register(void *p, int iface) {
    unsigned h, i;
    int32_t id = -1;
    EnterCriticalSection(&g_com_cs);
    h = obj_hash(p);
    for (i = 0; i < OBJ_CAP; i++) {
        obj_entry *e = &g_objs[(h + i) & (OBJ_CAP - 1)];
        if (!e->ptr || e->ptr == p) {
            if (!e->ptr || e->iface != iface) e->id = g_next_id++;
            e->ptr = p;
            e->iface = iface;
            id = e->id;
            break;
        }
    }
    LeaveCriticalSection(&g_com_cs);
    return id;
}

static int iface_by_name(const char *n) {
    unsigned i;
    for (i = 0; i < N_IFACES; i++)
        if (strcmp(g_com_ifaces[i].name, n) == 0) return (int)i;
    return -1;
}

/* ------------------------------------------------------------ thunks --- */
static void emit32(uint8_t **p, uint32_t v) {
    memcpy(*p, &v, 4);
    *p += 4;
}
static void emit_call(uint8_t **p, void *fn) {
    int32_t rel;
    **p = 0xE8;
    (*p)++;
    rel = (int32_t)((uint8_t *)fn - (*p + 4));
    memcpy(*p, &rel, 4);
    *p += 4;
}

void __attribute__((cdecl, used)) slr_com_pre(slr_com_desc *d, uint32_t *args);
void __attribute__((cdecl, used)) slr_com_post(slr_com_desc *d, uint32_t *args, uint32_t result);

static void *make_thunk(slr_com_desc *d, unsigned argc) {
    uint8_t *code = (uint8_t *)slr_alloc_exec(64 + argc * 8), *p = code;
    unsigned i;
    *p++ = 0x55;                                  /* push ebp */
    *p++ = 0x89; *p++ = 0xE5;                     /* mov ebp, esp */
    *p++ = 0x83; *p++ = 0xEC; *p++ = 0x08;        /* sub esp, 8 */
    *p++ = 0xD9; *p++ = 0x7D; *p++ = 0xFC;        /* fnstcw [ebp-4] */
    *p++ = 0x8D; *p++ = 0x45; *p++ = 0x08;        /* lea eax, [ebp+8] */
    *p++ = 0x50;                                  /* push eax */
    *p++ = 0x68; emit32(&p, (uint32_t)(uintptr_t)d); /* push desc */
    emit_call(&p, (void *)slr_com_pre);
    *p++ = 0x83; *p++ = 0xC4; *p++ = 0x08;        /* add esp, 8 */
    *p++ = 0xD9; *p++ = 0x6D; *p++ = 0xFC;        /* fldcw [ebp-4] */
    for (i = argc; i-- > 0;) {                    /* push dword [ebp+8+4i] */
        *p++ = 0xFF; *p++ = 0xB5; emit32(&p, 8 + 4 * i);
    }
    *p++ = 0xFF; *p++ = 0x15; emit32(&p, (uint32_t)(uintptr_t)&d->orig); /* call [orig] */
    *p++ = 0x89; *p++ = 0x45; *p++ = 0xF8;        /* mov [ebp-8], eax */
    *p++ = 0xD9; *p++ = 0x7D; *p++ = 0xFC;        /* fnstcw [ebp-4] */
    *p++ = 0x50;                                  /* push eax (result) */
    *p++ = 0x8D; *p++ = 0x45; *p++ = 0x08;        /* lea eax, [ebp+8] */
    *p++ = 0x50;                                  /* push eax */
    *p++ = 0x68; emit32(&p, (uint32_t)(uintptr_t)d); /* push desc */
    emit_call(&p, (void *)slr_com_post);
    *p++ = 0x83; *p++ = 0xC4; *p++ = 0x0C;        /* add esp, 12 */
    *p++ = 0xD9; *p++ = 0x6D; *p++ = 0xFC;        /* fldcw [ebp-4] */
    *p++ = 0x8B; *p++ = 0x45; *p++ = 0xF8;        /* mov eax, [ebp-8] */
    *p++ = 0xC9;                                  /* leave */
    *p++ = 0xC2; *p++ = (uint8_t)(argc * 4); *p++ = (uint8_t)((argc * 4) >> 8); /* ret argc*4 */
    return code;
}

static void patch_vtable(void **vtbl, int iface) {
    const slr_com_iface *I = &g_com_ifaces[iface];
    unsigned s, i;
    DWORD old;
    EnterCriticalSection(&g_com_cs);
    for (i = 0; i < g_patched_n; i++)
        if (g_patched[i] == vtbl) {
            LeaveCriticalSection(&g_com_cs);
            return;
        }
    if (g_patched_n < 256) g_patched[g_patched_n++] = vtbl;
    VirtualProtect(vtbl, I->n * sizeof(void *), PAGE_READWRITE, &old);
    for (s = 0; s < I->n; s++) {
        slr_com_desc *d;
        if (I->m[s].skip) continue;
        d = (slr_com_desc *)slr_alloc_exec(sizeof *d);
        d->iface = (unsigned)iface;
        d->slot = s;
        d->orig = vtbl[s];
        vtbl[s] = make_thunk(d, I->m[s].argc);
    }
    VirtualProtect(vtbl, I->n * sizeof(void *), old, &old);
    LeaveCriticalSection(&g_com_cs);
    slr_text("com: patched %s vtable %p (%u slots)", I->name, (void *)vtbl, I->n);
}

static int32_t wrap_object(void *obj, int iface) {
    if (!obj || iface < 0) return 0;
    if (slr_off("vtable")) return 0;
    if (slr_off("devwrap") && strcmp(g_com_ifaces[iface].name, "IDirect3DDevice9") == 0) return 0;
    patch_vtable(*(void ***)obj, iface);
    return obj_register(obj, iface);
}

/* ------------------------------------------------------------ logging --- */
static int readable(const void *p, size_t n) { return p && !IsBadReadPtr(p, n); }

static void put_args(slr_rec *r, const slr_com_method *M, const uint32_t *args) {
    uint32_t sz[MAXA] = {0};
    unsigned i;
    if (M->szf) M->szf(args + 1, sz);
    /* szf indexes sz[] by position among logged args (without This): compute with args+1 */
    slr_put_u32(r, M->nargs);
    for (i = 0; i < M->nargs; i++) {
        uint32_t a = args[1 + i];
        switch (M->kinds[i]) {
        case K_INT: slr_val_int(r, (int32_t)a); break;
        case K_FLT: slr_val_float_bits(r, a); break;
        case K_STRUCT:
            if (!a)
                slr_val_bool(r, 0);
            else if (readable((void *)(uintptr_t)a, sz[i]))
                slr_val_bytes(r, (void *)(uintptr_t)a, sz[i]);
            else
                slr_val_bool(r, 1);
            break;
        case K_OBJ: slr_val_int(r, obj_lookup((void *)(uintptr_t)a)); break;
        case K_GUID:
            if (readable((void *)(uintptr_t)a, 16)) slr_val_bytes(r, (void *)(uintptr_t)a, 16);
            else slr_val_bool(r, 0);
            break;
        default: slr_val_bool(r, a != 0); break; /* K_PTR, K_OUTOBJ */
        }
    }
}

static const char *qualified(const slr_com_iface *I, const slr_com_method *M, char *buf, size_t n) {
    if (strcmp(I->name, "IDirect3DDevice9") == 0 || strcmp(I->name, "IDirect3D9") == 0 ||
        strcmp(I->name, "IDirectSound") == 0 || strcmp(I->name, "IDirectSoundBuffer") == 0)
        return M->name;
    snprintf(buf, n, "%s.%s", I->name, M->name);
    return buf;
}

static void log_call(const slr_com_iface *I, const slr_com_method *M, const uint32_t *args) {
    slr_rec r;
    char q[96];
    if (!g_sink_open) return;
    slr_rec_init(&r);
    slr_put_u8(&r, M->rec_kind);
    slr_put_cstr(&r, qualified(I, M, q, sizeof q));
    put_args(&r, M, args);
    slr_sink_commit(&g_sink, I->fam == FAM_D3D9 ? SLR_TAG_D3D9 : SLR_TAG_DSOUND, &r);
    slr_note_activity();
}

void __attribute__((cdecl, used)) slr_com_pre(slr_com_desc *d, uint32_t *args) {
    const slr_com_iface *I = &g_com_ifaces[d->iface];
    const slr_com_method *M = &I->m[d->slot];
    if (g_cfg.trace[0] == 'c') slr_text("com> %s.%s", I->name, M->name);
    log_call(I, M, args);
    /* Replay injection: the display-mode change fails under this Wine, so a
     * requested full-screen device is created windowed. The log above holds
     * what the game asked for. */
    if (g_cfg.force_windowed && I->fam == FAM_D3D9) {
        D3DPRESENT_PARAMETERS *pp = NULL;
        if (strcmp(I->name, "IDirect3D9") == 0 && strcmp(M->name, "CreateDevice") == 0)
            pp = (D3DPRESENT_PARAMETERS *)(uintptr_t)args[5];
        else if (strcmp(I->name, "IDirect3DDevice9") == 0 && strcmp(M->name, "Reset") == 0)
            pp = (D3DPRESENT_PARAMETERS *)(uintptr_t)args[1];
        if (pp && readable(pp, sizeof *pp) && !pp->Windowed) {
            slr_text("com: forcing windowed (requested %ux%u fmt %u)", pp->BackBufferWidth,
                     pp->BackBufferHeight, (unsigned)pp->BackBufferFormat);
            pp->Windowed = TRUE;
            pp->FullScreen_RefreshRateInHz = 0;
        }
    }
}

void __attribute__((cdecl, used)) slr_com_post(slr_com_desc *d, uint32_t *args, uint32_t result) {
    const slr_com_iface *I = &g_com_ifaces[d->iface];
    const slr_com_method *M = &I->m[d->slot];
    unsigned i;
    if ((int32_t)result < 0 && M->name[0] != 'G') /* failures other than Get* probes */
        slr_text("com: %s.%s failed hr=%08x", I->name, M->name, result);
    if (M->rec_kind == 0xFF) return;
    if (result & 0x80000000u) return; /* FAILED(hr): nothing created */
    for (i = 0; i < M->nargs; i++) {
        if (M->kinds[i] == K_OUTOBJ && M->objs[i]) {
            void **pp = (void **)(uintptr_t)args[1 + i];
            if (readable(pp, sizeof(void *)) && *pp) wrap_object(*pp, iface_by_name(M->objs[i]));
        }
    }
    if (strcmp(M->name, "QueryInterface") == 0 && I->fam == FAM_DS) {
        const GUID *iid = (const GUID *)(uintptr_t)args[1];
        void **pp = (void **)(uintptr_t)args[2];
        unsigned k;
        if (readable(iid, 16) && readable(pp, 4) && *pp)
            for (k = 0; k < N_IFACES; k++)
                if (g_com_ifaces[k].fam == FAM_DS && IsEqualGUID(g_com_ifaces[k].iid, iid)) {
                    wrap_object(*pp, (int)k);
                    break;
                }
    }
}

/* -------------------------------------------------- creation entry points */
static IDirect3D9 *(WINAPI *real_Direct3DCreate9)(UINT);
static HRESULT(WINAPI *real_DirectSoundCreate)(LPCGUID, LPDIRECTSOUND *, LPUNKNOWN);

static IDirect3D9 *WINAPI my_Direct3DCreate9(UINT ver) {
    IDirect3D9 *o = real_Direct3DCreate9(ver);
    slr_rec r;
    if (g_sink_open) {
        slr_rec_init(&r);
        slr_put_u8(&r, 0);
        slr_put_cstr(&r, "Direct3DCreate9");
        slr_put_u32(&r, 1);
        slr_val_int(&r, (int32_t)ver);
        slr_sink_commit(&g_sink, SLR_TAG_D3D9, &r);
        slr_note_activity();
    }
    slr_text("com: Direct3DCreate9(%u) -> %p", ver, (void *)o);
    if (o) wrap_object(o, iface_by_name("IDirect3D9"));
    return o;
}

static HRESULT WINAPI my_DirectSoundCreate(LPCGUID guid, LPDIRECTSOUND *pp, LPUNKNOWN outer) {
    HRESULT hr = real_DirectSoundCreate(guid, pp, outer);
    slr_rec r;
    if (g_sink_open) {
        slr_rec_init(&r);
        slr_put_u8(&r, 0);
        slr_put_cstr(&r, "DirectSoundCreate");
        slr_put_u32(&r, 1);
        slr_val_bool(&r, guid != NULL);
        slr_sink_commit(&g_sink, SLR_TAG_DSOUND, &r);
        slr_note_activity();
    }
    slr_text("com: DirectSoundCreate -> hr=%08x", (unsigned)hr);
    if (SUCCEEDED(hr) && pp && *pp) wrap_object(*pp, iface_by_name("IDirectSound"));
    return hr;
}

void slr_com_install(void) {
    InitializeCriticalSection(&g_com_cs);
    real_Direct3DCreate9 = (void *)slr_iat_hook("d3d9.dll", "Direct3DCreate9", (void *)my_Direct3DCreate9);
    real_DirectSoundCreate = (void *)slr_iat_hook_ord("DSOUND.dll", 1, (void *)my_DirectSoundCreate);
    slr_text("com: hooks d3d9=%p dsound=%p", (void *)real_Direct3DCreate9, (void *)real_DirectSoundCreate);
}
