"""Eval tasks for Jian Code: prompts, setup (bug injection), reference solutions, verifiers.

Each task = a fixture project + a prompt + a verifier. `solve` is a reference solution used
only by `run.py --selftest`, which proves every verifier fails on the starting state and
passes on a correct solution, so a failure in a real run is the model's, not the harness's.
"""
from __future__ import annotations

import os
import re
import shutil
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Optional

FIXTURES = Path(__file__).parent / "fixtures"
SKIP_DIRS = {".git", "__pycache__", "target", "node_modules"}
TIERS = ["explore", "edit", "fix", "multi"]


class Fail(Exception):
    """A verifier check failed (the message says why)."""


def need(cond, msg):
    if not cond:
        raise Fail(msg)


# ----------------------------------------------------------------- helpers
def sh(cmd, cwd, env=None, timeout=240):
    e = {**os.environ, "CARGO_TERM_COLOR": "never", "NO_COLOR": "1",
         "PYTHONDONTWRITEBYTECODE": "1", **(env or {})}
    try:
        p = subprocess.run(cmd, cwd=cwd, env=e, capture_output=True, text=True,
                           timeout=timeout, stdin=subprocess.DEVNULL)
        return p.returncode, p.stdout, p.stderr
    except subprocess.TimeoutExpired:
        return 124, "", "timeout"


def rd(d, rel):
    return (Path(d) / rel).read_text()


def wr(d, rel, text):
    p = Path(d) / rel
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(text)


def sub_in(d, rel, old, new):
    t = rd(d, rel)
    assert old in t, f"harness bug: {old!r} not found in {rel}"
    wr(d, rel, t.replace(old, new, 1))


def project_files(d):
    for root, dirs, fs in os.walk(d):
        dirs[:] = [x for x in dirs if x not in SKIP_DIRS]
        for f in fs:
            yield Path(root) / f


def files_containing(d, needle):
    hits = []
    for p in project_files(d):
        try:
            if needle in p.read_text():
                hits.append(str(p.relative_to(d)))
        except (UnicodeDecodeError, OSError):
            pass
    return hits


def replace_everywhere(d, old, new):
    for rel in files_containing(d, old):
        wr(d, rel, rd(d, rel).replace(old, new))


def same(d, p, rel):
    return rd(d, rel) == rd(p, rel)


def tail_from(d, rel, marker):
    t = rd(d, rel)
    i = t.find(marker)
    return t[i:] if i >= 0 else None


def numbers(out):
    return re.findall(r"\d+(?:\.\d+)?", out)


# ----------------------------------------------------------------- tool checks
def node_ok():
    if not shutil.which("node"):
        return False
    m = re.match(r"v(\d+)\.(\d+)", subprocess.run(["node", "--version"], capture_output=True, text=True).stdout)
    return bool(m) and (int(m[1]), int(m[2])) >= (22, 18)


def tool_ok(name):
    if name == "cargo":
        return shutil.which("cargo") is not None
    if name == "node":
        return node_ok()
    return True


# ----------------------------------------------------------------- task model
@dataclass
class Task:
    id: str
    fixture: str
    tier: str
    prompt: str
    verify: Callable[[Path, str, Path], None]
    solve: Callable[[Path], str]
    setup: Optional[Callable[[Path], None]] = None

    @property
    def needs(self):
        return {"rust": "cargo", "node": "node"}.get(self.fixture, "")


def prepare(task, root: Path):
    """Copy the fixture, apply setup, init git. Returns (work_dir, pristine_copy)."""
    d, p = root / "work", root / "pristine"
    shutil.copytree(FIXTURES / task.fixture, d,
                    ignore=shutil.ignore_patterns("__pycache__", "target", "node_modules", "Cargo.lock"))
    if task.setup:
        task.setup(d)
    for cmd in (["git", "init", "-q"], ["git", "add", "-A"],
                ["git", "-c", "user.name=eval", "-c", "user.email=eval@example.com", "commit", "-qm", "init"]):
        subprocess.run(cmd, cwd=d, capture_output=True)
    shutil.copytree(d, p, ignore=shutil.ignore_patterns(".git"))
    return d, p


# ================================================================= PYTHON (tinycalc)
def py(d, *args, env=None):
    return sh(["python3", *args], d, env)


def py_tests(d):
    rc, o, e = sh(["python3", "-m", "unittest", "-q"], d)
    need(rc == 0, "unit tests fail: " + (o + e).strip()[-300:])


