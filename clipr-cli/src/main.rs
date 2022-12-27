use anyhow::Result;
use async_std::{net::TcpStream, prelude::*, task};
use clap::Parser;
use clipr_common::{write_command, Args, Command, Config, Response};
use std::sync::Arc;

async fn call(config: Arc<Config>, cmd: Command) -> Result<Response> {
    let connect_to = format!(
        "{}:{}",
        &config.host.as_ref().unwrap(),
        &config.raw_port.unwrap()
    );

    let mut stream = TcpStream::connect(connect_to).await?;

    write_command(&mut stream, cmd).await?;

    let mut buf = String::new();
    stream.read_to_string(&mut buf).await?;

    Ok(Response::Text(buf))
}

fn show_response(response: &Response) {
    match response {
        Response::Text(buf) => println!("{}", buf),
        _ => println!("..."),
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    let config = Arc::new(Config::load_from_args(&args)?);

    match args.command {
        Some(cmd) => match task::block_on(call(config, cmd)) {
            Ok(response) => {
                show_response(&response);
                Ok(())
            }
            Err(err) => Err(err),
        },
        None => Ok(()),
    }
}
