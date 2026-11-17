//! Runs the shared expression/action vectors (also run by the reader in TypeScript).

use darm_ir::expr::{apply, eval};
use darm_ir::{Action, Book, Expr, StateVar, Value};

fn book_with(state: Vec<StateVar>) -> Book {
    let json = serde_json::json!({
        "ir_version": "1.0", "grammar_version": 1,
        "meta": {"id": "t", "title": "T", "authors": ["A"], "language": "en", "layout": "reflow",
                 "page_aspect": 0.66, "offline": true, "generator": "test"},
        "theme": {"paper": "#ffffff", "ink": "#000000", "accent": "#333333",
                  "body_font": {"source": "builtin", "family": "serif"},
                  "heading_font": {"source": "builtin", "family": "serif"},
                  "paragraph": "indent", "justify": true, "hyphenate": true, "drop_caps": false},
        "cover": {"front": {"style": "classic", "color": "#000000", "ink": "#ffffff", "accent": "#cccccc",
                            "show_text": true, "title": "T", "author": "A"},
                  "back": {"color": "#000000", "ink": "#ffffff", "accent": "#cccccc"}},
        "chapters": [{"id": "c", "kind": "chapter", "title": "C"}],
    });
    let mut book: Book = serde_json::from_value(json).unwrap();
    book.state = state;
    book
}

#[test]
fn shared_vectors() {
    let v: serde_json::Value = serde_json::from_str(include_str!("vectors/expr.json")).unwrap();
    let decl: Vec<StateVar> = serde_json::from_value(v["state_decl"].clone()).unwrap();
    let book = book_with(decl);
    darm_ir::validate(&book).unwrap();
    let initial = darm_ir::expr::initial_state(&book);

    for (i, case) in v["exprs"].as_array().unwrap().iter().enumerate() {
        let e: Expr = serde_json::from_value(case["e"].clone()).unwrap();
        let got = eval(&e, &initial);
        if case["expect"].get("error").is_some() {
            assert!(got.is_err(), "expr #{i}: expected an error, got {got:?}");
        } else {
            let want: Value = serde_json::from_value(case["expect"].clone()).unwrap();
            assert_eq!(got.as_ref().ok(), Some(&want), "expr #{i}");
        }
    }

    for case in v["actions"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let actions: Vec<Action> = serde_json::from_value(case["run"].clone()).unwrap();
        let mut state = initial.clone();
        let fx = apply(&book, &actions, &mut state);
        for (k, want) in case["expect"].as_object().unwrap() {
            let want: Value = serde_json::from_value(want.clone()).unwrap();
            assert_eq!(state.get(k), Some(&want), "{name}: variable {k}");
        }
        assert_eq!(
            fx.goto.as_ref().map(|g| g.0.as_str()),
            case.get("goto").and_then(|g| g.as_str()),
            "{name}: goto"
        );
        assert_eq!(
            fx.reset_all,
            case.get("reset_all")
                .and_then(|r| r.as_bool())
                .unwrap_or(false),
            "{name}: reset_all"
        );
        assert!(
            !state.contains_key("ghost"),
            "{name}: unknown variables must not be created"
        );
    }
}