def py_check(d, code, msg):
    rc, o, e = py(d, "-c", code)
    need(rc == 0, f"{msg}: " + (o + e).strip()[-200:])


def v_py_find_def(d, out, p):
    need("utils/strings.py" in out.replace("\\", "/"), "answer should be utils/strings.py")


def v_py_default(d, out, p):
    need("6" in numbers(out), "answer should be 6")


def s_py_add_power(d):
    wr(d, "calc.py", rd(d, "calc.py") + "\n\ndef power(a, b):\n    return a ** b\n")
    return ""


def v_py_add_power(d, out, p):
    py_check(d, "from calc import power; assert power(2,3)==8 and power(5,0)==1 and power(2,-1)==0.5",
             "power() missing or wrong")
    py_tests(d)


def s_py_add_flag(d):
    sub_in(d, "cli.py", "import argparse\n", "import argparse\nimport sys\n")
    sub_in(d, "cli.py", '    parser.add_argument("--precision", type=int, default=6)\n',
           '    parser.add_argument("--precision", type=int, default=6)\n'
           '    parser.add_argument("--verbose", action="store_true")\n')
    sub_in(d, "cli.py", "    args = parser.parse_args(argv)\n",
           '    args = parser.parse_args(argv)\n    if args.verbose:\n        print("verbose mode", file=sys.stderr)\n')
    return ""


def v_py_add_flag(d, out, p):
    rc, o, e = py(d, "cli.py", "add", "1", "2", "--verbose")
    need(rc == 0, "cli failed with --verbose: " + e[-200:])
    need("verbose mode" in e, "'verbose mode' not on stderr")
    need(o.strip() == "3.000000", f"stdout changed: {o!r}")
    rc, o, e = py(d, "cli.py", "add", "1", "2")
    need(rc == 0 and "verbose mode" not in o + e, "verbose text printed without the flag")
    py_tests(d)


def st_py_fix_div(d):
    sub_in(d, "tests/test_calc.py", "\n\n\nif __name__",
           "\n\n    def test_div_by_zero(self):\n        with self.assertRaises(ValueError):\n"
           "            div(1, 0)\n\n\nif __name__")


def s_py_fix_div(d):
    sub_in(d, "calc.py", "    return a / b", '    if b == 0:\n        raise ValueError("division by zero")\n    return a / b')
    return ""


def v_py_fix_div(d, out, p):
    py_tests(d)
    need(same(d, p, "tests/test_calc.py"), "tests/test_calc.py was modified")


def st_py_fix_truncate(d):
    sub_in(d, "utils/strings.py", "text[: n - 3]", "text[: n - 2]")


def s_py_fix_truncate(d):
    sub_in(d, "utils/strings.py", "text[: n - 2]", "text[: n - 3]")
    return ""


def v_py_fix_truncate(d, out, p):
    py_tests(d)
    need(same(d, p, "tests/test_strings.py"), "tests/test_strings.py was modified")


def s_py_rename(d):
    replace_everywhere(d, "slugify", "to_slug")
    return ""


def v_py_rename(d, out, p):
    left = files_containing(d, "slugify")
    need(not left, f"'slugify' still appears in: {left}")
    py_check(d, "from utils.strings import to_slug; assert to_slug('Hello World')=='hello-world'", "to_slug missing")
    py_tests(d)


PALINDROME = ('def is_palindrome(s):\n    s = "".join(ch.lower() for ch in s if ch.isalnum())\n'
              '    return s == s[::-1]\n')


def s_py_move(d):
    cli = rd(d, "cli.py")
    assert PALINDROME + "\n\n" in cli
    cli = cli.replace(PALINDROME + "\n\n", "")
    cli = cli.replace("from calc import add, sub, mul, div\n",
                      "from calc import add, sub, mul, div\nfrom utils.strings import is_palindrome\n")
    wr(d, "cli.py", cli)
    wr(d, "utils/strings.py", rd(d, "utils/strings.py") + "\n\n" + PALINDROME)
    return ""


def v_py_move(d, out, p):
    py_check(d, "from utils.strings import is_palindrome; assert is_palindrome('Racecar') and not is_palindrome('abc')",
             "is_palindrome not importable from utils.strings")
    cli = rd(d, "cli.py")
    need("def is_palindrome" not in cli, "is_palindrome is still defined in cli.py")
    need("is_palindrome" in cli, "cli.py no longer imports is_palindrome")
    rc, o, e = py(d, "cli.py", "add", "1", "2")
    need(rc == 0 and o.strip() == "3.000000", "cli broke: " + e[-200:])
    py_tests(d)


