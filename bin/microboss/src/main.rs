use {
    anyhow::Result,
    clap::{App, AppSettings, Arg},
    seahash::SeaHasher,
    serialport::SerialPort,
    std::{
        fs::File,
        hash::Hasher,
        io::{BufRead, BufReader},
        path::Path,
        time::Duration,
    },
};

fn expect(port: &mut Box<dyn SerialPort>, c: u8) {
    let mut buf = [0u8; 1];
    match port.read(&mut buf) {
        Err(e) => panic!("Failed to receive from serial port: {}", e),
        Ok(b) if b == 1 && buf[0] == c => {
            return;
        }
        Ok(_) => {
            print!("{}", buf[0]);
            while port.read(&mut buf).is_ok() {
                print!("{}", buf[0]);
            }
            panic!("Failed to receive expected value");
        }
    }
}

fn load_kernel(kernel: &Path) -> Result<(File, u64), ()> {
    println!("[>>] Loading kernel image");

    let kernel_file = match std::fs::File::open(kernel) {
        Ok(file) => file,
        Err(_) => return Err(anyhow!("Couldn't open kernel file {}", kernel)),
    };
    let kernel_size: u64 = kernel_file.metadata()?.len();

    println!("[>>] .. {} ({} bytes)", kernel, kernel_size);

    Ok((kernel_file, kernel_size))
}

fn send_kernel(kernel_file: &File, kernel_size: u64) -> Result<()> {
    println!("[>>] Sending image size");

    port.write(&kernel_size.to_le_bytes())?;

    // Wait for OK response
    expect(&mut port, b'O');
    expect(&mut port, b'K');

    println!("[>>] Sending kernel image");

    let mut hasher = SeaHasher::new();
    let mut reader = BufReader::new(kernel_file);
    loop {
        let length = {
            let buf = reader.fill_buf()?;
            port.write(buf)?;
            hasher.write(buf);
            buf.len()
        };
        if length == 0 {
            break;
        }
        reader.consume(length);
    }
    let hashed_value: u64 = hasher.finish();

    println!("[>>] Sending image checksum {:x}", hashed_value);

    port.write(&hashed_value.to_le_bytes())
}

// 1. connect to given serial port, e.g. /dev/ttyUSB23234
// 2. send 3 consecutive \3 chars
// 3. get OK response
// 4. send selected kernel binary with checksum to the target
// 5. pass through the serial connection

fn main() -> Result<()> {
    let matches = App::new("MicroBoss - command microboot protocol")
        .about("Use to send freshly built kernel to microboot-compatible boot loader")
        .setting(AppSettings::DisableVersion)
        .arg(
            Arg::with_name("port")
                .help("The device path to a serial port, e.g. /dev/ttyUSB0")
                .required(true),
        )
        .arg(
            Arg::with_name("baud")
                .help("The baud rate to connect at")
                .use_delimiter(false)
                .required(true), // .validator(valid_baud),
        )
        .arg(
            Arg::with_name("kernel")
                .long("kernel")
                .help("Path of the binary kernel image to send")
                .takes_value(true)
                .default_value("kernel8.img"),
        )
        .get_matches();
    let port_name = matches.value_of("port").unwrap();
    let baud_rate = matches.value_of("baud").unwrap().parse::<u32>().unwrap();
    let kernel = matches.value_of("kernel").unwrap();

    let (kernel_file, kernel_size) = load_kernel(kernel)?;

    println!("[>>] Opening serial port");

    let mut port = serialport::new(port_name, baud_rate)
        .timeout(Duration::from_millis(1000))
        .open()
        .expect("Failed to open serial port");
    //.context?

    // Run in pass-through mode by default.
    // Once we receive BREAK (0x3) three times, switch to kernel send mode and upload kernel,
    // then switch back to pass-through mode.

    // Input from STDIN should pass through to serial
    // Input from serial should pass through to STDOUT

    println!("[>>] Waiting for handshake, pass-through");

    // Await for 3 consecutive \3 to start downloading
    let mut count = 0;
    loop {
        let c = CONSOLE.lock(|c| c.read_char()) as u8;

        if c == 3 {
            count += 1;
        } else {
            count = 0;
        }

        if count == 3 {
            break;
        }
    }

    print!("OK");

    send_kernel(&kernel_file, kernel_size)?;

    Ok(())
}
