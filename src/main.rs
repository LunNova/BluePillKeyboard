#![feature(proc_macro)]
#![feature(const_fn)]
#![feature(core_intrinsics)]
#![no_std]

extern crate bare_metal;
extern crate blue_pill;
extern crate cortex_m;
extern crate cortex_m_rtfm as rtfm; // <- this rename is required
extern crate cortex_m_semihosting as semihosting;
extern crate vcell;

use blue_pill::stm32f103xx as device;
use rtfm::app;

mod usb;

app! {
    // this is a path to a _device_ crate, a crate generated using svd2rust
    device: device,
    resources: {
        // Thanks to the work by Jorge Aparicio, we have a convenient wrapper
        // for peripherals which means we can declare a PMA peripheral:
        //
        static ON: bool = false;
    },

    tasks: {
        CAN1_RX0: {
            path: usb_interrupt,

            resources: [USB, ON]
        }
    // USB interrupts:
    // Page 625 DocID13902 Rev 17
    // Interrupt Mapper: This block is used to select how the possible USB events can
    // generate interrupts and map them to three different lines of the NVIC:

    // CAN1_RX0
    // USB low-priority interrupt (Channel 20): Triggered by all USB events (Correct
    // transfer, USB reset, etc.). The firmware has to check the interrupt source before
    // serving the interrupt.

    // CAN1_TX
    // USB high-priority interrupt (Channel 19): Triggered only by a correct transfer event
    // for isochronous and double-buffer bulk transfer to reach the highest possible
    // transfer rate.

    // USB_FS_WKUP
    // USB wakeup interrupt (Channel 42): Triggered by the wakeup event from the USB Suspend mode.
    }
}

fn usb_interrupt(_t: &mut rtfm::Threshold, _r: CAN1_RX0::Resources) {
    usb::usb_can1_rx0_interrupt(_t, _r, EventHandler{})
}

struct EventHandler {}



impl<'a> usb::UsbEventHandler<CAN1_RX0::Resources<'a>> for EventHandler {
    fn get_device_descriptor(&self, resources: CAN1_RX0::Resources) -> &'static usb::UsbDeviceDescriptor {
        static USB_DEVICE_DESCRIPTOR: usb::UsbDeviceDescriptor = usb::UsbDeviceDescriptor {
            specification_version: usb::UsbVersion::new(1, 1, 0),
            device_class: usb::UsbDeviceClass::HID as u8,
            device_sub_class: 0,
            device_protocol: 0,
            max_packet_size_ep0: 0,
            // http://pid.codes/1209/0001/
            vendor_id: 0x1209,
            product_id: 0x0001,
            device_version: usb::UsbVersion::new(0, 0, 1),
            manufacturer: usb::StandardStringIndex::None as u8,
            product: usb::StandardStringIndex::None as u8,
            serial_number: usb::StandardStringIndex::None as u8,
            num_configurations: 0,
        };
        &USB_DEVICE_DESCRIPTOR
    }
}

fn init(p: device::Peripherals, _r: init::Resources) {
    //let mut stdout = hio::hstdout().unwrap();
    //writeln!(stdout, "init").unwrap();
    //r.USB =
    blue_pill::led::init(p.GPIOC, p.RCC);
    ludicrous_speed_now(p.RCC, p.FLASH);
    init_usb(p.RCC, p.USB);
}

fn init_usb(rcc: &device::RCC, usb: &device::USB) {
    // enable IO port A clock (USB - and + on pins PA11/PA12 so need GPIO clock A on? maybe this isn't needed?)
    // we definitely need it on to trigger usb reset by making USB DP low output for a short time
    rcc.apb2enr.modify(|_, w| w.iopaen().enabled());

    // enable usb clocks
    rcc.apb1enr.modify(|_, w| w.usben().enabled());
    rcc.apb1rstr.modify(|_, w| w.usbrst().set_bit());
    rcc.apb1rstr.modify(|_, w| w.usbrst().clear_bit());

    //gpioa.crh.read().cnf10();

    usb.istr.reset();
    usb.ep0r
        .modify(|_, w| w.ep_type().control().ep_kind().clear_bit());
    usb.btable.reset();
    usb.cntr.write(|w| w
        .fres().set_bit()
        .resetm().set_bit()
        .errm().set_bit()
        .sofm().set_bit()
        .ctrm().set_bit()
        .suspm().set_bit()
        .wkupm().set_bit()
        //.pdwn().set_bit()
    );

    // must wait tSTARTUP (1us) before reset
    for _ in 0..(1 << 16) {
        cortex_m::asm::nop();
    }

    reset_usb(usb);
}

fn reset_usb(usb: &device::USB) {
    usb.cntr.modify(|_, w| w.fres().set_bit());
    usb.cntr.modify(|_, w| w.fres().clear_bit());
    //usb.daddr.modify(| _, w | w.ef().set_bit());
    //usb.ep0r.write(| w | w.)
}

/// Need to use external clock source + PLL to run at clock speed which allows USB
fn ludicrous_speed_now(rcc: &device::RCC, flash: &device::FLASH) {
    rcc.cr.modify(|_, w| w.hseon().enabled());
    while !rcc.cr.read().hserdy().is_ready() {}
    flash.acr.modify(|_, w| w.prftbe().enabled());
    flash.acr.modify(|_, w| w.latency().two());
    rcc.cfgr.modify(|_, w| w
        .hpre().div1()
        .ppre2().div1()
        .ppre1().div2()
        //.adcpre().bits(8)
        .pllsrc().external()
        .pllxtpre().div1()
        .pllmul().mul9()
    );
    rcc.cr.modify(|_, w| w.pllon().enabled());
    while rcc.cr.read().pllrdy().is_unlocked() {}
    rcc.cfgr.modify(|_, w| w.sw().pll());
    while !rcc.cfgr.read().sws().is_pll() {}
}

fn idle() -> ! {
    loop {
        rtfm::wfi();
    }
}
