//! The site an article page is on, and the DOI prefixes its publisher
//! registers works under (#98).
//!
//! A page's site is a hint, never a claimed field: it isn't a journal (a
//! guessed container would make `verify` report a journal mismatch), and
//! a claim isn't wrong for being shared from another site. But it is
//! evidence of the publisher. When a claim from a page has no identifier,
//! `verify` and `resolve` search the publisher's own works for its title
//! (Crossref's `prefix:` filter) and, between candidates that match about
//! as well, prefer one whose DOI is the publisher's ([`PUBLISHERS`]).
//!
//! Sites match by registrable domain ([`registrable_domain`]): any
//! subdomain is its parent's (`pubs.acs.org` is `acs.org`,
//! `www.cell.com` is `cell.com`), and `notnature.com` is not `nature.com`.

use crate::id::Doi;

/// Publishers' sites, by registrable domain, and the DOI prefixes the
/// works they host are registered under. Each prefix's registrant checked
/// against Crossref's `/prefixes/<prefix>` (2026-11-15). Prefixes are a
/// publisher's, not a site's alone: Springer Nature hosts Palgrave's
/// `10.1057` on `nature.com`, Wiley's journals sit under `10.1002` and
/// Blackwell's `10.1111`. To add a site: its registrable domain, and the
/// prefixes in the DOIs of its article pages, most common first (only the
/// first [`MAX_PREFIXES`] are searched).
pub const PUBLISHERS: &[(&str, &[&str])] = &[
    ("jamanetwork.com", &["10.1001"]),
    ("oup.com", &["10.1093"]),
    ("nature.com", &["10.1038", "10.1057"]),
    ("sciencedirect.com", &["10.1016"]),
    ("cell.com", &["10.1016"]),
    ("thelancet.com", &["10.1016"]),
    ("elsevier.com", &["10.1016"]),
    ("wiley.com", &["10.1002", "10.1111"]),
    ("springer.com", &["10.1007"]),
    ("bmj.com", &["10.1136"]),
    ("nejm.org", &["10.1056"]),
    ("tandfonline.com", &["10.1080"]),
    ("sagepub.com", &["10.1177"]),
    ("plos.org", &["10.1371"]),
    ("frontiersin.org", &["10.3389"]),
    ("mdpi.com", &["10.3390"]),
    ("pnas.org", &["10.1073"]),
    ("science.org", &["10.1126"]),
    ("ahajournals.org", &["10.1161"]),
    ("acpjournals.org", &["10.7326"]),
    ("cambridge.org", &["10.1017"]),
    ("lww.com", &["10.1097"]),
    ("karger.com", &["10.1159"]),
    ("thieme-connect.com", &["10.1055"]),
    ("thieme-connect.de", &["10.1055"]),
    ("biomedcentral.com", &["10.1186"]),
    ("elifesciences.org", &["10.7554"]),
    ("atsjournals.org", &["10.1164"]),
    ("ascopubs.org", &["10.1200"]),
    ("jci.org", &["10.1172"]),
    ("asm.org", &["10.1128"]),
    ("acs.org", &["10.1021"]),
    ("rsc.org", &["10.1039"]),
    ("iop.org", &["10.1088"]),
    ("aps.org", &["10.1103"]),
    ("ieee.org", &["10.1109"]),
    ("acm.org", &["10.1145"]),
    ("annualreviews.org", &["10.1146"]),
    ("rupress.org", &["10.1083"]),
];

/// Prefixes of a site searched for a claim's title: one search each.
pub const MAX_PREFIXES: usize = 2;

/// Second-level labels under a country's code that are public suffixes
/// themselves (`co.uk`, `ac.uk`, `com.au`, `co.jp`, …): enough for the
/// registrable domain of a publisher's site without the Public Suffix
/// List.
const SECOND_LEVELS: &[&str] = &[
    "ac", "co", "com", "edu", "go", "gob", "gov", "ne", "net", "or", "org",
];

/// The site of an address (or of a host written alone): its host, lower
/// case, without a leading `www.`, port or user. `None` when there's no
/// host to speak of (no dot, or characters no host has).
pub fn site_of(address: &str) -> Option<String> {
    let s = address.trim();
    let rest = s.split_once("://").map_or(s, |(_, rest)| rest);
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = host.rsplit('@').next().unwrap_or(host);
    let host = host.split(':').next().unwrap_or(host);
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let labels_ok = host.split('.').all(|l| {
        !l.is_empty()
            && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && !l.starts_with('-')
            && !l.ends_with('-')
    });
    (host.contains('.') && labels_ok).then(|| host.to_owned())
}

