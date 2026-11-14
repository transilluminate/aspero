//! The third-party notices aspero's artefacts ship: the FFI library
//! (`aspero-ffi`, in the xcframework) and the CLI (`aspero`). For each,
//! the crates it's built from (the normal dependency closure, for
//! [`TARGET`], from `cargo tree`), their licences and copyright lines
//! (from `cargo metadata` and the crates' own files),
//! and the citation style data bundled with them.
//!
//! `just notices` writes each artefact's `notices/dependencies.json` and
//! `notices/notices.txt`; `just notices --check` (and this crate's tests)
//! fail when those files are out of date.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use aspero_bib::cite::{
    LICENSE as CSL_LICENSE, LOCALES_LINK as LOCALES_REPO, STYLES_LINK as STYLES_REPO,
};
use cargo_metadata::{Metadata, MetadataCommand, Package};
use serde::Serialize;

pub type Error = Box<dyn std::error::Error>;

/// The platform the artefacts are resolved for: the macOS app and the
/// released binaries' main target.
pub const TARGET: &str = "aarch64-apple-darwin";

/// What fixing stale notices takes.
pub const FIX: &str = "run `just notices`";

/// A shipped artefact.
#[derive(Debug, Clone, Copy)]
pub struct Artefact {
    /// The package it's built from.
    pub package: &'static str,
    /// What it is, for the notices' header.
    pub what: &'static str,
    /// Where its notices go, from the workspace root.
    pub dir: &'static str,
}

pub const ARTEFACTS: [Artefact; 2] = [
    Artefact {
        package: "aspero-ffi",
        what: "The aspero-ffi library (AsperoFFI.xcframework)",
        dir: "crates/ffi/notices",
    },
    Artefact {
        package: "aspero",
        what: "The aspero command-line tool",
        dir: "crates/cli/notices",
    },
];

/// The file of crates and data, as the FFI and CLI read it.
pub const DEPENDENCIES: &str = "dependencies.json";
/// The full notices.
pub const NOTICES: &str = "notices.txt";

/// A file to write, from the workspace root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub path: PathBuf,
    pub contents: String,
}

/// The workspace this crate is in.
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every artefact's notices files, generated afresh.
pub fn generate(root: &Path) -> Result<Vec<Output>, Error> {
    let metadata = MetadataCommand::new()
        .manifest_path(root.join("Cargo.toml"))
        .other_options(vec![
            "--filter-platform".to_owned(),
            TARGET.to_owned(),
            "--locked".to_owned(),
        ])
        .exec()?;
    let data = Data::read(root)?;
    let mut outputs = Vec::new();
    for artefact in ARTEFACTS {
        let crates = closure(&metadata, root, artefact.package)?
            .into_iter()
            .map(|p| Crate::read(p, &metadata))
            .collect::<Result<Vec<_>, _>>()?;
        let version = crates
            .iter()
            .find(|c| c.name == artefact.package)
            .map(|c| c.version.clone())
            .ok_or("the artefact's own package is missing")?;
        let data = crates
            .iter()
            .any(|c| c.name == "hayagriva")
            .then_some(&data);
        let dir = Path::new(artefact.dir);
        outputs.push(Output {
            path: dir.join(DEPENDENCIES),
            contents: dependencies_json(&artefact, &version, &crates, data)?,
        });
        outputs.push(Output {
            path: dir.join(NOTICES),
            contents: notices_text(&artefact, &version, &crates, data)?,
        });
    }
    Ok(outputs)
}

/// The files among `outputs` that differ from what's on disk.
pub fn stale(root: &Path, outputs: &[Output]) -> Vec<PathBuf> {
    outputs
        .iter()
        .filter(|o| std::fs::read_to_string(root.join(&o.path)).ok().as_ref() != Some(&o.contents))
        .map(|o| o.path.clone())
        .collect()
}

