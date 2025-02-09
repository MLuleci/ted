use std::env;
use std::process;
use ted::Config;

fn main() {
    let args: Vec<String> = env::args().collect();

    let config = Config::build(&args).unwrap_or_else(|err| {
        eprintln!("Error parsing arguments: {err}");
        process::exit(1);
    });

    if let Err(err) = ted::run(config) {
        eprintln!("Runtime error: {err}");
        process::exit(1);
    }
}
