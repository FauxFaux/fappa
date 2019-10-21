use std::ffi::CStr;
use std::fmt::Display;
use std::fs;
use std::io;
use std::io::BufRead;
use std::process;

use failure::ensure;
use failure::err_msg;
use failure::format_err;
use failure::Error;
use failure::ResultExt;
use nix::unistd::getegid;
use nix::unistd::geteuid;
use nix::unistd::Pid;

pub fn map_us(first_fork: Pid) -> Result<(), Error> {
    let us = unsafe { bad_get_login() }?;
    map(first_fork, &us, geteuid(), "uid").with_context(|_| err_msg("mapping uid"))?;
    map(first_fork, &us, getegid(), "gid").with_context(|_| err_msg("mapping gid"))?;
    Ok(())
}

// the error handling here sucks
//
// tl;dr you should have an entry in both /etc/subuid and /etc/subgid for your user*name*,
// which looks like `faux:100000:65536`. The middle number can be anything, but the last number
// must be 65536. We only look at the first entry. This is what `adduser` does on reasonable,
// modern machines. If you've upgraded, you might not have it, and might need to make one
// yourself.
#[inline]
pub fn map<D: Display>(first_fork: Pid, us: &str, id: D, style: &'static str) -> Result<(), Error> {
    let file = format!("/etc/sub{}", style);
    let (start, len) = load_first_sub_id_entry(&us, &file)
        .with_context(|_| format_err!("loading {:?} from {:?}", us, file))?
        .ok_or(format_err!("no sub{} entry for {:?}", style, us))?;

    ensure!(len == 65536, "too few {}s: {}", style, len);

    let command = format!("new{}map", style);
    let pid = format!("{}", first_fork);
    let id = format!("{}", id);
    let start = format!("{}", start);

    let exit_status = process::Command::new(command)
        .args(&[
            &pid, // for this pid,
            "0", &id, "1", // root maps to `id`, for a range of 1
            "1", &start, "65535", // and `1` to `max-id` (assumed) maps to our sub range
        ])
        .status()
        .with_context(|_| format_err!("running new{}map (uidmap package)", style))?;

    ensure!(
        exit_status.success(),
        "setting up new{}map for worker failed: {}",
        style,
        exit_status,
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
