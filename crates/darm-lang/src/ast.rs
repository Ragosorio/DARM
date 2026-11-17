//! The abstract syntax tree of a `.darml` file.
//!
//! The tree is small and uniform: a file is a list of statements, and a
//! statement is an element (`name args [: block]`), an assignment, an `if`
//! chain or a component definition. Every node carries its source [`Span`].
//! Which elements exist and what their arguments mean is decided later by
//! semantic analysis in `darm-compiler`.

use crate::diag::{FileId, Span};
use crate::lexer::Comment;

#[derive(Debug, Clone, PartialEq)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct File {
    pub file: FileId,
    /// `darm N` pragma: version and span. `None` when missing (already reported).
    pub pragma: Option<(i64, Span)>,
    pub stmts: Vec<Stmt>,
    pub comments: Vec<Comment>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    /// Span of the statement header (up to, not including, its block).
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    Element(Element),
    Assign {
        target: Ident,
        op: AssignOp,
        value: Expr,
    },
    If {
        branches: Vec<Branch>,
        otherwise: Option<Block>,
    },
    Component {
        name: Ident,
        params: Vec<Param>,
        body: Block,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    pub name: Ident,
    pub args: Vec<Arg>,
    pub body: Option<Block>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Arg {
    /// `Some` for `name=value` arguments.
    pub name: Option<Ident>,
    pub value: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    /// Span from the first statement to the end of the last one.
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Branch {
    /// Span of the `if` / `elif` keyword.
    pub keyword: Span,
    pub cond: Expr,
    pub body: Block,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: Ident,
    pub default: Option<Expr>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    Set,
    Add,
    Sub,
}

impl AssignOp {
    pub fn symbol(self) -> &'static str {
        match self {
            AssignOp::Set => "=",
            AssignOp::Add => "+=",
            AssignOp::Sub => "-=",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Int(i64),
    Decimal(f64),
    Percent(f64),
    /// Milliseconds.
    Duration(u32),
    Bool(bool),
    Str {
        value: String,
        block: bool,
    },
    Name(String),
    List(Vec<Expr>),
    Tuple(Vec<Expr>),
    Call {
        name: Ident,
        args: Vec<Arg>,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// `lo..hi`, only valid as the domain of `in`.
    Range {
        lo: Box<Expr>,
        hi: Box<Expr>,
    },
    /// `value in domain` (state declarations).
    In {
        value: Box<Expr>,
        domain: Box<Expr>,
    },
    Paren(Box<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Not,
    Neg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
}

impl BinaryOp {
    pub fn symbol(self) -> &'static str {
        match self {
            BinaryOp::Or => "or",
            BinaryOp::And => "and",
            BinaryOp::Eq => "==",
            BinaryOp::Ne => "!=",
            BinaryOp::Lt => "<",
            BinaryOp::Le => "<=",
            BinaryOp::Gt => ">",
            BinaryOp::Ge => ">=",
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
        }
    }

    /// Binding strength, higher binds tighter.
    pub fn precedence(self) -> u8 {
        match self {
            BinaryOp::Or => 1,
            BinaryOp::And => 2,
            BinaryOp::Eq
            | BinaryOp::Ne
            | BinaryOp::Lt
            | BinaryOp::Le
            | BinaryOp::Gt
            | BinaryOp::Ge => 4,
            BinaryOp::Add | BinaryOp::Sub => 5,
            BinaryOp::Mul => 6,
        }
    }
}

impl Expr {
    /// The expression without redundant parentheses.
    pub fn unparen(&self) -> &Expr {
        match &self.kind {
            ExprKind::Paren(inner) => inner.unparen(),
            _ => self,
        }
    }

    /// A short description for "expected X, found Y" messages.
    pub fn describe(&self) -> &'static str {
        match &self.kind {
            ExprKind::Int(_) => "an integer",
            ExprKind::Decimal(_) => "a decimal number",
            ExprKind::Percent(_) => "a percentage",
            ExprKind::Duration(_) => "a duration",
            ExprKind::Bool(_) => "a boolean",
            ExprKind::Str { .. } => "a string",
            ExprKind::Name(_) => "a name",
            ExprKind::List(_) => "a list",
            ExprKind::Tuple(_) => "a tuple",
            ExprKind::Call { .. } => "a call",
            ExprKind::Unary { .. } | ExprKind::Binary { .. } => "an expression",
            ExprKind::Range { .. } => "a range",
            ExprKind::In { .. } => "an `in` expression",
            ExprKind::Paren(inner) => inner.describe(),
        }
    }
}

/// Structural equality that ignores spans and comments. Used to prove that
/// formatting never changes meaning.
pub fn same_meaning(a: &File, b: &File) -> bool {
    fn strip_expr(e: &Expr) -> Expr {
        let kind = match &e.kind {
            ExprKind::List(v) => ExprKind::List(v.iter().map(strip_expr).collect()),
            ExprKind::Tuple(v) => ExprKind::Tuple(v.iter().map(strip_expr).collect()),
            ExprKind::Call { name, args } => ExprKind::Call {
                name: strip_ident(name),
                args: args.iter().map(strip_arg).collect(),
            },
            ExprKind::Unary { op, expr } => ExprKind::Unary {
                op: *op,
                expr: Box::new(strip_expr(expr)),
            },
            ExprKind::Binary { op, left, right } => ExprKind::Binary {
                op: *op,
                left: Box::new(strip_expr(left)),
                right: Box::new(strip_expr(right)),
            },
            ExprKind::Range { lo, hi } => ExprKind::Range {
                lo: Box::new(strip_expr(lo)),
                hi: Box::new(strip_expr(hi)),
            },
            ExprKind::In { value, domain } => ExprKind::In {
                value: Box::new(strip_expr(value)),
                domain: Box::new(strip_expr(domain)),
            },
            ExprKind::Paren(inner) => ExprKind::Paren(Box::new(strip_expr(inner))),
            other => other.clone(),
        };
        Expr {
            kind,
            span: Span::default(),
        }
    }
    fn strip_ident(i: &Ident) -> Ident {
        Ident {
            name: i.name.clone(),
            span: Span::default(),
        }
    }
    fn strip_arg(a: &Arg) -> Arg {
        Arg {
            name: a.name.as_ref().map(strip_ident),
            value: strip_expr(&a.value),
        }
    }
    fn strip_block(b: &Block) -> Block {
        Block {
            stmts: b.stmts.iter().map(strip_stmt).collect(),
            span: Span::default(),
        }
    }
    fn strip_stmt(s: &Stmt) -> Stmt {
        let kind = match &s.kind {
            StmtKind::Element(e) => StmtKind::Element(Element {
                name: strip_ident(&e.name),
                args: e.args.iter().map(strip_arg).collect(),
                body: e.body.as_ref().map(strip_block),
            }),
            StmtKind::Assign { target, op, value } => StmtKind::Assign {
                target: strip_ident(target),
                op: *op,
                value: strip_expr(value),
            },
            StmtKind::If {
                branches,
                otherwise,
            } => StmtKind::If {
                branches: branches
                    .iter()
                    .map(|b| Branch {
                        keyword: Span::default(),
                        cond: strip_expr(&b.cond),
                        body: strip_block(&b.body),
                    })
                    .collect(),
                otherwise: otherwise.as_ref().map(strip_block),
            },
            StmtKind::Component { name, params, body } => StmtKind::Component {
                name: strip_ident(name),
                params: params
                    .iter()
                    .map(|p| Param {
                        name: strip_ident(&p.name),
                        default: p.default.as_ref().map(strip_expr),
                    })
                    .collect(),
                body: strip_block(body),
            },
        };
        Stmt {
            kind,
            span: Span::default(),
        }
    }
    a.pragma.map(|p| p.0) == b.pragma.map(|p| p.0)
        && a.stmts.iter().map(strip_stmt).collect::<Vec<_>>()
            == b.stmts.iter().map(strip_stmt).collect::<Vec<_>>()
}
