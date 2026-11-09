import { test } from "node:test";
import assert from "node:assert/strict";
import { formatNote, truncate } from "../src/format.ts";

test("truncate leaves short text alone", () => {
  assert.equal(truncate("hello", 10), "hello");
});

test("truncate shortens long text with an ellipsis", () => {
  assert.equal(truncate("hello world", 8), "hello...");
});

test("formatNote shows id and text", () => {
  assert.equal(formatNote({ id: 3, text: "hi", createdAt: "" }), "3: hi");
});