# ================================================================= RUST (wordstat)
SAMPLE = "the cat and the dog and the bird saw a fish\n"


def cargo(d, *args):
    return sh(["cargo", *args], d)


def cargo_tests(d, msg="cargo test fails"):
    rc, o, e = cargo(d, "test", "-q")
    need(rc == 0, msg + ": " + (o + e).strip()[-400:])


def run_bin(d, *args):
    return cargo(d, "run", "-q", "--", *args)


def word_lines(out):
    return [l.strip() for l in out.splitlines() if re.fullmatch(r"[a-z0-9]+ \d+", l.strip())]


def v_rs_find_fn(d, out, p):
    need("src/stats.rs" in out.replace("\\", "/"), "answer should be src/stats.rs")


def v_rs_count_pub(d, out, p):
    need("3" in numbers(out), "answer should be 3")


def s_rs_add_unique(d):
    sub_in(d, "src/stats.rs", "#[cfg(test)]\nmod tests",
           "/// Number of distinct words.\npub fn unique_count(words: &[String]) -> usize {\n"
           "    word_freq(words).len()\n}\n\n#[cfg(test)]\nmod tests")
    return ""


def v_rs_add_unique(d, out, p):
    wr(d, "tests/verify_unique.rs",
       'use wordstat::stats::unique_count;\n\n#[test]\nfn unique() {\n'
       '    let w: Vec<String> = ["a", "b", "a", "c"].iter().map(|s| s.to_string()).collect();\n'
       '    assert_eq!(unique_count(&w), 3);\n    assert_eq!(unique_count(&[]), 0);\n}\n')
    cargo_tests(d, "unique_count missing or wrong")


MAIN_TOP = '''use std::io::Read;
use wordstat::{report, stats, tokenize};

fn main() {
    let mut top = 5usize;
    let mut path: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--top" {
            top = args
                .next()
                .and_then(|v| v.parse().ok())
                .expect("--top needs a number");
        } else {
            path = Some(a);
        }
    }
    let mut text = String::new();
    match path {
        Some(p) => text = std::fs::read_to_string(&p).expect("could not read file"),
        None => {
            std::io::stdin()
                .read_to_string(&mut text)
                .expect("could not read stdin");
        }
    }
    let words = tokenize::tokenize(&text);
    let freq = stats::word_freq(&words);
    println!("{}", report::render(&freq, top));
    println!("total: {}", words.len());
}
'''

MAIN_MINLEN = MAIN_TOP.replace('let mut top = 5usize;', 'let mut min_len = 0usize;') \
    .replace('if a == "--top" {\n            top = args', 'if a == "--min-len" {\n            min_len = args') \
    .replace('.expect("--top needs a number")', '.expect("--min-len needs a number")') \
    .replace('let words = tokenize::tokenize(&text);',
             'let words = tokenize::filter_min_len(tokenize::tokenize(&text), min_len);') \
    .replace('report::render(&freq, top)', 'report::render(&freq, 5)')


def s_rs_add_top(d):
    wr(d, "src/main.rs", MAIN_TOP)
    return ""


def v_rs_add_top(d, out, p):
    wr(d, "sample.txt", SAMPLE)
    rc, o, e = run_bin(d, "--top", "2", "sample.txt")
    need(rc == 0, "run with --top failed: " + (o + e)[-300:])
    need(len(word_lines(o)) == 2, f"--top 2 printed {len(word_lines(o))} word lines")
    rc, o, e = run_bin(d, "sample.txt")
    need(rc == 0 and len(word_lines(o)) == 5, "default should still print 5 word lines")
    cargo_tests(d)


def st_rs_fix_test(d):
    sub_in(d, "src/tokenize.rs", ".map(|w| w.to_lowercase())", ".map(|w| w.to_string())")


def s_rs_fix_test(d):
    sub_in(d, "src/tokenize.rs", ".map(|w| w.to_string())", ".map(|w| w.to_lowercase())")
    return ""


def v_rs_fix_test(d, out, p):
    cargo_tests(d)
    need(tail_from(d, "src/tokenize.rs", "#[cfg(test)]") == tail_from(p, "src/tokenize.rs", "#[cfg(test)]"),
         "the tests in src/tokenize.rs were modified")


def st_rs_fix_compile(d):
    sub_in(d, "src/stats.rs", "total as f64 / words.len() as f64", "total / words.len()")


def s_rs_fix_compile(d):
    sub_in(d, "src/stats.rs", "total / words.len()", "total as f64 / words.len() as f64")
    return ""


