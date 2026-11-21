//! `paper-mcp`: an MCP server that lets AI assistants, such as Claude Code,
//! Codex, and Claude Desktop, read a Paper library. The assistant starts it
//! and talks to it over stdin and stdout.
//!
//! It opens the library read-only beside the app, so it works whether Paper
//! runs or not, and never changes the library. While Paper runs, it also
//! knows which paper the reader shows.

mod server;

use std::path::PathBuf;
use std::process::ExitCode;

use rmcp::ServiceExt;

const USAGE: &str = "\
paper-mcp: lets AI assistants read your Paper library over MCP (stdio).

Usage: paper-mcp [--library <folder>]

  --library <folder>  The library to read. Default: $PAPER_LIBRARY, else the
                      library Paper opened last.

Claude Code:  claude mcp add --scope user paper -- <path to paper-mcp>
Codex:        codex mcp add paper -- <path to paper-mcp>
";

#[tokio::main]
async fn main() -> ExitCode {
    let mut library = std::env::var_os("PAPER_LIBRARY").map(PathBuf::from);
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--library") => match args.next() {
                Some(path) => library = Some(path.into()),
                None => {
                    eprint!("--library needs a folder\n\n{USAGE}");
                    return ExitCode::from(2);
                }
            },
            Some("-h" | "--help") => {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            Some("-V" | "--version") => {
                println!("paper-mcp {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            _ => {
                eprint!("unknown argument {}\n\n{USAGE}", arg.to_string_lossy());
                return ExitCode::from(2);
            }
        }
    }

    // Stdout carries the protocol, so errors go to stderr, which the
    // assistants show in their MCP logs.
    let running = match server::PaperServer::new(library)
        .serve(rmcp::transport::stdio())
        .await
    {
        Ok(running) => running,
        Err(error) => {
            eprintln!("paper-mcp: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = running.waiting().await {
        eprintln!("paper-mcp: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
