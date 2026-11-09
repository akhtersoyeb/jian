import type { Note } from "./types.ts";

export function truncate(text: string, max: number): string {
  if (text.length <= max) return text;
  return text.slice(0, max - 3) + "...";
}

export function formatNote(note: Note): string {
  return `${note.id}: ${truncate(note.text, 40)}`;
}
