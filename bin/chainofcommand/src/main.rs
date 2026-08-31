#![feature(trait_alias)]
#![feature(drop_guard)]

use {
    anyhow::{Result, anyhow},
    bytes::Bytes,
    crossterm::{
        cursor,
        event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers},
        execute, style, terminal,
        tty::IsTty,
    },
    futures::{Stream, future::FutureExt},
    seahash::SeaHasher,
    std::{
        fmt::Formatter,
        fs::File,
        hash::Hasher,
        io::{BufRead, BufReader},
        path::Path,
        time::Duration,
    },
    tokio::{
        io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
        net::TcpStream,
        sync::mpsc,
    },
    tokio_serial::{SerialPortBuilderExt, SerialStream},
    tokio_stream::StreamExt,
};

trait Writable = std::io::Write + Send;
trait ThePath = AsRef<Path> + std::fmt::Display + Clone + Sync + Send + 'static;

trait FramedStream = Stream<Item = Result<Message, anyhow::Error>> + Unpin;

type Sender = mpsc::Sender<Result<Message>>;
type Receiver = mpsc::Receiver<Result<Message>>;

async fn expect(to_console2: &Sender, from_serial: &mut Receiver, m: &str) -> Result<()> {
    // Accumulate chunks until we have enough chars for an exact match.
    // Chunk boundaries are arbitrary (TCP/serial coalescing): "OK" may arrive
    // as one chunk or split across several, so we can't pull one chunk per char.
    let mut s = String::new();
    let want = m.chars().count();
    while s.chars().count() < want {
        // to_console2
        //     .send(Ok(Message::Text(format!(
        //         "\r\n[expect {m:?}: waiting, have {s:?}]"
        //     ))))
        //     .await?;

        let next_char = from_serial.recv().await;

        let Some(Ok(c)) = next_char else {
            return Err(anyhow!(
                "Failed to receive expected value {m:?}: got empty buf (accumulated {s:?})"
            ));
        };

        match c {
            Message::Text(payload) => {
                // to_console2
                //     .send(Ok(Message::Text(format!(
                //         "\r\n[expect {m:?}: got chunk {payload:?}]",
                //     ))))
                //     .await?;
                s.push_str(&payload);
                to_console2.send(Ok(Message::Text(payload))).await?;
            }
            Message::Binary(_) => unreachable!(),
        }
    }
    if s != m {
        return Err(anyhow!("Failed to receive expected value {m:?}: got {s:?}"));
    }
    // to_console2
    //     .send(Ok(Message::Text(format!("\r\n[expect {m:?}: matched]"))))
    //     .await?;
    Ok(())
}

async fn load_kernel<P>(to_console2: &Sender, kernel: P) -> Result<(File, u64)>
where
    P: ThePath,
{
    to_console2
        .send(Ok(Message::Text("⏩ Loading kernel image\r\n".into())))
        .await?;

    let Ok(kernel_file) = std::fs::File::open(kernel.clone()) else {
        return Err(anyhow!("Couldn't open kernel file {kernel}"));
    };
    let kernel_size = kernel_file.metadata()?.len();

    to_console2
        .send(Ok(Message::Text(format!(
            "⏩ .. {kernel} ({kernel_size} bytes)\r\n"
        ))))
        .await?;

    Ok((kernel_file, kernel_size))
}

