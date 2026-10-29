#![feature(trait_alias)]

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
    libchainboot_protocol::{
        BEACON, CONSOLE_ACK, CONSOLE_BEACON, HOST_CONSOLE_CONFIRM_TIMEOUT,
        HOST_FAST_BEACON_TIMEOUT, RATE_ACCEPTED, RateAnswer, Run, encode_rate_request,
        rate_answer,
    },
    seahash::SeaHasher,
    std::{
        collections::VecDeque,
        fmt::Formatter,
        fs::File,
        hash::Hasher,
        io::Read,
        path::Path,
        sync::Arc,
        time::{Duration, Instant},
    },
    tokio::{
        io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
        net::TcpStream,
        sync::{Notify, mpsc},
    },
    tokio_serial::{SerialPort, SerialPortBuilderExt, SerialStream},
    tokio_stream::StreamExt,
};

trait Writable = std::io::Write + Send;
trait ThePath = AsRef<Path> + std::fmt::Display + Clone + Sync + Send + 'static;

trait FramedStream = Stream<Item = Result<Message, anyhow::Error>> + Unpin;

type Sender = mpsc::Sender<Result<Message>>;
type Receiver = mpsc::Receiver<Result<Message>>;

/// How long the host waits for chainboot's answer to a rate request.
const RATE_ANSWER_TIMEOUT: Duration = Duration::from_secs(2);

/// How much of the kernel goes to the serial line at a time, so that the
/// progress line can follow.
const SEND_CHUNK: usize = 4096;

/// How often the progress line is redrawn, at most.
const PROGRESS_REDRAW: Duration = Duration::from_millis(100);

/// Over how long the transfer rate on the progress line is measured.
const RATE_WINDOW: Duration = Duration::from_secs(1);

/// How many garbage characters, with no newline between them, at a rate other
/// than the initial one mean the target restarted and talks at the initial rate.
const RESTART_GARBAGE: usize = 8;

/// How long the line may stay silent at a rate other than the initial one
/// before the host goes back to the initial rate: a restart whose output did
/// not read as garbage, or a target reset before chainofcommand started
/// listening, leaves chainboot beaconing at the initial rate unheard.
const SILENCE_FALLBACK: Duration = Duration::from_secs(30);

/// The target's answer: its first `length` characters.
///
/// What comes after them in the same chunk is the target going on ("OK" is
/// followed by its next message at once), not part of the answer. Every chunk
/// is passed on to the console as it arrives, the answer and the rest alike.
async fn receive_answer(
    to_console2: &Sender,
    from_serial: &mut Receiver,
    length: usize,
) -> Result<String> {
    // Accumulate chunks until we have enough chars for the answer.
    // Chunk boundaries are arbitrary (TCP/serial coalescing): "OK" may arrive
    // as one chunk or split across several, and may have more behind it, so we
    // can't pull one chunk per char.
    let mut answer = String::new();
    while answer.chars().count() < length {
        let Some(Ok(chunk)) = from_serial.recv().await else {
            return Err(anyhow!(
                "Failed to receive an answer: got empty buf (accumulated {answer:?})"
            ));
        };

        match chunk {
            Message::Text(payload) => {
                answer.push_str(&payload);
                to_console2.send(Ok(Message::Text(payload))).await?;
            }
            Message::Binary(_) | Message::SetRate(_) | Message::Written(_) => unreachable!(),
        }
    }
    Ok(answer)
}

/// Wait for the target to answer `m`.
///
/// Only the first characters of the pattern's length are the answer: see
/// [`receive_answer`].
async fn expect(to_console2: &Sender, from_serial: &mut Receiver, m: &str) -> Result<()> {
    let answer = receive_answer(to_console2, from_serial, m.chars().count())
        .await
        .map_err(|error| anyhow!("Failed to receive expected value {m:?}: {error}"))?;
    if !answer.starts_with(m) {
        return Err(anyhow!(
            "Failed to receive expected value {m:?}: got {answer:?}"
        ));
    }
    Ok(())
}

/// The byte a received character stands for, for watching for a beacon: only
/// ASCII characters are beacon bytes.
fn beacon_byte(character: char) -> u8 {
    u8::try_from(character)
        .ok()
        .filter(u8::is_ascii)
        .unwrap_or(u8::MAX)
}

/// Wait until `beacon` arrives. Anything else is the line settling at a new rate
/// and is dropped.
async fn wait_for_beacon(from_serial: &mut Receiver, beacon: [u8; 3]) -> Result<()> {
    let mut run = Run::of(beacon);
    loop {
        let Some(Ok(Message::Text(chunk))) = from_serial.recv().await else {
            return Err(anyhow!("The serial line closed while waiting for a beacon"));
        };
        if chunk.chars().any(|character| run.push(beacon_byte(character))) {
            return Ok(());
        }
    }
}

/// What the target does once it accepted a rate: the beacon it sends at the new
/// rate, how long the host waits for it, what the host answers it with, and what
/// the rate is for.
#[derive(Clone, Copy, Debug)]
struct AtNewRate {
    beacon: [u8; 3],
    timeout: Duration,
    answer: &'static [u8],
    purpose: &'static str,
}

/// Chainboot beacons for the load at the new rate; the size answers it.
const CHAINBOOT_LOAD: AtNewRate = AtNewRate {
    beacon: BEACON,
    timeout: HOST_FAST_BEACON_TIMEOUT,
    answer: &[],
    purpose: "Loading",
};

