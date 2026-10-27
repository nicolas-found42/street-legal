"""Whole-install format gate."""
from ..core import result
from ..registry import validator
from formats import formats_result


@validator("G2", executable=True)
def validate(ctx):
    if not ctx.live():
        return result("not_run", "Execute G2 for a fresh complete census and loader observations.")
    return formats_result(ctx.output_root)
