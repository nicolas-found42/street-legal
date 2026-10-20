"""Write the declared hypothetical post-window platform profile (ignored output).

Every number here is a declaration of a hypothetical DX9 shader-model-2 class
HAL device and desktop; none is observed from hardware. Capability bit names
follow the MinGW-w64 `d3d9caps.h`. Re-run to regenerate the JSON that
`startup.py --platform-profile` consumes:

    ~/slr-oracle-venv/bin/python re/headless/make_boot_profile.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
DEFAULT_OUT = REPO / "re/out/headless/boot/platform-profile.json"


def bits(*values):
    result = 0
    for value in values:
        result |= value
    return result


CAPS = {
    "DeviceType": 1,
    "AdapterOrdinal": 0,
    "Caps": 0x00020000,  # D3DCAPS_READ_SCANLINE
    "Caps2": bits(0x40000000, 0x20000000, 0x10000000, 0x00020000),
    "Caps3": bits(0x20, 0x80, 0x100, 0x200),
    "PresentationIntervals": bits(0x80000000, 1, 2, 4, 8),
    "CursorCaps": 3,
    "DevCaps": bits(0x10, 0x20, 0x40, 0x80, 0x100, 0x200, 0x400, 0x800, 0x1000, 0x2000,
                    0x8000, 0x10000, 0x20000, 0x80000, 0x100000),
    "PrimitiveMiscCaps": bits(0x2, 0x10, 0x20, 0x40, 0x80, 0x100, 0x200, 0x400, 0x800,
                              0x4000, 0x8000, 0x10000, 0x20000),
    "RasterCaps": bits(0x1, 0x10, 0x80, 0x100, 0x2000, 0x8000, 0x10000, 0x20000, 0x100000,
                       0x200000, 0x400000, 0x1000000, 0x2000000, 0x4000000, 0x8000000),
    "ZCmpCaps": 0xFF,
    "SrcBlendCaps": 0x3FFF,
    "DestBlendCaps": 0x3FFF,
    "AlphaCmpCaps": 0xFF,
    "ShadeCaps": bits(0x8, 0x200, 0x4000, 0x80000),
    "TextureCaps": bits(0x1, 0x2, 0x4, 0x100, 0x400, 0x800, 0x2000, 0x4000, 0x8000, 0x10000),
    "TextureFilterCaps": 0x07030700,
    "CubeTextureFilterCaps": 0x07030700,
    "VolumeTextureFilterCaps": 0x07030700,
    "TextureAddressCaps": 0x3F,
    "VolumeTextureAddressCaps": 0x3F,
    "LineCaps": 0x1F,
    "MaxTextureWidth": 4096,
    "MaxTextureHeight": 4096,
    "MaxVolumeExtent": 256,
    "MaxTextureRepeat": 8192,
    "MaxTextureAspectRatio": 4096,
    "MaxAnisotropy": 16,
    "MaxVertexW": 1e10,
    "GuardBandLeft": -8192.0,
    "GuardBandTop": -8192.0,
    "GuardBandRight": 8192.0,
    "GuardBandBottom": 8192.0,
    "ExtentsAdjust": 0.0,
    "StencilCaps": 0x1FF,
    "FVFCaps": bits(8, 0x80000, 0x100000),
    "TextureOpCaps": 0x03FFFFFF,
    "MaxTextureBlendStages": 8,
    "MaxSimultaneousTextures": 8,
    "VertexProcessingCaps": 0x37B,
    "MaxActiveLights": 8,
    "MaxUserClipPlanes": 6,
    "MaxVertexBlendMatrices": 4,
    "MaxVertexBlendMatrixIndex": 0,
    "MaxPointSize": 8192.0,
    "MaxPrimitiveCount": 0xFFFFF,
    "MaxVertexIndex": 0xFFFFF,
    "MaxStreams": 16,
    "MaxStreamStride": 255,
    "VertexShaderVersion": 0xFFFE0200,
    "MaxVertexShaderConst": 256,
    "PixelShaderVersion": 0xFFFF0200,
    "PixelShader1xMaxValue": 1.0,
    "DevCaps2": 0x3,
    "MaxNpatchTessellationLevel": 0.0,
    "MasterAdapterOrdinal": 0,
    "AdapterOrdinalInGroup": 0,
    "NumberOfAdaptersInGroup": 1,
    "DeclTypes": 0x3FF,
    "NumSimultaneousRTs": 4,
    "StretchRectFilterCaps": 0x03000300,
    "VS20Caps.Caps": 1,
    "VS20Caps.DynamicFlowControlDepth": 24,
    "VS20Caps.NumTemps": 32,
    "VS20Caps.StaticFlowControlDepth": 4,
    "PS20Caps.Caps": 0x1F,
    "PS20Caps.DynamicFlowControlDepth": 24,
    "PS20Caps.NumTemps": 32,
    "PS20Caps.StaticFlowControlDepth": 4,
    "PS20Caps.NumInstructionSlots": 512,
    "VertexTextureFilterCaps": 0,
    "MaxVShaderInstructionsExecuted": 65535,
    "MaxPShaderInstructionsExecuted": 65535,
    "MaxVertexShader30InstructionSlots": 0,
    "MaxPixelShader30InstructionSlots": 0,
}

PROFILE = {
    "label": "hypothetical-sm2-hal-desktop-v1",
    "provenance": (
        "Declared hypothetical shader-model-2 class HAL device, null desktop, no input "
        "devices unless a section says so. Not an observation of any machine."
    ),
    "modules": {
        # retail machine: the Direct3D debug runtime is not installed
        "absent": ["d3d9d.dll"],
        "exports": {"d3d9.dll": ["DebugSetMute"]},
    },
    "registry": {"keys": {"HKLM\\Software\\Microsoft\\Direct3D": {}}},
    "window": {"message_delivery": "elided", "message_queue": "empty"},
    "input": {
        # DIDC_ATTACHED; DI8DEVTYPE_KEYBOARD|PCENH and DI8DEVTYPE_MOUSE|TRADITIONAL
        "keyboard": {"flags": 1, "devtype": 0x403, "axes": 0, "buttons": 128, "povs": 0},
        "mouse": {"flags": 1, "devtype": 0x102, "axes": 3, "buttons": 4, "povs": 0},
        "game_controllers": [],
        "keyboard_layout": 0x04090409,  # declared US English layout handle
        "cursor_position": [512, 384],  # declared pointer position (screen centre)
    },
    # No DirectShow runtime: CLSID_FilterGraph is not registered, so the original's
    # own FMV code takes its CoCreateInstance-failed path (no intro videos).
    "com": {"unregistered_clsids": ["e436ebb3-524f-11ce-9f53-0020af0ba770"]},
    "timer": {"period_min_ms": 1, "period_max_ms": 1000000},
    "sound": {
        # Software-only (emulated) driver: no hardware mixing or 3D buffers.
        "caps": {
            "dwFlags": 0xF3F, "dwMinSecondarySampleRate": 100,
            "dwMaxSecondarySampleRate": 200000, "dwPrimaryBuffers": 1,
            "dwMaxHwMixingAllBuffers": 0, "dwMaxHwMixingStaticBuffers": 0,
            "dwMaxHwMixingStreamingBuffers": 0, "dwFreeHwMixingAllBuffers": 0,
            "dwFreeHwMixingStaticBuffers": 0, "dwFreeHwMixingStreamingBuffers": 0,
            "dwMaxHw3DAllBuffers": 0, "dwMaxHw3DStaticBuffers": 0,
            "dwMaxHw3DStreamingBuffers": 0, "dwFreeHw3DAllBuffers": 0,
            "dwFreeHw3DStaticBuffers": 0, "dwFreeHw3DStreamingBuffers": 0,
            "dwTotalHwMemBytes": 0, "dwFreeHwMemBytes": 0,
            "dwMaxContigFreeHwMemBytes": 0, "dwUnlockTransferRateHwBuffers": 0,
            "dwPlayCpuOverheadSwBuffers": 0, "dwReserved1": 0, "dwReserved2": 0,
        },
        "write_lead_ms": 10,
    },
    "d3d": {
        "caps": CAPS,
        "adapter_identifier": {
            "Driver": "hypothetical_d3d9.dll",
            "Description": "Hypothetical Direct3D9 HAL adapter",
            "DeviceName": "\\\\.\\DISPLAY1",
            "DriverVersion": [1, 1],
            "VendorId": 0,
            "DeviceId": 0,
            "SubSysId": 0,
            "Revision": 0,
            "DeviceIdentifier": "00000000-0000-4000-8000-000000000001",
            "WHQLLevel": 0,
        },
        "format_checks": {"default_result": 0, "overrides": []},
        "available_texture_mem": 256 * 1024 * 1024,
        "multisample_checks": {"default_result": 0},
        # no query type is declared available (D3DERR_NOTAVAILABLE for all)
        "supported_query_types": [],
    },
}


# IDirect3D9 enumeration for the same hypothetical device: three modes per format,
# 60 Hz only, 800x600 included because the stock configuration requests it.
MODES = [(640, 480), (800, 600), (1024, 768)]
GRAPHICS = {
    "label": "hypothetical-sm2-hal-modes-640-800-1024",
    "creation_sdk_version": 31,
    "creation_result": "interface",
    "adapter_count": 1,
    "mode_lists": {
        f"0:{fmt}": [[w, h, 60, fmt] for w, h in MODES] for fmt in (22, 23, 24)
    },
    "type_checks": {
        "0,1,24,24,0": 0,
        "0,1,23,23,0": 0,
        "0,1,22,22,0": 0,
        "0,1,22,21,0": 0,
    },
    "display_modes": {"0": [1024, 768, 60, 22]},
    "provenance": "Declared hypothetical modes; not enumerated from hardware.",
}


def main():
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_OUT
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(PROFILE, indent=2, sort_keys=True) + "\n")
    graphics = out.with_name("graphics-profile.json")
    graphics.write_text(json.dumps(GRAPHICS, indent=2, sort_keys=True) + "\n")
    print(out)
    print(graphics)


if __name__ == "__main__":
    main()
