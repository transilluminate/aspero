//! Titles from a page's `<title>`, `og:title` or a dragged link's name:
//! the work's title without the site's furniture, or nothing when what's
//! left isn't a title.
//!
//! Browsers title a window or a dragged address with the page's
//! `<title>`, which publishers build as `Title | Journal | Publisher`,
//! `Title - Site`, `Title — Site`, `Site | Title` or `Site: Title`, and
//! some add their own name (` - Google Chrome`). A link dragged from
//! inside a page is named by its text instead: "Full Text", "PDF".
//!
//! Suffixes go, segment by segment from the end, while the segment after
//! a separator (` | `, ` - `, ` – `, ` — `, ` · `, ` :: `) is the site's:
//! - a known site or browser name ([`SITES`]), or `: <site>` for one of
//!   them (Cell Press's `Title: Cell`);
//! - the address's name: `JAMA Network` on `jamanetwork.com`, `The
//!   Lancet` on `thelancet.com`;
//! - a name the page gives its own site (`og:site_name`, JSON-LD's
//!   website or publisher), exactly but for case and punctuation (see
//!   [`title_named`]);
//! - `Proceedings of …` (ACM's conference pages);
//! - a year, or names ending in one (Wiley's and Sage's `- Smith - 2020
//!   -`, `- Smith, Jones, 2020`);
//! - or short and title-cased (at most five words and 40 characters,
//!   each word capitalised but small ones: `| Journal of Hepatology |`),
//!   when what's before it is three words or more.
//!
//! A colon inside a title is the title's: only a known site's name before
//! or after one goes. Prefixes go when they're a known site or the
//! address's name before ` | ` (`Frontiers | Title`), a known site before
//! `: `, or furniture: `(PDF) `, `[PDF] `, `Full article: ` (Taylor &
//! Francis), `… | Free Full-Text | ` (MDPI), arXiv's `[1706.03762] `.
//! Suffix ` on JSTOR` likewise.
//!
//! What's left is a title ([`is_title`]) unless it's link text or a
//! site's furniture ([`NOT_TITLES`], parenthesised sizes aside: "PDF (2
//! MB)"), a challenge or error page's title ("Just a moment..."), a known
//! site's name alone, an address or identifier, or too short: under
//! [`MIN_WORDS`] words or [`MIN_CHARS`] characters for a hint, under two
//! words for a page's own title.

/// Fewest words with letters in a dragged link's name for it to be a
/// title: "Deep learning" could be anything's link text.
pub(super) const MIN_WORDS: usize = 3;
/// Fewest characters in a dragged link's name for it to be a title.
pub(super) const MIN_CHARS: usize = 15;
/// Fewest words in a page's own `<title>` or `og:title`, which name the
/// page: a two-word title is still the work's.
pub(super) const MIN_PAGE_WORDS: usize = 2;
/// Longest title kept, in characters: more is a page's description.
const MAX_CHARS: usize = 600;

/// Sites and browsers whose names end (or start) window titles, lowercase.
const SITES: &[&str] = &[
    // Publishers and platforms.
    "jama",
    "jama network",
    "jama network open",
    "sciencedirect",
    "elsevier",
    "nature",
    "nature.com",
    "springer",
    "springerlink",
    "springer nature link",
    "biomed central",
    "bmc",
    "oxford academic",
    "oup academic",
    "wiley online library",
    "wiley",
    "pnas",
    "bmj",
    "the bmj",
    "bmj open",
    "the lancet",
    "lancet",
    "cell",
    "cell press",
    "cell reports",
    "neuron",
    "immunity",
    "molecular cell",
    "current biology",
    "iscience",
    "science",
    "science advances",
    "plos one",
    "plos",
    "plos medicine",
    "plos biology",
    "nejm",
    "new england journal of medicine",
    "the new england journal of medicine",
    "taylor & francis online",
    "taylor and francis online",
    "taylor & francis",
    "sage journals",
    "cambridge core",
    "ieee xplore",
    "acm digital library",
    "frontiers",
    "mdpi",
    "karger publishers",
    "thieme",
    "annals of internal medicine",
    "american journal of psychiatry",
    "biorxiv",
    "medrxiv",
    "arxiv",
    "arxiv.org",
    "ssrn",
    "researchgate",
    "semantic scholar",
    "google scholar",
    "pubmed",
    "pmc",
    "europe pmc",
    "ncbi",
    "jstor",
    "europepmc",
    "openreview",
    "wikipedia",
    // Browsers.
    "google chrome",
    "chrome",
    "mozilla firefox",
    "firefox",
    "safari",
    "microsoft edge",
    "edge",
    "brave",
    "opera",
    "vivaldi",
    "arc",
];

