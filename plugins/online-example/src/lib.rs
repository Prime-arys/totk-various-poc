//! online-example: forces a mod pack downloaded from a server.
//!
//! What an online mode would do so that every player runs the same mods:
//!
//! 1. From `main`, take control of totk-mod-merger's mod list. The SD card's
//!    own mods are then left out, and the merge waits for us.
//! 2. On a thread, download `<server>/manifest.txt`, then every package it
//!    lists, into `sd:/totk/online/packs`.
//! 3. Add the packages (with their options) and commit. The merge goes to
//!    `sd:/totk/online/merged`, so the local merge stays cached.
//!
//! If the server cannot be reached, the packs from the last successful
//! download are used; if there are none, control is released and the SD
//! card's mods are merged as usual.
//!
//! Configuration, `sd:/totk/online/online.ini`:
//!
//! ```ini
//! server = http://192.168.1.20:8080
//! timeout_ms = 30000
//! ```
//!
//! `manifest.txt`, one package per line, options after it:
//!
//! ```text
//! pvp-rules.tkcl
//! weapons.tkcl  Damage=Balanced; Durability=Unbreakable
//! ```
//!
//! Plain HTTP only: this is an example, not a hardened client.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use totk_mod_merger_api::{Control, ModMerger};

const ROOT: &str = "sd:/totk/online";

fn log(message: &str) {
    skyline::println!("[online-example] {}", message);
    let path = format!("{}/online.log", ROOT);
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{}", message);
    }
}

struct Settings {
    server: String,
    timeout_ms: u32,
}

fn load_settings() -> Option<Settings> {
    let text = std::fs::read_to_string(format!("{}/online.ini", ROOT)).ok()?;
    let mut settings = Settings {
        server: String::new(),
        timeout_ms: 30_000,
    };
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "server" => settings.server = value.trim().trim_end_matches('/').to_string(),
            "timeout_ms" => settings.timeout_ms = value.trim().parse().unwrap_or(settings.timeout_ms),
            _ => {}
        }
    }
    (!settings.server.is_empty()).then_some(settings)
}

#[skyline::main(name = "online-example")]
pub fn main() {
    let _ = std::fs::create_dir_all(format!("{}/packs", ROOT));
    let _ = std::fs::remove_file(format!("{}/online.log", ROOT));

    let Some(settings) = load_settings() else {
        log("no server configured in sd:/totk/online/online.ini, staying out of the way");
        return;
    };
    let Some(merger) = ModMerger::find() else {
        log("totk-mod-merger-plugin is not loaded (or speaks another API version)");
        return;
    };
    merger.on_merged(report, core::ptr::null_mut());

    let Some(control) = merger.take_control("online", settings.timeout_ms) else {
        log("could not take control of the mod list");
        return;
    };
    log(&format!("took control of the mod list, fetching from {}", settings.server));

    let worker = std::thread::Builder::new()
        .name("online-example".into())
        .stack_size(256 * 1024)
        .spawn(move || choose_mods(control, &settings));
    match worker {
        // Never drop a JoinHandle on Skyline: dropping detaches the thread,
        // and nnSdk's pthread_detach crashes.
        Ok(handle) => std::mem::forget(handle),
        Err(error) => log(&format!("could not start the download thread: {}", error)),
    }
}

extern "C" fn report(state: i32, served_files: u32, _: *mut core::ffi::c_void) {
    log(&format!("merge finished: state {}, {} files served", state, served_files));
}

fn choose_mods(control: Control, settings: &Settings) {
    init_sockets();

    let manifest_path = format!("{}/manifest.txt", ROOT);
    let manifest = match http_get(&format!("{}/manifest.txt", settings.server)) {
        Ok(body) => {
            let text = String::from_utf8_lossy(&body).into_owned();
            let _ = std::fs::write(&manifest_path, &text);
            let mut complete = true;
            for (file, _) in parse_manifest(&text) {
                match http_get(&format!("{}/{}", settings.server, file)) {
                    Ok(data) => {
                        log(&format!("downloaded {} ({} bytes)", file, data.len()));
                        if std::fs::write(format!("{}/packs/{}", ROOT, file), data).is_err() {
                            complete = false;
                        }
                    }
                    Err(error) => {
                        log(&format!("could not download {}: {}", file, error));
                        complete = false;
                    }
                }
            }
            complete.then_some(text)
        }
        Err(error) => {
            log(&format!("server unreachable ({}), trying the last downloaded packs", error));
            None
        }
    }
    .or_else(|| std::fs::read_to_string(&manifest_path).ok());

    let Some(manifest) = manifest else {
        log("nothing to force, the SD card's mods will be used");
        control.release();
        return;
    };

    for (file, options) in parse_manifest(&manifest) {
        let path = format!("{}/packs/{}", ROOT, file);
        if std::fs::metadata(&path).is_err() {
            log(&format!("{} is missing, the SD card's mods will be used", file));
            control.release();
            return;
        }
        let Some(index) = control.add_mod(&path, Some(&file)) else {
            log(&format!("could not add {}", file));
            continue;
        };
        for (group, option) in options {
            control.select_option(index, &group, &option);
        }
    }

    control.set_merged_dir(&format!("{}/merged", ROOT));
    control.commit();
    log("mod list committed");
}

