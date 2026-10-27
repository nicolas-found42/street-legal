"""G1: stock inputs and runtime. Only the checksum leg exists so far (partial)."""
from .. import core
from ..registry import Context, validator


@validator("G1", executable=True)
def g1(ctx: Context) -> dict:
    if not ctx.live():
        return core.result("not_run", "Run --execute G1; no window/runtime validator exists yet, so G1 cannot pass.")
    try:
        stock = core.stock_identity(core.ROOT / "stock")
    except (OSError, ValueError) as error:
        return core.result("failed", str(error))
    return core.result("partial", "Stock checksums verified; native window/fullscreen, unchanged-script runtime and approximation-removal gates remain incomplete.",
        stock_files=len(stock["files"]), original_exe_sha256=stock["original_exe_sha256"])
