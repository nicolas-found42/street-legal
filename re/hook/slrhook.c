/* slrhook.c: proxy DLL core (M0-11): configuration, entry-point trampoline,
 * IAT patching, inline detours, import tracing, watchdog.
 *
 * Mechanism (jev_decide: proxy-dll): this DLL is installed as dinput8.dll in the
 * copy of the stock directory (WINEDLLOVERRIDES=dinput8=n,b). Wine loads it as
 * one of the exe's imports, before the exe's entry point runs. DllMain patches
 * the exe entry point with a stub; when the exe starts, the stub installs every
 * hook (the exe's imports are fully bound by then) and jumps to the real entry.
 */
#include "hook.h"
#include <stdarg.h>
#include <stdio.h>
#include <string.h>

slr_config g_cfg;
slr_sink g_sink;
int g_sink_open;
HMODULE g_self;

static HANDLE g_text = INVALID_HANDLE_VALUE;
static CRITICAL_SECTION g_text_cs;
static volatile LONG g_activity;
static volatile LONG g_finished;

/* ------------------------------------------------------------ config ---- */
static uint32_t env_u32(const char *name, uint32_t dflt) {
    char b[64];
    DWORD n = GetEnvironmentVariableA(name, b, sizeof b);
    if (n == 0 || n >= sizeof b) return dflt;
    return (uint32_t)strtoul(b, NULL, 0);
}
static void env_str(const char *name, char *dst, size_t cap, const char *dflt) {
    DWORD n = GetEnvironmentVariableA(name, dst, (DWORD)cap);
    if (n == 0 || n >= cap) {
        strncpy(dst, dflt, cap - 1);
        dst[cap - 1] = 0;
    }
}

static void config_init(void) {
    char exe[MAX_PATH], *slash;
    GetModuleFileNameA(NULL, exe, sizeof exe);
    slash = strrchr(exe, '\\');
    if (slash) *slash = 0;
    strncpy(g_cfg.game_dir, exe, sizeof g_cfg.game_dir - 1);
    env_str("SLR_HOOK_LOG", g_cfg.log_path, sizeof g_cfg.log_path, "hook.slrlog");
    env_str("SLR_HOOK_TEXT", g_cfg.text_path, sizeof g_cfg.text_path, "");
    env_str("SLR_HOOK_TRACE", g_cfg.trace, sizeof g_cfg.trace, "");
    env_str("SLR_HOOK_INPUT", g_cfg.input_path, sizeof g_cfg.input_path, "");
    env_str("SLR_HOOK_DISABLE", g_cfg.disable, sizeof g_cfg.disable, "");
    g_cfg.seed = env_u32("SLR_HOOK_SEED", 0x2005020b);
    g_cfg.dt_us = env_u32("SLR_HOOK_DT_US", 16667);
    g_cfg.step_us = env_u32("SLR_HOOK_STEP_US", 50);
    g_cfg.max_virtual_ms = env_u32("SLR_HOOK_MAX_MS", 0);
    g_cfg.idle_exit_ms = env_u32("SLR_HOOK_IDLE_MS", 0);
    g_cfg.max_records = env_u32("SLR_HOOK_MAX_RECORDS", 0);
    g_cfg.file_payload = env_u32("SLR_HOOK_FILE_PAYLOAD", 0);
    g_cfg.force_windowed = env_u32("SLR_HOOK_WINDOWED", 1);
}

int slr_off(const char *what) { return g_cfg.disable[0] && strstr(g_cfg.disable, what) != NULL; }

/* ------------------------------------------------------------- text ---- */
void slr_text(const char *fmt, ...) {
    char line[1024];
    va_list ap;
    int n;
    DWORD w;
    if (g_text == INVALID_HANDLE_VALUE) return;
    va_start(ap, fmt);
    n = vsnprintf(line, sizeof line - 2, fmt, ap);
    va_end(ap);
    if (n < 0) return;
    if (n > (int)sizeof line - 3) n = (int)sizeof line - 3;
    line[n++] = '\n';
    EnterCriticalSection(&g_text_cs);
    WriteFile(g_text, line, (DWORD)n, &w, NULL);
    LeaveCriticalSection(&g_text_cs);
}

void slr_note_activity(void) { InterlockedIncrement(&g_activity); }

void slr_finish(const char *why) {
    if (InterlockedExchange(&g_finished, 1)) return;
    slr_text("finish: %s records=%u", why, (unsigned)g_sink.count);
    if (g_sink_open) slr_sink_close(&g_sink);
    if (g_text != INVALID_HANDLE_VALUE) FlushFileBuffers(g_text);
    TerminateProcess(GetCurrentProcess(), 0);
}