def v_rs_fix_compile(d, out, p):
    cargo_tests(d)
    need(tail_from(d, "src/stats.rs", "#[cfg(test)]") == tail_from(p, "src/stats.rs", "#[cfg(test)]"),
         "the tests in src/stats.rs were modified")


def s_rs_rename(d):
    replace_everywhere(d, "word_freq", "frequencies")
    return ""


def v_rs_rename(d, out, p):
    left = files_containing(d, "word_freq")
    need(not left, f"'word_freq' still appears in: {left}")
    wr(d, "tests/verify_rename.rs",
       'use wordstat::stats::frequencies;\n\n#[test]\nfn renamed() {\n'
       '    let w = vec!["a".to_string(), "a".to_string()];\n    assert_eq!(frequencies(&w)["a"], 2);\n}\n')
    cargo_tests(d, "rename incomplete or tests failing")


def s_rs_min_len(d):
    sub_in(d, "src/tokenize.rs", "#[cfg(test)]\nmod tests",
           "/// Drop words shorter than `n` characters.\npub fn filter_min_len(words: Vec<String>, n: usize) -> Vec<String> {\n"
           "    words.into_iter().filter(|w| w.chars().count() >= n).collect()\n}\n\n#[cfg(test)]\nmod tests")
    wr(d, "src/main.rs", MAIN_MINLEN)
    return ""


def v_rs_min_len(d, out, p):
    wr(d, "tests/verify_min.rs",
       'use wordstat::tokenize::filter_min_len;\n\n#[test]\nfn filters() {\n'
       '    let w: Vec<String> = ["a", "bb", "ccc"].iter().map(|s| s.to_string()).collect();\n'
       '    assert_eq!(filter_min_len(w, 2), vec!["bb".to_string(), "ccc".to_string()]);\n}\n')
    cargo_tests(d, "filter_min_len missing or wrong")
    wr(d, "min.txt", "a bb ccc bb\n")
    rc, o, e = run_bin(d, "--min-len", "2", "min.txt")
    need(rc == 0, "run with --min-len failed: " + (o + e)[-300:])
    lines = word_lines(o)
    need("bb 2" in lines and "ccc 1" in lines, f"unexpected output with --min-len 2: {lines}")
    need("a 1" not in lines, "single-letter word was not filtered")
    rc, o, e = run_bin(d, "min.txt")
    need("a 1" in word_lines(o), "without the flag, nothing should be filtered")


# ================================================================= NODE (notes-cli)
def node(d, *args, env=None):
    return sh(["node", *args], d, env)


def node_tests(d, msg="node tests fail"):
    rc, o, e = sh(["node", "--test", "test/*.test.ts"], d)
    need(rc == 0, msg + ": " + (o + e).strip()[-400:])


def notes_file(d):
    return str(Path(d) / "verify-notes.json")


def cli(d, *args):
    return node(d, "src/cli.ts", *args, env={"NOTES_FILE": notes_file(d)})


def fresh_notes(d):
    Path(notes_file(d)).unlink(missing_ok=True)


def v_js_find_fn(d, out, p):
    need("src/format.ts" in out.replace("\\", "/"), "answer should be src/format.ts")


def v_js_default(d, out, p):
    need("notes.json" in out, "answer should be notes.json")


def s_js_add_count(d):
    sub_in(d, "src/store.ts", "  search(query: string)",
           "  count(): number {\n    return this.notes.length;\n  }\n\n  search(query: string)")
    return ""


def v_js_add_count(d, out, p):
    wr(d, "test/verify_count.test.ts",
       'import { test } from "node:test";\nimport assert from "node:assert/strict";\n'
       'import { NoteStore } from "../src/store.ts";\n\ntest("count", () => {\n  const s = new NoteStore();\n'
       '  assert.equal(s.count(), 0);\n  s.add("a");\n  s.add("b");\n  assert.equal(s.count(), 2);\n});\n')
    node_tests(d, "count() missing or wrong")


def s_js_add_limit(d):
    sub_in(d, "src/cli.ts", "    for (const note of store.list()) console.log(formatNote(note));",
           '    const i = rest.indexOf("--limit");\n    const limit = i >= 0 ? Number(rest[i + 1]) : Infinity;\n'
           "    for (const note of store.list().slice(0, limit)) console.log(formatNote(note));")
    return ""


