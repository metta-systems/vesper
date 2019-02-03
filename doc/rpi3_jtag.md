JTAG boards:

* TinCanTools Flyswatter
* OpenMoko DebugBoard_v3
* RasPi3 -
    - Pinouts for rpi3 - modelB https://www.element14.com/community/docs/DOC-73950/l/raspberry-pi-3-model-b-gpio-40-pin-block-pinout and model B+ https://www.element14.com/community/docs/DOC-88824/l/raspberry-pi-3-model-b-gpio-40-pin-block-poe-header-pinout (they are the same)
* Segger J-Link V9

# RPi3 to RPi3 jtag

Helpful RPi3 GPIO header pinouts from element14 [for Model B](https://www.element14.com/community/docs/DOC-73950/l/raspberry-pi-3-model-b-gpio-40-pin-block-pinout) and [here for Model B+](https://www.element14.com/community/docs/DOC-88824/l/raspberry-pi-3-model-b-gpio-40-pin-block-poe-header-pinout) (which is the same).

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
TDI   | GPIO26 |   37    | Alt4
TDO   | GPIO24 |   18    | Alt4
TRST  | GPIO22 |   15    | Alt4
GND   | GND    |   20    |
```

Connecting TDI to pin 7 (GPIO4) did not work!

In config.txt:

```
# Set GPIO pins for JTAG debugger connection on rpi3
gpio=22-27=a4
```

Alternatively, just specify @todo - verify this works with all alt4 pins

```
enable_jtag_gpio=1
```

## Connection between boards

```
Func | Host Pin | Wire color | Target pin
-----+----------+------------+-----------
TCK  |    23    |   yellow   |    22
TMS  |    22    |   brown    |    13
TDI  |    19    |   green    |    37
TDO  |    21    |   orange   |    18
TRST |    26    |    red     |    15
GND  |    20    |   black    |    20
```


## OpenOCD configuration on the host

You need two files: interface file for driving the host GPIO correctly, and target file for detecting the JTAG circuitry on the target RPi.

Interface configuration: [rpi3_interface.cfg](./rpi3_interface.cfg)

[Source](https://movr0.com/2016/09/02/use-raspberry-pi-23-as-a-jtagswd-adapter/), [source #2 - rpi3 speed_coeffs](https://forum.doozan.com/read.php?3,21789)

Target configuration: [rpi3_target.cfg](./rpi3_target.cfg)

[Source #1](https://electronics.stackexchange.com/questions/249008/how-to-use-rpi-2-to-debug-rpi-model-b-via-jtag-with-openocd/419724#419724), [source #2](https://sysprogs.com/tutorials/preparing-raspberry-pi-for-jtag-debugging/), [source #3](http://openocd.org/doc/html/Reset-Configuration.html), [source #4](http://infocenter.arm.com/help/topic/com.arm.doc.faqs/ka3854.html), [source #5](https://www.raspberrypi.org/forums/viewtopic.php?p=1013802), [source #6 - proper rpi3 ocd config](https://www.suse.com/c/debugging-raspberry-pi-3-with-jtag/), [source #7 - simpler rpi3 ocd config](https://github.com/daniel-k/openocd/blob/armv8/tcl/target/rpi3.cfg), [source #8 - explanations about SRST](https://catch22.eu/baremetal/openocd_sysfs_stm32/)

* @todo Check the expected tap id:
> If an SoC provides a JTAG debug interface and contains any CoreSight debug components (including any Cortex processor) you should expect to see the standard JTAG IDCODE of a single CoreSight SWJ-DP as one TAP on the JTAG chain.


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
