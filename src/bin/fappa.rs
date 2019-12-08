use failure::err_msg;
use failure::Error;
use failure::ResultExt;

use fappa::build;
use fappa::fetch_images;
use fappa::git;
use fappa::namespace;
use fappa::specs;
use fappa::RELEASES;

fn main() -> Result<(), Error> {
    pretty_env_logger::init();
    let dirs = directories::ProjectDirs::from("xxx", "fau", "fappa")
        .ok_or_else(|| err_msg("no project dirs"))?;

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
                for command in package.source() {
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
            let root = matches.is_present("root");
            let cmd = matches.value_of("cmd").unwrap().as_bytes();

            let child = namespace::unpack_to_temp(dirs.cache_dir(), "disco")
                .with_context(|_| err_msg("opening distro container"))?;

            let mut child = namespace::launch_our_init(&child)
                .with_context(|_| err_msg("launching init"))?;

            namespace::child::await_ready(&mut child)?;
            namespace::child::execute(&mut child, root, cmd)?;
            namespace::child::shutdown(&mut child)?;
        }
        ("fetch", _) => {
            let ubuntu_codenames = RELEASES
                .iter()
                .filter(|r| "ubuntu" == r.distro())
                .map(|r| r.codename())
                .collect::<Vec<_>>();

            fetch_images::fetch_ubuntu(dirs.cache_dir(), &ubuntu_codenames)?;
        }
        _ => unreachable!(),
    }

    Ok(())
}
