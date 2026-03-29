fn main() {
    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .include("src/re_unplayplay")
        .file("src/re_unplayplay/decrypt_key.cpp")
        .file("src/re_unplayplay/bind_key.cpp")
        .file("src/re_unplayplay/wrapper.cpp")
        .flag_if_supported("-O2")
        .flag_if_supported("-w")  // suppress warnings from decompiled code
        .compile("re_unplayplay");
    println!("cargo:rerun-if-changed=src/re_unplayplay/");
}
