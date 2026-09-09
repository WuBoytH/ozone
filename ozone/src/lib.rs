#![feature(if_let_guard, int_roundings, ptr_sub_ptr, lazy_cell)]

use std::{
    ffi::{c_char, CStr, CString},
    path::{Path, PathBuf}
};

use std::sync::atomic::{
    AtomicBool,
    Ordering::{SeqCst, Relaxed}
};

use log::Log;
use skyline::{hooks::{getRegionAddress, InlineCtx, Region},  nn};
use thiserror::Error;
use utils::env::is_emulator;

use crate::logger::{KernelLogger, TcpLogger};

mod api;
mod bootstrap;
mod loader;
mod logger;
mod old_api;
mod process;
mod utils;

static SOCKET_INIT: AtomicBool = AtomicBool::new(false);

#[skyline::hook(replace = nn::socket::Initialize)]
pub fn socket_initialize_hook(pool: *mut u8, poolSize: usize, allocPoolSize: usize, concurLimit: i32) -> i32 {
    if SOCKET_INIT.load(Relaxed) == false {
        println!("[ozone] nn::socket::Initialize called for the first time");
        SOCKET_INIT.store(true, SeqCst);
        call_original!(pool, poolSize, allocPoolSize, concurLimit)
    } else {
        // Pretend the operation was successful
        println!("[ozone] nn::socket::Initialize dummied out");
        0
    }
}

#[skyline::hook(replace = nn::socket::Initialize_Config)]
pub fn socket_initialize_config_hook(pool: *mut u8) -> i32 {
    if SOCKET_INIT.load(Relaxed) == false {
        println!("[ozone] nn::socket::Initialize (Config) called for the first time");
        SOCKET_INIT.store(true, SeqCst);
        call_original!(pool)
    } else {
        // Pretend the operation was successful
        println!("[ozone] nn::socket::Initialize (Config) dummied out");

        0
    }
}

static RO_INIT: AtomicBool = AtomicBool::new(false);

#[skyline::hook(replace = nn::ro::Initialize)]
pub fn ro_initialize_hook() -> i32 {
    if RO_INIT.load(Relaxed) == false {
        println!("[ozone] nn::ro::Initialize called for the first time");
        RO_INIT.store(true, SeqCst);
        call_original!()
    } else {
        // Pretend the operation was successful
        println!("[ozone] nn::ro::Initialize dummied out");
        0
    }
}

#[skyline::hook(replace = skyline::nn::fs::MountRom)]
pub fn mount_rom_hook(name: *const c_char, buffer: *const u8, buf_size: usize) -> i32 {
    let _ = unsafe { nn::fs::MountSdCardForDebug(skyline::c_str("sd\0")) };

    println!("[ozone] SD card mounted");

    let mount_point = unsafe { CStr::from_ptr(name).to_str() }.unwrap();

    let result = call_original!(name, buffer, buf_size);

    unsafe { nn::ro::Initialize(); }

    if let Ok(read_dir) = std::fs::read_dir(format!("{}:/skyline/plugins/", mount_point)) {
        let files = read_dir
            .into_iter()
            .map(|cock| cock.unwrap())
            .filter(|dir| dir.file_type().unwrap().is_file())
            .map(|idk| {
                idk.path()
            }).collect::<Vec<_>>();

        let nros = files
            .iter()
            .filter(|entry| {
                entry.extension().unwrap().to_str() == Some("nro")
            })
            .map(|entry| {
                println!("Entry: {}", entry.display());
                let file = std::fs::read(&entry).unwrap();
                loader::NroFile::from_slice(&file).unwrap()
            });

        let loader_results = loader::mount_plugins(nros);

        match loader_results {
            Ok(info) => unsafe {
                for module in info.modules.iter() {
                    match module.as_ref() {
                        Ok(plugin) => {
                            let mut symbol = 0usize;
                            nn::ro::LookupModuleSymbol(&mut symbol, plugin, b"main\0".as_ptr());
                            let nul = plugin
                                .Name
                                .iter()
                                .enumerate()
                                .find(|(_, byte)| **byte == 0)
                                .map(|(count, _)| count)
                                .unwrap();

                            let name = std::str::from_utf8_unchecked(&plugin.Name[0..nul]);

                            if symbol == 0 {
                                println!("[ozone] Plugin {} does not have a main function.", name);
                            } else {
                                let main_func: extern "C" fn() = std::mem::transmute(symbol);
                                println!("[ozone] Calling function 'main' in plugin {}", name);
                                main_func();
                                println!("[ozone] Function 'main' has finished in plugin {}", name);
                            }
                        },
                        Err(e) => println!("[ozone] Error mounting module: {}", e),
                    }
                }
            },
            Err(e) => println!("Loader failed: {}", e),
        }

        println!("[ozone] Finished loading all plugins");
    }

    result
}

#[ozone_macro::main]
pub fn main() {
    // Panic handler for Rust panics
    std::panic::set_hook(Box::new(|info| {
        let location = info.location().unwrap();

        let msg = match info.payload().downcast_ref::<&'static str>() {
            Some(s) => *s,
            None => match info.payload().downcast_ref::<String>() {
                Some(s) => &s[..],
                None => "Box<Any>",
            },
        };

        println!(
            "Ozone has panicked at '{}' with the following message: {}",
            location,
            msg
        );

        let err_msg = format!(
            "Ozone has panicked at '{}' with the following message:\n{}\0",
            location,
            msg
        );

        skyline::error::show_error(
            69,
            "Ozone has panicked! Please open the details and send a screenshot to the developer, then close the game.\n\0",
            err_msg.as_str(),
        );
    }));

    let mut loggers: Vec<Box<dyn Log>> = vec![];

    loggers.push(Box::new(logger::TcpLogger::new()));

    multi_log::MultiLogger::init(loggers, log::Level::Info).unwrap();

    skyline::install_hooks!(mount_rom_hook, socket_initialize_hook, socket_initialize_config_hook, ro_initialize_hook);

    println!("Ozone is installed and running!");
}
