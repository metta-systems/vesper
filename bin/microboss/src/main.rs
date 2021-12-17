use {
    anyhow::Result,
    clap::{App, AppSettings, Arg},
    seahash::SeaHasher,
    serialport::SerialPort,
    std::{
        hash::Hasher,
        io::{self, BufRead, BufReader},
        time::Duration,
    },
};

fn expect(port: &mut Box<dyn SerialPort>, c: u8) {
    let mut buf = vec![0u8; 1];
    match port.read(buf.as_mut_slice()) {
        Err(_e) => panic!("Failed to receive from serial port"),
        Ok(b) if b == 1 && buf[0] == c => {
            return;
        }
        Ok(_) => {
            print!("{}", buf[0]);
            while let Ok(_) = port.read(buf.as_mut_slice()) {
                print!("{}", buf[0]);
            }
            panic!("Failed to receive expected value");
        }
    }
}

// 1. connect to given serial port, e.g. /dev/ttyUSB23234
// 2. wait for 3 consecutive \3 chars
// 3. send selected kernel binary with checksum to the target
// 4. pass through the serial connection

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

    let kernel_file = match std::fs::File::open(kernel) {
        Ok(file) => file,
        Err(_) => panic!("Couldn't open kernel file {}", kernel),
    };
    let kernel_size: u64 = kernel_file.metadata().unwrap().len(); // TODO: unwrap

    // TODO: writeln!() to the serial fd instead of println?
    let mut port = serialport::new(port_name, baud_rate)
        .timeout(Duration::from_millis(10))
        .open()
        .expect("Failed to open serial port");
    //.context?

    // Await for 3 consecutive \3 to start uploading
    let mut count = 0;
    let mut buf = vec![0u8; 1];
    loop {
        match port.read(buf.as_mut_slice()) {
            Ok(t) if t == 1 && buf[0] == 3 => {
                count += 1;
            }
            Ok(t) => {
                count = 0;
                // Pass through whatever the board prints before \3\3\3
                if t > 0 {
                    print!("{}", buf[0]);
                }
            }
            Err(ref e) if e.kind() == io::ErrorKind::TimedOut => (),
            Err(e) => eprintln!("{:?}", e),
        }
        if count == 3 {
            break;
        }
    }

    println!("[>>] Sending image size");
    port.write(&kernel_size.to_le_bytes())?;

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
