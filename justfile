# Entry points only. The tasks live in mise.toml; `just` is a five-line
# wrapper so `just check` works for people who reach for it.
set windows-shell := ["pwsh", "-NoLogo", "-NoProfile", "-Command"]

bootstrap:
    mise run bootstrap
build:
    mise run build
test:
    mise run test
check:
    mise run check
