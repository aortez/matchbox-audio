use std::io::{Read, Write};

use anyhow::Context;
use clap::Parser;
use mba_bt::{encode_frame, FakePlayerBackend, FrameDecoder, RequestRouter, RouteOutput};
use mba_protocol::{ErrorCode, ProtocolError, ProtocolMessage};

#[derive(Debug, Parser)]
#[command(about = "Local Matchbox Bluetooth protocol exerciser")]
struct Args {
    #[arg(long, help = "Read and write length-prefixed protocol frames on stdio")]
    stdio: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    if !args.stdio {
        anyhow::bail!("no mode selected; use --stdio");
    }

    let router = RequestRouter::new(FakePlayerBackend::ready());
    serve_stdio(router).await
}

async fn serve_stdio<P>(router: RequestRouter<P>) -> anyhow::Result<()>
where
    P: mba_bt::PlayerBackend,
{
    let mut decoder = FrameDecoder::new();
    let mut stdin = std::io::stdin().lock();
    let mut stdout = std::io::stdout().lock();
    let mut read_buf = [0u8; 4096];

    loop {
        let n = stdin.read(&mut read_buf).context("failed to read stdin")?;
        if n == 0 {
            break;
        }

        let messages = match decoder.push(&read_buf[..n]) {
            Ok(messages) => messages,
            Err(error) => {
                write_output(
                    &mut stdout,
                    RouteOutput::single(ProtocolMessage::error_response(
                        0,
                        ProtocolError::new(ErrorCode::BadRequest, error.to_string()),
                    )),
                )?;
                continue;
            }
        };

        for message in messages {
            let output = router.route(message).await;
            write_output(&mut stdout, output)?;
        }
    }

    Ok(())
}

fn write_output<W>(writer: &mut W, output: RouteOutput) -> anyhow::Result<()>
where
    W: Write,
{
    for message in output.messages() {
        let frame = encode_frame(&message).context("failed to encode response frame")?;
        writer
            .write_all(&frame)
            .context("failed to write response frame")?;
    }
    writer.flush().context("failed to flush stdout")
}
