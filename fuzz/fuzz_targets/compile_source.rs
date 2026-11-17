#![no_main]
//! Fuzzes the whole front end: lexer, parser, formatter, semantic analysis
//! and lowering. Invariants: nothing panics; a successful compile always
//! yields IR that passes validation; formatting accepted source is stable
//! and never changes its meaning.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(source) = std::str::from_utf8(data) else { return };
    let compilation = darm_compiler::compile_str(source);
    if let Some(book) = &compilation.book {
        darm_ir::validate(book).expect("compiled IR must validate");
        let json = book.to_json();
        let back = darm_ir::Book::from_json(&json).expect("IR must round-trip through JSON");
        assert_eq!(&back, book);
    }
    let _ = compilation.render(false);
    if let Ok(formatted) = darm_lang::fmt::format_source(darm_lang::FileId(0), "fuzz.darml", source) {
        let again = darm_lang::fmt::format_source(darm_lang::FileId(0), "fuzz.darml", &formatted).expect("formatted source must parse");
        assert_eq!(formatted, again, "formatting must be idempotent");
        let a = darm_lang::parse(darm_lang::FileId(0), source).file;
        let b = darm_lang::parse(darm_lang::FileId(0), &formatted).file;
        assert!(darm_lang::ast::same_meaning(&a, &b), "formatting must not change meaning");
    }
});
