#!/usr/bin/env python3
"""Independent check of Qiyas measurements (QSM-304).

usage: check_accuracy.py <drawing.dxf> <dump.jsonl> [--tol 0.001] [--md table.md] [--min-matches 1]

`dump.jsonl` is the output of `cargo run -p qiyas-core --example accuracy_dump`. Every
measured entity is recomputed here from the DXF with ezdxf (a different reader and different
geometry code): block placement matrices composed by hand, curves flattened at 1e-6 of a drawing
unit, hatches filled even-odd. Needs `pip install ezdxf`. Exit 1 on any difference > tol.
"""
import json, math, os, sys
import ezdxf
from ezdxf import path as ezpath
from ezdxf.math import Matrix44, Vec3, OCS

# $INSUNITS -> metres per drawing unit; 0 (unitless) is read as mm, like Qiyas does.
FACTOR = {0: 0.001, 1: 0.0254, 2: 0.3048, 4: 0.001, 5: 0.01, 6: 1.0}
FLAT = 1e-6


def insert_matrix(doc, ins):
    """Block space -> WCS (or the outer block's space) for one INSERT, first array cell only."""
    d = ins.dxf
    base = Vec3(0, 0, 0)
    blk = doc.blocks.get(d.name)
    if blk is not None:
        base = Vec3(blk.block.dxf.base_point)
    sx, sy, sz = d.get("xscale", 1), d.get("yscale", 1), d.get("zscale", 1)
    rot = math.radians(d.get("rotation", 0))
    ocs = OCS(Vec3(d.get("extrusion", (0, 0, 1))))
    m = Matrix44.translate(-base.x, -base.y, -base.z)
    m = m @ Matrix44.scale(sx, sy, sz) @ Matrix44.z_rotate(rot)
    # OCS -> WCS, with the insertion point (given in OCS) placed after it.
    ucs = Matrix44.ucs(ocs.ux, ocs.uy, ocs.uz, ocs.to_wcs(Vec3(d.insert)))
    return m @ ucs


def ring_area(pts):
    return 0.5 * sum(pts[i].x * pts[(i + 1) % len(pts)].y - pts[(i + 1) % len(pts)].x * pts[i].y
                     for i in range(len(pts)))


def inside(p, ring):
    x, y, c = p.x, p.y, False
    for i in range(len(ring)):
        a, b = ring[i], ring[(i + 1) % len(ring)]
        if (a.y > y) != (b.y > y) and x < (b.x - a.x) * (y - a.y) / (b.y - a.y) + a.x:
            c = not c
    return c


ANGLE_STEP = 2e-5  # radians per chord: sagitta r*(step^2)/8 is far below 1e-6 for r < 1e4


def arc_points(c, r, a0, a1):
    """Points on a circle arc from a0 to a1 (radians, either direction), ends included."""
    n = max(2, int(abs(a1 - a0) / ANGLE_STEP) + 1)
    return [Vec3(c[0] + r * math.cos(a0 + (a1 - a0) * i / n), c[1] + r * math.sin(a0 + (a1 - a0) * i / n), 0)
            for i in range(n + 1)]


def bulge_points(p, q, bulge):
    """Points from p to q along a bulge arc (tan(sweep/4)), both ends included."""
    if abs(bulge) < 1e-12:
        return [Vec3(p[0], p[1], 0), Vec3(q[0], q[1], 0)]
    sweep = 4 * math.atan(bulge)
    chord = math.hypot(q[0] - p[0], q[1] - p[1])
    r = chord / (2 * math.sin(abs(sweep) / 2))
    mx, my = (p[0] + q[0]) / 2, (p[1] + q[1]) / 2
    h = r * math.cos(abs(sweep) / 2)  # negative past a half turn: the centre switches side
    nx, ny = -(q[1] - p[1]) / chord, (q[0] - p[0]) / chord  # left normal
    side = 1 if bulge > 0 else -1
    cx, cy = mx + side * nx * h, my + side * ny * h
    a0 = math.atan2(p[1] - cy, p[0] - cx)
    return arc_points((cx, cy), r, a0, a0 + sweep)


def vertex_ring(verts, closed):
    """verts: [(x, y, bulge)] in OCS; the ring with bulge arcs flattened (closed ring: no repeat)."""
    pts = []
    n = len(verts)
    for i in range(n if closed else n - 1):
        p, q = verts[i], verts[(i + 1) % n]
        seg = bulge_points(p, q, p[2] if len(p) > 2 else 0)
        pts.extend(seg[:-1])
    if not closed:
        pts.append(Vec3(verts[-1][0], verts[-1][1], 0))
    return pts


