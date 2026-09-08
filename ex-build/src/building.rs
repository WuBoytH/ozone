use std::path::PathBuf;
use thiserror::Error;
use walkdir::WalkDir;

use crate::env;

#[derive(Error, Debug)]
pub enum BuildError {
    #[error("The 'source' directory for exlaunch is missing")]
    ExlaunchMissingSource,

    #[error("The default PIC library directory is missing")]
    MissingDefaultLibPath,

    #[error("The GCC library directory is missing")]
    MissingGCCLibPath,

    #[error("{0:?}")]
    Environment(#[from] env::EnvironmentError),
}

fn exlaunch_source_path() -> Result<PathBuf, BuildError> {
    let exlaunch_source = env::exlaunch_root_path().map(|root| root.join("source"))?;

    if exlaunch_source.exists() {
        Ok(exlaunch_source)
    } else {
        Err(BuildError::ExlaunchMissingSource)
    }
}

#[derive(Debug)]
pub struct BuildPaths {
    pub c_source_files: Vec<PathBuf>,
    pub source_files: Vec<PathBuf>,
    pub include_files: Vec<PathBuf>,
    pub include_directories: Vec<PathBuf>,
    pub link_directories: Vec<PathBuf>,
}

impl BuildPaths {
    fn discover_files_by_extensions(extensions: &[&str]) -> Result<Vec<PathBuf>, BuildError> {
        let exlaunch_source = exlaunch_source_path()?;

        let mut files = vec![];

        for entry in WalkDir::new(exlaunch_source) {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    eprintln!(
                        "[ex-build::building] Failed to retrieve directory entry, skipping: {:?}",
                        e
                    );
                    continue;
                }
            };

            if !entry.file_type().is_file() {
                continue;
            }

            let extension = match entry.path().extension().map(|ext| ext.to_str()).flatten() {
                Some(ext) => ext,
                None => {
                    eprintln!(
                        "[ex-build::building] File path '{}' does not have an extension, skipping.",
                        entry.path().display()
                    );
                    continue;
                }
            };

            if extensions.contains(&extension) {
                files.push(entry.path().to_path_buf());
            }
        }

        Ok(files)
    }

    fn discover_source_files() -> Result<Vec<PathBuf>, BuildError> {
        static SOURCE_EXTENSIONS: &[&str] = &["cpp", "cc", "s"];

        let source_files = Self::discover_files_by_extensions(SOURCE_EXTENSIONS)?;
        Ok(source_files
            .into_iter()
            .filter_map(|path| {
                if path
                    .file_name()
                    .map(|name| name.to_str())
                    .flatten()
                    .map(|name| name == "main.cpp" || name == "crt0.s")
                    .unwrap_or(false)
                {
                    None
                } else {
                    Some(path)
                }
            })
            .collect())
    }

    fn discover_c_source_files() -> Result<Vec<PathBuf>, BuildError> {
        static SOURCE_EXTENSIONS: &[&str] = &["c"];

        Self::discover_files_by_extensions(SOURCE_EXTENSIONS)
    }

    fn discover_header_files() -> Result<Vec<PathBuf>, BuildError> {
        static HEADER_EXTENSIONS: &[&str] = &["h", "hpp"];

        Self::discover_files_by_extensions(HEADER_EXTENSIONS)
    }

    fn discover_include_directories() -> Result<Vec<PathBuf>, BuildError> {
        let exlaunch_source = exlaunch_source_path()?;

        let mut dirs = vec![];

        for entry in WalkDir::new(&exlaunch_source).min_depth(1).max_depth(1) {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    eprintln!(
                        "[ex-build::building] Failed to retrieve directory entry, skipping: {:?}",
                        e
                    );
                    continue;
                }
            };

            if !entry.file_type().is_dir() {
                continue;
            }

            dirs.push(entry.path().to_path_buf());
        }

        dirs.push(exlaunch_source);

        Ok(dirs)
    }

    fn discover_link_directories() -> Result<Vec<PathBuf>, BuildError> {
        let standard_pic_root = env::devkita64_path()?.join("aarch64-none-elf/lib/pic");

        if !standard_pic_root.exists() {
            return Err(BuildError::MissingDefaultLibPath);
        }

        let gcc_lib_path = env::devkita64_path()?.join("lib/gcc/aarch64-none-elf");
        if !gcc_lib_path.exists() {
            return Err(BuildError::MissingGCCLibPath);
        }

        let mut gcc_lib_root = None;

        for entry in WalkDir::new(gcc_lib_path).min_depth(1).max_depth(1) {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    eprintln!(
                        "[ex-build::building] Failed to retrieve directory entry, skipping: {:?}",
                        e
                    );
                    continue;
                }
            };

            if !entry.file_type().is_dir() {
                continue;
            }

            let root = entry.path().join("pic");
            if !root.exists() {
                continue;
            }

            gcc_lib_root = Some(root);
            break;
        }

        gcc_lib_root
            .ok_or(BuildError::MissingGCCLibPath)
            .map(|gcc_lib_root| vec![standard_pic_root, gcc_lib_root])
    }

    pub fn new() -> Result<Self, BuildError> {
        Ok(Self {
            c_source_files: Self::discover_c_source_files()?,
            source_files: Self::discover_source_files()?,
            include_files: Self::discover_header_files()?,
            include_directories: Self::discover_include_directories()?,
            link_directories: Self::discover_link_directories()?,
        })
    }
}
