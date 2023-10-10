use std::path::PathBuf;

fn main() {
    build();
    gen_bindings();
}

fn gen_bindings() {
    let bindings = bindgen::Builder::default()
        .header("src/wrapper.h")
        .clang_arg("-I./deps/LGMP/lgmp/include")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks))
        .generate()
        .expect("Unable to generate bindings");

    let out_path = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Unable to write bindings to file");
    println!("cargo:rerun-if-changed=wrapper.h");
}

fn build() {
    let dst = cmake::Config::new("deps/LGMP/lgmp")
        .build_target("lgmp")
        .build();

    println!("cargo:rustc-link-search=native={}/build", dst.display());
    println!("cargo:rustc-link-search=native={}/build", dst.display());
    println!("cargo:rustc-link-lib=static=lgmp");

    println!("cargo:rerun-if-changed=deps/LGMP/lgmp/CMakeLists.txt");
    println!("cargo:rerun-if-changed=deps/LGMP/lgmp/src/client.c");
    println!("cargo:rerun-if-changed=deps/LGMP/lgmp/src/host.c");
    println!("cargo:rerun-if-changed=deps/LGMP/lgmp/src/status.c");
    println!("cargo:rerun-if-changed=deps/LGMP/lgmp/src/headers.h");
    println!("cargo:rerun-if-changed=deps/LGMP/lgmp/src/lgmp.h");
}
