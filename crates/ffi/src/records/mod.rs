//! What calls return, as UniFFI records and enums: mirrors of the engine's
//! types, converted at the boundary so the engine stays free of FFI.
//!
//! Records carry what an app shows; `Work::json` has everything else, in
//! the JSON contract's shape. Identifiers are strings with their prefix
//! (`doi:10.1038/nature14539`), dates ISO 8601 as precise as known.

mod bib;
mod budget;
mod cite;
mod errors;
mod graph;
mod lookup;
mod page;
mod search;
mod settings;
mod text;
mod verify;
mod work;

pub use bib::*;
pub use budget::*;
pub use cite::*;
pub use errors::*;
pub use graph::*;
pub use lookup::*;
pub use page::*;
pub use search::*;
pub use settings::*;
pub use text::*;
pub use verify::*;
pub use work::*;

use std::time::SystemTime;

use jiff::Timestamp;

fn time(t: Timestamp) -> SystemTime {
    SystemTime::from(t)
}

/// A count or index as the app's `u32`, saturating (no list is that long).
pub(crate) fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use aspero_core::{
        BooleanSupport, Claim, FieldSet, Filter, FilterValue, ProviderId, Raw, Relation, Sort,
        SourceRecord, TermField, Work as CoreWork,
    };
    use aspero_engine::{self as engine, Candidate as CoreCandidate, Served};

    use super::*;

    /// A value's variant in snake case, from its `Debug`: `PartlyMatches {
    /// differs: [] }` is `partly_matches`.
    fn name(value: impl std::fmt::Debug) -> String {
        let debug = format!("{value:?}");
        let mut name = String::new();
        for c in debug.chars().take_while(char::is_ascii_alphanumeric) {
            if c.is_ascii_uppercase() && !name.is_empty() {
                name.push('_');
            }
            name.push(c.to_ascii_lowercase());
        }
        name
    }

    /// Engine values reach the app as the case of the same name, not a
    /// catch-all's: each `ALL` lists every value, so a new one is checked
    /// here as soon as it exists.
    #[test]
    fn values_keep_their_names() {
        for v in engine::Verdict::ALL {
            assert_eq!(name(Verdict::from(*v)), v.as_str());
        }
        for f in engine::CitedField::ALL {
            assert_eq!(name(CitedField::from(*f)), f.as_str());
        }
        for d in engine::Decision::ALL {
            assert_eq!(name(IdentifiedBy::from(*d)), d.as_str());
        }
        for s in engine::Served::ALL {
            assert_eq!(name(Freshness::from(*s)), s.as_str());
        }
        for s in engine::KeyStatus::ALL {
            assert_eq!(name(KeyStatus::from(*s)), s.as_str());
        }
        for k in aspero_core::matching::WorkKind::ALL {
            assert_eq!(name(WorkKind::from(*k)), k.as_str().replace(' ', "_"));
        }
        for s in Sort::ALL {
            let sort = SearchSort::from_core(*s).unwrap();
            assert_eq!(name(sort), s.as_str());
            assert_eq!(Sort::from(sort), *s);
        }
        // Each filter is a field or a filter, but for `place`, kept
        // internal.
        for f in Filter::ALL {
            let field = SearchField::from_core(*f).map(name);
            let filter = SearchFilter::from_core(*f).map(name);
            match (field, filter) {
                (Some(n), None) | (None, Some(n)) => assert_eq!(n, f.as_str()),
                (None, None) => assert_eq!(*f, Filter::Place),
                both => panic!("{f:?} maps twice: {both:?}"),
            }
        }
        // A request's option, not a tree's filter, but listed all the same.
        assert_eq!(
            SearchFilter::from_core(Filter::Since),
            Some(SearchFilter::Since)
        );
        // Query trees' fields, filters, relations and reaches.
        for f in TermField::ALL {
            let field = SearchField::from_core(f.filter()).unwrap();
            assert_eq!(TermField::from(field), *f);
        }
        for f in FilterValue::ALL {
            let value = SearchFilterValue::from_core(f).unwrap();
            assert_eq!(name(&value), f.filter().as_str());
            assert_eq!(FilterValue::from(value), *f);
            assert!(SearchFilter::from_core(f.filter()).is_some());
        }
        for r in Relation::ALL {
            let relation = SearchRelation::from_core(*r).unwrap();
            assert_eq!(name(relation), r.as_str());
            assert_eq!(Relation::from(relation), *r);
        }
        for b in BooleanSupport::ALL {
            assert_eq!(name(SearchBoolean::from_core(*b)), b.as_str());
        }
        // Citation links.
        for k in engine::AnalysisKind::ALL {
            let kind = AnalysisKind::from(*k);
            assert_eq!(name(kind), k.as_str());
            assert_eq!(kind.to_engine(20), *k);
        }
        for c in engine::Coverage::ALL {
            assert_eq!(name(Coverage::from(*c)), c.as_str());
        }
        for p in engine::SeedProblem::ALL {
            assert_eq!(name(SeedProblem::from(*p)), p.as_str());
        }
        for k in engine::IncompleteKind::ALL {
            assert_eq!(name(IncompleteKind::from(*k)), k.as_str());
        }
        for r in engine::OddReason::ALL {
            assert_eq!(name(OddReason::from(*r)), r.as_str());
        }
        for s in engine::Stage::ALL {
            assert_eq!(name(AnalysisStage::from(*s)), s.as_str());
        }
        // Failures.
        for k in aspero_core::ErrorKind::ALL {
            assert_eq!(name(ErrorKind::from(*k)), k.as_str());
        }

        // `IdElsewhere` cites a candidate.
        let record = SourceRecord::new(
            ProviderId::from_static("crossref"),
            Vec::new(),
            CoreWork::default(),
            FieldSet::ALL,
            Raw {
                media_type: "application/json".into(),
                body: "{}".into(),
            },
            Timestamp::UNIX_EPOCH,
        );
        let candidates = [CoreCandidate {
            assessment: aspero_core::matching::assess(&Claim::default(), &record),
            record,
            ids: Vec::new(),
            sightings: Vec::new(),
            updates: Vec::new(),
        }];
        for r in engine::Reason::ALL {
            assert_eq!(name(Reason::new(r, &candidates)), r.as_str());
        }
    }

    #[test]
    fn usage_carries_the_budget_and_prepaid_balance() {
        let mut u = aspero_core::Usage::default();
        u.cost = Some(0.001);
        u.limit = Some(1.0);
        u.prepaid_remaining = Some(4.5);
        let used = usage(&[(ProviderId::from_static("openalex"), u)]);
        assert_eq!(used[0].provider, "openalex");
        assert_eq!(used[0].limit_usd, Some(1.0));
        assert_eq!(used[0].prepaid_remaining_usd, Some(4.5));
        assert_eq!(used[0].remaining_usd, None);
    }

    #[test]
    fn a_budget_says_what_the_caps_leave_spendable() {
        let openalex = ProviderId::from_static("openalex");
        let engine = engine::Engine::builder()
            .daily_credit_cap(openalex.clone(), 0.25)
            .build();
        let mut b = engine::Budget::new(jiff::Timestamp::UNIX_EPOCH);
        let unknown = Budget::new(&openalex, &b).capped(&engine, &openalex, &b);
        assert_eq!(unknown.spendable_today_usd, None);
        assert_eq!(unknown.credit_cap_usd, Some(0.25));
        assert_eq!(unknown.credit_spent_today_here_usd, None);
        // The allowance gone: what's left of the day's credit cap, less
        // the credit spent here, whatever other tools spent on the key.
        b.limit = Some(1.0);
        b.remaining = Some(0.0);
        b.prepaid_remaining = Some(5.0);
        b.prepaid_spent_today = Some(3.0);
        b.credit_spent_today_here = Some(0.10);
        let capped = Budget::new(&openalex, &b).capped(&engine, &openalex, &b);
        assert_eq!(
            capped.spendable_today_usd,
            engine.spendable_today(&openalex, &b)
        );
        assert_eq!(capped.spendable_today_usd, Some(0.15));
        assert_eq!(capped.prepaid_spent_today_usd, Some(3.0));
        assert_eq!(capped.credit_spent_today_here_usd, Some(0.10));
    }

    #[test]
    fn a_budget_says_the_prices_the_provider_said() {
        let openalex = ProviderId::from_static("openalex");
        let mut b = engine::Budget::new(jiff::Timestamp::UNIX_EPOCH);
        let none = Budget::new(&openalex, &b);
        assert_eq!((none.search_price_usd, none.prices), (None, Vec::new()));
        let mut prices = aspero_core::Prices::default();
        prices.singleton = Some(0.0);
        prices.search = Some(0.001);
        b.prices = Some(prices);
        let said = Budget::new(&openalex, &b);
        assert_eq!(said.search_price_usd, Some(0.001));
        let price = |endpoint: &str, usd| BudgetPrice {
            endpoint: endpoint.into(),
            usd,
        };
        assert_eq!(
            said.prices,
            [price("singleton", 0.0), price("search", 0.001)]
        );
    }

    #[test]
    fn a_stop_says_its_stage_and_provider() {
        let stopped = engine::Incomplete::new(engine::IncompleteKind::ProviderFailed, 3, 8)
            .at(engine::Stage::Coupling)
            .from(ProviderId::from_static("openalex"));
        let i = Incomplete::from(&stopped);
        assert_eq!((i.done, i.total), (3, 8));
        assert_eq!(i.stage, Some(AnalysisStage::Coupling));
        assert_eq!(i.provider.as_deref(), Some("openalex"));
        let budget = engine::Incomplete::new(engine::IncompleteKind::Budget, 1, 2);
        let i = Incomplete::from(&budget);
        assert_eq!((i.stage, i.provider), (None, None));
    }

    #[test]
    fn searches_say_which_filters_were_ignored() {
        let report = engine::SearchReport {
            provider: ProviderId::from_static("crossref"),
            page: aspero_core::Page::default(),
            // `place` is internal: the app never asks for it by name.
            ignored_filters: vec![Filter::Years, Filter::Place, Filter::OpenAccess],
            usage: None,
            served: Served::Live,
        };
        let results = SearchResults::new(Ok(report));
        assert_eq!(
            results.ignored_filters,
            [SearchFilter::Years, SearchFilter::OpenAccess]
        );
        let failed = SearchResults::new(Err(engine::EngineError::NoCapableProvider("search")));
        assert!(failed.ignored_filters.is_empty());
    }

    #[test]
    fn a_reached_cap_carries_its_figures() {
        let resets = Timestamp::from_second(1_794_700_800).unwrap();
        let e = engine::EngineError::Provider {
            provider: ProviderId::from_static("openalex"),
            error: aspero_core::Error::BudgetCapped {
                cap: 0.5,
                used: 0.5012,
                resets_at: Some(resets),
                which: aspero_core::CapKind::Allowance,
            },
        };
        let failure = Failure::from(&e);
        assert_eq!(failure.kind, ErrorKind::BudgetCapped);
        assert_eq!(failure.provider.as_deref(), Some("openalex"));
        assert_eq!(
            failure.budget_cap,
            Some(BudgetCap {
                cap_usd: 0.5,
                spent_usd: 0.5012,
                resets_at: Some(time(resets)),
                cap_kind: Some(BudgetCapKind::Allowance),
            })
        );
        // So does a relation's work left unresolved by the cap.
        let unresolved = engine::EngineError::Unresolved {
            id: "10.1038/nature14539".into(),
            reason: "the lookup failed: …".into(),
            kind: aspero_core::ErrorKind::BudgetCapped,
            provider: Some(ProviderId::from_static("openalex")),
            error: Some(Box::new(aspero_core::Error::BudgetCapped {
                cap: 0.5,
                used: 0.5012,
                resets_at: Some(resets),
                which: aspero_core::CapKind::Credit,
            })),
        };
        let failure = Failure::from(&unresolved);
        assert_eq!(failure.provider.as_deref(), Some("openalex"));
        let cap = failure.budget_cap.unwrap();
        assert_eq!(
            (cap.cap_usd, cap.cap_kind),
            (0.5, Some(BudgetCapKind::Credit))
        );
        // Other failures carry none.
        let other = engine::EngineError::Provider {
            provider: ProviderId::from_static("openalex"),
            error: aspero_core::Error::Timeout,
        };
        assert_eq!(Failure::from(&other).budget_cap, None);
    }

    /// aspero-bib's enums have no `ALL`, so only the values known now are
    /// checked.
    #[test]
    fn bib_values_keep_their_names() {
        use aspero_bib::{Format, Problem, docx::Manager};
        for f in [Format::Bibtex, Format::Ris, Format::CslJson] {
            assert_eq!(name(BibFormat::from(f)), f.as_str());
        }
        let problems = [
            Problem::NotADoi,
            Problem::NotAnArxivId,
            Problem::NotAnIsbn,
            Problem::NotAPmid,
        ];
        for p in problems {
            assert_eq!(name(BibProblem::from(p)), p.as_str());
        }
        for m in [Manager::Zotero, Manager::Mendeley] {
            assert_eq!(name(CitationManager::from(m)), name(m));
        }
    }

    #[test]
    fn utf16_offsets_match_counting_from_the_start() {
        // "é" is one UTF-16 unit, "😀" and "𝔸" two, "👩‍⚕️" five.
        let text = "é 😀 doi:10.1/x\n👩‍⚕️ PMID 1 𝔸 end";
        let bytes: Vec<usize> = (0..=text.len())
            .filter(|&b| text.is_char_boundary(b))
            .collect();
        let from_start = |b: usize| text[..b].encode_utf16().count() as u64;
        let mut utf16 = Utf16Offsets::new(text);
        // In order, then out of order.
        for &b in bytes.iter().chain(bytes.iter().rev()) {
            assert_eq!(utf16.at(b), from_start(b), "at byte {b}");
        }
        let mut utf16 = Utf16Offsets::new(text);
        for pair in bytes.windows(2) {
            let span = pair[0]..pair[1];
            let expected = Utf16Range {
                start: from_start(span.start),
                end: from_start(span.end),
            };
            assert_eq!(utf16.range(&span), expected);
        }
    }
}