def v_js_add_limit(d, out, p):
    fresh_notes(d)
    for t in ("one", "two", "three"):
        rc, o, e = cli(d, "add", t)
        need(rc == 0, "add failed: " + e[-200:])
    rc, o, e = cli(d, "list", "--limit", "2")
    need(rc == 0 and len(o.strip().splitlines()) == 2, f"--limit 2 printed {len(o.strip().splitlines())} lines")
    rc, o, e = cli(d, "list")
    need(rc == 0 and len(o.strip().splitlines()) == 3, "list without the flag should print all notes")
    node_tests(d)


def st_js_fix_search(d):
    sub_in(d, "src/store.ts", "return this.notes.filter((n) => n.text.toLowerCase().includes(q));",
           "return this.notes.filter((n) => n.text.includes(query));")


def s_js_fix_search(d):
    sub_in(d, "src/store.ts", "return this.notes.filter((n) => n.text.includes(query));",
           "return this.notes.filter((n) => n.text.toLowerCase().includes(q));")
    return ""


def v_js_tests_untouched(d, out, p):
    node_tests(d)
    for rel in ("test/store.test.ts", "test/format.test.ts"):
        need(same(d, p, rel), f"{rel} was modified")


def st_js_fix_ids(d):
    sub_in(d, "src/store.ts", "Math.max(max, n.id), 0) + 1;", "Math.max(max, n.id), 0) + 0;")


def s_js_fix_ids(d):
    sub_in(d, "src/store.ts", "Math.max(max, n.id), 0) + 0;", "Math.max(max, n.id), 0) + 1;")
    return ""


def s_js_rename(d):
    replace_everywhere(d, "NoteStore", "Notebook")
    return ""


def v_js_rename(d, out, p):
    left = files_containing(d, "NoteStore")
    need(not left, f"'NoteStore' still appears in: {left}")
    wr(d, "test/verify_rename.test.ts",
       'import { test } from "node:test";\nimport assert from "node:assert/strict";\n'
       'import { Notebook } from "../src/store.ts";\n\ntest("renamed", () => {\n  const n = new Notebook();\n'
       '  assert.equal(n.add("x").id, 1);\n});\n')
    node_tests(d, "rename incomplete or tests failing")


def s_js_delete(d):
    sub_in(d, "src/store.ts", "  search(query: string)",
           "  remove(id: number): boolean {\n    const i = this.notes.findIndex((n) => n.id === id);\n"
           "    if (i < 0) return false;\n    this.notes.splice(i, 1);\n    return true;\n  }\n\n  search(query: string)")
    sub_in(d, "src/cli.ts", "  default:",
           '  case "delete": {\n    const id = Number(rest[0]);\n    if (!store.remove(id)) {\n'
           "      console.error(`no note with id ${id}`);\n      process.exit(1);\n    }\n    save(store);\n"
           "    console.log(`deleted ${id}`);\n    break;\n  }\n  default:")
    return ""


def v_js_delete(d, out, p):
    wr(d, "test/verify_remove.test.ts",
       'import { test } from "node:test";\nimport assert from "node:assert/strict";\n'
       'import { NoteStore } from "../src/store.ts";\n\ntest("remove", () => {\n  const s = new NoteStore();\n'
       '  s.add("a");\n  s.add("b");\n  assert.equal(s.remove(1), true);\n  assert.equal(s.remove(99), false);\n'
       '  assert.deepEqual(s.list().map((n) => n.id), [2]);\n});\n')
    node_tests(d, "remove() missing or wrong")
    fresh_notes(d)
    cli(d, "add", "first")
    cli(d, "add", "second")
    rc, o, e = cli(d, "delete", "1")
    need(rc == 0 and "deleted 1" in o, f"delete 1 should print 'deleted 1' (rc={rc}, out={o!r})")
    rc, o, e = cli(d, "list")
    need(o.strip().splitlines() == ["2: second"], f"delete was not persisted: {o!r}")
    rc, o, e = cli(d, "delete", "99")
    need(rc == 1, f"deleting a missing id should exit 1 (got {rc})")


