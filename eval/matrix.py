#!/usr/bin/env python3
"""Run the eval under several configs, one factor at a time, and compare them with the first config.

  python3 eval/matrix.py --runs 1                       screening pass over every config
  python3 eval/matrix.py --runs 3 --configs baseline,think-off
  python3 eval/matrix.py --root eval/results/matrix-XYZ --runs 1   resume (configs with a summary are skipped)
"""
from __future__ import annotations

import argparse
import json
import subprocess
import sys
from datetime import datetime
from pathlib import Path

HERE = Path(__file__).resolve().parent

# "baseline" = whatever your config file / defaults say. Each other entry changes ONE factor.
CONFIGS = {
    "baseline": [],
    "think-off": ["--think", "false"],
    "ctx-8k": ["--num-ctx", "8192"],
    "ctx-32k": ["--num-ctx", "32768"],
    "temp-0.2": ["--temperature", "0.2"],
}


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--configs", default=",".join(CONFIGS))
    ap.add_argument("--runs", type=int, default=1)
    ap.add_argument("--root", default="")
    ap.add_argument("--tiers", default="")
    ap.add_argument("--fixtures", default="")
    ap.add_argument("--only", default="")
    ap.add_argument("--timeout", default="420")
    ap.add_argument("--jian", default="")
    a = ap.parse_args()

    names = [n.strip() for n in a.configs.split(",") if n.strip()]
    unknown = [n for n in names if n not in CONFIGS]
    if unknown:
        print(f"unknown configs: {unknown}; known: {list(CONFIGS)}")
        return 2
    root = Path(a.root) if a.root else HERE / "results" / f"matrix-{datetime.now():%Y%m%d-%H%M%S}"
    root.mkdir(parents=True, exist_ok=True)

    for n in names:
        out = root / n
        if (out / "summary.json").exists():
            print(f"== {n}: already done, skipping")
            continue
        cmd = [sys.executable, str(HERE / "run.py"), "--label", n, "--out", str(out), "--runs", str(a.runs),
               "--timeout", a.timeout] + CONFIGS[n]
        for flag, val in (("--tiers", a.tiers), ("--fixtures", a.fixtures), ("--only", a.only), ("--jian", a.jian)):
            if val:
                cmd += [flag, val]
        print(f"\n===== {n}: {' '.join(CONFIGS[n]) or '(defaults)'}", flush=True)
        subprocess.run(cmd)

    rows = []
    for n in names:
        p = root / n / "summary.json"
        if p.exists():
            rows.append((n, json.loads(p.read_text())))
    if not rows:
        return 1
    base = rows[0][1]
    print(f"\n{'config':<10} {'overall':>8} {'d':>5} {'explore':>8} {'edit':>6} {'fix':>6} {'multi':>6} "
          f"{'secs':>7} {'steps':>6} {'tok/s':>6}")
    for n, s in rows:
        def tier(t):
            g = s["by_tier"].get(t)
            return f"{g['rate']:.0%}" if g else "-"
        d = (s["pass_rate"] - base["pass_rate"]) * 100
        print(f"{n:<10} {s['pass_rate']:>8.0%} {d:>+4.0f}p {tier('explore'):>8} {tier('edit'):>6} "
              f"{tier('fix'):>6} {tier('multi'):>6} {s['means']['secs']:>7} {s['means']['steps']:>6} "
              f"{s['means']['tok_s']:>6}")
    print(f"\nresults: {root}   (details: python3 eval/analyze.py show {root}/<config>)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