/// Sites that title pages `Site: Title`, lowercase: only names no title
/// starts with.
const COLON_PREFIXES: &[&str] = &[
    "sciencedirect",
    "springerlink",
    "jstor",
    "pubmed",
    "pmc",
    "europe pmc",
    "researchgate",
    "semantic scholar",
    "google scholar",
    "wiley online library",
    "taylor & francis online",
    "oxford academic",
    "jama network",
    "ieee xplore",
    "acm digital library",
    "biorxiv",
    "medrxiv",
    "arxiv",
    "arxiv.org",
    "ssrn",
];

/// Journals whose pages are titled `Title: Journal` (Cell Press),
/// lowercase.
const COLON_SUFFIXES: &[&str] = &[
    "cell",
    "cell reports",
    "cell metabolism",
    "cell host & microbe",
    "cell stem cell",
    "cell systems",
    "cancer cell",
    "developmental cell",
    "molecular cell",
    "neuron",
    "immunity",
    "current biology",
    "iscience",
    "joule",
    "chem",
    "structure",
    "trends in ecology & evolution",
];

/// Link text and page furniture that isn't a work's title, lowercase.
const NOT_TITLES: &[&str] = &[
    "full text",
    "full-text",
    "fulltext",
    "full text html",
    "html full text",
    "view full text",
    "full article",
    "article",
    "view article",
    "read article",
    "see article",
    "open article",
    "article pdf",
    "pdf",
    "view pdf",
    "download pdf",
    "pdf download",
    "get pdf",
    "epdf",
    "epub",
    "download",
    "download article",
    "abstract",
    "view abstract",
    "summary",
    "html",
    "link",
    "here",
    "click here",
    "read more",
    "more",
    "view",
    "open",
    "doi",
    "crossref",
    "pubmed",
    "google scholar",
    "scopus",
    "web of science",
    "references",
    "supplementary material",
    "supplementary materials",
    "supplementary data",
    "supplement",
    "figures",
    "tables",
    "get access",
    "access through your institution",
    "free access",
    "open access",
    "untitled",
    "home",
    "home page",
    "loading",
    "loading...",
];

/// The starts of challenge, error and sign-in pages' titles, lowercase.
const NOT_TITLE_STARTS: &[&str] = &[
    "just a moment",
    "attention required",
    "access denied",
    "are you a robot",
    "verify you are human",
    "security check",
    "page not found",
    "not found",
    "404",
    "403",
    "500",
    "error",
    "sign in",
    "log in",
    "login",
    "redirecting",
    "please wait",
    "cookie",
    "one moment",
];

/// Separators between a title and its site, longest first.
const SEPARATORS: &[&str] = &[" | ", " :: ", " - ", " – ", " — ", " · ", " • "];

/// Address labels too generic to name a site.
const GENERIC_LABELS: &[&str] = &[
    "www",
    "com",
    "org",
    "net",
    "edu",
    "gov",
    "int",
    "ac",
    "co",
    "uk",
    "de",
    "fr",
    "io",
    "journals",
    "journal",
    "academic",
    "onlinelibrary",
    "link",
    "pubs",
    "article",
    "articles",
    "online",
    "library",
    "web",
    "en",
    "m",
    "ncbi",
    "nlm",
    "nih",
];

/// `raw` (a page title or a link's name) without its site's furniture,
/// tidied, if it is a title of at least `min_words` words and
/// `min_chars` characters. `url` is the page's address.
pub(super) fn title_from(
    raw: &str,
    url: &str,
    min_words: usize,
    min_chars: usize,
) -> Option<String> {
    title_named(raw, url, &[], min_words, min_chars)
}