/// Kickstart beacons for [`CONSOLE_ACK`] at the new rate.
const KICKSTART_CONSOLE: AtNewRate = AtNewRate {
    beacon: CONSOLE_BEACON,
    timeout: HOST_CONSOLE_CONFIRM_TIMEOUT,
    answer: &CONSOLE_ACK,
    purpose: "Console",
};

/// Ask the target for `rate` (see the protocol crate's documentation) and switch
/// the host when it accepts. Returns whether both are at `rate`, the target's
/// beacon heard there (and answered, for the console). When not, the target goes
/// back to the boot rate, and the host has to go back to it.
async fn switch_rate(
    to_console2: &Sender,
    to_serial: &Sender,
    from_serial: &mut Receiver,
    rate: u32,
    at_new_rate: AtNewRate,
) -> Result<bool> {
    to_console2
        .send(Ok(Message::Text(format!("⏩ Asking for {rate} baud .. "))))
        .await?;
    to_serial
        .send(Ok(Message::Binary(Bytes::copy_from_slice(
            &encode_rate_request(rate),
        ))))
        .await?;

    let Ok(answer) = tokio::time::timeout(
        RATE_ANSWER_TIMEOUT,
        receive_answer(to_console2, from_serial, RATE_ACCEPTED.len()),
    )
    .await
    else {
        to_console2
            .send(Ok(Message::Text("no answer\r\n".into())))
            .await?;
        return Ok(false);
    };
    match rate_answer(&answer?) {
        RateAnswer::Accepted => {}
        RateAnswer::Refused => {
            to_console2
                .send(Ok(Message::Text(format!(
                    "\r\n⏩ The target refused {rate} baud\r\n"
                ))))
                .await?;
            return Ok(false);
        }
        RateAnswer::NotUnderstood => {
            to_console2
                .send(Ok(Message::Text(
                    "\r\n⏩ The target does not know rate requests\r\n".into(),
                )))
                .await?;
            return Ok(false);
        }
    }

    to_serial.send(Ok(Message::SetRate(rate))).await?;
    if let Ok(beacon) = tokio::time::timeout(
        at_new_rate.timeout,
        wait_for_beacon(from_serial, at_new_rate.beacon),
    )
    .await
    {
        beacon?;
        if !at_new_rate.answer.is_empty() {
            to_serial
                .send(Ok(Message::Binary(Bytes::from_static(at_new_rate.answer))))
                .await?;
        }
        to_console2
            .send(Ok(Message::Text(format!(
                "\r\n⏩ {} at {rate} baud\r\n",
                at_new_rate.purpose
            ))))
            .await?;
        Ok(true)
    } else {
        to_console2
            .send(Ok(Message::Text(format!(
                "\r\n⏩ No beacon at {rate} baud within {} ms\r\n",
                at_new_rate.timeout.as_millis()
            ))))
            .await?;
        Ok(false)
    }
}

/// Answer Kickstart's console beacon (the protocol crate's "The console
/// negotiation"): ask for `rate`, switch, and confirm it at the new rate.
async fn negotiate_console(
    to_console2: Sender,
    to_serial: Sender,
    mut from_serial: Receiver,
    rate: u32,
) -> Result<Upload> {
    let switched = switch_rate(
        &to_console2,
        &to_serial,
        &mut from_serial,
        rate,
        KICKSTART_CONSOLE,
    )
    .await?;
    Ok(if switched {
        Upload::Console { line_rate: rate }
    } else {
        Upload::ConsoleStayed
    })
}

/// The transfer rate over the last [`RATE_WINDOW`], from samples of how much
/// was sent by when.
#[derive(Debug, Default)]
struct RateWindow {
    samples: VecDeque<(Duration, u64)>,
}

impl RateWindow {
    /// `sent` bytes in all were sent `at` after the start.
    fn record(&mut self, at: Duration, sent: u64) {
        self.samples.push_back((at, sent));
        // Keep the newest sample at least a window old as the base.
        while self
            .samples
            .get(1)
            .is_some_and(|(second_at, _)| at.saturating_sub(*second_at) >= RATE_WINDOW)
        {
            self.samples.pop_front();
        }
    }

    /// The rate in bits per second, once two samples are some time apart.
    fn bits_per_second(&self) -> Option<u64> {
        let (first_at, first_sent) = self.samples.front()?;
        let (last_at, last_sent) = self.samples.back()?;
        let span = last_at.saturating_sub(*first_at).as_micros();
        if span == 0 {
            return None;
        }
        u64::try_from(u128::from(last_sent - first_sent) * 8 * 1_000_000 / span).ok()
    }
}

/// `seconds` as m:ss.
fn minutes_and_seconds(seconds: u64) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// The progress line while the kernel is sent: `sent` of `total` bytes at
/// `bits_per_second`, if known yet, with `frame` of the hourglass.
fn progress_line(frame: char, sent: u64, total: u64, bits_per_second: Option<u64>) -> String {
    let percent = (u128::from(sent) * 100)
        .checked_div(u128::from(total))
        .unwrap_or(100);
    let (rate, left) = match bits_per_second.filter(|rate| *rate > 0) {
        Some(rate) => (
            format!("{} Kbps", rate / 1000),
            minutes_and_seconds((total.saturating_sub(sent) * 8).div_ceil(rate)),
        ),
        None => ("- Kbps".to_owned(), "-:--".to_owned()),
    };
    format!("{frame} {percent:3}% {sent}/{total} bytes, {rate}, {left} left")
}

