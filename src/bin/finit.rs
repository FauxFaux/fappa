use std::fs;

use failure::Error;
use nix::unistd;

fn main() -> Result<(), Error> {
    #[cfg(not(debug_assertions))]
    assert_eq!(
        unistd::Pid::from_raw(1),
        unistd::getpid(),
        "we're expecting to be running as init (pid 1)!"
    );

    eprintln!("hi from init!");
    for p in psutil::process::all()? {
        eprintln!("{} {:?}", p.pid, p.cmdline()?);
    }

    for f in fs::read_dir("/proc/self/fd")? {
        let f = f?;
        println!("{:?} - {:?}", f.file_name(), fs::read_link(f.path()));
    }

    Ok(())
}
