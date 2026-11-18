//! Conferences and meetings as venues: a conference told by acronym or
//! name, a meeting's proceedings, and whether two proceedings are one meeting.

use crate::matching::normalise;

use super::journal::JOURNAL_STOPWORDS;

/// Conferences whose papers are cited by venue, each by its acronyms and
/// the words of its name that tell it apart (joining words and numbers
/// aside: "Proceedings of the 34th International Conference on Machine
/// Learning"). A proceedings' name is otherwise free ("2017 IEEE
/// Conference on Computer Vision and Pattern Recognition (CVPR)",
/// "Advances in Neural Information Processing Systems 30"), so these are
/// named rather than ruled.
const CONFERENCES: &[(&[&str], &[&str])] = &[
    (
        &["neurips", "nips"],
        &["neural", "information", "processing", "systems"],
    ),
    (
        &["icml"],
        &["international", "conference", "machine", "learning"],
    ),
    (&["iclr"], &["learning", "representations"]),
    (&["cvpr"], &["computer", "vision", "pattern", "recognition"]),
    (
        &["iccv"],
        &["international", "conference", "computer", "vision"],
    ),
    (&["eccv"], &["european", "conference", "computer", "vision"]),
    (
        &["wacv"],
        &["winter", "conference", "applications", "computer", "vision"],
    ),
    (&["bmvc"], &["british", "machine", "vision"]),
    (
        &["acl"],
        &[
            "annual",
            "meeting",
            "association",
            "computational",
            "linguistics",
        ],
    ),
    (
        &["emnlp"],
        &["empirical", "methods", "natural", "language", "processing"],
    ),
    (&["naacl"], &["north", "american", "chapter"]),
    (
        &["coling"],
        &[
            "international",
            "conference",
            "computational",
            "linguistics",
        ],
    ),
    (
        &["kdd", "sigkdd"],
        &["knowledge", "discovery", "data", "mining"],
    ),
    (
        &["aaai"],
        &["aaai", "conference", "artificial", "intelligence"],
    ),
    (
        &["ijcai"],
        &[
            "international",
            "joint",
            "conference",
            "artificial",
            "intelligence",
        ],
    ),
    (&["aistats"], &["artificial", "intelligence", "statistics"]),
    (&["uai"], &["uncertainty", "artificial", "intelligence"]),
    (&["colt"], &["conference", "learning", "theory"]),
    (&["chi"], &["human", "factors", "computing", "systems"]),
    (
        &["icra"],
        &["international", "conference", "robotics", "automation"],
    ),
    (&["iros"], &["intelligent", "robots", "systems"]),
    (&["corl"], &["conference", "robot", "learning"]),
    (
        &["isca"],
        &["international", "symposium", "computer", "architecture"],
    ),
    (
        &["micro"],
        &["international", "symposium", "microarchitecture"],
    ),
    (
        &["sigir"],
        &["research", "development", "information", "retrieval"],
    ),
    (
        &["sigmod"],
        &["international", "conference", "management", "data"],
    ),
    (&["vldb"], &["very", "large", "data", "bases"]),
    (
        &["icse"],
        &["international", "conference", "software", "engineering"],
    ),
    (
        &["osdi"],
        &["operating", "systems", "design", "implementation"],
    ),
    (
        &["sosp"],
        &["symposium", "operating", "systems", "principles"],
    ),
    (
        &["nsdi"],
        &["networked", "systems", "design", "implementation"],
    ),
    (
        &["pldi"],
        &["programming", "language", "design", "implementation"],
    ),
    (&["popl"], &["principles", "programming", "languages"]),
    (
        &["icassp"],
        &["acoustics", "speech", "signal", "processing"],
    ),
    (&["interspeech"], &["interspeech"]),
    (
        &["miccai"],
        &[
            "medical",
            "image",
            "computing",
            "computer",
            "assisted",
            "intervention",
        ],
    ),
    (&["wsdm"], &["web", "search", "data", "mining"]),
    (&["cikm"], &["information", "knowledge", "management"]),
    (&["recsys"], &["recommender", "systems"]),
];

/// Acronyms that are words or names too ("ACL" a ligament, "Chi" a
/// family name): read only in capitals.
const WORDLIKE_ACRONYMS: &[&str] = &["acl", "chi", "micro", "colt", "uai", "isca"];

