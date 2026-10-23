#!/usr/bin/env python3
"""Convert reviewed source geometry into metre DXFs and benchmark truth, never holdout."""

import argparse
import csv
import hashlib
import json
import math
import pickle
import re
import sys
from collections import Counter
from functools import lru_cache
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
STORE = ROOT / ".local" / "benchmark"
SOURCES = json.loads(Path(__file__).with_name("sources.json").read_text())["sources"]


def slug(value):
    return re.sub(r"[^a-zA-Z0-9_-]+", "-", str(value)).strip("-") or "unnamed"


@lru_cache(maxsize=8)
def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def verify_input(path, source):
    """Only a source file whose hash is in the downloader's verified log is input."""
    path = path.resolve(strict=True)
    path.relative_to((STORE / "sources" / source).resolve())
    relative = str(path.relative_to(STORE / "sources"))
    records = [json.loads(line) for line in (STORE / "sources" / "provenance.jsonl").read_text().splitlines()]
    matches = [record for record in records if record.get("source") == source
               and record.get("path") == relative and record.get("status") == "verified"]
    if not matches or sha256(path) != matches[-1]["sha256"]:
        raise ValueError("Input is not a verified download; run scripts/benchmark/fetch.sh first")
    return path


def parts(geometry):
    if geometry.geom_type.startswith("Multi") or geometry.geom_type == "GeometryCollection":
        for part in geometry.geoms:
            yield from parts(part)
    elif not geometry.is_empty:
        yield geometry


class Case:
    def __init__(self, case_id, project_type, source, source_file):
        import ezdxf
        self.id = case_id
        self.doc = ezdxf.new("R2010")
        self.doc.units = 6
        self.doc.header["$INSUNITS"] = 6
        self.model = self.doc.modelspace()
        self.items = []
        self.notes = []
        self.source = next(item for item in SOURCES if item["id"] == source)
        self.project_type = project_type
        self.source_file = source_file

    def draw(self, geometry, layer, area_only=False):
        """Preserve holes by tiling their net area; no inferred wall centre lines."""
        from shapely.ops import triangulate
        if layer not in self.doc.layers:
            self.doc.layers.new(layer)
        handles = []
        for part in parts(geometry):
            if area_only and (part.geom_type != "Polygon" or part.area <= 0):
                continue
            if part.geom_type == "Polygon":
                if part.interiors:
                    # Clipped triangles exactly partition the polygon including concave boundaries.
                    # Intersections can also contain boundary-only lines; those cannot cite area.
                    tiles = [piece for triangle in triangulate(part)
                             for piece in parts(triangle.intersection(part))
                             if piece.geom_type == "Polygon" and piece.area > 0]
                    if not math.isclose(sum(tile.area for tile in tiles), part.area,
                                        rel_tol=1e-10, abs_tol=1e-8):
                        raise ValueError("Polygon partition does not reproduce its net area")
                    for tile in tiles:
                        handles.extend(self.draw(tile, layer, area_only=True))
                else:
                    entity = self.model.add_lwpolyline(list(part.exterior.coords)[:-1], close=True, dxfattribs={"layer": layer})
                    handles.append(entity.dxf.handle)
            elif part.geom_type in ("LineString", "LinearRing"):
                entity = self.model.add_lwpolyline(list(part.coords), close=part.is_ring, dxfattribs={"layer": layer})
                handles.append(entity.dxf.handle)
            elif part.geom_type == "Point":
                entity = self.model.add_circle((part.x, part.y), 0.05, dxfattribs={"layer": layer})
                handles.append(entity.dxf.handle)
        return handles

    def item(self, item_id, description, geometry, category, model_id=None, volume=None):
        is_count = any(word in category.lower() for word in ("door", "window", "opening", "feature", "sink", "toilet", "fixture"))
        layer = slug(category).upper()[:80]
        if is_count:
            # Each opening/fixture gets an insert with its actual outline, rather than a generic marker.
            block_name = f"{layer}-{len(self.items):05d}"
            block = self.doc.blocks.new(block_name)
            previous = self.model
            self.model = block
            self.draw(geometry, layer)
            self.model = previous
            handle = self.model.add_blockref(block_name, (0, 0), dxfattribs={"layer": layer}).dxf.handle
            handles = [handle]
            kind, unit, quantity = "count", "pcs", 1.0
        else:
            handles = self.draw(geometry, layer, area_only=geometry.area > 0)
            if geometry.area > 0:
                kind, unit, quantity = "area", "m2", geometry.area
            else:
                kind, unit, quantity = "length", "m", geometry.length
        if not handles or quantity <= 0:
            return
        point = geometry.representative_point()
        item = {"id": str(item_id), "description": description, "kind": kind, "unit": unit,
                "quantity": quantity, "source_objects": handles, "basis": "geometric",
                "tolerance_class": "count" if is_count else "geometry", "position": [point.x, point.y]}
        if model_id:
            item["source_objects"].append("ifc:" + model_id)
        self.items.append(item)
        if volume is not None and volume > 0:
            self.items.append({**item, "id": str(item_id) + "-volume", "description": description + " model volume",
                               "kind": "volume", "unit": "m3", "quantity": volume,
                               "source_objects": ["ifc:" + model_id], "tolerance_class": "geometry"})

    def write(self):
        if not self.items:
            raise ValueError(f"{self.id}: no supported geometry; no empty benchmark case written")
        target = STORE / "cases" / self.id
        if target.exists():
            raise ValueError(f"Case already exists: {target}; choose another --prefix or remove its derived output explicitly")
        target.mkdir(parents=True)
        self.doc.saveas(target / "drawing.dxf")
        truth = {"schema_version": 1, "id": self.id, "project_type": self.project_type,
                 "drawing": "drawing.dxf", "items": self.items, "seeded_errors": [], "rules": {},
                 "source": {"id": self.source["id"], "url": self.source["url"], "license": self.source["license"],
                            "attribution": self.source["attribution"], "file": str(self.source_file.relative_to(STORE)),
                            "sha256": sha256(self.source_file)},
                 "conversion": {"tool": "scripts/benchmark/convert.py", "coordinates": "metres",
                                "notes": self.notes}}
        (target / "truth.json").write_text(json.dumps(truth, indent=2, allow_nan=False) + "\n")
        (target / "ATTRIBUTION.md").write_text(
            f"{self.source['attribution']}\n\n{self.source['url']}\n\n"
            "CC BY 4.0: https://creativecommons.org/licenses/by/4.0/\n\n"
            "Modified for Qiyas: geometry rendered to DXF in metres and measured to produce truth.json. "
            "See truth.json conversion notes; this is dataset geometry, not certified construction quantities.\n")
        return self.id


