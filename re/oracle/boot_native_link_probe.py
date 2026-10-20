#!/usr/bin/env python3
"""Original creation bridge with real429060 link and42a9e0/403820 field store.

Supplied decoded instance field storage, resource nodes and script-dispatch
recording boundaries. Also executes original ResourceRef newNative/set/id
wrappers against the resulting link. No script body or full startup is executed.
"""

import json
from boot_bridge_probe import run

if __name__ == "__main__":
    for result in [0, 1]:
        row = run(True, result, real_link=True)
        row["oracle"] = (
            "original53a5e0+408a70+429060+42a9e0+403820; ResourceRef47cea0/47cf80/47d290; supplied instance storage/resources/script dispatch"
        )
        print(json.dumps(row, sort_keys=True))
