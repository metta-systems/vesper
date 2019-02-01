JTAG boards:

* TinCanTools Flyswatter
* OpenMoko DebugBoard_v3
* RasPi3 -
    - Pinouts for rpi3 - modelB https://www.element14.com/community/docs/DOC-73950/l/raspberry-pi-3-model-b-gpio-40-pin-block-pinout and model B+ https://www.element14.com/community/docs/DOC-88824/l/raspberry-pi-3-model-b-gpio-40-pin-block-poe-header-pinout (they are the same)
* Segger J-Link V9

# RPi3 to RPi3 jtag

## Host configuration:

These are regular GPIO functions, which we specify in OpenOCD interface configuration to enable driving JTAG interface.

```
FUNC  |  GPIO  |  PIN #
------+--------+-------
TCK   | GPIO11 | 23
TMS   | GPIO25 | 22
TDI   | GPIO10 | 19
TDO   | GPIO9  | 21
TRST* | GPIO7  | 26
GND   | GND    | 20
```

RPi doesn't expose SRST so we ignore it.

[Source](https://movr0.com/2016/09/02/use-raspberry-pi-23-as-a-jtagswd-adapter/)

## Target configuration:

These are real JTAG pins of bcm2837, enabled on target RPi via config.txt options (see below).

```
FUNC  |  GPIO  |  PIN #  |  MODE
------+--------+---------+------
TCK   | GPIO25 |   22    | Alt4
TMS   | GPIO27 |   13    | Alt4
TDI   | GPIO4  |    7    | Alt5
TDO   | GPIO24 |   18    | Alt4
TRST  | GPIO22 |   15    | Alt4
GND   | GND    |   20    |
```

In config.txt:

```
# Set GPIO pins for JTAG debugger connection on rpi3
gpio=22-25,27=a4
gpio=4=a5
# gpio23 RTCK - unused? Don't forget to avoid frequency scaling in this case.
```

Alternatively, just specify

```
enable_jtag_gpio=1
```

## Connection between boards

```
Func | Host Pin | Wire color | Target pin
-----+----------+------------+-----------
TCK  |    23    |   yellow   |    22
TMS  |    22    |   brown    |    13
TDI  |    19    |   green    |     7
TDO  |    21    |   orange   |    18
TRST |    26    |    red     |    15
GND  |    20    |   black    |    20
```


## OpenOCD configuration on the host

You need two files: interface file for driving the host GPIO correctly, and target file for detecting the JTAG circuitry on the target RPi.

Interface configuration: rpi3_interface.cfg

```
# Broadcom 2835 on Raspberry Pi as JTAG host

interface bcm2835gpio
 
bcm2835gpio_peripheral_base 0x3F000000
 
# Transition delay calculation: SPEED_COEFF/khz - SPEED_OFFSET
# These depend on system clock, calibrated for stock 700MHz
# bcm2835gpio_speed SPEED_COEFF SPEED_OFFSET
bcm2835gpio_speed_coeffs 146203 36
 
# Each of the JTAG lines need a gpio number set: tck tms tdi tdo
# Header pin numbers: 23 22 19 21
bcm2835gpio_jtag_nums 11 25 10 9
 
# If you define trst or srst, use appropriate reset_config
# Header pin numbers: TRST - 26, SRST - 12
 
bcm2835gpio_trst_num 7
reset_config trst_only
```

[Source](https://movr0.com/2016/09/02/use-raspberry-pi-23-as-a-jtagswd-adapter/)

Target configuration: rpi3_target.cfg

```
# Broadcom 2835 on Raspberry Pi as JTAG target

telnet_port 4444
gdb_port 5555

adapter_khz 1000
transport select jtag

if { [info exists CHIPNAME] } {
set _CHIPNAME $CHIPNAME
} else {
set _CHIPNAME rspi
}
 
if { [info exists CPU_TAPID ] } {
set _CPU_TAPID $CPU_TAPID
} else {
set _CPU_TAPID 0x07b7617F
}
 
jtag newtap $_CHIPNAME arm -irlen 5 -expected-id $_CPU_TAPID
 
set _TARGETNAME $_CHIPNAME.arm
target create $_TARGETNAME arm11 -chain-position $_TARGETNAME
$_TARGETNAME configure -event gdb-attach { halt }
```

[Source #1](https://electronics.stackexchange.com/questions/249008/how-to-use-rpi-2-to-debug-rpi-model-b-via-jtag-with-openocd/419724#419724), [source #2](https://sysprogs.com/tutorials/preparing-raspberry-pi-for-jtag-debugging/)

## Run OpenOCD, GDB and attach to target

Need to verify if the following bug is still valid:
> There is a bug in OpenOCD that will prevent Raspberry PI from continuing correctly after a stop unless the initialization is done twice. Close OpenOCD with Ctrl-C and re-run it again. Now the debugging will be usable.

[Source](https://sysprogs.com/tutorials/preparing-raspberry-pi-for-jtag-debugging/)

Run `openocd -f rpi3_interface.cfg -f rpi3_target.cfg`

Run `gdb kernel.elf` and connect to device:

```
target remote :5555
x/10i $pc
stepi
x/2i $pc
```

If `stepi` command causes CPU to make one instruction step, everything is working.

[Source](https://sysprogs.com/tutorials/preparing-raspberry-pi-for-jtag-debugging/)
