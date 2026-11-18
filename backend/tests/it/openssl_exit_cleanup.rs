//! Connecting through `db` leaves OpenSSL's exit-time cleanup unarmed.
//!
//! libpq initializes OpenSSL on its first connection, and unless that first
//! initialization carries `OPENSSL_INIT_NO_ATEXIT`, OpenSSL registers a
//! cleanup that runs at exit and races the pools' unjoined worker threads
//! (`utils::openssl_exit`). `db::establish` and `db::ResettingManager::new`
//! set the flag before connecting. Each check here runs in a fresh child
//! process, since only a process's first initialization decides: the child
//! connects through one door only, then registers a handler with
//! `OPENSSL_atexit`, which `OPENSSL_cleanup` runs. The parent fails if the
//! handler ran when the child exited.
//!
//! No timing is involved, so a guard missing from either door fails every
//! run. OpenSSL 4.0 has no exit-time cleanup and no `OPENSSL_atexit`; the
//! checks skip there.

use std::process::Command;

use backend::db;
use backend::utils::openssl_exit;
use diesel::r2d2;

use crate::common;

const CHILD_ENV: &str = "NOSDESK_OPENSSL_EXIT_CHILD";
const CONNECTED: &str = "OPENSSL_EXIT_CHILD_CONNECTED";
const NO_ATEXIT_API: &str = "OPENSSL_EXIT_CHILD_NO_ATEXIT_API";
const CLEANUP_RAN: &str = "OPENSSL_EXIT_CHILD_CLEANUP_RAN";

#[test]
fn a_single_connection_leaves_no_exit_cleanup() {
    assert_child_exits_clean("establish", "child_connects_once");
}

#[test]
fn a_pool_leaves_no_exit_cleanup() {
    assert_child_exits_clean("pool", "child_connects_through_a_pool");
}

/// The child half of `a_single_connection_leaves_no_exit_cleanup`; does
/// nothing in the normal run.
#[test]
fn child_connects_once() {
    if std::env::var(CHILD_ENV).as_deref() != Ok("establish") {
        return;
    }
    let conn = db::establish(&common::admin_url()).expect("connect");
    after_connecting();
    drop(conn);
}

/// The child half of `a_pool_leaves_no_exit_cleanup`; does nothing in the
/// normal run.
#[test]
fn child_connects_through_a_pool() {
    if std::env::var(CHILD_ENV).as_deref() != Ok("pool") {
        return;
    }
    let pool = r2d2::Pool::builder()
        .max_size(1)
        .build(db::ResettingManager::new(common::admin_url()))
        .expect("build pool");
    let conn = pool.get().expect("check out");
    after_connecting();
    drop(conn);
}

/// Runs only after the child's first connection, so it can't decide the
/// first initialization itself.
fn after_connecting() {
    // libpq's libcrypto is found and took the flag: a lookup that finds
    // nothing would otherwise pass every check below.
    assert!(
        openssl_exit::ensure(),
        "libpq's libcrypto didn't take OPENSSL_INIT_NO_ATEXIT"
    );
    let Some(atexit) = openssl_exit::libpq_crypto_symbol(c"OPENSSL_atexit") else {
        eprintln!("{NO_ATEXIT_API}");
        return;
    };
    type Atexit = unsafe extern "C" fn(handler: extern "C" fn()) -> std::ffi::c_int;
    // SAFETY: libcrypto's `int OPENSSL_atexit(void (*handler)(void))`.
    let atexit = unsafe { std::mem::transmute::<*mut std::ffi::c_void, Atexit>(atexit.as_ptr()) };
    // SAFETY: registers a handler that only writes to stderr.
    assert_eq!(unsafe { atexit(cleanup_ran) }, 1, "OPENSSL_atexit failed");
    eprintln!("{CONNECTED}");
}

/// Run by `OPENSSL_cleanup`, which only runs at exit if it was armed.
extern "C" fn cleanup_ran() {
    const LINE: &[u8] = b"\nOPENSSL_EXIT_CHILD_CLEANUP_RAN\n";
    // SAFETY: writes a static buffer to stderr, with nothing for exit-time
    // teardown to have freed.
    unsafe { libc::write(2, LINE.as_ptr().cast(), LINE.len()) };
}

fn assert_child_exits_clean(door: &str, child: &str) {
    let exe = std::env::current_exe().expect("test binary path");
    let output = Command::new(exe)
        .args([
            "--exact",
            &format!("openssl_exit_cleanup::{child}"),
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_ENV, door)
        .output()
        .expect("run the child");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stderr.contains(NO_ATEXIT_API) {
        // OpenSSL 4.0 or later: no exit-time cleanup to arm.
        return;
    }
    assert!(
        output.status.success() && stderr.contains(CONNECTED),
        "the {door} child didn't connect and finish ({}):\n{stdout}\n{stderr}",
        output.status
    );
    assert!(
        !stderr.contains(CLEANUP_RAN),
        "connecting through db ({door}) armed OpenSSL's exit-time cleanup, which \
         races the pools' worker threads at exit (utils::openssl_exit)"
    );
}