async fn send_kernel<P: ThePath>(
    to_console2: Sender,
    to_serial: Sender,
    mut from_serial: Receiver,
    kernel: P,
) -> Result<()> {
    let (kernel_file, kernel_size) = load_kernel(&to_console2, kernel).await?;

    to_console2
        .send(Ok(Message::Text("⏩ Sending image size\r\n".into())))
        .await?;
    to_serial
        .send(Ok(Message::Binary(Bytes::copy_from_slice(
            &kernel_size.to_le_bytes(),
        ))))
        .await?;

    // Wait for OK response
    expect(&to_console2, &mut from_serial, "OK").await?;

    to_console2
        .send(Ok(Message::Text("⏩ Sending kernel image\r\n".into())))
        .await?;
    let to_console2 = &to_console2;
    let to_serial = &to_serial;

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
            "⏩ Sending image checksum {hashed_value:x}\r\n"
        ))))
        .await?;

    to_serial
        .send(Ok(Message::Binary(Bytes::copy_from_slice(
            &hashed_value.to_le_bytes(),
        ))))
        .await?;

    expect(to_console2, &mut from_serial, "OK").await?;

    Ok(())
}

// Async reading using Tokio: https://fasterthanli.me/articles/a-terminal-case-of-linux

async fn serial_loop<T>(mut port: T, to_console: Sender, mut from_console: Receiver) -> Result<()>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    let mut buf = [0; 256];
    loop {
        tokio::select! {
            // _ = poll_send => {},

            Some(msg) = from_console.recv() => {
                // debug!("serial write {} bytes", msg.len());
                match msg.unwrap() {
                    Message::Text(s) => {
                        port.write_all(s.as_bytes()).await?;
                    },
                    Message::Binary(b) => port.write_all(b.as_ref()).await?,
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

impl std::fmt::Display for Message {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Message::Binary(b) => {
                for c in b {
                    write!(f, "{c})")?;
                }
                Ok(())
            }
            Message::Text(s) => write!(f, "{s}"),
        }
    }
}

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

    // Protocol channel: while an upload task is running, serial bytes are
    // routed here instead of being printed, so send_kernel can await the
    // target's replies without starving the select loop's other branches.
    let (mut to_proto, from_proto) = mpsc::channel::<Result<Message>>(256);
    let mut from_proto = Some(from_proto);
    let mut upload: Option<tokio::task::JoinHandle<Result<()>>> = None;

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
                    // While an upload is in flight, hand bytes to the protocol task.
                    // If the task already finished (receiver dropped), fall through to
                    // pass-through printing instead of failing with "channel closed".
                    if upload.is_some() && to_proto.send(Ok(received.clone())).await.is_ok() {
                        continue;
                    }

                    let Message::Text(received) = received else {
                        unreachable!();
                    };
                    // execute!(w, style::Print(format!("⏩ Received {} bytes from serial\r\n", received.len())))?;

                    for x in received.chars() {
                        if x == 0x3 as char {
                            // execute!(w, style::Print("⏩ Received a BREAK\r\n"))?;
                            breaks += 1;
                            // Await for 3 consecutive \3 to start downloading
                            if breaks == 3 {
                                // execute!(w, style::Print("⏩ Received 3 BREAKs\r\n"))?;
                                w.flush()?;
                                breaks = 0;
                                // Spawn the upload so this loop keeps polling
                                // from_internal (status messages) and stdin.
                                let handle = tokio::spawn(send_kernel(
                                    to_console2.clone(),
                                    to_serial.clone(),
                                    from_proto.take().expect("proto receiver already taken"),
                                    kernel.clone(),
                                ));
                                upload = Some(handle);
                            }
                        } else {
                            while breaks > 0 {
                                execute!(w, style::Print(format!("{}", 3 as char)))?;
                                breaks -= 1;
                            }
                            // TODO decode buf with Utf8Codec here?
                            execute!(w, style::Print(format!("{x}")))?;
                            w.flush()?;
                        }
                    }
                }
            }

            Some(result) = async { match &mut upload { Some(h) => Some(h.await), None => None } } => {
                upload = None;
                // The task consumed from_proto; recreate the channel for the next round.
                let (tx, rx) = mpsc::channel::<Result<Message>>(256);
                to_proto = tx;
                from_proto = Some(rx);
                match result {
                    Ok(Ok(())) => {
                        to_console2.send(Ok(Message::Text("🦀 Send successful, pass-through\r\n".into()))).await?;
                    }
                    Ok(Err(e)) => {
                        execute!(w, style::Print(format!("\r\n\r\n❌ Upload failed: {e:?}\r\n")))?;
                        w.flush()?;
                    }
                    Err(join_err) => {
                        execute!(w, style::Print(format!("\r\n\r\n❌ Upload task panicked: {join_err:?}\r\n")))?;
                        w.flush()?;
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
                            execute!(w, style::Print(format!("{key:?}")))?;
                            w.flush()?;
                        }
                    }
                    Some(Ok(_)) => {},
                    Some(Err(e)) => {
                      execute!(w, style::Print(format!("Console read error: {e:?}\r\n")))?;
                        w.flush()?;
                    },
                    None => return Err(anyhow!("woops")),
                }
            }
        }
    }
}

