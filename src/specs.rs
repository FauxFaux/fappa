use std::fs;
use std::path::Path;

use failure::Error;
use failure::ResultExt;
use toml;
use url::Url;
use walkdir;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
struct Spec {
    package: Vec<PackageSerialisation>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
struct PackageSerialisation {
    name: String,
    build_dep: Vec<String>,
    dep: Vec<String>,
    source: Vec<String>,
    build: Vec<String>,
    install: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Package {
    pub name: String,
    pub build_dep: Vec<String>,
    pub dep: Vec<String>,
    pub source: Vec<Command>,
    pub build: Vec<Command>,
    pub install: Vec<Command>,
}

impl Package {
    fn from_str(p: PackageSerialisation) -> Result<Package, Error> {
        Ok(Package {
            name: p.name,
            build_dep: p.build_dep,
            dep: p.dep,
            source: parse_commands(p.source)?,
            build: parse_commands(p.build)?,
            install: parse_commands(p.install)?,
        })
    }
}

fn parse_commands(v: Vec<String>) -> Result<Vec<Command>, Error> {
    v.into_iter().map(parse_command).collect()
}

fn parse_command<S: AsRef<str>>(cmd: S) -> Result<Command, Error> {
    let cmd = cmd.as_ref();
    let (op, args) = split_space(cmd);

    Ok(match op {
        "CLONE" => {
            let (url, dest) = split_space(args);
            Command::Clone {
                repo: url.parse()?,
                dest: dest.to_string(),
            }
        }
        other => bail!("unrecognised op: {:?}", other),
    })
}

fn split_space(s: &str) -> (&str, &str) {
    match s.find(' ') {
        Some(p) => {
            let (left, right) = s.split_at(p);
            (left.trim(), right.trim_left())
        }
        None => (s.trim(), ""),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    Clone { repo: Url, dest: String },
}

pub fn load_from<P: AsRef<Path>>(dir: P) -> Result<Vec<Package>, Error> {
    let mut ret = Vec::new();

    for entry in walkdir::WalkDir::new(dir) {
        let entry = entry?;

        if entry.file_type().is_dir() {
            continue;
        }

        if entry
            .file_name()
            .to_str()
            .map(|s| s.starts_with("."))
            .unwrap_or(false)
        {
            continue;
        }

        let spec: Spec = toml::from_slice(
            &fs::read(entry.path()).with_context(|_| format!("opening {:?}", entry.path()))?
        )?;

        ret.extend(spec
            .package
            .into_iter()
            .map(Package::from_str)
            .collect::<Result<Vec<Package>, Error>>()?)
    }

    Ok(ret)
}
