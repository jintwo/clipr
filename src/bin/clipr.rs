use async_std::{io, net::TcpStream, prelude::*, task};
use cliprd::common::{load_config, write_command, Command, CommandParseError, Config, Response};
use std::env;
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

fn parse_cmd() -> Result<Command, CommandParseError> {
    let args = env::args();
    if args.len() < 3 {
        return Err(CommandParseError::InsufficientArgs);
    }

    let cmd_raw = args.skip(2).collect::<Vec<String>>().join(" ");
    cmd_raw.parse::<Command>()
}

fn show_response(response: &Response) {
    match response {
        Response::Data(buf) => println!("{}", buf),
        _ => println!("..."),
    }
}

fn main() -> io::Result<()> {
    let args = env::args();
    let config = Arc::new(if args.len() < 2 {
        println!("using default config...");
        Config::default()
    } else {
        let config_filename = args.skip(1).nth(0).unwrap();
        let config = load_config(config_filename.as_str())?;
        config
    });

    println!("using config = {:?}", config);

    match parse_cmd() {
        Ok(cmd) => match task::block_on(call(config, cmd)) {
            Ok(response) => {
                show_response(&response);
                Ok(())
            }
            Err(err) => Err(err),
        },
        Err(_) => Err(io::Error::new(io::ErrorKind::Other, "invalid command")),
    }
}
