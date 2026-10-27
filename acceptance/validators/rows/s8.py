"""S8: freshly executed original/port native save sessions."""
import saves
from .. import core
from ..registry import Context, validator


@validator("S8", executable=True)
def s8(ctx: Context) -> dict:
    if not ctx.live():
        return core.result("not_run", "Run --execute S8 for fresh original-vs-port save/config sessions; receipts cannot assert pass.")
    return saves.saves_result(ctx.output_root, core.stock_identity,
        lambda: core.boot_tools().git_identity(core.ROOT), core.candidate_source_matches, core.ORIGINAL_EXE)
