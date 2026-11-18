//! Keep OpenSSL from tearing itself down when a short-lived process exits.
//!
//! libpq speaks TLS through OpenSSL, and r2d2 opens its connections on its
//! own worker threads, so those threads hold OpenSSL per-thread state.
//! Dropping a pool only tells the workers to stop: `scheduled_thread_pool`
//! never joins them. A worker can therefore still be exiting, and running
//! OpenSSL's per-thread destructor, while the main thread's `exit` runs
//! `OPENSSL_cleanup`, which libcrypto registers with `atexit` the first time
//! it initializes. The destructor then calls into state the cleanup has
//! freed, and the process dies with SIGSEGV (exit 139) after its work is
//! done. `nosdesk-cli` did this on about half its runs.
//!
//! The race is OpenSSL's global teardown against threads nobody can join,
//! so the fix is to not tear down: `OPENSSL_INIT_NO_ATEXIT` on the first
//! initialization stops libcrypto registering the handler. Exiting threads
//! then find their state intact, other `atexit` handlers still run, and the
//! kernel reclaims the memory the cleanup would have freed. OpenSSL 4.0
//! dropped the exit-time cleanup altogether; this gives 3.x the same.
//!
//! Only the first initialization decides, and libpq is the only component
//! that initializes libcrypto without the flag (rust-openssl passes it). It
//! does so on its first connection, TLS or not, since SCRAM login uses
//! libcrypto. So [`ensure`] runs where Diesel reaches libpq, in `db`:
//! `ResettingManager::new` and `db::establish`. The `libpq_entry_lint` test
//! keeps every other connection path out, and `openssl_exit_cleanup` checks
//! that connecting through `db` leaves no cleanup armed.
//!
//! `OPENSSL_init_crypto` is looked up through libpq's own image, which
//! resolves to the libcrypto libpq links, rather than linked here. A macOS
//! process can hold several libcryptos (the system's beside Homebrew's), and
//! the flag only helps on the instance libpq uses. Where libpq has no
//! OpenSSL, nothing is found and nothing changes.

use std::ffi::{c_int, c_void, CStr};
use std::ptr::NonNull;
use std::sync::OnceLock;

/// `OPENSSL_INIT_NO_ATEXIT` (`openssl/crypto.h`, OpenSSL 1.1.1b and later).
const OPENSSL_INIT_NO_ATEXIT: u64 = 0x0008_0000;

type InitCrypto = unsafe extern "C" fn(opts: u64, settings: *const c_void) -> c_int;

extern "C" {
    /// libpq's version: any libpq symbol, to find libpq's image by.
    fn PQlibVersion() -> c_int;
}

/// A libcrypto symbol as libpq sees it: looked up in libpq's image, which
/// searches libpq and the libraries it links. None where libpq has no such
/// symbol (no OpenSSL, or an OpenSSL without it).
pub fn libpq_crypto_symbol(name: &CStr) -> Option<NonNull<c_void>> {
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
    NonNull::new(unsafe { libc::dlsym(libpq, name.as_ptr()) })
}

/// libpq's `OPENSSL_init_crypto`.
fn libpq_init_crypto() -> Option<InitCrypto> {
    let symbol = libpq_crypto_symbol(c"OPENSSL_init_crypto")?;
    // SAFETY: the symbol is libcrypto's `int OPENSSL_init_crypto(uint64_t
    // opts, const OPENSSL_INIT_SETTINGS *settings)`.
    Some(unsafe { std::mem::transmute::<*mut c_void, InitCrypto>(symbol.as_ptr()) })
}

/// Initialize libpq's libcrypto without its exit-time cleanup, once per
/// process. `db` calls it before every libpq connection; nothing else needs
/// to. Returns whether libpq's libcrypto took the flag (false where libpq has
/// no OpenSSL).
pub fn ensure() -> bool {
    static TAKEN: OnceLock<bool> = OnceLock::new();
    *TAKEN.get_or_init(skip_cleanup_at_exit)
}

fn skip_cleanup_at_exit() -> bool {
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

    // Checks the lookup only: OpenSSL hands back the first initialization's
    // result, so this passes even if the cleanup was armed first. The
    // ordering is checked by tests/it/openssl_exit_cleanup.rs.
    #[test]
    fn the_loaded_libcrypto_takes_the_flag() {
        // The test binary links libpq, which links libcrypto.
        assert!(ensure());
        assert!(skip_cleanup_at_exit(), "a repeated call is harmless");
    }
}
