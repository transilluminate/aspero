//! Red-team wild-6: a model's list of papers, each a label beside its
//! arXiv identifier ("Gated SAEs: 2404.16014", "Lookahead Decoding |
//! 2402.02057"), no authors.

use super::*;

fn plausible(a: &Assessment) -> bool {
    (PLAUSIBLE..VERIFIED).contains(&a.confidence)
}

/// wild-6 #319, #328: a method's name, words of the title, by the id.
#[test]
fn a_methods_name_beside_its_arxiv_id_is_plausible() {
    let r = arxiv_preprint(
        "arXiv:2403.10444",
        "Block Verification Accelerates Speculative Decoding",
        &["Ziteng Sun", "Uri Mendlovic", "Yaniv Leviathan"],
        2024,
    );
    let a = assess(&Claim::from_text("Block Verification | 2403.10444"), &r);
    assert_eq!(a.ids, Agreement::Match);
    assert!(plausible(&a), "{a:?}");
}

/// wild-6 #545, #590: an acronym in the label, for the title's words
/// ("SAEs" for "Sparse Autoencoders", "BAO" for "baryon acoustic
/// oscillations"), and a year.
#[test]
fn a_label_with_an_acronym_of_the_titles_words_is_plausible() {
    let gated = arxiv_preprint(
        "arXiv:2404.16014",
        "Improving Dictionary Learning with Gated Sparse Autoencoders",
        &["Senthooran Rajamanoharan", "Arthur Conmy"],
        2024,
    );
    let a = assess(&Claim::from_text("Gated SAEs: 2404.16014"), &gated);
    assert!(plausible(&a), "{a:?}");
    let desi = arxiv_preprint(
        "arXiv:2503.14738",
        "DESI DR2 results. II. Measurements of baryon acoustic oscillations and cosmological \
         constraints",
        &["M. Abdul Karim", "J. Aguilar"],
        2025,
    );
    let a = assess(
        &Claim::from_text("DESI DR2 BAO (arXiv:2503.14738, 2025)"),
        &desi,
    );
    assert!(plausible(&a), "{a:?}");
}

/// wild-6 #589: another paper's name ("DESI 2024 III", "DR1") beside this
/// one's id stays a mismatch: its words aren't this title's.
#[test]
fn another_works_label_beside_an_id_is_not_lifted() {
    let r = arxiv_preprint(
        "arXiv:2404.03002",
        "DESI 2024 VI: cosmological constraints from the measurements of baryon acoustic \
         oscillations",
        &["A. G. Adame", "J. Aguilar"],
        2025,
    );
    let a = assess(
        &Claim::from_text("DESI 2024 III (arXiv:2404.03002, DR1 BAO)"),
        &r,
    );
    assert!(a.confidence < PLAUSIBLE, "{a:?}");
    // Nor another paper's title, its words beside some of this one's.
    let r = arxiv_preprint(
        "arXiv:2403.10444",
        "Block Verification Accelerates Speculative Decoding",
        &["Ziteng Sun", "Uri Mendlovic"],
        2024,
    );
    let a = assess(
        &Claim::from_text(
            "Blockwise Parallel Decoding for Deep Autoregressive Models | 2403.10444",
        ),
        &r,
    );
    assert!(a.confidence < PLAUSIBLE, "{a:?}");
}

/// The label floor takes no one else's byline: other people beside the
/// label and the id are another paper's citation.
#[test]
fn a_label_beside_other_authors_is_not_lifted() {
    let r = arxiv_preprint(
        "arXiv:2403.10444",
        "Block Verification Accelerates Speculative Decoding",
        &["Ziteng Sun", "Uri Mendlovic"],
        2024,
    );
    let a = assess(
        &Claim::from_text("Harrowgate O, Thalubrecht HL. Block Verification. arXiv:2403.10444"),
        &r,
    );
    assert!(a.confidence < PLAUSIBLE, "{a:?}");
}

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
    let expand = super::super::acronyms::expand_acronyms;
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
