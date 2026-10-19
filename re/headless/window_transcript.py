"""Explicit hypothetical message inputs; no Windows notification-order model."""

from dataclasses import dataclass

TRANSCRIPT_NAMES = {
    3: "WM_MOVE",
    5: "WM_SIZE",
    0x1C: "WM_ACTIVATEAPP",
    0x46: "WM_WINDOWPOSCHANGING",
    0x47: "WM_WINDOWPOSCHANGED",
    6: "WM_ACTIVATE",
    7: "WM_SETFOCUS",
    8: "WM_KILLFOCUS",
}


@dataclass(frozen=True, init=False)
class MessageInput:
    message: int
    wparam: int
    lparam: int | None
    windowpos: tuple | None
    default_result: int
    expected_callback_result: int

    def __init__(self, data):
        if not isinstance(data, dict) or not data:
            raise ValueError("explicit transcript message required")
        message = data.get("message")
        if type(message) is not int or message not in TRANSCRIPT_NAMES:
            raise ValueError("unrecovered transcript message")
        pointer = message in (0x46, 0x47)
        fields = {"message", "wparam", "default_result", "expected_callback_result"}
        fields |= {"windowpos", "default_memory"} if pointer else {"lparam"}
        if set(data) != fields:
            raise ValueError("explicit message fields required")
        for key in ("default_result", "expected_callback_result"):
            if type(data[key]) is not int or data[key] != 0:
                raise ValueError("only explicitly declared zero results supported")
        if type(data["wparam"]) is not int or not 0 <= data["wparam"] <= 0xFFFFFFFF:
            raise ValueError("raw32bit wParam required")
        if pointer:
            words = data["windowpos"]
            if (
                data["wparam"] != 0
                or data["default_memory"] != "identity"
                or not isinstance(words, list)
                or len(words) != 6
                or words[0] not in ("top", "topmost")
                or any(type(v) is not int or not -8192 <= v <= 8192 for v in words[1:5])
                or type(words[5]) is not int
                or not 0 <= words[5] <= 0xFFFF
            ):
                raise ValueError("bounded explicit identity WINDOWPOS required")
            windowpos, lparam = tuple(words), None
        else:
            lparam, windowpos = data["lparam"], None
            if type(lparam) is not int or not 0 <= lparam <= 0xFFFFFFFF:
                raise ValueError("raw32bit lParam required")
        for key in ("message", "wparam", "default_result", "expected_callback_result"):
            object.__setattr__(self, key, data[key])
        object.__setattr__(self, "windowpos", windowpos)
        object.__setattr__(self, "lparam", lparam)

    @property
    def metadata(self):
        result = {
            key: getattr(self, key)
            for key in (
                "message",
                "wparam",
                "default_result",
                "expected_callback_result",
            )
        }
        if self.windowpos is None:
            result["lparam"] = self.lparam
        else:
            result.update(windowpos=list(self.windowpos), default_memory="identity")
        return result


@dataclass(frozen=True, init=False)
class TranscriptProfile:
    messages: tuple[MessageInput, ...]
    api_return: str

    def __init__(self, data):
        if (
            not isinstance(data, dict)
            or set(data) != {"messages", "api_return"}
            or not isinstance(data["messages"], list)
            or len(data["messages"]) > 8
            or data["api_return"] not in ("stop", "owned_provisional_handle")
        ):
            raise ValueError(
                "bounded transcript and explicit API-return choice required"
            )
        object.__setattr__(
            self, "messages", tuple(MessageInput(x) for x in data["messages"])
        )
        object.__setattr__(self, "api_return", data["api_return"])

    @property
    def metadata(self):
        return {
            "messages": [x.metadata for x in self.messages],
            "api_return": self.api_return,
        }


@dataclass(frozen=True, init=False)
class ShowMessageProfile:
    default_result: int
    expected_callback_result: int
    after: str
    transcript: TranscriptProfile | None

    def __init__(self, data):
        required = {"default_result", "expected_callback_result", "after"}
        if (
            not isinstance(data, dict)
            or not required <= set(data)
            or set(data) - required - {"transcript"}
        ):
            raise ValueError("explicit SHOW fields required")
        if (
            type(data["default_result"]) is not int
            or data["default_result"] != 0
            or type(data["expected_callback_result"]) is not int
            or data["expected_callback_result"] != 0
            or data["after"] not in ("stop", "transcript")
        ):
            raise ValueError(
                "only explicit SHOW zero and bounded continuation supported"
            )
        if ("transcript" in data) != (data["after"] == "transcript"):
            raise ValueError("transcript must be explicitly enabled")
        for key in required:
            object.__setattr__(self, key, data[key])
        object.__setattr__(
            self,
            "transcript",
            TranscriptProfile(data["transcript"]) if "transcript" in data else None,
        )

    @property
    def metadata(self):
        result = {
            "default_result": self.default_result,
            "expected_callback_result": self.expected_callback_result,
            "after": self.after,
        }
        if self.transcript is not None:
            result["transcript"] = self.transcript.metadata
        return result


@dataclass(frozen=True)
class DeclaredWindowReturn:
    hwnd: int


