#![feature(proc_macro)]
#![feature(const_fn)]
#![feature(core_intrinsics)]
#![no_std]

extern crate cortex_m_rtfm as rtfm; // <- this rename is required
extern crate cortex_m_semihosting as semihosting;
extern crate cortex_m;
extern crate blue_pill;
extern crate bare_metal;
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

}

fn init(p: device::Peripherals, r: init::Resources) {
    //let mut stdout = hio::hstdout().unwrap();
    //writeln!(stdout, "init").unwrap();
    blue_pill::led::init(p.GPIOC, p.RCC);
    ludicrous_speed_now(p.RCC, p.FLASH);
    init_usb(p.RCC, p.USB);
}

fn init_usb(rcc: &device::RCC, usb: &device::USB) {
    // enable IO port A clock (USB - and + on pins PA11/PA12 so need GPIO clock A on? maybe this isn't needed?)
    // we definitely need it on to trigger usb reset by making USB DP low output for a short time
    rcc.apb2enr.modify(|_, w| w.iopaen().enabled());

    // enable usb clocks
    rcc.apb1enr.modify( | _, w | w.usben().enabled());
    rcc.apb1rstr.modify(| _, w | w.usbrst().set_bit());
    rcc.apb1rstr.modify(| _, w | w.usbrst().clear_bit());

    //gpioa.crh.read().cnf10();

    usb.istr.reset();
    usb.ep0r.modify(| _, w | w.ep_type().control().ep_kind().clear_bit());
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
    for _ in 0..(1<<16) {
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
    rcc.cfgr.modify(|_,w| w.sw().pll());
    while !rcc.cfgr.read().sws().is_pll() {}
}

fn idle() -> ! {
    loop { rtfm::wfi(); }
}
