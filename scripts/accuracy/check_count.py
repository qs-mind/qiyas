"""Compare CountBlock with an independent model-space INSERT handle list.
Usage: python check_count.py drawing.dwg inspection.dxf output.json
Private block/layer names stay in memory. Requires the built accuracy_dump example.
Scope: visible, ordinary named model-space blocks; nested contents are excluded.
"""
import collections
import json
import hashlib
import os
import subprocess
import sys
import ezdxf

drawing, inspection, output = sys.argv[1:]
doc = ezdxf.readfile(inspection)
def visible(e):
    layer = doc.layers.get(e.dxf.layer)
    return not e.dxf.get('invisible', 0) and not layer.is_off() and not layer.is_frozen()
ins = [e for e in doc.modelspace().query('INSERT') if visible(e) and not e.dxf.name.startswith('*')]
name = collections.Counter(e.dxf.name for e in ins).most_common(1)[0][0]
selected = [e for e in ins if e.dxf.name == name]
layer = collections.Counter(e.dxf.layer for e in selected).most_common(1)[0][0]
other = next((x.dxf.name for x in doc.layers if x.dxf.name != layer), None)
scenarios = [('all-visible', None, None), ('layer-filter', layer, None), ('layer-hidden', layer, layer), ('all-hide-selected-layer', None, layer)]
if other:
    scenarios.append(('other-layer-filter', other, None))
rows = []
for label, only, hidden in scenarios:
    expected = sorted(e.dxf.handle for e in selected if (only is None or e.dxf.layer == only) and e.dxf.layer != hidden)
    env = os.environ.copy()
    for key in ['QIYAS_COUNT_BLOCK', 'QIYAS_COUNT_LAYER', 'QIYAS_HIDE_LAYER']:
        env.pop(key, None)
    env['QIYAS_COUNT_BLOCK'] = name
    if only is not None: env['QIYAS_COUNT_LAYER'] = only
    if hidden is not None: env['QIYAS_HIDE_LAYER'] = hidden
    actual = json.loads(subprocess.check_output(['target/debug/examples/accuracy_dump', drawing], env=env, text=True))
    passed = actual['count'] == len(expected) and sorted(actual['handles']) == expected and actual['xref_points'] == 0
    rows.append(dict(scenario=label, block_sha256=hashlib.sha256(name.encode()).hexdigest(), selected_layer_sha256=hashlib.sha256(layer.encode()).hexdigest(), independent_handles=expected, independent_count=len(expected), **actual, passed=passed))
with open(output, 'w') as f: json.dump(rows, f, indent=2)
print([(r['scenario'], r['count'], r['passed']) for r in rows])
raise SystemExit(0 if all(r['passed'] for r in rows) else 1)
