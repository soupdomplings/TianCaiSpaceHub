fn main() {
    println!("cargo:rerun-if-changed=assets/tci-hub-icon.png");
    if std::env::var("CARGO_CFG_WINDOWS").is_ok() {
        println!(
            "cargo:rustc-link-arg-bin=TianCaiSpaceHub=/MANIFESTINPUT:packaging/windows/TianCaiSpaceHub.exe.manifest"
        );
        println!("cargo:rustc-link-arg-bin=TianCaiSpaceHub=/MANIFEST:EMBED");
        println!("cargo:rerun-if-changed=packaging/windows/TianCaiSpaceHub.exe.manifest");
        println!("cargo:rerun-if-changed=packaging/windows/TianCaiSpaceHub.rc");
        println!("cargo:rerun-if-changed=packaging/icons/AppIcon.ico");
        let version = std::env::var("CARGO_PKG_VERSION").unwrap();
        let version_numbers = ["MAJOR", "MINOR", "PATCH"]
            .map(|part| std::env::var(format!("CARGO_PKG_VERSION_{part}")).unwrap())
            .join(",");
        let definitions = [
            format!("HUB_VERSION_STRING=\"{version}\""),
            format!("HUB_VERSION_NUMBERS={version_numbers},0"),
        ];
        embed_resource::compile("packaging/windows/TianCaiSpaceHub.rc", &definitions)
            .manifest_optional()
            .unwrap();
    }
}
