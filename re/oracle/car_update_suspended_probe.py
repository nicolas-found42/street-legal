#!/usr/bin/env python3
"""Complete original448430 runs on suspended/absent-script branches.

Same controlled collaborators as car_update_prefix_probe. These branches skip
script parameter projection and execute original4479d0 to return normally.
Dyadic inputs keep the tested arithmetic PC-insensitive; no float tolerance.
"""
import hashlib
import json
from pathlib import Path
from oracle import DEFAULT_EXE
from car_update_prefix_probe import run

if __name__ == '__main__':
    print(json.dumps(dict(exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),
                         scope=__doc__, rows=[
                             run(w, s, suspended, script, scale)
                             for w in [0, 3] for s in [False, True]
                             for suspended, script in [(True, 73), (False, 0)]
                             for scale in [-1.0, 0.0, 1.5]]), sort_keys=True))
