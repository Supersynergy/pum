crate := "apps/pum"

_default:
    @just --list

# Build the release binary
setup:
    cd {{crate}} && cargo build --release

# Build (release)
build:
    cd {{crate}} && cargo build --release

# Install `pum` to ~/.cargo/bin
install:
    cargo install --path {{crate}}

# Verify adapters on this host
doctor:
    cd {{crate}} && cargo run -q -- doctor

# Detect and inventory installed packages
scan:
    cd {{crate}} && cargo run -q -- scan

# Check for outdated packages
check:
    cd {{crate}} && cargo run -q -- check

# Print inventory report
report *FLAGS:
    cd {{crate}} && cargo run -q -- report {{FLAGS}}

# Run updates (--dry-run to preview)
update *FLAGS:
    cd {{crate}} && cargo run -q -- update {{FLAGS}}

# Self-update managers (--apply to run)
self *FLAGS:
    cd {{crate}} && cargo run -q -- self {{FLAGS}}

# Run unit tests
test:
    cd {{crate}} && cargo test

# Lint + format check
check-code:
    cd {{crate}} && cargo clippy --all-targets -- -D warnings && cargo fmt --check

# Format
fmt:
    cd {{crate}} && cargo fmt

# Full gate
ci: test check-code
