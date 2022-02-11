#![feature(trait_alias)]

use {
    anyhow::{anyhow, Result},
    bytes::Bytes,
    clap::{App, AppSettings, Arg},
    crossterm::{
        cursor,
        event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers},
        execute, style, terminal,
        tty::IsTty,
    },
    defer::defer,
    futures::{future::FutureExt, StreamExt},
    seahash::SeaHasher,
    std::{
        fs::File,
        hash::Hasher,
        io::{BufRead, BufReader},
        path::Path,
        time::Duration,
    },
    tokio::{io::AsyncReadExt, sync::mpsc},
    tokio_serial::{SerialPortBuilderExt, SerialStream},
};

trait Writable = std::io::Write + Send;
trait ThePath = AsRef<Path> + std::fmt::Display + Clone + Sync + Send + 'static;

async fn expect(
    to_console2: &mpsc::Sender<Vec<u8>>,
    from_serial: &mut mpsc::Receiver<Vec<u8>>,
    m: &str,
) -> Result<()> {
    if let Some(buf) = from_serial.recv().await {
        if buf.len() == m.len() && String::from_utf8_lossy(buf.as_ref()) == m {
            return Ok(());
        }
        to_console2.send(buf).await?;
        return Err(anyhow!("Failed to receive expected value"));
    }
    Err(anyhow!("Failed to receive expected value"))
}

async fn load_kernel<P>(to_console2: &mpsc::Sender<Vec<u8>>, kernel: P) -> Result<(File, u64)>
where
    P: ThePath,
{
    to_console2
        .send("[>>] Loading kernel image\n".into())
        .await?;

    let kernel_file = match std::fs::File::open(kernel.clone()) {
        Ok(file) => file,
        Err(_) => return Err(anyhow!("Couldn't open kernel file {}", kernel)),
    };
    let kernel_size: u64 = kernel_file.metadata()?.len();

    to_console2
        .send(format!("[>>] .. {} ({} bytes)\n", kernel, kernel_size).into())
        .await?;

    Ok((kernel_file, kernel_size))
}

