#![feature(trait_alias)]
#![allow(stable_features)]
#![feature(let_else)] // stabilised in 1.65.0
#![feature(slice_take)]

use {
    anyhow::{anyhow, Result},
    bytes::Bytes,
    clap::{value_parser, Arg, ArgAction, Command},
    crossterm::{
        cursor,
        event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers},
        execute, style, terminal,
        tty::IsTty,
    },
    defer::defer,
    futures::{future::FutureExt, Stream},
    seahash::SeaHasher,
    std::{
        fmt::Formatter,
        fs::File,
        hash::Hasher,
        io::{BufRead, BufReader},
        path::Path,
        time::Duration,
    },
    tokio::{io::AsyncReadExt, sync::mpsc},
    tokio_serial::{SerialPortBuilderExt, SerialStream},
    tokio_stream::StreamExt,
};

// mod utf8_codec;

trait Writable = std::io::Write + Send;
trait ThePath = AsRef<Path> + std::fmt::Display + Clone + Sync + Send + 'static;

trait FramedStream = Stream<Item = Result<Message, anyhow::Error>> + Unpin;

type Sender = mpsc::Sender<Result<Message>>;
type Receiver = mpsc::Receiver<Result<Message>>;

async fn expect(to_console2: &Sender, from_serial: &mut Receiver, m: &str) -> Result<()> {
    let mut s = String::new();
    for _x in m.chars() {
        let next_char = from_serial.recv().await;

        let Some(Ok(c)) = next_char else {
            return Err(anyhow!(
                "Failed to receive expected value {:?}: got empty buf",
                m,
            ));
        };

        match c {
            Message::Text(payload) => {
                s.push_str(&payload);
                to_console2.send(Ok(Message::Text(payload))).await?;
            }
            _ => unreachable!(),
        }
    }
    if s != m {
        return Err(anyhow!(
            "Failed to receive expected value {:?}: got {:?}",
            m,
            s
        ));
    }
    Ok(())
}

async fn load_kernel<P>(to_console2: &Sender, kernel: P) -> Result<(File, u64)>
where
    P: ThePath,
{
    to_console2
        .send(Ok(Message::Text("⏩ Loading kernel image\n".into())))
        .await?;

    let kernel_file = match std::fs::File::open(kernel.clone()) {
        Ok(file) => file,
        Err(_) => return Err(anyhow!("Couldn't open kernel file {}", kernel)),
    };
    let kernel_size: u64 = kernel_file.metadata()?.len();

    to_console2
        .send(Ok(Message::Text(format!(
            "⏩ .. {} ({} bytes)\n",
            kernel, kernel_size
        ))))
        .await?;

    Ok((kernel_file, kernel_size))
}

async fn send_kernel<P: ThePath>(
    to_console2: &Sender,
    to_serial: &Sender,
    from_serial: &mut Receiver,
    kernel: P,
) -> Result<()> {
    let (kernel_file, kernel_size) = load_kernel(to_console2, kernel).await?;

    to_console2
        .send(Ok(Message::Text("⏩ Sending image size\n".into())))
        .await?;
    to_serial
        .send(Ok(Message::Binary(Bytes::copy_from_slice(
            &kernel_size.to_le_bytes(),
        ))))
        .await?;

    // Wait for OK response
    expect(to_console2, from_serial, "OK").await?;

    to_console2
        .send(Ok(Message::Text("⏩ Sending kernel image\n".into())))
        .await?;

    let mut hasher = SeaHasher::new();
    let mut reader = BufReader::with_capacity(1, kernel_file);
    loop {
        let length = {
            let buf = reader.fill_buf()?;
            to_serial
                .send(Ok(Message::Binary(Bytes::copy_from_slice(buf))))
                .await?;
            hasher.write(buf);
            buf.len()
        };
        if length == 0 {
            break;
        }
        reader.consume(length);
    }
    let hashed_value: u64 = hasher.finish();

    to_console2
        .send(Ok(Message::Text(format!(
            "⏩ Sending image checksum {:x}\n",
            hashed_value
        ))))
        .await?;

    to_serial
        .send(Ok(Message::Binary(Bytes::copy_from_slice(
            &hashed_value.to_le_bytes(),
        ))))
        .await?;

    expect(to_console2, from_serial, "OK").await?;

    Ok(())
}

// Async reading using Tokio: https://fasterthanli.me/articles/a-terminal-case-of-linux

