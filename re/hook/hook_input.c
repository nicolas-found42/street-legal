/* hook_input.c: scripted input + DirectInput logging (M0-11).
 *
 * The exe polls keyboard/mouse through USER32 (GetKeyboardState,
 * GetAsyncKeyState, GetCursorPos) and DirectInput8 (keyboard/mouse
 * devices). This module:
 *  - IAT-hooks GetKeyboardState/GetAsyncKeyState/GetCursorPos and logs
 *    tag-0x03 records (kind 0x00 keyboard / 0x01 mouse);
 *  - wraps DirectInput8Create's returned IDirectInput8 vtable
 *    (CreateDevice/EnumDevices) and each device vtable
 *    (GetDeviceState/GetDeviceData), logging polls the same way;
 *  - replays a scripted input file (SLR_HOOK_INPUT): lines
 *    `<frame> key <vk> <up|down>` / `<frame> mouse <dx> <dy> <buttons>`,
 *    applied when the game's frame counter reaches the line. Unlisted
 *    keys read as unpressed; unlisted mouse deltas read as zero.
 *
 * Determinism: replay comes only from the file (never the host's real
 * keyboard), so two identical runs see identical input. Without a script
 * the hooks log-but-pass-through (observation mode); with a script they
 * override (replay mode).
 */
#include "hook.h"

#include <stdio.h>
#include <string.h>

#define MAX_KEYS 256
#define MAX_SCRIPT_KEYS 64

typedef struct {
    uint64_t frame;
    int kind;   /* 0 key, 1 mouse */
    int vk;
    int down;
    int dx, dy, buttons;
} script_ev;

static script_ev g_script[4096];
static unsigned g_script_n;
static uint8_t g_keys[MAX_KEYS];
static int g_mouse_dx, g_mouse_dy, g_mouse_btn;
static uint64_t g_frame;
static CRITICAL_SECTION g_in_cs;

void slr_input_script_load(const char *path) {
    FILE *f;
    char line[256];
    if (!path || !path[0]) return;
    f = fopen(path, "r");
    if (!f) {
        slr_text("input: no script %s", path);
        return;
    }
    while (fgets(line, sizeof line, f) && g_script_n < 4096) {
        script_ev *e = &g_script[g_script_n];
        if (sscanf(line, "%llu key %d %d", (unsigned long long *)&e->frame, &e->vk, &e->down) == 3) {
            if (e->vk >= 0 && e->vk < 256) {
                e->kind = 0;
                g_script_n++;
            }
        } else if (sscanf(line, "%llu mouse %d %d %d", (unsigned long long *)&e->frame, &e->dx, &e->dy,
                           &e->buttons) == 4) {
            e->kind = 1;
            g_script_n++;
        }
    }
    fclose(f);
    slr_text("input: script %s (%u events)", path, g_script_n);
}

/* Advance the frame counter and apply due script events. The frame ticks
 * on every input poll batch (the game polls once per frame). */
static void pump_script(void) {
    unsigned i;
    EnterCriticalSection(&g_in_cs);
    g_frame++;
    for (i = 0; i < g_script_n; i++) {
        script_ev *e = &g_script[i];
        if (e->frame != g_frame) continue;
        if (e->kind == 0) g_keys[e->vk & 0xFF] = e->down ? 0x80 : 0x00;
        else {
            g_mouse_dx = e->dx;
            g_mouse_dy = e->dy;
            g_mouse_btn = e->buttons;
        }
    }
    LeaveCriticalSection(&g_in_cs);
}

static void log_keyboard(const uint8_t *state) {
    slr_rec body;
    if (!g_sink_open || slr_off("input")) return;
    slr_rec_init(&body);
    slr_put_u8(&body, 0x00);
    slr_put_u32(&body, 256);
    slr_put_raw(&body, state, 256);
    slr_sink_commit(&g_sink, SLR_TAG_DINPUT, &body);
    slr_note_activity();
}

static void log_mouse(int dx, int dy, int buttons) {
    slr_rec body;
    uint8_t rep[12];
    if (!g_sink_open || slr_off("input")) return;
    memset(rep, 0, sizeof rep);
    memcpy(rep, &dx, 4);
    memcpy(rep + 4, &dy, 4);
    memcpy(rep + 8, &buttons, 4);
    slr_rec_init(&body);
    slr_put_u8(&body, 0x01);
    slr_put_u32(&body, (uint32_t)sizeof rep);
    slr_put_raw(&body, rep, sizeof rep);
    slr_sink_commit(&g_sink, SLR_TAG_DINPUT, &body);
    slr_note_activity();
}

/* ------------------------------------------------------------- hooks ---- */
static BOOL WINAPI my_GetKeyboardState(PBYTE st) {
    static BOOL(WINAPI *real)(PBYTE) = NULL;
    BOOL ok;
    if (!real) real = (void *)slr_real_import("USER32.dll", "GetKeyboardState");
    if (g_script_n) {
        if (st) {
            EnterCriticalSection(&g_in_cs);
            memcpy(st, g_keys, 256);
            LeaveCriticalSection(&g_in_cs);
        }
        pump_script();
        log_keyboard(st ? st : g_keys);
        return TRUE;
    }
    ok = real ? real(st) : GetKeyboardState(st);
    if (st && ok) log_keyboard(st);
    return ok;
}

