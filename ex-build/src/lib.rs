/// Provides methods for building using devkitpro
pub mod building;

/// Provides methods for getting required paths from the environment
pub mod env;

pub fn compile() {
    let build_paths = building::BuildPaths::new().unwrap();

    let building::BuildPaths {
        c_source_files,
        source_files,
        include_files,
        include_directories,
        link_directories,
    } = build_paths;

    for file in include_files {
        println!("cargo:rerun-if-changed={}", file.display());
    }

    for file in &source_files {
        println!("cargo:rerun-if-changed={}", file.display());
    }

    for directory in link_directories {
        println!("cargo:rustc-link-search={}", directory.display());
    }

    println!("cargo:rustc-link-lib=static=stdc++");
    println!("cargo:rustc-link-lib=static=gcc");
    println!("cargo:rustc-cdylib-link-args=--shared --export-dynamic --gc-sections --build-id=sha1 --nx-module-name -init=exl_module_init --exclude-libs=ALLb");
    // println!(
    //     "cargo:rustc-link-arg=-T{}/link.ld",
    //     env::exlaunch_root_path().unwrap().join("misc").display()
    // );
    cc::Build::new()
        .compiler(env::gcc_compiler_path().unwrap())
        .archiver(env::ar_compiler_path().unwrap())
        .cpp(false)
        .shared_flag(true)
        .static_flag(true)
        .no_default_flags(true)
        .warnings(false)
        .define("__SWITCH__", None)
        .define("__RTLD_6XX__", None)
        .define("EXL_LOAD_KIND", "Module")
        .define("EXL_LOAD_KIND_ENUM", "2")
        .define("EXL_PROGRAM_ID", "0x01006a800016e000")
        .flag("-g")
        .flag("-Wall")
        .flag("-Werror")
        .flag("-O3")
        .flag("-fdata-sections")
        .flag("-ffunction-sections")
        .flag("-fPIC")
        .flag("-fno-asynchronous-unwind-tables")
        .flag("-fno-exceptions")
        .flag("-fno-unwind-tables")
        .files(c_source_files)
        .includes(&include_directories)
        .compile("ozone-c");

    cc::Build::new()
        .compiler(env::gpp_compiler_path().unwrap())
        .archiver(env::ar_compiler_path().unwrap())
        .cpp(true)
        .shared_flag(true)
        .static_flag(true)
        .no_default_flags(true)
        .warnings(false)
        .define("__SWITCH__", None)
        .define("__RTLD_6XX__", None)
        .define("EXL_LOAD_KIND", "Module")
        .define("EXL_LOAD_KIND_ENUM", "2")
        .define("EXL_PROGRAM_ID", "0x01006a800016e000")
        .flag("-g")
        .flag("-Wall")
        .flag("-Werror")
        .flag("-O3")
        .flag("-std=gnu++20")
        .flag("-fdata-sections")
        .flag("-ffunction-sections")
        .flag("-fPIC")
        .flag("-fno-asynchronous-unwind-tables")
        .flag("-fno-exceptions")
        .flag("-fno-rtti")
        .flag("-fno-unwind-tables")
        .files(source_files)
        .includes(include_directories)
        .compile("ozone");
}

pub fn copy_link_script() {
    let script_path = env::exlaunch_root_path().unwrap().join("misc/link.ld");

    if !script_path.exists() {
        panic!("[ex-build] The linker script does not exist in the expected location!");
    }

    let target_path = env::cargo_home_path().unwrap().join("skyline/link.T");

    let _ = std::fs::copy(&target_path, target_path.with_extension("T.bak"));

    let _ = std::fs::copy(script_path, target_path);
}

pub fn restore_link_script() {
    let target_path = env::cargo_home_path().unwrap().join("skyline/link.T.bak");
    let _ = std::fs::copy(&target_path, target_path.with_extension("T"));
}
