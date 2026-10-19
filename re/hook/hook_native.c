/* hook_native.c: per-native call logging (M0-11).
 *
 * Every TUFA native registered through FUN_00416b00 is logged as an
 * SLRLOG tag-0x00 record: `str class, str name, u32 argc + values,
 * option<value> result`.
 *
 * Mechanism (two detours, no 353-way guessing):
 *  1. FUN_00416b00 (the registrar, __thiscall: ecx=VM ctx, stack =
 *     class, name, sig, impl) is detoured once. Each interception records
 *     impl VA -> (class, name, sig) in a runtime table AND in the static
 *     table below (generated from re/out/native_api.tsv, which the
 *     M0-04 builder cross-checks byte-for-byte against the exe).
 *  2. After all registrations (first callMethod is the signal; the exe
 *     registers everything up front in FUN_004427e0-style drivers), one
 *     detour per impl (cdecl, single pointer arg: the TUFA frame) logs
 *     the call and its result, then jumps to a trampoline holding the
 *     stolen prologue bytes. Stolen-byte sizing uses the minimal x86
 *     length decoder at the bottom of this file; impls whose first 5
 *     bytes contain a relative branch or an undecodable opcode are left
 *     unhooked and reported in the `native: skipped N` text line and in
 *     docs/hook-layer.md's gap list.
 *
 * Values: argument values live inside TUFA heap objects (M1-02's value
 * layout); without that layout no hook can read them safely, so M0-11
 * logs argc=0 and the typed result. Result typing comes from the last
 * sig character: V = absent; F = ST0 float bits; I/Z/B/S/C = EAX int;
 * J = EDX:EAX long; D = ST0 double bits; L/[ = EAX object (sequential
 * id, 0 = null; ids are per-run and deterministic for identical
 * replays). Raw pointers never reach the log.
 */
#include "hook.h"

#include <stdio.h>
#include <string.h>

/* ------------------------------------------------- static table ---- */
/* Generated from re/out/native_api.tsv (353 rows; 11 overloaded pairs).
 * Regen: python3 re/hook/gen_native.py > re/hook/native_table.h
 * (git-ignored outputs stay out; this checked-in file is the source). */
typedef struct {
    const char *cls, *name, *sig;
    uint32_t impl;      /* link-base VA (0x40xxxx); relocated at install */
} native_row;

#include "native_table.h"

/* ------------------------------------------------ runtime registry -- */
#define NAT_CAP 512
typedef struct {
    uint32_t impl;
    const char *cls, *name, *sig;
    void *tramp;        /* real impl via stolen-bytes trampoline */
    void *thunk;        /* our wrapper, installed as detour */
    int hooked;         /* -1 skipped (gap), 0 pending, 1 live */
} nat_entry;

static nat_entry g_nat[NAT_CAP];
static unsigned g_nat_n;
static CRITICAL_SECTION g_nat_cs;
static volatile LONG g_regs;    /* registrar interceptions seen */

/* Sequential ids for returned heap objects (never raw pointers).
 * Ids are assigned in order of first appearance: the pointer value itself
 * differs between runs, so any id derived from it (hash slot, address
 * order) would make two identical replays log different bytes. */
#define OBJ_CAP 65536
static void *g_obj_by_id[OBJ_CAP];   /* id-1 -> pointer, in assignment order */
static int32_t g_next_obj = 1;

static int32_t obj_id(void *p) {
    int32_t id;
    if (!p) return 0;
    EnterCriticalSection(&g_nat_cs);
    for (id = 0; id < g_next_obj - 1; id++)
        if (g_obj_by_id[id] == p) break;
    if (id == g_next_obj - 1) {
        if (g_next_obj - 1 < OBJ_CAP) {
            g_obj_by_id[id] = p;
            g_next_obj++;
            id++;                        /* 1-based log id */
        } else {
            id = 0;
        }
    } else {
        id++;                            /* found: 1-based */
    }
    LeaveCriticalSection(&g_nat_cs);
    return id;
}

static nat_entry *nat_find(uint32_t impl) {
    unsigned i;
    for (i = 0; i < g_nat_n; i++)
        if (g_nat[i].impl == impl) return &g_nat[i];
    return NULL;
}

