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
        // Windows Installer compares the fixed numeric file version, not the
        // display string. Preserve the custom revision so -4 and -5 differ.
        let revision = std::env::var("CARGO_PKG_VERSION_PRE").unwrap();
        let revision = if revision.is_empty() {
            0
        } else {
            revision
                .parse::<u16>()
                .expect("Windows custom revision must be an integer from 0 to 65535")
        };
        let definitions = [
            format!("HUB_VERSION_STRING=\"{version}\""),
            format!("HUB_VERSION_NUMBERS={version_numbers},{revision}"),
        ];
        embed_resource::compile("packaging/windows/TianCaiSpaceHub.rc", &definitions)
            .manifest_optional()
            .unwrap();
    }
}
