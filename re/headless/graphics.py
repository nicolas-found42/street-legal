"""Explicit hypothetical IDirect3D9 enumeration inputs, without a device model."""

from __future__ import annotations

import hashlib
import json
import re
from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path
from types import MappingProxyType

D3DERR_NOTAVAILABLE = 0x8876086A
D3DERR_INVALIDCALL = 0x8876086C
COM_SLOTS = (
    ("QueryInterface", 3),
    ("AddRef", 1),
    ("Release", 1),
    ("RegisterSoftwareDevice", 2),
    ("GetAdapterCount", 1),
    ("GetAdapterIdentifier", 4),
    ("GetAdapterModeCount", 3),
    ("EnumAdapterModes", 5),
    ("GetAdapterDisplayMode", 3),
    ("CheckDeviceType", 6),
    ("CheckDeviceFormat", 7),
    ("CheckDeviceMultiSampleType", 7),
    ("CheckDepthStencilMatch", 6),
    ("CheckDeviceFormatConversion", 5),
    ("GetDeviceCaps", 4),
    ("GetAdapterMonitor", 2),
    ("CreateDevice", 7),
)


@dataclass(frozen=True, init=False)
class GraphicsProfile:
    label: str
    sdk_version: int
    creation_result: str
    adapter_count: int
    modes: Mapping[tuple[int, int], tuple[tuple[int, int, int, int], ...]]
    display_modes: Mapping[int, tuple[int, int, int, int]]
    type_checks: Mapping[tuple[int, int, int, int, int], int]
    digest: str

    def __init__(self, data):
        allowed = {
            "label",
            "creation_sdk_version",
            "creation_result",
            "adapter_count",
            "mode_lists",
            "display_modes",
            "type_checks",
            "provenance",
        }
        if not isinstance(data, dict) or set(data) - allowed:
            raise ValueError("unknown graphics profile fields")
        label = data.get("label")
        if not isinstance(label, str) or not re.fullmatch(
            r"hypothetical-[A-Za-z0-9_.-]{1,67}", label
        ):
            raise ValueError("explicit hypothetical graphics label required")
        version = data.get("creation_sdk_version")
        if type(version) is not int or version not in (31, 32):
            raise ValueError("explicit creation SDK version 31 or 32 required")
        result = data.get("creation_result")
        if result not in ("interface", "null"):
            raise ValueError("explicit creation result required")
        count = data.get("adapter_count")
        if type(count) is not int or not 1 <= count <= 4:
            raise ValueError("adapter count must be 1..4")

        def mode_shape(mode, format=None):
            if (
                not isinstance(mode, list)
                or len(mode) != 4
                or any(type(v) is not int for v in mode)
                or not 1 <= mode[0] <= 8192
                or not 1 <= mode[1] <= 8192
                or not 0 <= mode[2] <= 1000
                or mode[3] not in (22, 23, 24)
                or (format is not None and mode[3] != format)
            ):
                raise ValueError("unsupported D3DDISPLAYMODE shape")
            return tuple(mode)

        source = data.get("mode_lists")
        if not isinstance(source, dict) or len(source) > 12:
            raise ValueError("bounded explicit mode lists required")
        modes = {}
        for key, values in source.items():
            if not isinstance(key, str) or not re.fullmatch(r"[0-3]:(22|23|24)", key):
                raise ValueError("unsupported mode-list key")
            adapter, format = map(int, key.split(":"))
            if adapter >= count or not isinstance(values, list) or len(values) > 32:
                raise ValueError("unsupported mode-list bounds")
            modes[(adapter, format)] = tuple(
                mode_shape(value, format) for value in values
            )
        checks = data.get("type_checks", {})
        if not isinstance(checks, dict) or len(checks) > 128:
            raise ValueError("bounded explicit device checks required")
        type_checks = {}
        for key, value in checks.items():
            if not isinstance(key, str) or not re.fullmatch(
                r"[0-3],[1-3],(22|23|24),(21|22|23|24|25),[01]", key
            ):
                raise ValueError("unsupported device-check shape")
            args = tuple(map(int, key.split(",")))
            if (
                args[0] >= count
                or type(value) is not int
                or value not in (0, D3DERR_NOTAVAILABLE, D3DERR_INVALIDCALL)
            ):
                raise ValueError("unsupported declared device-check result")
            if (
                value == 0
                and args[4] == 0
                and args[2] != args[3]
                and (args[2], args[3]) not in ((22, 21), (24, 25))
            ):
                raise ValueError("incompatible fullscreen format success")
            type_checks[args] = value
        source = data.get("display_modes", {})
        if not isinstance(source, dict) or len(source) > 4:
            raise ValueError("bounded display modes required")
        display = {}
        for key, value in source.items():
            if (
                not isinstance(key, str)
                or not re.fullmatch(r"[0-3]", key)
                or int(key) >= count
            ):
                raise ValueError("unsupported display adapter")
            display[int(key)] = mode_shape(value)
        provenance = data.get("provenance", "")
        if not isinstance(provenance, str) or len(provenance) > 1024:
            raise ValueError("provenance must be bounded text")
        digest = hashlib.sha256(
            json.dumps(data, sort_keys=True, separators=(",", ":")).encode()
        ).hexdigest()
        for name, value in {
            "label": label,
            "sdk_version": version,
            "creation_result": result,
            "adapter_count": count,
            "modes": MappingProxyType(modes),
            "display_modes": MappingProxyType(display),
            "type_checks": MappingProxyType(type_checks),
            "digest": digest,
        }.items():
            object.__setattr__(self, name, value)

    @classmethod
    def load(cls, path):
        with Path(path).open("rb") as stream:
            data = stream.read(65537)
        if len(data) > 65536:
            raise ValueError("graphics profile exceeds 64 KiB")

        def unique_object(pairs):
            result = {}
            for key, value in pairs:
                if key in result:
                    raise ValueError(f"duplicate graphics profile key: {key}")
                result[key] = value
            return result

        return cls(json.loads(data, object_pairs_hook=unique_object))

    @property
    def metadata(self):
        return {
            "label": self.label,
            "kind": "hypothetical",
            "profile_sha256": self.digest,
            "physical_gpu_observed": False,
            "declared_creation_sdk_version": self.sdk_version,
            "creation_result_assumption": self.creation_result,
            "abi_reference_sdk_macro": 32,
            "device_created": False,
        }


