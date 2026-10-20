"""Shared helpers for synthetic platform-model tests (need the local pinned exe)."""

import copy
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from graphics import GraphicsProfile
from make_boot_profile import GRAPHICS, PROFILE
from platform_profile import PlatformProfile
from startup import REPO, Boundary, Startup

EXE = REPO / "reference/raw/StreetLegal_Redline.bak.exe"
HWND = 0x41000010


def profile_data(**sections):
    data = copy.deepcopy(PROFILE)
    for name, value in sections.items():
        if value is None:
            data.pop(name, None)
        else:
            data[name] = value
    return data


def make_startup(**sections):
    graphics = GraphicsProfile(copy.deepcopy(GRAPHICS))
    startup = Startup(
        EXE,
        graphics_profile=graphics,
        platform_profile=PlatformProfile(profile_data(**sections)),
    )
    startup.windows[HWND] = {"class": "T", "title": "t", "visible": True, "style": 0x90000000}
    return startup


def create_d3d(startup):
    """Direct3DCreate9(31) through the declared enumerator; returns the IDirect3D9 pointer."""
    return startup.graphics.create([31])


def present_parameters(startup, **changes):
    values = {
        "BackBufferWidth": 800, "BackBufferHeight": 600, "BackBufferFormat": 21,
        "BackBufferCount": 1, "MultiSampleType": 0, "MultiSampleQuality": 0,
        "SwapEffect": 2, "hDeviceWindow": HWND, "Windowed": 0,
        "EnableAutoDepthStencil": 1, "AutoDepthStencilFormat": 71, "Flags": 0,
        "FullScreen_RefreshRateInHz": 0, "PresentationInterval": 0,
    }
    values.update(changes)
    pointer = startup.alloc(56)
    startup.write(pointer, struct.pack("<14I", *values.values()))
    return pointer


def create_device(startup, flags=0x84, **changes):
    d3d = create_d3d(startup)
    out = startup.alloc(4)
    result = startup.d3d.interface_call(
        16, [d3d, 0, 1, HWND, flags, present_parameters(startup, **changes), out]
    )
    pointer = startup.read_u32(out) if result == 0 else 0
    return result, startup.com.objects.get(pointer)


def call(startup, obj, method, *args):
    """Run a COM model method like the thunk would, returning its result."""
    return getattr(obj.model, method)(obj, list(args))
