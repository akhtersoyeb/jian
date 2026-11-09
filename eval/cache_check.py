#!/usr/bin/env python3
"""Does Ollama's prompt_eval_count report the FULL prompt size when its KV cache is hit?

Jian's context tracker assumes it does. If Ollama instead reports only the tokens it had to
re-evaluate, our "context %" runs low on every cached turn and we could overflow silently.

  python3 eval/cache_check.py [--model gemma4:e4b] [--host http://localhost:11434]
"""
import argparse
import json
import os
import urllib.request


def chat(host, model, text, num_ctx):
    body = {
        "model": model, "stream": False, "keep_alive": "10m",
        "options": {"num_ctx": num_ctx, "num_predict": 8, "temperature": 0},
        "messages": [{"role": "user", "content": text + "\n\nReply with the single word OK."}],
    }
    req = urllib.request.Request(f"{host}/api/chat", json.dumps(body).encode(),
                                 {"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=600) as r:
        return json.load(r)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", default="gemma4:e4b")
    ap.add_argument("--host", default=os.environ.get("OLLAMA_HOST") or "http://localhost:11434")
    ap.add_argument("--num-ctx", type=int, default=8192)
    a = ap.parse_args()
    host = a.host if "://" in a.host else f"http://{a.host}"

    base = " ".join(f"The quick brown fox jumps over the lazy dog, number {i}." for i in range(250))
    calls = [("A: fresh prompt", base),
             ("B: identical prompt (full cache hit)", base),
             ("C: same prefix + new tail", base + "\nAn extra paragraph that was not in the first prompt, " * 3)]
    got = []
    for name, text in calls:
        try:
            r = chat(host, a.model, text, a.num_ctx)
        except Exception as e:
            print(f"request failed: {e}")
            return 1
        n = r.get("prompt_eval_count")
        ms = (r.get("prompt_eval_duration") or 0) / 1e6
        got.append(n)
        print(f"{name:<40} prompt_eval_count={n}  prompt_eval_duration={ms:.0f} ms")

    A, B, C = got
    print()
    if A is None:
        print("Inconclusive: call A returned no prompt_eval_count.")
    elif B is None or B < 0.5 * A:
        print("RESULT: cached prompts are NOT counted in full (count is missing or much smaller on call B).")
        print("-> the measured value is not a reliable context size. V1 must size the prompt itself.")
    elif abs(B - A) <= 0.1 * A and (C or 0) > A:
        print("RESULT: OK. prompt_eval_count is the full prompt size even on cache hits.")
        print("-> check that call B's duration is much smaller than A's; that shows the cache is active.")
    else:
        print("RESULT: ambiguous; send me these numbers.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
