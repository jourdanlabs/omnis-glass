pub fn version_line(product: &str) -> String {
    format!("{} {} ({}, {})", product, env!("CARGO_PKG_VERSION"), env!("GLASS_BUILD_COMMIT"), env!("GLASS_BUILD_DIRTY"))
}
