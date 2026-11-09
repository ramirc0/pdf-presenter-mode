# List recipes.
default:
    @just --list

# Present a deck.
run deck:
    cargo run -- {{deck}}

# Run the tests.
test:
    cargo test

# Check formatting, lints and docs.
lint:
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps

# Rebuild the fixture PDFs from their Typst sources.
fixtures:
    for f in fixtures/*.typ; do typst compile --root fixtures "$f"; done
    head -c 4096 fixtures/basic.pdf > fixtures/truncated.pdf

# Run the GUI tests in a headless sway.
gui-test:
    cargo build
    tests/gui.sh target/debug/ppm