def to_wcs(pts, ocs, elevation, m):
    return [m.transform(ocs.to_wcs(Vec3(p.x, p.y, elevation))) for p in pts]


def ellipse_points(e, m):
    c, major = Vec3(e.dxf.center), Vec3(e.dxf.major_axis)
    ratio = e.dxf.ratio
    ext = Vec3(e.dxf.get("extrusion", (0, 0, 1)))
    minor = ext.cross(major).normalize(major.magnitude * ratio)
    a0, a1 = e.dxf.start_param, e.dxf.end_param
    if a1 <= a0:
        a1 += 2 * math.pi
    n = max(8, int((a1 - a0) / ANGLE_STEP))
    return [m.transform(c + major * math.cos(a0 + (a1 - a0) * i / n) + minor * math.sin(a0 + (a1 - a0) * i / n))
            for i in range(n + 1)]


def hatch_rings(h, m):
    ocs = OCS(Vec3(h.dxf.get("extrusion", (0, 0, 1))))
    elev = Vec3(h.dxf.get("elevation", (0, 0, 0))).z
    rings = []
    for path in h.paths:
        if path.type.name == "POLYLINE_PATH" or type(path).__name__ == "PolylinePath":
            v = [(x, y, b) for x, y, b in path.vertices]
            rings.append(to_wcs(vertex_ring(v, True), ocs, elev, m))
            continue
        pts = []
        for ed in path.edges:
            k = type(ed).__name__
            if k == "LineEdge":
                seg = [Vec3(ed.start[0], ed.start[1], 0), Vec3(ed.end[0], ed.end[1], 0)]
            elif k == "ArcEdge":
                # ezdxf hands the angles over as the arc's counter-clockwise geometry whatever the
                # edge's direction flag says (checked: each edge's end meets its neighbour's).
                a0, a1 = math.radians(ed.start_angle), math.radians(ed.end_angle)
                if a1 <= a0:
                    a1 += 2 * math.pi
                seg = arc_points(ed.center, ed.radius, a0, a1)
                if not ed.ccw:  # walked clockwise: from the stored end back to the start
                    seg = seg[::-1]
            elif k == "EllipseEdge":
                maj = Vec3(ed.major_axis[0], ed.major_axis[1], 0)
                mnr = Vec3(-maj.y, maj.x, 0) * ed.ratio
                a0, a1 = math.radians(ed.start_angle), math.radians(ed.end_angle)
                if a1 <= a0:
                    a1 += 2 * math.pi
                n = max(8, int((a1 - a0) / ANGLE_STEP))
                seg = [Vec3(ed.center[0], ed.center[1], 0) + maj * math.cos(a0 + (a1 - a0) * i / n)
                       + mnr * math.sin(a0 + (a1 - a0) * i / n) for i in range(n + 1)]
                if not ed.ccw:
                    seg = seg[::-1]
            else:  # spline edge: ezdxf's own flattening
                seg = [Vec3(p[0], p[1], 0) for p in ed.construction_tool().flattening(1e-6)]
            pts.extend(seg[:-1])
        if pts:
            rings.append(to_wcs(pts, ocs, elev, m))
    return [r for r in rings if len(r) >= 3]


def rings_of(h, m):
    return hatch_rings(h, m)


def even_odd_area(rings):
    total = 0.0
    for i, r in enumerate(rings):
        depth = sum(1 for j, o in enumerate(rings) if j != i and inside(r[0], o))
        a = abs(ring_area(r))
        total += a if depth % 2 == 0 else -a
    return total


