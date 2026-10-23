"""Explicit hypothetical window-message continuation, with no generic defaults."""

from __future__ import annotations

from dataclasses import dataclass

CREATION_MESSAGE_NAMES = {
    0x81: "WM_NCCREATE",
    0x83: "WM_NCCALCSIZE",
    1: "WM_CREATE",
    0x18: "WM_SHOWWINDOW",
}


@dataclass(frozen=True, init=False)
class CalculationProfile:
    rect_output: str
    default_result: int
    after: str

    def __init__(self, data):
        if not isinstance(data, dict) or set(data) != {
            "rect_output",
            "default_result",
            "after",
        }:
            raise ValueError("explicit NCCALCSIZE fields required")
        if (
            data["rect_output"] != "identity"
            or type(data["default_result"]) is not int
            or data["default_result"] != 0
            or data["after"] not in ("stop", "wm_create")
        ):
            raise ValueError("only declared identity RECT/zero result supported")
        for key, value in data.items():
            object.__setattr__(self, key, value)

    @property
    def metadata(self):
        return {
            "rect_output": self.rect_output,
            "default_result": self.default_result,
            "after": self.after,
        }


@dataclass(frozen=True, init=False)
class CreateMessageProfile:
    default_result: int
    after: str

    def __init__(self, data):
        if not isinstance(data, dict) or set(data) != {"default_result", "after"}:
            raise ValueError("explicit CREATE fields required")
        if (
            type(data["default_result"]) is not int
            or data["default_result"] not in (0, -1)
            or data["after"] not in ("stop", "wm_showwindow")
        ):
            raise ValueError("only declared CREATE0/-1 and bounded delivery supported")
        object.__setattr__(self, "default_result", data["default_result"])
        object.__setattr__(self, "after", data["after"])

    @property
    def metadata(self):
        return {"default_result": self.default_result, "after": self.after}


@dataclass(frozen=True, init=False)
class CreationProfile:
    default_nccreate_result: int
    after_nccreate: str
    nccalcsize: CalculationProfile | None
    wm_create: CreateMessageProfile | None

    def __init__(self, data):
        required = {
            "default_nccreate_result",
            "after_nccreate",
        }
        if (
            not isinstance(data, dict)
            or not required <= set(data)
            or set(data) - required - {"nccalcsize", "wm_create"}
        ):
            raise ValueError("explicit creation continuation fields required")
        result = data["default_nccreate_result"]
        next_phase = data["after_nccreate"]
        if type(result) is not int or result not in (0, 1):
            raise ValueError("declared WM_NCCREATE result must be 0 or 1")
        if next_phase not in ("stop", "nccalcsize_rect"):
            raise ValueError("unsupported post-NCCREATE phase")
        object.__setattr__(self, "default_nccreate_result", result)
        object.__setattr__(self, "after_nccreate", next_phase)
        calc = CalculationProfile(data["nccalcsize"]) if "nccalcsize" in data else None
        if calc is not None and next_phase != "nccalcsize_rect":
            raise ValueError("NCCALCSIZE delivery must be enabled")
        object.__setattr__(self, "nccalcsize", calc)
        create = (
            CreateMessageProfile(data["wm_create"]) if "wm_create" in data else None
        )
        if create is not None and (calc is None or calc.after != "wm_create"):
            raise ValueError("CREATE delivery must be enabled")
        object.__setattr__(self, "wm_create", create)

    @property
    def metadata(self):
        result = {
            "default_nccreate_result": self.default_nccreate_result,
            "after_nccreate": self.after_nccreate,
        }
        if self.nccalcsize is not None:
            result["nccalcsize"] = self.nccalcsize.metadata
        if self.wm_create is not None:
            result["wm_create"] = self.wm_create.metadata
        return result