/// [`title_from`], where a segment is the site's too when it is one of
/// `names`, the names the page gives its own site (`og:site_name`,
/// JSON-LD's publisher or website), as written but for case and
/// punctuation.
pub(super) fn title_named(
    raw: &str,
    url: &str,
    names: &[String],
    min_words: usize,
    min_chars: usize,
) -> Option<String> {
    let tidy = super::tidy(raw)?;
    let own = SiteNames {
        labels: host_labels(url),
        names: names
            .iter()
            .map(|n| fold(n))
            .filter(|n| !n.is_empty())
            .collect(),
    };
    let stripped = strip(&tidy, &own);
    is_title(&stripped, min_words, min_chars).then_some(stripped)
}

/// The site's name as a page title writes it: its last segment after a
/// separator, or its first before ` | `, when that names the address's
/// host ("Wikipedia" of "Sepsis - Wikipedia" on `en.wikipedia.org`).
pub(super) fn site_in_title(raw: &str, url: &str) -> Option<String> {
    let tidy = super::tidy(raw)?;
    let labels = host_labels(url);
    let last = SEPARATORS
        .iter()
        .filter_map(|sep| tidy.rfind(sep).map(|at| &tidy[at + sep.len()..]))
        .min_by_key(|segment| segment.len());
    let first = tidy.split_once(" | ").map(|(site, _)| site);
    [last, first]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|s| names_host(s, &labels))
        .map(str::to_owned)
}

/// What names the page's own site: its host's labels and the names the
/// page gives it, folded ([`fold`]).
struct SiteNames {
    labels: Vec<String>,
    names: Vec<String>,
}

impl SiteNames {
    /// Whether `segment` names the site: the host ([`names_host`]), or one
    /// of the page's names for it exactly.
    fn named(&self, segment: &str) -> bool {
        names_host(segment, &self.labels) || self.names.contains(&fold(segment))
    }
}

/// `s`'s letters and digits, lowercased: "JAMA Network" is `jamanetwork`.
fn fold(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// `title` without site prefixes and suffixes.
fn strip(title: &str, own: &SiteNames) -> String {
    let mut t = title.trim();
    for _ in 0..8 {
        let before = t;
        t = strip_prefix(t, own).trim();
        t = strip_suffix(t, own).trim();
        if t == before {
            break;
        }
    }
    t.to_owned()
}

/// `title` without one site prefix, if it has one.
fn strip_prefix<'a>(title: &'a str, own: &SiteNames) -> &'a str {
    for furniture in [
        "(pdf) ",
        "[pdf] ",
        "[html] ",
        "pdf: ",
        "full article: ",
        "full text: ",
    ] {
        if let Some(rest) = strip_prefix_ci(title, furniture) {
            return rest;
        }
    }
    // MDPI: "Sensors | Free Full-Text | Title".
    if let Some(at) = find_ci(title, "free full-text | ") {
        return &title[at + "free full-text | ".len()..];
    }
    // arXiv: "[1706.03762] Title", "[hep-th/9901001v2] Title".
    if let Some(rest) = title.strip_prefix('[')
        && let Some((id, after)) = rest.split_once("] ")
        && !id.contains(' ')
        && id.bytes().any(|b| b.is_ascii_digit())
    {
        return after;
    }
    for sep in [" | ", " - ", " — ", " – "] {
        if let Some((site, rest)) = title.split_once(sep)
            && (known_site(site) || own.named(site))
            && words(rest) >= 1
        {
            return rest;
        }
    }
    if let Some((site, rest)) = title.split_once(": ")
        && COLON_PREFIXES.contains(&site.trim().to_lowercase().as_str())
    {
        return rest;
    }
    title
}

