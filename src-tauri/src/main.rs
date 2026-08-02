use clap::Parser;
use std::net::IpAddr;

#[derive(Parser)]
#[command(
    name = "spwn",
    version,
    about = "CLI + web server for managing Claude Code sessions"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(clap::Subcommand)]
enum Cmd {
    /// Start the web server and open the UI in your browser (this is also the default
    /// when `spwn` is run with no subcommand).
    Serve(ServeArgs),
}

#[derive(clap::Args, Default)]
struct ServeArgs {
    /// Port to listen on.
    #[arg(long, default_value_t = 4317)]
    port: u16,
    /// Address to bind. Defaults to localhost; use 0.0.0.0 to expose on your LAN
    /// (note: there is no authentication).
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    /// Don't open the browser automatically.
    #[arg(long)]
    no_open: bool,
}

fn main() {
    // Hooks invoke these as short-lived subprocesses; handle them before any server or
    // clap machinery so they stay cheap and their arg passing is untouched.
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("prompt") => {
            let (code, out) = spwn_lib::run_prompt_cli(&args[2..]);
            if let Some(line) = out {
                println!("{line}");
            }
            std::process::exit(code);
        }
        Some("checkpoint") => {
            std::process::exit(spwn_lib::run_checkpoint_cli(&args[2..]));
        }
        _ => {}
    }

    let cli = Cli::parse();
    let serve = match cli.command {
        Some(Cmd::Serve(s)) => s,
        None => ServeArgs::default(),
    };

    let host: IpAddr = match serve.host.parse() {
        Ok(h) => h,
        Err(_) => {
            eprintln!("spwn: invalid --host '{}'", serve.host);
            std::process::exit(2);
        }
    };

    let opts = spwn_lib::ServeOpts {
        host,
        port: serve.port,
        no_open: serve.no_open,
    };

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");
    if let Err(e) = runtime.block_on(spwn_lib::serve(opts)) {
        eprintln!("spwn: {e}");
        std::process::exit(1);
    }
}
