"""Build private answers from inspection DXF coordinates, never measurement output.
Usage: python real_references.py private-directory source-a.dwg source-b.dwg a.dxf b.dxf
DXF decoding is shared with Qiyas. This proves arithmetic independence only.
"""
import hashlib
import json
import math
from pathlib import Path
import sys

out, *inputs = map(Path, sys.argv[1:])
out.mkdir(parents=True, exist_ok=True, mode=0o700)
cases = []
for alias, handle, source, dxf in zip(('real-a', 'real-b'), ('362F', 'D573'), inputs[:2], inputs[2:]):
    lines = dxf.read_text().splitlines()
    records, record = [], []
    for code, value in zip(lines[::2], lines[1::2]):
        code, value = code.strip(), value.strip()
        if code == '0':
            records.append(record)
            record = []
        record.append((code, value))
    records.append(record)
    matches = [dict(r) for r in records if ('5', handle) in r]
    assert len(matches) == 1
    entity = matches[0]
    assert entity['0'] == 'LINE' and entity['330'] == '1F'
    # INSUNITS=6 (metres) must be checked from the DXF header, without a fallback.
    pairs = [(c.strip(), v.strip()) for c, v in zip(lines[::2], lines[1::2])]
    index = pairs.index(('9', '$INSUNITS'))
    assert pairs[index + 1] == ('70', '6')
    start = [float(entity[c]) for c in ('10', '20', '30')]
    end = [float(entity[c]) for c in ('11', '21', '31')]
    expected = math.dist(start, end)
    assert math.isfinite(expected) and expected > 0
    cases.append(dict(id=alias, path=str(source.resolve()), sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
                      handle=int(handle, 16), layer=entity['8'], start=start, end=end,
                      question=f'ما طول الخط المحدد بالمقبض {handle} في مساحة النموذج بالمتر؟',
                      formula='sqrt(sum((end[i]-start[i])**2 for i in range(3)))',
                      expected=expected, unit='m', tolerance=0.001, expert_review='pending'))
path = out / 'references.json'
path.write_text(json.dumps(cases, ensure_ascii=False, indent=2) + '\n')
path.chmod(0o600)
print('Two private reference answers written. Professional review is pending.')