class TranscriptExecution:
    """One owned suspended CreateWindowExA call and its explicit message inputs."""

    def __init__(self, profile, host, boundary, hwnd, record):
        self.profile, self.host, self.boundary = profile, host, boundary
        self.hwnd, self.record = hwnd, record
        self.address, self.stack = host.reg("eip"), host.reg("esp")
        self.call_words = tuple(host.read_u32s(self.stack, 13))
        self.event = host.events[-1]
        self.active, self.message, self.cursor = None, None, 0
        self.default_consumed = False
        self.windowpos_words = None
        self.used = self.supplied = False
        self.ready = False
        self.validate_call()

    def validate_call(self):
        host = self.host
        if (
            self.used
            or host.thread_id != self.record["owner"]
            or host.reg("eip") != self.address
            or host.reg("esp") != self.stack
            or host.import_names.get(self.address) != "user32.dll!CreateWindowExA"
            or tuple(host.read_u32s(self.stack, 13)) != self.call_words
            or not any(e is self.event for e in host.events)
            or self.event.get("import") != "user32.dll!CreateWindowExA"
            or tuple(self.event.get("args", ())) != self.call_words[1:]
            or self.event.get("return_site") != self.call_words[0]
            or self.event.get("thread") != self.record["owner"]
            or "result" in self.event
        ):
            raise self.boundary("unsupported_suspended_creation_call", hwnd=self.hwnd)

    def begin_show(self):
        self.active, self.message = self.profile, 0x18
        self.default_consumed = False

    def validate_structures(self):
        words = self.host.read_u32s(self.record["parameters"], 12)
        if tuple(words) != self.record["words"]:
            raise self.boundary("unsupported_create_struct", words=words)
        if self.windowpos_words is not None:
            words = self.host.read_u32s(self.record["args"][3], 7)
            if tuple(words) != self.windowpos_words:
                raise self.boundary("unsupported_declared_windowpos", words=words)

    def default_proc(self, args):
        if self.active is None or args[1] != self.message:
            raise self.boundary("unlisted_transcript_message", args=args)
        self.validate_structures()
        if self.default_consumed:
            raise self.boundary("repeated_declared_default", message=self.message)
        self.default_consumed = True
        return self.active.default_result

    def after_callback(self, request, result, make_callback):
        if self.active is None or request.args[1] != self.message:
            raise self.boundary("unlisted_transcript_message", args=list(request.args))
        self.validate_structures()
        name = (
            "WM_SHOWWINDOW" if self.message == 0x18 else TRANSCRIPT_NAMES[self.message]
        )
        self.host.windows[self.hwnd]["creation"] = "after_" + name
        self.host.events[-1]["declared_callback"] = {
            "default_result": self.active.default_result,
            "expected_callback_result": self.active.expected_callback_result,
            "default_consumed": self.default_consumed,
        }
        if result != self.active.expected_callback_result:
            raise self.boundary(
                "unexpected_declared_callback_result",
                message=self.message,
                expected=self.active.expected_callback_result,
                actual=result,
                win32_result_supplied=False,
            )
        transcript = self.profile.transcript
        if transcript is None:
            raise self.boundary(
                "unsupported_window_creation_phase",
                phase="after_WM_SHOWWINDOW",
                hwnd=self.hwnd,
                callback_result=result,
                win32_result_supplied=False,
            )
        if self.cursor < len(transcript.messages):
            entry = transcript.messages[self.cursor]
            self.cursor += 1
            self.active, self.message, self.default_consumed = (
                entry,
                entry.message,
                False,
            )
            parameters = entry.lparam
            self.windowpos_words = None
            if entry.windowpos is not None:
                insert, x, y, cx, cy, flags = entry.windowpos
                self.windowpos_words = tuple(
                    v & 0xFFFFFFFF
                    for v in (
                        self.hwnd,
                        -1 if insert == "topmost" else 0,
                        x,
                        y,
                        cx,
                        cy,
                        flags,
                    )
                )
                parameters = self.host.alloc(28)
                self.host.write_u32s(parameters, self.windowpos_words)
            args = (self.hwnd, self.message, entry.wparam, parameters)
            self.record.update(message=self.message, args=args)
            self.host.windows[self.hwnd]["creation"] = (
                TRANSCRIPT_NAMES[self.message] + "_declared_pending"
            )
            return make_callback(request.target, args, self.hwnd)
        if transcript.api_return == "stop":
            self.host.windows[self.hwnd]["creation"] = (
                "after_declared_message_transcript"
            )
            raise self.boundary(
                "after_declared_message_transcript",
                messages_delivered=self.cursor,
                hwnd=self.hwnd,
                callback_result=result,
                win32_result_supplied=False,
            )
        self.validate_call()
        self.ready = True
        return DeclaredWindowReturn(self.hwnd)

    def consume_return(self):
        self.validate_call()
        self.validate_structures()
        if (
            self.profile.transcript is None
            or self.profile.transcript.api_return != "owned_provisional_handle"
            or self.cursor != len(self.profile.transcript.messages)
            or self.host.pending_window_callbacks
            or not self.ready
        ):
            raise self.boundary("unsupported_declared_window_return", hwnd=self.hwnd)
        self.used = True
        return self.address

    def record_return(self):
        self.supplied = True
        self.event.update(
            result=self.hwnd,
            return_assumption="declared-platform-api-result",
            windows_notification_sequence_established=False,
            physical_desktop_observed=False,
        )
        self.host.windows[self.hwnd]["creation"] = "declared_api_return_supplied"
