extern crate clap;

#[macro_use]
extern crate failure;
extern crate git2;
extern crate handlebars;

#[macro_use]
extern crate serde_derive;

#[macro_use]
extern crate serde_json;
extern crate shiplift;
extern crate tempdir;
extern crate toml;
extern crate url;
extern crate walkdir;

mod git;
mod specs;

use std::fs;
use std::io;
use std::io::Write;

use failure::Error;
use shiplift::BuildOptions;
use shiplift::Docker;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Release {
    DebianJessie,
    DebianStretch,
    DebianBuster,
    UbuntuTrusty,
    UbuntuXenial,
    UbuntuArtful,
    UbuntuBionic,
    UbuntuCosmic,
}

const RELEASES: [Release; 8] = [
    // best
    Release::UbuntuBionic,
    Release::DebianStretch,
    // older but supported
    Release::UbuntuXenial,
    Release::UbuntuTrusty,
    Release::UbuntuArtful,
    Release::DebianJessie,
    // pre-release
    Release::UbuntuCosmic,
    Release::DebianBuster,
];

impl Release {
    fn distro(&self) -> &'static str {
        use Release::*;
        match self {
            | DebianJessie | DebianStretch | DebianBuster => "debian",
            | UbuntuTrusty | UbuntuXenial | UbuntuArtful | UbuntuBionic | UbuntuCosmic => "ubuntu",
        }
    }

    fn codename(&self) -> &'static str {
        use Release::*;
        match self {
            | DebianJessie => "jessie",
            | DebianStretch => "stretch",
            | DebianBuster => "buster",
            | UbuntuTrusty => "trusty",
            | UbuntuXenial => "xenial",
            | UbuntuArtful => "artful",
            | UbuntuBionic => "bionic",
            | UbuntuCosmic => "cosmic",
        }
    }

    /// Older distros lack the locales-all package, which makes the locale
    /// environment a lot more sane for builds. Perhaps we should generate some
    /// extra locales on these distros?
    fn locales_all(&self) -> bool {
        use Release::*;
        match self {
            | DebianJessie => false,
            | DebianStretch => true,
            | DebianBuster => true,
            | UbuntuTrusty => false,
            | UbuntuXenial => true,
            | UbuntuArtful => true,
            | UbuntuBionic => true,
            | UbuntuCosmic => true,
        }
    }
}

fn build_template(docker: &Docker, release: Release) -> Result<(), Error> {
    let dir = tempdir::TempDir::new("fappa")?;
    let from = format!("{}:{}", release.distro(), release.codename());

    {
        let mut dockerfile = dir.path().to_path_buf();
        dockerfile.push("Dockerfile");
        let mut dockerfile = fs::File::create(dockerfile)?;

        let reg = handlebars::Handlebars::new();
        reg.render_template_to_write(
            include_str!("build.Dockerfile.hbs"),
            &json!({
            "from": from,
            "locales": if release.locales_all() { "locales-all" } else { "locales" },
        }),
            &mut dockerfile,
        )?;

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

    let dir_as_str = dir
        .path()
        .as_os_str()
        .to_str()
        .ok_or(format_err!("unrepresentable path and dumb library"))?;

    for line in docker.images().build(&BuildOptions::builder(dir_as_str)
        .tag(format!("fappa-{}", release.codename()))
        .network_mode("mope")
        .build())?
    {
        let line = line
            .as_object()
            .ok_or_else(|| format_err!("unexpected line: {:?}", line))?;
        if let Some(msg) = line.get("stream").and_then(|stream| stream.as_string()) {
            for line in msg.trim_right_matches('\n').split('\n') {
                println!(
                    "[{}] log: {}",
                    release.codename(),
                    line.replace(|c| u32::from(c) < 32, " ")
                );
            }
        } else if let Some(aux) = line.get("aux").and_then(|aux| aux.as_object()) {
            println!("[{}] aux: {:?}", release.codename(), aux)
        } else {
            bail!("unknown notification: {:?}", line);
        }
    }

    Ok(())
}

fn main() -> Result<(), Error> {
    use clap::Arg;
    use clap::SubCommand;
    let matches = clap::App::new("fappa")
        .setting(clap::AppSettings::SubcommandRequiredElseHelp)
        .subcommand(SubCommand::with_name("build-images").arg(Arg::with_name("pull").long("pull")))
        .subcommand(SubCommand::with_name("validate"))
        .get_matches();

    // oh no I think this panics inside. /o\
    let docker = shiplift::Docker::new();

    match matches.subcommand() {
        ("build-images", Some(matches)) => {
            if matches.is_present("pull") {
                for release in &RELEASES {
                    print!("Pulling {:?}..", release);
                    io::stdout().flush()?;
                    docker.images().pull(&shiplift::PullOptions::builder()
                        .image(release.distro())
                        .tag(release.codename())
                        .build())?;
                    println!(". done.");
                }
            }

            for release in &RELEASES {
                build_template(&docker, *release)?;
            }
        }
        ("validate", _) => {
            for package in specs::load_from("specs")? {
                git::check_cloned(package.source)?;
            }
        }
        _ => unreachable!(),
    }

    Ok(())
}
