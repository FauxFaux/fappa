use std::fs;

use async_fetcher::AsyncFetcher;
use failure::err_msg;
use failure::Error;
use futures::future::Future;
use reqwest::r#async::Client;
use tokio;

pub fn fetch() -> Result<(), Error> {
    let client = Client::new();

    let distros = &["trusty", "xenial", "bionic", "cosmic", "disco"];

    let mut futures = Vec::new();

    for distro in distros {
        fs::create_dir_all(distro)?;
        let root = format!(
            "https://partner-images.canonical.com/core/{}/current",
            distro
        );

        for (name, dest) in &[
            ("SHA256SUMS.gpg".to_string(), "SHA256SUMS.gpg".to_string()),
            ("SHA256SUMS".to_string(), "SHA256SUMS".to_string()),
            (
                format!("ubuntu-{}-core-cloudimg-amd64-root.tar.gz", distro),
                "amd64-root.tar.gz".to_string(),
            ),
        ] {
            let log_prefix = format!("{}: {}", distro, name);
            futures.push(
                AsyncFetcher::new(&client, format!("{}/{}", root, name))
                    .with_progress_callback(move |ev| {
                        use async_fetcher::FetchEvent::*;
                        match ev {
                            Get => println!("{}: Downloading...", log_prefix),
                            DownloadComplete => println!("{}: Complete.", log_prefix),
                            _ => (),
                        }
                    })
                    .request_to_path(format!("{}/{}", distro, name).into())
                    .then_download(format!("{}/.{}.partial", distro, name).into())
                    .then_rename()
                    .into_future(),
            );
        }
    }

    let mut runtime = tokio::runtime::Runtime::new()?;
    for future in futures {
        runtime.block_on(future)?;
    }

    //runtime.block_on(futures::future::join_all(futures))?;

    runtime
        .shutdown_now()
        .wait()
        .map_err(|()| err_msg("failed during shutdown"))?;

    Ok(())
}