/// `title` without one site suffix, if it has one.
fn strip_suffix<'a>(title: &'a str, own: &SiteNames) -> &'a str {
    if let Some(rest) = strip_suffix_ci(title, " on jstor") {
        return rest;
    }
    // The page's own name for its site, whole, though it holds a
    // separator itself ("Mozilla Hacks – the Web developer blog").
    for sep in SEPARATORS {
        for (at, _) in title.match_indices(sep) {
            let (head, tail) = (&title[..at], &title[at + sep.len()..]);
            if !head.trim().is_empty() && own.names.contains(&fold(tail)) {
                return head;
            }
        }
    }
    // The last separator wins: "A – B | Site" ends with "Site".
    let last = SEPARATORS
        .iter()
        .filter_map(|sep| title.rfind(sep).map(|at| (at, sep.len())))
        .max_by_key(|&(at, _)| at);
    if let Some((at, len)) = last {
        let (head, segment) = (&title[..at], title[at + len..].trim());
        if site_segment(segment, head, own) {
            return head;
        }
    }
    // Cell Press: "Title: Cell Reports".
    if let Some((head, site)) = title.rsplit_once(": ")
        && COLON_SUFFIXES.contains(&site.trim().to_lowercase().as_str())
        && words(head) >= MIN_WORDS
    {
        return head;
    }
    title
}

/// Whether `segment`, after a separator ending `head`, is the site's.
fn site_segment(segment: &str, head: &str, own: &SiteNames) -> bool {
    if head.trim().is_empty() {
        return false;
    }
    if known_site(segment) || own.named(segment) || dated(segment) {
        return true;
    }
    // ACM's "| Proceedings of the 2023 CHI Conference on …".
    if strip_prefix_ci(segment, "proceedings of ").is_some() && words(head) >= MIN_WORDS {
        return true;
    }
    let short = segment.chars().count() <= 40 && words(segment) <= 5;
    short && title_cased(segment) && words(head) >= MIN_WORDS
}

/// A year, or names ending in one: "2020", "Smith - 2020", "Smith, Jones,
/// 2020", "Smith et al., 2020".
fn dated(segment: &str) -> bool {
    let last = segment
        .rsplit([',', ' '])
        .find(|w| !w.is_empty())
        .unwrap_or("");
    let year = last.len() == 4
        && last.bytes().all(|b| b.is_ascii_digit())
        && (b"12".contains(&last.as_bytes()[0]));
    year && segment.chars().count() <= 80
}

/// Every word with a letter starts with a capital, but the small ones.
fn title_cased(segment: &str) -> bool {
    const SMALL: &[&str] = &[
        "of", "and", "the", "for", "in", "on", "at", "to", "a", "an", "de", "la", "du", "der",
        "und", "et", "&",
    ];
    let mut any = false;
    for w in segment.split_whitespace() {
        let Some(first) = w.chars().find(|c| c.is_alphabetic()) else {
            continue;
        };
        any = true;
        if !first.is_uppercase() && !SMALL.contains(&w.to_lowercase().as_str()) {
            return false;
        }
    }
    any
}

/// A known site's or browser's name.
fn known_site(s: &str) -> bool {
    let lower = s.trim().to_lowercase();
    SITES.contains(&lower.as_str())
}

/// Whether `segment` names the address's host: its letters and digits,
/// lowercased, are a label of the host's, or start with or are contained
/// in one, a label of four characters at least ("JAMA Network" on
/// `jamanetwork.com`, "PLOS One" on `journals.plos.org`).
fn names_host(segment: &str, labels: &[String]) -> bool {
    let key = fold(segment);
    key.len() >= 3
        && words(segment) <= 5
        && labels.iter().any(|l| {
            *l == key
                || (l.len() >= 4 && (key.starts_with(l.as_str()) || l.starts_with(key.as_str())))
        })
}

/// The host's labels that could name a site: `jamanetwork` of
/// `https://jamanetwork.com/…`, `plos` of `journals.plos.org`.
fn host_labels(url: &str) -> Vec<String> {
    let rest = url.trim().split_once("://").map_or(url.trim(), |(_, r)| r);
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.rsplit('@').next().unwrap_or(host);
    let host = host.split(':').next().unwrap_or(host);
    host.split('.')
        .map(str::to_ascii_lowercase)
        .filter(|l| !l.is_empty() && !GENERIC_LABELS.contains(&l.as_str()))
        .collect()
}

