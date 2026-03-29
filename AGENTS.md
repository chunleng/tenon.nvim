# Omnidash

Omnidash is a neovim plugin written in purely Rust, using `nvim-oxi`, a safe,
idiomatic Rust bindings to the Neovim text editor's API. The plugin is an
Agentic chat tool that can respond to request and call relevant tools to do so.

## Development Guideline

After changing rust code, remember to ensure to check the following:

1. `cargo build` to make sure things are not breaking
2. `cargo fmt` to make sure code are formatted properly
