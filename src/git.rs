use std::fs;
use std::path::PathBuf;

use failure::Error;
use git2;
use url::Url;

#[derive(Copy, Clone, Debug)]
enum GitSpecifier {
    Hash(git2::Oid),
}

pub struct LocalRepo {
    specifier: GitSpecifier,
    path: PathBuf,
}

impl GitSpecifier {
    fn find_in(self, repo: &git2::Repository) -> Result<bool, Error> {
        match self {
            GitSpecifier::Hash(oid) => if let Err(e) = repo.find_commit(oid) {
                match e.code() {
                    git2::ErrorCode::NotFound => Ok(false),
                    _ => bail!(e),
                }
            } else {
                Ok(true)
            },
        }
    }
}

pub fn check_cloned<S: AsRef<str>>(url: S) -> Result<LocalRepo, Error> {
    let mut url = Url::parse(url.as_ref())?;
    let specifier = {
        let (mode, value) = {
            let mut args = url.query_pairs();
            ensure!(1 == args.count(), "there must be only one arg");
            args.next().unwrap()
        };

        match mode.as_ref() {
            "rev" => GitSpecifier::Hash(value.parse()?),
            other => bail!("unsupported mode: {}", other),
        }
    };

    url.set_query(None);

    let (repo, path) = check_single(url, specifier)?;

    for submodule in repo.submodules()? {
        // TODO: not clear this index_id actually returns anything useful
        if let Some(oid) = submodule.index_id() {
            check_single(
                submodule
                    .url()
                    .ok_or(format_err!(
                        "invalid submodule utf-8: {:?}",
                        String::from_utf8_lossy(submodule.url_bytes())
                    ))?
                    .parse()?,
                GitSpecifier::Hash(oid),
            )?;
        }
    }

    Ok(LocalRepo { path, specifier })
}

fn check_single(url: Url, specifier: GitSpecifier) -> Result<(git2::Repository, PathBuf), Error> {
    let mut path = PathBuf::from(".cache");
    path.push(fs_safe_url(&url));

    let repo = if !path.is_dir() {
        fs::create_dir_all(&path)?;
        git2::Repository::init_bare(&path)?
    } else {
        git2::Repository::open_bare(&path)?
    };

    if !specifier.find_in(&repo)? {
        // empty fetch array here undocumented, but seems to work?
        repo.remote("upstream", url.as_str())?
            .fetch(&[], None, None)?;

        ensure!(
            specifier.find_in(&repo)?,
            "fetching didn't help {:?} appear",
            specifier
        );
    }

    Ok((repo, path))
}

fn fs_safe_url(url: &Url) -> String {
    assert_eq!(None, url.query());
    url.as_str().replace(|c: char| !c.is_alphanumeric(), "_")
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
