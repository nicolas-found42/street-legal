"""Exact x87 trust gate; missing original/hardware observations stay blocked."""
from ..core import result
from ..registry import validator
import g11


@validator("G11", executable=True)
def validate(ctx):
    if not ctx.live():
        return result("not_run", "Execute G11 with a pinned Bochs checkout and native Intel observation.")
    return g11.g11_result(ctx.output_root, ctx.options.get("bochs"), ctx.options.get("intel_observation"))
