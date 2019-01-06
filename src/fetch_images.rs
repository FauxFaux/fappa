use std::fs;
use std::sync::Arc;

use async_fetcher::AsyncFetcher;
use failure::err_msg;
use failure::Error;
use futures::future::Future;
use futures::stream;
use futures::stream::Stream;
use reqwest::r#async::Client;
use tokio;

pub fn fetch() -> Result<(), Error> {
    let distros = &["trusty", "xenial", "bionic", "cosmic", "disco"];

    let mut work = Vec::new();

    for distro in distros {
        fs::create_dir_all(distro)?;

        let root = format!(
            "https://partner-images.canonical.com/core/{}/current",
            distro
        );

        for (name, dest) in &[
            ("SHA256SUMS.gpg".to_string(), "SHA256SUMS.gpg"),
            ("SHA256SUMS".to_string(), "SHA256SUMS"),
            (
                format!("ubuntu-{}-core-cloudimg-amd64-root.tar.gz", distro),
                "amd64-root.tar.gz",
            ),
        ] {
            work.push((
                format!("{}/{}", root, name),
                format!("{}/{}", distro, name),
                format!("{}/.{}.partial", distro, name),
                format!("{}: {}", distro, name),
            ));
        }
    }

    let client = Arc::new(Client::new());

    let s = stream::iter_ok(work)
        .map(move |(url, dest, temp, log_prefix)| {
            AsyncFetcher::new(&client, url)
                .with_progress_callback(move |ev| {
                    use async_fetcher::FetchEvent::*;
                    match ev {
                        Get => println!("{}: Downloading...", log_prefix),
                        DownloadComplete => println!("{}: Complete.", log_prefix),
                        _ => (),
                    }
                })
                .request_to_path(dest.into())
                .then_download(temp.into())
                .then_rename()
                .into_future()
        })
        .buffer_unordered(6);

    let mut runtime = tokio::runtime::Runtime::new()?;

    runtime
        .block_on(s.into_future())
        .map_err(|(fetch, _todo)| fetch)?;

    runtime
        .shutdown_now()
        .wait()
        .map_err(|()| err_msg("failed during shutdown"))?;

    Ok(())
}