static DWORD WINAPI watchdog(LPVOID arg) {
    LONG last = -1;
    DWORD idle = 0;
    (void)arg;
    for (;;) {
        LONG a;
        Sleep(100);
        a = g_activity;
        if (g_cfg.max_records && g_sink.count >= g_cfg.max_records) slr_finish("max records");
        if (a == last) {
            idle += 100;
            if (g_cfg.idle_exit_ms && idle >= g_cfg.idle_exit_ms) slr_finish("idle");
        } else {
            idle = 0;
            last = a;
        }
    }
    return 0;
}

/* ---------------------------------------------------- code allocation ---- */
void *slr_alloc_exec(size_t n) {
    static uint8_t *pool;
    static size_t used, cap;
    void *p;
    if (!pool || used + n > cap) {
        cap = 4 * 1024 * 1024;
        pool = (uint8_t *)VirtualAlloc(NULL, cap, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE);
        used = 0;
        if (!pool) return NULL;
    }
    p = pool + used;
    used += (n + 15) & ~(size_t)15;
    return p;
}

/* ------------------------------------------------------- IAT patching ---- */
static IMAGE_IMPORT_DESCRIPTOR *import_dir(uint8_t *base) {
    IMAGE_DOS_HEADER *dh = (IMAGE_DOS_HEADER *)base;
    IMAGE_NT_HEADERS *nt = (IMAGE_NT_HEADERS *)(base + dh->e_lfanew);
    DWORD rva = nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT].VirtualAddress;
    return rva ? (IMAGE_IMPORT_DESCRIPTOR *)(base + rva) : NULL;
}

static void *patch_slot(void **slot, void *replacement) {
    DWORD old;
    void *prev = *slot;
    VirtualProtect(slot, sizeof(void *), PAGE_READWRITE, &old);
    *slot = replacement;
    VirtualProtect(slot, sizeof(void *), old, &old);
    return prev;
}

static void *iat_hook_impl(const char *dll, const char *name, unsigned ordinal, void *repl) {
    uint8_t *base = (uint8_t *)GetModuleHandleA(NULL);
    IMAGE_IMPORT_DESCRIPTOR *d = import_dir(base);
    for (; d && d->Name; d++) {
        IMAGE_THUNK_DATA *oft, *ft;
        if (lstrcmpiA((const char *)(base + d->Name), dll) != 0) continue;
        oft = (IMAGE_THUNK_DATA *)(base + (d->OriginalFirstThunk ? d->OriginalFirstThunk : d->FirstThunk));
        ft = (IMAGE_THUNK_DATA *)(base + d->FirstThunk);
        for (; oft->u1.AddressOfData; oft++, ft++) {
            if (IMAGE_SNAP_BY_ORDINAL(oft->u1.Ordinal)) {
                if (!name && IMAGE_ORDINAL(oft->u1.Ordinal) == ordinal)
                    return patch_slot((void **)&ft->u1.Function, repl);
            } else if (name) {
                IMAGE_IMPORT_BY_NAME *ibn = (IMAGE_IMPORT_BY_NAME *)(base + oft->u1.AddressOfData);
                if (strcmp((const char *)ibn->Name, name) == 0)
                    return patch_slot((void **)&ft->u1.Function, repl);
            }
        }
    }
    return NULL;
}
void *slr_iat_hook(const char *dll, const char *name, void *repl) {
    return iat_hook_impl(dll, name, 0, repl);
}
void *slr_iat_hook_ord(const char *dll, unsigned ordinal, void *repl) {
    return iat_hook_impl(dll, NULL, ordinal, repl);
}

/* Real import from the loaded module's export table. This is the only safe
 * way for a hook to reach the original function: asking the exe IAT again
 * after installing returns the hook itself (infinite recursion). */
void *slr_real_import(const char *dll, const char *name) {
    HMODULE m = GetModuleHandleA(dll);
    if (!m) m = LoadLibraryA(dll);
    return m ? (void *)GetProcAddress(m, name) : NULL;
}

/* ------------------------------------------------------ inline detour ---- */
/* The trampoline is published into *publish *before* the entry's first byte is
 * patched. Writing the jmp at the target makes the hook reachable to every
 * other thread instantly, and a thunk that hops through the slot
 * (`call [e->tramp]`, `jmp *g_reg_tramp`) reads it at call time: with the slot
 * assigned only after this function returned, a thread already running inside
 * the game could take the entry in between and fault on a NULL trampoline
 * (the same class of fault as the v1 0x1 read). `publish` may be NULL for a
 * hook that decodes its target from the target itself and never reads a slot
 * (the E9-alias tail thunk). */
