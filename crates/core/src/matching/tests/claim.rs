//! Reading a claim: an identifier or a reference, identifiers in running
//! text, its place, its probable title and search terms.

use super::*;

#[test]
fn input_is_an_identifier_or_a_reference() {
    let id_only = Claim::from_input(" 10.1038/nature14539 ");
    assert!(id_only.text.is_none() && id_only.ids.len() == 1);
    let reference = Claim::from_input("Deep learning. Nature 2015. doi:10.1038/nature14539");
    assert!(reference.text.is_some() && reference.ids.len() == 1);
    // A link a copy spaced out is still the whole input.
    let spaced = Claim::from_input("https:// doi. org/ 10. 1183/ 23120 541. 00489- 2021");
    assert!(spaced.text.is_none(), "{spaced:?}");
    assert_eq!(spaced.ids[0].to_string(), "doi:10.1183/23120541.00489-2021");
}

#[test]
fn search_terms_drop_noise() {
    assert_eq!(
        search_terms("LeCun Y, Bengio Y, et al. Deep learning. Nature. 2015;521:436-444."),
        "lecun bengio deep learning nature"
    );
}

#[test]
fn the_probable_title_is_read_from_a_references_shape() {
    // Red team (#102): no matcher hit, so no title was searched.
    for (text, title) in [
        (
            "Radford A, Kim JW, Hallacy C, et al. Learning transferable visual models from \
             natural language supervision. In: Proceedings of the 38th International Conference \
             on Machine Learning (ICML). PMLR; 2021:8748-8763.",
            "Learning transferable visual models from natural language supervision",
        ),
        (
            "Dosovitskiy, A. et al. (2021) 'An image is worth 16x16 words: Transformers for image \
             recognition at scale', International Conference on Learning Representations (ICLR).",
            "An image is worth 16x16 words: Transformers for image recognition at scale",
        ),
        (
            "Lundberg SM, Lee SI. A unified approach to interpreting model predictions. Adv Neural \
             Inf Process Syst. 2017;30.",
            "A unified approach to interpreting model predictions",
        ),
        (
            "Said EW. Orientalism. New York: Pantheon Books; 1978.",
            "Orientalism",
        ),
        (
            "Pillow, K. (2003) 'Did Goodman's Distinction Survive LeWitt?', Journal of Aesthetics.",
            "Did Goodman's Distinction Survive LeWitt?",
        ),
        (
            "E. Dijkstra, \"A note on two problems in connexion with graphs,\" Numerische \
             Mathematik, vol. 1, 1959.",
            "A note on two problems in connexion with graphs",
        ),
        (
            "@article{d, author = {Doe, J}, title = {Sepsis in {mice}}, year = {2020}}",
            "Sepsis in mice",
        ),
    ] {
        assert_eq!(probable_title(text).as_deref(), Some(title), "{text}");
    }
    assert_eq!(probable_title("2020. 12"), None);
}

#[test]
fn finds_ids_in_running_text() {
    let text = "See (https://doi.org/10.1145/3368089.3409741), arXiv:1706.03762v5 and \
                https://arxiv.org/abs/hep-th/9901001; also arXiv:1706.03762.";
    let ids: Vec<String> = find_ids(text).iter().map(ToString::to_string).collect();
    assert_eq!(
        ids,
        [
            "doi:10.1145/3368089.3409741",
            "arxiv:1706.03762v5",
            "arxiv:hep-th/9901001"
        ]
    );
    let book = "Goodfellow I, et al. Deep Learning. MIT Press; 2016. ISBN 978-0-262-03561-3. Also ISBN-10: 0262035618.";
    let ids: Vec<String> = find_ids(book).iter().map(ToString::to_string).collect();
    assert_eq!(ids, ["isbn:9780262035613"], "both forms are one book");
    assert!(
        find_ids("pages 9780262035613 and 2016").is_empty(),
        "unlabelled digits ignored"
    );
    assert!(find_ids("Published in 2017, pages 10-20.").is_empty());
}

#[test]
fn a_claims_place_takes_its_fields_then_its_text() {
    let claim = Claim {
        volume: Some("47".into()),
        text: Some("Injury 47 (2016) 797–804".into()),
        ..Claim::default()
    };
    let place = claim.place().expect("placed");
    assert_eq!(
        (place.volume.as_str(), place.first_page.as_str()),
        ("47", "797")
    );
    let read = claim.as_read();
    assert_eq!(read.page.as_deref(), Some("797"));
}
