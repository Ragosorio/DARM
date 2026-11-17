#![no_main]
//! Fuzzes the reader's trust boundary for book data: arbitrary JSON must
//! either be rejected or yield a book on which search and state evaluation
//! never panic.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(json) = std::str::from_utf8(data) else { return };
    let Ok(book) = darm_ir::Book::from_json(json) else { return };
    if darm_ir::validate(&book).is_err() {
        return;
    }
    let _ = darm_ir::search::search(&book, "the", 50);
    let mut state = darm_ir::expr::initial_state(&book);
    book.walk_blocks(|_, block| {
        if let darm_ir::BlockKind::Button { actions, .. } = &block.kind {
            let _ = darm_ir::expr::apply(&book, actions, &mut state);
        }
    });
});
