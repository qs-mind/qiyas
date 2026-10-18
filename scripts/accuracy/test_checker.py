"""Regression checks for the comparison gate. Run with Python + ezdxf."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import ezdxf

CHECKER = Path(__file__).with_name('check_accuracy.py')


class ComparisonGate(unittest.TestCase):
    def test_relative_gate_uses_full_precision(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            doc = ezdxf.new()
            doc.header['$INSUNITS'] = 6
            e = doc.modelspace().add_line((0, 0), (0.009893, 0))
            drawing = root / 'small.dxf'
            doc.saveas(drawing)
            dump, numeric = root / 'dump.jsonl', root / 'numeric.json'
            dump.write_text(json.dumps(dict(row='m', handle=e.dxf.handle, chain=[],
                type='Line', kind='length', shown=0.009770, unit='m')) + '\n')
            result = subprocess.run([sys.executable, str(CHECKER), str(drawing),
                str(dump), '--json', str(numeric)], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            row = json.loads(numeric.read_text())[0]
            self.assertTrue(row['absolute_pass'])
            self.assertFalse(row['relative_pass'])
            self.assertAlmostEqual(row['relative_error'], 0.000123 / 0.009893)

    def test_evidence_gate_and_failure_rows(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            doc = ezdxf.new()
            doc.header['$INSUNITS'] = 6
            entity = doc.modelspace().add_line((0, 0), (3, 4))
            drawing = root / 'line.dxf'
            doc.saveas(drawing)
            row = dict(row='m', handle=entity.dxf.handle, chain=[], type='Line',
                       kind='Length', shown=5.0, unit='m')
            cases = [
                ('empty', [], 1, None, []),
                ('unsupported', [dict(row, row='error', error='unsupported')], 1, 'n/a', []),
                ('match', [row], 0, 'ok', []),
                ('difference', [dict(row, shown=5.01)], 1, 'DIFF', []),
                ('nan', [dict(row, shown=float('nan'))], 1, 'DIFF', []),
                ('coverage', [row], 1, 'ok', ['--min-matches', '2']),
            ]
            for name, rows, code, status, extra in cases:
                with self.subTest(name=name):
                    dump, report = root / 'dump.jsonl', root / 'report.md'
                    dump.write_text(''.join(json.dumps(r) + '\n' for r in rows))
                    result = subprocess.run(
                        [sys.executable, str(CHECKER), str(drawing), str(dump),
                         '--md', str(report), *extra], capture_output=True, text=True)
                    self.assertEqual(result.returncode, code, result.stdout + result.stderr)
                    if status:
                        self.assertIn(f'| {status} |', report.read_text())


if __name__ == '__main__':
    unittest.main()
