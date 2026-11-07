# Jian Code

A coding agent for your terminal, like Claude Code, but **fully local**. It runs a model
through [Ollama](https://ollama.com) on your machine: no account, no API key, no rate limits,
no internet needed after setup.

It can search and read your code, edit files, and run commands (tests, builds), asking for
your approval before every change.

## Quickstart

1. **Install Ollama** from https://ollama.com and make sure it is running.
2. **Pull the model:**
   ```bash
   ollama pull gemma4:e4b
   ```
3. **Install Jian Code**, using one of these:
   - Download the archive for your platform from the
     [Releases](https://github.com/<you>/jian/releases) page, extract it, and put `jian` on your `PATH`:
     ```bash
     tar xzf jian-v0.1.0-*.tar.gz
     install -m 755 jian-v0.1.0-*/jian ~/.local/bin/jian
     ```
   - Or build from source (recent stable Rust required):
     ```bash
     cargo install --git https://github.com/<you>/jian
     ```
4. **Check your setup:**
   ```bash
   jian doctor
   ```
5. **Use it** from any project directory:
   ```bash
   cd ~/my-project
   jian
   ```

## Usage

```
jian                          interactive session in the current directory
jian -p "explain this repo"   one-shot; the answer goes to stdout, progress to stderr
jian -p "..." --yes           one-shot with every edit/command auto-approved (see Safety)
jian doctor                   diagnose Ollama, the model and the config
jian --model other:tag        use a different Ollama model
```

In the session: `/help`, `/clear`, `/context`, `/exit`. End a line with `\` to continue it.
Ctrl-C cancels the current response.

## Safety model

- **Reads** (`list_dir`, `grep`, `read_file`) are automatic and confined to the project directory.
- **Edits, new files and shell commands** show a preview (a diff for edits, the exact command
  for `bash`) and wait for `y`/`n`. `a` approves that kind of action for the rest of the session.
  Compound commands (`;`, `&&`, pipes) and risky ones (`rm`, `sudo`, `curl`, ...) never get `a`.
- The path sandbox covers file tools only. **`bash` runs with your full user privileges**:
  read the command before pressing `y`.
- `--yes` skips every prompt. Use it only in throwaway directories or containers.
- The only network traffic is to your Ollama server.
- There is no undo. Work in a git repo and use `git diff` / `git checkout`.

## Project instructions

Put a `JIAN.md` in your project root (build commands, conventions, things to avoid). It is
added to the system prompt at the start of every session.

## Configuration

Optional file (run `jian doctor` to see the exact path on your OS):
Linux `~/.config/jian/config.toml`, macOS `~/Library/Application Support/jian/config.toml`,
Windows `%APPDATA%\jian\config.toml`.

```toml
model = "gemma4:e4b"
host = "http://localhost:11434"   # OLLAMA_HOST is also respected
num_ctx = 16384                   # context window in tokens; lower it if RAM is tight
think = false                     # omit to let the model decide; false is faster
```

Precedence for the host: `--host` flag, then config file, then `OLLAMA_HOST`, then the default.

## Limitations

- Small local models are slower and less reliable than hosted ones. Keep tasks well-scoped.
- No conversation compaction yet: long sessions hit the context limit (old tool output is
  trimmed automatically; use `/clear`).
- `https://` Ollama hosts are not supported.
- Windows support is experimental.

## Troubleshooting

- **"Can't reach Ollama"**: start it with `ollama serve`, or fix `host` / `OLLAMA_HOST`.
- **"Model isn't installed"**: `ollama pull <model>`.
- **macOS says the binary can't be opened**: `xattr -d com.apple.quarantine jian`
  (it is unsigned; downloading with `curl` avoids the quarantine flag).
- **Windows SmartScreen warning**: the binary is unsigned; choose "Run anyway".
- Anything else: run `jian doctor` first.
