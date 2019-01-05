// sudo setcap cap_mknod=+ep safenod

use std::env;
use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path;

use failure::bail;
use failure::Error;
use nix::sys::stat;

fn main() -> Result<(), Error> {
    let mut args = env::args_os();

    if !args.next().is_some() {
        bail!("argv[0] is unset?");
    }

    let path = match args.next() {
        Some(arg) => arg,
        None => bail!("usage: path-to-directory"),
    };

    let mut path = path::PathBuf::from(path);
    path.push("dev");

    fs::DirBuilder::new().mode(0o755).create(&path)?;

    let all_read_write = stat::Mode::from_bits(0o666).expect("static data");

    for (name, major, minor) in &[
        ("null", 1, 3),
        ("zero", 1, 5),
        ("full", 1, 7),
        (
            "random",
            1,
            #[cfg(feature = "real-random")]
            8,
            #[cfg(not(feature = "real-random"))]
            9,
        ),
        ("urandom", 1, 9),
    ] {
        path.push(name);
        stat::mknod(
            &path,
            stat::SFlag::S_IFCHR,
            all_read_write,
            stat::makedev(*major, *minor),
        )?;
        path.pop();
    }

    Ok(())
}