/* ------------------------------------------------------ logging ---- */
void slr_log_native(const char *cls, const char *name, slr_rec *argvals, unsigned argc,
                    const slr_rec *res, int has_res) {
    slr_rec body;
    if (!g_sink_open || slr_off("natives")) return;
    (void)argc;
    slr_rec_init(&body);
    slr_put_cstr(&body, cls);
    slr_put_cstr(&body, name);
    slr_put_u32(&body, 0);              /* argc: values need M1-02 layout (gap) */
    if (argvals) slr_rec_free(argvals);
    if (has_res && res) {
        slr_put_u8(&body, 1);
        slr_put_raw(&body, res->p, res->n);
    } else {
        slr_put_u8(&body, 0);
    }
    slr_sink_commit(&g_sink, SLR_TAG_NATIVE, &body);
    slr_note_activity();
}

/* Called from the asm wrapper: (entry, frameptr). Logs pre-call identity;
 * the result is logged by the wrapper after the trampoline returns. */
void __attribute__((cdecl, used)) slr_native_pre(nat_entry *e, void *frame) {
    (void)frame;
    if (g_cfg.trace[0] == 'c') slr_text("nat> %s.%s %s", e->cls, e->name, e->sig);
}

/* Result capture helper: sig last char -> how to read (eax, edx, st0). */
void __attribute__((cdecl, used)) slr_native_post(nat_entry *e, uint32_t eax, uint32_t edx,
                                                  uint32_t st0lo, uint32_t st0hi) {
    slr_rec res;
    const char *sig = e->sig;
    char ret = sig[strlen(sig) - 1];
    slr_rec_init(&res);
    switch (ret) {
    case 'V':
        slr_log_native(e->cls, e->name, NULL, 0, NULL, 0);
        break;
    case 'F': {
        slr_val_float_bits(&res, st0lo);
        slr_log_native(e->cls, e->name, NULL, 0, &res, 1);
        break;
    }
    case 'D': {
        uint64_t b = ((uint64_t)st0hi << 32) | st0lo;
        slr_val_double_bits(&res, b);
        slr_log_native(e->cls, e->name, NULL, 0, &res, 1);
        break;
    }
    case 'J': {
        int64_t v = ((int64_t)(int32_t)edx << 32) | eax;
        slr_val_long(&res, v);
        slr_log_native(e->cls, e->name, NULL, 0, &res, 1);
        break;
    }
    case 'L':
    case '[':
        slr_val_int(&res, obj_id((void *)(uintptr_t)eax));
        slr_log_native(e->cls, e->name, NULL, 0, &res, 1);
        break;
    default:    /* I Z B S C and anything else: EAX int */
        slr_val_int(&res, (int32_t)eax);
        slr_log_native(e->cls, e->name, NULL, 0, &res, 1);
        break;
    }
    slr_rec_free(&res);
}

/* ------------------------------------------------- asm wrappers ---- */
/* One wrapper per hooked impl (built by make_thunk): cdecl, 1 arg.
 * Preserves everything except EAX/EDX/ST0 (the result channels), and
 * restores ST0 after sampling it so the dispatcher sees the real F/D
 * result. Stack discipline: our frame, then push frameptr + call real
 * (cdecl: we clean), sample, log, return WITHOUT touching the caller's
 * 4-byte arg (dispatcher cleans, as before). */
void __attribute__((cdecl, used)) slr_native_pre_tramp(nat_entry *e);

static void emit32(uint8_t **p, uint32_t v) { memcpy(*p, &v, 4); *p += 4; }
static void emit_call(uint8_t **p, void *fn) {
    int32_t rel;
    **p = 0xE8;
    (*p)++;
    rel = (int32_t)((uint8_t *)fn - (*p + 4));
    memcpy(*p, &rel, 4);
    *p += 4;
}

/* Layout (x87=false): push ebp; mov ebp,esp; sub esp,12;
 * push [ebp+8](frame); push e; call pre; add esp,8;
 * push [ebp+8]; call [e->tramp]; add esp,4 (= real impl, cdecl 1 arg);
 * mov [ebp-8],eax; mov [ebp-12],edx;
 * push 0; push 0; push edx; push eax; push e; call post; add esp,20;
 * mov eax,[ebp-8]; mov edx,[ebp-12]; leave; ret
 *
 * The x87 stack is NOT touched unless the native returns F/D: an int/void
 * native must leave ST0.. untouched for its caller. */
