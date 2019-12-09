use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use failure::bail;
use failure::format_err;
use failure::Error;
use failure::ResultExt;
use log::info;
use tempfile_fast::Sponge;

pub fn base_image<P: AsRef<Path>>(cache: P, distro: &str) -> Result<PathBuf, Error> {
    let mut path = cache.as_ref().to_path_buf();
    path.push("base-images");
    path.push(distro);
    Ok(path)
}

pub fn fetch_ubuntu(cache: &Path, distros: &[&str]) -> Result<(), Error> {
    for distro in distros {
        let mut path = base_image(cache, distro)?;
        fs::create_dir_all(&path)?;
        path.push("root.tar.gz");

        let url = format!(
            "https://partner-images.canonical.com/core/{0}/current/ubuntu-{0}-core-cloudimg-amd64-root.tar.gz",
            distro
        );

        info!("downloading {} to {:?}", url, path);

        if download_if_newer::ensure_downloaded_slop(&url, &path, Duration::from_secs(18 * 60 * 60))
            .with_context(|_| format_err!("downloading {:?} to {:?}", url, path))?
        {
            let gz = path.clone();
            path.pop();
            path.push("root.tar.zstd");
            let mut sponge = Sponge::new_for(&path)?;
            let mut encoder = zstd::Encoder::new(sponge, 3)?;
            io::copy(
                &mut flate2::read::GzDecoder::new(fs::File::open(&gz)?),
                &mut encoder,
            )?;
            encoder.finish()?.commit()?;
        }
    }

    Ok(())
}
