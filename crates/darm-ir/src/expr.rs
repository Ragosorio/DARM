//! Reference semantics of DARM expressions and actions.
//!
//! The language is deliberately not Turing-complete: expressions are finite
//! trees without calls or loops, and evaluation always terminates. Integer
//! arithmetic saturates, and assignments clamp to the variable's range, so
//! state is bounded by construction.
//!
//! The reader implements the same rules in TypeScript; both implementations
//! run the shared vectors in `tests/vectors/expr.json`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Action, BinOp, Book, Expr, StateType, StateVar};

/// Integer results saturate at ±(2^53 − 1), the largest range in which the
/// reader's JavaScript numbers and Rust's `i64` agree exactly.
pub const INT_SATURATION: i64 = 9_007_199_254_740_991;

fn sat(v: i64) -> i64 {
    v.clamp(-INT_SATURATION, INT_SATURATION)
}

/// A runtime value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Int(i64),
    Bool(bool),
    Str(String),
}

/// Static type of an expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ty {
    Int,
    Bool,
    Str,
}

impl Ty {
    pub fn name(self) -> &'static str {
        match self {
            Ty::Int => "int",
            Ty::Bool => "bool",
            Ty::Str => "text",
        }
    }
}

impl Value {
    pub fn ty(&self) -> Ty {
        match self {
            Value::Int(_) => Ty::Int,
            Value::Bool(_) => Ty::Bool,
            Value::Str(_) => Ty::Str,
        }
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Int(i) => write!(f, "{i}"),
            Value::Bool(b) => write!(f, "{}", if *b { "yes" } else { "no" }),
            Value::Str(s) => f.write_str(s),
        }
    }
}

/// Variable name → current value.
pub type State = BTreeMap<String, Value>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvalError {
    UnknownVar(String),
    Type { expected: Ty, found: Ty },
}

impl std::fmt::Display for EvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EvalError::UnknownVar(v) => write!(f, "unknown state variable `{v}`"),
            EvalError::Type { expected, found } => {
                write!(f, "expected {} but found {}", expected.name(), found.name())
            }
        }
    }
}

impl std::error::Error for EvalError {}

impl StateVar {
    pub fn initial(&self) -> Value {
        match &self.ty {
            StateType::Int { initial, .. } => Value::Int(*initial),
            StateType::Bool { initial } => Value::Bool(*initial),
            StateType::Enum { initial, .. } => Value::Str(initial.clone()),
        }
    }

    pub fn ty(&self) -> Ty {
        match &self.ty {
            StateType::Int { .. } => Ty::Int,
            StateType::Bool { .. } => Ty::Bool,
            StateType::Enum { .. } => Ty::Str,
        }
    }

    /// Coerce `v` into this variable's domain. Ints clamp; an enum value outside
    /// the set, or a value of the wrong type, yields `None` (assignment ignored).
    pub fn coerce(&self, v: Value) -> Option<Value> {
        match (&self.ty, v) {
            (StateType::Int { min, max, .. }, Value::Int(i)) => {
                Some(Value::Int(i.clamp(*min, *max)))
            }
            (StateType::Bool { .. }, Value::Bool(b)) => Some(Value::Bool(b)),
            (StateType::Enum { values, .. }, Value::Str(s)) if values.contains(&s) => {
                Some(Value::Str(s))
            }
            _ => None,
        }
    }
}

/// Initial state of a book.
pub fn initial_state(book: &Book) -> State {
    book.state
        .iter()
        .map(|v| (v.name.clone(), v.initial()))
        .collect()
}

fn int(v: Value) -> Result<i64, EvalError> {
    match v {
        Value::Int(i) => Ok(i),
        other => Err(EvalError::Type {
            expected: Ty::Int,
            found: other.ty(),
        }),
    }
}

fn boolean(v: Value) -> Result<bool, EvalError> {
    match v {
        Value::Bool(b) => Ok(b),
        other => Err(EvalError::Type {
            expected: Ty::Bool,
            found: other.ty(),
        }),
    }
}