def swiss(args, path):
    from shapely import from_wkt
    plans = {}
    with path.open(newline="") as stream:
        reader = csv.DictReader(stream)
        required = {args.group_column, args.geometry_column, "entity_type", "entity_subtype"}
        if not required.issubset(reader.fieldnames or []):
            raise ValueError(f"Swiss CSV requires {sorted(required)}; got {reader.fieldnames}. "
                             "Use --group-column / --geometry-column for explicit schema mapping.")
        for index, row in enumerate(reader):
            group = row[args.group_column]
            if not group:
                continue
            if group not in plans:
                if len(plans) >= args.limit:
                    continue
                plans[group] = Case(f"{args.prefix}-{slug(group)}", "apartment-floor", args.source, path)
                plans[group].notes.append("Swiss source-plan apartment areas have up to 5% deviation (median 1.2%); truth measures the released geometry.")
            geometry = from_wkt(row[args.geometry_column])
            if not geometry.is_valid:
                raise ValueError(f"Swiss row {index}: invalid geometry; repair must be reviewed rather than silently changing truth")
            category = row["entity_type"] + "-" + row["entity_subtype"]
            plans[group].item(f"swiss-row-{index}", category, geometry, category)
    return [case.write() for case in plans.values()]


class GeometryUnpickler(pickle.Unpickler):
    """Allow geometry reconstruction, never arbitrary modules/functions in downloaded pickle."""
    ALLOWED = {
        ("shapely.io", "from_wkb"), ("shapely", "from_wkb"),
        ("shapely.geometry.point", "Point"), ("shapely.geometry.linestring", "LineString"),
        ("shapely.geometry.polygon", "Polygon"), ("shapely.geometry.multipolygon", "MultiPolygon"),
        ("shapely.geometry.multilinestring", "MultiLineString"), ("shapely.geometry.multipoint", "MultiPoint"),
        ("shapely.geometry.collection", "GeometryCollection"),
        ("numpy.core.multiarray", "_reconstruct"), ("numpy._core.multiarray", "_reconstruct"),
        ("numpy.core.multiarray", "scalar"), ("numpy._core.multiarray", "scalar"),
        ("numpy", "ndarray"), ("numpy", "dtype"),
        ("networkx.classes.graph", "Graph"), ("networkx.classes.digraph", "DiGraph"),
        ("collections", "defaultdict"), ("builtins", "set"), ("builtins", "frozenset"),
    }

    def find_class(self, module, name):
        if (module, name) not in self.ALLOWED:
            raise pickle.UnpicklingError(f"Unsupported pickle class {module}.{name}; review the upstream format")
        return super().find_class(module, name)


