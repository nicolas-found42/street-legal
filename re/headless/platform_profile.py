"""Declared hypothetical platform scenario for the boot beyond the window prefix.

One bounded JSON document (`--platform-profile`) holds the scenario inputs the
original needs after window creation: the D3D9 device capabilities and adapter
identity, the answers to format-support queries, registry contents, input
devices, sound hardware and the like. Every field is an explicit declaration;
none is an observation of a physical machine, and results record a profile
hash and `physical_*_observed: false` flags. Sections are validated when the
profile is loaded; unknown fields and duplicate keys are rejected.
"""

from __future__ import annotations

import hashlib
import json
import re
import struct
from pathlib import Path

from dinput_platform import InputProfile
from dsound_platform import SoundProfile

LABEL = re.compile(r"hypothetical-[A-Za-z0-9_.-]{1,67}")
MAX_BYTES = 256 * 1024

# D3DCAPS9 in header order; (name, is_float). Every field is one 32-bit word,
# nested D3DVSHADERCAPS2_0 / D3DPSHADERCAPS2_0 are flattened with a prefix.
D3DCAPS9_FIELDS = tuple(
    (name, name in {"MaxVertexW", "GuardBandLeft", "GuardBandTop", "GuardBandRight",
                    "GuardBandBottom", "ExtentsAdjust", "MaxPointSize",
                    "PixelShader1xMaxValue", "MaxNpatchTessellationLevel"})
    for name in (
        "DeviceType AdapterOrdinal Caps Caps2 Caps3 PresentationIntervals CursorCaps "
        "DevCaps PrimitiveMiscCaps RasterCaps ZCmpCaps SrcBlendCaps DestBlendCaps "
        "AlphaCmpCaps ShadeCaps TextureCaps TextureFilterCaps CubeTextureFilterCaps "
        "VolumeTextureFilterCaps TextureAddressCaps VolumeTextureAddressCaps LineCaps "
        "MaxTextureWidth MaxTextureHeight MaxVolumeExtent MaxTextureRepeat "
        "MaxTextureAspectRatio MaxAnisotropy MaxVertexW GuardBandLeft GuardBandTop "
        "GuardBandRight GuardBandBottom ExtentsAdjust StencilCaps FVFCaps TextureOpCaps "
        "MaxTextureBlendStages MaxSimultaneousTextures VertexProcessingCaps "
        "MaxActiveLights MaxUserClipPlanes MaxVertexBlendMatrices "
        "MaxVertexBlendMatrixIndex MaxPointSize MaxPrimitiveCount MaxVertexIndex "
        "MaxStreams MaxStreamStride VertexShaderVersion MaxVertexShaderConst "
        "PixelShaderVersion PixelShader1xMaxValue DevCaps2 MaxNpatchTessellationLevel "
        "Reserved5 MasterAdapterOrdinal AdapterOrdinalInGroup NumberOfAdaptersInGroup "
        "DeclTypes NumSimultaneousRTs StretchRectFilterCaps VS20Caps.Caps "
        "VS20Caps.DynamicFlowControlDepth VS20Caps.NumTemps "
        "VS20Caps.StaticFlowControlDepth PS20Caps.Caps PS20Caps.DynamicFlowControlDepth "
        "PS20Caps.NumTemps PS20Caps.StaticFlowControlDepth "
        "PS20Caps.NumInstructionSlots VertexTextureFilterCaps "
        "MaxVShaderInstructionsExecuted MaxPShaderInstructionsExecuted "
        "MaxVertexShader30InstructionSlots MaxPixelShader30InstructionSlots"
    ).split()
)
assert len(D3DCAPS9_FIELDS) == 76
D3DCAPS9_SIZE = 4 * len(D3DCAPS9_FIELDS)

IDENTIFIER_SIZE = 1096  # D3DADAPTER_IDENTIFIER9 on i686 (LARGE_INTEGER aligned to 8)


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate platform profile key: {key}")
        result[key] = value
    return result


def _u32(value, what):
    if type(value) is not int or not 0 <= value <= 0xFFFFFFFF:
        raise ValueError(f"{what} must be an unsigned 32-bit integer")
    return value


def _guid_bytes(text):
    if not isinstance(text, str) or not re.fullmatch(
        r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}", text
    ):
        raise ValueError("GUID must be 8-4-4-4-12 hexadecimal text")
    parts = text.split("-")
    return struct.pack(
        "<IHH", int(parts[0], 16), int(parts[1], 16), int(parts[2], 16)
    ) + bytes.fromhex(parts[3] + parts[4])


