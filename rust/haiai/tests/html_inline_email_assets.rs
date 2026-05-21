#[test]
fn hai_jacs_logo_asset_is_bundled_png() {
    let logo = include_bytes!("../assets/hai-jacs-logo.png");

    assert!(logo.starts_with(b"\x89PNG\r\n\x1a\n"));
}
