use std::ffi::CStr;
use std::fs;
use std::io;
use std::io::BufRead;

use failure::ensure;
use failure::err_msg;
use failure::format_err;
use failure::Error;
use failure::ResultExt;
use nix::unistd::getegid;
use nix::unistd::geteuid;
use nix::unistd::Pid;

pub fn map_us(first_fork: Pid) -> Result<(), Error> {
    let real_euid = geteuid();
    let real_egid = getegid();

    let us = unsafe { bad_get_login()? };

    // the error handling here sucks
    // tl;dr you should have an entry in both /etc/subuid and /etc/subgid for your user*name*,
    // which looks like `faux:100000:65536`. The middle number can be anything, but the last number
    // must be 65536. We only look at the first entry. This is what `adduser` does on reasonable,
    // modern machines. If you've upgraded, you might not have it, and might need to make one
    // yourself.

    let (uid_start, uid_len) = load_first_sub_id_entry(&us, "/etc/subuid")?
        .ok_or(format_err!("no subuid entry for {:?}", us))?;
    let (gid_start, gid_len) = load_first_sub_id_entry(&us, "/etc/subgid")?
        .ok_or(format_err!("no subgid entry for {:?}", us))?;

    ensure!(uid_len == 65536 || gid_len == 65536, "too few ids");

    ensure!(
        std::process::Command::new("newuidmap")
            .args(&[
                &format!("{}", first_fork),
                "0",
                &format!("{}", real_euid),
                "1",
                "1",
                &format!("{}", uid_start),
                "65535"
            ])
            .status()
            .with_context(|_| err_msg("running newuidmap (uidmap package)"))?
            .success(),
        "setting up newuidmap for worker"
    );

    ensure!(
        std::process::Command::new("newgidmap")
            .args(&[
                &format!("{}", first_fork),
                "0",
                &format!("{}", real_egid),
                "1",
                "1",
                &format!("{}", gid_start),
                "65535"
            ])
            .status()
            .with_context(|_| err_msg("running newgidmap (uidmap package)"))?
            .success(),
        "setting up newgidmap for worker"
    );

    Ok(())
}

fn load_first_sub_id_entry(id: &str, file: &str) -> Result<Option<(u64, u64)>, Error> {
    let file = io::BufReader::new(fs::File::open(file)?);
    for line in file.lines() {
        let line = line?;
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split(':');
        let name_or_id = parts.next().ok_or(err_msg("invalid line: no name"))?;
        if name_or_id == id {
            let start = parts
                .next()
                .ok_or(err_msg("invalid line: no first number"))?;
            let end = parts
                .next()
                .ok_or(err_msg("invalid line: no second number"))?;
            return Ok(Some((start.parse()?, end.parse()?)));
        }
    }

    Ok(None)
}

// nix is adding getpwuid_r, which would be way better
// not thread safe
unsafe fn bad_get_login() -> Result<String, Error> {
    Ok(CStr::from_ptr(libc::getlogin()).to_str()?.to_string())
}