class Direct3D9:
    """An enumerator interface only. Unknown slots stop before returning a value."""

    def __init__(self, profile, host, boundary, allocate_method):
        self.profile = profile
        self.host = host
        self.boundary = boundary
        self.allocate_method = allocate_method
        self.methods = None
        self.objects = set()
        self.creation_versions = []
        self.extension = None
        self.last_call = {}

    def create(self, args):
        self.creation_versions.append(args[0])
        if args != [self.profile.sdk_version]:
            raise self.boundary(
                "undeclared_d3d_creation_version",
                args=args,
                declared=self.profile.sdk_version,
            )
        if self.profile.creation_result == "null":
            return 0
        if len(self.objects) >= 4:
            raise self.boundary("d3d_interface_budget", limit=4)
        if self.methods is None:
            self.methods = tuple(
                self.allocate_method(slot, name, nargs)
                for slot, (name, nargs) in enumerate(COM_SLOTS)
            )
        table = self.host.alloc(17 * 4)
        pointer = self.host.alloc(4)
        self.host.write_u32s(table, self.methods)
        self.host.write_u32(pointer, table)
        self.objects.add(pointer)
        return pointer

    def call(self, slot, args):
        self.last_call = {"profile": self.profile.label, "output_write": None}
        if args[0] not in self.objects:
            raise self.boundary(
                "invalid_com_this", interface="IDirect3D9", this=args[0]
            )
        values = args[1:]
        if slot == 4:
            return self.profile.adapter_count
        if slot == 6:
            adapter, format = values
            if adapter >= self.profile.adapter_count:
                return 0
            return len(self.mode_list(adapter, format))
        if slot == 7:
            adapter, format, index, output = values
            if adapter >= self.profile.adapter_count or not output:
                return D3DERR_INVALIDCALL
            modes = self.mode_list(adapter, format)
            if index >= len(modes):
                return D3DERR_INVALIDCALL
            return self.output_mode(output, modes[index])
        if slot == 8:
            adapter, output = values
            if adapter >= self.profile.adapter_count or not output:
                return D3DERR_INVALIDCALL
            if adapter not in self.profile.display_modes:
                raise self.boundary("undeclared_display_mode", adapter=adapter)
            return self.output_mode(output, self.profile.display_modes[adapter])
        if slot == 9:
            adapter, device, *_ = values
            if adapter >= self.profile.adapter_count or device not in (1, 2, 3):
                return D3DERR_INVALIDCALL
            key = tuple(values)
            if key not in self.profile.type_checks:
                raise self.boundary("undeclared_device_type_check", args=values)
            return self.profile.type_checks[key]
        if self.extension is not None and slot in (1, 2, 5, 10, 11, 12, 14, 16):
            self.extension.last_call = self.last_call
            return self.extension.interface_call(slot, args)
        raise self.boundary(
            "unsupported_com_method",
            interface="IDirect3D9",
            slot=slot,
            method=COM_SLOTS[slot][0],
            args=args,
            method_result_supplied=False,
        )

    def mode_list(self, adapter, format):
        key = (adapter, format)
        if key not in self.profile.modes:
            raise self.boundary("undeclared_mode_list", adapter=adapter, format=format)
        return self.profile.modes[key]

    def output_mode(self, pointer, mode):
        self.host.write_u32s(pointer, mode)
        self.last_call["output_write"] = {"address": pointer, "words": list(mode)}
        return 0
