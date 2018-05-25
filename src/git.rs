use std::path::PathBuf;

use failure::Error;
use git2;
use url::Url;

pub fn check_cloned<S: AsRef<str>>(url: S) -> Result<PathBuf, Error> {
    let mut url = Url::parse(url.as_ref())?;
    {
        let (mode, value) = {
            let mut args = url.query_pairs();
            ensure!(1 == args.count(), "there must be only one arg");
            args.next().unwrap()
        };

        match mode.as_ref() {
            "rev" => (),
            other => bail!("unsupported mode: {}", other),
        }
    }

    url.set_query(None);

    println!("{}", fs_safe_url(&url));

    unimplemented!()
}

fn fs_safe_url(url: &Url) -> String {
    assert_eq!(None, url.query());
    url.as_str()
        .replace(|c: char| !c.is_ascii_alphabetic(), "_")
}

#[cfg(test)]
mod tests {
    #[test]
    fn fs_safe() {
        assert_eq!(
            "git___sigrok_org_libsigrok",
            super::fs_safe_url(&"git://sigrok.org/libsigrok".parse().unwrap())
        )
    }
}
