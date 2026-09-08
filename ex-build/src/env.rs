use std::{env, path::PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum EnvironmentError {
    /// If somehow this gets called and the manifest directory isn't set, it's impossible to continue
    #[error("Unable to receive the manifest directory from the cargo environment (are you building without cargo?)")]
    NoManifestDir,

    /// If the manifest does not have a valid parent we can't get the path to exlaunch
    #[error("The manifest directory ({0:?}) does not have a parent directory")]
    NoManifestParentDir(PathBuf),

    /// If exlaunch is not in the folder structure, it's likely that the repository wasn't recursively cloned
    #[error("The folder for exlaunch is missing in the repository, make sure that you recursively cloned")]
    MissingExlaunch,

    /// The exlaunch file is not a directory
    #[error("The path for exlaunch was found but was not a directory")]
    ExlaunchNotDirectory,

    #[error("devkitpro is not installed on this system")]
    DevkitproMissing,

    #[error("devkitA64 is not installed on this system")]
    DevkitA64Missing,

    #[error("The g++ compiler for devkitA64 is missing")]
    DevkitA64CompilerMissing,

    #[error("The file for module specs is missing")]
    MissingModuleSpecs,

    /// Any other IO error that does not have explicit error handling
    #[error("Unhandled IO Error: {0:?}")]
    IO(#[from] std::io::Error),
}

/// Retrieves the root of the repository for skyline-ex
pub fn repository_root_path() -> Result<PathBuf, EnvironmentError> {
    // attempt to get the manifest/project directory (the one that cargo is called in)
    let manifest_dir = manifest_root_path()?;

    // attempt to get the parent of that project directory, as it should be the repository root
    manifest_dir
        .parent()
        .map(|dir| dir.to_path_buf())
        .ok_or(EnvironmentError::NoManifestParentDir(manifest_dir))
}

/// Retrieves the root of the exlaunch folder in the repository
pub fn exlaunch_root_path() -> Result<PathBuf, EnvironmentError> {
    // get the repository root (at any point in building it should be safe to assume that the
    // folder structure is identical to the one on GitHub)
    let repo_root = repository_root_path()?;

    // the git submodule should be found at the root of the repo in the exlaunch folder,
    // perform sanity checks to make sure that it both exists and is not a file
    let exlaunch_root = repo_root.join("exlaunch");
    if !exlaunch_root.exists() {
        return Err(EnvironmentError::MissingExlaunch);
    }

    // try to stat the exlaunch root and make sure that it's a directory
    if !std::fs::metadata(&exlaunch_root)?.is_dir() {
        Err(EnvironmentError::ExlaunchNotDirectory)
    } else {
        Ok(exlaunch_root)
    }
}

pub fn cargo_home_path() -> Result<PathBuf, EnvironmentError> {
    env::var("CARGO_HOME")
        .map_err(|_| EnvironmentError::NoManifestDir)
        .map(PathBuf::from)
}

pub fn manifest_root_path() -> Result<PathBuf, EnvironmentError> {
    env::var("CARGO_MANIFEST_DIR")
        .map_err(|_| EnvironmentError::NoManifestDir)
        .map(|dir| PathBuf::from(dir))
}

pub fn devkitpro_path() -> Result<PathBuf, EnvironmentError> {
    env::var("DEVKITPRO")
        .map_err(|_| EnvironmentError::DevkitproMissing)
        .map(|dir| PathBuf::from(dir))
}

/// Retrieves the path to devkitpro for our needs (in this case it is devkitA64)
pub fn devkita64_path() -> Result<PathBuf, EnvironmentError> {
    let devkitpro_path = devkitpro_path()?;

    // the path for devkitA64 is DEVKITPRO/devkitA64
    let devkita64_path = devkitpro_path.join("devkitA64");

    // if the path for devkitA64 doesn't exist, it means that it isn't installed in the place that we expect
    if devkita64_path.exists() {
        Ok(devkita64_path)
    } else {
        Err(EnvironmentError::DevkitA64Missing)
    }
}

#[cfg(target_os = "windows")]
static GPP_FILE_PATH: &'static str = "bin/aarch64-none-elf-g++.exe";

#[cfg(target_os = "windows")]
static GCC_FILE_PATH: &'static str = "bin/aarch64-none-elf-gcc.exe";

#[cfg(not(target_os = "windows"))]
static GPP_FILE_PATH: &'static str = "bin/aarch64-none-elf-g++";

#[cfg(not(target_os = "windows"))]
static GCC_FILE_PATH: &'static str = "bin/aarch64-none-elf-gcc";

#[cfg(target_os = "windows")]
static AR_FILE_PATH: &'static str = "bin/aarch64-none-elf-ar.exe";

#[cfg(not(target_os = "windows"))]
static AR_FILE_PATH: &'static str = "bin/aarch64-none-elf-ar";

pub fn gpp_compiler_path() -> Result<PathBuf, EnvironmentError> {
    let compiler_path = devkita64_path()?.join(GPP_FILE_PATH);

    if compiler_path.exists() {
        Ok(compiler_path)
    } else {
        Err(EnvironmentError::DevkitA64CompilerMissing)
    }
}

pub fn ar_compiler_path() -> Result<PathBuf, EnvironmentError> {
    let compiler_path = devkita64_path()?.join(AR_FILE_PATH);

    if compiler_path.exists() {
        Ok(compiler_path)
    } else {
        Err(EnvironmentError::DevkitA64CompilerMissing)
    }
}

pub fn gcc_compiler_path() -> Result<PathBuf, EnvironmentError> {
    let compiler_path = devkita64_path()?.join(GCC_FILE_PATH);

    if compiler_path.exists() {
        Ok(compiler_path)
    } else {
        Err(EnvironmentError::DevkitA64CompilerMissing)
    }
}

pub fn switch_specs_path() -> Result<PathBuf, EnvironmentError> {
    let specs_path = exlaunch_root_path()?.join("misc/specs/module.specs");

    if specs_path.exists() {
        Ok(specs_path)
    } else {
        Err(EnvironmentError::MissingModuleSpecs)
    }
}