/// Whether `s` (stripped) is a work's title: see the module notes.
pub(super) fn is_title(s: &str, min_words: usize, min_chars: usize) -> bool {
    let s = s.trim();
    if s.is_empty() || s.chars().count() > MAX_CHARS {
        return false;
    }
    let lower = s.to_lowercase();
    // "PDF (2 MB)", "Full Text [HTML]": the furniture without its note.
    let bare = without_brackets(&lower);
    let bare = bare
        .trim()
        .trim_end_matches([':', '.', '»', '›', '>'])
        .trim();
    if NOT_TITLES.contains(&bare) || known_site(bare) {
        return false;
    }
    if NOT_TITLE_STARTS.iter().any(|p| lower.starts_with(p)) {
        return false;
    }
    // An address or an identifier, not a title.
    let address = lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("www.")
        || (!s.contains(' ') && s.contains(['.', '/']));
    let identifier = crate::id::scan(s)
        .iter()
        .any(|m| m.span.len() * 2 >= s.len());
    !address && !identifier && words(s) >= min_words.max(1) && s.chars().count() >= min_chars
}

/// `s` without its `(…)` and `[…]` parts.
fn without_brackets(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0usize;
    for c in s.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Words with a letter in them.
fn words(s: &str) -> usize {
    s.split_whitespace()
        .filter(|w| w.chars().any(char::is_alphabetic))
        .count()
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &s[prefix.len()..])
}

fn strip_suffix_ci<'a>(s: &'a str, suffix: &str) -> Option<&'a str> {
    let at = s.len().checked_sub(suffix.len())?;
    let tail = s.get(at..)?;
    tail.eq_ignore_ascii_case(suffix).then(|| &s[..at])
}

