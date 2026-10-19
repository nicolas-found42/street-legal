/* stubs.c: proxy export + misc install (M0-11).
 *
 * This DLL poses as dinput8.dll (WINEDLLOVERRIDES=dinput8=n,b). The only
 * export the exe imports is DirectInput8Create: forward it to the real
 * system dinput8, then wrap the returned IDirectInput8 (see hook_input.c)
 * so device enumeration and polling are logged.
 */
#include "hook.h"

void slr_input_wrap_di8(void *obj);
void slr_misc_install_inner(void);

HRESULT WINAPI DirectInput8Create(HINSTANCE h, DWORD ver, REFIID riid, LPVOID *out, LPUNKNOWN outer) {
    static HRESULT(WINAPI *real)(HINSTANCE, DWORD, REFIID, LPVOID *, LPUNKNOWN) = NULL;
    HRESULT hr;
    if (!real) {
        char path[MAX_PATH];
        HMODULE m;
        GetSystemDirectoryA(path, sizeof path);
        strncat(path, "\\dinput8.dll", sizeof path - strlen(path) - 1);
        m = LoadLibraryA(path);
        real = m ? (void *)GetProcAddress(m, "DirectInput8Create") : NULL;
    }
    hr = real ? real(h, ver, riid, out, outer) : E_FAIL;
    if (SUCCEEDED(hr) && out && *out) slr_input_wrap_di8(*out);
    slr_text("dinput: DirectInput8Create -> hr=%08x", (unsigned)hr);
    return hr;
}

HRESULT WINAPI slr_real_DirectInput8Create(HINSTANCE h, DWORD ver, REFIID riid, LPVOID *out,
                                           LPUNKNOWN outer) {
    return DirectInput8Create(h, ver, riid, out, outer);
}

void slr_misc_install(void) {
    slr_misc_install_inner();
    slr_text("misc: installed");
}