/// (package file, [(group, option)]) per line.
fn parse_manifest(text: &str) -> Vec<(String, Vec<(String, String)>)> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let (file, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
            // Only plain file names: the manifest must not write outside our folder.
            if file.contains('/') || file.contains('\\') || file.contains("..") {
                return None;
            }
            let options = rest
                .split(';')
                .filter_map(|pair| pair.split_once('='))
                .map(|(group, option)| (group.trim().to_string(), option.trim().to_string()))
                .collect();
            Some((file.to_string(), options))
        })
        .collect()
}

/// skyline-totk brings sockets up on demand for plugins that need the network
/// this early in boot.
fn init_sockets() {
    let mut address: usize = 0;
    let name = b"skyline_totk_init_sockets\0";
    let found = unsafe { skyline::nn::ro::LookupSymbol(&mut address as *mut usize, name.as_ptr()) } == 0;
    if found && address != 0 {
        let init: extern "C" fn() -> bool = unsafe { core::mem::transmute(address) };
        if !init() {
            log("socket initialization failed");
        }
    }
}

/// A plain HTTP/1.0 GET.
fn http_get(url: &str) -> Result<Vec<u8>, String> {
    let rest = url.strip_prefix("http://").ok_or("only http:// URLs are supported")?;
    let (host_port, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let address = if host_port.contains(':') {
        host_port.to_string()
    } else {
        format!("{}:80", host_port)
    };

    let socket_address = address
        .to_socket_addrs()
        .map_err(|e| format!("{}: {}", address, e))?
        .next()
        .ok_or_else(|| format!("{}: no address", address))?;
    let mut stream =
        TcpStream::connect_timeout(&socket_address, Duration::from_secs(5)).map_err(|e| e.to_string())?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(20)));

    let host = host_port.split(':').next().unwrap_or(host_port);
    write!(stream, "GET {} HTTP/1.0\r\nHost: {}\r\nConnection: close\r\n\r\n", path, host).map_err(|e| e.to_string())?;

    // Read until the body announced by Content-Length is complete rather than
    // until the server closes: not every socket stack reports the close.
    let mut response = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    let mut header_end: Option<usize> = None;
    let mut body_length: Option<usize> = None;
    loop {
        if let (Some(end), Some(length)) = (header_end, body_length) {
            if response.len() >= end + 4 + length {
                break;
            }
        }
        let read = match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => read,
            // A timeout after a complete body is not an error.
            Err(error) if header_end.is_some() && body_length.is_none() => {
                log(&format!("read ended: {}", error));
                break;
            }
            Err(error) => return Err(error.to_string()),
        };
        response.extend_from_slice(&chunk[..read]);

        if header_end.is_none() {
            if let Some(end) = response.windows(4).position(|w| w == b"\r\n\r\n") {
                header_end = Some(end);
                body_length = String::from_utf8_lossy(&response[..end])
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.trim().eq_ignore_ascii_case("content-length").then(|| value.trim().parse().ok())?
                    });
            }
        }
    }

    let header_end = header_end.ok_or("malformed HTTP response")?;
    let header = String::from_utf8_lossy(&response[..header_end]);
    let status = header.lines().next().unwrap_or("");
    if !status.split_whitespace().nth(1).map_or(false, |code| code == "200") {
        return Err(format!("{} for {}", status, path));
    }
    let body = &response[header_end + 4..];
    Ok(match body_length {
        Some(length) => body[..length.min(body.len())].to_vec(),
        None => body.to_vec(),
    })
}