/// Where `needle` (ASCII, lowercase) is in `s`, ignoring ASCII case.
fn find_ci(s: &str, needle: &str) -> Option<usize> {
    s.char_indices().map(|(i, _)| i).find(|&i| {
        s.get(i..i + needle.len())
            .is_some_and(|w| w.eq_ignore_ascii_case(needle))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hint(raw: &str, url: &str) -> Option<String> {
        title_from(raw, url, MIN_WORDS, MIN_CHARS)
    }

    #[test]
    fn publishers_window_titles_lose_their_site() {
        let cases = [
            (
                "Semaglutide and Kidney Outcomes in Type 2 Diabetes | JAMA | JAMA Network",
                "https://jamanetwork.com/journals/jama/fullarticle/2856706",
                "Semaglutide and Kidney Outcomes in Type 2 Diabetes",
            ),
            (
                "Effect of Vitamin D on Falls: A Randomized Trial | JAMA Internal Medicine | JAMA Network",
                "https://jamanetwork.com/journals/jamainternalmedicine/fullarticle/1",
                "Effect of Vitamin D on Falls: A Randomized Trial",
            ),
            (
                "Deep learning | Nature",
                "https://www.nature.com/articles/nature14539",
                "Deep learning",
            ),
            (
                "Global burden of disease: a systematic analysis - ScienceDirect",
                "https://www.sciencedirect.com/science/article/pii/S0140673620301835",
                "Global burden of disease: a systematic analysis",
            ),
            (
                "Sleep and memory consolidation | Brain | Oxford Academic",
                "https://academic.oup.com/brain/article/1/2/3/4",
                "Sleep and memory consolidation",
            ),
            (
                "Sleep and memory consolidation — Oxford Academic",
                "https://academic.oup.com/brain/article/1/2/3/4",
                "Sleep and memory consolidation",
            ),
            (
                "Machine learning for sepsis - Smith - 2020 - Critical Care Medicine - Wiley Online Library",
                "https://onlinelibrary.wiley.com/doi/10.1002/ccm.1",
                "Machine learning for sepsis",
            ),
            (
                "Machine learning for sepsis - John Smith, Jane Doe, 2020",
                "https://journals.sagepub.com/doi/10.1177/1",
                "Machine learning for sepsis",
            ),
            (
                "CRISPR screens in human cells | PNAS",
                "https://www.pnas.org/doi/10.1073/pnas.1",
                "CRISPR screens in human cells",
            ),
            (
                "Statins for primary prevention | The BMJ",
                "https://www.bmj.com/content/1/2",
                "Statins for primary prevention",
            ),
            (
                "Association between COVID-19 pandemic and the suicide rates in Nepal | PLOS One",
                "https://journals.plos.org/plosone/article?id=10.1371/journal.pone.0262958",
                "Association between COVID-19 pandemic and the suicide rates in Nepal",
            ),
            (
                "Tumour evolution in lung cancer - PubMed",
                "https://pubmed.ncbi.nlm.nih.gov/31452104/",
                "Tumour evolution in lung cancer",
            ),
            (
                "[1706.03762] Attention Is All You Need",
                "https://arxiv.org/abs/1706.03762",
                "Attention Is All You Need",
            ),
            (
                "Frontiers | Gut microbiota and depression in adults",
                "https://www.frontiersin.org/articles/10.3389/fpsyt.2020.1",
                "Gut microbiota and depression in adults",
            ),
            (
                "Sensors | Free Full-Text | Deep Learning for Gait Analysis",
                "https://www.mdpi.com/1424-8220/1/2/3",
                "Deep Learning for Gait Analysis",
            ),
            (
                "Full article: Nurse staffing and patient outcomes",
                "https://www.tandfonline.com/doi/full/10.1080/1",
                "Nurse staffing and patient outcomes",
            ),
            (
                "T cell exhaustion in chronic infection: Immunity",
                "https://www.cell.com/immunity/fulltext/S1074-7613(19)30001-1",
                "T cell exhaustion in chronic infection",
            ),
            (
                "Thrombolysis in stroke - The Lancet",
                "https://www.thelancet.com/journals/lancet/article/PIIS0140-6736(20)30183-5/fulltext",
                "Thrombolysis in stroke",
            ),
            (
                "Nurse staffing and patient outcomes on JSTOR",
                "https://www.jstor.org/stable/1",
                "Nurse staffing and patient outcomes",
            ),
            (
                "Wearable sensors for gait analysis | Journal of Medical Systems | Springer Nature Link",
                "https://link.springer.com/article/10.1007/s10916-020-1",
                "Wearable sensors for gait analysis",
            ),
            (
                "Fibrinolysis for intermediate-risk pulmonary embolism | New England Journal of Medicine",
                "https://www.nejm.org/doi/full/10.1056/NEJMoa1302097",
                "Fibrinolysis for intermediate-risk pulmonary embolism",
            ),
            (
                "Graph neural networks for traffic forecasting | IEEE Journals & Magazine | IEEE Xplore",
                "https://ieeexplore.ieee.org/document/1",
                "Graph neural networks for traffic forecasting",
            ),
            (
                "Designing for older adults' trust | Proceedings of the 2023 CHI Conference on Human Factors in Computing Systems",
                "https://dl.acm.org/doi/10.1145/3544548.1",
                "Designing for older adults' trust",
            ),
            (
                "Childhood adversity and adult depression | Psychological Medicine | Cambridge Core",
                "https://www.cambridge.org/core/journals/psychological-medicine/article/x",
                "Childhood adversity and adult depression",
            ),
            (
                "Single-cell atlas of the ageing mouse brain | bioRxiv",
                "https://www.biorxiv.org/content/10.1101/2020.01.01.1v1",
                "Single-cell atlas of the ageing mouse brain",
            ),
            (
                "Tumour evolution in lung cancer - PMC",
                "https://pmc.ncbi.nlm.nih.gov/articles/PMC6716493/",
                "Tumour evolution in lung cancer",
            ),
            (
                "Ambulance response times and survival | Emergency Medicine Journal",
                "https://emj.bmj.com/content/1/2",
                "Ambulance response times and survival",
            ),
            // Browsers' own names, as window titles carry them.
            (
                "Deep learning | Nature - Google Chrome",
                "https://www.nature.com/articles/nature14539",
                "Deep learning",
            ),
            (
                "Sleep and memory consolidation | Brain | Oxford Academic — Mozilla Firefox",
                "https://academic.oup.com/brain/article/1/2/3/4",
                "Sleep and memory consolidation",
            ),
            (
                "Thrombolysis in stroke - The Lancet - Microsoft Edge",
                "https://www.thelancet.com/journals/lancet/article/x",
                "Thrombolysis in stroke",
            ),
            // Safari names a page by its title alone.
            (
                "Semaglutide and Kidney Outcomes in Type 2 Diabetes",
                "https://jamanetwork.com/journals/jama/fullarticle/2856706",
                "Semaglutide and Kidney Outcomes in Type 2 Diabetes",
            ),
            // Some sites title pages "Site: Title".
            (
                "ScienceDirect: Global burden of disease in 204 countries",
                "https://www.sciencedirect.com/",
                "Global burden of disease in 204 countries",
            ),
            (
                "PubMed: Tumour evolution in lung cancer",
                "https://pubmed.ncbi.nlm.nih.gov/1/",
                "Tumour evolution in lung cancer",
            ),
            (
                "(PDF) Nurse staffing and patient outcomes | ResearchGate",
                "https://www.researchgate.net/publication/1",
                "Nurse staffing and patient outcomes",
            ),
        ];
        for (raw, url, want) in cases {
            assert_eq!(
                title_from(raw, url, MIN_PAGE_WORDS, 1).as_deref(),
                Some(want),
                "{raw:?}"
            );
            // Dragged from the address bar, the same title is the hint.
            if words(want) >= MIN_WORDS && want.chars().count() >= MIN_CHARS {
                assert_eq!(hint(raw, url).as_deref(), Some(want), "hint {raw:?}");
            }
        }
    }

    #[test]
    fn titles_keep_their_own_colons_and_dashes() {
        let cases = [
            (
                "COVID-19: a review of treatments in 2020",
                "COVID-19: a review of treatments in 2020",
            ),
            (
                "Long-term outcomes - a cohort study of adults | Example Journal",
                "Long-term outcomes - a cohort study of adults",
            ),
            (
                "Cell: the unit of life in biology",
                "Cell: the unit of life in biology",
            ),
            (
                "Nature: a history of the scientific journal",
                "Nature: a history of the scientific journal",
            ),
        ];
        for (raw, want) in cases {
            assert_eq!(
                hint(raw, "https://example.org/x").as_deref(),
                Some(want),
                "{raw:?}"
            );
        }
    }

    #[test]
    fn link_text_and_furniture_are_not_titles() {
        for junk in [
            "Full Text",
            "PDF",
            "Abstract",
            "Download PDF",
            "View Article",
            "Article",
            "Link",
            "here",
            "doi",
            "PDF (2.1 MB)",
            "Full Text [HTML]",
            "View PDF »",
            "https://jamanetwork.com/journals/jama/fullarticle/2856706",
            "jamanetwork.com/journals/jama/fullarticle/2856706",
            "10.1001/jama.2026.19482",
            "https://doi.org/10.1001/jama.2026.19482",
            "Just a moment...",
            "Access Denied",
            "Page not found | JAMA Network",
            "Article | JAMA Network",
            "PDF | ScienceDirect",
            "Download PDF (1.2 MB)",
            "Full Text (HTML)",
            "Just a moment... - Google Chrome",
            "Sign in to your account | Wiley Online Library",
            "Cookie settings and privacy",
            "JAMA Network",
            "Oxford Academic",
            "Deep learning",
            "Short title",
            "",
            "   ",
        ] {
            assert_eq!(hint(junk, "https://jamanetwork.com/x"), None, "{junk:?}");
        }
    }

    #[test]
    fn a_page_title_may_be_short() {
        assert_eq!(
            title_from(
                "Deep learning | Nature",
                "https://www.nature.com/",
                MIN_PAGE_WORDS,
                1
            )
            .as_deref(),
            Some("Deep learning")
        );
        assert_eq!(
            hint("Deep learning | Nature", "https://www.nature.com/"),
            None
        );
        assert_eq!(
            title_from("Nature", "https://www.nature.com/", MIN_PAGE_WORDS, 1),
            None
        );
    }
}
