"""S10: retire the approximate slice and exercise its replacement."""
from .. import retirement
from ..registry import Context, validator


@validator("S10", executable=True, requires=("S1",))
def s10(ctx: Context) -> dict:
    live = "S10" in ctx.execute
    return retirement.retirement_result(ctx.output_root, ctx.results["S1"] if live else None, live_execution=live)