async fn serial_loop(
    mut port: tokio_serial::SerialStream,
    to_console: Sender,
    mut from_console: Receiver,
) -> Result<()> {
    let mut buf = [0; 256];
    loop {
        tokio::select! {
            // _ = poll_send => {},

            Some(msg) = from_console.recv() => {
                // debug!("serial write {} bytes", msg.len());
                match msg.unwrap() {
                    Message::Text(s) => {
                        tokio::io::AsyncWriteExt::write_all(&mut port, s.as_bytes()).await?;
                    },
                    Message::Binary(b) => tokio::io::AsyncWriteExt::write_all(&mut port, b.as_ref()).await?,
                }
             }

            res = port.read(&mut buf) => {
                match res {
                    Ok(0) => {
                        // info!("Serial <EOF>");
                        return Ok(());
                    }
                    Ok(n) => {
                        // debug!("Serial read {n} bytes.");
                        // let codec = Utf8Codec::new(buf);
                        let s = String::from_utf8_lossy(&buf[0..n]);
                        to_console.send(Ok(Message::Text(s.to_string()))).await?;
                    }
                    Err(e) => {
            //             if e.kind() == ErrorKind::TimedOut {
            //                 execute!(w, style::Print("\r\nTimeout: the serial device has been unplugged!"))?;
            //             } else {
            //                 execute!(w, style::Print(format!("\r\nSerial Error: {:?}\r", e)))?;
            //             }
            //             break;
                        return Err(anyhow!(e));
                    }
                }
            }
        }
    }
}

// Always send Binary() to serial
// Convert Text() to bytes and send in serial_loop
// Receive and convert bytes to Text() in serial_loop
#[derive(Clone, Debug)]
enum Message {
    Binary(Bytes),
    Text(String),
}

// impl Message {
//     pub fn len(&self) -> usize {
//         match self {
//             Message::Binary(b) => b.len(),
//             Message::Text(s) => s.len(),
//         }
//     }
// }

impl std::fmt::Display for Message {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Message::Binary(b) => {
                for c in b {
                    write!(f, "{})", c)?;
                }
                Ok(())
            }
            Message::Text(s) => write!(f, "{}", s),
        }
    }
}

// impl Buf for Message {
//     fn remaining(&self) -> usize {
//         todo!()
//     }
//
//     fn chunk(&self) -> &[u8] {
//         todo!()
//     }
//
//     fn advance(&mut self, cnt: usize) {
//         todo!()
//     }
// }

async fn console_loop<P>(
    to_console2: Sender,
    mut from_internal: Receiver,
    to_serial: Sender,
    mut from_serial: Receiver,
    kernel: P,
) -> Result<()>
where
    P: ThePath,
{
    let mut w = std::io::stdout();

    let mut breaks = 0;

    let mut event_reader = EventStream::new();

    loop {
        tokio::select! {
            biased;

            Some(received) = from_internal.recv() => {
                if let Ok(message) = received {
                    execute!(w, style::Print(message))?;
                    w.flush()?;
                }
            }

            Some(received) = from_serial.recv() => { // returns Vec<char>
                if let Ok(received) = received {
                    let Message::Text(received) = received else {
                        unreachable!();
                    };
                    execute!(w, cursor::MoveToNextLine(1), style::Print(format!("[>>] Received {} bytes from serial", received.len())), cursor::MoveToNextLine(1))?;

                    for x in received.chars() {
                        if x == 0x3 as char {
                            // execute!(w, cursor::MoveToNextLine(1), style::Print("[>>] Received a BREAK"), cursor::MoveToNextLine(1))?;
                            breaks += 1;
                            // Await for 3 consecutive \3 to start downloading
                            if breaks == 3 {
                                // execute!(w, cursor::MoveToNextLine(1), style::Print("[>>] Received 3 BREAKs"), cursor::MoveToNextLine(1))?;
                                breaks = 0;
                                send_kernel(&to_console2, &to_serial, &mut from_serial, kernel.clone()).await?;
                                to_console2.send(Ok(Message::Text("🦀 Send successful, pass-through\n".into()))).await?;
                            }
                        } else {
                            while breaks > 0 {
                                execute!(w, style::Print(format!("{}", 3 as char)))?;
                                breaks -= 1;
                            }
                            // TODO decode buf with Utf8Codec here?
                            execute!(w, style::Print(format!("{}", x)))?;
                            w.flush()?;
                        }
                    }
                }
            }

            maybe_event = event_reader.next().fuse() => {
                match maybe_event {
                    Some(Ok(Event::Key(key_event))) => {
                        if key_event.code == KeyCode::Char('c') && key_event.modifiers == KeyModifiers::CONTROL {
                            return Ok(());
                        }
                        if let Some(key) = handle_key_event(key_event) {
                            to_serial.send(Ok(Message::Binary(Bytes::copy_from_slice(&key)))).await?;
                            // Local echo
                            execute!(w, style::Print(format!("{:?}", key)))?;
                            w.flush()?;
                        }
                    }
                    Some(Ok(_)) => {},
                    Some(Err(e)) => {
                        execute!(w, style::Print(format!("Console read error: {:?}\r", e)))?;
                        w.flush()?;
                    },
                    None => return Err(anyhow!("woops")),
                }
            }
        }
    }
}

