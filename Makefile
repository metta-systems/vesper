#
# MIT License
#
# Copyright (c) 2018 Andre Richter <andre.o.richter@gmail.com>
# Copyright (c) 2019 Berkus Decker <berkus+github@metta.systems>
#
# Permission is hereby granted, free of charge, to any person obtaining a copy
# of this software and associated documentation files (the "Software"), to deal
# in the Software without restriction, including without limitation the rights
# to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
# copies of the Software, and to permit persons to whom the Software is
# furnished to do so, subject to the following conditions:
#
# The above copyright notice and this permission notice shall be included in all
# copies or substantial portions of the Software.
#
# THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
# IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
# FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
# AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
# LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
# OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
# SOFTWARE.
#

TARGET = aarch64-vesper-metta
TARGET_JSON = targets/$(TARGET).json

SOURCES = $(shell find src -name "*.rs" -o -name "*.S") $(shell find linker -name "*.ld")

DEVICE_FEATURES = --features "noserial"
QEMU_FEATURES =

OBJCOPY = cargo objcopy --
OBJCOPY_PARAMS = --strip-all -O binary

UTILS_CONTAINER = andrerichter/raspi3-utils
DOCKER_CMD = docker run -it --rm -v $(shell pwd):/work -w /work -p 5900:5900
QEMU_CMD = qemu-system-aarch64

# -d in_asm,unimp,int -S
QEMU_OPTS = -M raspi3 -d int
QEMU_SERIAL = -serial null -serial stdio
QEMU = /usr/local/Cellar/qemu/HEAD-3365de01b5-custom/bin/qemu-system-aarch64

GDB = /usr/local/opt/gdb-8.2.1-aarhc64/bin/aarch64-linux-elf-gdb

OPENOCD = /usr/local/openocd-aeb7b327-rtt/bin/openocd

.PHONY: all qemu clippy clean objdump nm

all: kernel8.img

target/$(TARGET)/release/vesper: $(SOURCES)
	cargo xbuild --target=$(TARGET_JSON) --release --features="jlink"

kernel8.img: target/$(TARGET)/release/vesper $(SOURCES)
	cp $< ./kernel8
	$(OBJCOPY) $(OBJCOPY_PARAMS) $< kernel8.img

docker_qemu: all
	$(DOCKER_CMD) $(UTILS_CONTAINER) $(QEMU_CMD) $(QEMU_OPTS) -serial stdio -kernel kernel8.img

qemu: all
	$(QEMU) $(QEMU_OPTS) $(QEMU_SERIAL) -kernel kernel8.img

sdcard: all
	cp kernel8.img /Volumes/BOOT/

sdeject: sdcard
	diskutil unmount /Volumes/BOOT/

clippy:
	cargo xclippy --target=$(TARGET_JSON)

clean:
	cargo clean

objdump:
	cargo objdump --target $(TARGET_JSON) -- -disassemble -print-imm-hex kernel8

nm:
	cargo nm --target $(TARGET_JSON) -- kernel8 | sort

hopper: all
	hopperv4 -l RAW --base-address 0x80000 --entrypoint 0x80000 --file-offset 0 --aarch64 -e kernel8.img

openocd:
	$(OPENOCD) -f interface/jlink.cfg -f ./doc/rpi3_jlink_suse.cfg

openocd_naotako:
	$(OPENOCD) -f interface/jlink.cfg -f ./doc/rpi3_jlink_naotako.cfg

gdb: kernel8.img
	make nm | grep _SEGGER_RTT | awk '{print $$1}' | ./make-gdb-connect.sh
	env RUST_GDB=$(GDB) rust-gdb -x gdb-connect kernel8

gdbdash: kernel8.img
	make nm | grep _SEGGER_RTT | awk '{print $$1}' | ./make-gdb-connect.sh
	env RUST_GDB=$(GDB) rust-gdb -x gdb-connect -x ~/.gdbinit_dashboard kernel8

gdbgui:
	gdbgui -g $(GDB) --gdb-args='--init-eval-command="set startup-with-shell off"'

