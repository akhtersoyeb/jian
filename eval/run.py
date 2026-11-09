#!/usr/bin/env python3
"""Jian Code eval runner. See eval/README.md.

  python3 eval/run.py --selftest                      prove verifiers + fixtures are sound (no model)
  python3 eval/run.py --list                          list tasks
  python3 eval/run.py --runs 1 --fixtures python      quick smoke run
  python3 eval/run.py --runs 3 --label baseline       full run
"""
from __future__ import annotations

import argparse
import json
import os
import signal
import subprocess
import sys
import tempfile
import time
from datetime import datetime
from pathlib import Path

import analyze as A
import tasks as T

HERE = Path(__file__).resolve().parent


def parse_args():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--jian", default=str(HERE.parent / "target" / "release" / "jian"))
    ap.add_argument("--runs", type=int, default=3, help="runs per task (default 3)")
    ap.add_argument("--tiers", default="", help="comma list: explore,edit,fix,multi")
    ap.add_argument("--fixtures", default="", help="comma list: python,rust,node")
    ap.add_argument("--only", default="", help="comma list of substrings of task ids")
    ap.add_argument("--timeout", type=int, default=420, help="seconds per run (default 420)")
    ap.add_argument("--label", default="run")
    ap.add_argument("--out", default="", help="results dir (default eval/results/<timestamp>-<label>)")
    ap.add_argument("--think", choices=["true", "false"])
    ap.add_argument("--num-ctx", type=int)
    ap.add_argument("--temperature", type=float)
    ap.add_argument("--model")
    ap.add_argument("--selftest", action="store_true")
    ap.add_argument("--list", action="store_true")
    return ap.parse_args()


def select(args, verbose=True):
    def want(csv, value):
        return not csv or value in [x.strip() for x in csv.split(",")]

    only = [x.strip() for x in args.only.split(",") if x.strip()]
    chosen, skipped = [], set()
    for t in T.TASKS:
        if not (want(args.tiers, t.tier) and want(args.fixtures, t.fixture)):
            continue
        if only and not any(o in t.id for o in only):
            continue
        if not T.tool_ok(t.needs):
            skipped.add(t.needs)
            continue
        chosen.append(t)
    if verbose:
        for tool in sorted(skipped):
            print(f"note: skipping {tool}-based tasks ({tool} not found"
                  f"{' or too old; need Node >= 22.18' if tool == 'node' else ''})")
    return chosen


# ------------------------------------------------------------------ selftest
def selftest(args):
    failures = 0
    tasks = select(args)
    for t in tasks:
        with tempfile.TemporaryDirectory(prefix="jian-selftest-") as tmp:
            d, p = T.prepare(t, Path(tmp))
            problems = []
            try:
                t.verify(d, "", p)
                problems.append("verifier PASSES on the unsolved project")
            except T.Fail:
                pass
            except Exception as e:  # harness bug, not a task failure
                problems.append(f"verifier crashed on unsolved project: {e!r}")
            try:
                answer = t.solve(d)
                t.verify(d, answer, p)
            except T.Fail as e:
                problems.append(f"verifier FAILS on the reference solution: {e}")
            except Exception as e:
                problems.append(f"solve/verify crashed: {e!r}")
        print(f"{'ok  ' if not problems else 'FAIL'} {t.id}" + ("" if not problems else "  <- " + "; ".join(problems)))
        failures += bool(problems)
    print(f"\nselftest: {len(tasks) - failures}/{len(tasks)} tasks sound")
    return 1 if failures else 0


# ------------------------------------------------------------------ real runs
def run_jian(cmd, cwd, timeout):
    """Run jian in its own session. On timeout: SIGINT first (jian cleans up its bash children), then SIGKILL."""
    # PYTHONDONTWRITEBYTECODE: a stale .pyc (same size, same mtime second) can hide an edit from a test run.
    env = {**os.environ, "PYTHONDONTWRITEBYTECODE": "1"}
    proc = subprocess.Popen(cmd, cwd=cwd, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, text=True, start_new_session=True)
    timed_out = False
    try:
        out, err = proc.communicate(timeout=timeout)
    except subprocess.TimeoutExpired:
        timed_out = True
        try:
            proc.send_signal(signal.SIGINT)
            out, err = proc.communicate(timeout=15)
        except subprocess.TimeoutExpired:
            os.killpg(proc.pid, signal.SIGKILL)
            out, err = proc.communicate()
    return proc.returncode, out or "", err or "", timed_out


