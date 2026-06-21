fn main() {
    slint_build::compile_with_config(
        "src/gui.slint",
        slint_build::CompilerConfiguration::new()
            .with_style("fluent-dark".into()),
    )
    .expect("Slint build failed");

    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() == "windows" {
        let mut res = winres::WindowsResource::new();
        res.set_icon("icon.ico");
        res.compile().expect("Failed to compile Windows resources");

        // Embed DPI-aware manifest (embed-manifest crate handles test binary conflicts automatically)
        embed_manifest::embed_manifest(embed_manifest::new_manifest(
            include_str!(".windows/app.manifest"),
        ))
        .expect("Failed to embed manifest");
    }
}
