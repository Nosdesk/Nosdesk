use bcrypt::{hash, DEFAULT_COST};
use diesel::prelude::*;
use diesel::pg::PgConnection;
use std::env;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    // Get command line arguments
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: {} <email> <new_password>", args[0]);
        return Ok(());
    }

    let email = &args[1];
    let password = &args[2];

    // Hash the password
    let hashed_password = hash(password, DEFAULT_COST)?;
    println!("Generated hash: {hashed_password}");
    
    // Convert to bytes for storage
    let password_bytes = hashed_password.into_bytes();

    // Connect to the database
    let database_url = "postgres://kylephillips@localhost/helpdesk";
    let mut conn = PgConnection::establish(database_url)?;

    // Update the user's password
    let affected_rows = diesel::sql_query(
        "UPDATE users SET password_hash = $1 WHERE email = $2"
    )
    .bind::<diesel::sql_types::Binary, _>(&password_bytes)
    .bind::<diesel::sql_types::Text, _>(email)
    .execute(&mut conn)?;

    println!("Updated {affected_rows} user(s) with email: {email}");

    Ok(())
} 