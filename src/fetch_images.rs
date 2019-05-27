use std::fs;
use std::io;
use std::path::Path;

use failure::bail;
use failure::Error;
use log::info;

pub fn fetch_ubuntu(cache: &Path, distros: &[&str]) -> Result<(), Error> {
    for distro in distros {
        let mut path = cache.to_path_buf();
        path.push("base-images");
        path.push(distro);
        fs::create_dir_all(&path)?;
        path.push("root.tar.gz");

        let url = format!(
            "https://partner-images.canonical.com/core/{0}/current/ubuntu-{0}-core-cloudimg-amd64-root.tar.gz",
            distro
        );

        info!("downloading {} to {:?}", url, path);

        let mut target_file = fs::File::create(path)?;

        let resp = ureq::get(&url).call();

        if !resp.ok() {
            bail!("download of {} failed: {}", url, resp.status());
        }

        io::copy(&mut resp.into_reader(), &mut target_file)?;
    }

    Ok(())
}
