# notes-cli

A tiny notes CLI (Node 22.18+, TypeScript via built-in type stripping, no dependencies).

Usage: `node src/cli.ts add "buy milk"`, `node src/cli.ts list`, `node src/cli.ts search milk`

Notes are kept by the `NoteStore` class in `src/store.ts` and saved to a JSON file.