def geometries(value, category="geometry"):
    from shapely.geometry.base import BaseGeometry
    if isinstance(value, BaseGeometry):
        yield category, value
    elif isinstance(value, dict):
        for key, item in value.items():
            if key not in ("graph", "connectivity", "id", "name", "metadata"):
                yield from geometries(item, f"{category}/{key}" if category != "geometry" else str(key))
    elif isinstance(value, (list, tuple)):
        for item in value:
            yield from geometries(item, category)


def resplan(args, path):
    import zipfile
    if path.suffix.lower() == ".zip":
        with zipfile.ZipFile(path) as archive:
            names = [name for name in archive.namelist() if Path(name).name.lower() == "resplan.pkl"]
            if len(names) != 1:
                raise ValueError("Expected exactly one ResPlan.pkl in archive")
            with archive.open(names[0]) as stream:
                data = GeometryUnpickler(stream).load()
    else:
        with path.open("rb") as stream:
            data = GeometryUnpickler(stream).load()
    if isinstance(data, dict):
        data = data.get("plans", data)
    records = data.items() if isinstance(data, dict) else enumerate(data)
    written = []
    skipped = []
    for index, plan in records:
        if len(written) >= args.limit:
            break
        plan_geometry = list(geometries(plan))
        invalid = [category for category, geometry in plan_geometry if not geometry.is_valid]
        if invalid:
            # Reject the whole plan before drawing anything: partial plans would falsify truth.
            reason = "Invalid geometry needs explicit review"
            skipped.append({"plan": str(index), "categories": invalid, "reason": reason})
            print(f"ResPlan {index}: skipped ({', '.join(invalid)}): {reason}", file=sys.stderr)
            continue
        case = Case(f"{args.prefix}-{slug(index)}", "apartment-floor", args.source, path)
        for n, (category, geometry) in enumerate(plan_geometry):
            # Separate instances within opening/fixture multipolygons preserve count truth.
            countable = any(word in category.lower() for word in ("door", "window", "fixture"))
            for part_index, geometry_part in enumerate(parts(geometry) if countable else [geometry]):
                case.item(f"resplan-{index}-{n}-{part_index}", category, geometry_part, category)
        case.notes.append("Coordinates interpreted as metres, as declared by ResPlan. Polygon areas are net of holes; no wall centreline is inferred.")
        written.append(case.write())
    report = {"cases": written, "skipped_plans": skipped}
    audit = STORE / "sources" / args.source / (path.stem + ".conversion.json")
    audit.write_text(json.dumps(report, indent=2) + "\n")
    return report


def footprint(vertices, faces):
    from shapely.geometry import Polygon
    from shapely.ops import unary_union
    triangles = []
    for a, b, c in faces:
        triangle = Polygon([vertices[a][:2], vertices[b][:2], vertices[c][:2]])
        if triangle.area > 1e-12:
            triangles.append(triangle)
    return unary_union(triangles)


def section(vertices, faces, height):
    from shapely.geometry import LineString, Polygon
    from shapely.ops import polygonize, unary_union
    lines = []
    for triangle in faces:
        points = []
        for a, b in zip(triangle, triangle[1:] + triangle[:1]):
            p, q = vertices[a], vertices[b]
            if (p[2] <= height < q[2]) or (q[2] <= height < p[2]):
                t = (height - p[2]) / (q[2] - p[2])
                points.append((round(p[0] + t * (q[0] - p[0]), 8), round(p[1] + t * (q[1] - p[1]), 8)))
        if len(points) == 2 and points[0] != points[1]:
            lines.append(LineString(points))
    cells = list(polygonize(unary_union(lines)))
    # Polygonize also returns the inner face of a void. Keep odd nesting depths.
    rings = [Polygon(cell.exterior) for cell in cells]
    return unary_union([cell for cell in cells
                        if sum(ring.contains(cell.representative_point()) for ring in rings) % 2])


def volume(vertices, faces):
    edges = Counter(tuple(sorted(edge)) for triangle in faces for edge in zip(triangle, triangle[1:] + triangle[:1]))
    if any(count != 2 for count in edges.values()):
        return None
    # Closed oriented triangle meshes: signed tetrahedra against the model origin.
    signed = 0.0
    for a, b, c in faces:
        p, q, r = vertices[a], vertices[b], vertices[c]
        signed += (p[0] * (q[1] * r[2] - q[2] * r[1])
                   - p[1] * (q[0] * r[2] - q[2] * r[0])
                   + p[2] * (q[0] * r[1] - q[1] * r[0])) / 6
    return abs(signed)


