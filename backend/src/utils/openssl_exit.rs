//! Keep OpenSSL from tearing itself down when a short-lived process exits.
//!
//! libpq speaks TLS through OpenSSL, and r2d2 opens its connections on its
//! own worker threads, so those threads hold OpenSSL per-thread state.
//! Dropping a pool only tells the workers to stop: `scheduled_thread_pool`
//! never joins them. A worker can therefore still be exiting, and running
//! OpenSSL's per-thread destructor, while the main thread's `exit` runs
//! `OPENSSL_cleanup`, which libcrypto registers with `atexit` the first time
//! it initialises. The destructor then calls into state the cleanup has
//! freed, and the process dies with SIGSEGV (exit 139) after its work is
//! done. `nosdesk-cli` did this on about half its runs.
//!
//! The race is OpenSSL's global teardown against threads nobody can join,
//! so the fix is to not tear down: `OPENSSL_INIT_NO_ATEXIT` on the first
//! initialisation stops libcrypto registering the handler. Exiting threads
//! then find their state intact, other `atexit` handlers still run, and the
//! kernel reclaims the memory the cleanup would have freed.
//!
//! `OPENSSL_init_crypto` is looked up through libpq's own image, which
//! resolves to the libcrypto libpq links, rather than linked here. A macOS
//! process can hold several libcryptos (the system's beside Homebrew's), and
//! the flag only helps on the instance libpq uses. Where libpq has no
//! OpenSSL, nothing is found and nothing changes.

use std::ffi::{c_int, c_void};

/// `OPENSSL_INIT_NO_ATEXIT` (`openssl/crypto.h`, OpenSSL 1.1.1b and later).
const OPENSSL_INIT_NO_ATEXIT: u64 = 0x0008_0000;

type InitCrypto = unsafe extern "C" fn(opts: u64, settings: *const c_void) -> c_int;

extern "C" {
    /// libpq's version: any libpq symbol, to find libpq's image by.
    fn PQlibVersion() -> c_int;
}

/// libpq's `OPENSSL_init_crypto`: looked up in libpq's image, which searches
/// libpq and the libraries it links.
fn libpq_init_crypto() -> Option<InitCrypto> {
    // SAFETY: dladdr fills `info` for an address inside a loaded image;
    // PQlibVersion is one, as libpq is linked.
    let mut info: libc::Dl_info = unsafe { std::mem::zeroed() };
    let found = unsafe { libc::dladdr(PQlibVersion as *const c_void, &mut info) };
    if found == 0 || info.dli_fname.is_null() {
        return None;
    }
    // SAFETY: RTLD_NOLOAD only hands back a handle to the image already
    // loaded at that path, raising its reference count, which is never
    // dropped: libpq stays loaded for the life of the process anyway.
    let libpq = unsafe { libc::dlopen(info.dli_fname, libc::RTLD_NOLOAD | libc::RTLD_LAZY) };
    if libpq.is_null() {
        return None;
    }
    // SAFETY: a NUL-terminated name looked up through a live handle.
    let symbol = unsafe { libc::dlsym(libpq, c"OPENSSL_init_crypto".as_ptr()) };
    if symbol.is_null() {
        return None;
    }
    // SAFETY: the symbol is libcrypto's `int OPENSSL_init_crypto(uint64_t
    // opts, const OPENSSL_INIT_SETTINGS *settings)`.
    Some(unsafe { std::mem::transmute::<*mut c_void, InitCrypto>(symbol) })
}

/// Initialise libpq's libcrypto without its exit-time cleanup. Call it first
/// thing in `main`, before anything opens a database connection: the first
/// initialisation decides. Returns whether libpq's libcrypto took the flag.
pub fn skip_cleanup_at_exit() -> bool {
    match libpq_init_crypto() {
        // SAFETY: a null settings pointer is allowed, and the call is safe
        // to repeat.
        Some(init) => unsafe { init(OPENSSL_INIT_NO_ATEXIT, std::ptr::null()) == 1 },
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_loaded_libcrypto_takes_the_flag() {
        // The test binary links libpq, which links libcrypto.
        assert!(skip_cleanup_at_exit());
        assert!(skip_cleanup_at_exit(), "a second call is harmless");
    }
}
