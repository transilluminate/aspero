//! Identifiers as written: invalid inputs keep their place, and DOIs whose
//! trailing punctuation belongs to them are found on a second try.

mod common;

use std::sync::Arc;

use aspero_core::{Doi, WorkId};
use aspero_engine::{Extras, InputLookup};
use common::{Scripted, engine, work};

#[tokio::test]
async fn inputs_keep_their_place_and_punctuated_dois_are_retried() {
    let mut dotted = work("crossref", &[], "Ends in a dot", &["A. Author"], 2020);
    dotted.ids = vec![WorkId::Doi(
        Doi::with_trailing_punctuation("10.1234/x.").unwrap(),
    )];
    let known = work(
        "crossref",
        &["10.1234/known"],
        "Known",
        &["A. Author"],
        2020,
    );
    let engine = engine(vec![Arc::new(
        Scripted::new("crossref").with(vec![dotted, known]),
    )]);
    let inputs = ["10.1234/x.", "not an id", "10.1234/known."].map(String::from);
    let report = engine.lookup_inputs(&inputs, Extras::default()).await;
    let looked = |i: usize| match &report.items[i] {
        InputLookup::Looked(l) => l,
        InputLookup::Invalid(e) => panic!("{e}"),
    };
    assert!(looked(0).result.record.is_some() && looked(0).with_punctuation);
    assert!(matches!(report.items[1], InputLookup::Invalid(_)));
    // Found as parsed (the dot read as punctuation): no retry needed.
    assert!(looked(2).result.record.is_some() && !looked(2).with_punctuation);
}
