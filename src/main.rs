#![feature(proc_macro)]
#![feature(const_fn)]
#![feature(core_intrinsics)]
#![no_std]

extern crate bare_metal;
extern crate cortex_m;
extern crate cortex_m_rt;
extern crate cortex_m_rtfm as rtfm; // <- this rename is required
extern crate cortex_m_semihosting as semihosting;
extern crate stm32f103xx_hal as hal;
extern crate vcell;

use hal::prelude::*;
use hal::rcc::*;
use hal::flash::*;
use hal::stm32f103xx as device;
use rtfm::app;
use rtfm::Threshold;

mod usb;

app! {
    // this is a path to a _device_ crate, a crate generated using svd2rust
    device: device,
    resources: {
        // Thanks to the work by Jorge Aparicio, we have a convenient wrapper
        // for peripherals which means we can declare a PMA peripheral:
        //
        static ON: bool = false;
        static USB: device::USB;
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

//TODO: matrix scan
//systick
//0.0ms on col 0
//0.1ms scan row 0 off col 0
//0.2ms on col 1
//0.2ms scan row 0 off col 1
// Splitting matrix scanning into pieces will keep time spent low enough to not interfere with USB
// without needing different interrupt priorities and critical sections for tracking held keys
// should keep things simple

fn usb_interrupt(_t: &mut rtfm::Threshold, _r: CAN1_RX0::Resources) {
    usb::usb_can1_rx0_interrupt(_t, _r, EventHandler {})
}

struct EventHandler {}

impl usb::UsbEventHandler<CAN1_RX0::Resources> for EventHandler {
    fn get_device_descriptor(
        &self,
        _resources: CAN1_RX0::Resources,
    ) -> &'static usb::UsbDeviceDescriptor {
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

fn init(p: init::Peripherals, _r: init::Resources) -> init::LateResources {
    //let mut stdout = hio::hstdout().unwrap();
    //writeln!(stdout, "init").unwrap();
    //r.USB =

    // TODO: no API in HAL for this yet
    p.device.RCC.apb1enr.modify(|_, w| w.usben().enabled());
    p.device.RCC.apb1rstr.modify(|_, w| w.usbrst().set_bit());
    p.device.RCC.apb1rstr.modify(|_, w| w.usbrst().clear_bit());

    // should p.device.RCC.cfgr.freeze handle this?
    // https://github.com/japaric/stm32f103xx-hal/issues/38
    p.device.RCC.cr.modify(|_, w| w.hseon().enabled());
    while !p.device.RCC.cr.read().hserdy().is_ready() {}
    p.device
        .RCC
        .cfgr
        .modify(|_, w| w.pllsrc().external().pllxtpre().no_div());

    let mut flash: hal::flash::Parts = p.device.FLASH.constrain();
    let mut rcc: hal::rcc::Rcc = p.device.RCC.constrain();
    // enable IO port A clock
    // (USB - and + on pins PA11/PA12 so need GPIO clock A on? maybe this isn't needed?)
    // we definitely need it on to trigger usb reset by making USB DP low output for a short time
    let _gpioa: hal::gpio::gpioa::Parts = p.device.GPIOA.split(&mut rcc.apb2);

    let mut gpioc: hal::gpio::gpioc::Parts = p.device.GPIOC.split(&mut rcc.apb2);
    gpioc.pc13.into_push_pull_output(&mut gpioc.crh);

    let _clocks: Clocks = rcc.cfgr
        .sysclk(72.mhz())
        .hclk(72.mhz())
        .pclk2(72.mhz())
        .pclk1(36.mhz())
        .freeze(&mut flash.acr);

    //blue_pill::led::init(p.GPIOC, p.RCC);
    //ludicrous_speed_now(&p.device.RCC, &p.device.FLASH);
    init_usb(&p.device.USB);
    init::LateResources { USB: p.device.USB }
}

fn init_usb(usb: &device::USB) {
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
}

fn idle() -> ! {
    loop {
        rtfm::wfi();
    }
}