def expected(doc, e, m):
    """(kind, value in drawing units) of entity e placed by m, or None when not comparable."""
    t = e.dxftype()
    if t == "INSERT":
        return "count", 1.0
    if t == "DIMENSION":
        if e.dimtype & 7 in (2, 5):  # angular
            return None
        s = (m.transform(Vec3(1, 0, 0)) - m.transform(Vec3(0, 0, 0))).magnitude
        kind = e.dimtype & 7
        d = e.dxf
        if kind in (0, 1):  # rotated / aligned: the two extension points, projected on the dim line
            v = Vec3(d.defpoint3) - Vec3(d.defpoint2)
            if kind == 0:
                a = math.radians(d.get("angle", 0))
                v = Vec3(math.cos(a), math.sin(a), 0) * v.dot(Vec3(math.cos(a), math.sin(a), 0))
            return "length", v.magnitude * s
        if kind in (3, 4):  # diameter / radius: the two defining points
            return "length", (Vec3(d.defpoint) - Vec3(d.defpoint4)).magnitude * s
        return None
    if t == "LINE":
        a, b = m.transform(Vec3(e.dxf.start)), m.transform(Vec3(e.dxf.end))
        return "length", (b - a).magnitude
    if t == "HATCH":
        return "area", even_odd_area(rings_of(e, m))
    ocs = OCS(Vec3(e.dxf.get("extrusion", (0, 0, 1)))) if e.dxf.hasattr("extrusion") else OCS(Vec3(0, 0, 1))
    if t == "CIRCLE":
        c = e.dxf.center
        ring = arc_points((c[0], c[1]), e.dxf.radius, 0, 2 * math.pi)[:-1]
        return "area", abs(ring_area(to_wcs(ring, ocs, c[2], m)))
    if t == "ARC":
        c = e.dxf.center
        a0, a1 = math.radians(e.dxf.start_angle), math.radians(e.dxf.end_angle)
        if a1 <= a0:
            a1 += 2 * math.pi
        pts = to_wcs(arc_points((c[0], c[1]), e.dxf.radius, a0, a1), ocs, c[2], m)
        return "length", sum((pts[i + 1] - pts[i]).magnitude for i in range(len(pts) - 1))
    if t == "LWPOLYLINE":
        v = [(x, y, b) for x, y, b in e.get_points("xyb")]
        closed = e.closed
        pts = to_wcs(vertex_ring(v, closed), ocs, e.dxf.get("elevation", 0), m)
        if closed:
            return "area", abs(ring_area(pts))
        return "length", sum((pts[i + 1] - pts[i]).magnitude for i in range(len(pts) - 1))
    if t == "ELLIPSE":
        pts = ellipse_points(e, m)
        full = abs((e.dxf.end_param - e.dxf.start_param) % (2 * math.pi)) < 1e-9
        if full:
            return "area", abs(ring_area(pts[:-1]))
        return "length", sum((pts[i + 1] - pts[i]).magnitude for i in range(len(pts) - 1))
    if t in ("SOLID", "TRACE"):
        v = [Vec3(e.dxf.vtx0), Vec3(e.dxf.vtx1), Vec3(e.dxf.vtx3), Vec3(e.dxf.vtx2)]
        ocs = OCS(Vec3(e.dxf.get("extrusion", (0, 0, 1))))
        v = [m.transform(ocs.to_wcs(p)) for p in v]
        if (e.dxf.vtx2 == e.dxf.vtx3):
            v = v[:3]
        return "area", abs(ring_area(v))
    if t in ("POLYLINE", "SPLINE"):
        p = ezpath.make_path(e).transform(m)
        pts = [Vec3(v) for v in p.flattening(FLAT)]
        closed = bool(getattr(e, "is_closed", False)) or (t == "ELLIPSE" and abs(
            (e.dxf.end_param - e.dxf.start_param) % (2 * math.pi)) < 1e-9)
        if closed:
            return "area", abs(ring_area(pts))
        return "length", sum((pts[i + 1] - pts[i]).magnitude for i in range(len(pts) - 1))
    return None


