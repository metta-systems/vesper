# Connecting RPi3 UART

## Using cp2104 usb-to-ttl converter

Download drivers from [SiLabs driver page](https://www.silabs.com/products/development-tools/software/usb-to-uart-bridge-vcp-drivers).

Install `minicom`: `brew install minicom`

Configure minicom to use `/dev/tty.SLAB_USBtoUART` port. On macOS Big Sur it might be `/dev/tty.usbserial-019586E3` or similar depending on the serial number of the adapter.

Connect RPi wires to cp2014:

_NB:_ Swap the TXD and RXD wires. I.e. RXD pin of CP2104 should go to TXD pin on RPi and vice versa.

```
UART0

RPi Func  |  RPi GPIO  |  PIN #  |  MODE  | CP2104 Pin | Wire color
----------+------------+---------+--------+------------+------------
RXD0      | GPIO15     |   10    |  Alt0  | TXD        | Red
TXD0      | GPIO14     |    8    |  Alt0  | RXD        | Brown
```

```
MiniUart (UART1)

RPi Func  |  RPi GPIO  |  PIN #  |  MODE  | CP2104 Pin | Wire color
----------+------------+---------+--------+------------+------------
RXD1      | GPIO15     |   10    |  Alt5  | TXD        | Red
TXD1      | GPIO14     |    8    |  Alt5  | RXD        | Brown
GND       | GND        |    6    |        | GND        | Green
```
