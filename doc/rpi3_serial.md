# Connecting RPi3 UART

## Using cp2104 usb-to-ttl converter

Download drivers from: https://www.silabs.com/products/development-tools/software/usb-to-uart-bridge-vcp-drivers

Install `minicom`: `brew install minicom`

Configure minicom to use `/dev/tty.SLAB_USBtoUART` port.

Connect rpi wires to cp2014:

```
UART0

FUNC  |  GPIO  |  PIN #  |  MODE  | Wire color
------+--------+---------+--------+------------
RXD0  | GPIO15 |   10    |  Alt0  | Brown
TXD0  | GPIO14 |    8    |  Alt0  | Red
```

```
MiniUart (UART1)

FUNC  |  GPIO  |  PIN #  |  MODE  | Wire color
------+--------+---------+--------+------------
RXD1  | GPIO15 |   10    |  Alt5  | Brown
TXD1  | GPIO14 |    8    |  Alt5  | Red
GND   | GND    |    6    |        | Green
```
