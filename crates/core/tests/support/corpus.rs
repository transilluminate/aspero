//! Generated documents for testing and benchmarking `scan`: plausible
//! prose, with decoys that look like identifiers but aren't, and
//! identifiers in the forms people write them, each one known. Seeded, so
//! a corpus is the same on every run.

use std::ops::Range;

/// A generated document and what `scan` should find in it.
pub struct Corpus {
    pub text: String,
    /// Canonical id and byte span as written, per mention, in order.
    pub expected: Vec<(String, Range<usize>)>,
}

/// How often identifiers appear.
#[derive(Clone, Copy)]
pub enum Density {
    /// The body of a paper: an identifier every few kilobytes.
    Prose,
    /// A reference list: one or two per reference.
    References,
    /// Prose without any, only decoys.
    None,
}

/// A document of about `bytes` bytes.
pub fn generate(seed: u64, bytes: usize, density: Density) -> Corpus {
    let mut g = Gen {
        rng: Rng(seed),
        text: String::with_capacity(bytes + 256),
        expected: Vec::new(),
        column: 0,
    };
    let mut n = 1;
    while g.text.len() < bytes {
        match density {
            Density::Prose | Density::None => g.paragraph(density),
            Density::References => {
                g.reference(n);
                n += 1;
            }
        }
    }
    Corpus {
        text: g.text,
        expected: g.expected,
    }
}

struct Rng(u64);

impl Rng {
    /// SplitMix64.
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len() as u64) as usize]
    }
}

const WORDS: &[&str] = &[
    "the",
    "of",
    "and",
    "in",
    "to",
    "a",
    "with",
    "for",
    "was",
    "were",
    "that",
    "by",
    "patients",
    "study",
    "results",
    "analysis",
    "data",
    "model",
    "effect",
    "risk",
    "treatment",
    "group",
    "clinical",
    "outcome",
    "trial",
    "associated",
    "significant",
    "increased",
    "reduced",
    "compared",
    "between",
    "cohort",
    "randomised",
    "baseline",
    "follow-up",
    "mortality",
    "incidence",
    "prevalence",
    "survival",
    "hazard",
    "ratio",
    "confidence",
    "interval",
    "observed",
    "reported",
    "previous",
    "studies",
    "evidence",
    "however",
    "although",
    "these",
    "findings",
    "suggest",
    "mechanism",
    "protein",
    "expression",
    "cells",
    "tissue",
    "samples",
    "measured",
    "using",
    "method",
    "approach",
    "network",
    "training",
    "performance",
    "dataset",
    "learning",
    "neural",
    "attention",
    "transformer",
    "accuracy",
    "benchmark",
    "proposed",
    "framework",
    "algorithm",
    "estimate",
    "variance",
    "regression",
    "adjusted",
    "covariates",
    "sensitivity",
    "specificity",
    "primary",
    "secondary",
    "endpoint",
    "dose",
    "response",
    "café",
    "naïve",
    "über",
    "señor",
    "Zürich",
    "Øresund",
    "résumé",
    "approximately",
    "systematic",
    "review",
    "meta-analysis",
    "heterogeneity",
    "bias",
    "quality",
    "guideline",
    "recommendation",
    "population",
    "sample",
    "size",
    "power",
    "statistical",
    "inference",
];

const NAMES: &[&str] = &[
    "Smith",
    "Jones",
    "Wang",
    "Li",
    "García",
    "Müller",
    "Nguyen",
    "Kumar",
    "O'Brien",
    "Okafor",
    "van der Berg",
    "Dubois",
    "Rossi",
    "Kowalski",
    "Sato",
    "Andersson",
    "Silva",
    "Cohen",
];

const JOURNALS: &[&str] = &[
    "Lancet",
    "N Engl J Med",
    "JAMA",
    "BMJ",
    "Nature",
    "Science",
    "Cell",
    "PLoS One",
    "Ann Intern Med",
    "Proc Natl Acad Sci U S A",
    "J Clin Oncol",
    "Circulation",
];

