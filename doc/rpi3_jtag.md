# Connecting RPi3 JTAG

Possible JTAG boards:

* RasPi3
* Segger J-Link V9
* TinCanTools Flyswatter
* OpenMoko DebugBoard_v3 - [this is the version have](http://wiki.openmoko.org/wiki/Debug_Board_v3)

## RPi3 to RPi3 JTAG

Helpful RPi3 GPIO header pinouts from element14 [for Model B](https://www.element14.com/community/docs/DOC-73950/l/raspberry-pi-3-model-b-gpio-40-pin-block-pinout) and [here for Model B+](https://www.element14.com/community/docs/DOC-88824/l/raspberry-pi-3-model-b-gpio-40-pin-block-poe-header-pinout) (they are the same).

### Host configuration:

These are regular GPIO functions, which we specify in OpenOCD interface configuration to enable driving JTAG interface. We should be able to choose pretty much any available pins.

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

### Target configuration:

These are real JTAG pins of bcm2837, enabled on target RPi via config.txt options (see below).

```
FUNC  |  GPIO  |  PIN #  |  MODE
------+--------+---------+------
TCK   | GPIO25 |   22    |  Alt4
TMS   | GPIO27 |   13    |  Alt4
TDI   | GPIO26 |   37    |  Alt4
TDO   | GPIO24 |   18    |  Alt4
TRST  | GPIO22 |   15    |  Alt4
RTCK  | GPIO23 |   16    |  Alt4
GND   | GND    |   20    |
```

Connecting TDI to pin 7 (GPIO4) did not work!

[Source (section 6.2 Alternative Function Assignments)](https://www.raspberrypi.org/app/uploads/2012/02/BCM2835-ARM-Peripherals.pdf)

In config.txt:

```
# Set GPIO pins for JTAG debugger connection on all rpi models
enable_jtag_gpio=1
```

Quote from [official doc](https://www.raspberrypi.org/documentation/configuration/config-txt/gpio.md):
> Setting enable_jtag_gpio=1 selects Alt4 mode for GPIO pins 22-27, and sets up some internal SoC connections, thus enabling the JTAG interface for the ARM CPU. It works on all models of Raspberry Pi.

### Wire Connection between boards

```
Func | Host Pin | Wire color | Target pin
-----+----------+------------+-----------
TCK  |    23    |   yellow   |    22
TMS  |    22    |   brown    |    13
TDI  |    19    |   green    |    37
TDO  |    21    |   orange   |    18
TRST |    26    |   red      |    15
GND  |    20    |   black    |    20
```

### OpenOCD configuration on the host

You need two files: interface file for driving the host GPIO correctly, and target file for detecting the JTAG circuitry on the target RPi.

Interface configuration: [rpi3_interface.cfg](./rpi2rpi_jtag/rpi3_interface.cfg)

[Source](https://movr0.com/2016/09/02/use-raspberry-pi-23-as-a-jtagswd-adapter/), [source #2 - rpi3 speed_coeffs](https://forum.doozan.com/read.php?3,21789)

Target configuration: [rpi3_target.cfg](./rpi2rpi_jtag/rpi3_target.cfg)

[Source #1](https://electronics.stackexchange.com/questions/249008/how-to-use-rpi-2-to-debug-rpi-model-b-via-jtag-with-openocd/419724#419724), [source #2](https://sysprogs.com/tutorials/preparing-raspberry-pi-for-jtag-debugging/), [source #3](http://openocd.org/doc/html/Reset-Configuration.html), [source #4](http://infocenter.arm.com/help/topic/com.arm.doc.faqs/ka3854.html), [source #5](https://www.raspberrypi.org/forums/viewtopic.php?p=1013802), [source #6 - proper rpi3 ocd config](https://www.suse.com/c/debugging-raspberry-pi-3-with-jtag/), [source #7 - simpler rpi3 ocd config](https://github.com/daniel-k/openocd/blob/armv8/tcl/target/rpi3.cfg), [source #8 - explanations about SRST](https://catch22.eu/baremetal/openocd_sysfs_stm32/), [source #9 - example RPi target config](https://github.com/OP-TEE/build/blob/master/rpi3/debugger/pi3.cfg), [source #10 - some JTAG debug hints on rpi](https://www.raspberrypi.org/forums/viewtopic.php?p=1013802), [source #11 - jtag vs swd and CoreSight info links](https://electronics.stackexchange.com/questions/53571/jtag-vs-swd-debugging?rq=1)

> If a SoC provides a JTAG debug interface and contains any CoreSight debug components (including any Cortex processor) you should expect to see the standard JTAG IDCODE of a single CoreSight SWJ-DP as one TAP on the JTAG chain.

### Run OpenOCD, GDB and attach to target

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

[Source](https://sysprogs.com/tutorials/preparing-raspberry-pi-for-jtag-debugging/), [source #2](https://www.op-tee.org/docs/rpi3/#6-openocd-and-jtag), [source #3 - monitor reset halt](http://www.openstm32.org/forumthread823)

I got RPi3-to-RPi3 JTAG working and even debugged a bit directly on the CPU, but a few things make it not an ideal experience:

* RPi is a bit too slow for bitbanging and oftentimes opening a browser window, or running some other command caused OpenOCD to spew JTAG synchronization errors.
* To properly debug my kernel from RPi I would need to compile it locally (otherwise all the paths in the debug info are wrong and GDB will not find the source files, I did not want to mess around with symlinks). Compiling rust on rpi3 is _slow_.

Fortunately, at this point a Segger J-Link 9 arrived and I went to use it.

## J-Link to RPi3 JTAG

> https://www.segger.com/downloads/jlink/

> https://habr.com/ru/post/259205/

JTAG pinout on JLink is in UM08001_JLink.pdf distributed with the J-Link software kit, in section `18.1.1 Pinout for JTAG`.

Reproduced here in ASCII:

```
       +-----------------+
VTRef  |  1 *       * 2  | NC
nTRST  |  3 *       * 4  | GND
TDI    |  5 *       * 6  | GND
TMS    |  7 *       * 8  | GND
TCK   ||  9 *       * 10 | GND
RTCK  || 11 *       * 12 | GND
TDO    | 13 *       * 14 | *
RESET  | 15 *       * 16 | *
DBGRQ  | 17 *       * 18 | *
+5V    | 19 *       * 20 | *
       +-----------------+
```

This adds VTref for target voltage detection.
Additionally, with this pinout J-Link is able to power and boot up RPi3 board itself!

Explanation of pins from J-Link manual:

<table>
    <thead>
    <tr>
        <th>Pin</th>
        <th>Signal</th>
        <th>Direction</th>
        <th>Description</th>
    </tr>
    </thead>
    <tbody>
    <tr>
        <td>1</td>
        <td>VTref</td>
        <td>Input</td>
        <td>This is the target reference voltage. It is used to check if the target has power, to create the logic-level reference for the input comparators and to control the output logic levels to the target. It is normally fed from VDD of the target board and must not have a series resistor.</td>
    </tr>
    <tr>
        <td>2</td>
        <td>NC</td>
        <td>Not connected</td>
        <td>This pin is not connected in J-Link.</td>
    </tr>
    <tr>
        <td>3</td>
        <td>nTRST</td>
        <td>Output</td>
        <td>JTAG Reset. Output from J-Link to the Reset signal of the target JTAG port. Typically connected to nTRST of the target CPU. This pin is normally pulled HIGH on the target to avoid unintentional resets when there is no connection.</td>
    </tr>
    <tr>
        <td>4, 6, 8, 10, 12</td>
        <td>GND</td>
        <td>Ground</td>
        <td>Pins connected to GND in J-Link. They should also be connected to GND in the target system.</td>
    </tr>
    <tr>
        <td>5</td>
        <td>TDI</td>
        <td>Output</td>
        <td>JTAG data input of target CPU. It is recommended that this pin is pulled to a defined state on the target board. Typically connected to TDI of the target CPU.</td>
    </tr>
    <tr>
        <td>7</td>
        <td>TMS</td>
        <td>Output</td>
        <td>JTAG mode set input of target CPU. This pin should be pulled up on the target. Typically connected to TMS of the target CPU.</td>
    </tr>
    <tr>
        <td>9</td>
        <td>TCK</td>
        <td>Output</td>
        <td>JTAG clock signal to target CPU. It is recommended that this pin is pulled to a defined state of the target board. Typically connected to TCK of the target CPU.</td>
    </tr>
    <tr>
        <td>11</td>
        <td>RTCK</td>
        <td>Input</td>
        <td>Return test clock signal from the target. Some targets must synchronize the JTAG inputs to internal clocks. To assist in meeting this requirement, you can use a returned, and re-timed, TCK to dynamically control the TCK rate. J-Link supports adaptive clocking, which waits for TCK changes to be echoed correctly before making further changes. Connect to RTCK if available, <b>otherwise to GND.</b></td>
    </tr>
    <tr>
        <td>13</td>
        <td>TDO</td>
        <td>Input</td>
        <td>JTAG data output from target CPU. Typically connected to TDO of the target CPU.</td>
    </tr>
    <tr>
        <td>15</td>
        <td>nRESET</td>
        <td>I/O</td>
        <td>Target CPU reset signal. Typically connected to the RESET pin of the target CPU, which is typically called “nRST”, “nRESET” or “RESET”. This signal is an active low signal.</td>
    </tr>
    <tr>
        <td>17</td>
        <td>DBGRQ</td>
        <td>Not connected</td>
        <td>This pin is not connected in J-Link</td>
    </tr>
    <tr>
        <td>19</td>
        <td>5V-Supply</td>
        <td>Output</td>
        <td>This pin can be used to supply power to the target hardware. Older J-Links may not be able to supply power on this pin.</td>
    </tr>
    </tbody>
</table>

### J-Link wire connection with RPi3

```
Func  |  J-Link Pin  | Wire color  | Target pin | Target GPIO | Target Func
------+--------------+-------------+------------+-------------+-------------
VTref |       1      | white       |  1         |             |
nTRST |       3      | red         | 15         | GPIO22      | Alt4
TDI   |       5      | green       | 37         | GPIO26      | Alt4
TMS   |       7      | brown       | 13         | GPIO27      | Alt4
TCK   |       9      | yellow      | 22         | GPIO25      | Alt4
RTCK  |      11      | magenta     | 16         | GPIO23      | Alt4
TDO   |      13      | orange      | 18         | GPIO24      | Alt4
GND   |       4      | black       | 20, 14     |             |
```

[Useful article](https://www.suse.com/c/debugging-raspberry-pi-3-with-jtag/).

### Run with OpenOCD

Rebuild openocd from git and voila, it works with 

`openocd -f interface/jlink.cfg -f rpi3_jtag.cfg`

### Run with probe-rs

To be written when probe-rs starts supporting RPi3/4.

## Andre Richter's tutorials

Andre Richter has created an entry in his excellent RPi tutorials dedicated exactly to [JTAG debugging](https://github.com/rust-embedded/rust-raspberrypi-OS-tutorials/tree/master/09_hw_debug_JTAG).

So debugging is a lot easier now - just drop [specifically-built JTAG enabler](https://github.com/rust-embedded/rust-raspberrypi-OS-tutorials/tree/master/X1_JTAG_boot) binary to sdcard, connect over JTAG via openocd and gdb and go load your kernel!
