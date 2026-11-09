#!/usr/bin/env python3
"""Transcript metrics, failure classification, summaries and comparisons.

  python3 eval/analyze.py show DIR          re-print a results dir's summary
  python3 eval/analyze.py compare A B       compare two results dirs (A = baseline)
"""
from __future__ import annotations

import json
import sys
from collections import Counter
from pathlib import Path

FAILURE_CLASSES = [
    "timeout", "context", "ollama_error", "crash", "loop", "step_limit", "empty_response",
    "edit_mismatch", "hallucinated_path", "no_action", "wrong_answer", "wrong_change",
]


def load_events(path):
    events = []
    try:
        with open(path) as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                try:
                    events.append(json.loads(line))
                except json.JSONDecodeError:
                    pass  # truncated last line from a killed process
    except OSError:
        pass
    return events


def metrics(events):
    steps = [e for e in events if e.get("type") == "step"]
    tools = [e for e in events if e.get("type") == "tool"]
    ends = [e for e in events if e.get("type") == "end"]
    start = next((e for e in events if e.get("type") == "session_start"), {})
    end = ends[-1] if ends else None

    # Does Ollama's prompt_eval_count behave like a FULL prompt size? Within one turn the prompt
    # only grows (append-only history), so a drop with no trim in between means cached tokens
    # are being left out of the count (and our context estimate would run low).
    drops, last, ratios = 0, None, []
    for e in events:
        if e.get("type") == "trim":
            last = None
        elif e.get("type") == "step":
            pt = e.get("prompt_tokens") or 0
            if last is not None and pt and pt < last * 0.9:
                drops += 1
            if pt:
                last = pt
                if e.get("est_prompt"):
                    ratios.append(e["est_prompt"] / pt)

    def total(key):
        return sum((e.get(key) or 0) for e in steps)

    tool_names = Counter(e.get("name") for e in tools)
    errors = [e for e in tools if e.get("outcome") == "error"]
    gen_ms = total("wall_ms")
    return {
        "steps": len(steps),
        "prompt_tokens": total("prompt_tokens"),
        "completion_tokens": total("completion_tokens"),
        "thinking_chars": total("thinking_chars"),
        "model_ms": gen_ms,
        "tok_s": (total("completion_tokens") / (gen_ms / 1000)) if gen_ms else None,
        "tool_calls": len(tools),
        "tools": dict(tool_names),
        "tool_errors": len(errors),
        "edit_errors": sum(1 for e in errors if e.get("name") in ("edit_file", "write_file")),
        "path_errors": sum(1 for e in errors if "path not found" in (e.get("error") or "")),
        "duplicates": sum(1 for e in tools if e.get("outcome") == "duplicate"),
        "denied": sum(1 for e in tools if e.get("outcome") == "denied"),
        "successful_writes": sum(1 for e in tools if e.get("name") in ("edit_file", "write_file")
                                 and e.get("outcome") == "ok"),
        "trims": sum(1 for e in events if e.get("type") == "trim"),
        "rescues": sum(1 for e in events if e.get("type") == "rescue"),
        "nudges": sum(1 for e in events if e.get("type") == "nudge"),
        "max_ctx_pct": max([e.get("ctx_pct") or 0 for e in steps], default=0),
        "end_status": end.get("status") if end else None,
        "end_error": (end.get("error") or "")[:200] if end else "",
        "prompt_drops": drops,
        "est_ratio": (sum(ratios) / len(ratios)) if ratios else None,
        "config": {k: start.get(k) for k in ("model", "num_ctx", "think", "temperature", "version")},
    }


def classify(tier, timed_out, m):
    """Best-guess reason for a FAILED run, from its transcript."""
    status = m["end_status"]
    err = (m["end_error"] or "").lower()
    if timed_out:
        return "timeout"
    if status == "context_full" or "context" in err:
        return "context"
    if status == "error":
        return "ollama_error" if ("ollama" in err or "stream" in err or "request" in err) else "crash"
    if status == "step_limit":
        return "loop" if m["duplicates"] >= 2 else "step_limit"
    if status == "empty_response":
        return "empty_response"
    if m["edit_errors"] >= 2:
        return "edit_mismatch"
    if m["path_errors"] >= 2:
        return "hallucinated_path"
    if tier != "explore" and m["successful_writes"] == 0:
        return "no_action"
    if tier == "explore":
        return "wrong_answer"
    return "wrong_change"


def _rate(passed, runs):
    return round(passed / runs, 3) if runs else 0.0


def _mean(xs):
    xs = [x for x in xs if x is not None]
    return round(sum(xs) / len(xs), 2) if xs else None


