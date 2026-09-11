use skyline::nn::{self, ro::RegistrationInfo};

use nn::ro::{NrrHeader, Module};
use std::sync::Mutex;
use thiserror::Error;

/// (name, module_base, module_base + image size) for every plugin mounted by ozone.
/// Backs the `get_plugin_addresses` export consumed by skyline-rs and crash symbolisation.
static LOADED_PLUGINS: Mutex<Vec<(String, usize, usize)>> = Mutex::new(Vec::new());

/// Returns the (start, end) range of the mounted plugin that contains `addr`, if any.
pub fn containing_plugin(addr: usize) -> Option<(usize, usize)> {
    LOADED_PLUGINS
        .lock()
        .unwrap()
        .iter()
        .find(|(_, start, end)| *start <= addr && addr < *end)
        .map(|(_, start, end)| (*start, *end))
}

/// Returns the plugin name and offset for `addr`. Uses `try_lock` so it is safe to
/// call from the exception handler even if the fault happened while mounting.
pub fn plugin_for_address(addr: usize) -> Option<(String, usize)> {
    let plugins = LOADED_PLUGINS.try_lock().ok()?;
    plugins
        .iter()
        .find(|(_, start, end)| *start <= addr && addr < *end)
        .map(|(name, start, _)| (name.clone(), addr - *start))
}

macro_rules! align_up {
    ($x:expr, $a:expr) => {
        ((($x) + (($a) - 1)) & !(($a) - 1))
    };
}

#[derive(Error, Debug)]
pub enum LoaderError {
    #[error("{0}")]
    IO(#[from] std::io::Error),

    #[error("Error registering modules: {0:#x}")]
    RegistrationError(u32),

    #[error("Error mounting module: {0:#x}")]
    MountError(u32),

    #[error("Error retrieving buffer size: {0:#x}")]
    InvalidModuleBuffer(u32),

    #[error("NRO file is empty")]
    EmptyImage,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Sha256Hash([u8; 0x20]);

impl Sha256Hash {
    pub fn new(data: &[u8]) -> Self {
        let mut hash = [0u8; 0x20];
        unsafe {
            nn::crypto::GenerateSha256Hash(hash.as_mut_ptr() as _, 0x20, data.as_ptr() as _, data.len() as u64);
        }
        Self(hash)
    }
}

/// A page-aligned, heap-allocated buffer. `nn::ro::LoadModule` requires the NRO image to be
/// 0x1000-aligned, so the file is read straight into one of these and mapped in place: no
/// intermediate `Vec` copies.
pub struct AlignedImage {
    ptr: std::ptr::NonNull<u8>,
    layout: std::alloc::Layout,
}

impl AlignedImage {
    const ALIGN: usize = 0x1000;

    fn new(size: usize) -> Result<Self, LoaderError> {
        if size == 0 {
            return Err(LoaderError::EmptyImage);
        }
        let layout = std::alloc::Layout::from_size_align(size, Self::ALIGN).map_err(|_| LoaderError::EmptyImage)?;
        // SAFETY: layout has a non-zero size.
        let ptr = std::ptr::NonNull::new(unsafe { std::alloc::alloc(layout) })
            .unwrap_or_else(|| std::alloc::handle_alloc_error(layout));
        Ok(Self { ptr, layout })
    }

    /// Gives up ownership; the memory is never freed (the module stays mapped for the rest of
    /// the process).
    fn into_raw(self) -> *mut u8 {
        let ptr = self.ptr.as_ptr();
        std::mem::forget(self);
        ptr
    }
}

impl std::ops::Deref for AlignedImage {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        // SAFETY: `ptr` points to `layout.size()` initialised bytes (filled by `NroFile::open`).
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.layout.size()) }
    }
}

impl std::ops::DerefMut for AlignedImage {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.layout.size()) }
    }
}

impl Drop for AlignedImage {
    fn drop(&mut self) {
        unsafe { std::alloc::dealloc(self.ptr.as_ptr(), self.layout) }
    }
}

pub struct NroFile {
    image: AlignedImage,
    name: String,
}

fn u32_at(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(b[off..off + 4].try_into().unwrap())
}

fn nro_module_name(nro: &[u8]) -> Option<String> {
    if nro.get(0x10..0x14)? != b"NRO0" {
        return None;
    }
    // segment headers start at 0x20: text (0x20), rodata (0x28), data (0x30)
    let ro_off = u32_at(nro, 0x28) as usize;
    let ro_size = u32_at(nro, 0x2C) as usize;
    let ro = nro.get(ro_off..ro_off + ro_size)?;

    // .nx-module-name: u32 unk, u32 len, u8[len]
    let len = u32_at(ro.get(..8)?, 4) as usize;
    let name = ro.get(8..8 + len)?;
    let name = name.split(|&c| c == 0).next()?; // drop trailing NUL if present
    Some(String::from_utf8_lossy(name).into_owned())
}