/// Evaluate `expr` against `state`.
pub fn eval(expr: &Expr, state: &State) -> Result<Value, EvalError> {
    Ok(match expr {
        Expr::Int { value } => Value::Int(sat(*value)),
        Expr::Bool { value } => Value::Bool(*value),
        Expr::Str { value } => Value::Str(value.clone()),
        Expr::Var { name } => state
            .get(name)
            .cloned()
            .ok_or_else(|| EvalError::UnknownVar(name.clone()))?,
        Expr::Not { expr } => Value::Bool(!boolean(eval(expr, state)?)?),
        Expr::Neg { expr } => Value::Int(sat(int(eval(expr, state)?)?.saturating_neg())),
        Expr::Bin { kind, left, right } => match kind {
            // `and` / `or` short-circuit, like Python.
            BinOp::And => {
                Value::Bool(boolean(eval(left, state)?)? && boolean(eval(right, state)?)?)
            }
            BinOp::Or => Value::Bool(boolean(eval(left, state)?)? || boolean(eval(right, state)?)?),
            BinOp::Add => Value::Int(sat(
                int(eval(left, state)?)?.saturating_add(int(eval(right, state)?)?)
            )),
            BinOp::Sub => Value::Int(sat(
                int(eval(left, state)?)?.saturating_sub(int(eval(right, state)?)?)
            )),
            BinOp::Mul => Value::Int(sat(
                int(eval(left, state)?)?.saturating_mul(int(eval(right, state)?)?)
            )),
            BinOp::Eq | BinOp::Ne => {
                let (l, r) = (eval(left, state)?, eval(right, state)?);
                if l.ty() != r.ty() {
                    return Err(EvalError::Type {
                        expected: l.ty(),
                        found: r.ty(),
                    });
                }
                Value::Bool((l == r) == (*kind == BinOp::Eq))
            }
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let (l, r) = (int(eval(left, state)?)?, int(eval(right, state)?)?);
                Value::Bool(match kind {
                    BinOp::Lt => l < r,
                    BinOp::Le => l <= r,
                    BinOp::Gt => l > r,
                    _ => l >= r,
                })
            }
        },
    })
}

/// Evaluate a condition. Absent conditions hold; a condition that fails to
/// evaluate (only possible with a corrupted IR) is treated as `false`.
pub fn holds(when: Option<&Expr>, state: &State) -> bool {
    match when {
        None => true,
        Some(e) => matches!(eval(e, state), Ok(Value::Bool(true))),
    }
}

/// Outcome of running a list of actions.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Effects {
    /// Navigation requested by the last `goto` executed, if any.
    pub goto: Option<(String, Option<crate::Transition>)>,
    /// `true` when a bare `reset` ran: the reader must also clear quiz/choice records.
    pub reset_all: bool,
}

/// Run `actions` in order against `state`. Never fails: an assignment whose
/// value cannot be computed or coerced is skipped.
pub fn apply(book: &Book, actions: &[Action], state: &mut State) -> Effects {
    let mut fx = Effects::default();
    for action in actions {
        match action {
            Action::Set { var, value } => {
                if let (Some(decl), Ok(v)) = (book.state_var(var), eval(value, state))
                    && let Some(v) = decl.coerce(v)
                {
                    state.insert(var.clone(), v);
                }
            }
            Action::Goto { target, transition } => fx.goto = Some((target.clone(), *transition)),
            Action::Reset { var: Some(var) } => {
                if let Some(decl) = book.state_var(var) {
                    state.insert(var.clone(), decl.initial());
                }
            }
            Action::Reset { var: None } => {
                *state = initial_state(book);
                fx.reset_all = true;
            }
        }
    }
    fx
}

