use cfg_aliases::cfg_aliases;
use prost_wkt_build::{FileDescriptorSet, Message as _};
use std::{env, path::PathBuf};

fn workdir() -> Option<String> {
    if let Ok(cargo_manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        return Some(cargo_manifest_dir);
    }

    let dest = std::env::var("OUT_DIR");
    if dest.is_err() {
        return None;
    }
    let dest = dest.unwrap();

    let seperator = regex::Regex::new(r"(/target/(.+?)/build/)|(\\target\\(.+?)\\build\\)")
        .expect("Invalid regex");
    let parts = seperator.split(dest.as_str()).collect::<Vec<_>>();

    if parts.len() >= 2 {
        return Some(parts[0].to_string());
    }

    None
}

fn check_locale() {
    let workdir = workdir().unwrap_or("./".to_string());

    let locale_path = format!("{workdir}/**/locales/**/*");
    if let Ok(globs) = globwalk::glob(locale_path) {
        for entry in globs {
            if let Err(e) = entry {
                println!("cargo:i18n-error={e}");
                continue;
            }

            let entry = entry.unwrap().into_path();
            println!("cargo:rerun-if-changed={}", entry.display());
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    cfg_aliases! {
        mobile: {
            any(
                target_os = "android",
                target_os = "ios",
                all(target_os = "macos", feature = "macos-ne"),
                target_env = "ohos"
            )
        }
    }

    // The build uses PROTOC or protoc on PATH. Never download a compiler.
    println!("cargo:rerun-if-env-changed=PROTOC");

    let proto_files_reflect = ["src/proto/peer_rpc.proto", "src/proto/common.proto"];

    let proto_files = [
        "src/proto/error.proto",
        "src/proto/tests.proto",
        "src/proto/api_instance.proto",
        "src/proto/api_logger.proto",
        "src/proto/api_config.proto",
        "src/proto/api_manage.proto",
        "src/proto/web.proto",
        "src/proto/magic_dns.proto",
        "src/proto/acl.proto",
    ];

    for proto_file in proto_files.iter().chain(proto_files_reflect.iter()) {
        println!("cargo:rerun-if-changed={proto_file}");
    }

    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let descriptor_file = out.join("descriptors.bin");

    let mut config = prost_build::Config::new();
    config
        .type_attribute(".", "#[derive(serde::Serialize,serde::Deserialize)]")
        .extern_path(".google.protobuf.Any", "::prost_wkt_types::Any")
        .extern_path(".google.protobuf.Timestamp", "::prost_wkt_types::Timestamp")
        .extern_path(".google.protobuf.Value", "::prost_wkt_types::Value")
        .file_descriptor_set_path(&descriptor_file)
        .protoc_arg("--experimental_allow_proto3_optional")
        .type_attribute("peer_rpc.DirectConnectedPeerInfo", "#[derive(Hash)]")
        .type_attribute("peer_rpc.PeerInfoForGlobalMap", "#[derive(Hash)]")
        .type_attribute("peer_rpc.ForeignNetworkRouteInfoKey", "#[derive(Hash, Eq)]")
        .type_attribute(
            "peer_rpc.RouteForeignNetworkSummary.Info",
            "#[derive(Hash, Eq)]",
        )
        .type_attribute("peer_rpc.RouteForeignNetworkSummary", "#[derive(Hash, Eq)]")
        .type_attribute("common.RpcDescriptor", "#[derive(Hash, Eq)]")
        .type_attribute("acl.Acl", "#[serde(default)]")
        .type_attribute("acl.AclV1", "#[serde(default)]")
        .type_attribute("acl.Chain", "#[serde(default)]")
        .type_attribute("acl.Rule", "#[serde(default)]")
        .type_attribute("acl.GroupInfo", "#[serde(default)]")
        .field_attribute(".api.manage.NetworkConfig", "#[serde(default)]")
        .service_generator(Box::new(easytier_rpc_build::ServiceGenerator::default()))
        .btree_map(["."])
        .skip_debug([".common.Ipv4Addr", ".common.Ipv6Addr", ".common.UUID"]);

    config.compile_protos(&proto_files, &["src/proto/"])?;

    prost_reflect_build::Builder::new()
        .file_descriptor_set_bytes("crate::proto::DESCRIPTOR_POOL_BYTES")
        .compile_protos_with_config(config, &proto_files_reflect, &["src/proto/"])?;

    let descriptor_bytes = std::fs::read(descriptor_file).unwrap();
    let descriptor = FileDescriptorSet::decode(&descriptor_bytes[..]).unwrap();
    prost_wkt_build::add_serde(out, descriptor);

    check_locale();
    Ok(())
}
