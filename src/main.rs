use std::io::{self, Read, Write};

fn run() -> io::Result<()> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;

    let output = wide_md::unwrap_markdown(&input);
    io::stdout().write_all(output.as_bytes())
}

fn main() {
    if let Err(error) = run()
        && error.kind() != io::ErrorKind::BrokenPipe
    {
        eprintln!("wide-md: {error}");
        std::process::exit(1);
    }
}
