mod build;
mod fetch_images;
mod git;
mod namespace;
mod specs;
mod unpack;

use failure::bail;
use failure::Error;
use serde_json::json;

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

fn build_template(release: Release) -> Result<(), Error> {
    let from = format!("{}:{}", release.distro(), release.codename());

    {
        include_str!("prepare-image.Dockerfile.hbs");
        &json!({
            "from": from,
            "locales": if release.locales_all() { "locales-all" } else { "locales" },
        });
    }

    unimplemented!("can't build");
}

fn main() -> Result<(), Error> {
    pretty_env_logger::init();

    use clap::Arg;
    use clap::SubCommand;
    let matches = clap::App::new("fappa")
        .setting(clap::AppSettings::SubcommandRequiredElseHelp)
        .subcommand(SubCommand::with_name("build-images").arg(Arg::with_name("pull").long("pull")))
        .subcommand(SubCommand::with_name("validate"))
        .subcommand(SubCommand::with_name("build"))
        .subcommand(
            SubCommand::with_name("namespace")
                .arg(
                    Arg::with_name("cmd")
                        .short("c")
                        .required(true)
                        .takes_value(true),
                )
                .arg(Arg::with_name("root").short("r")),
        )
        .subcommand(SubCommand::with_name("fetch"))
        .subcommand(SubCommand::with_name("null"))
        .get_matches();

    match matches.subcommand() {
        ("build-images", _) => {
            for release in &RELEASES {
                build_template(*release)?;
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
                    build::build(release, &package)?;
                }
            }
        }
        ("namespace", Some(matches)) => {
            use namespace::child::FromChild;
            let mut child = namespace::prepare("cosmic")?;
            while let Some(event) = child.msg()? {
                match event {
                    FromChild::Ready => break,
                    FromChild::Debug(m) => println!("child says: {}", m),
                    _ => bail!("unexpected event: {:?}", event),
                }
            }

            use std::os::unix::ffi::OsStrExt;

            let code = match matches.is_present("root") {
                true => 102,
                false => 100,
            };
            child.write_msg(code, matches.value_of_os("cmd").unwrap().as_bytes())?;

            while let Some(event) = child.msg()? {
                match event {
                    FromChild::Debug(m) => println!("child says: {}", m),
                    FromChild::Output(m) => {
                        println!("child printed: {:?}", String::from_utf8_lossy(&m))
                    }
                    FromChild::SubExited(c) => {
                        println!("child exited: {}", c);
                        break;
                    }
                    _ => bail!("unexpected event: {:?}", event),
                }
            }
            child.write_msg(101, &[])?;
            println!("{:?}", child.msg()?);
        }
        ("null", _) => {
            println!();
        }
        ("fetch", _) => {
            let ubuntu_codenames = RELEASES
                .iter()
                .filter(|r| "ubuntu" == r.distro())
                .map(|r| r.codename())
                .collect::<Vec<_>>();
            fetch_images::fetch_ubuntu(&ubuntu_codenames)?;

            unimplemented!("fetch debian");
        }
        _ => unreachable!(),
    }

    Ok(())
}
