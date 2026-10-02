import assert from "node:assert/strict";
import test from "node:test";
import { readFile } from "node:fs/promises";
import ts from "typescript";

// paging.ts only type-imports from api.ts, so the transpiled module runs standalone.
// Executing the real merge logic (instead of regex-checking it) pins the page
// accounting that infinite scroll depends on.
const source = await readFile(new URL("../src/paging.ts", import.meta.url), "utf8");
const { outputText } = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
});
const { mergeServerPage, MAX_SEARCH_OFFSET } = await import(
  `data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`
);

const station = (id) => ({ id, name: `Station ${id}`, tags: [] });

test("stage 8 merge keeps unique rows while the next offset advances by the server page size", () => {
  const first = mergeServerPage(
    [],
    { request_id: "r1", stations: [station("a"), station("b")], total: 5, has_more: true },
    0
  );
  assert.deepEqual(first.stations.map((s) => s.id), ["a", "b"]);
  assert.equal(first.serverOffset, 2);
  assert.equal(first.hasMore, true);

  // The server repeats "b" in the next page; offset still advances by the full page.
  const second = mergeServerPage(
    first.stations,
    { request_id: "r2", stations: [station("b"), station("c")], total: 5, has_more: true },
    first.serverOffset
  );
  assert.deepEqual(second.stations.map((s) => s.id), ["a", "b", "c"]);
  assert.equal(second.addedCount, 1);
  assert.equal(second.serverOffset, 4);
});

test("stage 8 an empty page ends paging even when the server claims more", () => {
  const merged = mergeServerPage(
    [],
    { request_id: "r1", stations: [], total: 10, has_more: true },
    20
  );
  assert.equal(merged.hasMore, false);
  assert.equal(merged.boundary, false);
  assert.equal(merged.serverOffset, 20);
});

test("stage 8 has_more falls back to the server total when the flag is absent", () => {
  const exact = mergeServerPage([], { request_id: "r1", stations: [station("a")], total: 1 }, 0);
  assert.equal(exact.hasMore, false);
  const more = mergeServerPage([], { request_id: "r2", stations: [station("a")], total: 2 }, 0);
  assert.equal(more.hasMore, true);
  assert.equal(more.total, 2);
});

test("stage 8 crossing the server offset boundary stops paging explicitly", () => {
  // A request at offset 10 000 is the last one the server accepts.
  const merged = mergeServerPage(
    [],
    {
      request_id: "r1",
      stations: Array.from({ length: 20 }, (_, index) => station(`s${index}`)),
      total: 500,
      has_more: true,
    },
    MAX_SEARCH_OFFSET
  );
  assert.equal(merged.serverOffset, MAX_SEARCH_OFFSET + 20);
  assert.equal(merged.hasMore, false);
  assert.equal(merged.boundary, true);
});

test("stage 8 a page ending exactly at the total reports no more data without a boundary", () => {
  const merged = mergeServerPage(
    [station("a")],
    { request_id: "r1", stations: [station("b")], total: 2, has_more: false },
    1
  );
  assert.equal(merged.hasMore, false);
  assert.equal(merged.boundary, false);
});
