use clap::{Parser, Subcommand};
use serde::Deserialize;
use squeeze_pic_core::ProcessOptions;
use std::{
    io::{self, BufRead, Read},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Parser)]
#[command(
    name = "squeeze-pic",
    version,
    about = "Offline image compression and conversion"
)]
struct Args {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Inspect {
        input: PathBuf,
    },
    Convert {
        input: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        #[arg(long, value_parser = ["jpeg", "png", "webp", "avif", "jxl", "heif", "gif", "bmp", "tiff", "ico", "icns"])]
        format: Option<String>,
        #[arg(long)]
        options: Option<PathBuf>,
    },
    #[command(hide = true)]
    Worker,
}
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "lowercase")]
enum Request {
    Inspect {
        input: PathBuf,
    },
    Convert {
        input: PathBuf,
        output: PathBuf,
        options: ProcessOptions,
    },
    Preview {
        input: PathBuf,
        output: PathBuf,
        max_edge: u32,
    },
}
fn dispatch(
    request: Request,
    cancel: &AtomicBool,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    Ok(match request {
        Request::Inspect { input } => serde_json::to_value(squeeze_pic_core::inspect_file(input)?)?,
        Request::Preview {
            input,
            output,
            max_edge,
        } => serde_json::to_value(squeeze_pic_core::create_preview(input, output, max_edge)?)?,
        Request::Convert {
            input,
            output,
            options,
        } => serde_json::to_value(squeeze_pic_core::process_file(
            input,
            output,
            &options,
            cancel,
            |phase| {
                eprintln!("{}", serde_json::json!({"phase": phase}));
            },
        )?)?,
    })
}
fn run() -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let args = Args::parse();
    let cancel = Arc::new(AtomicBool::new(false));
    let signal = cancel.clone();
    ctrlc::set_handler(move || signal.store(true, Ordering::SeqCst))?;
    match args.command {
        Command::Inspect { input } => dispatch(Request::Inspect { input }, &cancel),
        Command::Convert {
            input,
            output,
            format,
            options,
        } => {
            let mut options: ProcessOptions = match options {
                Some(path) => serde_json::from_reader(std::fs::File::open(path)?)?,
                None => ProcessOptions::default(),
            };
            if let Some(format) = format {
                options.format = Some(serde_json::from_value(serde_json::Value::String(format))?);
            }
            dispatch(
                Request::Convert {
                    input,
                    output,
                    options,
                },
                &cancel,
            )
        }
        Command::Worker => {
            let mut text = String::new();
            io::stdin().lock().take(65_537).read_line(&mut text)?;
            if text.len() > 65_536 {
                return Err("Worker request exceeds limit".into());
            }
            dispatch(serde_json::from_str(&text)?, &cancel)
        }
    }
}
fn main() {
    match run() {
        Ok(value) => println!("{value}"),
        Err(error) => {
            if let Some(error) = error.downcast_ref::<squeeze_pic_core::PicError>() {
                eprintln!("{}", serde_json::to_string(error).unwrap());
            } else {
                eprintln!(
                    "{}",
                    serde_json::json!({"code": "worker", "message": error.to_string()})
                );
            }
            std::process::exit(1);
        }
    }
}