# ================================================================= registry
TASKS = [
    # ---- python
    Task("py-find-def", "python", "explore",
         "Which file defines the slugify function? Reply with just the file path.",
         v_py_find_def, lambda d: "utils/strings.py"),
    Task("py-default", "python", "explore",
         "What is the default value of the --precision option in cli.py? Reply with just the number.",
         v_py_default, lambda d: "6"),
    Task("py-add-power", "python", "edit",
         "Add a power(a, b) function to calc.py that returns a raised to the power b.",
         v_py_add_power, s_py_add_power),
    Task("py-add-flag", "python", "edit",
         "Add a --verbose flag to cli.py. When set, print 'verbose mode' to stderr.",
         v_py_add_flag, s_py_add_flag),
    Task("py-fix-div", "python", "fix",
         "The tests are failing. Run them, find the cause, and fix the code (not the tests).",
         v_py_fix_div, s_py_fix_div, st_py_fix_div),
    Task("py-fix-truncate", "python", "fix",
         "Some tests fail. Run them, find the bug, and fix the code (not the tests).",
         v_py_fix_truncate, s_py_fix_truncate, st_py_fix_truncate),
    Task("py-rename", "python", "multi",
         "Rename the function slugify to to_slug everywhere in the project, including tests and docs.",
         v_py_rename, s_py_rename),
    Task("py-move", "python", "multi",
         "Move the is_palindrome function from cli.py into utils/strings.py, and import it in cli.py.",
         v_py_move, s_py_move),
    # ---- rust
    Task("rs-find-fn", "rust", "explore",
         "Which file defines the function word_freq? Reply with just the file path.",
         v_rs_find_fn, lambda d: "src/stats.rs"),
    Task("rs-count-pub", "rust", "explore",
         "How many public functions are defined in src/stats.rs? Reply with just the number.",
         v_rs_count_pub, lambda d: "3"),
    Task("rs-add-fn", "rust", "edit",
         "Add a public function `unique_count(words: &[String]) -> usize` to src/stats.rs that returns "
         "the number of distinct words.",
         v_rs_add_unique, s_rs_add_unique),
    Task("rs-add-top", "rust", "edit",
         "Add a --top N option to the wordstat CLI so the number of words shown is configurable "
         "(default 5). Usage after the change: `wordstat [--top N] [FILE]`.",
         v_rs_add_top, s_rs_add_top),
    Task("rs-fix-test", "rust", "fix",
         "`cargo test` is failing. Find the cause and fix the code (not the tests).",
         v_rs_fix_test, s_rs_fix_test, st_rs_fix_test),
    Task("rs-fix-compile", "rust", "fix",
         "The project doesn't compile. Fix it (don't change the tests).",
         v_rs_fix_compile, s_rs_fix_compile, st_rs_fix_compile),
    Task("rs-rename", "rust", "multi",
         "Rename the function word_freq to frequencies everywhere in the project, including tests and docs.",
         v_rs_rename, s_rs_rename),
    Task("rs-min-len", "rust", "multi",
         "Add a --min-len N option to the CLI that ignores words shorter than N characters. "
         "Usage after the change: `wordstat [--min-len N] [FILE]`. Put the filtering logic in a function "
         "`filter_min_len(words: Vec<String>, n: usize) -> Vec<String>` in src/tokenize.rs.",
         v_rs_min_len, s_rs_min_len),
    # ---- node
    Task("js-find-fn", "node", "explore",
         "Which file defines the formatNote function? Reply with just the file path.",
         v_js_find_fn, lambda d: "src/format.ts"),
    Task("js-default", "node", "explore",
         "Which file does the CLI read and write notes from by default? Reply with just the file name.",
         v_js_default, lambda d: "notes.json"),
    Task("js-add-count", "node", "edit",
         "Add a count() method to the NoteStore class that returns how many notes it holds.",
         v_js_add_count, s_js_add_count),
    Task("js-add-limit", "node", "edit",
         "Add a --limit N option to the list command (usage: `list [--limit N]`) that prints at most N "
         "notes (default: all).",
         v_js_add_limit, s_js_add_limit),
    Task("js-fix-search", "node", "fix",
         "Tests are failing. Run them, find the cause, and fix the code (not the tests).",
         v_js_tests_untouched, s_js_fix_search, st_js_fix_search),
    Task("js-fix-ids", "node", "fix",
         "Tests are failing: new notes get wrong ids. Run the tests, find the cause, and fix the code "
         "(not the tests).",
         v_js_tests_untouched, s_js_fix_ids, st_js_fix_ids),
    Task("js-rename", "node", "multi",
         "Rename the class NoteStore to Notebook everywhere in the project, including tests and docs.",
         v_js_rename, s_js_rename),
    Task("js-delete", "node", "multi",
         "Add a `delete <id>` command to the CLI. Implement a remove(id: number): boolean method on "
         "NoteStore (true if a note was removed) and use it from the CLI. On success print "
         "`deleted <id>`; if the id doesn't exist, print an error to stderr and exit with code 1.",
         v_js_delete, s_js_delete),
]
