"""Explicit hypothetical CD-audio scenarios, never physical-disc observations."""

from __future__ import annotations

import hashlib
import json
import re
from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path
from types import MappingProxyType


@dataclass(frozen=True, init=False)
class MediaProfile:
    """Validated immutable scenario inputs; native-derived vectors stay external."""

    label: str
    drive: str
    tracks: tuple[int, ...]
    total: int
    failures: Mapping[str, int]
    digest: str

    def __init__(self, data):
        allowed = {
            "label",
            "drive",
            "track_lengths_ms",
            "total_ms",
            "failures",
            "provenance",
        }
        if not isinstance(data, dict) or set(data) - allowed:
            raise ValueError("unknown media profile fields")
        label = data.get("label")
        if not isinstance(label, str) or not re.fullmatch(
            r"hypothetical-[A-Za-z0-9_.-]{1,67}", label
        ):
            raise ValueError(
                "explicit hypothetical label required (at most 80 ASCII characters)"
            )
        drive = data.get("drive")
        if (
            not isinstance(drive, str)
            or len(drive) != 1
            or not "A" <= drive <= "Z"
            or drive == "C"
        ):
            raise ValueError("one non-C scenario drive required")
        tracks = data.get("track_lengths_ms")
        if (
            not isinstance(tracks, list)
            or not 1 <= len(tracks) <= 128
            or any(type(v) is not int or not 0 < v <= 0x7FFFFFFF for v in tracks)
        ):
            raise ValueError("1..128 bounded positive integer track lengths required")
        total = data.get("total_ms")
        if type(total) is not int or not sum(tracks) <= total <= 0x7FFFFFFF:
            raise ValueError("coherent bounded total duration required")
        failures = data.get("failures", {})
        stages = {"open", "set", "status:count", "status:total", "close"} | {
            f"status:track:{t}" for t in range(1, len(tracks) + 1)
        }
        if (
            not isinstance(failures, dict)
            or len(failures) > 133
            or any(
                k not in stages or type(v) is not int or not 256 <= v <= 1023
                for k, v in failures.items()
            )
        ):
            raise ValueError("unsupported explicit API failure scenario")
        provenance = data.get("provenance", "")
        if not isinstance(provenance, str) or len(provenance) > 1024:
            raise ValueError("provenance must be bounded text")
        digest = hashlib.sha256(
            json.dumps(data, sort_keys=True, separators=(",", ":")).encode()
        ).hexdigest()
        for name, value in {
            "label": label,
            "drive": drive,
            "tracks": tuple(tracks),
            "total": total,
            "failures": MappingProxyType(dict(failures)),
            "digest": digest,
        }.items():
            object.__setattr__(self, name, value)

    @classmethod
    def load(cls, path):
        with Path(path).open("rb") as stream:
            data = stream.read(16385)
        if len(data) > 16384:
            raise ValueError("media profile exceeds 16 KiB")

        def unique_object(pairs):
            result = {}
            for key, value in pairs:
                if key in result:
                    raise ValueError(f"duplicate media profile key: {key}")
                result[key] = value
            return result

        return cls(json.loads(data, object_pairs_hook=unique_object))

    @property
    def metadata(self):
        return {
            "label": self.label,
            "profile_sha256": self.digest,
            "kind": "hypothetical",
            "physical_media_observed": False,
        }


class MciDevice:
    """WinMM 32-bit stdcall boundary over an explicit CD-audio scenario.

    host supplies read_u32s/read_cstr/write_u32. Every successful output write
    is recorded separately from configured values; API failures write nothing.
    """

    def __init__(self, profile, host, boundary):
        self.profile = profile
        self.host = host
        self.boundary = boundary
        self.devices = {}
        self.next_id = 1
        self.last_call = {}

    def record(self, stage, **facts):
        self.last_call = {
            "stage": stage,
            "profile": self.profile.label,
            "output_write": None,
            **facts,
        }

    def send_command(self, args):
        device, command, flags, parameters = args
        self.record("command", device=device, command=command, flags=flags)
        if command == 0x803:
            if device or flags != 0x3302:
                raise self.boundary("unsupported_mci_open", args=args)
            callback, _, kind, element, alias = self.host.read_u32s(parameters, 5)
            drive = self.host.read_cstr(element, 260).decode("cp1252")
            if (
                callback
                or kind != 0x204
                or drive.upper() != self.profile.drive + ":"
                or alias
            ):
                raise self.boundary(
                    "unsupported_mci_open_parameters",
                    drive=drive,
                    device_type=kind,
                    alias=alias,
                    callback=callback,
                )
            self.record("open", drive=drive)
            if "open" in self.profile.failures:
                return self.profile.failures["open"]
            if len(self.devices) >= 16:
                raise self.boundary("mci_device_limit", limit=16)
            result = self.next_id
            self.next_id += 1
            self.devices[result] = {"time_format": 2}
            self.host.write_u32(parameters + 4, result)
            self.record(
                "open",
                drive=drive,
                output_write={"address": parameters + 4, "value": result},
            )
            return 0
        if command not in (0x80D, 0x814, 0x804):
            raise self.boundary("unsupported_mci_command", command=command)
        if device not in self.devices:
            return 257
        state = self.devices[device]
        if command == 0x80D:
            if flags != 0x400:
                raise self.boundary("unsupported_mci_set_flags", flags=flags)
            callback, time_format, audio = self.host.read_u32s(parameters, 3)
            if callback or time_format != 0 or audio:
                raise self.boundary(
                    "unsupported_mci_set_parameters",
                    args=[callback, time_format, audio],
                )
            self.record("set", device=device, time_format=time_format)
            if "set" in self.profile.failures:
                return self.profile.failures["set"]
            state["time_format"] = time_format
            return 0
        if command == 0x804:
            if flags or parameters:
                raise self.boundary("unsupported_mci_close", args=args)
            self.record("close", device=device)
            if "close" in self.profile.failures:
                return self.profile.failures["close"]
            del self.devices[device]
            return 0
        if flags not in (0x100, 0x110):
            raise self.boundary("unsupported_mci_status_flags", flags=flags)
        callback, _, item, track = self.host.read_u32s(parameters, 4)
        if callback:
            raise self.boundary("unsupported_mci_callback", callback=callback)
        if flags == 0x100 and item == 3:
            stage = "status:count"
            result = len(self.profile.tracks)
        elif item == 1 and flags == 0x110:
            if not 1 <= track <= len(self.profile.tracks):
                return 282
            stage = f"status:track:{track}"
            result = self.profile.tracks[track - 1]
        elif item == 1 and flags == 0x100:
            stage = "status:total"
            result = self.profile.total
        else:
            raise self.boundary("unsupported_mci_status_item", item=item, flags=flags)
        if state["time_format"] != 0:
            raise self.boundary(
                "unsupported_mci_time_format", format=state["time_format"]
            )
        facts = {
            "device": device,
            "item": item,
            "track": track if flags & 0x10 else None,
            "profile_value": result,
        }
        self.record(stage, **facts)
        if stage in self.profile.failures:
            return self.profile.failures[stage]
        self.host.write_u32(parameters + 4, result)
        self.record(
            stage, **facts, output_write={"address": parameters + 4, "value": result}
        )
        return 0
