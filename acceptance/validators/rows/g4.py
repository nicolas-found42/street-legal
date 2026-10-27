"""Native physics differential gate."""
from ..core import result
from ..registry import validator
from g4 import g4_result


@validator("G4", executable=True)
def validate(ctx):
    if not ctx.live():
        return result("not_run", "Execute G4 for fresh per-native physics classification and original vectors.")
    return g4_result(ctx.output_root)
