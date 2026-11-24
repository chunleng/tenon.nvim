fn main() -> Result<(), nvim_oxi::tests::BuildError> {
    if std::env::var_os("CARGO_FEATURE_NVIM_TESTS").is_some() {
        nvim_oxi::tests::build()?;
    }
    Ok(())
}
