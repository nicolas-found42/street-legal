/* hook.h: shared declarations for the slrhook proxy DLL (M0-11). */
#ifndef SLR_HOOK_H
#define SLR_HOOK_H

#define SLRLOG_WIN32
#include <windows.h>
#include <stdint.h>
#include "slrlog.h"

/* ---- configuration (environment, read once in slr_config_init) ---------- */
typedef struct {
    char log_path[MAX_PATH];     /* SLR_HOOK_LOG: SLRLOG output (default hook.slrlog) */
    char text_path[MAX_PATH];    /* SLR_HOOK_TEXT: human diagnostic log, "" = off */
    char game_dir[MAX_PATH];     /* directory of the exe, no trailing slash */
    char trace[256];             /* SLR_HOOK_TRACE: import-trace dlls, "all" or "" */
    char input_path[MAX_PATH];   /* SLR_HOOK_INPUT: scripted input file */
    uint32_t seed;               /* SLR_HOOK_SEED: pinned epoch/seed */
    uint32_t dt_us;              /* SLR_HOOK_DT_US: fixed per-frame time step, microseconds */
    uint32_t step_us;            /* SLR_HOOK_STEP_US: virtual time added per clock read */
    uint32_t max_virtual_ms;     /* SLR_HOOK_MAX_MS: exit when virtual time reaches this (0 = never) */
    uint32_t idle_exit_ms;       /* SLR_HOOK_IDLE_MS: wall-clock idle watchdog (0 = off) */
    uint32_t max_records;        /* SLR_HOOK_MAX_RECORDS (0 = unlimited) */
    uint32_t file_payload;       /* SLR_HOOK_FILE_PAYLOAD=1: log payload bytes, not just crc */
    char disable[128];           /* SLR_HOOK_DISABLE: comma list (com,natives,files,time,...) */
    uint32_t force_windowed;     /* SLR_HOOK_WINDOWED (default 1): rewrite fullscreen present params */
} slr_config;

extern slr_config g_cfg;
extern slr_sink g_sink;
extern int g_sink_open;
extern HMODULE g_self;

int slr_off(const char *what);

/* ---- diagnostics -------------------------------------------------------- */
void slr_text(const char *fmt, ...);

/* ---- hooking primitives ------------------------------------------------- */
void *slr_alloc_exec(size_t n);
/* Overwrite an exe IAT slot. Returns the previous value (0 if not found). */
void *slr_iat_hook(const char *dll, const char *name, void *replacement);
/* Overwrite the IAT slot for an import by ordinal. */
void *slr_iat_hook_ord(const char *dll, unsigned ordinal, void *replacement);
/* Resolve the real import from the loaded DLL (never from the exe IAT, which
 * may already hold our hook). Returns NULL if the DLL/import is missing. */
void *slr_real_import(const char *dll, const char *name);
/* Inline detour of a 32-bit function whose first `stolen` bytes are position
 * independent: copies them to a trampoline that runs them then jumps back.
 *
 * Publish order: the trampoline is stored into *publish BEFORE the target's
 * entry bytes are patched, so a hook that punts to the trampoline can never be
 * entered with a NULL slot. `publish` is the slot the hop reads at call time;
 * pass NULL only when the hook never reads a trampoline. */
void slr_detour(void *target, void *detour, unsigned stolen, void **publish);

/* ---- subsystems --------------------------------------------------------- */
void slr_time_install(void);
void slr_file_install(void);
void slr_native_install(void);
void slr_com_install(void);
void slr_misc_install(void);
void slr_input_install(void);
void slr_input_script_load(const char *path);
void slr_native_hook_all(void);
/* record helpers */
void slr_log_native(const char *cls, const char *name, slr_rec *argvals, unsigned argc,
                    const slr_rec *res, int has_res);
void slr_note_activity(void);
uint64_t slr_virtual_us(void);
void slr_finish(const char *why);

#endif