async fn send_kernel<P>(
    to_console2: &mpsc::Sender<Vec<u8>>,
    to_serial: &mpsc::Sender<Vec<u8>>,
    from_serial: &mut mpsc::Receiver<Vec<u8>>,
    kernel: P,
) -> Result<()>
where
    P: ThePath,
{
    let (kernel_file, kernel_size) = load_kernel(to_console2, kernel).await?;

    to_console2.send("[>>] Sending image size\n".into()).await?;

    to_serial.send(kernel_size.to_le_bytes().into()).await?;

    // Wait for OK response
    expect(to_console2, from_serial, "OK").await?;

    to_console2
        .send("[>>] Sending kernel image\n".into())
        .await?;

    let mut hasher = SeaHasher::new();
    let mut reader = BufReader::with_capacity(1, kernel_file);
    loop {
        let length = {
            let buf = reader.fill_buf()?;
            to_serial.send(buf.into()).await?;
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
        .send(format!("[>>] Sending image checksum {:x}\n", hashed_value).into())
        .await?;

    to_serial.send(hashed_value.to_le_bytes().into()).await?;

    expect(to_console2, from_serial, "OK").await?;

    Ok(())
}

// Async reading using Tokio: https://fasterthanli.me/articles/a-terminal-case-of-linux

async fn serial_loop(
    mut port: tokio_serial::SerialStream,
    to_console: mpsc::Sender<Vec<u8>>,
    mut from_console: mpsc::Receiver<Vec<u8>>,
) -> Result<()> {
    let mut buf = [0; 256];
    loop {
        tokio::select! {
            // _ = poll_send => {},

            Some(msg) = from_console.recv() => {
                // debug!("serial write {} bytes", msg.len());
                tokio::io::AsyncWriteExt::write_all(&mut port, msg.as_ref()).await?;
            }

            res = port.read(&mut buf) => {
                match res {
                    Ok(0) => {
                        // info!("Serial <EOF>");
                        return Ok(());
                    }
                    Ok(n) => {
                        // debug!("Serial read {n} bytes.");
                        to_console.send(buf[0..n].to_owned()).await?;
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

async fn console_loop<P>(
    to_console2: mpsc::Sender<Vec<u8>>,
    mut from_internal: mpsc::Receiver<Vec<u8>>,
    to_serial: mpsc::Sender<Vec<u8>>,
    mut from_serial: mpsc::Receiver<Vec<u8>>,
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
                for &x in &received[..] {
                    execute!(w, style::Print(format!("{}", x as char)))?;
                }
                w.flush()?;
            }

            Some(received) = from_serial.recv() => {
                // execute!(w, cursor::MoveToNextLine(1), style::Print(format!("[>>] Received {} bytes from serial", from_serial.len())), cursor::MoveToNextLine(1))?;

                for &x in &received[..] {
                    if x == 0x3 {
                        // execute!(w, cursor::MoveToNextLine(1), style::Print("[>>] Received a BREAK"), cursor::MoveToNextLine(1))?;
                        breaks += 1;
                        // Await for 3 consecutive \3 to start downloading
                        if breaks == 3 {
                            // execute!(w, cursor::MoveToNextLine(1), style::Print("[>>] Received 3 BREAKs"), cursor::MoveToNextLine(1))?;
                            breaks = 0;
                            send_kernel(&to_console2, &to_serial, &mut from_serial, kernel.clone()).await?;
                            to_console2.send("[>>] Send successful, pass-through\n".into()).await?;
                        }
                    } else {
                        while breaks > 0 {
                            execute!(w, style::Print(format!("{}", 3 as char)))?;
                            breaks -= 1;
                        }
                        execute!(w, style::Print(format!("{}", x as char)))?;
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
                            to_serial.send(key.to_vec()).await?;
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
    let (to_console, from_serial) = mpsc::channel(256);
    let (to_console2, from_internal) = mpsc::channel(256);

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
                if ('a'..='z').contains(&ch) || (ch == ' ') {
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
    let matches = App::new("ChainOfCommand - command chainboot protocol")
        .about("Use to send freshly built kernel to chainboot-compatible boot loader")
        .setting(AppSettings::DisableVersionFlag)
        .arg(
            Arg::new("port")
                .help("The device path to a serial port, e.g. /dev/ttyUSB0")
                .required(true),
        )
        .arg(
            Arg::new("baud")
                .help("The baud rate to connect at")
                .use_delimiter(false)
                .required(true), // .validator(valid_baud),
        )
        .arg(
            Arg::new("kernel")
                .long("kernel")
                .help("Path of the binary kernel image to send")
                .takes_value(true)
                .default_value("kernel8.img"),
        )
        .get_matches();
    let port_name = matches.value_of("port").unwrap();
    let baud_rate = matches.value_of("baud").unwrap().parse::<u32>().unwrap();
    let kernel = matches.value_of("kernel").unwrap().to_owned();

    // Check that STDIN is a proper tty
    if !std::io::stdin().is_tty() {
        panic!("Must have a TTY for stdin");
    }

    // Disable line buffering, local echo, etc.
    terminal::enable_raw_mode()?;
    defer(|| terminal::disable_raw_mode().unwrap_or(()));

    let mut serial_toggle = false;
    let mut stdout = std::io::stdout();

    execute!(stdout, cursor::SavePosition)?;

    loop {
        execute!(
            stdout,
            cursor::RestorePosition,
            style::Print("[>>] Opening serial port       ")
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
                        "[>>] Waiting for serial port {}\r",
                        if serial_toggle { "# " } else { " #" }
                    ))
                )?;
                stdout.flush()?;
                serial_toggle = !serial_toggle;

                if crossterm::event::poll(Duration::from_millis(1000))? {
                    if let Event::Key(KeyEvent { code, modifiers }) = crossterm::event::read()? {
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
            style::Print("\n[>>] Waiting for handshake, pass-through"),
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

            let cont = match e.downcast_ref::<std::io::Error>() {
                Some(e)
                    if e.kind() == std::io::ErrorKind::NotFound
                        || e.kind() == std::io::ErrorKind::PermissionDenied =>
                {
                    true
                }
                _ => false,
            } || matches!(e.downcast_ref::<tokio_serial::Error>(), Some(e) if e.kind == tokio_serial::ErrorKind::NoDevice)
                || matches!(
                    e.downcast_ref::<tokio::sync::mpsc::error::SendError<Vec<u8>>>(),
                    Some(_)
                );

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