def run_one(task, idx, args, out_dir):
    with tempfile.TemporaryDirectory(prefix="jian-eval-") as tmp:
        d, pristine = T.prepare(task, Path(tmp))
        tpath = out_dir / "transcripts" / f"{task.id}.{idx}.jsonl"
        tpath.parent.mkdir(parents=True, exist_ok=True)
        tpath.unlink(missing_ok=True)
        cmd = [args.jian, "-y", "-p", task.prompt, "--transcript", str(tpath)]
        if args.think:
            cmd += ["--think", args.think]
        if args.num_ctx:
            cmd += ["--num-ctx", str(args.num_ctx)]
        if args.temperature is not None:
            cmd += ["--temperature", str(args.temperature)]
        if args.model:
            cmd += ["--model", args.model]

        t0 = time.time()
        rc, out, err, timed_out = run_jian(cmd, d, args.timeout)
        secs = round(time.time() - t0, 1)

        reason = ""
        try:
            task.verify(d, out, pristine)
            passed = True
        except T.Fail as e:
            passed, reason = False, str(e)
        except Exception as e:
            passed, reason = False, f"verifier error: {e!r}"

    logs = out_dir / "logs"
    logs.mkdir(exist_ok=True)
    (logs / f"{task.id}.{idx}.out").write_text(out)
    (logs / f"{task.id}.{idx}.err").write_text(err)

    m = A.metrics(A.load_events(tpath))
    failure = "" if passed else A.classify(task.tier, timed_out, m)
    return {"task": task.id, "fixture": task.fixture, "tier": task.tier, "run": idx, "passed": passed,
            "reason": reason, "failure": failure, "secs": secs, "timed_out": timed_out, "exit_code": rc,
            "metrics": m}


def main():
    args = parse_args()
    if args.list:
        for t in select(args, verbose=False):
            print(f"{t.id:<18} {t.fixture:<7} {t.tier}")
        return 0
    if args.selftest:
        return selftest(args)

    jian = Path(args.jian)
    if not jian.exists():
        print(f"jian binary not found at {jian}. Build it (cargo build --release) or pass --jian.")
        return 2
    tasks = select(args)
    if not tasks:
        print("no tasks selected")
        return 2

    stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    out_dir = Path(args.out) if args.out else HERE / "results" / f"{stamp}-{args.label}"
    out_dir.mkdir(parents=True, exist_ok=True)
    results = []
    total = len(tasks) * args.runs
    print(f"{len(tasks)} tasks x {args.runs} runs = {total} runs -> {out_dir}")

    done = 0
    with open(out_dir / "results.jsonl", "w") as log:
        for t in tasks:
            for i in range(1, args.runs + 1):
                done += 1
                r = run_one(t, i, args, out_dir)
                results.append(r)
                log.write(json.dumps(r) + "\n")
                log.flush()
                mark = "PASS" if r["passed"] else f"FAIL[{r['failure']}]"
                extra = "" if r["passed"] else f"  {r['reason'][:90]}"
                print(f"[{done}/{total}] {t.id:<16} #{i} {mark:<22} {r['secs']:>6}s "
                      f"{r['metrics']['steps']} steps{extra}", flush=True)

    cfg = results[0]["metrics"]["config"] if results else {}
    meta = {"label": args.label, "stamp": stamp, "runs_per_task": args.runs, "config": cfg,
            "flags": {"think": args.think, "num_ctx": args.num_ctx, "temperature": args.temperature,
                      "model": args.model}}
    summary = A.summarize(results, meta)
    (out_dir / "summary.json").write_text(json.dumps(summary, indent=2))
    A.print_summary(summary)
    print(f"\nresults: {out_dir}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