/// The progress line's last state: `total` bytes sent in `elapsed`.
fn finished_line(total: u64, elapsed: Duration) -> String {
    let average = u128::from(total) * 8 * 1_000_000 / elapsed.as_micros().max(1);
    format!(
        "✅ 100% {total}/{total} bytes in {}, {} Kbps average",
        minutes_and_seconds(elapsed.as_secs()),
        average / 1000
    )
}

/// Send the size, the kernel and its checksum, with chainboot's answers, at the
/// rate the line is at. The kernel goes in [`SEND_CHUNK`] pieces, each waited
/// for until it is written to the line, and a progress line follows it.
async fn send_image(
    to_console2: &Sender,
    to_serial: &Sender,
    from_serial: &mut Receiver,
    mut kernel_file: File,
    kernel_size: u64,
) -> Result<()> {
    to_console2
        .send(Ok(Message::Text("⏩ Sending image size .. ".into())))
        .await?;
    to_serial
        .send(Ok(Message::Binary(Bytes::copy_from_slice(
            &kernel_size.to_le_bytes(),
        ))))
        .await?;

    // Wait for OK response
    expect(to_console2, from_serial, "OK").await?;

    to_console2
        .send(Ok(Message::Text("\r\n⏩ Sending kernel image\r\n".into())))
        .await?;

    let mut hasher = SeaHasher::new();
    let mut chunk = vec![0; SEND_CHUNK];
    let started = Instant::now();
    let mut rate_window = RateWindow::default();
    rate_window.record(Duration::ZERO, 0);
    let mut drawn: Option<Duration> = None;
    let mut hourglass_step = 0_usize;
    let mut sent: u64 = 0;
    loop {
        let length = kernel_file.read(&mut chunk)?;
        if length == 0 {
            break;
        }
        hasher.write(&chunk[..length]);
        let written = Arc::new(Notify::new());
        to_serial
            .send(Ok(Message::Binary(Bytes::copy_from_slice(
                &chunk[..length],
            ))))
            .await?;
        to_serial
            .send(Ok(Message::Written(Arc::clone(&written))))
            .await?;
        written.notified().await;
        sent += u64::try_from(length)?;

        let now = started.elapsed();
        rate_window.record(now, sent);
        if drawn.is_none_or(|drawn_at| now.saturating_sub(drawn_at) >= PROGRESS_REDRAW) {
            drawn = Some(now);
            let line = progress_line(
                animated(&mut hourglass_step),
                sent,
                kernel_size,
                rate_window.bits_per_second(),
            );
            to_console2
                .send(Ok(Message::Text(format!("\r{line}\x1b[K"))))
                .await?;
        }
    }
    let line = finished_line(sent, started.elapsed());
    to_console2
        .send(Ok(Message::Text(format!("\r{line}\x1b[K\r\n"))))
        .await?;
    let hashed_value: u64 = hasher.finish();

    to_console2
        .send(Ok(Message::Text(format!(
            "⏩ Sending image checksum {hashed_value:x} .. "
        ))))
        .await?;

    to_serial
        .send(Ok(Message::Binary(Bytes::copy_from_slice(
            &hashed_value.to_le_bytes(),
        ))))
        .await?;

    expect(to_console2, from_serial, "OK").await
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

/// The rates the host uses.
#[derive(Clone, Copy, Debug)]
struct Rates {
    /// The rate the port opens at, chainboot's boot rate.
    initial: u32,
    /// The rate to ask chainboot to load at, if any.
    load: Option<u32>,
}

/// How an upload, or the console negotiation, ended when nothing went wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Upload {
    /// The kernel runs, and the line is at `line_rate`.
    Loaded { line_rate: u32 },
    /// Chainboot did not take the load rate; it waits at the boot rate again.
    FellBack,
    /// Kickstart's console runs at `line_rate`.
    Console { line_rate: u32 },
    /// Kickstart's console stays at the boot rate, or went back to it.
    ConsoleStayed,
}

async fn send_kernel<P: ThePath>(
    to_console2: Sender,
    to_serial: Sender,
    mut from_serial: Receiver,
    kernel: P,
    rates: Rates,
) -> Result<Upload> {
    let (kernel_file, kernel_size) = load_kernel(&to_console2, kernel).await?;

    let mut line_rate = rates.initial;
    if let Some(load_rate) = rates.load {
        if !switch_rate(
            &to_console2,
            &to_serial,
            &mut from_serial,
            load_rate,
            CHAINBOOT_LOAD,
        )
        .await?
        {
            return Ok(Upload::FellBack);
        }
        line_rate = load_rate;
    }

    send_image(
        &to_console2,
        &to_serial,
        &mut from_serial,
        kernel_file,
        kernel_size,
    )
    .await?;

    // The kernel stays at the rate the boot loader set (chainboot's
    // `kernel_main`), so the console stays at the rate the load ran at.
    Ok(Upload::Loaded { line_rate })
}

