use rand::{RngCore, SeedableRng};
use size::Size;
use std::env;
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const ENOENT: i32 = 2;
const ENOSPC: i32 = 28;
#[cfg_attr(not(windows), allow(dead_code))]
const ERROR_PATH_NOT_FOUND: i32 = 3;
#[cfg_attr(not(windows), allow(dead_code))]
const ERROR_ACCESS_DENIED: i32 = 5;

fn print_usage() {
    #[cfg(not(windows))]
    println!("USAGE: hddrand [--verify] /dev/disk");
    #[cfg(windows)]
    println!("USAGE: hddrand [--verify] \\??\\PhysicalDrive8");
}

fn main() {
    let args: Vec<_> = env::args().skip(1).collect();
    if args.len() == 0 {
        print_usage();
        std::process::exit(0);
    }

    let mut drive = None;
    let mut verify = false;
    for arg in args.iter() {
        match arg.as_str() {
            "verify" | "--verify" => verify = true,
            path => {
                if path.starts_with('/') || path.starts_with('\\') {
                    drive = Some(path);
                }
            }
        }
    }

    let drive = match drive {
        None => {
            print_usage();
            std::process::exit(1);
        }
        Some(drive) => drive,
    };

    let path = Path::new(&drive);
    // NtCreateFile expects a path in the NT namespace (\) while CreateFile takes paths from the
    // global namespace \GLOBAL?? (aka \??). PhysicalDriveX is in the global namespace but not in
    // the root of the NT namespace. We can open PhysicalDriveX by prepending \\?\ to the path, but
    // Path::exists() doesn't support such a path, so we have to use our own exists wrapper.
    #[cfg_attr(windows, allow(unreachable_code))]
    while !path.exists() {
        #[cfg(windows)]
        match nt_exists(&drive) {
            NtFileStatus::Found => {
                break;
            }
            NtFileStatus::NotFound => {
                eprintln!("{}: not found!", path.display());
                std::process::exit(ERROR_PATH_NOT_FOUND);
            }
            NtFileStatus::AccessDenied => {
                eprintln!("{}: access denied!", path.display());
                std::process::exit(ERROR_ACCESS_DENIED);
            }
        }
        eprintln!("{}: not found!", path.display());
        std::process::exit(ENOENT);
    }

    let result = if verify {
        verify_drive(path)
    } else {
        fill_drive(path)
    };

    eprintln!("\n");

    match result {
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(-1);
        }
        Ok(result) => println!(
            "{} {} bytes to {} in {} seconds",
            if verify { "Verified" } else { "Wrote" },
            result.0,
            path.display(),
            result.1.as_secs()
        ),
    };
}

struct OnDrop<F>(F)
where
    F: FnMut() -> ();

impl<F> Drop for OnDrop<F>
where
    F: FnMut() -> (),
{
    fn drop(&mut self) -> () {
        self.0();
    }
}

fn verify_drive(path: &Path) -> std::io::Result<(usize, Duration)> {
    let mut first_time = true;
    let mut read_buffer = Vec::new();
    read_buffer.resize(8 * 1024 * 1024, 0u8);
    let mut rand_buffer = Vec::new();
    rand_buffer.resize(8 * 1024 * 1024, 0u8);

    let start = Instant::now();
    let done = Arc::new(AtomicBool::new(false));
    let total_read = Arc::new(AtomicUsize::new(0));
    let mut file = OpenOptions::new().read(true).open(path)?;

    start_progress_thread(total_read.clone(), done.clone());
    let on_drop = OnDrop(|| done.clone().store(true, Ordering::Release));

    // This needs to be a multiple of the page size on some platforms!
    let mut seed_buf = [0u8; 1024];
    {
        let mut bytes_read = 0;
        while bytes_read < 32 {
            let read = file.read(&mut seed_buf)?;
            bytes_read += read;
            if read == 0 {
                panic!("Unable to read the seed out of the source!");
            }
        }
        file.seek(SeekFrom::Start(0))?;
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&seed_buf[0..32]);
    let mut rng = rand_chacha::ChaCha8Rng::from_seed(seed);

    loop {
        rng.fill_bytes(&mut rand_buffer[..]);
        if first_time {
            (&mut rand_buffer[..]).write_all(&seed[..])?;
            first_time = false;
        }

        let mut write_offset = 0;
        loop {
            let read = file.read(&mut read_buffer[write_offset..])?;

            if read_buffer[write_offset..][..read] != rand_buffer[write_offset..][..read] {
                // Mismatch in expected contents!
                drop(on_drop);

                // Find the start of the mismatch
                let mut mismatch_start = 0;
                for i in write_offset..(write_offset + read) {
                    if read_buffer[i] != rand_buffer[i] {
                        mismatch_start = i;
                        break;
                    }
                }
                eprintln!(
                    "Content mismatch starting at offset {:x}",
                    write_offset + mismatch_start
                );
                eprintln!(
                    "Expected {:x}, found {:x}",
                    rand_buffer[mismatch_start], read_buffer[mismatch_start]
                );
                return Ok((total_read.load(Ordering::Acquire), start.elapsed()));
            }

            write_offset += read;
            total_read.fetch_add(read, Ordering::SeqCst);

            if read == rand_buffer.len() {
                break;
            }
            if read == 0 {
                return Ok((total_read.load(Ordering::Acquire), start.elapsed()));
            }
        }
    }
}

