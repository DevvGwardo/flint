#!/bin/sh
# Hidden check for rename-across-files. Usage: check.sh <workspace> <fixture-dir>
set -e
cd "$1"
if grep -rn "area_of_rect" --include='*.py' .; then echo "old name still present"; exit 1; fi
test "$(python3 cli.py 3 4)" = "12"
python3 - <<'PY'
import geometry, report
assert geometry.rectangle_area(3, 4) == 12
assert geometry.perimeter_of_rect(3, 4) == 14
assert report.summary([(2, 3), (1, 1)]).endswith("total area 7")
try:
    geometry.rectangle_area(-1, 2)
except ValueError:
    pass
else:
    raise AssertionError("negative sides must still raise")
print("hidden checks passed")
PY