class CreationMessages:
    """Declared creation defaults; unsupported messages remain strict."""

    def __init__(self, profile, host, boundary, make_callback):
        self.profile = profile
        self.host = host
        self.boundary = boundary
        self.make_callback = make_callback
        self.records = {}
        self.last_call = {}

    def watch_creation(self, handle, target, parameters, words):
        self.records[handle] = {
            "target": target,
            "parameters": parameters,
            "words": tuple(words),
            "owner": self.host.thread_id,
            "message": 0x81,
            "args": (handle, 0x81, 0, parameters),
        }

    def default_proc(self, args):
        handle, message, _, parameters = args
        self.last_call = {
            "stage": "default_proc",
            "hwnd": handle,
            "message": message,
            "output_write": None,
        }
        frame = self.host.pending_window_callbacks.get(self.host.thread_id)
        record = self.records.get(handle)
        if (
            frame is None
            or record is None
            or record["owner"] != self.host.thread_id
            or frame["request"].target != record["target"]
            or frame["request"].args != tuple(args)
            or record["args"] != tuple(args)
        ):
            raise self.boundary("unsupported_default_proc_context", args=args)
        if message != 0x81:
            if message == 0x83:
                rectangle = self.host.read_u32s(parameters, 4)
                if self.profile.nccalcsize is not None:
                    self.host.write_u32s(parameters, rectangle)
                    self.last_call.update(
                        input_rect=rectangle,
                        output_rect=rectangle.copy(),
                        output_write={"address": parameters, "bytes": 16},
                        declared_lresult=self.profile.nccalcsize.default_result,
                    )
                    return self.profile.nccalcsize.default_result
                self.last_call["rect_words"] = rectangle
            elif message == 1:
                self.last_call["create_struct_words"] = self.host.read_u32s(
                    parameters, 12
                )
                if self.profile.wm_create is not None:
                    if (
                        parameters != record["parameters"]
                        or tuple(self.last_call["create_struct_words"])
                        != record["words"]
                    ):
                        raise self.boundary(
                            "unsupported_create_struct",
                            hwnd=handle,
                            words=self.last_call["create_struct_words"],
                        )
                    result = self.profile.wm_create.default_result & 0xFFFFFFFF
                    self.last_call["declared_lresult"] = result
                    return result
            raise self.boundary(
                "unsupported_default_window_message",
                message=message,
                args=args,
                default_result_supplied=False,
            )
        if record["message"] != 0x81 or parameters != record["parameters"]:
            raise self.boundary("unsupported_default_proc_context", args=args)
        words = self.host.read_u32s(parameters, 12)
        if tuple(words) != record["words"]:
            raise self.boundary("unsupported_create_struct", hwnd=handle, words=words)
        self.last_call.update(
            create_struct_words=words,
            declared_nccreate_result=self.profile.default_nccreate_result,
        )
        return self.profile.default_nccreate_result

    def after_callback(self, request, result):
        record = self.records.get(request.hwnd)
        if (
            record is None
            or record["target"] != request.target
            or record["owner"] != self.host.thread_id
            or record["args"] != request.args
        ):
            raise self.boundary(
                "unsupported_creation_callback", args=list(request.args)
            )
        message = request.args[1]
        if (
            message == 0x81
            and result
            and self.profile.after_nccreate == "nccalcsize_rect"
        ):
            words = record["words"]
            x, y, width, height = words[7], words[6], words[5], words[4]
            rectangle = self.host.alloc(16)
            self.host.write_u32s(
                rectangle, [x, y, (x + width) & 0xFFFFFFFF, (y + height) & 0xFFFFFFFF]
            )
            record.update(message=0x83, args=(request.hwnd, 0x83, 0, rectangle))
            self.host.windows[request.hwnd]["creation"] = "WM_NCCALCSIZE_pending"
            return self.make_callback(request.target, record["args"], request.hwnd)
        if (
            message == 0x83
            and result == 0
            and self.profile.nccalcsize is not None
            and self.profile.nccalcsize.after == "wm_create"
        ):
            record.update(message=1, args=(request.hwnd, 1, 0, record["parameters"]))
            self.host.windows[request.hwnd]["creation"] = "WM_CREATE_pending"
            return self.make_callback(request.target, record["args"], request.hwnd)
        if (
            message == 1
            and result == 0
            and self.profile.wm_create is not None
            and self.profile.wm_create.after == "wm_showwindow"
        ):
            if not record["words"][8] & 0x10000000:
                raise self.boundary(
                    "unsupported_visible_notification",
                    hwnd=request.hwnd,
                    style=record["words"][8],
                )
            record.update(message=0x18, args=(request.hwnd, 0x18, 1, 0))
            self.host.windows[request.hwnd]["creation"] = "WM_SHOWWINDOW_pending"
            return self.make_callback(request.target, record["args"], request.hwnd)
        phase = (
            ("after_WM_NCCREATE_acceptance" if result else "after_WM_NCCREATE_refusal")
            if message == 0x81
            else "after_" + CREATION_MESSAGE_NAMES[message]
        )
        self.host.windows[request.hwnd]["creation"] = phase
        facts = {
            "phase": phase,
            "hwnd": request.hwnd,
            "callback_result": result,
            "win32_result_supplied": False,
        }
        if message == 0x83:
            facts["rect_words"] = self.host.read_u32s(request.args[3], 4)
        raise self.boundary("unsupported_window_creation_phase", **facts)
