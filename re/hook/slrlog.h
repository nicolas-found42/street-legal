/* slrlog.h: header-only SLRLOG v1 emitter (docs/call-log.md, crates/slr-calllog).
 *
 * Byte-compatible with the Rust LogWriter: little-endian, floats as exact bit
 * patterns, strings as u32 length + UTF-8 (no NUL). A record is built in a
 * caller-owned buffer (slr_rec), then slr_log_commit() frames it
 * (tag, u32 body length, body) and appends it to the file under a lock, so
 * records from different threads never interleave mid-record.
 *
 * Host-portable on purpose (no Win32 types in the encoder core): the unit
 * test re/hook/tests/emit_test.c compiles this header with the host cc and
 * its output is checked against the Python reference emitter and the Rust
 * reader. Define SLRLOG_WIN32 before including to use Win32 file/lock calls.
 */
#ifndef SLRLOG_H
#define SLRLOG_H

#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

#define SLRLOG_VERSION 1
#define SLR_FN static __attribute__((unused))

/* record tags */
#define SLR_TAG_NATIVE 0x00
#define SLR_TAG_D3D9 0x01
#define SLR_TAG_DSOUND 0x02
#define SLR_TAG_DINPUT 0x03
#define SLR_TAG_FILE 0x04
#define SLR_TAG_VMSNAP 0x05
#define SLR_TAG_VEHICLE 0x06

/* value tags */
#define SLR_V_INT 0x00
#define SLR_V_LONG 0x01
#define SLR_V_FLOAT 0x02
#define SLR_V_DOUBLE 0x03
#define SLR_V_STR 0x04
#define SLR_V_BYTES 0x05
#define SLR_V_BOOL 0x06

/* sanity caps mirrored from the reader */
#define SLR_MAX_ARGS 64
#define SLR_MAX_PAYLOAD (8u * 1024u * 1024u)

typedef struct {
    uint8_t *p;
    size_t n, cap;
} slr_rec;

SLR_FN void slr_rec_init(slr_rec *r) {
    r->p = NULL;
    r->n = 0;
    r->cap = 0;
}
SLR_FN void slr_rec_free(slr_rec *r) {
    free(r->p);
    slr_rec_init(r);
}
SLR_FN void slr_rec_reserve(slr_rec *r, size_t extra) {
    if (r->n + extra > r->cap) {
        size_t nc = r->cap ? r->cap : 256;
        while (nc < r->n + extra) nc *= 2;
        r->p = (uint8_t *)realloc(r->p, nc);
        r->cap = nc;
    }
}
SLR_FN void slr_put_u8(slr_rec *r, uint8_t v) {
    slr_rec_reserve(r, 1);
    r->p[r->n++] = v;
}
SLR_FN void slr_put_u16(slr_rec *r, uint16_t v) {
    slr_put_u8(r, (uint8_t)v);
    slr_put_u8(r, (uint8_t)(v >> 8));
}
SLR_FN void slr_put_u32(slr_rec *r, uint32_t v) {
    slr_put_u8(r, (uint8_t)v);
    slr_put_u8(r, (uint8_t)(v >> 8));
    slr_put_u8(r, (uint8_t)(v >> 16));
    slr_put_u8(r, (uint8_t)(v >> 24));
}
SLR_FN void slr_put_u64(slr_rec *r, uint64_t v) {
    slr_put_u32(r, (uint32_t)v);
    slr_put_u32(r, (uint32_t)(v >> 32));
}
SLR_FN void slr_put_raw(slr_rec *r, const void *d, size_t n) {
    slr_rec_reserve(r, n);
    if (n) memcpy(r->p + r->n, d, n);
    r->n += n;
}

/* str primitive from UTF-8 bytes */
SLR_FN void slr_put_str_utf8(slr_rec *r, const char *s, size_t n) {
    if (n > SLR_MAX_PAYLOAD) n = SLR_MAX_PAYLOAD;
    slr_put_u32(r, (uint32_t)n);
    slr_put_raw(r, s, n);
}

/* str primitive from ANSI/Latin-1 bytes: bytes >= 0x80 are widened to their
 * two-byte UTF-8 form so the Rust reader (UTF-8 only) always accepts the log.
 * ASCII-only input is passed through unchanged. */
