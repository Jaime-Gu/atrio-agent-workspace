#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--workspace-mcp") {
        if let Err(error) = pixel_workspace_lib::run_workspace_mcp() {
            eprintln!("Atrio MCP: {error}");
            std::process::exit(1);
        }
        return;
    }
    pixel_workspace_lib::run()
}
