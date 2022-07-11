use async_std::{io, net::TcpStream, prelude::*, task};
use cliprd::common::{Command, CommandParseError, Response, HEADER_LEN};
use std::env;

async fn call(cmd: Command) -> io::Result<Response> {
    println!("command = {:?}", cmd);

    let mut stream = TcpStream::connect("127.0.0.1:8931").await?;

    let cmd_payload: Vec<u8> = cmd.into();
    let cmd_header = &cmd_payload.len().to_le_bytes()[0..HEADER_LEN];

    println!("header = {:?}, payload = {:?}", cmd_header, cmd_payload);

    stream.write_all(cmd_header).await?;
    stream.write_all(cmd_payload.as_slice()).await?;
    stream.flush().await?;

    let mut buf = String::new();
    stream.read_to_string(&mut buf).await?;

    Ok(Response::Data(buf))
}

fn parse_cmd() -> Result<Command, CommandParseError> {
    let args = env::args();
    if args.len() < 1 {
        return Err(CommandParseError::InsufficientArgs);
    }

    let cmd_raw = args.skip(1).collect::<Vec<String>>().join(" ");
    cmd_raw.parse::<Command>()
}

fn show_response(response: &Response) {
    match response {
        Response::Data(buf) => println!("{}", buf),
        _ => println!("..."),
    }
}
fn main() -> io::Result<()> {
    match parse_cmd() {
        Ok(cmd) => match task::block_on(call(cmd)) {
            Ok(response) => {
                show_response(&response);
                Ok(())
            }
            Err(err) => Err(err),
        },
        Err(_) => Err(io::Error::new(io::ErrorKind::Other, "invalid command")),
    }
}
