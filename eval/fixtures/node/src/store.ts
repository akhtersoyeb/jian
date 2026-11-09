import type { Note } from "./types.ts";

export class NoteStore {
  private notes: Note[] = [];
  private nextId = 1;

  constructor(initial: Note[] = []) {
    this.notes = [...initial];
    this.nextId = initial.reduce((max, n) => Math.max(max, n.id), 0) + 1;
  }

  add(text: string): Note {
    const note: Note = { id: this.nextId++, text, createdAt: new Date().toISOString() };
    this.notes.push(note);
    return note;
  }

  list(): Note[] {
    return [...this.notes];
  }

  search(query: string): Note[] {
    const q = query.toLowerCase();
    return this.notes.filter((n) => n.text.toLowerCase().includes(q));
  }
}
