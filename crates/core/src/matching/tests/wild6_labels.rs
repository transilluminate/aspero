//! Red-team wild-6: a model's list of papers, each a label beside its
//! arXiv identifier ("Gated SAEs: 2404.16014", "Lookahead Decoding |
//! 2402.02057"), no authors.

use super::*;

/// wild-6 #578: the whole title, "SAEs" for its "Sparse Autoencoders" (a
/// compound's two initials), and the id: verified.
#[test]
fn a_title_with_a_compounds_acronym_and_its_id_verifies() {
    let r = arxiv_preprint(
        "arXiv:2502.04878",
        "Sparse Autoencoders Do Not Find Canonical Units of Analysis",
        &["Patrick Leask", "Bart Bussmann"],
        2025,
    );
    let a = assess(
        &Claim::from_text("SAEs Do Not Find Canonical Units of Analysis: 2502.04878"),
        &r,
    );
    assert_eq!(a.title.agreement, Agreement::Match, "{a:?}");
    assert!(a.confidence >= VERIFIED, "{a:?}");
}

/// wild-3 #454, #1042: no compound read into a word that isn't one ("GAT"
/// isn't "Graph Attention"), and an acronym its words follow, written out
/// ("MAE | Masked Autoencoders …"), defines them.
#[test]
fn acronyms_take_a_compound_only_where_one_is() {
    let expand = super::super::titles::expand_acronyms;
    assert_eq!(
        expand("Gated SAEs", "Gated Sparse Autoencoders").as_deref(),
        Some("Gated Sparse Autoencoders")
    );
    assert_eq!(expand("GAT", "Graph Attention Networks"), None);
    assert_eq!(
        expand(
            "MAE | Masked Autoencoders Are Scalable Vision Learners",
            "Masked Autoencoders Are Scalable Vision Learners"
        ),
        None
    );
    // Written out before it, as BibTeX's "Network ({RED-CNN})": it's
    // expanded (wild-6 #181).
    assert!(
        expand(
            "Residual Encoder-Decoder Convolutional Neural Network (RED-CNN)",
            "Residual Encoder-Decoder Convolutional Neural Network"
        )
        .is_some()
    );
}
