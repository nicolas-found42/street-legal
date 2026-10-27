"""Fresh original/port rendering observations, with explicit incomplete scenes."""
from ..core import ROOT, stock_identity
from ..registry import validator
from render_stream import not_run, run_render


@validator("S5", executable=True)
def validate(ctx):
    if not ctx.live():
        return not_run("S5")
    observed = run_render(ctx.output_root, stock_check=lambda: stock_identity(ROOT / "stock"))
    return {**observed["S5"], "g6_observation": observed["G6"]}