/// Watches what arrives at a rate other than the initial one for chainboot
/// talking at the boot rate after the target restarted. Read too fast, each of
/// its characters comes as a NUL (the line held low for longer than a frame is
/// a break) or as bytes that are not text, and no newline among them.
#[derive(Debug, Default)]
struct RestartWatch {
    garbage_run: usize,
}

impl RestartWatch {
    /// Whether `text`, which arrived, makes it look like a restart.
    fn observe(&mut self, text: &str) -> bool {
        for character in text.chars() {
            match character {
                '\0' | char::REPLACEMENT_CHARACTER => self.garbage_run += 1,
                '\n' => self.garbage_run = 0,
                _ => {}
            }
            if self.garbage_run >= RESTART_GARBAGE {
                self.garbage_run = 0;
                return true;
            }
        }
        false
    }
}

// Async reading using Tokio: https://fasterthanli.me/articles/a-terminal-case-of-linux

/// A line whose rate can be changed. A serial port's can; QEMU's TCP and PTY
/// lines carry bytes at any rate, so for them it is nothing to do.
trait RateControl {
    fn set_rate(&mut self, rate: u32) -> Result<()>;
}

impl RateControl for SerialStream {
    fn set_rate(&mut self, rate: u32) -> Result<()> {
        self.set_baud_rate(rate)?;
        Ok(())
    }
}

impl RateControl for TcpStream {
    fn set_rate(&mut self, _rate: u32) -> Result<()> {
        Ok(())
    }
}

impl RateControl for tokio::fs::File {
    fn set_rate(&mut self, _rate: u32) -> Result<()> {
        Ok(())
    }
}

async fn serial_loop<T>(mut port: T, to_console: Sender, mut from_console: Receiver) -> Result<()>
where
    T: AsyncRead + AsyncWrite + RateControl + Unpin,
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
                    Message::SetRate(rate) => port.set_rate(rate)?,
                    Message::Written(written) => {
                        port.flush().await?;
                        written.notify_one();
                    }
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
// SetRate() and Written() only go to serial_loop.
#[derive(Clone, Debug)]
enum Message {
    Binary(Bytes),
    Text(String),
    /// Switch the line to this rate, after what was sent before.
    SetRate(u32),
    /// Notified once what was sent before is written to the line.
    Written(Arc<Notify>),
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
            Message::SetRate(rate) => write!(f, "[rate {rate}]"),
            Message::Written(_) => write!(f, "[written]"),
        }
    }
}