impl NroFile {
    /// Reads the NRO at `path` directly into its final, page-aligned buffer.
    pub fn open<P: AsRef<std::path::Path>>(path: P) -> Result<Self, LoaderError> {
        use std::io::Read as _;

        let path = path.as_ref();
        let mut file = std::fs::File::open(path)?;
        let size = file.metadata()?.len() as usize;

        let mut image = AlignedImage::new(size)?;
        file.read_exact(&mut image)?;

        let name = nro_module_name(&image)
            .unwrap_or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "plugin".to_string()));

        Ok(Self { image, name })
    }

    pub fn fix_bss_size(&mut self) {
        unsafe {
            // Get the mod header offset
            let mod_header_offset = *(self.image.as_ptr().add(4) as *const u32);

            let mod_header = self.image.as_mut_ptr().add(mod_header_offset as usize + 0x18) as *mut u32;

            let bss_end_offset = *mod_header.add(3);
            let module_object_offset = *mod_header.add(7);

            if bss_end_offset == module_object_offset {
                *mod_header.add(3) += 0xD0;
            }
        }
    }

    pub fn hash(&self) -> Sha256Hash {
        Sha256Hash::new(&self.image)
    }

    /// Loads the NRO. The returned `Module` is leaked on purpose: nn::ro keeps referring to it
    /// (and to the NRR `RegistrationInfo`) after the call, so both must stay at a fixed address
    /// for the rest of the process. Returning them by value and dropping them later leaves nn::ro
    /// with dangling list nodes that it writes through on the next module (un)load, which
    /// corrupted a saved return address on the main thread.
    ///
    /// The image buffer is mapped in place and leaked with the module.
    pub fn mount(self) -> Result<&'static mut Module, LoaderError> {
        use std::alloc;

        let Self { image, name } = self;

        let image_size = image.len();

        let bss_size = unsafe {
            let mut size = 0;
            let rc = nn::ro::GetBufferSize(&mut size, image.as_ptr() as _);
            if rc != 0 {
                // `image` is freed on return.
                return Err(LoaderError::InvalidModuleBuffer(rc));
            }
            size as usize
        };

        let bss_layout = alloc::Layout::from_size_align(bss_size, 0x1000).unwrap();

        let bss_memory = unsafe {
            alloc::alloc(bss_layout)
        };

        let module: &'static mut Module = Box::leak(Box::new(unsafe { std::mem::MaybeUninit::zeroed().assume_init() }));

        unsafe {
            let name_len = name.len().min(module.Name.len() - 1);
            module.Name[0..name_len].copy_from_slice(&name.as_bytes()[..name_len]);

            let image_ptr = image.as_ptr();
            let rc = nn::ro::LoadModule(
                module,
                image_ptr as _,
                bss_memory as _,
                bss_size as u64,
                nn::ro::BindFlag_BindFlag_Lazy as i32
            );

            if rc != 0 {
                drop(image);
                alloc::dealloc(bss_memory, bss_layout);
                drop(Box::from_raw(module));

                Err(LoaderError::MountError(rc))
            } else {
                let base = (*module.ModuleObject).module_base as usize;
                LOADED_PLUGINS.lock().unwrap().push((name, base, base + image_size));
                image.into_raw();
                Ok(module)
            }
        }
    }
}


pub struct MountInfo {
    /// Leaked on purpose, see [`NroFile::mount`].
    pub modules: Vec<Result<&'static mut Module, LoaderError>>,
    /// Leaked on purpose, see [`NroFile::mount`].
    pub registration_info: &'static mut RegistrationInfo,
}

pub struct NrrBuilder(Vec<Sha256Hash>);

impl NrrBuilder {
    pub fn new() -> Self {
        Self(
            Vec::new()
        )
    }
    pub fn register_nro(&mut self, nro: &NroFile) {
        let hash = nro.hash();

        self.0.push(hash);
    }

    pub fn build<'a>(mut self) -> &'static mut NrrHeader {
        // Sort and then remove duplicate hashes (unallowed) to work with the proper image size
        self.0.sort();
        self.0.dedup();

        let image_size = align_up!(std::mem::size_of::<nn::ro::NrrHeader>() + self.0.len() * std::mem::size_of::<Sha256Hash>(), 0x1000);

        let (header, hashes) = unsafe {
            let layout = std::alloc::Layout::from_size_align(image_size, 0x1000).unwrap();
            let memory = std::alloc::alloc_zeroed(layout);
            (
                &mut *(memory as *mut NrrHeader),
                std::slice::from_raw_parts_mut(
                    memory.add(std::mem::size_of::<NrrHeader>()) as *mut Sha256Hash,
                    self.0.len()
                )
            )
        };

        hashes.copy_from_slice(&self.0);

        header.magic = 0x3052524E;
        header.program_id = nn::ro::ProgramId { value: horizon_svc::get_program_id() };
        header.size = image_size as u32;
        header.type_ = 0;
        header.hashes_offset = std::mem::size_of::<NrrHeader>() as u32;
        header.num_hashes = self.0.len() as u32;

        header
    }
}

pub fn mount_plugins(plugins: impl Iterator<Item = NroFile>) -> Result<MountInfo, LoaderError> {
    use std::alloc;

    let plugins: Vec<NroFile> = plugins.collect();

    // Handle creating the raw NRR image
    let registration_info = {
        let mut nrr = NrrBuilder::new();

        for file in &plugins {
            nrr.register_nro(&file);
        }

        let header = nrr.build();

        // nn::ro links the RegistrationInfo into its registration list, so it must never move
        // or be freed (see `NroFile::mount`).
        let nrr_info: &'static mut std::mem::MaybeUninit<RegistrationInfo> = Box::leak(Box::new(std::mem::MaybeUninit::uninit()));
        unsafe {
            let rc = nn::ro::RegisterModuleInfo(nrr_info.as_mut_ptr(), header as *mut NrrHeader as _);
            if rc != 0 {
                let layout = alloc::Layout::from_size_align(header.size as _, 0x1000).unwrap();
                alloc::dealloc(header as *mut NrrHeader as _, layout);
                drop(Box::from_raw(nrr_info));
                return Err(LoaderError::RegistrationError(rc));
            }
            nrr_info.assume_init_mut()
        }
    };

    let modules = plugins
        .into_iter()
        .map(NroFile::mount)
        .collect();


    Ok(MountInfo {
        modules,
        registration_info
    })
}
