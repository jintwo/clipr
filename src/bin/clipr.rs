use async_std::{io, net::TcpStream, prelude::*, task};
use clap::Parser;
use cliprd::common::{load_config, write_command, Args, Command, Config, Response};
use std::sync::Arc;

async fn call(config: Arc<Config>, cmd: Command) -> io::Result<Response> {
    let connect_to = format!(
        "{}:{}",
        &config.host.as_ref().unwrap(),
        &config.port.unwrap()
    );

    let mut stream = TcpStream::connect(connect_to).await?;

    write_command(&mut stream, cmd).await?;

    let mut buf = String::new();
    stream.read_to_string(&mut buf).await?;

    Ok(Response::Data(buf))
}

fn show_response(response: &Response) {
    match response {
        Response::Data(buf) => println!("{}", buf),
        _ => println!("..."),
    }
}

fn main() -> io::Result<()> {
    let args = Args::parse();
    let config = Arc::new(if let Some(filename) = args.config.as_deref() {
        load_config(filename)?
    } else {
        Config::default()
    });

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
