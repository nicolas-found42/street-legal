"""S11: fresh ledger collection and idempotent regeneration."""
from .. import core, ledger
from ..registry import Context, validator


@validator("S11", executable=True)
def s11(ctx: Context) -> dict:
    if ctx.live():
        return ledger.ledger_result(ctx.output_root)
    return core.result("not_run", "Run --execute S11 for fresh isolated ledger collection/regeneration; receipts cannot assert pass.")
