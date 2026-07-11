use std::{
    io::{BufRead, BufReader, BufWriter, Read, Write},
    process::{ChildStdin, ChildStdout},
};

use anyhow::{Context, Result, anyhow};
use serde_json::Value;

pub(super) fn write_message(stdin: &mut BufWriter<ChildStdin>, payload: &Value) -> Result<()> {
    let body = serde_json::to_vec(payload)?;
    write!(stdin, "Content-Length: {}\r\n\r\n", body.len())?;
    stdin.write_all(&body)?;
    stdin.flush()?;
    Ok(())
}

pub(super) fn read_message(
    stdout: &mut BufReader<ChildStdout>,
    server_name: &str,
) -> Result<Value> {
    let mut content_length = None;

    loop {
        let mut header = String::new();
        let bytes = stdout.read_line(&mut header)?;
        if bytes == 0 {
            return Err(anyhow!("{} closed the LSP stream", server_name));
        }

        if header == "\r\n" || header == "\n" {
            break;
        }

        if let Some((name, value)) = header.split_once(':')
            && name.eq_ignore_ascii_case("Content-Length")
        {
            content_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .context("invalid LSP Content-Length header")?,
            );
        }
    }

    let length = content_length.context("missing LSP Content-Length header")?;
    let mut payload = vec![0_u8; length];
    stdout.read_exact(&mut payload)?;
    serde_json::from_slice(&payload).context("invalid LSP JSON payload")
}
