import { test } from "node:test";
import assert from "node:assert/strict";
import { NoteStore } from "../src/store.ts";

test("add assigns increasing ids", () => {
  const s = new NoteStore();
  assert.equal(s.add("a").id, 1);
  assert.equal(s.add("b").id, 2);
});

test("ids continue after loading existing notes", () => {
  const s = new NoteStore([{ id: 5, text: "old", createdAt: "" }]);
  assert.equal(s.add("new").id, 6);
});

test("search is case-insensitive", () => {
  const s = new NoteStore();
  s.add("Buy Milk");
  s.add("call mom");
  assert.deepEqual(s.search("milk").map((n) => n.text), ["Buy Milk"]);
});

test("list returns a copy", () => {
  const s = new NoteStore();
  s.add("x");
  s.list().pop();
  assert.equal(s.list().length, 1);
});
