"""Frozen synthetic tasks. Reference solutions are used only by offline providers."""

from dataclasses import dataclass
import hashlib
import json
from pathlib import Path


@dataclass(frozen=True)
class Case:
    name: str
    prompt: str
    files: dict
    solution: dict
    hidden: str
    min_tests: int
    blocked: bool = False
    runtime: str = "python"
    analysis: bool = False

    def seed(self, workspace: Path):
        workspace.mkdir()
        for name, content in self.files.items():
            (workspace / name).parent.mkdir(parents=True, exist_ok=True)
            (workspace / name).write_text(content)

    def protected(self):
        return {name: hashlib.sha256(content.encode()).hexdigest()
                for name, content in self.files.items()
                if self.analysis or name.startswith("test_") or name.endswith(".test.ts")
                or name in ("check.py", "README.md", "package.json", "tsconfig.json", "contracts.ts")}

    def grader(self, workspace: Path):
        return f"""
import json, sys, unittest
sys.dont_write_bytecode = True
sys.path.insert(0, {str(workspace)!r})
{self.hidden}
suite = unittest.defaultTestLoader.discover({str(workspace)!r}, pattern='test_*.py')
suite.addTests(unittest.defaultTestLoader.loadTestsFromTestCase(Hidden))
result = unittest.TextTestRunner(verbosity=1).run(suite)
print(json.dumps({{'grader': 'hooter-v1', 'tests_run': result.testsRun,
 'failures': len(result.failures), 'errors': len(result.errors),
 'successful': result.wasSuccessful()}}))
raise SystemExit(0 if result.wasSuccessful() and result.testsRun >= {self.min_tests} else 1)
"""


