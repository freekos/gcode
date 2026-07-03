fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("--version") | Some("-V") => println!("gcode {}", gcode_core::version()),
        _ => {
            println!("gcode {} — the agentic IDE engine", gcode_core::version());
            println!("usage: gcode --version   (real commands arrive with Phase 1)");
        }
    }
}
