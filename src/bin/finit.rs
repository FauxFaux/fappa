use std::env;
use std::fs;
use std::io::Write;
use std::os::unix::io::FromRawFd;

use byteorder::WriteBytesExt;
use byteorder::LE;
use failure::Error;
use nix::unistd;

fn main() -> Result<(), Error> {
    assert_eq!(
        unistd::Pid::from_raw(1),
        unistd::getpid(),
        "we're expecting to be running as init (pid 1)!"
    );

    assert_eq!(3, env::args().len());
    let recv = env::args().nth(1).unwrap().parse()?;
    let send = env::args().nth(2).unwrap().parse()?;

    let recv = unsafe { os_pipe::PipeReader::from_raw_fd(recv) };
    let mut send = unsafe { os_pipe::PipeWriter::from_raw_fd(send) };

    {
        let mut msg = Vec::new();
        msg.write_u64::<LE>(16)?;
        msg.write_u64::<LE>(1)?;
        send.write_all(&msg)?;
    }

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