async fn main_loop<P>(port: SerialStream, kernel: P) -> Result<()>
where
    P: ThePath,
{
    // read from serial -> to_console==>from_serial -> output to console
    let (to_console, from_serial) = mpsc::channel::<Result<Message>>(256);
    let (to_console2, from_internal) = mpsc::channel::<Result<Message>>(256);

    // Make a Stream from Receiver
    // let stream = ReceiverStream::new(from_serial);
    // // Make AsyncRead from Stream
    // let async_stream = StreamReader::new(stream);
    // // Make FramedRead (Stream+Sink) from AsyncRead
    // let from_serial = FramedRead::new(async_stream, Utf8Codec::new());

    // read from console -> to_serial==>from_console -> output to serial
    let (to_serial, from_console) = mpsc::channel(256);

    tokio::spawn(serial_loop(port, to_console.clone(), from_console));
    console_loop(to_console2, from_internal, to_serial, from_serial, kernel).await

    // TODO: framed

    // rx_device -> serial_reader -> app
    // app -> serial_writer -> serial_consumer -> (poll_send to drive) -> serial_sink -> tx_device
    // let (rx_device, tx_device) = split(port);

    // let mut serial_reader = FramedRead::new(rx_device, BytesCodec::new());
    // let serial_sink = FramedWrite::new(tx_device, BytesCodec::new());
    //
    // let (serial_writer, serial_consumer) = mpsc::unbounded::<Bytes>();
    // let mut poll_send = serial_consumer.map(Ok).forward(serial_sink);
}

// From remote_serial -- https://github.com/zhp-rs/remote_serial/ (Licensed under MIT License)
fn handle_key_event(key_event: KeyEvent) -> Option<Bytes> {
    let mut buf = [0; 4];

    let key_str: Option<&[u8]> = match key_event.code {
        KeyCode::Backspace => Some(b"\x08"),
        KeyCode::Enter => Some(b"\x0D"),
        KeyCode::Left => Some(b"\x1b[D"),
        KeyCode::Right => Some(b"\x1b[C"),
        KeyCode::Home => Some(b"\x1b[H"),
        KeyCode::End => Some(b"\x1b[F"),
        KeyCode::Up => Some(b"\x1b[A"),
        KeyCode::Down => Some(b"\x1b[B"),
        KeyCode::Tab => Some(b"\x09"),
        KeyCode::Delete => Some(b"\x1b[3~"),
        KeyCode::Insert => Some(b"\x1b[2~"),
        KeyCode::Esc => Some(b"\x1b"),
        KeyCode::Char(ch) => {
            if key_event.modifiers & KeyModifiers::CONTROL == KeyModifiers::CONTROL {
                buf[0] = ch as u8;
                if ch.is_ascii_lowercase() || (ch == ' ') {
                    buf[0] &= 0x1f;
                    Some(&buf[0..1])
                } else if ('4'..='7').contains(&ch) {
                    // crossterm returns Control-4 thru 7 for \x1c thru \x1f
                    buf[0] = (buf[0] + 8) & 0x1f;
                    Some(&buf[0..1])
                } else {
                    Some(ch.encode_utf8(&mut buf).as_bytes())
                }
            } else {
                Some(ch.encode_utf8(&mut buf).as_bytes())
            }
        }
        _ => None,
    };
    key_str.map(Bytes::copy_from_slice)
}