async fn console_loop<P>(
    to_console2: Sender,
    mut from_internal: Receiver,
    to_serial: Sender,
    mut from_serial: Receiver,
    kernel: Option<P>,
    rates: Rates,
) -> Result<()>
where
    P: ThePath,
{
    let mut w = std::io::stdout();

    let mut breaks = 0;
    // Kickstart's console beacon; it is not printed.
    let mut console_beacon = Run::of(CONSOLE_BEACON);
    // Whether console mode said once that it sends no kernel.
    let mut told_no_kernel = false;

    // The rate to load at, until it fails once; then loads run at the initial rate.
    let mut load_rate = rates.load;
    // The rate the line is at, and the watch for a restart while it is not the initial one.
    let mut line_rate = rates.initial;
    let mut restart_watch = RestartWatch::default();
    // When the line last carried anything, for the silence fallback.
    let mut last_heard = tokio::time::Instant::now();

    let mut event_reader = EventStream::new();

    // Protocol channel: while an upload task is running, serial bytes are
    // routed here instead of being printed, so send_kernel can await the
    // target's replies without starving the select loop's other branches.
    let (mut to_proto, from_proto) = mpsc::channel::<Result<Message>>(256);
    let mut from_proto = Some(from_proto);
    let mut upload: Option<tokio::task::JoinHandle<Result<Upload>>> = None;

    loop {
        tokio::select! {
            biased;

            Some(received) = from_internal.recv() => {
                if let Ok(message) = received {
                    execute!(w, style::Print(message))?;
                    w.flush()?;
                }
            }

            // Silent too long at another rate: the target may be beaconing at the initial one.
            () = tokio::time::sleep_until(last_heard + SILENCE_FALLBACK),
                if line_rate != rates.initial && upload.is_none() => {
                to_serial.send(Ok(Message::SetRate(rates.initial))).await?;
                execute!(w, style::Print(format!(
                    "\r\n⏩ Nothing heard at {line_rate} baud for {} s, the target restarted? Back to {} baud\r\n",
                    SILENCE_FALLBACK.as_secs(),
                    rates.initial
                )))?;
                w.flush()?;
                line_rate = rates.initial;
                restart_watch = RestartWatch::default();
            }

            Some(received) = from_serial.recv() => { // returns Vec<char>
                last_heard = tokio::time::Instant::now();
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

                    // After a restart chainboot beacons at the initial rate: go back to it to hear it.
                    if line_rate != rates.initial && restart_watch.observe(&received) {
                        to_serial.send(Ok(Message::SetRate(rates.initial))).await?;
                        execute!(w, style::Print(format!(
                            "\r\n⏩ Garbage at {line_rate} baud, the target restarted? Back to {} baud\r\n",
                            rates.initial
                        )))?;
                        w.flush()?;
                        line_rate = rates.initial;
                    }

                    for x in received.chars() {
                        if x == char::from(CONSOLE_BEACON[0]) {
                            // Kickstart offers a faster console: ask for the load rate
                            // when the line is at the initial one.
                            if console_beacon.push(CONSOLE_BEACON[0])
                                && upload.is_none()
                                && line_rate == rates.initial
                                && let Some(rate) = load_rate
                            {
                                let handle = tokio::spawn(negotiate_console(
                                    to_console2.clone(),
                                    to_serial.clone(),
                                    from_proto.take().expect("proto receiver already taken"),
                                    rate,
                                ));
                                upload = Some(handle);
                            }
                            continue;
                        }
                        console_beacon = Run::of(CONSOLE_BEACON);
                        if x == 0x3 as char {
                            // execute!(w, style::Print("⏩ Received a BREAK\r\n"))?;
                            breaks += 1;
                            // Await for 3 consecutive \3 to start downloading
                            if breaks == 3 {
                                // execute!(w, style::Print("⏩ Received 3 BREAKs\r\n"))?;
                                w.flush()?;
                                breaks = 0;
                                let Some(kernel) = kernel.clone() else {
                                    if !told_no_kernel {
                                        told_no_kernel = true;
                                        execute!(w, style::Print(
                                            "\r\n⏩ Chainboot asks for a kernel; console mode sends none (use --kernel)\r\n"
                                        ))?;
                                        w.flush()?;
                                    }
                                    continue;
                                };
                                // Spawn the upload so this loop keeps polling
                                // from_internal (status messages) and stdin.
                                let handle = tokio::spawn(send_kernel(
                                    to_console2.clone(),
                                    to_serial.clone(),
                                    from_proto.take().expect("proto receiver already taken"),
                                    kernel,
                                    Rates { initial: rates.initial, load: load_rate },
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
                if let Ok(Ok(Upload::Loaded { line_rate: loaded_at })) = result {
                    line_rate = loaded_at;
                    restart_watch = RestartWatch::default();
                    to_console2.send(Ok(Message::Text(format!("🦀 Send successful, pass-through at {loaded_at} baud\r\n")))).await?;
                } else if let Ok(Ok(Upload::Console { line_rate: console_at })) = result {
                    line_rate = console_at;
                    restart_watch = RestartWatch::default();
                    to_console2.send(Ok(Message::Text(format!("🦀 Kickstart's console, pass-through at {console_at} baud\r\n")))).await?;
                } else if let Ok(Ok(Upload::ConsoleStayed)) = result {
                    // Kickstart is at the boot rate, or goes back to it: so does the host.
                    // The load rate stays for the next boot.
                    to_serial.send(Ok(Message::SetRate(rates.initial))).await?;
                    line_rate = rates.initial;
                } else {
                    // Chainboot waits at the boot rate again after anything else: so does the host.
                    to_serial.send(Ok(Message::SetRate(rates.initial))).await?;
                    line_rate = rates.initial;
                    match result {
                        Ok(Err(e)) => {
                            execute!(w, style::Print(format!("\r\n\r\n❌ Upload failed: {e:?}\r\n")))?;
                        }
                        Err(join_err) => {
                            execute!(w, style::Print(format!("\r\n\r\n❌ Upload task panicked: {join_err:?}\r\n")))?;
                        }
                        Ok(Ok(_fell_back)) => {}
                    }
                    if let Some(rate) = load_rate.take() {
                        execute!(w, style::Print(format!(
                            "\r\n⏩ Not loading at {rate} baud again: loads run at {} baud from now on\r\n",
                            rates.initial
                        )))?;
                    }
                    w.flush()?;
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

async fn main_loop<T, P>(port: T, kernel: Option<P>, rates: Rates) -> Result<()>
where
    T: AsyncRead + AsyncWrite + RateControl + Unpin + Send + 'static,
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
    console_loop(
        to_console2,
        from_internal,
        to_serial,
        from_serial,
        kernel,
        rates,
    )
    .await

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
// 3. with --load-baud, ask chainboot for that rate and switch to it
// 4. send selected kernel binary with checksum to the target
// 5. stay at the rate the load ran at for the console; on garbage there, which is
//    chainboot restarted at the initial rate, go back to the initial rate
// 6. go to 2
// Meanwhile, on Kickstart's console beacon (a kernel booted from the SD card), ask
// it for the --load-baud rate too, and stay at it for the console.

/// `ChainOfCommand` - command chainboot protocol
///
/// Use to send freshly built kernel to chainboot-compatible boot loader, or, with
/// --console, as the console of a kernel booted from the SD card.
#[derive(argh::FromArgs)]
struct Args {
    /// device path to a serial port, e.g. /dev/ttyUSB0
    #[argh(option, short = 'p')]
    port: Option<String>,
    /// baud rate to connect at, chainboot's boot rate (115200)
    #[argh(option, short = 'b')]
    baud: Option<u32>,
    /// baud rate to ask chainboot to load the kernel at, and Kickstart to run its
    /// console at when it boots without chainboot; the console stays at it
    /// afterwards. Without it everything runs at the connect rate
    #[argh(option)]
    load_baud: Option<u32>,
    /// path of the binary kernel image to send
    #[argh(option, short = 'k')]
    kernel: Option<String>,
    /// console only: send no kernel when chainboot asks for one (for a kernel
    /// booted from the SD card)
    #[argh(switch)]
    console: bool,
    /// positional form: <port> [baud] [kernel]
    #[argh(positional)]
    positional: Vec<String>,
}

/// What the arguments come to.
struct Settings {
    port: String,
    rates: Rates,
    /// The kernel to send, none in console mode.
    kernel: Option<String>,
}

impl Args {
    fn resolve(self) -> anyhow::Result<Settings> {
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
        let kernel = self.kernel.or_else(|| pos(2));
        if self.console && kernel.is_some() {
            return Err(anyhow::anyhow!("console mode sends no kernel"));
        }
        let kernel = (!self.console).then(|| kernel.unwrap_or_else(|| String::from("kernel8.img")));
        if self.load_baud == Some(0) {
            return Err(anyhow::anyhow!("invalid load baud rate: 0"));
        }
        Ok(Settings {
            port,
            rates: Rates {
                initial: baud,
                load: self.load_baud,
            },
            kernel,
        })
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
    let Settings {
        port,
        rates,
        kernel,
    } = args.resolve()?;
    let baud = rates.initial;

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
                            terminal::Clear(terminal::ClearType::UntilNewLine),
                            style::Print(format!(
                                "{} Waiting for serial port {port} ({e})\r",
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
            OpenedPort::Serial(port) => main_loop(port, kernel.clone(), rates).await,
            OpenedPort::Raw(port) => main_loop(port, kernel.clone(), rates).await,
            OpenedPort::Tcp(port) => main_loop(port, kernel.clone(), rates).await,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A serial line that has delivered `chunks`, with its sender, kept open so
    /// that the line does not look closed.
    fn serial(chunks: &[&str]) -> (Sender, Receiver) {
        let (to_serial, from_serial) = mpsc::channel(16);
        for chunk in chunks {
            to_serial
                .try_send(Ok(Message::Text((*chunk).to_owned())))
                .expect("room for the chunks");
        }
        (to_serial, from_serial)
    }

    /// What `expect` passed on to the console, joined.
    fn passed_on(console: &mut Receiver) -> String {
        let mut text = String::new();
        while let Ok(Ok(Message::Text(chunk))) = console.try_recv() {
            text.push_str(&chunk);
        }
        text
    }

    #[tokio::test]
    async fn the_answer_alone_matches() {
        let (_line, mut from_serial) = serial(&["OK"]);
        let (to_console, mut console) = mpsc::channel(16);
        expect(&to_console, &mut from_serial, "OK")
            .await
            .expect("OK matches");
        assert_eq!(passed_on(&mut console), "OK");
    }

    #[tokio::test]
    async fn what_follows_the_answer_in_the_chunk_is_passed_on() {
        let chunk = "OK⏪ Loaded! Executing the payload now from 0x80000\r\n\r\n";
        let (_line, mut from_serial) = serial(&[chunk]);
        let (to_console, mut console) = mpsc::channel(16);
        expect(&to_console, &mut from_serial, "OK")
            .await
            .expect("OK and more matches");
        assert_eq!(passed_on(&mut console), chunk);
    }

    #[tokio::test]
    async fn an_answer_split_over_chunks_matches() {
        let (_line, mut from_serial) = serial(&["O", "K⏪ Loaded"]);
        let (to_console, mut console) = mpsc::channel(16);
        expect(&to_console, &mut from_serial, "OK")
            .await
            .expect("a split OK matches");
        assert_eq!(passed_on(&mut console), "OK⏪ Loaded");
    }

    #[tokio::test]
    async fn the_rest_is_not_waited_for() {
        // The line stays open and silent after "OK": expect must not wait for more.
        let (_line, mut from_serial) = serial(&["OK"]);
        let (to_console, _console) = mpsc::channel(16);
        let answered = tokio::time::timeout(
            Duration::from_secs(5),
            expect(&to_console, &mut from_serial, "OK"),
        )
        .await;
        assert!(matches!(answered, Ok(Ok(()))));
    }

    #[tokio::test]
    async fn a_wrong_answer_fails() {
        let (_line, mut from_serial) = serial(&["ERR ❌ Kernel image too big"]);
        let (to_console, mut console) = mpsc::channel(16);
        let error = expect(&to_console, &mut from_serial, "OK")
            .await
            .expect_err("ERR is not OK");
        assert!(error.to_string().contains("ERR"));
        // The target's words still reach the console.
        assert_eq!(passed_on(&mut console), "ERR ❌ Kernel image too big");
    }

    #[tokio::test]
    async fn a_wrong_answer_split_over_chunks_fails() {
        let (_line, mut from_serial) = serial(&["E", "RR"]);
        let (to_console, _console) = mpsc::channel(16);
        assert!(expect(&to_console, &mut from_serial, "OK").await.is_err());
    }

    #[tokio::test]
    async fn a_closed_line_fails() {
        let (line, mut from_serial) = serial(&[]);
        drop(line);
        let (to_console, _console) = mpsc::channel(16);
        assert!(expect(&to_console, &mut from_serial, "OK").await.is_err());
    }

    /// What went to the serial line: the bytes, and the rates it was switched to.
    fn sent_to_serial(line: &mut Receiver) -> (Vec<u8>, Vec<u32>) {
        let mut bytes = Vec::new();
        let mut rates = Vec::new();
        while let Ok(Ok(message)) = line.try_recv() {
            match message {
                Message::Binary(chunk) => bytes.extend_from_slice(&chunk),
                Message::SetRate(rate) => rates.push(rate),
                Message::Text(_) | Message::Written(_) => {}
            }
        }
        (bytes, rates)
    }

    #[tokio::test]
    async fn an_accepted_rate_switches_and_waits_for_the_beacon() {
        let (_line, mut from_serial) = serial(&["BAUD OK", "\u{fffd}\0", "\u{3}\u{3}", "\u{3}"]);
        let (to_console, _console) = mpsc::channel(16);
        let (to_serial, mut serial_line) = mpsc::channel(16);
        let switched = switch_rate(&to_console, &to_serial, &mut from_serial, 2_000_000, CHAINBOOT_LOAD)
            .await
            .expect("no line errors");
        assert!(switched);
        let (bytes, rates) = sent_to_serial(&mut serial_line);
        assert_eq!(bytes, encode_rate_request(2_000_000));
        assert_eq!(rates, [2_000_000]);
    }

    #[tokio::test]
    async fn a_refused_rate_does_not_switch() {
        let (_line, mut from_serial) =
            serial(&["BAUD NO\r\n⏪ Staying at the rate: 4000000 baud is out of range"]);
        let (to_console, _console) = mpsc::channel(16);
        let (to_serial, mut serial_line) = mpsc::channel(16);
        let switched = switch_rate(&to_console, &to_serial, &mut from_serial, 4_000_000, CHAINBOOT_LOAD)
            .await
            .expect("no line errors");
        assert!(!switched);
        assert!(sent_to_serial(&mut serial_line).1.is_empty());
    }

    #[tokio::test]
    async fn an_older_chainboot_does_not_switch() {
        let (_line, mut from_serial) = serial(&["ERR ❌ Kernel image too big (over 1 bytes)"]);
        let (to_console, _console) = mpsc::channel(16);
        let (to_serial, mut serial_line) = mpsc::channel(16);
        let switched = switch_rate(&to_console, &to_serial, &mut from_serial, 2_000_000, CHAINBOOT_LOAD)
            .await
            .expect("no line errors");
        assert!(!switched);
        assert!(sent_to_serial(&mut serial_line).1.is_empty());
    }

    #[tokio::test]
    async fn no_beacon_at_the_new_rate_gives_up() {
        // Takes HOST_FAST_BEACON_TIMEOUT of real time.
        let (_line, mut from_serial) = serial(&["BAUD OK"]);
        let (to_console, _console) = mpsc::channel(16);
        let (to_serial, mut serial_line) = mpsc::channel(16);
        let started = Instant::now();
        let switched = switch_rate(&to_console, &to_serial, &mut from_serial, 2_000_000, CHAINBOOT_LOAD)
            .await
            .expect("no line errors");
        assert!(!switched);
        assert!(started.elapsed() >= HOST_FAST_BEACON_TIMEOUT);
        // The host switched and gives up; the console loop takes it back.
        assert_eq!(sent_to_serial(&mut serial_line).1, [2_000_000]);
    }

    #[test]
    fn the_progress_line_says_how_far_how_fast_and_how_long() {
        // 2 Mbit/s with 1_000_000 bytes to go: 4 s.
        assert_eq!(
            progress_line('⏳', 1_000_000, 2_000_000, Some(2_000_000)),
            "⏳  50% 1000000/2000000 bytes, 2000 Kbps, 0:04 left"
        );
        // 115200 baud with 5 MB to go: 347.2 s, rounded up.
        assert_eq!(
            progress_line('⌛', 0, 5_000_000, Some(115_200)),
            "⌛   0% 0/5000000 bytes, 115 Kbps, 5:48 left"
        );
        assert_eq!(
            progress_line('⏳', 4096, 8192, None),
            "⏳  50% 4096/8192 bytes, - Kbps, -:-- left"
        );
        assert_eq!(
            progress_line('⏳', 0, 0, Some(0)),
            "⏳ 100% 0/0 bytes, - Kbps, -:-- left"
        );
    }

    #[test]
    fn the_finished_line_says_the_average() {
        assert_eq!(
            finished_line(5_000_000, Duration::from_secs(25)),
            "✅ 100% 5000000/5000000 bytes in 0:25, 1600 Kbps average"
        );
        assert_eq!(
            finished_line(0, Duration::ZERO),
            "✅ 100% 0/0 bytes in 0:00, 0 Kbps average"
        );
    }

    #[test]
    fn minutes_and_seconds_pad_the_seconds() {
        assert_eq!(minutes_and_seconds(0), "0:00");
        assert_eq!(minutes_and_seconds(59), "0:59");
        assert_eq!(minutes_and_seconds(61), "1:01");
        assert_eq!(minutes_and_seconds(3600), "60:00");
    }

    #[test]
    fn the_rate_is_measured_over_the_last_second() {
        let mut window = RateWindow::default();
        assert_eq!(window.bits_per_second(), None);
        window.record(Duration::ZERO, 0);
        assert_eq!(window.bits_per_second(), None);
        // A fast start: 100 KB in the first 100 ms.
        window.record(Duration::from_millis(100), 100_000);
        assert_eq!(window.bits_per_second(), Some(8_000_000));
        // Then 25 KB every 100 ms, 2 Mbit/s, for two seconds.
        for tenth in 2..=21 {
            window.record(
                Duration::from_millis(100 * tenth),
                100_000 + 25_000 * (tenth - 1),
            );
        }
        // The fast start is out of the window.
        assert_eq!(window.bits_per_second(), Some(2_000_000));
        assert!(window.samples.len() <= 12);
    }

    #[test]
    fn garbage_without_newlines_is_a_restart() {
        let mut watch = RestartWatch::default();
        assert!(!watch.observe("Vesper console at 2000000 baud\r\n"));
        assert!(!watch.observe("\0\0\u{fffd}\0"));
        assert!(watch.observe("\0\u{fffd}\0\0"));
        // It starts over after saying so.
        assert!(!watch.observe("\0"));
    }

    #[test]
    fn a_newline_clears_the_garbage() {
        let mut watch = RestartWatch::default();
        for _ in 0..10 {
            assert!(!watch.observe("\u{fffd}\u{fffd}\u{fffd} split characters\r\n"));
        }
    }

    #[test]
    fn load_baud_is_optional() {
        let args = Args {
            port: None,
            baud: None,
            load_baud: None,
            kernel: None,
            console: false,
            positional: vec!["/dev/ttyUSB0".into()],
        };
        let settings = args.resolve().expect("valid");
        assert_eq!(settings.port, "/dev/ttyUSB0");
        assert_eq!(settings.rates.initial, 115_200);
        assert_eq!(settings.rates.load, None);
        assert_eq!(settings.kernel.as_deref(), Some("kernel8.img"));

        let args = Args {
            port: None,
            baud: None,
            load_baud: Some(2_000_000),
            kernel: Some("wood.bin".into()),
            console: false,
            positional: vec!["/dev/ttyUSB0".into(), "115200".into()],
        };
        let settings = args.resolve().expect("valid");
        assert_eq!(settings.rates.initial, 115_200);
        assert_eq!(settings.rates.load, Some(2_000_000));
        assert_eq!(settings.kernel.as_deref(), Some("wood.bin"));
    }

    #[test]
    fn console_mode_sends_no_kernel() {
        let args = Args {
            port: None,
            baud: None,
            load_baud: Some(1_500_000),
            kernel: None,
            console: true,
            positional: vec!["/dev/ttyUSB0".into(), "115200".into()],
        };
        let settings = args.resolve().expect("valid");
        assert_eq!(settings.kernel, None);
        assert_eq!(settings.rates.load, Some(1_500_000));

        let args = Args {
            port: None,
            baud: None,
            load_baud: None,
            kernel: Some("wood.bin".into()),
            console: true,
            positional: vec!["/dev/ttyUSB0".into()],
        };
        assert!(args.resolve().is_err());
    }

    #[tokio::test]
    async fn the_console_rate_is_confirmed_with_an_ack() {
        let (_line, from_serial) = serial(&["BAUD OK", "\u{fffd}\0", "\u{2}\u{2}", "\u{2}"]);
        let (to_console, _console) = mpsc::channel(16);
        let (to_serial, mut serial_line) = mpsc::channel(16);
        let outcome = negotiate_console(to_console, to_serial, from_serial, 1_500_000)
            .await
            .expect("no line errors");
        assert_eq!(outcome, Upload::Console { line_rate: 1_500_000 });
        let (bytes, rates) = sent_to_serial(&mut serial_line);
        let mut expected = encode_rate_request(1_500_000).to_vec();
        expected.extend_from_slice(&CONSOLE_ACK);
        assert_eq!(bytes, expected);
        assert_eq!(rates, [1_500_000]);
    }

    #[tokio::test]
    async fn a_load_beacon_does_not_confirm_the_console() {
        // Takes HOST_CONSOLE_CONFIRM_TIMEOUT of real time.
        let (_line, from_serial) = serial(&["BAUD OK", "\u{3}\u{3}\u{3}"]);
        let (to_console, _console) = mpsc::channel(16);
        let (to_serial, mut serial_line) = mpsc::channel(16);
        let started = Instant::now();
        let outcome = negotiate_console(to_console, to_serial, from_serial, 1_500_000)
            .await
            .expect("no line errors");
        assert_eq!(outcome, Upload::ConsoleStayed);
        assert!(started.elapsed() >= HOST_CONSOLE_CONFIRM_TIMEOUT);
        // The host switched, sent no ACK, and the console loop takes it back.
        let (bytes, rates) = sent_to_serial(&mut serial_line);
        assert_eq!(bytes, encode_rate_request(1_500_000));
        assert_eq!(rates, [1_500_000]);
    }

    #[tokio::test]
    async fn a_refused_console_rate_stays() {
        let (_line, from_serial) =
            serial(&["BAUD NO\r\n🥾 Staying at the rate: 4000000 baud is out of range"]);
        let (to_console, _console) = mpsc::channel(16);
        let (to_serial, mut serial_line) = mpsc::channel(16);
        let outcome = negotiate_console(to_console, to_serial, from_serial, 4_000_000)
            .await
            .expect("no line errors");
        assert_eq!(outcome, Upload::ConsoleStayed);
        assert!(sent_to_serial(&mut serial_line).1.is_empty());
    }

    #[test]
    fn only_ascii_characters_are_beacon_bytes() {
        assert_eq!(beacon_byte('\u{2}'), 2);
        assert_eq!(beacon_byte('\u{3}'), 3);
        assert_eq!(beacon_byte(char::REPLACEMENT_CHARACTER), u8::MAX);
        assert_eq!(beacon_byte('\u{82}'), u8::MAX);
    }
}
