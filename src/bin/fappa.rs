use failure::bail;
use failure::Error;

use fappa::build;
use fappa::fetch_images;
use fappa::git;
use fappa::namespace;
use fappa::specs;
use fappa::RELEASES;

fn main() -> Result<(), Error> {
    pretty_env_logger::init();

    use clap::Arg;
    use clap::SubCommand;
    let matches = clap::App::new("fappa")
        .setting(clap::AppSettings::SubcommandRequiredElseHelp)
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
        .get_matches();

    match matches.subcommand() {
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