class D3DProfile:
    """The declared D3D9 hardware: caps, adapter identity and query answers."""

    def __init__(self, data):
        allowed = {
            "caps",
            "adapter_identifier",
            "format_checks",
            "available_texture_mem",
            "multisample_checks",
            "supported_query_types",
        }
        if not isinstance(data, dict) or set(data) - allowed or allowed - set(data):
            raise ValueError("d3d section needs exactly: " + ", ".join(sorted(allowed)))
        caps = data["caps"]
        names = dict(D3DCAPS9_FIELDS)
        if not isinstance(caps, dict) or set(caps) - set(names):
            raise ValueError("unknown D3DCAPS9 field")
        words = []
        for name, is_float in D3DCAPS9_FIELDS:
            value = caps.get(name, 0)
            if is_float:
                if type(value) not in (int, float):
                    raise ValueError(f"{name} must be a number")
                words.append(struct.unpack("<I", struct.pack("<f", float(value)))[0])
            else:
                words.append(_u32(value, name))
        if words[0] != 1 or words[1] != 0:
            raise ValueError("caps must declare DeviceType 1 (HAL) on adapter 0")
        self.caps_words = tuple(words)
        self.caps_blob = struct.pack(f"<{len(words)}I", *words)
        self.unspecified_caps_fields = sorted(set(names) - set(caps))
        ident = data["adapter_identifier"]
        fields = {
            "Driver",
            "Description",
            "DeviceName",
            "DriverVersion",
            "VendorId",
            "DeviceId",
            "SubSysId",
            "Revision",
            "DeviceIdentifier",
            "WHQLLevel",
        }
        if not isinstance(ident, dict) or set(ident) != fields:
            raise ValueError("adapter_identifier needs exactly " + ", ".join(sorted(fields)))
        blob = bytearray(IDENTIFIER_SIZE)
        for key, offset, size in (("Driver", 0, 512), ("Description", 512, 512), ("DeviceName", 1024, 32)):
            text = ident[key]
            if not isinstance(text, str) or not text.isascii() or len(text) >= size:
                raise ValueError(f"{key} must be short ASCII text")
            blob[offset : offset + len(text)] = text.encode("ascii")
        version = ident["DriverVersion"]
        if (
            not isinstance(version, list)
            or len(version) != 2
            or any(type(v) is not int or not 0 <= v <= 0xFFFFFFFF for v in version)
        ):
            raise ValueError("DriverVersion is [low, high] unsigned words")
        blob[1056:1064] = struct.pack("<II", version[0], version[1])
        blob[1064:1080] = struct.pack(
            "<4I",
            _u32(ident["VendorId"], "VendorId"),
            _u32(ident["DeviceId"], "DeviceId"),
            _u32(ident["SubSysId"], "SubSysId"),
            _u32(ident["Revision"], "Revision"),
        )
        blob[1080:1096] = _guid_bytes(ident["DeviceIdentifier"])
        blob[1092:1096] = struct.pack("<I", _u32(ident["WHQLLevel"], "WHQLLevel"))
        self.identifier_blob = bytes(blob)
        checks = data["format_checks"]
        if (
            not isinstance(checks, dict)
            or set(checks) != {"default_result", "overrides"}
            or not isinstance(checks["overrides"], list)
            or len(checks["overrides"]) > 256
        ):
            raise ValueError("format_checks needs default_result and a bounded overrides list")
        self.format_default = self._hresult(checks["default_result"])
        overrides = []
        for rule in checks["overrides"]:
            if (
                not isinstance(rule, dict)
                or "method" not in rule
                or "result" not in rule
                or rule["method"] not in ("CheckDeviceFormat", "CheckDepthStencilMatch")
            ):
                raise ValueError("override needs method and result")
            match = {k: v for k, v in rule.items() if k not in ("method", "result")}
            if set(match) - {
                "adapter_format",
                "usage",
                "resource_type",
                "format",
                "render_target_format",
                "depth_stencil_format",
            } or any(type(v) is not int for v in match.values()):
                raise ValueError("unsupported format-check match field")
            overrides.append((rule["method"], tuple(sorted(match.items())), self._hresult(rule["result"])))
        self.format_overrides = tuple(overrides)
        mem = data["available_texture_mem"]
        if type(mem) is not int or not 1 << 20 <= mem <= 0xFFFFFFFF:
            raise ValueError("available_texture_mem must be a bounded byte count")
        self.available_texture_mem = mem
        queries = data["supported_query_types"]
        if (
            not isinstance(queries, list)
            or len(queries) > 16
            or any(type(q) is not int or not 0 <= q <= 16 for q in queries)
        ):
            raise ValueError("supported_query_types is a bounded list of D3DQUERYTYPE integers")
        self.supported_query_types = frozenset(queries)
        ms = data["multisample_checks"]
        if not isinstance(ms, dict) or set(ms) != {"default_result"}:
            raise ValueError("multisample_checks needs default_result")
        self.multisample_default = self._hresult(ms["default_result"])

    @staticmethod
    def _hresult(value):
        if type(value) is not int or value not in (0, 0x8876086A, 0x8876086C):
            raise ValueError("declared result must be D3D_OK, D3DERR_NOTAVAILABLE or D3DERR_INVALIDCALL")
        return value