/// The packages `package` is built from, itself included: its normal
/// dependencies, theirs, and so on, sorted by name (ignoring case), then
/// version. Proc macros, build and dev dependencies don't count: they run
/// when building and aren't in what ships. (A proc macro's own
/// dependencies also resolve for the machine building, not the target,
/// so counting them would make the list depend on where it's generated.)
///
/// `cargo tree` says which: it resolves features for this package alone,
/// as `cargo build -p` does, where `cargo metadata` unifies them across
/// the workspace and its dev-dependencies (and so names crates the
/// artefact doesn't contain).
fn closure<'m>(
    metadata: &'m Metadata,
    root: &Path,
    package: &str,
) -> Result<Vec<&'m Package>, Error> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let out = Command::new(cargo)
        .args(["tree", "--quiet", "--locked", "--package", package])
        .args(["--edges", "normal,no-proc-macro", "--target", TARGET])
        .args(["--prefix", "none", "--format", "{p}"])
        .arg("--manifest-path")
        .arg(root.join("Cargo.toml"))
        .output()?;
    if !out.status.success() {
        return Err(format!(
            "cargo tree failed: {}",
            String::from_utf8_lossy(&out.stderr)
        )
        .into());
    }
    let mut ids = BTreeSet::new();
    for line in String::from_utf8(out.stdout)?.lines() {
        let mut words = line.split_whitespace();
        let (Some(name), Some(version)) = (words.next(), words.next()) else {
            continue;
        };
        let version = version.trim_start_matches('v');
        let mut matching = metadata
            .packages
            .iter()
            .filter(|p| p.name.as_str() == name && p.version.to_string() == version);
        match (matching.next(), matching.next()) {
            (Some(p), None) => ids.insert(&p.id),
            _ => return Err(format!("{name} {version}: not one package in cargo metadata").into()),
        };
    }
    let mut found: Vec<&Package> = metadata
        .packages
        .iter()
        .filter(|p| ids.contains(&p.id))
        .collect();
    found.sort_by_key(|p| (p.name.to_lowercase(), p.version.clone()));
    Ok(found)
}

/// A crate in an artefact, with its licence texts.
#[derive(Debug)]
struct Crate {
    name: String,
    version: String,
    license: String,
    repository: Option<String>,
    copyrights: Vec<String>,
    authors: Vec<String>,
    texts: Vec<Text>,
}

/// A licence or notice text, without its copyright lines.
#[derive(Debug)]
struct Text {
    /// The SPDX ids of the licences it holds (none for a notice).
    ids: Vec<&'static str>,
    /// Its file's name, or the template's.
    file: String,
    body: String,
}

impl Crate {
    fn read(package: &Package, metadata: &Metadata) -> Result<Self, Error> {
        let name = package.name.to_string();
        let license = match (&package.license, &package.license_file) {
            (Some(spdx), _) => spdx.trim().to_owned(),
            (None, Some(_)) => format!("LicenseRef-{name}"),
            (None, None) => return Err(format!("{name} states no licence").into()),
        };
        let dir = package
            .manifest_path
            .parent()
            .ok_or("a manifest without a directory")?;
        let mut files = licence_files(dir.as_std_path())?;
        if let Some(file) = package.license_file()
            && !files.contains(file.as_std_path())
        {
            files.insert(file.into_std_path_buf());
        }
        // Ours keep theirs at the workspace root.
        if files.is_empty() && metadata.workspace_members.contains(&package.id) {
            files = licence_files(metadata.workspace_root.as_std_path())?;
        }
        let mut copyrights = Vec::new();
        let mut texts = Vec::new();
        for file in &files {
            let raw = std::fs::read(file)?;
            let raw = String::from_utf8_lossy(&raw);
            let (body, found) = split_copyrights(&raw);
            for c in found {
                if !copyrights.contains(&c) {
                    copyrights.push(c);
                }
            }
            if body.trim().is_empty() {
                continue;
            }
            texts.push(Text {
                ids: classify(&body),
                file: file
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                body,
            });
        }
        let covered: BTreeSet<&str> = texts.iter().flat_map(|t| t.ids.iter().copied()).collect();
        let expr =
            Expr::parse(&license).map_err(|e| format!("{name}: licence {license:?}: {e}"))?;
        for id in expr.missing(&covered) {
            let body = template(&id).ok_or_else(|| {
                format!(
                    "{name} {}: no text for {id} in its files, and no template for it \
                     (add one to crates/notices-gen/licenses)",
                    package.version
                )
            })?;
            // A template's own copyright lines (Unicode's) are owed too.
            let (body, found) = split_copyrights(body);
            for c in found {
                if !copyrights.contains(&c) {
                    copyrights.push(c);
                }
            }
            texts.push(Text {
                ids: classify(&body),
                file: format!("{id} (canonical text)"),
                body,
            });
        }
        let authors = package
            .authors
            .iter()
            .map(|a| a.split('<').next().unwrap_or(a).trim().to_owned())
            .filter(|a| !a.is_empty())
            .collect();
        Ok(Crate {
            name,
            version: package.version.to_string(),
            license,
            repository: package.repository.clone(),
            copyrights,
            authors,
            texts,
        })
    }