/// The conferences (indices into [`CONFERENCES`]) a venue's name, or the
/// text that cites one, names: by acronym ("ICCV", "NeurIPS 2017",
/// "CVPR'17") or by its name's words in order.
pub(crate) fn conferences(name: &str) -> Vec<usize> {
    let tokens: Vec<&str> = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect();
    let acronym = |a: &str| {
        tokens.iter().enumerate().any(|(i, t)| {
            let t = t.trim_end_matches(|c: char| c.is_ascii_digit());
            if holds_meeting(&tokens, i) {
                return false;
            }
            if WORDLIKE_ACRONYMS.contains(&a) {
                t.eq_ignore_ascii_case(a) && t.chars().all(|c| c.is_ascii_uppercase())
            } else {
                t.eq_ignore_ascii_case(a)
            }
        })
    };
    let words: Vec<String> = normalise(name)
        .split(' ')
        .filter(|w| !w.is_empty() && !JOURNAL_STOPWORDS.contains(w))
        .filter(|w| !w.chars().any(|c| c.is_ascii_digit()))
        .map(str::to_owned)
        .collect();
    let named = |phrase: &[&str]| words.windows(phrase.len()).any(|run| run == phrase);
    CONFERENCES
        .iter()
        .enumerate()
        .filter(|(_, (acronyms, phrase))| acronyms.iter().any(|a| acronym(a)) || named(phrase))
        .map(|(i, _)| i)
        .collect()
}

/// Whether the acronym at `tokens[i]` names who holds a meeting rather
/// than the meeting: a society before its other conferences ("the
/// International AAAI Conference on Web and Social Media" is ICWSM, not
/// AAAI's conference), a conference before its workshops ("ICML Workshop
/// on …").
fn holds_meeting(tokens: &[&str], i: usize) -> bool {
    let meeting = |t: &&str| {
        [
            "conference",
            "symposium",
            "workshop",
            "workshops",
            "congress",
            "meeting",
            "colloquium",
        ]
        .iter()
        .any(|m| t.eq_ignore_ascii_case(m))
    };
    match tokens.get(i + 1) {
        Some(next) if meeting(next) => true,
        Some(next) if next.eq_ignore_ascii_case("international") => {
            tokens.get(i + 2).is_some_and(meeting)
        }
        _ => false,
    }
}

/// Whether a venue cited is a conference's or a workshop's proceedings:
/// "In Proceedings of …", "… Conference on …", "Advances in Neural …".
pub(crate) fn proceedings(venue: &str) -> bool {
    !conferences(venue).is_empty()
        || normalise(venue).split(' ').any(|w| {
            matches!(
                w,
                "proceedings" | "proc" | "conference" | "symposium" | "workshop" | "congress"
            )
        })
}

/// Words of a proceedings' name that say what kind of meeting, who holds
/// it and when, not what it is about: "Proceedings of the 16th Annual
/// ACM/IEEE International Conference on …", "Companion of the …", and the
/// furniture a reference puts after it ("pp.", "eds.").
const PROCEEDINGS_BOILERPLATE: &[&str] = &[
    "proceedings",
    "proceeding",
    "proc",
    "procs",
    "companion",
    "conference",
    "conferences",
    "conf",
    "symposium",
    "symposia",
    "symp",
    "workshop",
    "workshops",
    "wksp",
    "congress",
    "meeting",
    "colloquium",
    "summit",
    "forum",
    "annual",
    "annu",
    "international",
    "int",
    "intl",
    "world",
    "joint",
    "acm",
    "ieee",
    "cvf",
    "rsj",
    "ifip",
    "siam",
    "spie",
    "usenix",
    "edition",
    "ed",
    "eds",
    "editor",
    "editors",
    "vol",
    "volume",
    "pp",
    "page",
    "pages",
    "part",
    "extended",
    "abstracts",
    "adjunct",
    "papers",
    "poster",
    "posters",
    "demo",
    "demos",
    "track",
    "session",
    "series",
    "held",
    "th",
    "st",
    "nd",
    "rd",
    "a",
    "an",
    "at",
    "to",
    "with",
];