static void *make_thunk(nat_entry *e, int x87) {
    uint8_t *c = (uint8_t *)slr_alloc_exec(192), *p = c;
    *p++ = 0x55;
    *p++ = 0x89; *p++ = 0xE5;
    *p++ = 0x83; *p++ = 0xEC; *p++ = x87 ? 0x20 : 0x0C;
    if (x87) *p++ = 0xD9, *p++ = 0x7D, *p++ = 0xFC;    /* fnstcw [ebp-4] */
    *p++ = 0xFF; *p++ = 0x75; *p++ = 0x08;
    *p++ = 0x68; emit32(&p, (uint32_t)(uintptr_t)e);
    emit_call(&p, (void *)slr_native_pre);
    *p++ = 0x83; *p++ = 0xC4; *p++ = 0x08;
    if (x87) *p++ = 0xD9, *p++ = 0x6D, *p++ = 0xFC;    /* fldcw [ebp-4] */
    *p++ = 0xFF; *p++ = 0x75; *p++ = 0x08;
    *p++ = 0xFF; *p++ = 0x15; emit32(&p, (uint32_t)(uintptr_t)&e->tramp);
    *p++ = 0x83; *p++ = 0xC4; *p++ = 0x04;
    *p++ = 0x89; *p++ = 0x45; *p++ = 0xF8;
    *p++ = 0x89; *p++ = 0x55; *p++ = 0xF4;
    if (x87) {
        *p++ = 0xD9; *p++ = 0x7D; *p++ = 0xFC;
        *p++ = 0xDD; *p++ = 0x5D; *p++ = 0xEC;   /* fstpl [ebp-20] */
        *p++ = 0xD9; *p++ = 0x6D; *p++ = 0xFC;
    }
    if (x87) {
        *p++ = 0xFF; *p++ = 0x75; *p++ = 0xF0;   /* push hi */
        *p++ = 0xFF; *p++ = 0x75; *p++ = 0xEC;   /* push lo */
    } else {
        *p++ = 0x6A; *p++ = 0x00;                /* push 0 (hi unused) */
        *p++ = 0x6A; *p++ = 0x00;                /* push 0 (lo unused) */
    }
    *p++ = 0x52;                             /* push edx */
    *p++ = 0x50;                             /* push eax */
    *p++ = 0x68; emit32(&p, (uint32_t)(uintptr_t)e);
    emit_call(&p, (void *)slr_native_post);
    *p++ = 0x83; *p++ = 0xC4; *p++ = 0x14;
    if (x87) {
        *p++ = 0xD9; *p++ = 0x6D; *p++ = 0xFC;
        *p++ = 0xDD; *p++ = 0x45; *p++ = 0xEC;   /* fldl [ebp-20] */
        *p++ = 0xD9; *p++ = 0x7D; *p++ = 0xFC;
    }
    *p++ = 0x8B; *p++ = 0x45; *p++ = 0xF8;
    *p++ = 0x8B; *p++ = 0x55; *p++ = 0xF4;
    if (x87) *p++ = 0xD9, *p++ = 0x6D, *p++ = 0xFC;
    *p++ = 0xC9;
    *p++ = 0xC3;
    return c;
}

/* Tail detour for E9 alias stubs (jmp to a shared body): the 5-byte E9 is
 * replaced by a jmp to this thunk, which pre-logs the call and then takes
 * the original jump.
 *
 * The alias target is decoded from the stub's own rel32, and only here:
 * this runs BEFORE slr_detour, so e->tramp is still NULL (dereferencing
 * tramp+1 was the v1 page fault: read 0x1 at dinput8+0x59FB), and even
 * after assignment the trampoline cannot supply the base, because the
 * copied E9's rel32 is relative to the stub, not to the trampoline. The
 * thunk's jmp is therefore computed from the stub address and the thunk's
 * own location, with no dependence on any detour state.
 *
 * Single hop is complete for this exe: of the 353 table entries exactly 6
 * are E9 stubs (0x47c380, 0x47c390, 0x47c440, 0x47c450, 0x487640,
 * 0x487650) and none of their targets is itself an E9 stub or a table
 * entry, so one rel32 read reaches the real body.
 *
 * No result capture: the shared body never returns here. Stack
 * discipline: tail jump — the caller's [ret][frameptr] passes through. */
