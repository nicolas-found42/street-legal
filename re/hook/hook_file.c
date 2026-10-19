/* hook_file.c: file-I/O logging (M0-11).
 *
 * Every file call the exe imports is IAT-hooked and appended to the SLRLOG
 * as a tag-0x04 record: `u8 op, str path, u32 n + values detail`.
 * Handles are tracked in a small table so reads/writes/closes log the path
 * the handle was opened for; FindFirstFile hits log the enumerated name.
 * Byte payloads are replaced by a CRC32 unless SLR_HOOK_FILE_PAYLOAD=1
 * (content still identifies the file, without bloating the log). Paths are
 * logged verbatim (Wine Z: paths), so two runs from fresh APFS clones
 * (re/hook/run.sh) produce identical bytes.
 *
 * Ops: open(0x00) detail=[str mode, bool existed]; read(0x01)
 * detail=[long count, long crc]; write(0x02) detail=[long count, long crc];
 * close(0x03) detail=[bool ok]; find(0x04) detail=[str hit, bool more].
 */
#include "hook.h"

#include <stdio.h>

#define FH_CAP 1024

typedef struct {
    HANDLE h;
    char path[MAX_PATH];
} fh_entry;

static fh_entry g_fh[FH_CAP];
static CRITICAL_SECTION g_file_cs;

/* Small CRC32 (IEEE) to stand in for payload bytes. */
static uint32_t crc32(const void *d, size_t n) {
    static uint32_t tab[256];
    static int init;
    const uint8_t *p = (const uint8_t *)d;
    uint32_t c = 0xFFFFFFFFu;
    size_t i;
    unsigned k;
    if (!init) {
        for (i = 0; i < 256; i++) {
            uint32_t v = (uint32_t)i;
            for (k = 0; k < 8; k++) v = (v & 1) ? (v >> 1) ^ 0xEDB88320u : v >> 1;
            tab[i] = v;
        }
        init = 1;
    }
    for (i = 0; i < n; i++) c = tab[(c ^ p[i]) & 0xFF] ^ (c >> 8);
    return c ^ 0xFFFFFFFFu;
}

static void fh_put(HANDLE h, const char *path) {
    unsigned i;
    if (h == NULL || h == INVALID_HANDLE_VALUE) return;
    EnterCriticalSection(&g_file_cs);
    for (i = 0; i < FH_CAP; i++)
        if (!g_fh[i].h || g_fh[i].h == h) {
            g_fh[i].h = h;
            strncpy(g_fh[i].path, path ? path : "?", sizeof g_fh[i].path - 1);
            g_fh[i].path[sizeof g_fh[i].path - 1] = 0;
            break;
        }
    LeaveCriticalSection(&g_file_cs);
}

static void fh_path(HANDLE h, char *dst, size_t cap) {
    unsigned i;
    strncpy(dst, "?", cap - 1);
    dst[cap - 1] = 0;
    if (!h || h == INVALID_HANDLE_VALUE) return;
    EnterCriticalSection(&g_file_cs);
    for (i = 0; i < FH_CAP; i++)
        if (g_fh[i].h == h) {
            strncpy(dst, g_fh[i].path, cap - 1);
            dst[cap - 1] = 0;
            break;
        }
    LeaveCriticalSection(&g_file_cs);
}

static void fh_del(HANDLE h) {
    unsigned i;
    EnterCriticalSection(&g_file_cs);
    for (i = 0; i < FH_CAP; i++)
        if (g_fh[i].h == h) {
            g_fh[i].h = NULL;
            g_fh[i].path[0] = 0;
            break;
        }
    LeaveCriticalSection(&g_file_cs);
}


/* Explicit-count variant used by every hook below. */
static void log_file_n(uint8_t op, const char *path, slr_rec *detail, uint32_t nvals) {
    slr_rec body;
    if (!g_sink_open || slr_off("files")) {
        slr_rec_free(detail);
        return;
    }
    slr_rec_init(&body);
    slr_put_u8(&body, op);
    slr_put_cstr(&body, path ? path : "?");
    slr_put_u32(&body, nvals);
    slr_put_raw(&body, detail->p, detail->n);
    slr_rec_free(detail);
    slr_sink_commit(&g_sink, SLR_TAG_FILE, &body);
    slr_note_activity();
}

