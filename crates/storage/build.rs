fn main() -> Result<(), Box<dyn std::error::Error>> {
    link_static_duckdb();
    // Generate a client for jaeger.storage.v2.TraceReader. The OTLP types it
    // references are mapped onto the opentelemetry-proto crate's generated
    // types so TracesData flows straight between the two APIs.
    tonic_prost_build::configure()
        .build_server(true)
        .build_client(true)
        .extern_path(
            ".opentelemetry.proto.trace.v1",
            "::opentelemetry_proto::tonic::trace::v1",
        )
        .extern_path(
            ".opentelemetry.proto.logs.v1",
            "::opentelemetry_proto::tonic::logs::v1",
        )
        .extern_path(
            ".opentelemetry.proto.metrics.v1",
            "::opentelemetry_proto::tonic::metrics::v1",
        )
        .extern_path(
            ".opentelemetry.proto.common.v1",
            "::opentelemetry_proto::tonic::common::v1",
        )
        .extern_path(
            ".opentelemetry.proto.resource.v1",
            "::opentelemetry_proto::tonic::resource::v1",
        )
        .compile_protos(
            &[
                "proto/jaeger/storage/v2/trace_storage.proto",
                "proto/otelview/storage/v1/storage.proto",
            ],
            &["proto", "proto/gnostic"],
        )?;
    Ok(())
}

/// Link the extra archives from DuckDB's pre-built static bundle
/// (scripts/fetch-duckdb.sh downloads it and points DUCKDB_LIB_DIR at it).
/// libduckdb-sys links libduckdb_static itself (DUCKDB_STATIC=1); the bundle's
/// extension and third-party archives plus the C++ runtime are linked here.
fn link_static_duckdb() {
    println!("cargo:rerun-if-env-changed=DUCKDB_LIB_DIR");
    println!("cargo:rerun-if-env-changed=DUCKDB_STATIC");
    let is_static = std::env::var("DUCKDB_STATIC")
        .map(|v| v != "0")
        .unwrap_or(false);
    let Ok(dir) = std::env::var("DUCKDB_LIB_DIR") else {
        return;
    };
    if !is_static {
        return;
    }
    println!("cargo:rerun-if-changed={dir}");
    println!("cargo:rustc-link-search=native={dir}");
    // Re-list duckdb_static first so single-pass linkers (GNU ld) resolve the
    // extension archives that follow it; libduckdb-sys lists it again last,
    // closing the cycle in the other direction.
    println!("cargo:rustc-link-lib=static=duckdb_static");
    let mut extras: Vec<String> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.strip_prefix("lib")
                .and_then(|n| n.strip_suffix(".a"))
                .filter(|n| *n != "duckdb_static")
                .map(str::to_string)
        })
        .collect();
    extras.sort();
    for lib in extras {
        println!("cargo:rustc-link-lib=static={lib}");
    }
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    match target_os.as_str() {
        "macos" => println!("cargo:rustc-link-lib=dylib=c++"),
        "linux" => println!("cargo:rustc-link-lib=dylib=stdc++"),
        _ => {}
    }
}