/// Ordinals spelt out, as editions are: "Tenth Annual …".
const ORDINALS: &[&str] = &[
    "first",
    "second",
    "third",
    "fourth",
    "fifth",
    "sixth",
    "seventh",
    "eighth",
    "ninth",
    "tenth",
    "eleventh",
    "twelfth",
    "thirteenth",
    "fourteenth",
    "fifteenth",
    "sixteenth",
    "seventeenth",
    "eighteenth",
    "nineteenth",
    "twentieth",
    "thirtieth",
    "fortieth",
    "fiftieth",
    "twenty",
    "thirty",
    "forty",
    "fifty",
    "sixty",
];

/// Capitalised tokens that name a publisher, place or numeral, not a
/// conference.
const NOT_ACRONYMS: &[&str] = &[
    "acm", "ieee", "cvf", "rsj", "ifip", "siam", "spie", "usenix", "usa", "ny", "uk", "lncs", "pp",
    "ii", "iii", "iv", "vi", "vii", "viii", "ix", "xi", "xii", "eds", "lnai", "lnbi", "ccis",
    "pmlr", "ceur", "lipics", "eptcs", "oasics", "jmlr", "aisc", "doi", "isbn", "issn",
];

/// A proceedings' name as words of its subject and acronyms: "Proceedings
/// of the Tenth Annual ACM/IEEE International Conference on Human-Robot
/// Interaction (HRI '15)" is about "human robot interaction", "HRI". A
/// special interest group (SIGCHI, SIGACCESS) holds a meeting; its name is
/// neither.
struct Subject {
    words: Vec<String>,
    acronyms: Vec<String>,
    /// The name's words, lower-cased, numbers aside: what an acronym of
    /// the name is made of ([`Subject::fits`]). Each says whether it is
    /// the name's furniture ("Proceedings", "of", "Annual"), which gives an
    /// acronym its initial only.
    letters: Vec<(Vec<char>, bool)>,
}

impl Subject {
    fn new(name: &str) -> Subject {
        let tokens: Vec<&str> = name
            .split(|c: char| !c.is_alphanumeric())
            .filter(|t| !t.is_empty())
            .collect();
        let letters: Vec<(Vec<char>, bool)> = tokens
            .iter()
            .filter(|t| t.chars().next().is_some_and(char::is_alphabetic))
            .map(|t| {
                let lower: String = t.chars().flat_map(char::to_lowercase).collect();
                let furniture = JOURNAL_STOPWORDS.contains(&lower.as_str())
                    || PROCEEDINGS_BOILERPLATE.contains(&lower.as_str())
                    || ORDINALS.contains(&lower.as_str());
                (lower.chars().collect(), furniture)
            })
            .collect();
        // A society before its other meetings ("International AAAI
        // Conference on Web and Social Media") names neither; a word
        // before one is the meeting's ("The Web Conference", "Designing
        // Interactive Systems Conference").
        let holders: Vec<String> = (0..tokens.len())
            .filter(|&i| {
                holds_meeting(&tokens, i)
                    && tokens[i].chars().filter(char::is_ascii_uppercase).count() >= 2
            })
            .map(|i| tokens[i].to_ascii_lowercase())
            .collect();
        let alphabetic: Vec<&&str> = tokens
            .iter()
            .filter(|t| t.chars().all(char::is_alphabetic) && t.chars().count() > 3)
            .collect();
        // A name in capitals throughout has words, not acronyms.
        let shouting = alphabetic.len() >= 3
            && alphabetic
                .iter()
                .filter(|t| t.chars().all(char::is_uppercase))
                .count()
                * 2
                > alphabetic.len();
        let mut acronyms: Vec<String> = Vec::new();
        if !shouting {
            for (i, t) in tokens.iter().enumerate() {
                let t = t.trim_end_matches(|c: char| c.is_ascii_digit());
                let upper = t.chars().filter(char::is_ascii_uppercase).count();
                let lower = t.to_ascii_lowercase();
                if upper >= 2
                    && !holds_meeting(&tokens, i)
                    && (2..=12).contains(&t.len())
                    && t.chars().all(|c| c.is_ascii_alphabetic())
                    && !NOT_ACRONYMS.contains(&lower.as_str())
                    && !lower.starts_with("sig")
                    && !acronyms.contains(&lower)
                {
                    acronyms.push(lower);
                }
            }
        }
        let words = normalise(name)
            .split(' ')
            .filter(|w| w.chars().count() > 1)
            .filter(|w| !w.chars().any(|c| c.is_ascii_digit()))
            .filter(|w| {
                !JOURNAL_STOPWORDS.contains(w)
                    && !PROCEEDINGS_BOILERPLATE.contains(w)
                    && !ORDINALS.contains(w)
                    && !w.starts_with("sig")
                    && !acronyms.iter().any(|a| a == w)
                    && !holders.iter().any(|h| h == w)
            })
            .map(str::to_owned)
            .fold(Vec::new(), |mut words, w| {
                if !words.contains(&w) {
                    words.push(w);
                }
                words
            });
        // A meeting named only by its special interest group ("ACM
        // SIGCOMM Conference (SIGCOMM)") goes by that name.
        if acronyms.is_empty() && words.is_empty() {
            for t in &tokens {
                let lower = t.to_ascii_lowercase();
                if lower.starts_with("sig")
                    && lower.len() > 3
                    && t.chars().filter(char::is_ascii_uppercase).count() >= 3
                    && !acronyms.contains(&lower)
                {
                    acronyms.push(lower);
                }
            }
        }
        Subject {
            words,
            acronyms,
            letters,
        }
    }

