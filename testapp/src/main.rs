use total5::{F, PATH};
use std::path::PathBuf;

fn main() {
    // Initialize the framework
    println!("Framework version: {}", F.version);
    
  
    // Use some path operations
    let logs_path = PATH.logs(Some("debug.log"));
    println!("Application log path: {:?}", logs_path);
    
    // Create directories if they don't exist
    PATH.verify(&logs_path.parent().unwrap());
    
    // Check if paths exist
    let exists = PATH.exists_dir(&logs_path);
    println!("Log file exists: {}", exists);
    
    // Example of routing paths
    let plugin_route = PATH.route("_myplugin/public/style.css", "public");
    println!("Plugin route: {:?}", plugin_route);
    
    // Access different directory types
    let public_assets = PATH.public(Some("assets"));
    let template_file = PATH.templates(Some("default.html"));
    
    println!("Public assets directory: {:?}", public_assets);
    println!("Default template path: {:?}", template_file);
    
    println!("Test application completed!");
}