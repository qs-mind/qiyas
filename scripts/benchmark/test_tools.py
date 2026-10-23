"""Offline checks for download verification and conversion's non-library primitives."""

import hashlib
import importlib.util
import io
import json
import pickle
import sys
import tempfile
import types
import unittest
from pathlib import Path
from unittest.mock import patch


def module(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + ".py"))
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


fetch = module("fetch")
convert = module("convert")


class SourceToolsTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.store = Path(self.temporary.name)
        self.source = fetch.CATALOG["sources"][0]

    def test_verified_download_records_digest_and_attribution(self):
        payload = b"reviewed model bytes"
        digest = hashlib.sha256(payload).hexdigest()
        target = self.store / "model.ifc"
        with patch.object(fetch, "STORE", self.store), patch.object(fetch, "read_url", return_value=io.BytesIO(payload)):
            fetch.fetch_file(self.source, "https://example.test/model", target, digest)
        record = json.loads((self.store / "provenance.jsonl").read_text())
        self.assertEqual(target.read_bytes(), payload)
        self.assertEqual(record["sha256"], digest)
        self.assertEqual(record["license"], "CC-BY-4.0")
        self.assertTrue(record["attribution"])

    def test_corrupted_download_is_never_published(self):
        target = self.store / "model.ifc"
        with patch.object(fetch, "STORE", self.store), patch.object(fetch, "read_url", return_value=io.BytesIO(b"wrong")):
            with self.assertRaisesRegex(ValueError, "Checksum mismatch"):
                fetch.fetch_file(self.source, "https://example.test/model", target, "0" * 64)
        self.assertFalse(target.exists())
        self.assertFalse(target.with_suffix(".ifc.part").exists())

    def test_git_blob_digest_includes_header(self):
        payload = b"original notice"
        digest = hashlib.sha1(f"blob {len(payload)}\0".encode() + payload).hexdigest()
        with patch.object(fetch, "STORE", self.store), patch.object(fetch, "read_url", return_value=io.BytesIO(payload)):
            fetch.fetch_file(self.source, "https://example.test/notice", self.store / "notice", digest, "sha1", git_blob=True)
        self.assertTrue((self.store / "notice").exists())

    def test_github_only_fetches_reviewed_project_paths(self):
        prefix = self.source["directory"]
        tree = {"tree": [{"type": "blob", "path": path, "sha": "a" * 40} for path in (
            "LICENSE", prefix + "/model.ifc", prefix + "/README.md",
            "IFC 2.3.0.1 (IFC 2x3)/Schependomlaan/restricted.ifc",
            "IFC 2.3.0.1 (IFC 2x3)/SDK - S1/restricted.ifc",
        )]}
        with patch.object(fetch, "metadata", side_effect=[{"default_branch": "main"}, {"sha": "b" * 40}, tree]), \
                patch.object(fetch, "github_blob") as download, patch.object(fetch, "log"):
            fetch.github(self.source, types.SimpleNamespace(resplan_url=None, resplan_sha256=None))
        paths = [call.args[2]["path"] for call in download.call_args_list]
        self.assertEqual(paths, ["LICENSE", prefix + "/model.ifc", prefix + "/README.md"])

    def test_unknown_zenodo_licence_stops_before_file_download(self):
        source = fetch.CATALOG["sources"][-1]
        with patch.object(fetch, "metadata", return_value={"metadata": {"license": {"id": "cc-by-nc-4.0"}}}), \
                patch.object(fetch, "fetch_file") as download:
            with self.assertRaisesRegex(ValueError, "Unexpected Zenodo licence"):
                fetch.swiss(source)
        download.assert_not_called()

    def test_closed_mesh_volume_and_open_mesh_rejection(self):
        vertices = [(10, 10, 10), (11, 10, 10), (10, 11, 10), (10, 10, 11)]
        faces = [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]]
        self.assertAlmostEqual(convert.volume(vertices, faces), 1 / 6)
        self.assertIsNone(convert.volume(vertices, faces[:-1]))

    def test_pickle_cannot_load_arbitrary_functions(self):
        with self.assertRaisesRegex(pickle.UnpicklingError, "Unsupported pickle class"):
            convert.GeometryUnpickler(io.BytesIO(pickle.dumps(eval))).load()

    def test_resplan_rejects_whole_invalid_plan_and_reports_it_then_continues(self):
        source = self.store / "sources" / "resplan" / "ResPlan.pkl"
        source.parent.mkdir(parents=True)
        source.write_bytes(pickle.dumps({"plans": {"6": "valid", "7": "invalid", "8": "valid", "9": "past-limit"}}))
        valid = types.SimpleNamespace(is_valid=True)
        invalid = types.SimpleNamespace(is_valid=False)
        started, drawn = [], []

        class FakeCase:
            def __init__(self, case_id, *_):
                self.id, self.notes = case_id, []
                started.append(case_id)

            def item(self, *args):
                drawn.append(args)

            def write(self):
                target = convert.STORE / "cases" / self.id
                target.mkdir(parents=True)
                (target / "truth.json").write_text("{}")
                return self.id

        def geometry(plan):
            self.assertNotEqual(plan, "past-limit")
            # The bad door follows a usable wall: no partial case may escape.
            return [("wall", valid), ("door", invalid)] if plan == "invalid" else [("wall", valid)]

        output, errors = io.StringIO(), io.StringIO()
        args = ["convert.py", "resplan", "--input", str(source), "--limit", "2"]
        with patch.object(convert, "STORE", self.store), patch.object(convert, "Case", FakeCase), \
                patch.object(convert, "geometries", side_effect=geometry), \
                patch.object(convert, "verify_input", return_value=source), \
                patch.object(sys, "argv", args), patch.object(sys, "stdout", output), patch.object(sys, "stderr", errors):
            convert.main()
        self.assertEqual(started, ["resplan-6", "resplan-8"])
        self.assertEqual(len(drawn), 2)
        self.assertFalse((self.store / "cases" / "resplan-7").exists())
        report = json.loads(output.getvalue())
        self.assertEqual(report["cases"], started)
        self.assertEqual(report["count"], 2)
        self.assertEqual(report["skipped_plans"], [{
            "plan": "7", "categories": ["door"], "reason": "Invalid geometry needs explicit review",
        }])
        audit = json.loads(source.with_suffix(".conversion.json").read_text())
        self.assertEqual(audit, {key: report[key] for key in ("cases", "skipped_plans")})
        self.assertIn("ResPlan 7: skipped (door)", errors.getvalue())

    def test_resplan_reports_all_invalid_plans_without_writing_cases(self):
        source = self.store / "sources" / "resplan" / "ResPlan.pkl"
        source.parent.mkdir(parents=True)
        source.write_bytes(pickle.dumps(["invalid"]))
        args = types.SimpleNamespace(prefix="resplan", source="resplan", limit=2)
        with patch.object(convert, "STORE", self.store), patch.object(convert, "Case") as case, \
                patch.object(convert, "geometries", return_value=[("door", types.SimpleNamespace(is_valid=False))]), \
                patch.object(sys, "stderr", io.StringIO()):
            report = convert.resplan(args, source)
        case.assert_not_called()
        self.assertEqual(report["cases"], [])
        self.assertEqual(report["skipped_plans"][0]["plan"], "0")
        self.assertEqual(json.loads(source.with_suffix(".conversion.json").read_text()), report)

    def test_hole_area_citations_sum_net_tiles_and_exclude_boundary_lines(self):
        # GEOS can return both polygons and boundary lines from clipped triangles.
        # Supply that boundary result without requiring optional geometry packages.
        def polygon(x0, y0, x1, y1):
            return types.SimpleNamespace(
                geom_type="Polygon", is_empty=False, interiors=[],
                area=(x1 - x0) * (y1 - y0),
                exterior=types.SimpleNamespace(coords=[(x0, y0), (x1, y0), (x1, y1), (x0, y1), (x0, y0)]),
            )
        tiles = [polygon(0, 0, 4, 1), polygon(0, 3, 4, 4), polygon(0, 1, 1, 3), polygon(3, 1, 4, 3)]
        boundary = types.SimpleNamespace(geom_type="LineString", is_empty=False, area=0)
        clipped = types.SimpleNamespace(geom_type="GeometryCollection", geoms=[*tiles, boundary])
        triangle = types.SimpleNamespace(intersection=lambda _: clipped)
        original = types.SimpleNamespace(geom_type="Polygon", is_empty=False, interiors=[object()], area=12)
        drawn = []

        def add_polyline(vertices, close, dxfattribs):
            self.assertTrue(close)
            drawn.append(vertices)
            return types.SimpleNamespace(dxf=types.SimpleNamespace(handle=f"{len(drawn):X}"))

        case = convert.Case.__new__(convert.Case)
        case.doc = types.SimpleNamespace(layers={"ROOM"})
        case.model = types.SimpleNamespace(add_lwpolyline=add_polyline)
        ops = types.ModuleType("shapely.ops")
        ops.triangulate = lambda _: [triangle]
        with patch.dict(sys.modules, {"shapely.ops": ops}):
            handles = case.draw(original, "ROOM", area_only=True)
        self.assertEqual(handles, ["1", "2", "3", "4"])
        # Same sum-of-positive-areas convention as the Rust handle remeasurement.
        area = sum(abs(sum(x0 * y1 - x1 * y0 for (x0, y0), (x1, y1)
                           in zip(points, points[1:] + points[:1]))) / 2 for points in drawn)
        self.assertEqual(area, original.area)
        original.area = 13
        with patch.dict(sys.modules, {"shapely.ops": ops}):
            with self.assertRaisesRegex(ValueError, "does not reproduce"):
                case.draw(original, "ROOM", area_only=True)


if __name__ == "__main__":
    unittest.main()