static void *make_tail_thunk(nat_entry *e) {
    uint8_t *c = (uint8_t *)slr_alloc_exec(64), *p = c;
    uint8_t *stub = (uint8_t *)(uintptr_t)e->impl;
    uintptr_t target;
    int32_t rel, stubrel;
    *p++ = 0x55;
    *p++ = 0x89; *p++ = 0xE5;
    *p++ = 0x83; *p++ = 0xEC; *p++ = 0x08;
    *p++ = 0xD9; *p++ = 0x7D; *p++ = 0xFC;
    *p++ = 0xFF; *p++ = 0x75; *p++ = 0x08;
    *p++ = 0x68; emit32(&p, (uint32_t)(uintptr_t)e);
    emit_call(&p, (void *)slr_native_pre);
    *p++ = 0x83; *p++ = 0xC4; *p++ = 0x08;
    *p++ = 0xD9; *p++ = 0x6D; *p++ = 0xFC;
    *p++ = 0xC9;
    /* jmp to the alias target: the stub's own E9 rel32, rebased onto the
     * thunk. e->tramp is written only after this thunk is built. */
    *p++ = 0xE9;
    memcpy(&stubrel, stub + 1, sizeof stubrel);
    target = (uintptr_t)(stub + 5) + (uintptr_t)(intptr_t)stubrel;
    rel = (int32_t)(target - (uintptr_t)(p + 4));
    emit32(&p, (uint32_t)rel);
    return c;
}

/* --------------------------------------------- minimal x86 LDE ---- */
/* Length-decode one instruction at p (cap end). Returns length or 0 if
 * undecodable; *branch is set for relative control transfers (E8/E9/EB,
 * 70..7F, 0F 80..8F). Only what prologues use; anything else -> 0. */
