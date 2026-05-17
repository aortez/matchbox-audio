use std::{
    io::{Read, Write},
    path::PathBuf,
};

use anyhow::Context;
use clap::Parser;
use mba_bt::{
    encode_frame, run_ble_gatt, BleGattOptions, FrameDecoder, MatchboxPlayerBackend, RequestRouter,
    RouteOutput,
};
use mba_protocol::{
    ErrorCode, ProtocolError, ProtocolMessage, DEFAULT_BT_CONTROL_SOCKET, DEFAULT_BT_STATE_DIR,
};
use reqwest::Url;

#[derive(Debug, Parser)]
#[command(about = "Local Matchbox Bluetooth protocol exerciser")]
struct Args {
    #[arg(long, help = "Read and write length-prefixed protocol frames on stdio")]
    stdio: bool,
    #[arg(long, help = "Advertise the local BLE GATT server")]
    ble_local: bool,
    #[arg(
        long,
        default_value = "http://127.0.0.1:8090",
        help = "mba-player base URL used by the real player backend"
    )]
    player_server: Url,
    #[arg(
        long,
        help = "Use a deterministic fake player backend instead of querying mba-player"
    )]
    fake_player: bool,
    #[arg(
        long,
        default_value = DEFAULT_BT_CONTROL_SOCKET,
        help = "Path for the local mba-bt control socket in BLE-local mode"
    )]
    control_socket: PathBuf,
    #[arg(long, help = "Disable the local mba-bt control socket")]
    no_control_socket: bool,
    #[arg(
        long,
        default_value = DEFAULT_BT_STATE_DIR,
        help = "Persistent Bluetooth trust/state directory in BLE-local mode"
    )]
    state_dir: PathBuf,
    #[arg(long, help = "Disable persistent Bluetooth state directory setup")]
    no_state_dir: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "mba_bt=info".into()),
        )
        .init();

    let args = Args::parse();
    if args.stdio == args.ble_local {
        anyhow::bail!("select exactly one mode: --stdio or --ble-local");
    }

    let player = player_backend(&args)?;
    if args.ble_local {
        let router = RequestRouter::new(player).with_build_version("mba-bt-ble-local");
        let mut options = BleGattOptions::default();
        options.control_socket = if args.no_control_socket {
            None
        } else {
            Some(args.control_socket)
        };
        options.state_dir = if args.no_state_dir {
            None
        } else {
            Some(args.state_dir)
        };
        return run_ble_gatt(router, options).await;
    }

    let router = RequestRouter::new(player).with_build_version("mba-bt-stdio");
    serve_stdio(router).await
}

fn player_backend(args: &Args) -> anyhow::Result<MatchboxPlayerBackend> {
    if args.fake_player {
        return Ok(MatchboxPlayerBackend::fake_ready());
    }

    MatchboxPlayerBackend::http(args.player_server.clone()).map_err(Into::into)
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
