extern crate clap;
extern crate fs_extra;

#[macro_use]
extern crate failure;
extern crate git2;

#[macro_use]
extern crate serde_derive;

#[macro_use]
extern crate serde_json;
extern crate toml;
extern crate url;
extern crate walkdir;

mod build;
mod fetch_images;
mod git;
mod namespace;
mod specs;
mod unpack;

use std::fs;
use std::io;
use std::io::Write;

use failure::Error;
use tempfile::TempDir;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Release {
    DebianJessie,
    DebianStretch,
    DebianBuster,
    UbuntuTrusty,
    UbuntuXenial,
    UbuntuBionic,
    UbuntuCosmic,
    UbuntuDisco,
}

const RELEASES: [Release; 8] = [
    // best
    Release::UbuntuBionic,
    Release::DebianStretch,
    // older but supported
    Release::UbuntuXenial,
    Release::UbuntuTrusty,
    Release::DebianJessie,
    // pre-release
    Release::UbuntuCosmic,
    Release::UbuntuDisco,
    Release::DebianBuster,
];

impl Release {
    fn distro(&self) -> &'static str {
        use crate::Release::*;
        match self {
            DebianJessie | DebianStretch | DebianBuster => "debian",
            UbuntuTrusty | UbuntuXenial | UbuntuBionic | UbuntuCosmic | UbuntuDisco => "ubuntu",
        }
    }

    fn codename(&self) -> &'static str {
        use crate::Release::*;
        match self {
            DebianJessie => "jessie",
            DebianStretch => "stretch",
            DebianBuster => "buster",
            UbuntuTrusty => "trusty",
            UbuntuXenial => "xenial",
            UbuntuBionic => "bionic",
            UbuntuCosmic => "cosmic",
            UbuntuDisco => "disco",
        }
    }

    /// Older distros lack the locales-all package, which makes the locale
    /// environment a lot more sane for builds. Perhaps we should generate some
    /// extra locales on these distros?
    fn locales_all(&self) -> bool {
        use crate::Release::*;
        match self {
            DebianJessie => false,
            DebianStretch => true,
            DebianBuster => true,
            UbuntuTrusty => false,
            UbuntuXenial => true,
            UbuntuBionic => true,
            UbuntuCosmic => true,
            UbuntuDisco => true,
        }
    }
}

fn build_template(docker: (), release: Release) -> Result<(), Error> {
    let dir = tempfile::TempDir::new()?;
    let from = format!("{}:{}", release.distro(), release.codename());

    {
        let mut dockerfile = dir.path().to_path_buf();
        dockerfile.push("Dockerfile");
        let mut dockerfile = fs::File::create(dockerfile)?;

        include_str!("prepare-image.Dockerfile.hbs");
        &json!({
            "from": from,
            "locales": if release.locales_all() { "locales-all" } else { "locales" },
        });

        for (file, content) in &[
            (
                "drop-privs-harder.c",
                &include_bytes!("../security-tools/drop-privs-harder.c")[..],
            ),
            (
                "drop-all-caps.c",
                &include_bytes!("../security-tools/drop-all-caps.c")[..],
            ),
            (
                "all-caps.h",
                &include_bytes!("../security-tools/all-caps.h")[..],
            ),
        ] {
            let mut new_file = dir.path().to_path_buf();
            new_file.push(file);
            fs::File::create(new_file)?.write_all(content)?;
        }
    }
    let tag = format!("fappa-{}", release.codename());

    dump_lines(
        release,
        unimplemented!(
            r"
        &docker.images().build(
            &BuildOptions::builder(tempdir_as_bad_str(&dir)?)
                .tag(tag)
                .build(),
        )?"
        ),
    )?;

    Ok(())
}

fn dump_lines(release: Release, lines: &[serde_json::Value]) -> Result<Option<String>, Error> {
    let mut last_id = None;

    for line in lines {
        let line = line
            .as_object()
            .ok_or_else(|| format_err!("unexpected line: {:?}", line))?;
        if let Some(msg) = line.get("stream").and_then(|stream| stream.as_str()) {
            for line in msg.trim_end_matches('\n').split('\n') {
                println!(
                    "[{}] log: {}",
                    release.codename(),
                    line.replace(|c| u32::from(c) < 32, " ")
                );
            }
        } else if let Some(aux) = line.get("aux").and_then(|aux| aux.as_object()) {
            if let Some(id) = aux.get("ID").and_then(|id| id.as_str()) {
                last_id = Some(id.to_string());
            }
            println!("[{}] aux: {:?}", release.codename(), aux)
        } else {
            bail!("unknown notification: {:?}", line);
        }
    }

    Ok(last_id)
}

fn main() -> Result<(), Error> {
    use clap::Arg;
    use clap::SubCommand;
    let matches = clap::App::new("fappa")
        .setting(clap::AppSettings::SubcommandRequiredElseHelp)
        .subcommand(SubCommand::with_name("build-images").arg(Arg::with_name("pull").long("pull")))
        .subcommand(SubCommand::with_name("validate"))
        .subcommand(SubCommand::with_name("build"))
        .subcommand(SubCommand::with_name("namespace"))
        .subcommand(SubCommand::with_name("fetch"))
        .get_matches();

    match matches.subcommand() {
        ("build-images", Some(matches)) => {
            if matches.is_present("pull") {
                for release in &RELEASES {
                    print!("Pulling {:?}..", release);
                    io::stdout().flush()?;
                    unimplemented!();
                    println!(". done.");
                }
            }

            for release in &RELEASES {
                build_template((), *release)?;
            }
        }
        ("validate", _) => {
            for package in specs::load_from("specs")? {
                for command in package.source {
                    match command {
                        specs::Command::Clone { repo, .. } => git::check_cloned(repo)?,
                        _ => continue,
                    };
                }
            }
        }
        ("build", _) => {
            for package in specs::load_from("specs")? {
                for release in &RELEASES {
                    build::build((), release, &package)?;
                }
            }
        }
        ("namespace", _) => {
            println!("{:?}", namespace::prepare("cosmic")?.wait()?.code());
        }
        ("fetch", _) => {
            let ubuntu_codenames = RELEASES
                .iter()
                .filter(|r| "ubuntu" == r.distro())
                .map(|r| r.codename())
                .collect::<Vec<_>>();
            fetch_images::fetch_ubuntu(&ubuntu_codenames)?;
        }
        _ => unreachable!(),
    }

    Ok(())
}
