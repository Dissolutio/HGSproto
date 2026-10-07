#[tauri::command]
pub fn calculate_hex_distance(q1: i32, r1: i32, q2: i32, r2: i32) -> String {
    // 🚀 Proof of concept logic. Later, this will pass data straight to your shared library.
    let distance = (q1 - q2).abs() + (r1 - r2).abs();
    format!("Rust Native Engine calculated hex distance: {distance} tiles.")
}