static unsigned lde_len(const uint8_t *p, const uint8_t *end, int *branch) {
    const uint8_t *q = p;
    uint8_t b, mod, rm;
    *branch = 0;
    if (q >= end) return 0;
    /* prefixes */
    for (;;) {
        b = *q;
        if (b == 0xF0 || b == 0xF2 || b == 0xF3 || b == 0x2E || b == 0x36 || b == 0x3E ||
            b == 0x26 || b == 0x64 || b == 0x65 || b == 0x66 || b == 0x67) {
            q++;
            if (q >= end) return 0;
        } else break;
    }
    b = *q++;
    if (b >= 0x50 && b <= 0x5F) return (unsigned)(q - p);            /* push/pop reg */
    if (b == 0x6A) return (q + 1 <= end) ? (unsigned)(q + 1 - p) : 0; /* push imm8 */
    if (b == 0x68) return (q + 4 <= end) ? (unsigned)(q + 4 - p) : 0; /* push imm32 */
    if (b == 0x90 || b == 0xC3 || b == 0xC9) return (unsigned)(q - p);
    if (b == 0xC2) return (q + 2 <= end) ? (unsigned)(q + 2 - p) : 0;
    if (b == 0xE8 || b == 0xE9) { *branch = 1; return (q + 4 <= end) ? (unsigned)(q + 4 - p) : 0; }
    if (b == 0xEB) { *branch = 1; return (q + 1 <= end) ? (unsigned)(q + 1 - p) : 0; }
    if (b >= 0x70 && b <= 0x7F) { *branch = 1; return (q + 1 <= end) ? (unsigned)(q + 1 - p) : 0; }
    if (b == 0x0F) {
        uint8_t b2;
        if (q >= end) return 0;
        b2 = *q++;
        if (b2 >= 0x80 && b2 <= 0x8F) { *branch = 1; return (q + 4 <= end) ? (unsigned)(q + 4 - p) : 0; }
        if (b2 == 0xB6 || b2 == 0xB7 || b2 == 0xBE || b2 == 0xBF || b2 == 0xAF || b2 == 0xB0 ||
            b2 == 0xB1 || b2 == 0x6F || b2 == 0x7F || b2 == 0x28 || b2 == 0x29) goto modrm;
        return 0;
    }
    if (b == 0xA1 || b == 0xA3) return (q + 4 <= end) ? (unsigned)(q + 4 - p) : 0;
    if (b >= 0xB8 && b <= 0xBF) return (q + 4 <= end) ? (unsigned)(q + 4 - p) : 0; /* mov reg,imm32 */
    if (b == 0xA8 || b == 0xA9) return (q + ((b & 1) ? 4 : 1) <= end) ? (unsigned)(q + ((b & 1) ? 4 : 1) - p) : 0;
    if (b == 0x81 || b == 0x83 || b == 0xC7 || b == 0x8B || b == 0x8D || b == 0x89 || b == 0x8A ||
        b == 0x33 || b == 0x3B || b == 0x85 || b == 0x09 || b == 0x39 || b == 0x01 ||
        b == 0x03 || b == 0x29 || b == 0x2B || b == 0x31 || b == 0x21 || b == 0x11 ||
        b == 0x13 || b == 0x1B || b == 0xF6 || b == 0xF7 || b == 0xFF || b == 0xFE ||
        b == 0xD9 || b == 0xDB || b == 0xDD || b == 0xDF || b == 0xD8 || b == 0xDA ||
        b == 0xDC || b == 0xDE) goto modrm;
    return 0;
modrm:
    if (q >= end) return 0;
    mod = *q++;
    rm = mod & 7;
    if (b == 0xF6) {
        if (((mod >> 3) & 7) > 1) return 0;
        if (((mod >> 3) & 7) == 0 || ((mod >> 3) & 7) == 1) { /* /0 /1: imm8 */ }
    }
    if (mod >> 6 == 3) {
        /* register direct */
        if (b == 0x83) return (q + 1 <= end) ? (unsigned)(q + 1 - p) : 0;
        if (b == 0x81 || b == 0xC7 || b == 0xF7) return (q + 4 <= end) ? (unsigned)(q + 4 - p) : 0;
        if (b == 0xF6) return (q + ((((mod >> 3) & 7) <= 1) ? 1 : 0) <= end) ? (unsigned)(q + ((((mod >> 3) & 7) <= 1) ? 1 : 0) - p) : 0;
        return (unsigned)(q - p);
    }
    /* memory: sib? */
    if (rm == 4) {
        uint8_t sib;
        if (q >= end) return 0;
        sib = *q++;
        if ((sib & 7) == 5 && (mod >> 6) == 0) {
            if (q + 4 > end) return 0;
            q += 4;
        }
    }
    if ((mod >> 6) == 0) {
        if (rm == 5) {
            if (q + 4 > end) return 0;
            q += 4;
        }
    } else if ((mod >> 6) == 1) {
        q += 1;
    } else {
        q += 4;
    }
    if (b == 0x83 || (b == 0xF6 && (((mod >> 3) & 7) <= 1))) {
        if (q + 1 > end) return 0;
        q += 1;
    }
    if (b == 0x81 || b == 0xC7 || b == 0xF7) {
        if (q + 4 > end) return 0;
        q += 4;
    }
    return (unsigned)(q - p);
}

/* Bytes to steal: whole instructions, >= 5, position-independent
 * (no relative branch inside). Returns 0 if the head is unhookable. */
static unsigned stolen_bytes(uint8_t *fn) {
    uint8_t *end = fn + 32;
    unsigned n = 0;
    while (n < 5) {
        int br = 0;
        unsigned l = lde_len(fn + n, end, &br);
        if (!l || br) return 0;
        n += l;
    }
    return n;
}

/* ------------------------------------------------------- install ---- */
/* Registrar detour: __thiscall FUN_00416b00. Naked asm reads ecx + the
 * four stack args, records, then jumps to the trampoline. */
/* non-static: referenced by the asm hook below (a static symbol fails to
 * link under -O1 -g because the top-level asm lands in another section) */
void *g_reg_tramp;

