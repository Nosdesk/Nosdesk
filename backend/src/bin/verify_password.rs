use bcrypt::{verify, DEFAULT_COST};
use std::env;

fn main() {
    // Get command line arguments
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: {} <password> <hash>", args[0]);
        return;
    }

    let password = &args[1];
    let hash_str = &args[2];

    // Verify the password against the hash
    match verify(password, hash_str) {
        Ok(result) => {
            println!("Password verification result: {result}");
            if !result {
                println!("Password does not match the hash.");
            }
        },
        Err(e) => {
            println!("Error verifying password: {e:?}");
        }
    }

    // Generate a new hash for the password
    match bcrypt::hash(password, DEFAULT_COST) {
        Ok(new_hash) => {
            println!("New hash for '{password}': {new_hash}");
            println!("Bytes: {:?}", new_hash.as_bytes());
        },
        Err(e) => {
            println!("Error generating hash: {e:?}");
        }
    }
} 