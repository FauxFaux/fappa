use std::fs;
use std::path::PathBuf;

use failure::Error;
use failure::ResultExt;
use git2;
use url::Url;

#[derive(Copy, Clone, Debug)]
pub enum GitSpecifier {
    Hash(git2::Oid),
}

pub struct LocalRepo {
    pub specifier: GitSpecifier,
    pub path: String,
}

impl GitSpecifier {
    fn find_in(self, repo: &git2::Repository) -> Result<bool, Error> {
        match self {
            GitSpecifier::Hash(oid) => {
                if let Err(e) = repo.find_commit(oid) {
                    match e.code() {
                        git2::ErrorCode::NotFound => Ok(false),
                        _ => bail!(e),
                    }
                } else {
                    Ok(true)
                }
            }
        }
    }

    pub fn git_args(self) -> String {
        match self {
            GitSpecifier::Hash(oid) => format!("checkout -B build {}", oid),
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

    let (repo, path) = check_single(&url, specifier)
        .with_context(|_| format_err!("checking repo {} {:?}", url, specifier))?;

    // fails 'cos we don't have a working tree, right
    if false {
        for submodule in repo.submodules()? {
            // TODO: not clear this index_id actually returns anything useful
            if let Some(oid) = submodule.index_id() {
                check_single(
                    &submodule
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
    }

    Ok(LocalRepo { path, specifier })
}

fn check_single(url: &Url, specifier: GitSpecifier) -> Result<(git2::Repository, String), Error> {
    let mut path = PathBuf::from(".cache");
    let safe_url = fs_safe_url(&url);
    path.push(&safe_url);

    let repo = if !path.is_dir() {
        fs::create_dir_all(&path).with_context(|_| format_err!("creating repo dir in cache"))?;
        git2::Repository::init_bare(&path)
            .with_context(|_| format_err!("initialising cache repository"))?
    } else {
        git2::Repository::open_bare(&path).with_context(|_| format_err!("opening cache repo"))?
    };

    if !specifier.find_in(&repo)? {
        let remote_name = "upstream";

        // if it's already there, just blow it away; easier than checking the URL is right etc.
        // not too horribly inefficient as we're only here 'cos we're going to download anyway
        if repo.find_remote(remote_name).is_ok() {
            repo.remote_delete(remote_name)?;
        }

        // empty fetch array here undocumented, but seems to work?
        repo.remote(remote_name, url.as_str())?
            .fetch(&[], None, None)?;

        ensure!(
            specifier.find_in(&repo)?,
            "fetching didn't help {:?} appear",
            specifier
        );
    }

    Ok((repo, safe_url))
}

pub fn fs_safe_url(url: &Url) -> String {
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
