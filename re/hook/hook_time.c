/* hook_time.c: deterministic virtual clock (M0-11).
 *
 * The stock exe reads wall time through KERNEL32 GetTickCount /
 * QueryPerformanceCounter+Frequency, GetSystemTime / GetLocalTime, and WINMM
 * timeGetTime. Under Wine each of these returns host time, so two runs of
 * the same replay diverge. This module IAT-hooks all of them onto one
 * virtual clock:
 *
 *   virtual_us starts at seed*1000 (SLR_HOOK_SEED, default 0x2005020b =
 *   the build-606 release date) and advances by a fixed SLR_HOOK_STEP_US
 *   (default 50) on every clock read. GetTickCount/timeGetTime return
 *   virtual_us/1000; QueryPerformanceCounter returns virtual_us*10
 *   (a fixed 10 MHz frequency); GetSystemTime returns the fixed date
 *   2005-02-11 (build 606 month) plus virtual elapsed days carry.
 *
 * Real sleeps still sleep (timing), but Sleep(0)/SwitchToThread yield
 * without logging; Sleep(N>0) advances the virtual clock by N ms so game
 * timeouts expire deterministically. Nothing is emitted to the SLRLOG
 * (clocks are inputs, sampled implicitly through the calls they order);
 * determinism is verified by whole-log equality across runs.
 */
#include "hook.h"

#include <stdio.h>

static uint64_t g_vus;          /* virtual microseconds since pinned epoch */
static uint64_t g_qpf = 10000000ULL;    /* fixed performance frequency (10 MHz) */
static CRITICAL_SECTION g_time_cs;

uint64_t slr_virtual_us(void) {
    uint64_t v;
    EnterCriticalSection(&g_time_cs);
    v = g_vus;
    LeaveCriticalSection(&g_time_cs);
    return v;
}

/* Advance by one step and return the new value. Every clock read is one
 * deterministic tick; no host time ever enters. */
static uint64_t tick(void) {
    /* No activity signal: clock reads happen on every thread (including
     * the timer thread's 1ms loop) and must never gate exit. */
    uint64_t v;
    EnterCriticalSection(&g_time_cs);
    g_vus += g_cfg.step_us ? g_cfg.step_us : 50;
    v = g_vus;
    LeaveCriticalSection(&g_time_cs);
    if (g_cfg.max_virtual_ms && v / 1000 >= g_cfg.max_virtual_ms)
        slr_finish("virtual time budget");
    return v;
}

/* ------------------------------------------------------------ hooks ---- */
static DWORD WINAPI my_GetTickCount(void) { return (DWORD)(tick() / 1000); }

static DWORD WINAPI my_timeGetTime(void) { return (DWORD)(tick() / 1000); }

static BOOL WINAPI my_QueryPerformanceCounter(LARGE_INTEGER *p) {
    uint64_t v = tick();
    if (p) p->QuadPart = (LONGLONG)(v * 10);    /* 10 ticks/us at 10 MHz */
    return TRUE;
}

static BOOL WINAPI my_QueryPerformanceFrequency(LARGE_INTEGER *p) {
    if (p) p->QuadPart = (LONGLONG)g_qpf;
    return TRUE;
}

/* Fixed build-606-era date; wDayOfWeek computed for 2005-02-11 (Friday). */
static void fill_systime(LPSYSTEMTIME t, uint64_t v_us) {
    uint64_t days = v_us / (uint64_t)86400000000ULL;
    if (t) {
        t->wYear = 2005;
        t->wMonth = 2;
        t->wDay = (WORD)(11 + days);
        t->wHour = 11;
        t->wMinute = 40;
        t->wSecond = 53;
        t->wMilliseconds = (WORD)((v_us / 1000) % 1000);
        t->wDayOfWeek = (WORD)((5 + days) % 7);  /* 2005-02-11 was a Friday */
    }
}

static void WINAPI my_GetSystemTime(LPSYSTEMTIME t) {
    uint64_t v = tick();
    fill_systime(t, v);
}

static void WINAPI my_GetLocalTime(LPSYSTEMTIME t) {
    uint64_t v = tick();
    fill_systime(t, v);
}

static void WINAPI my_GetSystemTimeAsFileTime(LPFILETIME t) {
    uint64_t v = tick();
    /* 2005-02-11 11:40:53 UTC as Windows FILETIME (100ns since 1601-01-01):
     * 127842276530000000 + virtual microseconds * 10. */
    uint64_t ft = (uint64_t)127842276530000000ULL + v * 10;
    if (t) {
        t->dwLowDateTime = (DWORD)ft;
        t->dwHighDateTime = (DWORD)(ft >> 32);
    }
}

static void WINAPI my_Sleep(DWORD ms) {
    if (ms == 0) return;        /* pure yield: no virtual advance */
    EnterCriticalSection(&g_time_cs);
    g_vus += (uint64_t)ms * 1000;
    LeaveCriticalSection(&g_time_cs);
    /* Real wait is skipped: the virtual clock already accounts for it and
     * a real sleep would only add wall-clock nondeterminism. Message-pump
     * progress is driven by the watchdog-free main loop instead. */
}

static MMRESULT WINAPI my_timeBeginPeriod(UINT u) { (void)u; return TIMERR_NOERROR; }
static MMRESULT WINAPI my_timeEndPeriod(UINT u) { (void)u; return TIMERR_NOERROR; }

void slr_time_install(void) {
    InitializeCriticalSection(&g_time_cs);
    g_vus = (uint64_t)g_cfg.seed * 1000;
    slr_iat_hook("KERNEL32.dll", "GetTickCount", (void *)my_GetTickCount);
    slr_iat_hook("KERNEL32.dll", "QueryPerformanceCounter", (void *)my_QueryPerformanceCounter);
    slr_iat_hook("KERNEL32.dll", "QueryPerformanceFrequency", (void *)my_QueryPerformanceFrequency);
    slr_iat_hook("KERNEL32.dll", "GetSystemTime", (void *)my_GetSystemTime);
    slr_iat_hook("KERNEL32.dll", "GetLocalTime", (void *)my_GetLocalTime);
    slr_iat_hook("KERNEL32.dll", "GetSystemTimeAsFileTime", (void *)my_GetSystemTimeAsFileTime);
    slr_iat_hook("KERNEL32.dll", "Sleep", (void *)my_Sleep);
    slr_iat_hook("WINMM.dll", "timeGetTime", (void *)my_timeGetTime);
    slr_iat_hook("WINMM.dll", "timeBeginPeriod", (void *)my_timeBeginPeriod);
    slr_iat_hook("WINMM.dll", "timeEndPeriod", (void *)my_timeEndPeriod);
    /* GetSystemInfo stays unhooked: topology is host truth, constant across
     * runs, and faking a 1-CPU box would change the game's thread pool. */
    slr_text("time: seed=%08x step_us=%u v0=%llu", g_cfg.seed, g_cfg.step_us ? g_cfg.step_us : 50,
             (unsigned long long)g_vus);
}