void slr_detour(void *target, void *detour, unsigned stolen, void **publish) {
    uint8_t *t = (uint8_t *)target;
    uint8_t *tramp = (uint8_t *)slr_alloc_exec(stolen + 5);
    DWORD old;
    int32_t rel;
    memcpy(tramp, t, stolen);
    tramp[stolen] = 0xE9;
    rel = (int32_t)((t + stolen) - (tramp + stolen + 5));
    memcpy(tramp + stolen + 1, &rel, 4);
    if (publish) {
        *publish = tramp;
        /* Compiler + hardware fence (`lock orl $0,(%esp)` on i686): the slot
         * store must be visible before the entry jmp can be executed. */
        __atomic_thread_fence(__ATOMIC_SEQ_CST);
    }
    VirtualProtect(t, stolen, PAGE_EXECUTE_READWRITE, &old);
    t[0] = 0xE9;
    rel = (int32_t)((uint8_t *)detour - (t + 5));
    memcpy(t + 1, &rel, 4);
    { unsigned i; for (i = 5; i < stolen; i++) t[i] = 0x90; }
    VirtualProtect(t, stolen, old, &old);
    FlushInstructionCache(GetCurrentProcess(), t, stolen);
}

/* ------------------------------------------------------- import trace ---- */
typedef struct {
    const char *dll;
    char name[48];
    void *real;
} trace_entry;
static trace_entry *g_trace;
static unsigned g_trace_n;
static volatile LONG g_trace_seq;

void __attribute__((cdecl, used)) slr_trace_cb(unsigned idx) {
    /* No activity signal: import tracing is diagnostic only. Signalling
     * here would keep an otherwise-idle process alive past its natural
     * stall point whenever tracing is on. */
    LONG s = InterlockedIncrement(&g_trace_seq);
    slr_text("[%ld] t%lu %s!%s", s, GetCurrentThreadId(), g_trace[idx].dll, g_trace[idx].name);
}

static int trace_wanted(const char *dll, const char *name) {
    if (!g_cfg.trace[0]) return 0;
    if (strcmp(g_cfg.trace, "all") == 0) {
        /* hot CRT/heap calls drown the text log; always skipped */
        static const char *hot[] = {"HeapAlloc", "HeapFree", "HeapReAlloc", "HeapSize", "TlsGetValue",
                                    "TlsSetValue", "GetLastError", "SetLastError", "EnterCriticalSection",
                                    "LeaveCriticalSection", "InterlockedIncrement", "InterlockedDecrement",
                                    "InterlockedExchange", "GetCurrentThreadId", "GetProcessHeap",
                                    "VirtualAlloc", "VirtualFree", "GetModuleHandleA", "SetFilePointer",
                                    "ReadFile", "WriteFile", "GetFileType", "lstrlenA", "GetFileSize", NULL};
        int i;
        for (i = 0; hot[i]; i++)
            if (strcmp(hot[i], name) == 0) return 0;
        return 1;
    }
    if (g_cfg.trace[0] == 'c') return 0;
    return strstr(g_cfg.trace, dll) != NULL;
}

static void install_import_trace(void) {
    uint8_t *base = (uint8_t *)GetModuleHandleA(NULL);
    IMAGE_IMPORT_DESCRIPTOR *d = import_dir(base);
    unsigned cap = 1024;
    if (!g_cfg.trace[0]) return;
    g_trace = (trace_entry *)calloc(cap, sizeof(trace_entry));
    for (; d && d->Name; d++) {
        IMAGE_THUNK_DATA *oft, *ft;
        const char *dll = (const char *)(base + d->Name);
        oft = (IMAGE_THUNK_DATA *)(base + (d->OriginalFirstThunk ? d->OriginalFirstThunk : d->FirstThunk));
        ft = (IMAGE_THUNK_DATA *)(base + d->FirstThunk);
        for (; oft->u1.AddressOfData; oft++, ft++) {
            char nm[48];
            uint8_t *stub;
            trace_entry *e;
            unsigned idx;
            int32_t rel;
            if (IMAGE_SNAP_BY_ORDINAL(oft->u1.Ordinal))
                snprintf(nm, sizeof nm, "#%u", (unsigned)IMAGE_ORDINAL(oft->u1.Ordinal));
            else
                snprintf(nm, sizeof nm, "%s", ((IMAGE_IMPORT_BY_NAME *)(base + oft->u1.AddressOfData))->Name);
            if (!trace_wanted(dll, nm) || g_trace_n >= cap) continue;
            idx = g_trace_n++;
            e = &g_trace[idx];
            e->dll = dll;
            snprintf(e->name, sizeof e->name, "%s", nm);
            e->real = (void *)ft->u1.Function;
            /* pushfd; pushad; push idx; call cb; add esp,4; popad; popfd; jmp [&e->real] */
            stub = (uint8_t *)slr_alloc_exec(40);
            {
                uint8_t *p = stub;
                *p++ = 0x9C;
                *p++ = 0x60;
                *p++ = 0x68;
                memcpy(p, &idx, 4);
                p += 4;
                *p++ = 0xE8;
                rel = (int32_t)((uint8_t *)slr_trace_cb - (p + 4));
                memcpy(p, &rel, 4);
                p += 4;
                *p++ = 0x83; *p++ = 0xC4; *p++ = 0x04;
                *p++ = 0x61;
                *p++ = 0x9D;
                *p++ = 0xFF; *p++ = 0x25;
                { void *addr = &e->real; memcpy(p, &addr, 4); }
            }
            patch_slot((void **)&ft->u1.Function, stub);
        }
    }
    slr_text("import trace: %u functions (%s)", g_trace_n, g_cfg.trace);
}