/// The part of a host its owner registered: the last two labels, or three
/// under a country's second level (`SECOND_LEVELS`: `example.co.uk`).
/// `academic.oup.com` is `oup.com`; a host of one label is itself.
pub fn registrable_domain(host: &str) -> &str {
    let labels: Vec<&str> = host.rsplitn(4, '.').collect();
    let keep = match labels.as_slice() {
        [tld, second, _, ..] if tld.len() == 2 && SECOND_LEVELS.contains(second) => 3,
        _ => 2,
    };
    if labels.len() <= keep {
        return host;
    }
    // Byte offset of the first kept label.
    let tail: usize = labels[..keep].iter().map(|l| l.len()).sum::<usize>() + keep - 1;
    &host[host.len() - tail..]
}

/// The DOI prefixes a site's publisher registers under ([`PUBLISHERS`]),
/// matched by registrable domain; none for a site not listed.
pub fn doi_prefixes(site: &str) -> &'static [&'static str] {
    let Some(site) = site_of(site) else {
        return &[];
    };
    let domain = registrable_domain(&site);
    PUBLISHERS
        .iter()
        .find(|(d, _)| *d == domain)
        .map_or(&[], |(_, prefixes)| prefixes)
}

/// Whether a DOI is registered under one of `prefixes`.
pub fn has_prefix(doi: &Doi, prefixes: &[&str]) -> bool {
    prefixes.contains(&doi.prefix())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_site_is_its_host_without_www_port_or_case() {
        for (address, site) in [
            (
                "https://jamanetwork.com/journals/jama/article-abstract/2856706",
                Some("jamanetwork.com"),
            ),
            ("https://WWW.Nature.com/articles/x", Some("nature.com")),
            (
                "http://user@pubs.acs.org:443/doi/10.1021/x",
                Some("pubs.acs.org"),
            ),
            ("academic.oup.com", Some("academic.oup.com")),
            ("https://www.cell.com./x", Some("cell.com")),
            ("https://localhost/x", None),
            ("not a host", None),
            ("", None),
        ] {
            assert_eq!(site_of(address).as_deref(), site, "{address}");
        }
    }

    #[test]
    fn subdomains_are_their_registrable_domain() {
        for (host, domain) in [
            ("pubs.acs.org", "acs.org"),
            ("academic.oup.com", "oup.com"),
            ("onlinelibrary.wiley.com", "wiley.com"),
            ("a.b.c.thelancet.com", "thelancet.com"),
            ("nature.com", "nature.com"),
            ("journals.example.co.uk", "example.co.uk"),
            ("example.ac.uk", "example.ac.uk"),
            ("www.example.com.au", "example.com.au"),
            // `co` is a second level only under a country's code.
            ("x.co.com", "co.com"),
            ("localhost", "localhost"),
        ] {
            assert_eq!(registrable_domain(host), domain, "{host}");
        }
    }

    #[test]
    fn prefixes_match_by_registrable_domain_not_by_suffix() {
        assert_eq!(doi_prefixes("jamanetwork.com"), ["10.1001"]);
        assert_eq!(doi_prefixes("www.nature.com"), ["10.1038", "10.1057"]);
        assert_eq!(doi_prefixes("pubs.acs.org"), ["10.1021"]);
        assert_eq!(
            doi_prefixes("onlinelibrary.wiley.com"),
            ["10.1002", "10.1111"]
        );
        assert_eq!(
            doi_prefixes("https://academic.oup.com/brain/x"),
            ["10.1093"]
        );
        for near_miss in [
            "notnature.com",
            "nature.com.evil.org",
            "jamanetwork.co",
            "x.org",
        ] {
            assert!(doi_prefixes(near_miss).is_empty(), "{near_miss}");
        }
    }

    #[test]
    fn every_listed_site_and_prefix_is_well_formed() {
        for (domain, prefixes) in PUBLISHERS {
            assert_eq!(registrable_domain(domain), *domain, "{domain}");
            assert_eq!(site_of(domain).as_deref(), Some(*domain));
            assert!(!prefixes.is_empty(), "{domain}");
            for p in *prefixes {
                let doi: Doi = format!("{p}/x").parse().unwrap();
                assert!(has_prefix(&doi, prefixes), "{p}");
            }
        }
        let doi: Doi = "10.1001/jama.2026.19482".parse().unwrap();
        assert!(has_prefix(&doi, doi_prefixes("jamanetwork.com")));
        assert!(!has_prefix(&doi, doi_prefixes("nature.com")));
    }
}