__asm__(".text\n"
        ".globl _slr_reg_hook\n"
        "_slr_reg_hook:\n"
        "  pushal\n"
        "  pushfl\n"
        /* esp shifts by 4 per push, so one fixed offset reads impl, then
         * sig, name, class in reverse push order. */
        "  pushl 52(%esp)\n"    /* impl (4th stack arg) */
        "  pushl 52(%esp)\n"    /* sig */
        "  pushl 52(%esp)\n"    /* name */
        "  pushl 52(%esp)\n"    /* class (1st stack arg) */
        "  pushl %ecx\n"        /* VM ctx */
        "  call _slr_reg_capture\n"
        "  addl $20, %esp\n"
        "  popfl\n"
        "  popal\n"
        "  jmp *_g_reg_tramp\n");
extern void slr_reg_hook(void);

void __attribute__((cdecl, used)) slr_reg_capture(void *ctx, uint32_t cls, uint32_t nam,
                                                 uint32_t sig, uint32_t impl) {
    char c0[256], n0[128], s0[64];
    int ok;
    nat_entry *e;
    (void)ctx;
    InterlockedIncrement(&g_regs);
    ok = !IsBadReadPtr((void *)(uintptr_t)cls, 1) && !IsBadStringPtrA((void *)(uintptr_t)cls, 1) &&
         !IsBadReadPtr((void *)(uintptr_t)nam, 1) && !IsBadStringPtrA((void *)(uintptr_t)nam, 1) &&
         !IsBadReadPtr((void *)(uintptr_t)sig, 1) && !IsBadStringPtrA((void *)(uintptr_t)sig, 1);
    EnterCriticalSection(&g_nat_cs);
    e = nat_find(impl);
    if (!e && g_nat_n < NAT_CAP) {
        e = &g_nat[g_nat_n++];
        e->impl = impl;
        e->tramp = NULL;
        e->thunk = NULL;
        e->hooked = 0;
        if (ok) {
            strncpy(c0, (const char *)(uintptr_t)cls, sizeof c0 - 1);
            strncpy(n0, (const char *)(uintptr_t)nam, sizeof n0 - 1);
            strncpy(s0, (const char *)(uintptr_t)sig, sizeof s0 - 1);
            c0[sizeof c0 - 1] = n0[sizeof n0 - 1] = s0[sizeof s0 - 1] = 0;
            e->cls = _strdup(c0);
            e->name = _strdup(n0);
            e->sig = _strdup(s0);
        } else {
            e->cls = "?";
            e->name = "?";
            e->sig = "()V";
        }
    }
    LeaveCriticalSection(&g_nat_cs);
    /* All 360 registrar call sites fire during startup (re/build_native_table.py
     * asserts the count against the exe bytes). When the last one lands,
     * every impl VA is known: detour them all now, on this thread. */
    if ((LONG)g_regs == 360) {
        slr_text("native: registration complete (%u table rows)", g_nat_n);
        slr_native_hook_all();
    }
}

/* Patch one registered entry. Called with g_nat_cs held: the whole
 * check-then-patch sequence, slr_detour included, is serialized against every
 * other hook_all caller, so two overlapping passes cannot both see
 * hooked == 0 and patch one entry twice — the second detour would copy the
 * first patch's own E9 into a fresh trampoline and re-point the thunk at
 * itself. Nothing here re-enters g_nat_cs: slr_detour only VirtualAllocs,
 * memcpys and VirtualProtects, and the thunk builders only emit bytes into
 * the shared exec pool. `live`/`skip`/`preonly` are the caller's counters;
 * an entry already classified by an earlier pass only bumps one of them. */
static void nat_hook_one(nat_entry *e, unsigned *live, unsigned *skip, unsigned *preonly) {
    uint8_t *fn;
    unsigned st;
    if (e->hooked > 0) {
        (*live)++;
        return;
    }
    if (e->hooked < 0) {
        (*skip)++;
        return;
    }
    fn = (uint8_t *)(uintptr_t)e->impl;
    if (IsBadCodePtr((FARPROC)fn)) {
        e->hooked = -1;
        (*skip)++;
        return;
    }
    if (fn[0] == 0xE9) {
        /* Alias stub (jmp to a shared body): tail-detour, pre-log only.
         * The thunk resolves the target from the stub itself, so no
         * trampoline is recorded (and none may be read). */
        e->thunk = make_tail_thunk(e);
        slr_detour(fn, e->thunk, 5, NULL);
        e->hooked = 2;
        (*preonly)++;
        return;
    }
    st = stolen_bytes(fn);
    if (!st) {
        e->hooked = -1;
        (*skip)++;
        return;
    }
    e->thunk = make_thunk(e, e->sig && (e->sig[strlen(e->sig) - 1] == 'F' ||
                                        e->sig[strlen(e->sig) - 1] == 'D'));
    /* The thunk reads e->tramp at call time, and the entry jmp goes live
     * inside slr_detour: the slot must be filled before that patch, not
     * after the call returns (residual window of the W1 fix). */
    slr_detour(fn, e->thunk, st, &e->tramp);
    e->hooked = 1;
    (*live)++;
}

