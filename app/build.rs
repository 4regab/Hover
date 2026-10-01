fn main() {
    slint_build::compile("ui/app.slint").expect("ui/app.slint compiles");
    // The exe's icon (hover.rc). Nothing on other targets; the icon is cosmetic, so a
    // missing resource compiler doesn't fail the build.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=hover.rc");
        println!("cargo:rerun-if-changed=assets/hover.ico");
        embed_resource::compile_for("hover.rc", ["hoverai"], embed_resource::NONE).manifest_optional().unwrap();
    }
}
