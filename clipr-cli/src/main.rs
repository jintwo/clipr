use anyhow::{bail, Result};
use clap::Parser;
use clipr_common::{Args, Command, Config, Payload};
use socket2::{Domain, Socket, Type};
use std::io::Read;
use std::net::{SocketAddr, TcpStream};
use std::{io::Write, sync::Arc, time::Duration};

fn client(listen_on: &str, cmd: Command) -> Result<Payload> {
    let socket = Socket::new(Domain::IPV4, Type::STREAM, None)?;
    socket.set_reuse_port(true)?;
    socket.set_reuse_address(true)?;
    let address: SocketAddr = listen_on.parse()?;
    socket.connect(&address.into())?;
    let stream: TcpStream = socket.into();
    let cmd_body = serde_json::to_string(&cmd)?;
    stream.write_all("POST /command HTTP/1.1\r\n\r\n".as_bytes())?;
    stream.write_all(cmd_body.as_bytes())?;
    stream.flush()?;
    // stream.read_to_string
    // TODO:
    // +1. serialize command
    // +2. write data
    // +3. flush
    // 4. read response
    // 5. deserialize response
}

// TODO: move to common (http?)
fn call(config: Arc<Config>, cmd: Command) -> Result<Payload> {
    let uri = format!("http://{}/command", config.listen_on());
    let rep = reqwest::blocking::Client::new()
        .post(uri)
        .timeout(Duration::from_secs(2))
        .json(&cmd)
        .send()?
        .json::<Payload>()?;
    Ok(rep)
}

fn main() -> Result<()> {
    let args = Args::parse();
    let config = Arc::new(Config::load_from_args(&args)?);

    if let Some(cmd) = args.command {
        match call(config, cmd) {
            Ok(payload) => println!("{}", String::from(&payload)),
            Err(err) => bail!(err),
        }
    }

    Ok(())
}