/// Things that look a little like identifiers but aren't, so must not be
/// found. None starts with a dotted token (`2.3.1`): after a DOI ending a
/// line, that reads as the DOI broken mid-word, as PDFs break them.
const DECOYS: &[&str] = &[
    "2019",
    "(2020)",
    "3.14",
    "p < 0.05",
    "10.5 mg",
    "n = 1,204",
    "Fig. 3",
    "Table 2",
    "In 1706.03762 cases",
    "W1",
    "31452104 patients",
    "e-mail: a.b@example.org",
    "https://example.org/data/2020",
    "version 2.3.1",
    "COVID-19",
    "IL-6",
    "95% CI 1.2\u{2013}3.4",
    "10.1/abcd",
    "ISO 9001",
    "the doi of",
    "PMID pending",
    "\u{201C}quoted\u{201D}",
    "\u{FB01}nding",
    "section 1.2.3",
    "IP 10.10.10.10",
    "www.example.com/10.1234",
];

struct Gen {
    rng: Rng,
    text: String,
    expected: Vec<(String, Range<usize>)>,
    /// Characters on the current line, for wrapping.
    column: usize,
}

impl Gen {
    fn paragraph(&mut self, density: Density) {
        let sentences = 3 + self.rng.below(6);
        for _ in 0..sentences {
            self.sentence(!matches!(density, Density::None));
        }
        self.text.push_str("\n\n");
        self.column = 0;
    }

    fn sentence(&mut self, identifiers: bool) {
        let words = 8 + self.rng.below(18);
        for i in 0..words {
            if self.rng.chance(4) {
                let decoy = self.rng.pick(DECOYS);
                self.word(decoy);
            } else if self.rng.chance(1) && identifiers {
                self.identifier();
            } else {
                let w = self.rng.pick(WORDS);
                if i == 0 {
                    let mut c = w.chars();
                    let first = c.next().unwrap().to_uppercase().collect::<String>();
                    self.word(&format!("{first}{}", c.as_str()));
                } else {
                    self.word(w);
                }
            }
        }
        self.text.push('.');
    }

    fn reference(&mut self, n: usize) {
        let authors = 1 + self.rng.below(4);
        let mut start = format!("{n}. ");
        for i in 0..authors {
            if i > 0 {
                start.push_str(", ");
            }
            start.push_str(self.rng.pick(NAMES));
            start.push_str(" A");
        }
        self.text.push_str(&start);
        self.column = start.chars().count();
        self.text.push('.');
        let words = 6 + self.rng.below(10);
        for _ in 0..words {
            let w = self.rng.pick(WORDS);
            self.word(w);
        }
        self.text.push('.');
        let journal = self.rng.pick(JOURNALS);
        self.word(journal);
        let year = 1990 + self.rng.below(35);
        let volume = 1 + self.rng.below(400);
        let page = 1 + self.rng.below(2000);
        self.word(&format!("{year};{volume}:{page}-{}.", page + 9));
        self.identifier();
        if self.rng.chance(30) {
            self.identifier();
        }
        if self.rng.chance(15) {
            // A link broken after the DOI's prefix, as PDFs break them.
            let doi = self.doi();
            let (prefix, suffix) = doi.split_once('/').unwrap_or_default();
            self.text.push(' ');
            let start = self.text.len();
            self.text
                .push_str(&format!("https://doi.org/{prefix}/\n   {suffix}"));
            self.expected.push((
                format!("doi:{}", doi.to_lowercase()),
                start..self.text.len(),
            ));
            self.text.push('.');
        }
        self.text.push('\n');
        self.column = 0;
    }

    /// A word, after a space or at the start of a new line.
    fn word(&mut self, w: &str) {
        let len = w.chars().count();
        if self.column > 0 && self.column + 1 + len > 78 {
            self.text.push('\n');
            self.column = 0;
        } else if self.column > 0 {
            self.text.push(' ');
            self.column += 1;
        }
        self.text.push_str(w);
        self.column += len;
    }