SLR_FN void slr_put_str_latin1(slr_rec *r, const char *s, size_t n) {
    size_t extra = 0, i;
    for (i = 0; i < n; i++)
        if ((uint8_t)s[i] >= 0x80) extra++;
    if (n + extra > SLR_MAX_PAYLOAD) {
        n = SLR_MAX_PAYLOAD - extra;
    }
    slr_put_u32(r, (uint32_t)(n + extra));
    for (i = 0; i < n; i++) {
        uint8_t c = (uint8_t)s[i];
        if (c < 0x80) {
            slr_put_u8(r, c);
        } else {
            slr_put_u8(r, (uint8_t)(0xC0 | (c >> 6)));
            slr_put_u8(r, (uint8_t)(0x80 | (c & 0x3F)));
        }
    }
}
SLR_FN void slr_put_cstr(slr_rec *r, const char *s) { slr_put_str_latin1(r, s, strlen(s)); }

/* values: each is tag + payload */
SLR_FN void slr_val_int(slr_rec *r, int32_t v) {
    slr_put_u8(r, SLR_V_INT);
    slr_put_u32(r, (uint32_t)v);
}
SLR_FN void slr_val_long(slr_rec *r, int64_t v) {
    slr_put_u8(r, SLR_V_LONG);
    slr_put_u64(r, (uint64_t)v);
}
SLR_FN void slr_val_float_bits(slr_rec *r, uint32_t bits) {
    slr_put_u8(r, SLR_V_FLOAT);
    slr_put_u32(r, bits);
}
SLR_FN void slr_val_double_bits(slr_rec *r, uint64_t bits) {
    slr_put_u8(r, SLR_V_DOUBLE);
    slr_put_u64(r, bits);
}
SLR_FN void slr_val_str_latin1(slr_rec *r, const char *s, size_t n) {
    slr_put_u8(r, SLR_V_STR);
    slr_put_str_latin1(r, s, n);
}
SLR_FN void slr_val_cstr(slr_rec *r, const char *s) { slr_val_str_latin1(r, s, strlen(s)); }
SLR_FN void slr_val_bytes(slr_rec *r, const void *d, size_t n) {
    if (n > SLR_MAX_PAYLOAD) n = SLR_MAX_PAYLOAD;
    slr_put_u8(r, SLR_V_BYTES);
    slr_put_u32(r, (uint32_t)n);
    slr_put_raw(r, d, n);
}
SLR_FN void slr_val_bool(slr_rec *r, int v) {
    slr_put_u8(r, SLR_V_BOOL);
    slr_put_u8(r, v ? 1 : 0);
}

/* ---- sink ---------------------------------------------------------------
 * A file-backed sink with a 256 KiB write buffer. The lock and the file
 * primitives are supplied by the includer (SLRLOG_WIN32 selects Win32). */
typedef struct {
    void *file;      /* FILE* or HANDLE */
    uint8_t *buf;
    size_t n, cap;
    uint64_t count;  /* records committed */
    void *lock;      /* CRITICAL_SECTION* when WIN32 */
#ifdef SLRLOG_WIN32
    /* Re-entrancy guard: the hook layer IAT-hooks WriteFile/CloseHandle
     * in the same process. The sink calls the REAL imports (captured at
     * open time, before hooking) and sets `inside` around them, so a
     * hooked call made while inside skips logging instead of recursing. */
    void *real_write;   /* BOOL(WINAPI*)(HANDLE,LPCVOID,DWORD,LPDWORD,LPOVERLAPPED) */
    void *real_close;   /* BOOL(WINAPI*)(HANDLE) */
    volatile LONG inside;
#endif
} slr_sink;