/* Late hook pass: detour every registered impl (idempotent). Called from
 * the watchdog the first time the registrar goes quiet, and again from
 * slr_install after a grace note. Simpler: call once at install + on the
 * first D3D9 CreateDevice (registration is long done by then). */
void slr_native_hook_all(void) {
    unsigned i, live = 0, skip = 0, preonly = 0;
    for (i = 0; i < g_nat_n; i++) {
        /* The lock is taken per entry and spans the patch inside
         * nat_hook_one: see the note there for why it may not be dropped
         * between the hooked check and the detour. */
        EnterCriticalSection(&g_nat_cs);
        nat_hook_one(&g_nat[i], &live, &skip, &preonly);
        LeaveCriticalSection(&g_nat_cs);
    }
    slr_text("native: hooked %u live + %u pre-only, %u skipped (of %u regs, %ld interceptions)", live,
             preonly, skip, g_nat_n, (long)g_regs);
}

/* Seed the runtime table from the static TSV-derived list, then detour
 * the registrar so late registrations are still captured. */
void slr_native_install(void) {
    unsigned i;
    uint8_t *reg, *base;
    InitializeCriticalSection(&g_nat_cs);
    if (slr_off("natives")) {
        slr_text("natives: disabled");
        return;
    }
    /* The exe usually loads at its 0x400000 link base, but Wine may
     * relocate it: resolve the real base and treat the table's addresses
     * (which match Ghidra VAs at link base) as base-relative RVAs. */
    base = (uint8_t *)GetModuleHandleA(NULL);
    for (i = 0; i < sizeof g_native_rows / sizeof g_native_rows[0] && g_nat_n < NAT_CAP; i++) {
        nat_entry *e = &g_nat[g_nat_n++];
        /* table VA - link base 0x400000 + actual base */
        e->impl = (uint32_t)(uintptr_t)(base + (g_native_rows[i].impl - 0x400000));
        e->cls = g_native_rows[i].cls;
        e->name = g_native_rows[i].name;
        e->sig = g_native_rows[i].sig;
        e->tramp = NULL;
        e->thunk = NULL;
        e->hooked = 0;
    }
    /* FUN_00416b00 registrar: RVA 0x16B00 in .text (link base 0x400000). */
    reg = base + (0x416B00 - 0x400000);
    if (!IsBadCodePtr((FARPROC)reg)) {
        /* The prologue is `53 55 56 57 6A 18` (four pushes then push 0x18):
         * stealing a flat 5 bytes splits the immediate and the trampoline
         * runs the truncated instruction stream (observed as the boot crash
         * into the exec pool). Size the steal with the same length decoder
         * used for the natives: whole instructions, >= 5, no branch. */
        unsigned st = stolen_bytes(reg);
        if (!st) {
            slr_text("native: registrar prologue unhookable; static rows only (%u)", g_nat_n);
        } else {
            /* Same publish-order rule as the native entries: _slr_reg_hook ends
             * in `jmp *g_reg_tramp`, so the slot is filled before the patch. */
            slr_detour(reg, (void *)slr_reg_hook, st, &g_reg_tramp);
            slr_text("native: registrar hooked (%u static rows, %u stolen bytes)", g_nat_n, st);
        }
    } else {
        slr_text("native: registrar VA unmapped?? static rows only (%u)", g_nat_n);
    }
    /* The static table is complete (it is generated from the exe's registrar
     * call sites), so detour every impl now instead of waiting for a
     * registration count that Wine may never reach. Late registrations are
     * hooked from slr_reg_capture. */
    slr_native_hook_all();
}