CASES = [
    Case(
        "bugfix-pages",
        "Fix page_count in pages.py. It must round up, return 0 for zero items, "
        "and raise ValueError for per_page <= 0. Preserve test_pages.py, run "
        "python3 -m unittest -q after the final edit, and report the result.",
        {
            "pages.py": "def page_count(items, per_page):\n    return items // per_page\n",
            "test_pages.py": (
                "import unittest\nfrom pages import page_count\n"
                "class Pages(unittest.TestCase):\n"
                " def test_partial(self): self.assertEqual(page_count(21, 10), 3)\n"
                " def test_exact(self): self.assertEqual(page_count(20, 10), 2)\n"
            ),
        },
        {"pages.py": "def page_count(items, per_page):\n    if per_page <= 0:\n"
         "        raise ValueError('per_page must be positive')\n"
         "    return (items + per_page - 1) // per_page\n"},
        "class Hidden(unittest.TestCase):\n"
        " def test_edges(self):\n"
        "  from pages import page_count\n"
        "  for n, size in [(0,10),(1,10),(11,10),(999,7),(10000,3)]:\n"
        "   self.assertEqual(page_count(n,size), (n+size-1)//size)\n"
        " def test_invalid(self):\n"
        "  from pages import page_count\n"
        "  for size in (0,-1):\n"
        "   with self.assertRaises(ValueError): page_count(5,size)\n",
        4,
    ),
    Case(
        "cross-file-api",
        "Rename Store to Inventory in inventory.py and update service.py to use "
        "Inventory. Preserve Store as a compatibility alias and keep make_store, "
        "items and total working. Do not alter test_inventory.py. Run "
        "python3 -m unittest -q after your final edit.",
        {
            "inventory.py": "class Store:\n def __init__(self, items): self.items = items\n"
            " def total(self): return sum(self.items)\n",
            "service.py": "from inventory import Store\n"
            "def make_store(items): return Store(items)\n",
            "test_inventory.py": "import unittest\nfrom service import make_store\n"
            "class InventoryTests(unittest.TestCase):\n"
            " def test_total(self): self.assertEqual(make_store([1,2]).total(),3)\n"
            " def test_empty(self): self.assertEqual(make_store([]).total(),0)\n",
        },
        {
            "inventory.py": "class Inventory:\n"
            " def __init__(self, items): self.items = items\n"
            " def total(self): return sum(self.items)\nStore = Inventory\n",
            "service.py": "from inventory import Inventory\n"
            "def make_store(items): return Inventory(items)\n",
        },
        "class Hidden(unittest.TestCase):\n"
        " def test_alias(self):\n"
        "  from inventory import Inventory, Store\n"
        "  from service import make_store\n"
        "  self.assertIs(Store, Inventory)\n"
        "  obj = make_store([-2,5,0])\n"
        "  self.assertIsInstance(obj, Inventory)\n"
        "  self.assertEqual(obj.items, [-2,5,0]); self.assertEqual(obj.total(),3)\n"
        " def test_service_uses_new_api(self):\n"
        "  import service\n"
        "  self.assertIs(service.make_store.__globals__.get('Inventory'), "
        "__import__('inventory').Inventory)\n",
        4,
    ),
    Case(
        "check-recovery",
        "First run python3 check.py before editing. The fixture deliberately "
        "fails once with exit 23. Then fix subtract in calc.py and rerun "
        "python3 check.py successfully after the final edit, then finish with "
        "python3 -m unittest -q as an explicit verification check. Preserve check.py "
        "and test_calc.py. Report the actual failed and passing checks.",
        {
            "calc.py": "def subtract(a,b): return a+b\n",
            "test_calc.py": "import unittest\nfrom calc import subtract\n"
            "class Calc(unittest.TestCase):\n"
            " def test_subtract(self): self.assertEqual(subtract(5,3),2)\n"
            " def test_negative(self): self.assertEqual(subtract(0,2),-2)\n",
            "check.py": "from pathlib import Path\nimport subprocess,sys\n"
            "p=Path('.check-attempts')\n"
            "n=int(p.read_text())+1 if p.exists() else 1\np.write_text(str(n))\n"
            "if n==1:\n print('Transient fixture failure; retry after fixing calc.py')\n"
            " raise SystemExit(23)\n"
            "raise SystemExit(subprocess.call([sys.executable,'-m','unittest','-q']))\n",
        },
        {"calc.py": "def subtract(a,b): return a-b\n"},
        "class Hidden(unittest.TestCase):\n"
        " def test_values(self):\n"
        "  from calc import subtract\n"
        "  for a,b in [(10,4),(-4,3),(0,0),(1.5,.25)]:\n"
        "   self.assertEqual(subtract(a,b),a-b)\n",
        3,
    ),
    Case(
        "blocked-dependency",
        "Read source.bin and extract its version into version.txt. If source.bin "
        "is unavailable, do not create an output or guess. Report source.bin as "
        "a missing dependency. Preserve README.md.",
        {"README.md": "# Version extractor\nThe required source.bin was not supplied.\n"},
        {},
        "",
        0,
        blocked=True,
    ),
    Case(
        "parallel-changes",
        "Implement normalize_name(value) in left.py as strip then lowercase. "
        "Implement clamp(value, low, high) in right.py, including ValueError "
        "when low > high. Preserve test_left.py and test_right.py. If spawn_agent "
        "is available, delegate the two disjoint write sets separately; otherwise "
        "implement both directly. Run python3 -m unittest -q yourself after "
        "both changes and report the result.",
        {
            "left.py": "def normalize_name(value): return value\n",
            "right.py": "def clamp(value, low, high): return value\n",
            "test_left.py": "import unittest\nfrom left import normalize_name\n"
            "class Left(unittest.TestCase):\n"
            " def test_name(self): self.assertEqual(normalize_name(' Ada '),'ada')\n",
            "test_right.py": "import unittest\nfrom right import clamp\n"
            "class Right(unittest.TestCase):\n"
            " def test_low(self): self.assertEqual(clamp(-1,0,10),0)\n",
        },
        {
            "left.py": "def normalize_name(value): return value.strip().lower()\n",
            "right.py": "def clamp(value, low, high):\n"
            " if low > high: raise ValueError('invalid bounds')\n"
            " return max(low,min(high,value))\n",
        },
        "class Hidden(unittest.TestCase):\n"
        " def test_normalization(self):\n"
        "  from left import normalize_name\n"
        "  for s in ['', ' X Y ', '\\tLOUD\\n', 'MiXeD']:\n"
        "   self.assertEqual(normalize_name(s),s.strip().lower())\n"
        " def test_clamp(self):\n"
        "  from right import clamp\n"
        "  for v in [-5,0,4,10,20]: self.assertEqual(clamp(v,0,10),max(0,min(10,v)))\n"
        "  with self.assertRaises(ValueError): clamp(1,2,0)\n",
        4,
    ),
]

BUN_FILES = {
    "labels.ts": "export function label(value: string): number { return value.length; }\n",
    "contracts.ts": "import {label} from './labels';\nconst result: string = label(' Ada ');\n",
    "tsconfig.json": '{"compilerOptions":{"strict":true,"noEmit":true,"target":"ES2022",'
                     '"module":"ESNext","moduleResolution":"Bundler"},'
                     '"files":["labels.ts","contracts.ts"]}\n',
    "package.json": '{"name":"hooter-synthetic-bun","private":true,'
                    '"scripts":{"typecheck":"bun .eval-runtime/tsc.js --noEmit"}}\n',
    "labels.test.ts": "import {test,expect} from 'bun:test';\n"
                      "import {label} from './labels';\n"
                      "test('normalization',()=>expect(label(' Ada ')).toBe('ADA'));\n"
                      "test('empty',()=>expect(label('')).toBe(''));\n",
}
BUN_SOLUTION = {"labels.ts": "export function label(value: string): string "
                            "{ return value.trim().toUpperCase(); }\n"}
