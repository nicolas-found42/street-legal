"""S9 proves the real native display path independently of stock-scene fidelity."""
from ..core import result
from ..registry import validator
from window import window_result

@validator("S9", executable=True)
def validate(ctx):
    if not ctx.live():
        return result("not_run", "Run --execute S9 for fresh source-bound native macOS display observations.")
    return window_result(ctx.output_root)
