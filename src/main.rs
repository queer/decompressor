use std::io::Read;
use std::io;

use atty::Stream;
use clap::{ArgAction, Parser};
use color_eyre::eyre::{Result, eyre};
use ctx::{CompressionType, Context};

mod ctx;

#[derive(Debug, Parser)]
pub struct Flags {
    #[arg(short, long, default_value = "false", action = ArgAction::SetTrue)]
    quiet: bool,

    #[arg(
        index = 1,
        default_value = "unknown",
        help = "Hint for the compression type, e.g. `brotli`"
    )]
    hint: String,

    #[arg(
        short,
        long,
        help = "Force the output to be compressed with the given type, e.g. `brotli`",
        default_value = "none"
    )]
    output_type: Option<CompressionType>,
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let flags = Flags::parse();
    if atty::is(Stream::Stdin) {
        return Err(eyre!("input is a terminal, please pipe data via stdin!"));
    }

    let stdin = io::stdin();
    let mut stdin = stdin.lock();

    
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    let (kind, magic) = ctx::detect_stream_characteristics(&mut stdin, &flags)?;
    // chain magic to stdin
    let mut stdin = magic.chain(stdin);

    let mut context = Context::new_from_stream(&mut stdin, &mut stdout, kind, &flags)?;
    context.translate_stream()
}