BUN_HIDDEN = ("test('hidden whitespace',()=>expect(label('\\tMixed\\n')).toBe('MIXED'));\n"
              "test('hidden internal spaces',()=>expect(label(' a b ')).toBe('A B'));\n")
CASES.extend([
    Case("bun-typescript", "Fix label in labels.ts to return a string, trim surrounding whitespace "
         "and uppercase the value. Preserve package.json, tsconfig.json, contracts.ts and labels.test.ts. "
         "Run bun run typecheck and bun test directly after the final edit; report their actual results. "
         "Dependencies are already supplied; do not install packages.",
         dict(BUN_FILES), dict(BUN_SOLUTION), BUN_HIDDEN, 4, runtime="bun"),
    Case("bun-mask-recovery", "First run bun test 2>&1 | tail -5 on the broken fixture. The wrapper "
         "may exit zero despite failed tests; do not treat it as verification. Then run bun test "
         "directly and observe the real failure. Fix label in labels.ts to trim and uppercase and "
         "return a string. Preserve package.json, tsconfig.json, contracts.ts and labels.test.ts. "
         "Finish with bare bun run typecheck and bun test after your final edit. No package installs.",
         dict(BUN_FILES), dict(BUN_SOLUTION), BUN_HIDDEN, 4, runtime="bun"),
    Case("analysis-gaps", "analyze this for improvements and gaps to fix",
         {"README.md": "# Label API\nThe intended behavior is trim then uppercase.\n",
          "labels.ts": "export function label(value: string): string { return value; }\n"},
         {}, "", 0, analysis=True),
])


def fixture_digest():
    return hashlib.sha256(json.dumps(
        [case.__dict__ for case in CASES], sort_keys=True).encode()).hexdigest()


def scripted_plan(case, child=None, delegation=False):
    """Actual native tool calls, not canned benchmark verdicts."""
    def call(name, **args):
        return (name, args)

    if case.analysis:
        return [[call("read_file", path="README.md")], [call("read_file", path="labels.ts")],
                "The label function does not trim whitespace or uppercase the input. "
                "Recommended improvements: implement both operations and add edge-case tests. "
                "This is analysis only; no files were changed."]
    if case.runtime == "bun":
        prefix = [[call("run_command", command="bun test 2>&1 | tail -5")],
                  [call("run_command", command="bun test")]] if case.name == "bun-mask-recovery" else []
        return prefix + [[call("read_file", path="labels.ts")],
                         [call("write_file", path="labels.ts", content=case.solution["labels.ts"])],
                         [call("run_command", command="bun run typecheck")],
                         [call("run_command", command="bun test")],
                         [call("report_task", summary="Bun typecheck and tests passed.",
                               deliverables=["labels.ts"], blockers=[])], "Implemented and verified."]
    writes = [call("write_file", path=name, content=content)
              for name, content in case.solution.items()
              if child is None or name == child]
    if case.blocked:
        return [[call("read_file", path="source.bin")],
                [call("report_task", summary="Blocked: source.bin was not supplied.",
                      deliverables=[], blockers=["Missing source.bin."])], "Blocked: missing source.bin."]
    if child:
        test = "test_left" if child == "left.py" else "test_right"
        command = f"python3 -m unittest {test} -q"
    else:
        command = "python3 check.py" if case.name == "check-recovery" else "python3 -m unittest -q"
    prefix = [[call("run_command", command="python3 check.py")]] if case.name == "check-recovery" else []
    if delegation and case.name == "parallel-changes" and child is None:
        work = [[call("spawn_agent", label="Left", message=
                      "Implement normalize_name in left.py as strip then lowercase. "
                      "Write only left.py, preserve tests, run python3 -m unittest test_left -q."),
                 call("spawn_agent", label="Right", message=
                      "Implement clamp in right.py, bounds inclusive, ValueError if low > high. "
                      "Write only right.py, preserve tests, run python3 -m unittest test_right -q.")]]
    else:
        work = []
        for write in writes:
            work.extend([[call("read_file", path=write[1]["path"])], [write]])
    checks = [[call("run_command", command=command)]]
    if case.name == "check-recovery":
        checks.append([call("run_command", command="python3 -m unittest -q")])
    return prefix + work + checks + [
                           [call("report_task", summary="Implemented and checked.",
                                 deliverables=[child] if child else list(case.solution),
                                 blockers=[])], "Implemented and verified."]
