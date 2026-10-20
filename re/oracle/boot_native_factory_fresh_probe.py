#!/usr/bin/env python3
"""Original native factory over a real fresh instance constructor.

Runs original53e6e0 then53a2a0, real postcreate4988a0/498b10/48ae40 and links.
Other resource/virtual/event collaborators remain supplied as in base probe.
No original script body or startup. Redirect output to ignored re/out/ only.
"""
import json
from boot_native_factory_probe import run

if __name__ == '__main__':
    row = run(0xffff007e, 0xffff007d, fresh_defaults=True)
    row['oracle'] = 'Unicorn original53e6e0 +53a2a0 +empty postcreate helpers/links; supplied template/virtual/event boundaries'
    print(json.dumps(row, sort_keys=True))
