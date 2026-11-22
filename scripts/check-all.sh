#!/usr/bin/env bash
# The local mirror of CI. Run it before pushing a branch: nothing runs it for
# you, and the forge runs the same commands after the push rather than before.
#
# The first seven commands are .forgejo/workflows/ci.yml's, in the same order
# with the same flags, and the last is .forgejo/workflows/advisories.yml's
# check. When a command changes there, change it here, or the two gates drift
# and a branch that is green here goes red on the forge.
#
# One step of ci.yml is not here: the check at the declared rust-version,
# which installs a second toolchain.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --no-default-features --features bundled -- -D warnings
RUSTDOCFLAGS="-D rustdoc::broken_intra_doc_links" cargo doc --workspace --no-deps --all-features
cargo test --workspace --all-features
cargo test --workspace --no-default-features --features bundled
cargo deny --locked check advisories