    /// Its copyright lines, or failing those its authors.
    fn credit(&self) -> String {
        if !self.copyrights.is_empty() {
            self.copyrights.join("; ")
        } else if !self.authors.is_empty() {
            format!("authors: {}", self.authors.join(", "))
        } else {
            String::new()
        }
    }
}

/// The licence and notice files at the top of a package's directory.
fn licence_files(dir: &Path) -> Result<BTreeSet<PathBuf>, Error> {
    const PREFIXES: [&str; 6] = [
        "license",
        "licence",
        "copying",
        "notice",
        "unlicense",
        "copyright",
    ];
    let mut files = BTreeSet::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_lowercase();
        let source = [".rs", ".toml", ".json", ".yml", ".yaml", ".sh", ".py"];
        if entry.file_type()?.is_file()
            && PREFIXES.iter().any(|p| name.starts_with(p))
            && !source.iter().any(|s| name.ends_with(s))
        {
            files.insert(entry.path());
        }
    }
    Ok(files)
}

/// A text without its copyright lines (real ones, and templates'
/// placeholders), tidied; and the real ones, as found.
fn split_copyrights(raw: &str) -> (String, Vec<String>) {
    let raw = raw.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let mut found = Vec::new();
    let mut lines: Vec<&str> = Vec::new();
    for line in raw.lines() {
        if placeholder(line) {
            continue;
        }
        if let Some(c) = copyright(line) {
            if !found.contains(&c) {
                found.push(c);
            }
            continue;
        }
        let line = line.trim_end();
        if line.is_empty() && lines.last().is_none_or(|l| l.is_empty()) {
            continue;
        }
        lines.push(line);
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    (lines.join("\n"), found)
}

/// A template's copyright line, to be filled in.
fn placeholder(line: &str) -> bool {
    const MARKS: [&str; 9] = [
        "YEAR by AUTHOR",
        "[yyyy]",
        "{yyyy}",
        "<year>",
        "[year]",
        "<copyright holders>",
        "[name of copyright owner]",
        "<owner>",
        "[fullname]",
    ];
    line.contains("Copyright") && MARKS.iter().any(|m| line.contains(m))
}

/// `line` as a copyright line, if it is one: it starts "Copyright" (not
/// "Copyright and Related Rights" or "Copyright License") or "©", or has
/// "Copyright (c)" or "Copyright ©".
fn copyright(line: &str) -> Option<String> {
    let t = line
        .trim()
        .trim_start_matches(['#', '/', '*', '-', ';', ' '])
        .trim();
    let starts = if let Some(rest) = t.strip_prefix("Copyright") {
        let next = rest.trim_start();
        let mark = next.starts_with("(c)") || next.starts_with("(C)") || next.starts_with('©');
        let word = next.split_whitespace().next().unwrap_or("");
        let named = word.starts_with(|c: char| c.is_ascii_digit() || c.is_uppercase())
            && !["License", "Licence", "Notice", "Owner", "Holder", "Holders"].contains(&word);
        rest.starts_with([' ', '\t']) && (mark || named)
    } else {
        t.starts_with('©')
    };
    // Mid-line, only with the holder on this line too.
    let marked = ["Copyright (c)", "Copyright (C)", "Copyright ©"]
        .iter()
        .any(|m| {
            t.split_once(m)
                .is_some_and(|(_, rest)| !rest.trim().is_empty())
        });
    (starts || marked).then(|| t.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// The SPDX ids of the licences a text holds, by their wording: one, as
/// a rule, but some files gather several (aws-lc's).
fn classify(body: &str) -> Vec<&'static str> {
    // As words, without comment markers (`//`, `#`, ` *`).
    let t = body
        .to_lowercase()
        .lines()
        .flat_map(|l| {
            l.trim_start()
                .trim_start_matches(['/', '#', '*'])
                .split_whitespace()
        })
        .collect::<Vec<_>>()
        .join(" ");
    let has = |s: &str| t.contains(s);
    let mut ids = Vec::new();
    if has("apache license")
        && has("version 2.0")
        && has("terms and conditions for use, reproduction, and distribution")
    {
        ids.push("Apache-2.0");
    }
    if has("mozilla public license version 2.0") || has("mozilla public license, version 2.0") {
        ids.push("MPL-2.0");
    }
    let mit = "permission is hereby granted, free of charge, to any person obtaining a copy of \
               this software and associated documentation files";
    if has(mit) && has("the above copyright notice and this permission notice shall be included") {
        ids.push("MIT");
    }
    if has(mit) && has("is furnished to do so. the software is provided") {
        ids.push("MIT-0");
    }
    if has("this is free and unencumbered software released into the public domain") {
        ids.push("Unlicense");
    }
    if has("unicode license v3") {
        ids.push("Unicode-3.0");
    }
    if has("unicode, inc. license agreement - data files and software") {
        ids.push("Unicode-DFS-2016");
    }
    for isc in [
        "permission to use, copy, modify, and/or distribute this software for any purpose with or without fee is hereby granted",
        "permission to use, copy, modify, and distribute this software for any purpose with or without fee is hereby granted",
    ] {
        for (at, _) in t.match_indices(isc) {
            let id = match t[at + isc.len()..].starts_with(", provided that the above copyright") {
                true => "ISC",
                false => "0BSD",
            };
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    if has("redistribution and use in source and binary forms") {
        let three = has("neither the name") || has("endorse or promote products");
        ids.push(if three {
            "BSD-3-Clause"
        } else {
            "BSD-2-Clause"
        });
    }
    if has("altered source versions must be plainly marked") {
        ids.push("Zlib");
    }
    if has("boost software license - version 1.0") {
        ids.push("BSL-1.0");
    }
    if has("cc0 1.0 universal") {
        ids.push("CC0-1.0");
    }
    if has("openssl license") || has("developed by the openssl project") {
        ids.push("OpenSSL");
    }
    if has("written by eric young") {
        ids.push("SSLeay");
    }
    ids
}

/// The licence's name, for a heading.
fn title(id: &str) -> &str {
    match id {
        "Apache-2.0" => "Apache License 2.0",
        "MPL-2.0" => "Mozilla Public License 2.0",
        "MIT" => "MIT License",
        "MIT-0" => "MIT No Attribution",
        "Unlicense" => "The Unlicense",
        "Unicode-3.0" => "Unicode License v3",
        "Unicode-DFS-2016" => "Unicode License Agreement - Data Files and Software (2016)",
        "ISC" => "ISC License",
        "0BSD" => "BSD Zero Clause License",
        "BSD-2-Clause" => "BSD 2-Clause License",
        "BSD-3-Clause" => "BSD 3-Clause License",
        "Zlib" => "zlib License",
        "BSL-1.0" => "Boost Software License 1.0",
        "CC0-1.0" => "Creative Commons Zero v1.0 Universal",
        other => other,
    }
}

/// The canonical text of a licence, for crates that ship none.
fn template(id: &str) -> Option<&'static str> {
    Some(match id {
        "0BSD" => include_str!("../licenses/0BSD.txt"),
        "Apache-2.0" => include_str!("../licenses/Apache-2.0.txt"),
        "BSD-2-Clause" => include_str!("../licenses/BSD-2-Clause.txt"),
        "BSD-3-Clause" => include_str!("../licenses/BSD-3-Clause.txt"),
        "ISC" => include_str!("../licenses/ISC.txt"),
        "MIT" => include_str!("../licenses/MIT.txt"),
        "MPL-2.0" => include_str!("../licenses/MPL-2.0.txt"),
        "Unicode-3.0" => include_str!("../licenses/Unicode-3.0.txt"),
        "Zlib" => include_str!("../licenses/Zlib.txt"),
        _ => return None,
    })
}

/// An SPDX licence expression.
#[derive(Debug, PartialEq)]
enum Expr {
    Id(String),
    And(Vec<Expr>),
    Or(Vec<Expr>),
}

impl Expr {
    fn parse(s: &str) -> Result<Self, String> {
        let spaced = s
            .replace('(', " ( ")
            .replace(')', " ) ")
            .replace('/', " OR ");
        let tokens: Vec<&str> = spaced.split_whitespace().collect();
        let mut at = 0;
        let expr = Expr::or(&tokens, &mut at)?;
        match tokens.get(at) {
            None => Ok(expr),
            Some(t) => Err(format!("unexpected {t:?}")),
        }
    }

    fn or(tokens: &[&str], at: &mut usize) -> Result<Self, String> {
        let mut parts = vec![Expr::and(tokens, at)?];
        while tokens.get(*at) == Some(&"OR") {
            *at += 1;
            parts.push(Expr::and(tokens, at)?);
        }
        Ok(if parts.len() == 1 {
            parts.remove(0)
        } else {
            Expr::Or(parts)
        })
    }

    fn and(tokens: &[&str], at: &mut usize) -> Result<Self, String> {
        let mut parts = vec![Expr::atom(tokens, at)?];
        while tokens.get(*at) == Some(&"AND") {
            *at += 1;
            parts.push(Expr::atom(tokens, at)?);
        }
        Ok(if parts.len() == 1 {
            parts.remove(0)
        } else {
            Expr::And(parts)
        })
    }

    fn atom(tokens: &[&str], at: &mut usize) -> Result<Self, String> {
        let token = *tokens.get(*at).ok_or("ends early")?;
        *at += 1;
        if token == "(" {
            let inner = Expr::or(tokens, at)?;
            if tokens.get(*at) != Some(&")") {
                return Err("unclosed (".into());
            }
            *at += 1;
            return Ok(inner);
        }
        if ["AND", "OR", "WITH", ")"].contains(&token) {
            return Err(format!("unexpected {token:?}"));
        }
        // An exception adds permissions; the licence's text is what's owed.
        if tokens.get(*at) == Some(&"WITH") {
            *at += 2;
        }
        Ok(Expr::Id(token.trim_end_matches('+').to_owned()))
    }

    fn satisfied(&self, covered: &BTreeSet<&str>) -> bool {
        match self {
            Expr::Id(id) => covered.contains(id.as_str()),
            Expr::And(parts) => parts.iter().all(|p| p.satisfied(covered)),
            Expr::Or(parts) => parts.iter().any(|p| p.satisfied(covered)),
        }
    }

    /// The licences whose texts are owed beyond those `covered`: for a
    /// choice none of whose options is covered, the first option's.
    fn missing(&self, covered: &BTreeSet<&str>) -> Vec<String> {
        if self.satisfied(covered) {
            return Vec::new();
        }
        match self {
            Expr::Id(id) => vec![id.clone()],
            Expr::And(parts) => parts.iter().flat_map(|p| p.missing(covered)).collect(),
            Expr::Or(parts) => parts[0].missing(covered),
        }
    }
}

/// The CSL styles and locales `aspero-bib` bundles (`crates/bib/styles`).
#[derive(Debug)]
struct Data {
    styles: usize,
    locales: usize,
    styles_commit: String,
    locales_commit: String,
}

const CC_BY_SA_3: &str = "http://creativecommons.org/licenses/by-sa/3.0/";

impl Data {
    fn read(root: &Path) -> Result<Self, Error> {
        let dir = root.join("crates/bib/styles");
        let (mut styles, mut locales) = (0, 0);
        for entry in std::fs::read_dir(&dir)? {
            let path = entry?.path();
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            let style = name.ends_with(".csl");
            let locale = name.starts_with("locales-") && name.ends_with(".xml");
            if !style && !locale {
                continue;
            }
            let xml = std::fs::read_to_string(&path)?;
            if !xml.contains(&format!("<rights license=\"{CC_BY_SA_3}\">")) {
                return Err(format!("{name}: not marked CC BY-SA 3.0; check its <rights>").into());
            }
            if style {
                styles += 1;
            } else {
                locales += 1;
            }
        }
        if styles != aspero_bib::cite::BUNDLED.len() {
            return Err(format!(
                "{styles} .csl files in crates/bib/styles, but {} bundled styles",
                aspero_bib::cite::BUNDLED.len()
            )
            .into());
        }
        // As the FFI's citationStyles() and styleProject() credit them.
        for b in aspero_bib::cite::BUNDLED {
            if b.credits().license.as_deref() != Some(CSL_LICENSE) {
                return Err(format!("style {}: not {CSL_LICENSE}", b.id).into());
            }
        }
        if locales != aspero_bib::cite::locales().len() {
            return Err(format!("{locales} locale files, but others bundled").into());
        }
        let notice = std::fs::read_to_string(dir.join("NOTICE.md"))?;
        let commit = |repo: &str| -> Result<String, Error> {
            let line = notice
                .lines()
                .skip_while(|l| !l.contains(&format!("<{repo}>")))
                .take(2)
                .collect::<String>();
            line.split('`')
                .nth(1)
                .filter(|c| c.len() == 40)
                .map(|c| c[..7].to_owned())
                .ok_or_else(|| {
                    format!("no commit for {repo} in crates/bib/styles/NOTICE.md").into()
                })
        };
        Ok(Data {
            styles,
            locales,
            styles_commit: commit(STYLES_REPO)?,
            locales_commit: commit(LOCALES_REPO)?,
        })
    }

    fn entries(&self) -> [Dependency; 2] {
        [
            Dependency {
                name: format!("CSL styles ({})", self.styles),
                version: self.styles_commit.clone(),
                license: CSL_LICENSE.into(),
                repository: Some(STYLES_REPO.into()),
            },
            Dependency {
                name: format!("CSL locales ({})", self.locales),
                version: self.locales_commit.clone(),
                license: CSL_LICENSE.into(),
                repository: Some(LOCALES_REPO.into()),
            },
        ]
    }
}

#[derive(Debug, Serialize)]
struct Dependency {
    name: String,
    version: String,
    license: String,
    repository: Option<String>,
}

#[derive(Debug, Serialize)]
struct Dependencies<'a> {
    artefact: &'a str,
    version: &'a str,
    target: &'a str,
    /// Crates and data, by name (ignoring case).
    dependencies: Vec<Dependency>,
}

fn dependencies_json(
    artefact: &Artefact,
    version: &str,
    crates: &[Crate],
    data: Option<&Data>,
) -> Result<String, Error> {
    let mut dependencies: Vec<Dependency> = crates
        .iter()
        .map(|c| Dependency {
            name: c.name.clone(),
            version: c.version.clone(),
            license: c.license.clone(),
            repository: c.repository.clone(),
        })
        .collect();
    dependencies.extend(data.into_iter().flat_map(Data::entries));
    dependencies.sort_by(|a, b| {
        (a.name.to_lowercase(), &a.name, &a.version).cmp(&(
            b.name.to_lowercase(),
            &b.name,
            &b.version,
        ))
    });
    let file = Dependencies {
        artefact: artefact.package,
        version,
        target: TARGET,
        dependencies,
    };
    Ok(serde_json::to_string_pretty(&file)? + "\n")
}

const RULE: &str =
    "================================================================================";

fn notices_text(
    artefact: &Artefact,
    version: &str,
    crates: &[Crate],
    data: Option<&Data>,
) -> Result<String, Error> {
    // Each distinct text once (ignoring layout), with the crates using it.
    struct Group<'a> {
        heading: String,
        body: &'a str,
        users: BTreeSet<usize>,
    }
    let mut groups: BTreeMap<String, Group> = BTreeMap::new();
    for (i, c) in crates.iter().enumerate() {
        for text in &c.texts {
            let key = text.body.split_whitespace().collect::<Vec<_>>().join(" ");
            let group = groups.entry(key).or_insert_with(|| Group {
                heading: match &text.ids[..] {
                    [] => format!("Notice ({})", text.file),
                    [id] => format!("{} ({id})", title(id)),
                    ids => format!("Licences {} ({}, one text)", ids.join(", "), text.file),
                },
                body: &text.body,
                users: BTreeSet::new(),
            });
            group.users.insert(i);
        }
    }
    let mut groups: Vec<Group> = groups.into_values().collect();
    groups.sort_by(|a, b| {
        (&a.heading, a.users.first(), a.body).cmp(&(&b.heading, b.users.first(), b.body))
    });

    let mut out = String::new();
    writeln!(out, "Third-party software and data in aspero {version}")?;
    writeln!(out)?;
    let data_too = match data {
        Some(_) => " It also bundles the citation style data at the end.",
        None => "",
    };
    let intro = format!(
        "{}, built for {TARGET}, is made from the {} Rust crates below, \
         aspero's own among them.{data_too} Each licence or notice text appears \
         once, after the crates under it and their copyright notices.",
        artefact.what,
        crates.len(),
    );
    writeln!(out, "{}", wrap(&intro, 76))?;
    writeln!(out)?;
    writeln!(
        out,
        "Generated from Cargo.lock by `just notices`; don't edit by hand."
    )?;
    for group in &groups {
        writeln!(out)?;
        writeln!(out, "{RULE}")?;
        writeln!(out, "{}", group.heading)?;
        writeln!(out)?;
        writeln!(out, "Used by:")?;
        for &i in &group.users {
            let c = &crates[i];
            let credit = c.credit();
            if credit.is_empty() {
                writeln!(out, "  {} {}", c.name, c.version)?;
            } else {
                writeln!(out, "  {} {} ({credit})", c.name, c.version)?;
            }
        }
        writeln!(out)?;
        writeln!(out, "{}", group.body)?;
    }
    if let Some(data) = data {
        writeln!(out)?;
        writeln!(out, "{RULE}")?;
        writeln!(
            out,
            "Citation Style Language styles and locales (CC-BY-SA-3.0)"
        )?;
        writeln!(out)?;
        for entry in data.entries() {
            let repository = entry.repository.unwrap_or_default();
            writeln!(out, "  {} {}: {repository}", entry.name, entry.version)?;
        }
        writeln!(out)?;
        write!(out, "{}", aspero_bib::cite::credits())?;
        writeln!(out)?;
        writeln!(
            out,
            "The files are unmodified. Creative Commons Attribution-ShareAlike 3.0\n\
             Unported: https://creativecommons.org/licenses/by-sa/3.0/legalcode"
        )?;
    }
    Ok(out)
}

/// `text` broken into lines of at most `width` (words allowing).
fn wrap(text: &str, width: usize) -> String {
    let mut out = String::new();
    let mut line = 0;
    for word in text.split_whitespace() {
        if line > 0 && line + 1 + word.len() > width {
            out.push('\n');
            line = 0;
        } else if line > 0 {
            out.push(' ');
            line += 1;
        }
        out.push_str(word);
        line += word.len();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(s: &str, covered: &[&str]) -> Vec<String> {
        let covered = covered.iter().copied().collect();
        Expr::parse(s).unwrap().missing(&covered)
    }

    #[test]
    fn expressions_say_which_texts_are_owed() {
        assert_eq!(
            ids("MIT OR Apache-2.0", &["Apache-2.0"]),
            Vec::<String>::new()
        );
        assert_eq!(ids("MIT OR Apache-2.0", &[]), ["MIT"]);
        assert_eq!(ids("MIT/Apache-2.0", &[]), ["MIT"]);
        assert_eq!(
            ids("(MIT OR Apache-2.0) AND Unicode-3.0", &["MIT"]),
            ["Unicode-3.0"]
        );
        assert_eq!(
            ids("Apache-2.0 WITH LLVM-exception OR MIT", &[]),
            ["Apache-2.0"]
        );
        assert_eq!(
            ids("ISC AND (Apache-2.0 OR ISC)", &["ISC"]),
            Vec::<String>::new()
        );
        assert!(Expr::parse("MIT OR").is_err());
        assert!(Expr::parse("(MIT").is_err());
    }

    #[test]
    fn copyright_lines_are_told_from_licence_wording() {
        let yes = [
            "Copyright (c) 2015 Andrew Gallant",
            "Copyright 2018 The Rust Project Developers",
            "  Copyright © 1991-2024 Unicode, Inc.",
            "// Copyright The Example Authors",
            "Portions Copyright (C) 2001 Someone",
        ];
        for line in yes {
            assert!(copyright(line).is_some(), "{line}");
        }
        let no = [
            "   2. Grant of Copyright License. Subject to the terms",
            "Copyright and Related Rights (defined below) upon the creative work",
            "THE SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS",
            "The above copyright notice and this permission notice shall be included",
        ];
        for line in no {
            assert!(copyright(line).is_none(), "{line}");
        }
        assert!(placeholder("Copyright [yyyy] [name of copyright owner]"));
        assert!(placeholder("Copyright (c) <year> <copyright holders>"));
    }

    #[test]
    fn templates_read_as_their_licence() {
        for id in [
            "0BSD",
            "Apache-2.0",
            "BSD-2-Clause",
            "BSD-3-Clause",
            "ISC",
            "MIT",
            "MPL-2.0",
            "Unicode-3.0",
            "Zlib",
        ] {
            let (body, _) = split_copyrights(template(id).unwrap());
            assert_eq!(classify(&body), [id]);
        }
    }

    #[test]
    fn notices_are_current() {
        let root = workspace_root();
        let outputs = generate(&root).unwrap();
        let stale = stale(&root, &outputs);
        assert!(
            stale.is_empty(),
            "third-party notices are out of date ({stale:?}): {FIX}"
        );
    }

    #[test]
    fn generating_twice_gives_the_same_files() {
        let root = workspace_root();
        assert_eq!(generate(&root).unwrap(), generate(&root).unwrap());
    }
}
