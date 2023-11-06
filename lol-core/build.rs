fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("DOCS_RS").is_ok() {
        return Ok(());
    }

    let mut config = prost_build::Config::new();
    config.bytes(&[
        ".lol_core.AppendStreamEntry.command",
        ".lol_core.GetSnapshotRep.chunk",
    ]);
    // Output the generated rs files to `src/proto/`
    tonic_build::configure()
        .out_dir("src/proto/")
        .protoc_arg("--experimental_allow_proto3_optional")
        .compile_with_config(config, &["lol_core.proto"], &["proto"])?;

    Ok(())
}
