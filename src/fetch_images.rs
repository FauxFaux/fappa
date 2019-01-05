use std::fs;

use async_fetcher::AsyncFetcher;
use failure::Error;
use reqwest::r#async::Client;
use tokio;

pub fn fetch() -> Result<(), Error> {
    let client = Client::new();
    let mut runtime = tokio::runtime::Runtime::new()?;

    let distros = &["trusty", "xenial", "bionic", "cosmic", "disco"];

    let mut futures = Vec::new();

    for distro in distros {
        fs::create_dir_all(distro)?;
        let root = format!("https://partner-images.canonical.com/core/{}/current", distro);

        for name in &["SHA256SUMS.gpg", "SHA256SUMS"] {
            futures.push(AsyncFetcher::new(&client, format!("{}/{}", root, name))
                .request_to_path(format!("{}/{}", distro, name).into())
                .then_download(format!("{}/.{}.partial", distro, name).into())
                .then_rename()
                .into_future());
        }
    }

    runtime.block_on(futures::future::join_all(futures))?;

    Ok(())
}