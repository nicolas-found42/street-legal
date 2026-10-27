"""Whole render gate consumes the same live S5 execution."""
from ..core import result
from ..registry import validator
from render_stream import not_run


@validator("G6", executable=True, requires=("S5",))
def validate(ctx):
    if not ctx.live():
        return not_run("G6")
    supporting = ctx.results["S5"]
    observed = supporting.get("g6_observation")
    if not isinstance(observed, dict):
        return result("blocked", "S5 did not produce a live rendering observation.")
    if observed.get("result") == "passed" and supporting["result"] != "passed":
        return result("blocked", "The complete S5 rendering harness must pass before G6 can pass.")
    return observed
