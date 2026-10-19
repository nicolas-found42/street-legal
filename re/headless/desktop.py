"""Bounded hypothetical desktop state; PE resources are read-only inputs."""

from __future__ import annotations

import hashlib
import json
import re
import struct
from dataclasses import dataclass
from pathlib import Path

from window_messages import CreationMessages, CreationProfile


@dataclass(frozen=True, init=False)
class DesktopProfile:
    label: str
    resource_language: int
    cursor_size: tuple[int, int]
    icon_size: tuple[int, int]
    system_arrow_available: bool
    black_brush_available: bool
    digest: str
    creation: CreationProfile | None

    def __init__(self, data):
        allowed = {
            "label",
            "resource_language",
            "cursor_size",
            "icon_size",
            "system_arrow_available",
            "black_brush_available",
            "provenance",
            "creation",
        }
        if not isinstance(data, dict) or set(data) - allowed:
            raise ValueError("unknown desktop profile fields")
        label = data.get("label")
        if not isinstance(label, str) or not re.fullmatch(
            r"hypothetical-[A-Za-z0-9_.-]{1,67}", label
        ):
            raise ValueError("explicit hypothetical desktop label required")
        language = data.get("resource_language")
        if type(language) is not int or not 0 <= language <= 65535:
            raise ValueError("bounded explicit resource language required")
        values = {"label": label, "resource_language": language}
        for key in ("cursor_size", "icon_size"):
            size = data.get(key)
            if (
                not isinstance(size, list)
                or len(size) != 2
                or any(type(v) is not int or not 1 <= v <= 256 for v in size)
            ):
                raise ValueError("bounded explicit image sizes required")
            values[key] = tuple(size)
        for key in ("system_arrow_available", "black_brush_available"):
            if type(data.get(key)) is not bool:
                raise ValueError("explicit predefined-object availability required")
            values[key] = data[key]
        provenance = data.get("provenance", "")
        if not isinstance(provenance, str) or len(provenance) > 1024:
            raise ValueError("provenance must be bounded text")
        values["creation"] = (
            CreationProfile(data["creation"]) if "creation" in data else None
        )
        values["digest"] = hashlib.sha256(
            json.dumps(data, sort_keys=True, separators=(",", ":")).encode()
        ).hexdigest()
        for key, value in values.items():
            object.__setattr__(self, key, value)

    @classmethod
    def load(cls, path):
        with Path(path).open("rb") as stream:
            data = stream.read(8193)
        if len(data) > 8192:
            raise ValueError("desktop profile exceeds 8 KiB")

        def unique_object(pairs):
            result = {}
            for key, value in pairs:
                if key in result:
                    raise ValueError(f"duplicate desktop profile key: {key}")
                result[key] = value
            return result

        return cls(json.loads(data, object_pairs_hook=unique_object))

    @property
    def metadata(self):
        result = {
            "label": self.label,
            "kind": "hypothetical",
            "profile_sha256": self.digest,
            "physical_desktop_observed": False,
            "resource_language": self.resource_language,
            "cursor_size": list(self.cursor_size),
            "icon_size": list(self.icon_size),
            "system_arrow_available": self.system_arrow_available,
            "black_brush_available": self.black_brush_available,
        }
        if self.creation is not None:
            result["creation"] = self.creation.metadata
            if self.creation.wm_showwindow is not None:
                result.update(
                    windows_notification_sequence_established=False,
                    unlisted_notifications_modeled=False,
                )
        return result


