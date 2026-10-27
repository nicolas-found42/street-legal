"""G9: script-level saves remain blocked by the native-VM/render frontier."""
import saves
from ..registry import Context, validator


@validator("G9", executable=True, requires=("S8",))
def g9(ctx: Context) -> dict:
    return saves.gate_result(ctx.results.get("S8"))
