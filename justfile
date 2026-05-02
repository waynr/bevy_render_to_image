build:
  cargo build

build-refactor:
  # requires cargo-limit to be installed
  reset
  (cargo lbuild --example basic --color=always 2>&1) | less -R

test:
  cargo test

test-refactor:
  # requires cargo-limit to be installed
  reset
  (cargo ltest --color=always 2>&1) | less -R

watchexec target:
  watchexec \
    -c \
    -e toml,rs \
    -w justfile \
    -w Cargo.toml \
    -w src \
    -w examples \
    --restart \
    just {{target}}

we-build-refactor:
  just watchexec build-refactor

we-build:
  just watchexec build

we-test-refactor:
  just watchexec test-refactor

we-test:
  just watchexec test
