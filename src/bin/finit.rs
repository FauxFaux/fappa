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

    Ok(())
}
