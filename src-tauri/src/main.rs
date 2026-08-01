// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `spwn prompt …`: a hook helper that raises a UI prompt and prints the choice.
    // Handle it as a plain CLI (no GUI boot) and exit with the client's status.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("prompt") {
        let (code, out) = spwn_lib::run_prompt_cli(&args[2..]);
        if let Some(line) = out {
            println!("{line}");
        }
        std::process::exit(code);
    }
    // `spwn checkpoint <turn_uuid>`: the default session-turn hook's snapshot helper.
    if args.get(1).map(String::as_str) == Some("checkpoint") {
        std::process::exit(spwn_lib::run_checkpoint_cli(&args[2..]));
    }
    spwn_lib::run()
}
