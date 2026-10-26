fn main() {
    println!("cargo:rerun-if-changed=assets/mdbxer.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resources = winres::WindowsResource::new();
        resources.set_icon("assets/mdbxer.ico");
        resources
            .compile()
            .expect("failed to embed the MDBXer application icon");
    }
}