// 1. connect to given serial port, e.g. /dev/ttyUSB23234
// 2. Await for \3\3\3 start signal, meanwhile pass-through all traffic to console
// 3. send selected kernel binary with checksum to the target
// 4. go to 2

#[tokio::main]
async fn main() -> Result<()> {
    let matches = Command::new("ChainOfCommand - command chainboot protocol")
        .about("Use to send freshly built kernel to chainboot-compatible boot loader")
        .disable_version_flag(true)
        .arg(
            Arg::new("port")
                .help("The device path to a serial port, e.g. /dev/ttyUSB0")
                .required(true),
        )
        .arg(
            Arg::new("baud")
                .help("The baud rate to connect at")
                .use_value_delimiter(false)
                .action(ArgAction::Set)
                .value_parser(value_parser!(u32))
                .required(true), // .validator(valid_baud),
        )
        .arg(
            Arg::new("kernel")
                .long("kernel")
                .help("Path of the binary kernel image to send")
                .default_value("kernel8.img"),
        )
        .get_matches();
    let port_name = matches
        .get_one::<String>("port")
        .expect("port must be specified");
    let baud_rate = matches
        .get_one("baud")
        .copied()
        .expect("baud rate must be an integer");
    let kernel = matches
        .get_one::<String>("kernel")
        .expect("kernel file must be specified");

    // Check that STDIN is a proper tty
    if !std::io::stdin().is_tty() {
        panic!("Must have a TTY for stdin");
    }

    // Disable line buffering, local echo, etc.
    terminal::enable_raw_mode()?;
    defer!(terminal::disable_raw_mode().unwrap_or(()));

    let mut serial_toggle = false;
    let mut stdout = std::io::stdout();

    execute!(stdout, cursor::SavePosition)?;

    loop {
        execute!(
            stdout,
            cursor::RestorePosition,
            style::Print("⏩ Opening serial port       ")
        )?;

        // tokio_serial::new() creates a builder with 8N1 setup without flow control by default.
        let port = tokio_serial::new(port_name, baud_rate).open_native_async();
        if let Err(e) = port {
            let cont = match e.kind {
                tokio_serial::ErrorKind::NoDevice => true,
                tokio_serial::ErrorKind::Io(e)
                    if e == std::io::ErrorKind::NotFound
                        || e == std::io::ErrorKind::PermissionDenied =>
                {
                    true
                }
                _ => false,
            };
            if cont {
                execute!(
                    stdout,
                    cursor::RestorePosition,
                    style::Print(format!(
                        "⏳ Waiting for serial port {}\r",
                        if serial_toggle { "# " } else { " #" }
                    ))
                )?;
                stdout.flush()?;
                serial_toggle = !serial_toggle;

                if crossterm::event::poll(Duration::from_millis(1000))? {
                    if let Event::Key(KeyEvent {
                        code, modifiers, ..
                    }) = crossterm::event::read()?
                    {
                        if code == KeyCode::Char('c') && modifiers == KeyModifiers::CONTROL {
                            return Ok(());
                        }
                    }
                }

                continue;
            }
            return Err(e.into());
        }

        execute!(
            stdout,
            style::Print("\n✅ Waiting for handshake, pass-through. 🔌 Power the target now."),
        )?;
        stdout.flush()?;

        // Run in pass-through mode by default.
        // Once we receive BREAK (0x3) three times, switch to kernel send mode and upload kernel,
        // then switch back to pass-through mode.

        // Input from STDIN should pass through to serial
        // Input from serial should pass through to STDOUT

        let port = port?;

        if let Err(e) = main_loop(port, kernel.clone()).await {
            execute!(stdout, style::Print(format!("\nError: {:?}\n", e)))?;
            stdout.flush()?;

            let cont = matches!(e.downcast_ref::<std::io::Error>(),
                Some(e) if e.kind() == std::io::ErrorKind::NotFound || e.kind() == std::io::ErrorKind::PermissionDenied)
                || matches!(e.downcast_ref::<tokio_serial::Error>(), Some(e) if e.kind == tokio_serial::ErrorKind::NoDevice)
                || e.downcast_ref::<tokio::sync::mpsc::error::SendError<Vec<u8>>>()
                    .is_some();

            if !cont {
                break;
            }
        } else {
            // main_loop() returned Ok() we're good to finish
            break;
        }
        execute!(stdout, cursor::SavePosition)?;
    }

    Ok(())
}