    /// Whether `acronym` could be this name's: its letters in order, each
    /// word giving its initial and, after it, any of its letters ("mwscas"
    /// of "Midwest Symposium on Circuits and Systems", "aies" of
    /// "AAAI/ACM Conference on AI, Ethics, and Society"; not "icecs" of
    /// "International Symposium on Circuits and Systems"). The name's
    /// furniture gives only its initial ("podc" isn't "Proceedings of the
    /// … Symposium on Theory of Computing"), and two letters or more come
    /// from two words or more ("mm" isn't "… Conference on Multimedia
    /// Retrieval").
    fn fits(&self, acronym: &str) -> bool {
        // From the acronym's `k`th letter, in word `word` after its letter
        // `at` (none begun: `None`), with `used` words begun (two count as
        // many); states that failed are remembered.
        type State = (usize, usize, Option<usize>, usize);
        fn from(
            acronym: &[char],
            words: &[(Vec<char>, bool)],
            (k, word, at, used): State,
            failed: &mut std::collections::HashSet<State>,
        ) -> bool {
            let Some(&c) = acronym.get(k) else {
                return used >= acronym.len().min(2);
            };
            if failed.contains(&(k, word, at, used)) {
                return false;
            }
            // On in the word begun, after the letter used last.
            let within = at.is_some_and(|at| {
                let (letters, furniture) = &words[word];
                !furniture
                    && (at + 1..letters.len()).any(|i| {
                        letters[i] == c
                            && from(acronym, words, (k + 1, word, Some(i), used), failed)
                    })
            });
            let found = within
                || (word + usize::from(at.is_some())..words.len()).any(|w| {
                    words[w].0.first() == Some(&c)
                        && from(
                            acronym,
                            words,
                            (k + 1, w, Some(0), (used + 1).min(2)),
                            failed,
                        )
                });
            if !found {
                failed.insert((k, word, at, used));
            }
            found
        }
        let acronym: Vec<char> = acronym.chars().collect();
        from(
            &acronym,
            &self.letters,
            (0, 0, None, 0),
            &mut std::collections::HashSet::new(),
        )
    }
}

/// One subject word for another, or for its abbreviation or plural:
/// "Robot." for "Robotics", "Computers" for "Computer".
fn same_subject_word(a: &str, b: &str) -> bool {
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    short == long || (short.len() >= 3 && long.starts_with(short))
}

