use std::fs;
use std::path::Path;

use anyhow::bail;
use anyhow::ensure;
use anyhow::Error;
use anyhow::Context;
use walkdir;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageSerialisation {
    pub commands: Vec<Command>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Package {
    pub ser: PackageSerialisation,
}

impl Package {
    fn from_str(ser: PackageSerialisation) -> Result<Package, Error> {
        Ok(Package { ser })
    }

    pub fn exclude_files(&self) -> ! {
        unimplemented!()
    }
    pub fn source(&self) -> Vec<Command> {
        unimplemented!()
    }
}

fn parse_commands(v: Vec<String>) -> Result<Vec<Command>, Error> {
    v.into_iter().map(parse_command).collect()
}

fn parse_command<S: AsRef<str>>(cmd: S) -> Result<Command, Error> {
    let cmd = cmd.as_ref();

    ensure!(
        !cmd.contains(|c: char| c.is_control()),
        "no control characters in the tunnel"
    );

    let (op, args) = split_space(cmd);

    Ok(match op {
        "CLONE" => {
            let mut args = args.split(' ');
            Command::Clone {
                repo: args.next().unwrap().to_string(),
                branch: args.next().unwrap().to_string(),
                sha: args.next().unwrap().to_string(),
                dest: args.next().unwrap().to_string(),
            }
        }
        "AUTORECONF" => {
            ensure!(args.is_empty(), "autoreconf takes no arguments: {:?}", args);
            Command::Autoreconf
        }
        "CMAKE" => {
            ensure!(args.is_empty(), "cmake takes no arguments: {:?}", args);
            Command::CMake
        }
        //"WORKDIR" => Command::WorkDir(args.to_string()),
        //"RUN" => Command::Run(args.to_string()),
        other => bail!("unrecognised op: {:?}", other),
    })
}

fn split_space(s: &str) -> (&str, &str) {
    match s.find(' ') {
        Some(p) => {
            let (left, right) = s.split_at(p);
            (left.trim(), right.trim_start())
        }
        None => (s.trim(), ""),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    Clone {
        repo: String,
        branch: String,
        sha: String,
        dest: String,
    },
    Autoreconf,
    CMake,
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

        let text =
            fs::read(entry.path()).with_context(|| format!("opening {:?}", entry.path()))?;
        let text = String::from_utf8(text)?;
        let commands = blocks(&text)
            .into_iter()
            .map(parse_command)
            .collect::<Result<Vec<_>, Error>>()
            .with_context(|| format!("parsing an entry in {:?}", entry.path()))?;

        ret.push(Package {
            ser: PackageSerialisation { commands },
        });
    }

    Ok(ret)
}

fn blocks(text: &str) -> Vec<String> {
    let text: Vec<_> = text
        .lines()
        .map(|l| l.trim_end())
        .filter(|l| !l.starts_with('#'))
        .collect();

    text.split(|l| l.is_empty())
        .filter(|b| !b.is_empty())
        .map(|lines| lines.join("\n"))
        .collect()
}
