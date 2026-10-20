#!/usr/bin/env python3
"""Original408ce0 monitor-container drain used at28960 frame completion.

The real drain runs over declared waiter slots;41f6f0 is a recording boundary.
No boot, script, renderer, or scheduler behavior is supplied by these rows.
"""
import hashlib
import json
from pathlib import Path

from oracle import Oracle


def run(slots, present=True):
    exe = Path(__file__).resolve().parents[2] / 'reference/raw/StreetLegal_Redline.bak.exe'
    digest = hashlib.sha256(exe.read_bytes()).hexdigest()
    assert digest == '7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde'
    oracle = Oracle(exe=str(exe))
    instance, container, data = [oracle.alloc(128) for _ in range(3)]
    oracle.write_u32(instance + 0x14, container if present else 0)
    oracle.write_u32s(container, [len(slots), data, len(slots)])
    if slots:
        oracle.write_u32s(data, slots)
    notified = []

    def notify(current, _args):
        notified.append(current.reg('ecx'))
        return 0

    oracle.hook(0x41F6F0, notify, 'thiscall', 0)
    oracle.call(0x408CE0, convention='thiscall', this=instance)
    remaining = oracle.read_u32(container + 8)
    after = oracle.read_u32s(data, len(slots))
    expected = [word for word in reversed(slots) if word] if present else []
    assert notified == expected
    assert remaining == (0 if present else len(slots))
    assert after == ([0] * len(slots) if present else slots)
    return {'exe_sha256': digest, 'slots': slots, 'container_present': present,
            'notified': notified, 'remaining': remaining, 'slots_after': after,
            'boundary': '41f6f0 records notifications; real408ce0 drains cells'}


if __name__ == '__main__':
    for slots, present in [([], False), ([], True), ([0x1100, 0, 0x2200, 0x3300], True)]:
        print(json.dumps(run(slots, present), sort_keys=True))
