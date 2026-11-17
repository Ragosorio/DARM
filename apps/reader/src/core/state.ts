/**
 * Expression evaluation and actions: the reader-side implementation of the
 * semantics defined in crates/darm-ir/src/expr.rs. Both run the shared
 * vectors in crates/darm-ir/tests/vectors/expr.json.
 *
 * Expressions are finite trees with no calls or loops, so evaluation always
 * terminates. Integer arithmetic saturates at ±(2^53 − 1), where JavaScript
 * numbers and Rust's i64 agree exactly.
 */
import type { Action, Book, Expr, State, StateVar, Transition, Value } from "../ir";

export const INT_SATURATION = Number.MAX_SAFE_INTEGER;

export class EvalError extends Error {}

function sat(v: number): number {
  return Math.max(-INT_SATURATION, Math.min(INT_SATURATION, v));
}

function int(v: Value): number {
  if (typeof v !== "number") throw new EvalError("expected int");
  return v;
}

function bool(v: Value): boolean {
  if (typeof v !== "boolean") throw new EvalError("expected bool");
  return v;
}

export function evaluate(e: Expr, state: State): Value {
  switch (e.op) {
    case "int":
      return sat(e.value);
    case "bool":
    case "str":
      return e.value;
    case "var": {
      const v = Object.prototype.hasOwnProperty.call(state, e.name) ? state[e.name] : undefined;
      if (v === undefined) throw new EvalError(`unknown state variable ${e.name}`);
      return v;
    }
    case "not":
      return !bool(evaluate(e.expr, state));
    case "neg":
      return sat(-int(evaluate(e.expr, state)));
    case "bin": {
      // `and` / `or` short-circuit, like Python.
      if (e.kind === "and") return bool(evaluate(e.left, state)) && bool(evaluate(e.right, state));
      if (e.kind === "or") return bool(evaluate(e.left, state)) || bool(evaluate(e.right, state));
      const l = evaluate(e.left, state);
      const r = evaluate(e.right, state);
      switch (e.kind) {
        case "add":
          return sat(int(l) + int(r));
        case "sub":
          return sat(int(l) - int(r));
        case "mul":
          return sat(int(l) * int(r));
        case "eq":
        case "ne":
          if (typeof l !== typeof r) throw new EvalError("cannot compare values of different types");
          return (l === r) === (e.kind === "eq");
        case "lt":
          return int(l) < int(r);
        case "le":
          return int(l) <= int(r);
        case "gt":
          return int(l) > int(r);
        case "ge":
          return int(l) >= int(r);
      }
    }
  }
}

/** An absent condition holds; one that fails to evaluate is false. */
export function holds(when: Expr | undefined, state: State): boolean {
  if (!when) return true;
  try {
    return evaluate(when, state) === true;
  } catch {
    return false;
  }
}

export function initialState(book: Book): State {
  const s: State = {};
  for (const v of book.state) s[v.name] = v.initial;
  return s;
}

/** Coerce into the variable's domain; `undefined` means "ignore the assignment". */
export function coerce(decl: StateVar, v: Value): Value | undefined {
  if (decl.type === "int" && typeof v === "number") return Math.max(decl.min, Math.min(decl.max, v));
  if (decl.type === "bool" && typeof v === "boolean") return v;
  if (decl.type === "enum" && typeof v === "string" && decl.values.includes(v)) return v;
  return undefined;
}

export interface Effects {
  goto?: { target: string; transition?: Transition };
  resetAll: boolean;
}

/** Run actions in order. Never throws: an assignment that cannot be computed is skipped. */
export function applyActions(book: Book, actions: Action[], state: State): { state: State; effects: Effects } {
  let next: State = { ...state };
  const effects: Effects = { resetAll: false };
  for (const a of actions) {
    if (a.do === "set") {
      const decl = book.state.find((v) => v.name === a.var);
      if (!decl) continue;
      try {
        const v = coerce(decl, evaluate(a.value, next));
        if (v !== undefined) next[a.var] = v;
      } catch {
        /* skipped by definition */
      }
    } else if (a.do === "goto") {
      effects.goto = { target: a.target, transition: a.transition };
    } else if (a.var !== undefined) {
      const decl = book.state.find((v) => v.name === a.var);
      if (decl) next[a.var] = decl.initial;
    } else {
      next = initialState(book);
      effects.resetAll = true;
    }
  }
  return { state: next, effects };
}

/** Text shown for `{name}` interpolation. */
export function displayValue(v: Value | undefined): string {
  if (v === undefined) return "";
  if (typeof v === "boolean") return v ? "yes" : "no";
  return String(v);
}

/** Sanitise persisted state against the book's declarations (stale or tampered storage). */
export function reconcileState(book: Book, saved: unknown): State {
  const out = initialState(book);
  if (saved && typeof saved === "object") {
    for (const decl of book.state) {
      const raw = (saved as Record<string, unknown>)[decl.name];
      if (typeof raw === "number" || typeof raw === "boolean" || typeof raw === "string") {
        const v = coerce(decl, raw);
        if (v !== undefined && (typeof v !== "number" || Number.isSafeInteger(v))) out[decl.name] = v;
      }
    }
  }
  return out;
}
