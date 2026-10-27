"""S1: boot milestone (completed MainMenu + 300 idle frames, complete streams)."""
from .. import boot, core
from ..registry import Context, validator


@validator("S1", executable=True)
def s1(ctx: Context) -> dict:
    if ctx.live():
        return boot.run_boot(ctx.output_root)
    return boot.boot_result(ctx.boot_bundle)