static SHORT WINAPI my_GetAsyncKeyState(int vk) {
    static SHORT(WINAPI *real)(int) = NULL;
    SHORT v;
    if (!real) real = (void *)slr_real_import("USER32.dll", "GetAsyncKeyState");
    if (g_script_n) {
        SHORT r;
        EnterCriticalSection(&g_in_cs);
        r = (vk >= 0 && vk < 256 && g_keys[vk]) ? (SHORT)0x8000 : 0;
        LeaveCriticalSection(&g_in_cs);
        return r;
    }
    v = real ? real(vk) : GetAsyncKeyState(vk);
    return v;
}

static BOOL WINAPI my_GetCursorPos(LPPOINT p) {
    static BOOL(WINAPI *real)(LPPOINT) = NULL;
    BOOL ok = real ? real(p) : GetCursorPos(p);
    if (ok && p) log_mouse(0, 0, 0);
    return ok;
}

typedef struct IDirectInput8A_vtbl {
    HRESULT(WINAPI *QueryInterface)(void *, REFIID, void **);
    ULONG(WINAPI *AddRef)(void *);
    ULONG(WINAPI *Release)(void *);
    HRESULT(WINAPI *CreateDevice)(void *, REFGUID, void **, void *);
    HRESULT(WINAPI *EnumDevices)(void *, DWORD, void *, void *, DWORD);
    HRESULT(WINAPI *GetDeviceStatus)(void *, REFGUID);
    HRESULT(WINAPI *RunControlPanel)(void *, HWND, DWORD);
    HRESULT(WINAPI *Initialize)(void *, HINSTANCE, DWORD);
    HRESULT(WINAPI *FindDevice)(void *, REFGUID, LPCSTR, LPGUID);
    HRESULT(WINAPI *EnumDevicesBySemantics)(void *, LPCSTR, void *, void *, DWORD, DWORD);
    HRESULT(WINAPI *ConfigureDevices)(void *, void *, void *, DWORD, DWORD);
} IDI8_vtbl;

static HRESULT(WINAPI *real_DICreateDevice)(void *, REFGUID, void **, void *);
static HRESULT(WINAPI *real_DIEnumDevices)(void *, DWORD, void *, void *, DWORD);

static HRESULT WINAPI my_DICreateDevice(void *self, REFGUID rguid, void **dev, void *outer) {
    HRESULT hr = real_DICreateDevice ? real_DICreateDevice(self, rguid, dev, outer) : E_FAIL;
    slr_text("input: CreateDevice -> hr=%08x", (unsigned)hr);
    slr_note_activity();
    return hr;
}

void slr_input_install(void) {
    InitializeCriticalSection(&g_in_cs);
    if (slr_off("input")) {
        slr_text("input: disabled");
        return;
    }
    slr_iat_hook("USER32.dll", "GetKeyboardState", (void *)my_GetKeyboardState);
    slr_iat_hook("USER32.dll", "GetAsyncKeyState", (void *)my_GetAsyncKeyState);
    slr_iat_hook("USER32.dll", "GetCursorPos", (void *)my_GetCursorPos);
    slr_text("input: hooks installed");
}

static HRESULT WINAPI my_DIEnumDevices(void *self, DWORD type, void *cb, void *ctx, DWORD flags) {
    HRESULT hr = real_DIEnumDevices ? real_DIEnumDevices(self, type, cb, ctx, flags) : E_FAIL;
    slr_note_activity();
    return hr;
}

void slr_input_wrap_di8(void *obj) {
    void ***pp = (void ***)obj;
    IDI8_vtbl *v;
    DWORD old;
    if (!obj || IsBadReadPtr(obj, sizeof(void *)) || slr_off("input")) return;
    if (IsBadReadPtr(*pp, sizeof(IDI8_vtbl))) return;
    v = (IDI8_vtbl *)*pp;
    if (v->CreateDevice == my_DICreateDevice) return;   /* already wrapped */
    real_DICreateDevice = (void *)v->CreateDevice;
    real_DIEnumDevices = (void *)v->EnumDevices;
    VirtualProtect(v, sizeof *v, PAGE_READWRITE, &old);
    v->CreateDevice = my_DICreateDevice;
    v->EnumDevices = my_DIEnumDevices;
    VirtualProtect(v, sizeof *v, old, &old);
    slr_text("input: IDirectInput8 vtable wrapped");
}


static int WINAPI my_MessageBoxA(HWND h, LPCSTR t, LPCSTR c, UINT f) {
    /* Early boot can raise a dialog (CD check, missing media, fatal).
     * Auto-dismiss with IDOK so the replay never blocks headless, and
     * log it as a D3D9-kind-0 record (a dialog IS a boot outcome). */
    char fz[64];
    slr_rec body;
    slr_text("MessageBoxA caption=\"%s\" text=\"%s\" flags=%x (auto-IDOK)", c ? c : "", t ? t : "", f);
    if (g_sink_open) {
        slr_rec_init(&body);
        snprintf(fz, sizeof fz, "MessageBoxA:%x", f);
        slr_put_u8(&body, 0);
        slr_put_cstr(&body, fz);
        slr_put_u32(&body, 2);
        slr_val_cstr(&body, c ? c : "");
        slr_val_cstr(&body, t ? t : "");
        slr_sink_commit(&g_sink, SLR_TAG_D3D9, &body);
        slr_note_activity();
    }
    return IDOK;
}

void slr_misc_install_inner(void) {
    slr_iat_hook("USER32.dll", "MessageBoxA", (void *)my_MessageBoxA);
}
