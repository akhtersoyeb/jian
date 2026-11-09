import fs from "node:fs";
import { NoteStore } from "./store.ts";
import { formatNote } from "./format.ts";
import type { Note } from "./types.ts";

const file = process.env.NOTES_FILE ?? "notes.json";

function load(): NoteStore {
  if (!fs.existsSync(file)) return new NoteStore();
  return new NoteStore(JSON.parse(fs.readFileSync(file, "utf8")) as Note[]);
}

function save(store: NoteStore): void {
  fs.writeFileSync(file, JSON.stringify(store.list(), null, 2));
}

const [command, ...rest] = process.argv.slice(2);
const store = load();

switch (command) {
  case "add": {
    const note = store.add(rest.join(" "));
    save(store);
    console.log(`added ${note.id}`);
    break;
  }
  case "list": {
    for (const note of store.list()) console.log(formatNote(note));
    break;
  }
  case "search": {
    for (const note of store.search(rest.join(" "))) console.log(formatNote(note));
    break;
  }
  default:
    console.error("usage: cli <add|list|search> [text]");
    process.exit(1);
}