async fn main_loop<T, P>(port: T, kernel: P) -> Result<()>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
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

/// `ChainOfCommand` - command chainboot protocol
///
/// Use to send freshly built kernel to chainboot-compatible boot loader.
#[derive(argh::FromArgs)]
struct Args {
    /// device path to a serial port, e.g. /dev/ttyUSB0
    #[argh(option, short = 'p')]
    port: Option<String>,
    /// baud rate to connect at
    #[argh(option, short = 'b')]
    baud: Option<u32>,
    /// path of the binary kernel image to send
    #[argh(option, short = 'k')]
    kernel: Option<String>,
    /// positional form: <port> [baud] [kernel]
    #[argh(positional)]
    positional: Vec<String>,
}

impl Args {
    fn resolve(self) -> anyhow::Result<(String, u32, String)> {
        let pos = |index: usize| self.positional.get(index).cloned();

        let port = self
            .port
            .or_else(|| pos(0))
            .ok_or_else(|| anyhow::anyhow!("missing serial port (first parameter or --port)"))?;
        let baud = match self.baud {
            Some(b) => b,
            None => match pos(1) {
                Some(s) => s
                    .parse()
                    .map_err(|e| anyhow::anyhow!("invalid baud rate: {e}"))?,
                None => 115200,
            },
        };
        let kernel = self
            .kernel
            .or_else(|| pos(2))
            .unwrap_or_else(|| String::from("kernel8.img"));
        Ok((port, baud, kernel))
    }
}

fn animated(step: &mut usize) -> char {
    let frames = ['⏳', '⌛'];
    if *step >= frames.len() {
        *step = 0;
    }
    let s = *step;
    *step += 1;
    frames[s]
}

enum OpenedPort {
    Serial(SerialStream),
    Raw(tokio::fs::File),
    Tcp(TcpStream),
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Args = argh::from_env();
    let (port, baud, kernel) = args.resolve()?;

    // Check that STDIN is a proper tty
    assert!(std::io::stdin().is_tty(), "Must have a TTY for stdin"); // TODO: relax this requirement

    // Disable line buffering, local echo, etc.
    terminal::enable_raw_mode()?;
    let _terminal_drop_guard =
        std::mem::DropGuard::new((), |()| terminal::disable_raw_mode().unwrap_or(()));

    let mut serial_step = 0_usize;
    let mut stdout = std::io::stdout();

    execute!(stdout, cursor::SavePosition)?;