/// Whether a proceedings cited is the record's, by what each is about:
/// `Some(true)` when they share an acronym or most of the record's
/// subject, `Some(false)` when they're clearly about different things
/// ("… Workshop on User Interface Test Automation" for "… World Conference
/// on Mobile and Contextual Learning", or "`RecSys '26`" alone for a name
/// whose own acronym is "IUI"), `None` when it can't be told (an acronym
/// against a name without one: "ASSETS" for "… Conference on Computers and
/// Accessibility"). `cited` may run on into what a reference puts after
/// the venue (pages, a publisher): words there only make a mismatch less
/// likely.
pub(crate) fn proceedings_agree(cited: &str, actual: &str) -> Option<bool> {
    let known = conferences(actual);
    let (cited, actual) = (Subject::new(cited), Subject::new(actual));
    let same_acronym =
        |a: &String, b: &String| a == b || a.ends_with(b.as_str()) || b.ends_with(a.as_str());
    if cited
        .acronyms
        .iter()
        .any(|c| actual.acronyms.iter().any(|a| same_acronym(c, a)))
    {
        return Some(true);
    }
    // Named by an acronym alone that a known conference's name can't make
    // and isn't its: another meeting of the same field and society
    // ("(WWW '18)" for "… Conference on Information and Knowledge
    // Management"). Other names needn't make theirs ("ASSETS" of "…
    // Conference on Computers & Accessibility"). Two acronyms that differ
    // are two meetings ("(CoNEXT '13)" for "ACM SIGCOMM Conference
    // (SIGCOMM)").
    let fits = |c: &String| actual.fits(c);
    if cited.words.is_empty() {
        let its_own = |c: &String| {
            known.iter().any(|&k| {
                CONFERENCES[k]
                    .0
                    .iter()
                    .any(|a| c.trim_start_matches("sig") == *a)
            })
        };
        let foreign = !cited.acronyms.is_empty()
            && (!actual.acronyms.is_empty()
                || (!known.is_empty() && !cited.acronyms.iter().any(|c| fits(c) || its_own(c))));
        return foreign.then_some(false);
    }
    if actual.words.is_empty() {
        return (!cited.acronyms.is_empty() && !actual.acronyms.is_empty()).then_some(false);
    }
    let shared = actual
        .words
        .iter()
        .filter(|a| cited.words.iter().any(|c| same_subject_word(c, a)))
        .count();
    // So too with its words shared ("… Conference on Electronics,
    // Circuits and Systems (ICECS)" for the "… Midwest Symposium on
    // Circuits and Systems", "(ICMR '25)" for "… Conference on Multimedia
    // in Asia").
    let foreign = !cited.acronyms.is_empty() && !cited.acronyms.iter().any(fits);
    if shared * 2 >= actual.words.len() {
        Some(!foreign)
    } else if shared * 3 <= actual.words.len() {
        Some(false)
    } else {
        None
    }
}

/// Whether a venue's name is a meeting's, as a record's container: "…
/// Conference on …", "Companion of …", and with `proceedings` "Proceedings
/// of …" too (journals are called that: the Royal Society's).
pub(crate) fn meeting_name(venue: &str, proceedings: bool) -> bool {
    normalise(venue).split(' ').any(|w| {
        matches!(
            w,
            "conference"
                | "symposium"
                | "workshop"
                | "workshops"
                | "congress"
                | "companion"
                | "colloquium"
        ) || (proceedings && w == "proceedings")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conferences_are_told_by_acronym_or_name() {
        let named = |venue: &str| -> Vec<&str> {
            conferences(venue)
                .into_iter()
                .map(|i| CONFERENCES[i].0[0])
                .collect()
        };
        assert_eq!(
            named("2017 IEEE Conference on Computer Vision and Pattern Recognition (CVPR)"),
            ["cvpr"]
        );
        assert_eq!(
            named("In IEEE International Conference on Computer Vision (ICCV), 5967–5976."),
            ["iccv"]
        );
        assert_eq!(
            named("Advances in Neural Information Processing Systems 30"),
            ["neurips"]
        );
        assert_eq!(named("NeurIPS'17"), ["neurips"]);
        assert_eq!(
            named(
                "Proceedings of the 20th ACM SIGKDD international conference on Knowledge \
                 discovery and data mining"
            ),
            ["kdd"]
        );
        assert_eq!(
            named("Proceedings of the 2019 CHI Conference on Human Factors in Computing Systems"),
            ["chi"]
        );
        // Words that are acronyms too, not in capitals, name none.
        assert!(named("Chi H, Acl reconstruction outcomes. Am J Sports Med.").is_empty());
        assert!(named("Pattern Recognition").is_empty());
        assert!(proceedings("In: Proceedings of the 5th Workshop on Things"));
        assert!(!proceedings("Journal of Machine Learning Research"));
    }
}