/* ------------------------------------------------------------- hooks ---- */
static HANDLE WINAPI my_CreateFileA(LPCSTR name, DWORD acc, DWORD share, LPSECURITY_ATTRIBUTES sa,
                                    DWORD disp, DWORD flags, HANDLE tmpl) {
    static HANDLE(WINAPI *real)(LPCSTR, DWORD, DWORD, LPSECURITY_ATTRIBUTES, DWORD, DWORD, HANDLE) = NULL;
    HANDLE h;
    slr_rec d;
    if (!real) real = (void *)slr_real_import("KERNEL32.dll", "CreateFileA");
    h = real ? real(name, acc, share, sa, disp, flags, tmpl)
             : CreateFileA(name, acc, share, sa, disp, flags, tmpl);
    fh_put(h, name);
    slr_rec_init(&d);
    {
        char mode[16];
        snprintf(mode, sizeof mode, "%s%s%u", (acc & GENERIC_WRITE) ? "w" : "r", (acc & GENERIC_READ) && (acc & GENERIC_WRITE) ? "+" : "", (unsigned)disp);
        slr_val_cstr(&d, mode);
        slr_val_bool(&d, h != INVALID_HANDLE_VALUE);
    }
    log_file_n(0x00, name, &d, 2);
    return h;
}
static BOOL WINAPI my_ReadFile(HANDLE h, LPVOID buf, DWORD n, LPDWORD got, LPOVERLAPPED ov) {
    static BOOL(WINAPI *real)(HANDLE, LPVOID, DWORD, LPDWORD, LPOVERLAPPED) = NULL;
    BOOL ok;
    DWORD g = 0;
    char path[MAX_PATH];
    if (g_sink_open && slr_sink_inside(&g_sink)) {
        if (!real) real = (void *)slr_real_import("KERNEL32.dll", "ReadFile");
        return real ? real(h, buf, n, got, ov) : ReadFile(h, buf, n, got, ov);
    }
    if (!real) real = (void *)slr_real_import("KERNEL32.dll", "ReadFile");
    ok = real ? real(h, buf, n, got ? got : &g, ov) : ReadFile(h, buf, n, got ? got : &g, ov);
    fh_path(h, path, sizeof path);
    {
        slr_rec d;
        DWORD have = got ? *got : g;
        slr_rec_init(&d);
        slr_val_long(&d, (int64_t)have);
        if (g_cfg.file_payload && have && buf && !IsBadReadPtr(buf, have))
            slr_val_bytes(&d, buf, have);
        else
            slr_val_long(&d, (int64_t)(have && buf && !IsBadReadPtr(buf, have) ? crc32(buf, have) : 0));
        log_file_n(0x01, path, &d, 2);
    }
    return ok;
}

static BOOL WINAPI my_WriteFile(HANDLE h, LPCVOID buf, DWORD n, LPDWORD put, LPOVERLAPPED ov) {
    static BOOL(WINAPI *real)(HANDLE, LPCVOID, DWORD, LPDWORD, LPOVERLAPPED) = NULL;
    BOOL ok;
    DWORD p = 0;
    char path[MAX_PATH];
    if (g_sink_open && slr_sink_inside(&g_sink)) {
        if (!real) real = (void *)slr_real_import("KERNEL32.dll", "WriteFile");
        return real ? real(h, buf, n, put, ov) : WriteFile(h, buf, n, put, ov);
    }
    if (!real) real = (void *)slr_real_import("KERNEL32.dll", "WriteFile");
    fh_path(h, path, sizeof path);
    {
        /* Log the intent before the write (a failing write still logs). */
        slr_rec d;
        slr_rec_init(&d);
        slr_val_long(&d, (int64_t)n);
        if (g_cfg.file_payload && n && buf && !IsBadReadPtr(buf, n))
            slr_val_bytes(&d, buf, n);
        else
            slr_val_long(&d, (int64_t)(n && buf && !IsBadReadPtr(buf, n) ? crc32(buf, n) : 0));
        log_file_n(0x02, path, &d, 2);
    }
    ok = real ? real(h, buf, n, put ? put : &p, ov) : WriteFile(h, buf, n, put ? put : &p, ov);
    return ok;
}