/// Static type of `expr` given the declared variables, or a description of the first type error.
pub fn type_of(expr: &Expr, vars: &dyn Fn(&str) -> Option<Ty>) -> Result<Ty, String> {
    let want = |e: &Expr, t: Ty, what: &str| -> Result<(), String> {
        let found = type_of(e, vars)?;
        if found == t {
            Ok(())
        } else {
            Err(format!(
                "{what} needs {} but got {}",
                t.name(),
                found.name()
            ))
        }
    };
    Ok(match expr {
        Expr::Int { .. } => Ty::Int,
        Expr::Bool { .. } => Ty::Bool,
        Expr::Str { .. } => Ty::Str,
        Expr::Var { name } => {
            vars(name).ok_or_else(|| format!("unknown state variable `{name}`"))?
        }
        Expr::Not { expr } => {
            want(expr, Ty::Bool, "`not`")?;
            Ty::Bool
        }
        Expr::Neg { expr } => {
            want(expr, Ty::Int, "unary `-`")?;
            Ty::Int
        }
        Expr::Bin { kind, left, right } => match kind {
            BinOp::And | BinOp::Or => {
                let op = if *kind == BinOp::And { "`and`" } else { "`or`" };
                want(left, Ty::Bool, op)?;
                want(right, Ty::Bool, op)?;
                Ty::Bool
            }
            BinOp::Add | BinOp::Sub | BinOp::Mul => {
                want(left, Ty::Int, "arithmetic")?;
                want(right, Ty::Int, "arithmetic")?;
                Ty::Int
            }
            BinOp::Eq | BinOp::Ne => {
                let (l, r) = (type_of(left, vars)?, type_of(right, vars)?);
                if l != r {
                    return Err(format!("cannot compare {} with {}", l.name(), r.name()));
                }
                Ty::Bool
            }
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                want(left, Ty::Int, "ordering comparison")?;
                want(right, Ty::Int, "ordering comparison")?;
                Ty::Bool
            }
        },
    })
}

/// Depth of an expression tree (used to bound hostile IR).
pub fn depth(expr: &Expr) -> usize {
    match expr {
        Expr::Not { expr } | Expr::Neg { expr } => 1 + depth(expr),
        Expr::Bin { left, right, .. } => 1 + depth(left).max(depth(right)),
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn var(n: &str) -> Expr {
        Expr::Var { name: n.into() }
    }
    fn bin(kind: BinOp, l: Expr, r: Expr) -> Expr {
        Expr::Bin {
            kind,
            left: Box::new(l),
            right: Box::new(r),
        }
    }

    #[test]
    fn arithmetic_saturates() {
        let s = State::new();
        let e = bin(
            BinOp::Add,
            Expr::Int { value: i64::MAX },
            Expr::Int { value: 5 },
        );
        assert_eq!(eval(&e, &s), Ok(Value::Int(INT_SATURATION)));
        let e = Expr::Neg {
            expr: Box::new(Expr::Int { value: i64::MIN }),
        };
        assert_eq!(eval(&e, &s), Ok(Value::Int(INT_SATURATION)));
    }

    #[test]
    fn short_circuit_hides_errors_like_python() {
        let s = State::new();
        let e = bin(BinOp::Or, Expr::Bool { value: true }, var("missing"));
        assert_eq!(eval(&e, &s), Ok(Value::Bool(true)));
        let e = bin(BinOp::And, Expr::Bool { value: true }, var("missing"));
        assert_eq!(eval(&e, &s), Err(EvalError::UnknownVar("missing".into())));
    }

    #[test]
    fn coerce_clamps_and_rejects() {
        let v = StateVar {
            name: "n".into(),
            ty: StateType::Int {
                initial: 0,
                min: 0,
                max: 3,
            },
        };
        assert_eq!(v.coerce(Value::Int(99)), Some(Value::Int(3)));
        assert_eq!(v.coerce(Value::Bool(true)), None);
        let e = StateVar {
            name: "r".into(),
            ty: StateType::Enum {
                initial: "a".into(),
                values: vec!["a".into(), "b".into()],
            },
        };
        assert_eq!(e.coerce(Value::Str("c".into())), None);
        assert_eq!(
            e.coerce(Value::Str("b".into())),
            Some(Value::Str("b".into()))
        );
    }

    #[test]
    fn type_errors_are_described() {
        let vars = |n: &str| if n == "n" { Some(Ty::Int) } else { None };
        let e = bin(BinOp::And, var("n"), Expr::Bool { value: true });
        assert!(type_of(&e, &vars).unwrap_err().contains("needs bool"));
        let e = bin(BinOp::Eq, var("n"), Expr::Str { value: "x".into() });
        assert!(type_of(&e, &vars).unwrap_err().contains("cannot compare"));
    }
}