    loop {
        execute!(
            stdout,
            cursor::RestorePosition,
            style::Print("⏩ Opening serial port       ")
        )?;

        let opened_port = if let Some(addr) = port.strip_prefix("tcp:") {
            // TCP transport, e.g. for QEMU's `-serial tcp:127.0.0.1:4321,server,nowait`.
            // Avoids macOS PTY quirks; retry while QEMU isn't listening yet.
            match TcpStream::connect(addr).await {
                Ok(stream) => {
                    stream.set_nodelay(true)?;
                    OpenedPort::Tcp(stream)
                }
                Err(e)
                    if e.kind() == std::io::ErrorKind::ConnectionRefused
                        || e.kind() == std::io::ErrorKind::NotFound =>
                {
                    execute!(
                        stdout,
                        cursor::RestorePosition,
                        style::Print(format!(
                            "{} Waiting for QEMU TCP port {addr}\r",
                            animated(&mut serial_step)
                        ))
                    )?;
                    stdout.flush()?;

                    if crossterm::event::poll(Duration::from_millis(1000))?
                        && let Event::Key(KeyEvent {
                            code, modifiers, ..
                        }) = crossterm::event::read()?
                        && code == KeyCode::Char('c')
                        && modifiers == KeyModifiers::CONTROL
                    {
                        return Ok(());
                    }

                    continue;
                }
                Err(e) => return Err(e.into()),
            }
        } else {
            // tokio_serial::new() creates a builder with 8N1 setup without flow control by default.
            // On macOS, QEMU PTYs (/dev/ttys*) may reject serial ioctls with ENOTTY
            // ("Not a typewriter"). In that case, fall back to plain async file I/O.
            match tokio_serial::new(port.clone(), baud).open_native_async() {
                Ok(p) => OpenedPort::Serial(p),
                Err(e) => {
                    let should_wait = matches!(
                        e.kind,
                        tokio_serial::ErrorKind::NoDevice
                            | tokio_serial::ErrorKind::Io(
                                std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied
                            )
                    );

                    if should_wait {
                        execute!(
                            stdout,
                            cursor::RestorePosition,
                            style::Print(format!(
                                "{} Waiting for serial port {port}\r",
                                animated(&mut serial_step)
                            ))
                        )?;
                        stdout.flush()?;

                        if crossterm::event::poll(Duration::from_millis(1000))?
                            && let Event::Key(KeyEvent {
                                code, modifiers, ..
                            }) = crossterm::event::read()?
                            && code == KeyCode::Char('c')
                            && modifiers == KeyModifiers::CONTROL
                        {
                            return Ok(());
                        }

                        continue;
                    }

                    // macOS PTY fallback for QEMU -serial pty.
                    // Some PTYs reject serial ioctls with ENOTTY ("Not a typewriter"), but still
                    // work fine as a plain byte stream.
                    let is_macos_tty_path = cfg!(target_os = "macos")
                        && (port.starts_with("/dev/tty") || port.starts_with("/dev/cu"));

                    if is_macos_tty_path {
                        execute!(
                            stdout,
                            cursor::RestorePosition,
                            style::Print(format!(
                                "⚠️  serial open failed ({e}); trying raw stream fallback\r\n"
                            ))
                        )?;
                        stdout.flush()?;

                        let raw_open = tokio::fs::OpenOptions::new()
                            .read(true)
                            .write(true)
                            .open(&port)
                            .await;

                        match raw_open {
                            Ok(raw) => OpenedPort::Raw(raw),
                            Err(raw_e) => {
                                return Err(anyhow!(
                                    "serial open failed ({e}); raw fallback open failed ({raw_e})"
                                ));
                            }
                        }
                    } else {
                        return Err(e.into());
                    }
                }
            }
        };

        execute!(
            stdout,
            style::Print("\r\n✅ Waiting for handshake, pass-through. 🔌 Power on the target now."),
        )?;
        stdout.flush()?;

        // Run in pass-through mode by default.
        // Once we receive BREAK (0x3) three times, switch to kernel send mode and upload kernel,
        // then switch back to pass-through mode.

        // Input from STDIN should pass through to serial
        // Input from serial should pass through to STDOUT

        let loop_result = match opened_port {
            OpenedPort::Serial(port) => main_loop(port, kernel.clone()).await,
            OpenedPort::Raw(port) => main_loop(port, kernel.clone()).await,
            OpenedPort::Tcp(port) => main_loop(port, kernel.clone()).await,
        };

        if let Err(e) = loop_result {
            execute!(stdout, style::Print(format!("\nError: {e:?}\n")))?;
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