def decode_image(group, payload, kind, size):
    """Validate one unscaled BI_RGB group entry; return identity, not pixels."""
    if kind not in ("cursor", "icon") or len(group) != 20:
        raise ValueError("only single-entry image groups supported")
    reserved, type, count = struct.unpack_from("<HHH", group)
    if reserved or type != (2 if kind == "cursor" else 1) or count != 1:
        raise ValueError("unsupported image group header")
    if kind == "cursor":
        width, height, planes, bits, declared, image_id = struct.unpack_from(
            "<HHHHIH", group, 6
        )
        if height % 2:
            raise ValueError("odd cursor group height")
        height //= 2
    else:
        width, height, colors, reserved, planes, bits, declared, image_id = (
            struct.unpack_from("<BBBBHHIH", group, 6)
        )
        width = width or 256
        height = height or 256
        if reserved or colors not in (0, 1 << bits if bits <= 8 else 0):
            raise ValueError("unsupported icon group entry")
    offset = 4 if kind == "cursor" else 0
    if len(payload) != declared or len(payload) < offset + 40:
        raise ValueError("image payload size mismatch")
    header, w, h, p, b, compression, image_bytes, _, _, used, _ = struct.unpack_from(
        "<IiiHHIIiiII", payload, offset
    )
    if (
        header != 40
        or p != 1
        or planes != 1
        or compression
        or b != bits
        or b not in (1, 4, 8, 24, 32)
        or w != width
        or h != height * 2
        or (width, height) != size
    ):
        raise ValueError("unsupported image layout or scaling")
    palette = (used or 1 << b) * 4 if b <= 8 else 0
    if used and (b > 8 or used > 1 << b):
        raise ValueError("unsupported image palette")
    pixels = (((w * b + 31) // 32) * 4 + ((w + 31) // 32) * 4) * height
    if image_bytes not in (0, pixels) or len(payload) != offset + 40 + palette + pixels:
        raise ValueError("truncated or unsupported image bitmap")
    hotspot = list(struct.unpack_from("<HH", payload)) if offset else None
    if hotspot is not None and (hotspot[0] >= width or hotspot[1] >= height):
        raise ValueError("invalid cursor hotspot")
    return {
        "image_id": image_id,
        "width": width,
        "height": height,
        "bits_per_pixel": bits,
        "hotspot": hotspot,
        "group_sha256": hashlib.sha256(group).hexdigest(),
        "payload_sha256": hashlib.sha256(payload).hexdigest(),
    }


@dataclass(frozen=True)
class WindowCallback:
    target: int
    args: tuple[int, int, int, int]
    hwnd: int


class Desktop:
    """Win32 object state under a declared scenario; no display backend."""

    def __init__(self, profile, host, boundary):
        self.profile = profile
        self.host = host
        self.boundary = boundary
        self.handles = {}
        self.last_call = {}
        self.resources = None
        self.classes = {}
        self.window_attempts = 0
        self.messages = (
            CreationMessages(profile.creation, host, boundary, WindowCallback)
            if profile.creation is not None
            else None
        )

    def shared(self, key):
        if key not in self.handles:
            if len(self.handles) >= 64:
                raise self.boundary("desktop_object_limit", limit=64)
            self.handles[key] = 0x41000000 + len(self.handles) * 16
        return self.handles[key]

    def cursor(self, args):
        self.last_call = {
            "source": "predefined",
            "kind": "cursor",
            "identifier": args[1],
            "output_write": None,
        }
        if args[0]:
            return self.image(args, "cursor")
        if args != [0, 32512]:
            raise self.boundary("unsupported_cursor_lookup", args=args)
        if not self.profile.system_arrow_available:
            return 0
        return self.shared(("system", "arrow"))

    def stock_object(self, args):
        self.last_call = {
            "source": "stock",
            "kind": "black_brush",
            "output_write": None,
        }
        if args != [4]:
            raise self.boundary("unsupported_stock_object", args=args)
        if not self.profile.black_brush_available:
            return 0
        return self.shared(("stock", "black_brush"))

    def icon(self, args):
        return self.image(args, "icon")

    def resource_index(self):
        if self.resources is not None:
            return self.resources
        result = {}
        total = 0
        for kind in self.host.pe.DIRECTORY_ENTRY_RESOURCE.entries:
            for name in kind.directory.entries:
                for language in name.directory.entries:
                    if kind.id is None or name.id is None or language.id is None:
                        raise self.boundary("unsupported_named_pe_resource")
                    key = (kind.id, name.id, language.id)
                    size = language.data.struct.Size
                    total += size
                    if len(result) >= 512 or total > 1048576 or key in result:
                        raise self.boundary("pe_resource_budget_or_duplicate")
                    data = self.host.pe.get_data(
                        language.data.struct.OffsetToData, size
                    )
                    if len(data) != size:
                        raise self.boundary("truncated_pe_resource")
                    result[key] = data
        self.resources = result
        return result

    def image(self, args, kind):
        module, identifier = args
        self.last_call = {
            "source": "pe",
            "kind": kind,
            "identifier": identifier,
            "output_write": None,
        }
        if module != self.host.base or identifier > 65535:
            raise self.boundary("unsupported_image_lookup", image_kind=kind, args=args)
        group_type, payload_type = (12, 1) if kind == "cursor" else (14, 3)
        resources = self.resource_index()
        language = self.profile.resource_language
        if (group_type, identifier, language) not in resources and (
            group_type,
            identifier,
            0,
        ) in resources:
            language = 0
        key = (group_type, identifier, language)
        self.last_call["language"] = language
        if key not in resources:
            self.host.last_error = 1814
            self.last_call["lookup"] = "missing"
            return 0
        group = resources[key]
        if len(group) != 20:
            raise self.boundary(
                "unsupported_image_group", image_kind=kind, identifier=identifier
            )
        image_id = struct.unpack_from("<H", group, 18)[0]
        payload = resources.get((payload_type, image_id, language))
        if payload is None:
            raise self.boundary(
                "missing_image_payload",
                image_kind=kind,
                identifier=identifier,
                image_id=image_id,
            )
        size = self.profile.cursor_size if kind == "cursor" else self.profile.icon_size
        try:
            info = decode_image(group, payload, kind, size)
        except ValueError as error:
            raise self.boundary(
                "unsupported_pe_image",
                image_kind=kind,
                identifier=identifier,
                message=str(error),
            ) from error
        self.last_call.update(info)
        return self.shared(("pe", group_type, identifier, language))

    def class_name(self, pointer):
        if pointer <= 65535:
            raise self.boundary("unsupported_class_name", pointer=pointer)
        name = self.host.read_cstr(pointer, 129).decode("ascii")
        if not 1 <= len(name) <= 128 or any(not 32 <= ord(c) <= 126 for c in name):
            raise self.boundary("unsupported_class_name", name=name)
        return name

    def register_class(self, args):
        words = self.host.read_u32s(args[0], 10)
        (
            style,
            procedure,
            cls_extra,
            wnd_extra,
            instance,
            icon,
            cursor,
            brush,
            menu,
            name,
        ) = words
        self.last_call = {
            "stage": "register_class",
            "attributes": words.copy(),
            "output_write": None,
        }
        if (
            style
            or cls_extra
            or wnd_extra
            or instance != self.host.base
            or not procedure
        ):
            raise self.boundary("unsupported_window_class", attributes=words)
        self.host.read(procedure, 1)

        def owns(handle, role):
            return not handle or any(
                value == handle and role(key) for key, value in self.handles.items()
            )

        if (
            not owns(icon, lambda key: key[0] == "pe" and key[1] == 14)
            or not owns(
                cursor,
                lambda key: (
                    key == ("system", "arrow") or key[0] == "pe" and key[1] == 12
                ),
            )
            or not owns(brush, lambda key: key == ("stock", "black_brush"))
        ):
            raise self.boundary("unknown_class_object_handle", attributes=words)
        if menu and (menu <= 65535 or self.host.read_cstr(menu, 129)):
            raise self.boundary("unsupported_class_menu", menu=menu)
        text = self.class_name(name)
        key = (instance, text.lower())
        self.last_call.update(class_name=text, wndproc=procedure)
        if key in self.classes:
            self.host.last_error = 1410
            return 0
        if len(self.classes) >= 16:
            raise self.boundary("window_class_budget", limit=16)
        atom = 0xC000 + len(self.classes)
        self.classes[key] = {
            "atom": atom,
            "name": text,
            "wndproc": procedure,
            "attributes": tuple(words),
        }
        return atom

    def create_window(self, args):
        (
            extended,
            name,
            title,
            style,
            x,
            y,
            width,
            height,
            parent,
            menu,
            instance,
            parameter,
        ) = args
        self.last_call = {"stage": "create_window", "output_write": None}
        signed = lambda value: value if value < 0x80000000 else value - 0x100000000
        if (
            extended not in (0, 8)
            or style not in (0x80000000, 0x90000000)
            or parent
            or menu
            or instance != self.host.base
            or abs(signed(x)) > 8192
            or abs(signed(y)) > 8192
            or width > 8192
            or height > 8192
        ):
            raise self.boundary("unsupported_popup_shape", args=args)
        if name <= 65535:
            entry = next(
                (
                    v
                    for (module, _), v in self.classes.items()
                    if module == instance and v["atom"] == name
                ),
                None,
            )
        else:
            entry = self.classes.get((instance, self.class_name(name).lower()))
        if entry is None:
            self.host.last_error = 1407
            return 0
        text = self.host.read_cstr(title, 513).decode("cp1252") if title else ""
        if len(text) > 512:
            raise self.boundary("window_title_limit", limit=512)
        if self.window_attempts >= 16:
            raise self.boundary("window_creation_budget", limit=16)
        self.window_attempts += 1
        handle = self.shared(("window", self.window_attempts))
        words = [
            parameter,
            instance,
            menu,
            parent,
            height,
            width,
            y,
            x,
            style,
            title,
            name,
            extended,
        ]
        parameters = self.host.alloc(48)
        self.host.write_u32s(parameters, words)
        self.host.windows[handle] = {
            "class": entry["name"],
            "title": text,
            "instance": instance,
            "creation": "pending_WM_NCCREATE",
        }
        self.last_call.update(
            class_name=entry["name"],
            title=text,
            hwnd=handle,
            create_struct=parameters,
            create_struct_words=words,
        )
        if self.messages is not None:
            self.messages.watch_creation(handle, entry["wndproc"], parameters, words)
        return WindowCallback(entry["wndproc"], (handle, 0x81, 0, parameters), handle)

    def default_window_proc(self, args):
        try:
            return self.messages.default_proc(args)
        finally:
            self.last_call = self.messages.last_call

    def finish_callback(self, request, result):
        if self.messages is not None:
            return self.messages.after_callback(request, result)
        phase = (
            "after_WM_NCCREATE_acceptance" if result else "after_WM_NCCREATE_refusal"
        )
        self.host.windows[request.hwnd]["creation"] = phase
        raise self.boundary(
            "unsupported_window_creation_phase",
            phase=phase,
            hwnd=request.hwnd,
            callback_result=result,
            win32_result_supplied=False,
        )