def ifc(args, path):
    import ifcopenshell
    import ifcopenshell.geom
    import ifcopenshell.util.element
    import ifcopenshell.util.placement
    import ifcopenshell.util.unit
    model = ifcopenshell.open(str(path))
    settings = ifcopenshell.geom.settings()
    settings.set(settings.USE_WORLD_COORDS, True)
    unit_scale = ifcopenshell.util.unit.calculate_unit_scale(model)
    storeys = {s.id(): s for s in model.by_type("IfcBuildingStorey")}
    cases = {}
    failures = []
    for product in model.by_type("IfcProduct"):
        if not product.Representation:
            continue
        storey = ifcopenshell.util.element.get_container(product)
        if product.is_a("IfcSpace"):
            storey = ifcopenshell.util.element.get_aggregate(product)
        while storey is not None and not storey.is_a("IfcBuildingStorey"):
            storey = ifcopenshell.util.element.get_aggregate(storey)
        if storey is None or storey.id() not in storeys:
            continue
        if storey.id() not in cases:
            if len(cases) >= args.limit:
                continue
            cases[storey.id()] = Case(f"{args.prefix}-{slug(path.stem)}-{slug(storey.GlobalId)}",
                                     "clinic" if "clinic" in args.source else "apartment-floor", args.source, path)
        case = cases[storey.id()]
        try:
            shape = ifcopenshell.geom.create_shape(settings, product)
            raw = shape.geometry
            vertices = [tuple(raw.verts[i:i + 3]) for i in range(0, len(raw.verts), 3)]
            faces = [list(raw.faces[i:i + 3]) for i in range(0, len(raw.faces), 3)]
            if not vertices:
                continue
            elevation = float(ifcopenshell.util.placement.get_local_placement(storey.ObjectPlacement)[2, 3]) * unit_scale
            cut = section(vertices, faces, elevation + args.cut_height)
            projected = footprint(vertices, faces)
            # Slabs and low fixtures lie below the cut; retain their projected silhouettes.
            geometry = cut if not cut.is_empty else projected
            if geometry.is_empty:
                failures.append({"model_id": product.GlobalId, "reason": "No planar section or footprint"})
                continue
            amount = volume(vertices, faces) if product.is_a() in ("IfcWall", "IfcWallStandardCase", "IfcSlab", "IfcBeam", "IfcColumn") else None
            case.item(product.GlobalId, f"{product.is_a()} {product.Name or product.GlobalId}", geometry,
                      product.is_a(), product.GlobalId, amount)
        except (RuntimeError, ValueError) as error:
            failures.append({"model_id": product.GlobalId, "reason": str(error)})
    result = []
    for case in cases.values():
        if not case.items:
            continue
        case.notes.extend([
            f"Storey section at +{args.cut_height} m, projected silhouette for objects outside the cut; IfcOpenShell world coordinates in metres.",
            "Truth comes from triangulated model geometry, not IFC quantity properties. Curved surfaces inherit meshing precision.",
            "Volume requires the cited IFC model and a closed triangle mesh; its 2D footprint alone is not a volume trace.",
            "COBie count comparison and independent 10% hand-check remain required before treating these as reviewed gold cases.",
        ])
        result.append(case.write())
    audit = STORE / "sources" / args.source / (path.stem + ".conversion.json")
    audit.write_text(json.dumps({"cases": result, "unconverted_objects": failures}, indent=2) + "\n")
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", choices=[s["id"] for s in SOURCES])
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--limit", type=int, default=150)
    parser.add_argument("--prefix")
    parser.add_argument("--group-column", default="plan_id")
    parser.add_argument("--geometry-column", default="geometry")
    parser.add_argument("--cut-height", type=float, default=1.2)
    args = parser.parse_args()
    if args.limit < 1 or not math.isfinite(args.cut_height) or args.cut_height <= 0:
        parser.error("--limit and --cut-height must be positive")
    args.prefix = slug(args.prefix or args.source)
    path = verify_input(args.input, args.source)
    if args.source == "resplan":
        report = resplan(args, path)
    else:
        converter = swiss if args.source == "swiss-dwellings" else ifc
        report = {"cases": converter(args, path)}
    written = report["cases"]
    if not written:
        raise ValueError("No cases converted; inspect the publisher format before using this source")
    print(json.dumps({"source": args.source, **report, "count": len(written)}, indent=2))


if __name__ == "__main__":
    try:
        main()
    except (ValueError, ImportError, OSError, pickle.UnpicklingError) as error:
        sys.exit(f"benchmark converter: {error}")