class ModulesProfile:
    """Which DLLs are absent from the declared machine and which exports exist."""

    def __init__(self, data):
        if not isinstance(data, dict) or set(data) != {"absent", "exports"}:
            raise ValueError("modules needs exactly absent and exports")
        absent = data["absent"]
        if not isinstance(absent, list) or len(absent) > 32 or any(
            not isinstance(n, str) or not re.fullmatch(r"[a-z0-9_.-]{1,64}\.dll", n)
            for n in absent
        ):
            raise ValueError("absent modules are bounded lowercase DLL names")
        self.absent = frozenset(absent)
        exports = data["exports"]
        if not isinstance(exports, dict) or len(exports) > 16:
            raise ValueError("bounded exports map required")
        result = {}
        for module, names in exports.items():
            if not re.fullmatch(r"[a-z0-9_.-]{1,64}\.dll", module) or not isinstance(
                names, list
            ) or len(names) > 32 or any(
                not isinstance(n, str) or not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]{0,63}", n)
                for n in names
            ):
                raise ValueError("unsupported export declaration")
            result[module] = tuple(names)
        self.exports = result


class RegistryProfile:
    """Existing registry keys and values; every other key is absent."""

    def __init__(self, data):
        if not isinstance(data, dict) or set(data) != {"keys"}:
            raise ValueError("registry needs exactly keys")
        keys = data["keys"]
        if not isinstance(keys, dict) or len(keys) > 64:
            raise ValueError("bounded registry keys required")
        result = {}
        for path, values in keys.items():
            if not isinstance(path, str) or not re.fullmatch(r"(HKLM|HKCU|HKCR|HKU)(\\[^\\]{1,128}){0,8}", path):
                raise ValueError("registry key path must be HKLM\\... style")
            if not isinstance(values, dict) or len(values) > 64:
                raise ValueError("bounded registry values required")
            table = {}
            for name, spec in values.items():
                if not isinstance(spec, dict) or set(spec) != {"type", "data"}:
                    raise ValueError("registry value needs type and data")
                if spec["type"] == "REG_DWORD":
                    payload = struct.pack("<I", _u32(spec["data"], "REG_DWORD"))
                    kind = 4
                elif spec["type"] == "REG_SZ" and isinstance(spec["data"], str) and spec["data"].isascii():
                    payload = spec["data"].encode("ascii") + b"\0"
                    kind = 1
                else:
                    raise ValueError("unsupported registry value")
                table[name.lower()] = (kind, payload)
            result[path.lower()] = table
        self.keys = result


class WindowProfile:
    def __init__(self, data):
        if not isinstance(data, dict) or set(data) != {"message_delivery"}:
            raise ValueError("window needs exactly message_delivery")
        if data["message_delivery"] != "elided":
            raise ValueError("only declared elided window message delivery is modelled")
        self.message_delivery = data["message_delivery"]


class ComProfile:
    """COM classes the declared machine does not register (REGDB_E_CLASSNOTREG)."""

    def __init__(self, data):
        if not isinstance(data, dict) or set(data) != {"unregistered_clsids"}:
            raise ValueError("com needs exactly unregistered_clsids")
        values = data["unregistered_clsids"]
        if not isinstance(values, list) or len(values) > 32:
            raise ValueError("bounded unregistered_clsids list required")
        self.unregistered = frozenset(_guid_bytes(v) for v in values)


class PlatformProfile:
    SECTIONS = {"d3d", "modules", "registry", "window", "input", "sound", "com"}

    def __init__(self, data):
        allowed = {"label", "provenance"} | self.SECTIONS
        if not isinstance(data, dict) or set(data) - allowed:
            raise ValueError("unknown platform profile fields")
        label = data.get("label")
        if not isinstance(label, str) or not LABEL.fullmatch(label):
            raise ValueError("explicit hypothetical platform label required")
        provenance = data.get("provenance", "")
        if not isinstance(provenance, str) or len(provenance) > 2048:
            raise ValueError("provenance must be bounded text")
        self.label = label
        self.d3d = D3DProfile(data["d3d"]) if "d3d" in data else None
        self.modules = ModulesProfile(data["modules"]) if "modules" in data else None
        self.registry = RegistryProfile(data["registry"]) if "registry" in data else None
        self.window = WindowProfile(data["window"]) if "window" in data else None
        self.input = InputProfile(data["input"]) if "input" in data else None
        self.sound = SoundProfile(data["sound"]) if "sound" in data else None
        self.com = ComProfile(data["com"]) if "com" in data else None
        self.digest = hashlib.sha256(
            json.dumps(data, sort_keys=True, separators=(",", ":")).encode()
        ).hexdigest()

    @classmethod
    def load(cls, path):
        with Path(path).open("rb") as stream:
            data = stream.read(MAX_BYTES + 1)
        if len(data) > MAX_BYTES:
            raise ValueError("platform profile exceeds 256 KiB")
        return cls(json.loads(data, object_pairs_hook=unique_object))

    @property
    def metadata(self):
        return {
            "label": self.label,
            "kind": "hypothetical",
            "profile_sha256": self.digest,
            "physical_gpu_observed": False,
            "physical_input_observed": False,
            "physical_sound_observed": False,
            "sections": sorted(
                name for name in self.SECTIONS if getattr(self, name) is not None
            ),
        }
