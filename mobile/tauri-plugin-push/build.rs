const COMMANDS: &[&str] = &["request_permission", "get_token"];

fn main() {
  tauri_plugin::Builder::new(COMMANDS)
    .android_path("android")
    .ios_path("ios")
    .build();
}