def summarize(results, meta):
    def group(key):
        out = {}
        for r in results:
            g = out.setdefault(r[key], {"runs": 0, "passed": 0})
            g["runs"] += 1
            g["passed"] += int(r["passed"])
        for g in out.values():
            g["rate"] = _rate(g["passed"], g["runs"])
        return out

    by_task = {}
    for r in results:
        g = by_task.setdefault(r["task"], {"tier": r["tier"], "runs": 0, "passed": 0, "secs": [], "steps": []})
        g["runs"] += 1
        g["passed"] += int(r["passed"])
        g["secs"].append(r["secs"])
        g["steps"].append(r["metrics"]["steps"])
    for g in by_task.values():
        g["mean_secs"] = _mean(g.pop("secs"))
        g["mean_steps"] = _mean(g.pop("steps"))

    ms = [r["metrics"] for r in results]
    passed = sum(int(r["passed"]) for r in results)
    return {
        "meta": meta,
        "runs": len(results),
        "passed": passed,
        "pass_rate": _rate(passed, len(results)),
        "by_tier": group("tier"),
        "by_fixture": group("fixture"),
        "by_task": by_task,
        "failures": dict(Counter(r["failure"] for r in results if not r["passed"])),
        "means": {
            "secs": _mean([r["secs"] for r in results]),
            "steps": _mean([m["steps"] for m in ms]),
            "prompt_tokens": _mean([m["prompt_tokens"] for m in ms]),
            "completion_tokens": _mean([m["completion_tokens"] for m in ms]),
            "thinking_chars": _mean([m["thinking_chars"] for m in ms]),
            "tok_s": _mean([m["tok_s"] for m in ms]),
        },
        "tracker": {
            "runs_with_prompt_drops": sum(1 for m in ms if m["prompt_drops"] > 0),
            "mean_est_over_measured": _mean([m["est_ratio"] for m in ms]),
        },
    }


def print_summary(s):
    cfg = s["meta"].get("config") or {}
    print(f"\n== {s['meta'].get('label', '')}  {cfg}")
    print(f"overall: {s['passed']}/{s['runs']} = {s['pass_rate']:.0%}")
    print("\ntier       pass   rate")
    for t in ("explore", "edit", "fix", "multi"):
        g = s["by_tier"].get(t)
        if g:
            print(f"{t:<10} {g['passed']:>2}/{g['runs']:<3} {g['rate']:.0%}")
    print("\nfixture    pass   rate")
    for t, g in sorted(s["by_fixture"].items()):
        print(f"{t:<10} {g['passed']:>2}/{g['runs']:<3} {g['rate']:.0%}")
    print("\ntask               tier     pass  secs   steps")
    for t, g in s["by_task"].items():
        print(f"{t:<18} {g['tier']:<8} {g['passed']}/{g['runs']}   {g['mean_secs']:<6} {g['mean_steps']}")
    if s["failures"]:
        print("\nfailure classes:", ", ".join(f"{k}={v}" for k, v in sorted(s["failures"].items(), key=lambda kv: -kv[1])))
    m, tr = s["means"], s["tracker"]
    print(f"\nmeans: {m['secs']}s/run, {m['steps']} steps, {m['prompt_tokens']} prompt tok, "
          f"{m['completion_tokens']} gen tok, {m['tok_s']} tok/s, {m['thinking_chars']} thinking chars")
    print(f"tracker: runs with prompt-count drops = {tr['runs_with_prompt_drops']}, "
          f"mean estimate/measured = {tr['mean_est_over_measured']}")


def compare(a, b):
    print(f"\nA = {a['meta'].get('label')}   B = {b['meta'].get('label')}")
    print(f"{'':<12} {'A':>7} {'B':>7} {'delta':>7}   secs A/B")
    rows = [("overall", a, b, None)]
    rows += [(t, a["by_tier"].get(t), b["by_tier"].get(t), "tier") for t in ("explore", "edit", "fix", "multi")]

    def line(name, ra, rb, secs=""):
        if ra is None or rb is None:
            return
        ra_rate = ra.get("pass_rate", ra.get("rate"))
        rb_rate = rb.get("pass_rate", rb.get("rate"))
        print(f"{name:<12} {ra_rate:>7.0%} {rb_rate:>7.0%} {(rb_rate - ra_rate) * 100:>+6.0f}pt   {secs}")

    line("overall", a, b, f"{a['means']['secs']} / {b['means']['secs']}")
    for t in ("explore", "edit", "fix", "multi"):
        line(t, a["by_tier"].get(t), b["by_tier"].get(t))


def _load_summary(path):
    p = Path(path)
    p = p / "summary.json" if p.is_dir() else p
    return json.loads(p.read_text())


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "show":
        print_summary(_load_summary(sys.argv[2]))
    elif len(sys.argv) == 4 and sys.argv[1] == "compare":
        compare(_load_summary(sys.argv[2]), _load_summary(sys.argv[3]))
    else:
        print(__doc__)
        sys.exit(2)