#ifdef SLRLOG_WIN32
#include <windows.h>
SLR_FN void slr_sink_lock(slr_sink *s) { EnterCriticalSection((CRITICAL_SECTION *)s->lock); }
SLR_FN void slr_sink_unlock(slr_sink *s) { LeaveCriticalSection((CRITICAL_SECTION *)s->lock); }
SLR_FN int slr_sink_inside(slr_sink *s) { return InterlockedCompareExchange(&s->inside, 1, 1) != 0; }
SLR_FN void slr_sink_write_raw(slr_sink *s, const void *d, size_t n) {
    /* Call the REAL WriteFile captured at open time: the IAT-hooked
     * version would log (and recurse) here. */
    BOOL (WINAPI *real)(HANDLE, LPCVOID, DWORD, LPDWORD, LPOVERLAPPED);
    DWORD w;
    const uint8_t *p = (const uint8_t *)d;
    real = (void *)s->real_write;
    if (!real) return;
    InterlockedIncrement(&s->inside);
    while (n) {
        DWORD chunk = n > 0x100000 ? 0x100000 : (DWORD)n;
        if (!real((HANDLE)s->file, p, chunk, &w, NULL) || w == 0) break;
        p += w;
        n -= w;
    }
    InterlockedDecrement(&s->inside);
}
SLR_FN int slr_sink_open(slr_sink *s, const char *path) {
    /* Capture REAL imports before any IAT hooking happens (this runs in
     * DllMain, before the entry-point trampoline installs hooks). */
    HANDLE (WINAPI *cre)(LPCSTR, DWORD, DWORD, LPSECURITY_ATTRIBUTES, DWORD, DWORD, HANDLE);
    BOOL (WINAPI *wr)(HANDLE, LPCVOID, DWORD, LPDWORD, LPOVERLAPPED);
    BOOL (WINAPI *cl)(HANDLE);
    HANDLE h;
    CRITICAL_SECTION *cs;
    uint8_t hdr[8] = {'S', 'L', 'R', 'L', 'O', 'G', SLRLOG_VERSION, 0};
    HMODULE k32 = GetModuleHandleA("KERNEL32.dll");
    cre = (void *)GetProcAddress(k32, "CreateFileA");
    wr = (void *)GetProcAddress(k32, "WriteFile");
    cl = (void *)GetProcAddress(k32, "CloseHandle");
    if (!cre || !wr || !cl) return -1;
    h = cre(path, GENERIC_WRITE, FILE_SHARE_READ, NULL, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
    if (h == INVALID_HANDLE_VALUE) return -1;
    cs = (CRITICAL_SECTION *)malloc(sizeof *cs);
    InitializeCriticalSection(cs);
    s->file = h;
    s->lock = cs;
    s->real_write = (void *)wr;
    s->real_close = (void *)cl;
    s->inside = 0;
    s->cap = 256 * 1024;
    s->buf = (uint8_t *)malloc(s->cap);
    s->n = 0;
    s->count = 0;
    slr_sink_write_raw(s, hdr, 8);
    return 0;
}
SLR_FN void slr_sink_close_file(slr_sink *s) {
    BOOL (WINAPI *real)(HANDLE) = (void *)s->real_close;
    InterlockedIncrement(&s->inside);
    if (real) real((HANDLE)s->file);
    InterlockedDecrement(&s->inside);
}
#else
SLR_FN void slr_sink_lock(slr_sink *s) { (void)s; }
SLR_FN void slr_sink_unlock(slr_sink *s) { (void)s; }
SLR_FN void slr_sink_write_raw(slr_sink *s, const void *d, size_t n) {
    fwrite(d, 1, n, (FILE *)s->file);
}
SLR_FN int slr_sink_open(slr_sink *s, const char *path) {
    FILE *f = fopen(path, "wb");
    uint8_t hdr[8] = {'S', 'L', 'R', 'L', 'O', 'G', SLRLOG_VERSION, 0};
    if (!f) return -1;
    s->file = f;
    s->lock = NULL;
    s->cap = 256 * 1024;
    s->buf = (uint8_t *)malloc(s->cap);
    s->n = 0;
    s->count = 0;
    slr_sink_write_raw(s, hdr, 8);
    return 0;
}
SLR_FN void slr_sink_close_file(slr_sink *s) { fclose((FILE *)s->file); }
#endif

SLR_FN void slr_sink_flush_locked(slr_sink *s) {
    if (s->n) {
        slr_sink_write_raw(s, s->buf, s->n);
        s->n = 0;
    }
}
SLR_FN void slr_sink_flush(slr_sink *s) {
    slr_sink_lock(s);
    slr_sink_flush_locked(s);
    slr_sink_unlock(s);
}

/* Append one framed record: tag, u32 LE body length, body. Frees the body. */
SLR_FN void slr_sink_commit(slr_sink *s, uint8_t tag, slr_rec *body) {
    uint8_t hdr[5];
    hdr[0] = tag;
    hdr[1] = (uint8_t)body->n;
    hdr[2] = (uint8_t)(body->n >> 8);
    hdr[3] = (uint8_t)(body->n >> 16);
    hdr[4] = (uint8_t)(body->n >> 24);
    slr_sink_lock(s);
    if (s->n + 5 + body->n > s->cap) slr_sink_flush_locked(s);
    if (5 + body->n > s->cap) {
        slr_sink_write_raw(s, hdr, 5);
        slr_sink_write_raw(s, body->p, body->n);
    } else {
        memcpy(s->buf + s->n, hdr, 5);
        s->n += 5;
        if (body->n) memcpy(s->buf + s->n, body->p, body->n);
        s->n += body->n;
    }
    s->count++;
    slr_sink_unlock(s);
    slr_rec_free(body);
}

SLR_FN void slr_sink_close(slr_sink *s) {
    slr_sink_flush(s);
    slr_sink_close_file(s);
}

#endif /* SLRLOG_H */
