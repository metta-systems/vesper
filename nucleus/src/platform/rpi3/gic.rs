/*

rpi-3-b-plus FDT:

interrupt-controller@7e00b200 {
    compatible = "brcm,bcm2836-armctrl-ic";
    reg = <0x7e00b200 0x00000200>;
    interrupt-controller;
    #interrupt-cells = <0x00000002>;
    interrupt-parent = <0x00000018>;
    interrupts = <0x00000008 0x00000004>;
    phandle = <0x00000001>;
};

local_intc@40000000 {
    compatible = "brcm,bcm2836-l1-intc";
    reg = <0x40000000 0x00000100>;
    interrupt-controller;
    #interrupt-cells = <0x00000002>;
    interrupt-parent = <0x00000018>;
    phandle = <0x00000018>;
};

timer {
    compatible = "arm,armv7-timer";
    interrupt-parent = <0x00000018>;
    interrupts = <0x00000000 0x00000004 0x00000001 0x00000004 0x00000003 0x00000004 0x00000002 0x00000004>;
    always-on;
};

rpi-400 FDT:

interrupt-controller@40041000 {
    interrupt-controller;
    #interrupt-cells = <0x00000003>;
    compatible = "arm,gic-400";
    reg = <0x40041000 0x00001000 0x40042000 0x00002000 0x40044000 0x00002000 0x40046000 0x00002000>;
    interrupts = <0x00000001 0x00000009 0x00000f04>;
    phandle = <0x00000001>;
};

local_intc@40000000 {
    compatible = "brcm,bcm2836-l1-intc";
    reg = <0x40000000 0x00000100>;
    phandle = <0x000000b2>;
};

timer@7e003000 {
    compatible = "brcm,bcm2835-system-timer";
    reg = <0x7e003000 0x00001000>;
    interrupts = <0x00000000 0x00000040 0x00000004 0x00000000 0x00000041 0x00000004 0x00000000 0x00000042 0x00000004 0x00000000 0x00000043 0x00000004>;
    clock-frequency = <0x000f4240>;
    phandle = <0x00000044>;
};

*/

pub fn get_active_irq() -> core::Result<u16> {}
