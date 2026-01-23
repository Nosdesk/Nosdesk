use std::env;
use std::fs;
use std::path::Path;

// Import from the parent crate
extern crate backend;
use backend::db;
use backend::models::TicketsJson;
use backend::repository;

fn main() {
    // Get the JSON file path from command line arguments
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Usage: {} <path_to_json_file>", args[0]);
        return;
    }

    let json_path = &args[1];
    let path = Path::new(json_path);
    
    // Read the JSON file
    let json_content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) => {
            println!("Failed to read JSON file: {e}");
            return;
        }
    };

    // Parse the JSON
    let tickets_json: TicketsJson = match serde_json::from_str(&json_content) {
        Ok(tickets) => tickets,
        Err(e) => {
            println!("Failed to parse JSON: {e}");
            return;
        }
    };

    // Establish database connection
    let pool = db::establish_connection_pool();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(e) => {
            println!("Database connection error: {e}");
            return;
        }
    };

    // Import each ticket
    let mut imported_count = 0;
    let mut failed_count = 0;

    for ticket_json in tickets_json.tickets {
        println!("Importing ticket: {}", ticket_json.title);
        match repository::import_ticket_from_json(&mut conn, &ticket_json) {
            Ok(_) => {
                imported_count += 1;
                println!("  Success!");
            }
            Err(e) => {
                failed_count += 1;
                println!("  Failed: {e}");
            }
        }
    }

    println!("Import complete. Imported: {imported_count}, Failed: {failed_count}");
} 