#!/usr/bin/env python3
"""Writes the synthetic drawings of the accuracy table (QSM-304) with known answers.

usage: make_synthetic.py <out-dir>
Each `<name>.dxf` gets `<name>.expect.json`: {handle: [kind, value in drawing units]} worked out
by hand here (pi r^2, scale factors...), so check_accuracy.py can compare against it too.
"""
import json, math, os, sys
import ezdxf

out = sys.argv[1]
os.makedirs(out, exist_ok=True)


def save(doc, name, expect):
    doc.saveas(os.path.join(out, name + ".dxf"))
    json.dump(expect, open(os.path.join(out, name + ".expect.json"), "w"), indent=1)


def new(units):
    d = ezdxf.new("R2018")
    d.header["$INSUNITS"] = units
    return d


# 1. Units 0 / 1 / 2 / 4 / 5 / 6: a rectangle, a bulged polyline, a circle, a line, an arc.
for u in (0, 1, 2, 4, 5, 6):
    d = new(u)
    ms = d.modelspace()
    exp = {}
    e = ms.add_lwpolyline([(0, 0), (4, 0), (4, 3), (0, 3)], close=True)
    exp[e.dxf.handle] = ["area", 12.0]
    e = ms.add_lwpolyline([(10, 0, 1), (14, 0, 1)], format="xyb", close=True)  # two half circles
    exp[e.dxf.handle] = ["area", math.pi * 4.0]
    e = ms.add_circle((20, 0), 1.5)
    exp[e.dxf.handle] = ["area", math.pi * 2.25]
    e = ms.add_line((0, 10), (3, 14))
    exp[e.dxf.handle] = ["length", 5.0]
    e = ms.add_arc((0, 20), 2, 0, 90)
    exp[e.dxf.handle] = ["length", math.pi]
    # open polyline with a quarter-circle bulge: 3 + pi/2 * 2 + 3
    e = ms.add_lwpolyline([(0, 30, 0), (3, 30, math.tan(math.pi / 8)), (5, 32, 0), (5, 35)], format="xyb")
    r = (2 ** 0.5 * 2) / (2 * math.sin(math.pi / 4))  # chord (3,30)-(5,32) is 2*sqrt2, 90 deg
    exp[e.dxf.handle] = ["length", 3 + r * math.pi / 2 + 3]
    save(d, f"units_{u}", exp)

# 2. Blocks inside blocks, scaled, rotated, mirrored, non-uniform.
d = new(4)  # mm
inner = d.blocks.new("INNER")
r = inner.add_lwpolyline([(0, 0), (100, 0), (100, 50), (0, 50)], close=True)
c = inner.add_circle((50, 25), 10)
l = inner.add_line((0, 0), (100, 50))
mid = d.blocks.new("MID")
mid.add_blockref("INNER", (10, 0), dxfattribs={"xscale": 2, "yscale": 2, "rotation": 90})
mid.add_blockref("INNER", (0, 200), dxfattribs={"xscale": 3, "yscale": 1})  # non-uniform
outer = d.blocks.new("OUTER")
outer.add_blockref("MID", (0, 0), dxfattribs={"xscale": 1, "yscale": 1})
ms = d.modelspace()
o1 = ms.add_blockref("OUTER", (1000, 0), dxfattribs={"xscale": -0.5, "yscale": 0.5, "rotation": 30})
mid_h = [e for e in outer if e.dxftype() == "INSERT"][0].dxf.handle
i1, i2 = [e for e in mid if e.dxftype() == "INSERT"]
exp = {}
# chain model>OUTER>MID>INNER(2x, rotated): area scale 0.5*0.5*1*(2*2)
exp[f"{o1.dxf.handle}>{mid_h}>{i1.dxf.handle}>{r.dxf.handle}"] = ["area", 5000 * 4 * 0.25]
exp[f"{o1.dxf.handle}>{mid_h}>{i1.dxf.handle}>{c.dxf.handle}"] = ["area", math.pi * 100 * 4 * 0.25]
exp[f"{o1.dxf.handle}>{mid_h}>{i1.dxf.handle}>{l.dxf.handle}"] = ["length", math.hypot(100, 50) * 2 * 0.5]
# chain ... INNER with x3,y1 then outer 0.5: rect area 5000*3*1*0.25; the line is the diagonal
exp[f"{o1.dxf.handle}>{mid_h}>{i2.dxf.handle}>{r.dxf.handle}"] = ["area", 5000 * 3 * 0.25]
exp[f"{o1.dxf.handle}>{mid_h}>{i2.dxf.handle}>{l.dxf.handle}"] = ["length", math.hypot(300, 50) * 0.5]
save(d, "nested", exp)

# 3. Hatches: a circle with a square hole (polyline path), an edge path with arcs.
d = new(6)
ms = d.modelspace()
h = ms.add_hatch(color=1)
h.paths.add_polyline_path([(0, 0, 1), (10, 0, 1)], is_closed=True)  # circle diameter 10
h.paths.add_polyline_path([(3, -1), (7, -1), (7, 1), (3, 1)], is_closed=True)  # 4 x 2 hole
exp = {h.dxf.handle: ["area", math.pi * 25 - 8]}
h2 = ms.add_hatch(color=2)
ep = h2.paths.add_edge_path()
ep.add_line((20, 0), (30, 0))
ep.add_arc((25, 0), 5, 0, 180, ccw=True)  # half disc above the line
exp[h2.dxf.handle] = ["area", math.pi * 25 / 2]
save(d, "hatch", exp)

# 4. An xref placed with scale 0.5, rotated 90 degrees.
x = new(6)
xm = x.modelspace()
xr = xm.add_lwpolyline([(0, 0), (10, 0), (10, 4), (0, 4)], close=True)
xc = xm.add_circle((5, 2), 1)
x.saveas(os.path.join(out, "xr.dxf"))
d = new(6)
d.add_xref_def("xr.dxf", "XR")
ins = d.modelspace().add_blockref("XR", (5, 5), dxfattribs={"xscale": 0.5, "yscale": 0.5, "rotation": 90})
exp = {f"{ins.dxf.handle}>{xr.dxf.handle}": ["area", 10.0], f"{ins.dxf.handle}>{xc.dxf.handle}": ["area", math.pi * 0.25]}
save(d, "xref", exp)
