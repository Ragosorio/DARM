import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import type { Action, Book, Expr, StateVar } from "../src/ir";
import { applyActions, evaluate, initialState, reconcileState } from "../src/core/state";

const vectors = JSON.parse(
  readFileSync(new URL("../../../crates/darm-ir/tests/vectors/expr.json", import.meta.url), "utf8"),
) as {
  state_decl: StateVar[];
  exprs: { e: Expr; expect: unknown }[];
  actions: { name: string; run: Action[]; expect: Record<string, unknown>; goto?: string; transition?: string; reset_all?: boolean }[];
};

const book = { state: vectors.state_decl } as unknown as Book;

describe("shared expression vectors (same file as the Rust tests)", () => {
  const initial = initialState(book);
  vectors.exprs.forEach((c, i) => {
    it(`expr #${i}`, () => {
      if (c.expect !== null && typeof c.expect === "object") {
        expect(() => evaluate(c.e, initial)).toThrow();
      } else {
        expect(evaluate(c.e, initial)).toStrictEqual(c.expect);
      }
    });
  });
  for (const c of vectors.actions) {
    it(`actions: ${c.name}`, () => {
      const { state, effects } = applyActions(book, c.run, initial);
      for (const [k, v] of Object.entries(c.expect)) expect(state[k]).toStrictEqual(v);
      expect(effects.goto?.target).toBe(c.goto);
      if (c.transition) expect(effects.goto?.transition).toBe(c.transition);
      expect(effects.resetAll).toBe(c.reset_all ?? false);
      expect("ghost" in state).toBe(false);
      // The input state is never mutated.
      expect(initial).toStrictEqual(initialState(book));
    });
  }
});

describe("persisted state is sanitised", () => {
  it("drops unknown keys, wrong types and out-of-set values; clamps ints", () => {
    const s = reconcileState(book, { n: 99, flag: "yes", route: "moon", ghost: 1, big: -5e12, __proto__: { x: 1 } });
    expect(s).toStrictEqual({ n: 5, big: -1000000000, flag: false, route: "shore" });
    expect(reconcileState(book, null)).toStrictEqual(initialState(book));
    expect(reconcileState(book, "garbage")).toStrictEqual(initialState(book));
  });
});
