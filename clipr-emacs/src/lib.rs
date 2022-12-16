// use async_std::{net::TcpStream, prelude::*, task};
use emacs::{defun, Env, Result, Value};

// Emacs won't load the module without this.
emacs::plugin_is_GPL_compatible!();

// Register the initialization hook that Emacs will call when it loads the module.
#[emacs::module]
fn init(env: &Env) -> Result<Value<'_>> {
    env.message("Done loading!")
}

// Define a function callable by Lisp code.
#[defun]
fn say_hello(env: &Env, name: String) -> Result<Value<'_>> {
    env.message(&format!("Hello, {}!", name))
}

// #[defun]
// fn call(env: &Env, cmd: String) -> Result<Value<'_>> {
//     let connect_to = format!(
//         "{}:{}",
//         &config.host.as_ref().unwrap(),
//         &config.port.unwrap()
//     );

//     let mut stream = TcpStream::connect(connect_to).await?;

//     write_command(&mut stream, cmd).await?;

//     let mut buf = String::new();
//     stream.read_to_string(&mut buf).await?;

//     Ok(Response::Data(buf))
// }
