#!/usr/bin/env python3
"""Original44a250 parameter camera destinations and source weakrefs, with48b940 as declared callback.
Full original physics path through44b486 executes unchanged; the callback records
ECX and all five stack parameters without mutating resources. Native48b940 itself
is verified separately by native_camera_probe.py.
"""
import json
from native_car_graph_probe import run
if __name__=='__main__':
 result=run(attachments=2);result['scope']=__doc__;print(json.dumps(result,sort_keys=True))
