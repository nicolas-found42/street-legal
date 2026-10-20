#!/usr/bin/env python3
"""Original native factory with a declared materialized empty type1 parent.

Real parent/child constructors and inherited-range setters execute. Parent load
state is supplied; resource construction/virtual template/event seams remain.
No package load, original scripts or complete startup. Output in re/out/ only.
"""
import json
from boot_native_factory_probe import run

if __name__ == '__main__':
    row = run(0xffff007e, 0xffff007d, fresh_defaults=True, fresh_child=True)
    row['oracle'] = 'Unicorn original53e6e0 parent/child +53a2a0 +inherited-range setters +empty postcreate helpers/links; declared loaded emptyparent/template/virtual/event boundaries'
    print(json.dumps(row, sort_keys=True))
