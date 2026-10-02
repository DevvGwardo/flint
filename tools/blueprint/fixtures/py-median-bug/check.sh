#!/bin/sh
# Hidden check for py-median-bug. Usage: check.sh <workspace> <fixture-dir>
set -e
cd "$1"
cmp -s test_stats.py "$2/repo/test_stats.py" || { echo "test_stats.py was modified"; exit 1; }
python3 -m unittest -q test_stats
python3 - <<'PY'
from stats import median
assert median([1, 2, 3, 4]) == 2.5, median([1, 2, 3, 4])
assert median([10, 2, 38, 23, 38, 23, 21]) == 23
assert median([7]) == 7
assert median([2.0, 1.0]) == 1.5
try:
    median([])
except ValueError:
    pass
else:
    raise AssertionError("median([]) must raise ValueError")
print("hidden checks passed")
PY