static BOOL WINAPI my_CloseHandle(HANDLE h) {
    static BOOL(WINAPI *real)(HANDLE) = NULL;
    char path[MAX_PATH];
    BOOL ok;
    if (g_sink_open && slr_sink_inside(&g_sink)) {
        if (!real) real = (void *)slr_real_import("KERNEL32.dll", "CloseHandle");
        return real ? real(h) : CloseHandle(h);
    }
    if (!real) real = (void *)slr_real_import("KERNEL32.dll", "CloseHandle");
    fh_path(h, path, sizeof path);
    ok = real ? real(h) : CloseHandle(h);
    {
        slr_rec d;
        slr_rec_init(&d);
        slr_val_bool(&d, ok);
        log_file_n(0x03, path, &d, 1);
    }
    fh_del(h);
    return ok;
}

static HANDLE WINAPI my_FindFirstFileA(LPCSTR pat, LPWIN32_FIND_DATAA fd) {
    static HANDLE(WINAPI *real)(LPCSTR, LPWIN32_FIND_DATAA) = NULL;
    HANDLE h;
    if (!real) real = (void *)slr_real_import("KERNEL32.dll", "FindFirstFileA");
    h = real ? real(pat, fd) : FindFirstFileA(pat, fd);
    {
        slr_rec d;
        slr_rec_init(&d);
        slr_val_cstr(&d, (h != INVALID_HANDLE_VALUE && fd) ? fd->cFileName : "");
        slr_val_bool(&d, h != INVALID_HANDLE_VALUE);
        log_file_n(0x04, pat, &d, 2);
    }
    return h;
}

static BOOL WINAPI my_FindNextFileA(HANDLE h, LPWIN32_FIND_DATAA fd) {
    static BOOL(WINAPI *real)(HANDLE, LPWIN32_FIND_DATAA) = NULL;
    BOOL ok;
    char path[MAX_PATH];
    if (!real) real = (void *)slr_real_import("KERNEL32.dll", "FindNextFileA");
    snprintf(path, sizeof path, "find:%p", h);
    ok = real ? real(h, fd) : FindNextFileA(h, fd);
    {
        slr_rec d;
        slr_rec_init(&d);
        slr_val_cstr(&d, (ok && fd) ? fd->cFileName : "");
        slr_val_bool(&d, ok);
        log_file_n(0x04, path, &d, 2);
    }
    return ok;
}
static HANDLE WINAPI my_CreateFileW(LPCWSTR name, DWORD acc, DWORD share, LPSECURITY_ATTRIBUTES sa,
                                    DWORD disp, DWORD flags, HANDLE tmpl) {
    static HANDLE(WINAPI *real)(LPCWSTR, DWORD, DWORD, LPSECURITY_ATTRIBUTES, DWORD, DWORD, HANDLE) = NULL;
    HANDLE h;
    char narrow[MAX_PATH * 2];
    slr_rec d;
    if (!real) real = (void *)slr_real_import("KERNEL32.dll", "CreateFileW");
    h = real ? real(name, acc, share, sa, disp, flags, tmpl)
             : CreateFileW(name, acc, share, sa, disp, flags, tmpl);
    narrow[0] = 0;
    if (name) WideCharToMultiByte(CP_UTF8, 0, name, -1, narrow, sizeof narrow, NULL, NULL);
    fh_put(h, narrow);
    slr_rec_init(&d);
    {
        char mode[16];
        snprintf(mode, sizeof mode, "%s%s%u", (acc & GENERIC_WRITE) ? "w" : "r", (acc & GENERIC_READ) && (acc & GENERIC_WRITE) ? "+" : "", (unsigned)disp);
        slr_val_cstr(&d, mode);
        slr_val_bool(&d, h != INVALID_HANDLE_VALUE);
    }
    log_file_n(0x00, narrow, &d, 2);
    return h;
}


void slr_file_install(void) {
    InitializeCriticalSection(&g_file_cs);
    if (slr_off("files")) {
        slr_text("files: disabled");
        return;
    }
    slr_iat_hook("KERNEL32.dll", "CreateFileA", (void *)my_CreateFileA);
    slr_iat_hook("KERNEL32.dll", "CreateFileW", (void *)my_CreateFileW);
    slr_iat_hook("KERNEL32.dll", "ReadFile", (void *)my_ReadFile);
    slr_iat_hook("KERNEL32.dll", "WriteFile", (void *)my_WriteFile);
    slr_iat_hook("KERNEL32.dll", "CloseHandle", (void *)my_CloseHandle);
    slr_iat_hook("KERNEL32.dll", "FindFirstFileA", (void *)my_FindFirstFileA);
    slr_iat_hook("KERNEL32.dll", "FindNextFileA", (void *)my_FindNextFileA);
    slr_text("files: hooks installed");
}
