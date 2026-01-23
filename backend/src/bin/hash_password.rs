use bcrypt::{hash, DEFAULT_COST};
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    
    if args.len() < 2 {
        println!("Usage: {} <password>", args[0]);
        return;
    }
    
    let password = &args[1];
    match hash(password, DEFAULT_COST) {
        Ok(hashed) => {
            println!("Password: {password}");
            println!("Bcrypt hash: {hashed}");
        },
        Err(e) => {
            eprintln!("Error hashing password: {e}");
        }
    }
} 