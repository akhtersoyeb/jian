#!/usr/bin/env bash
# Jian Code eval: 10 tasks on a tiny Python fixture.
# Usage: cargo build --release && eval/run.sh      (JIAN=/path/to/jian to override)
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
JIAN=${JIAN:-"$HERE/../target/release/jian"}
TASK_TIMEOUT=${TASK_TIMEOUT:-300}
LOGS="$HERE/logs"
mkdir -p "$LOGS"
command -v python3 >/dev/null || { echo "python3 is required"; exit 1; }
[ -x "$JIAN" ] || { echo "build first: cargo build --release"; exit 1; }
TIMEOUT_BIN=$(command -v timeout || command -v gtimeout || true)

make_fixture() {
  local d=$1
  mkdir -p "$d/utils" "$d/tests"
  cat > "$d/calc.py" <<'EOF'
def add(a, b):
    return a + b


def sub(a, b):
    return a - b


def mul(a, b):
    return a * b


def div(a, b):
    return a / b
EOF
  cat > "$d/cli.py" <<'EOF'
import argparse

from calc import add, sub, mul, div

OPS = {"add": add, "sub": sub, "mul": mul, "div": div}


def is_palindrome(s):
    s = "".join(ch.lower() for ch in s if ch.isalnum())
    return s == s[::-1]


def main(argv=None):
    parser = argparse.ArgumentParser(prog="tinycalc")
    parser.add_argument("op", choices=sorted(OPS))
    parser.add_argument("a", type=float)
    parser.add_argument("b", type=float)
    parser.add_argument("--precision", type=int, default=6)
    args = parser.parse_args(argv)
    result = OPS[args.op](args.a, args.b)
    print(f"{result:.{args.precision}f}")


if __name__ == "__main__":
    main()
EOF
  cat > "$d/utils/strings.py" <<'EOF'
import re


def slugify(text):
    text = text.lower().strip()
    text = re.sub(r"[^a-z0-9]+", "-", text)
    return text.strip("-")


def truncate(text, n):
    if len(text) <= n:
        return text
    return text[: n - 3] + "..."


def count_words(text):
    return len(text.split())
EOF
  : > "$d/utils/__init__.py"
  : > "$d/tests/__init__.py"
  cat > "$d/tests/test_calc.py" <<'EOF'
import unittest

from calc import add, sub, mul, div


class TestCalc(unittest.TestCase):
    def test_add(self):
        self.assertEqual(add(2, 3), 5)

    def test_sub(self):
        self.assertEqual(sub(5, 3), 2)

    def test_mul(self):
        self.assertEqual(mul(4, 3), 12)

    def test_div(self):
        self.assertEqual(div(6, 3), 2)

    def test_div_by_zero(self):
        with self.assertRaises(ValueError):
            div(1, 0)


if __name__ == "__main__":
    unittest.main()
EOF
  cat > "$d/README.md" <<'EOF'
# tinycalc

A tiny calculator CLI.

Usage: `python3 cli.py add 1 2`

String helpers live in `utils/strings.py` (for example `slugify`).
EOF
  (cd "$d" && git init -q && git add -A &&
   git -c user.name=eval -c user.email=eval@example.com commit -qm init)
}

# ---- verifiers (run inside the fixture dir; $OUT = jian's stdout) ----
all_docstrings() {
  python3 - "$1" <<'PY'
import ast, sys
tree = ast.parse(open(sys.argv[1]).read())
fns = [n for n in ast.walk(tree) if isinstance(n, ast.FunctionDef)]
sys.exit(0 if fns and all(ast.get_docstring(f) for f in fns) else 1)
PY
}
check_path()    { grep -q "utils/strings.py" "$OUT"; }
check_four()    { grep -qw 4 "$OUT"; }
check_six()     { grep -qw 6 "$OUT"; }
check_fix()     { git diff --quiet -- tests/ && python3 -m unittest tests.test_calc >/dev/null 2>&1; }
check_power()   { python3 -c "from calc import power; assert power(2, 3) == 8"; }
check_verbose() { python3 cli.py add 1 2 --verbose 2>&1 >/dev/null | grep -q "verbose mode" && python3 cli.py add 1 2 | grep -q "3.0"; }
check_rename()  { ! grep -rq --exclude-dir=.git slugify . && python3 -c "from utils.strings import to_slug; assert to_slug('Hello World') == 'hello-world'"; }
check_tests()   { [ -f tests/test_strings.py ] && grep -q slugify tests/test_strings.py && python3 -m unittest tests.test_strings >/dev/null 2>&1; }
check_docs()    { all_docstrings utils/strings.py; }
check_move()    { python3 -c "from utils.strings import is_palindrome; assert is_palindrome('Racecar')" && ! grep -q "def is_palindrome" cli.py && grep -q is_palindrome cli.py && python3 cli.py add 1 2 | grep -q "3.0"; }

NAMES=(); PROMPTS=(); CHECKS=()
task() { NAMES+=("$1"); PROMPTS+=("$2"); CHECKS+=("$3"); }

task find-definition  "Which file defines the slugify function? Reply with just the file path." check_path
task count-functions  "How many functions are defined in calc.py? Reply with just the number." check_four
task read-default     "What is the default value of the --precision option in cli.py? Reply with just the number." check_six
task fix-failing-test "The tests in tests/ are failing. Run them, find the cause, and fix the code." check_fix
task add-function     "Add a power(a, b) function to calc.py that returns a raised to the power b." check_power
task add-flag         "Add a --verbose flag to cli.py. When set, print 'verbose mode' to stderr." check_verbose
task rename-function  "Rename the function slugify to to_slug everywhere in the project, including docs." check_rename
task write-tests      "Create tests/test_strings.py with unit tests for slugify and truncate." check_tests
task add-docstrings   "Add a docstring to every function in utils/strings.py." check_docs
task move-function    "Move the is_palindrome function from cli.py into utils/strings.py and import it in cli.py." check_move

pass=0; total=${#NAMES[@]}
printf '%-18s %-6s %s\n' TASK RESULT SECS
for i in "${!NAMES[@]}"; do
  name=${NAMES[$i]}
  dir=$(mktemp -d)
  make_fixture "$dir"
  export OUT="$LOGS/$name.out"
  start=$SECONDS
  ( cd "$dir" && $TIMEOUT_BIN ${TIMEOUT_BIN:+"$TASK_TIMEOUT"} "$JIAN" -y -p "${PROMPTS[$i]}" >"$OUT" 2>"$LOGS/$name.err" )
  if ( cd "$dir" && ${CHECKS[$i]} ); then res=PASS; pass=$((pass + 1)); else res=FAIL; fi
  printf '%-18s %-6s %s\n' "$name" "$res" "$((SECONDS - start))"
  rm -rf "$dir"
done
echo "passed $pass/$total   (logs in $LOGS)"
