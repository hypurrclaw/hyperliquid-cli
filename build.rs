fn main() {
    println!("cargo:rerun-if-env-changed=HYPERLIQUID_DEFAULT_BUILDER_ADDRESS");
    println!("cargo:rerun-if-env-changed=HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE");
    println!("cargo:rerun-if-env-changed=HYPERLIQUID_DEFAULT_REFERRAL_CODE");
    println!("cargo:rerun-if-env-changed=HYPERLIQUID_FEEDBACK_URL");

    if let Ok(url) = std::env::var("HYPERLIQUID_FEEDBACK_URL")
        && !url.trim().is_empty()
    {
        println!("cargo:rustc-env=HYPERLIQUID_BUILD_FEEDBACK_URL={url}");
    }
}
