# Jian Code eval v2

24 tasks (explore / edit / fix / multi x python, rust, node). Each run: fresh copy of a fixture,
`jian -y -p "<prompt>" --transcript ...`, then a verifier checks the result with real
commands. Needs: Python 3.9+, git, and `cargo` (rust tasks) / Node >= 22.18 (node tasks);
tasks whose tool is missing are skipped. POSIX only (macOS/Linux).

    python3 eval/run.py --selftest          # no model: checks the fixtures + verifiers (must be 24/24)
    python3 eval/run.py --list
    python3 eval/run.py --runs 1 --fixtures python --label smoke
    python3 eval/run.py --runs 3 --label baseline
    python3 eval/matrix.py --runs 1         # one-factor-at-a-time config sweep
    python3 eval/analyze.py compare eval/results/A eval/results/B
    python3 eval/cache_check.py             # does prompt_eval_count include cached tokens?

Results go to `eval/results/<stamp>-<label>/`: `results.jsonl` (one line per run),
`summary.json`, `transcripts/` (full JSONL per run), `logs/` (jian stdout/stderr).

Runs use your real jian config for anything you don't override; the effective values are
recorded in each transcript and in the summary.

Failure classes (heuristic, from the transcript): timeout, context, ollama_error, crash, loop,
step_limit, empty_response, edit_mismatch, hallucinated_path, no_action, wrong_answer, wrong_change.