    /// An identifier, in one of the forms people write: before it a space
    /// (never a line break, which could join it to the line before), after
    /// it a space or the end of a reference.
    fn identifier(&mut self) {
        let (id, written) = self.written_id();
        let (open, close) = match self.rng.below(8) {
            0 => ("(", ")"),
            1 => ("", ";"),
            2 => ("", ","),
            3 => ("[", "]."),
            4 if !written.contains(['(', ')']) => ("*", "*"),
            _ => ("", ""),
        };
        let len = open.len() + written.chars().count() + close.len();
        if self.column > 0 && self.column + 1 + len > 78 {
            self.text.push('\n');
            self.column = 0;
        }
        if self.column > 0 {
            self.text.push(' ');
        }
        self.text.push_str(open);
        let start = self.text.len();
        self.text.push_str(&written);
        self.expected.push((id, start..self.text.len()));
        self.text.push_str(close);
        // A trailing space, so the identifier never ends a line and the
        // next word never continues it.
        self.text.push(' ');
        self.column += len + 2;
    }

    /// Canonical id and as written.
    fn written_id(&mut self) -> (String, String) {
        let n = 1 + self.rng.below(40_000_000);
        match self.rng.below(12) {
            0..=3 => {
                let doi = self.doi();
                let canonical = format!("doi:{}", doi.to_lowercase());
                let written = match self.rng.below(6) {
                    0 => format!("doi:{doi}"),
                    1 => format!("DOI: {doi}"),
                    2 => format!("https://doi.org/{doi}"),
                    3 => format!("https://dx.doi.org/{doi}"),
                    4 if doi.contains('-') => doi.replacen('-', "\u{2013}", 1),
                    _ => doi,
                };
                (canonical, written)
            }
            4 => (format!("pmid:{n}"), format!("PMID: {n}")),
            5 => (
                format!("pmid:{n}"),
                format!("https://pubmed.ncbi.nlm.nih.gov/{n}/"),
            ),
            6 => (format!("pmcid:PMC{n}"), format!("PMC{n}")),
            7 => (format!("nbk:NBK{n}"), format!("NBK{n}")),
            8 => {
                let id = format!(
                    "{:02}{:02}.{:05}",
                    15 + self.rng.below(10),
                    1 + self.rng.below(12),
                    self.rng.below(100_000)
                );
                (format!("arxiv:{id}"), format!("arXiv:{id}"))
            }
            9 => {
                let id = format!(
                    "{:02}{:02}.{:05}",
                    15 + self.rng.below(10),
                    1 + self.rng.below(12),
                    self.rng.below(100_000)
                );
                (format!("arxiv:{id}"), format!("https://arxiv.org/abs/{id}"))
            }
            10 => {
                let isbn = isbn13(self.rng.below(1_000_000_000));
                let hyphenated = format!(
                    "{}-{}-{}-{}-{}",
                    &isbn[..3],
                    &isbn[3..4],
                    &isbn[4..7],
                    &isbn[7..12],
                    &isbn[12..]
                );
                (format!("isbn:{isbn}"), format!("ISBN {hyphenated}"))
            }
            _ => {
                let doi = self.doi();
                (
                    format!("doi:{}", doi.to_lowercase()),
                    format!("https://doi.org/{doi}"),
                )
            }
        }
    }

    fn doi(&mut self) -> String {
        let prefix = 1000 + self.rng.below(9000);
        let r = |g: &mut Self, n| g.rng.below(n);
        let suffix = match self.rng.below(5) {
            0 => format!(
                "s{}-0{}-{:04}-{}",
                41000 + r(self, 1000),
                r(self, 25),
                r(self, 10_000),
                r(self, 10)
            ),
            1 => format!("NEJMoa{}", 1_000_000 + r(self, 9_000_000)),
            2 => format!(
                "j.cell.{}.{:02}.{:03}",
                2000 + r(self, 25),
                1 + r(self, 12),
                r(self, 1000)
            ),
            3 => format!(
                "S0140-6736({:02}){:05}-{}",
                r(self, 25),
                r(self, 100_000),
                r(self, 10)
            ),
            _ => format!("jama.{}.{}", 2000 + r(self, 25), r(self, 100_000)),
        };
        format!("10.{prefix}/{suffix}")
    }
}

/// A checksum-valid ISBN-13 from nine digits.
fn isbn13(body: u64) -> String {
    let first12 = format!("978{body:09}");
    let sum: u32 = first12
        .bytes()
        .enumerate()
        .map(|(i, b)| u32::from(b - b'0') * if i % 2 == 0 { 1 } else { 3 })
        .sum();
    format!("{first12}{}", (10 - sum % 10) % 10)
}