/* ---------------------------------------------------- entry trampoline -- */
static uint8_t *g_ep;
static uint8_t g_ep_saved[5];
void *slr_ep_target;

__asm__(".text\n"
        ".globl _slr_entry_stub\n"
        "_slr_entry_stub:\n"
        "  pushal\n"
        "  pushfl\n"
        "  call _slr_install\n"
        "  popfl\n"
        "  popal\n"
        "  jmp *_slr_ep_target\n");
extern void slr_entry_stub(void);

void __attribute__((cdecl, used)) slr_install(void) {
    DWORD old;
    /* undo the entry patch first so the exe starts clean */
    VirtualProtect(g_ep, 5, PAGE_EXECUTE_READWRITE, &old);
    memcpy(g_ep, g_ep_saved, 5);
    VirtualProtect(g_ep, 5, old, &old);
    FlushInstructionCache(GetCurrentProcess(), g_ep, 5);

    slr_text("install: exe entry %p, thread %lu", (void *)g_ep, GetCurrentThreadId());
    install_import_trace();
    slr_time_install();
    slr_file_install();
    slr_native_install();
    slr_com_install();
    slr_input_install();
    slr_misc_install();
    if (g_cfg.input_path[0]) slr_input_script_load(g_cfg.input_path);
    if (g_cfg.idle_exit_ms || g_cfg.max_records)
        CreateThread(NULL, 0, watchdog, NULL, 0, NULL);
    slr_text("install: done");
}

static void patch_entry(void) {
    uint8_t *base = (uint8_t *)GetModuleHandleA(NULL);
    IMAGE_DOS_HEADER *dh = (IMAGE_DOS_HEADER *)base;
    IMAGE_NT_HEADERS *nt = (IMAGE_NT_HEADERS *)(base + dh->e_lfanew);
    DWORD old;
    int32_t rel;
    g_ep = base + nt->OptionalHeader.AddressOfEntryPoint;
    slr_ep_target = g_ep;
    memcpy(g_ep_saved, g_ep, 5);
    VirtualProtect(g_ep, 5, PAGE_EXECUTE_READWRITE, &old);
    g_ep[0] = 0xE9;
    rel = (int32_t)((uint8_t *)slr_entry_stub - (g_ep + 5));
    memcpy(g_ep + 1, &rel, 4);
    VirtualProtect(g_ep, 5, old, &old);
    FlushInstructionCache(GetCurrentProcess(), g_ep, 5);
}


BOOL WINAPI DllMain(HINSTANCE inst, DWORD reason, LPVOID reserved) {
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH) {
        g_self = inst;
        InitializeCriticalSection(&g_text_cs);
        config_init();
        if (g_cfg.text_path[0])
            g_text = CreateFileA(g_cfg.text_path, GENERIC_WRITE, FILE_SHARE_READ, NULL, CREATE_ALWAYS,
                                 FILE_ATTRIBUTE_NORMAL, NULL);
        if (slr_sink_open(&g_sink, g_cfg.log_path) == 0) g_sink_open = 1;
        slr_text("attach: seed=%08x dt_us=%u step_us=%u log=%s", g_cfg.seed, g_cfg.dt_us, g_cfg.step_us,
                 g_cfg.log_path);
        patch_entry();
    }
    return TRUE;
}
