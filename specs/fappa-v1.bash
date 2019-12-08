# usage: url branch sha dest
# e.g. https://github.com/foo/bar master aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d .
# `branch` is required because it allows reaching non-default-refspec commits
git-export() {
  URL="$1"
  BRANCH="$2"
  SHA="$3"
  DEST=$(readlink -f "$4")
  (
    _cdt
    git clone --single-branch --branch "${BRANCH}" -- "${URL}" foo
    (
      cd foo || exit 7
      git checkout -- "${SHA}"
      rm -rf .git
      mv -- * .* "${DEST}"
    )
    rm -rf .
  )
}

_cdt() {
  D="$(mktemp -d .cdt-XXXXXX)"
  cd "$D" || exit 7
}

package() {
  echo "${INCLUDE}"
}