fn fill_drive(path: &Path) -> std::io::Result<(usize, Duration)> {
    let seed: [u8; 32] = rand::random();
    let mut rng = rand_chacha::ChaCha8Rng::from_seed(seed);

    let mut buffer = Vec::new();
    let mut first_time = true;
    buffer.resize(8 * 1024 * 1024, 0u8);

    let start = Instant::now();
    let done = Arc::new(AtomicBool::new(false));
    let total_written = Arc::new(AtomicUsize::new(0));
    let mut file = OpenOptions::new().write(true).open(path)?;

    start_progress_thread(total_written.clone(), done.clone());
    let _on_drop = OnDrop(|| done.clone().store(true, Ordering::Release));

    loop {
        rng.fill_bytes(&mut buffer[..]);
        if first_time {
            (&mut buffer[..]).write_all(&seed)?;
            first_time = false;
        }

        let mut read_offset = 0;
        loop {
            let written = match file.write(&buffer[read_offset..]) {
                Ok(bytes) => bytes,
                Err(e) if e.raw_os_error() == Some(ENOSPC) => {
                    return Ok((total_written.load(Ordering::Acquire), start.elapsed()))
                }
                Err(e) => return Err(e),
            };
            read_offset += written;
            total_written.fetch_add(written, Ordering::SeqCst);

            if written == buffer.len() {
                break;
            }
            if written == 0 {
                return Ok((total_written.load(Ordering::Acquire), start.elapsed()));
            }
        }
    }
}

fn start_progress_thread(total_bytes: Arc<AtomicUsize>, done: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let mut timer = Instant::now();
        let mut previous_bytes = total_bytes.load(Ordering::Relaxed);
        loop {
            std::thread::sleep(Duration::from_secs(1));
            if !done.load(Ordering::Acquire) {
                let new_bytes = total_bytes.load(Ordering::Relaxed);
                let written = new_bytes - previous_bytes;
                previous_bytes = new_bytes;
                let elapsed_secs = timer.elapsed().as_nanos() as f64 / (10_u64.pow(9) as f64);
                let rate = written as f64 / elapsed_secs;
                timer = Instant::now();
                eprint!(
                    "\r{} @ {}/sec     \x08\x08\x08\x08\x08",
                    Size::from_bytes(new_bytes),
                    Size::from_bytes(rate)
                );
            } else {
                break;
            }
        }
    });
}

#[cfg(windows)]
enum NtFileStatus {
    NotFound,
    Found,
    AccessDenied,
}

#[cfg(windows)]
fn nt_exists(path: &str) -> NtFileStatus {
    use windows_sys::{
        Win32::Foundation::*, Win32::Storage::FileSystem::*, Win32::System::Kernel::*,
        Win32::System::WindowsProgramming::*,
    };

    let mut unicode_str: Vec<u16> = path.encode_utf16().into_iter().collect();
    let mut unicode_path = UNICODE_STRING {
        Length: (unicode_str.len() * std::mem::size_of::<u16>()) as _,
        MaximumLength: (unicode_str.len() * std::mem::size_of::<u16>()) as _,
        Buffer: unicode_str.as_mut_ptr(),
    };
    let mut obj_attr = OBJECT_ATTRIBUTES {
        Length: 0,
        RootDirectory: 0,
        ObjectName: &mut unicode_path as *mut _,
        Attributes: OBJ_CASE_INSENSITIVE as _,
        SecurityDescriptor: std::ptr::null_mut() as *mut _,
        SecurityQualityOfService: std::ptr::null_mut() as *mut _,
    };
    obj_attr.Length = std::mem::size_of_val(&obj_attr) as _;

    unsafe {
        let mut handle = 0;
        let mut status_block: IO_STATUS_BLOCK = std::mem::zeroed();

        let result = NtCreateFile(
            &mut handle as *mut _,
            FILE_READ_DATA | FILE_READ_ATTRIBUTES,
            &mut obj_attr as *mut _,
            &mut status_block as *mut _,
            std::ptr::null_mut() as *mut _,
            FILE_ATTRIBUTE_NORMAL,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_OPEN,
            FILE_NON_DIRECTORY_FILE,
            std::ptr::null::<core::ffi::c_void>() as *mut _,
            0,
        );

        /*
        eprintln!("NTSTATUS: {:#X}", result);
        // Information is FILE_CREATED/OPENED/OVERWRITTEN/SUPERSEDED/EXISTS/DOES_NOT_EXIST
        eprintln!("IO_STATUS_BLOCK.Information: {}", status_block.Information);
        */

        match result {
            STATUS_SUCCESS => {
                NtClose(handle);
                NtFileStatus::Found
            }
            STATUS_ACCESS_DENIED => NtFileStatus::AccessDenied,
            _ => NtFileStatus::NotFound,
        }
    }
}
