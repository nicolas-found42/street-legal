"""Run two stream legs over the same input and tabulate agreement."""
import subprocess
import sys


def run(cmd, data):
    r = subprocess.run(cmd, input=data, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if r.returncode:
        raise RuntimeError(f"{cmd}: {r.returncode} {r.stderr[:500]!r}")
    return r.stdout


def records(buf, size):
    return [buf[i:i + size] for i in range(0, len(buf), size)]
