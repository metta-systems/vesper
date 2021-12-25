use {
    anyhow::Result,
    clap::{App, AppSettings, Arg},
    seahash::SeaHasher,
    serialport::SerialPort,
    std::{
        hash::Hasher,
        io::{BufRead, BufReader},
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

    println!("[>>] Loading kernel image");

    let kernel_file = match std::fs::File::open(kernel) {
        Ok(file) => file,
        Err(_) => panic!("Couldn't open kernel file {}", kernel),
    };
    let kernel_size: u64 = kernel_file.metadata().unwrap().len(); // TODO: unwrap

    println!("[>>] .. {} ({} bytes)", kernel, kernel_size);

    println!("[>>] Opening serial port");

    // TODO: writeln!() to the serial fd instead of println?
    let mut port = serialport::new(port_name, baud_rate)
        .timeout(Duration::from_millis(1000))
        .open()
        .expect("Failed to open serial port");
    //.context?

    println!("[>>] Waiting for handshake");

    // Notify `microboot` to receive the binary.
    for _ in 0..3 {
        port.write(&3u8.to_le_bytes())?;
    }

    // Wait for OK response
    expect(&mut port, b'O');
    expect(&mut port, b'K');

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

    port.write(&hashed_value.to_le_bytes())?;

    Ok(())
}