def main():
    args = sys.argv[1:]
    tol = 0.001
    md = None
    json_path = None
    if "--json" in args:
        i = args.index("--json"); json_path = args[i + 1]; del args[i:i + 2]
    min_matches = 1
    if "--tol" in args:
        i = args.index("--tol"); tol = float(args[i + 1]); del args[i:i + 2]
    if "--md" in args:
        i = args.index("--md"); md = args[i + 1]; del args[i:i + 2]
    if "--min-matches" in args:
        i = args.index("--min-matches"); min_matches = int(args[i + 1]); del args[i:i + 2]
    if not math.isfinite(tol) or tol < 0 or min_matches < 1:
        raise SystemExit("tol must be finite and non-negative; min-matches must be positive")
    dxf_path, dump_path = args
    expect = {}
    side = dxf_path[:-4] + ".expect.json"
    if os.path.exists(side):  # worked out by hand when the drawing was made (make_synthetic.py)
        expect = {k.upper(): v for k, v in json.load(open(side)).items()}
    doc = ezdxf.readfile(dxf_path)
    units = doc.header.get("$INSUNITS", 0)
    factor = FACTOR.get(units, 0.001)
    # xref documents, by block name (read beside the drawing, like Qiyas does)
    xdocs = {}
    for b in doc.blocks:
        if b.block.dxf.flags & 4:
            cand = os.path.join(os.path.dirname(dxf_path), os.path.basename(b.block.dxf.xref_path or ""))
            if os.path.exists(cand):
                xdocs[b.name.upper()] = ezdxf.readfile(cand)
    rows, bad = [], 0
    for line in open(dump_path):
        r = json.loads(line)
        if r["row"] == "header":
            print("drawing", r["file"], "units", r["units"], "INSUNITS", units, "problems", r["problems"])
            continue
        if r["row"] != "m":
            rows.append((r, None, "n/a", r.get("error") or r.get("outcome")))
            continue
        cur, m, ok = doc, Matrix44(), True
        try:
            for h in r["chain"]:
                ins = cur.entitydb[h]
                m = insert_matrix(cur, ins) @ m  # row vectors: innermost first
                x = xdocs.get(ins.dxf.name.upper())
                if x is not None:
                    cur = x
            ent = cur.entitydb[r["handle"]]
        except Exception as ex:
            rows.append((r, None, "n/a", f"not found: {ex!r}"))
            continue
        exp = expected(cur, ent, m)
        if exp is None:
            rows.append((r, None, "n/a", f"no independent calc for {ent.dxftype()}"))
            continue
        kind, val = exp
        hand = expect.get(">".join(r["chain"] + [r["handle"]]).upper())
        if hand is not None:
            hk, hv = hand
            if hk != kind or abs(hv - val) > tol / 10 / max(factor, 1e-9) ** (2 if kind == "area" else 1):
                rows.append((r, None, "DIFF", f"independent calcs disagree: ezdxf {val} vs by hand {hv}"))
                bad += 1
                continue
        want = val * factor ** (2 if kind == "area" else 1 if kind == "length" else 0)
        got = r["shown"]
        same_kind = {"length": "length", "area": "area", "count": "count"}[kind] == r["kind"].lower()
        diff = abs(got - want)
        status = "ok" if same_kind and math.isfinite(got) and math.isfinite(want) and diff <= tol else "DIFF"
        if status == "DIFF":
            bad += 1
        rows.append((r, want, status, f"{diff:.6f}" + ("" if same_kind else f" kind {r['kind']} vs {kind}")))
    ok = sum(1 for x in rows if x[2] == "ok")
    na = sum(1 for x in rows if x[2] == "n/a")
    print(f"{len(rows)} measured: {ok} match within {tol}, {bad} differ, {na} not comparable")
    from collections import Counter
    for (ty, why), n in sorted(Counter((r["type"], note) for r, _, st, note in rows if st == "n/a").items()):
        print(f"  n/a  {n:4} x {ty:<12} {why[:90]}")
    for r, want, st, note in rows:
        if st == "DIFF":
            print(f"  {st:4} {r['type']:<12} {r['handle']:>6} chain={'>'.join(r.get('chain', []))} "
                  f"qiyas={r.get('shown')} indep={want} {note}")
    numeric = []
    for r, want, st, note in rows:
        got = r.get("shown")
        valid = want is not None and got is not None and math.isfinite(got) and math.isfinite(want)
        diff = abs(got - want) if valid else None
        rel = (diff / abs(want) if want else (0.0 if got == 0 else None)) if valid else None
        numeric.append(dict(entity=">".join(r.get("chain", []) + [r["handle"]]), type=r["type"],
                            qiyas=got, independent=want, absolute_error=diff,
                            absolute_pass=st == "ok", relative_error=rel,
                            relative_pass=rel <= 0.01 if rel is not None else None))
    if json_path:
        with open(json_path, "w") as f:
            json.dump(numeric, f, indent=2, allow_nan=False)
    if md:
        with open(md, "w") as f:
            f.write(f"Tolerance: {tol}. Matches: {ok}. Differences: {bad}. Not comparable: {na}. Required matches: {min_matches}.\n\n")
            f.write("| Entity chain | Type | Qiyas | Independent | Status | Difference or reason | Relative error | Pass 1% |\n|---|---|---|---|---|---|---|---|\n")
            for (r, want, st, note), n in zip(rows, numeric):
                entity = ">".join(r.get("chain", []) + [r["handle"]])
                shown = f"{r['shown']:.6f} {r['unit']}" if "shown" in r else "—"
                independent = f"{want:.6f}" if want is not None else "—"
                note = str(note).replace("|", "\\|").replace("\n", " ")
                relative = f"{n['relative_error']:.12g}" if n['relative_error'] is not None else "—"
                f.write(f"| {entity} | {r['type']} | {shown} | {independent} | {st} | {note} | {relative} | {n['relative_pass']} |\n")
    sys.exit(1 if bad or ok < min_matches else 0)


main()
