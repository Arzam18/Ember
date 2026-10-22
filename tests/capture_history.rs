use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use ember_chess::board::{BoardState, INF};
use ember_chess::search::Searcher;
use ember_chess::tt::SharedTT;
use ember_chess::Engine;

fn state_from_fen(fen: &str) -> BoardState {
    let mut engine = Engine::new();
    engine.set_fen(fen);
    engine.st
}

// The contract under test is table population, export/import round-tripping, and
// clear-learning reset of capture_history; a TSV fixture asserts move choices and
// cannot observe the table itself.
#[test]
fn capture_cutoffs_populate_capture_history_and_roundtrip() {
    let mut st =
        state_from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1");
    let stopped = Arc::new(AtomicBool::new(false));
    let shared_tt = Arc::new(SharedTT::new(16));
    let mut searcher = Searcher::new(shared_tt, stopped);
    searcher.init_nnue_stack(&st);
    let mut nodes = 0u64;

    assert!(searcher
        .capture_history
        .iter()
        .flatten()
        .flatten()
        .all(|&v| v == 0));

    let _ = searcher.negamax(&mut st, 6, 0, -INF, INF, true, &mut nodes);

    let nonzero = searcher
        .capture_history
        .iter()
        .flatten()
        .flatten()
        .filter(|&&v| v != 0)
        .count();
    assert!(nonzero > 0, "capture cutoffs must populate capture_history");

    let mut other = Searcher::new(Arc::new(SharedTT::new(1)), Arc::new(AtomicBool::new(false)));
    other.import_learning(&searcher.export_learning());
    assert_eq!(other.capture_history, searcher.capture_history);

    other.clear_learning();
    assert!(other
        .capture_history
        .iter()
        .flatten()
        .flatten()
        .all(|&v| v == 0));
}
