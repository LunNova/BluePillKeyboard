use super::device as device;
use cortex_m;
use bare_metal::Peripheral;
use vcell::VolatileCell;
use core;
use core::marker::PhantomData;
use rtfm;

type EndpointIndex = u8;
type PmaAddress = u16;
const PMA_SIZE: PmaAddress = 512;
const ENDPOINTS: EndpointIndex = 8;
type StringIndex = u8;
const PMA: Peripheral<PMA> = unsafe { Peripheral::new(0x4000_6000) };

// The USB descriptors with a header are sort of a tagged union with a length field.
// not sure how best to represent this with a struct - probably treat the header separately then
// the descriptor part without the length and ID can be its own struct.
#[repr(u8)]
enum UsbDescriptorType {
    DEVICE = 0
}

/*
#[repr(C, packed)]
struct UsbDescriptorHeader<DescriptorType: UsbDescriptorTypeProvider> {
    length: u8,
    descriptor_type: UsbDescriptorType,
    _ignored: PhantomData<*const DescriptorType>
}

#[repr(C, packed)]
struct UsbDescriptorWithHeader<DescriptorType: UsbDescriptorTypeProvider> {
    header: UsbDescriptorHeader<DescriptorType>,
    descriptor: DescriptorType
    //descriptor: T
}

impl <DescriptorType: UsbDescriptorTypeProvider> UsbDescriptorHeader<DescriptorType> {
    const fn length() -> usize {
        core::mem::size_of::<UsbDescriptorWithHeader<DescriptorType>>()
    }

//    fn new(descriptor: T) -> UsbDescriptor<T> {
//        UsbDescriptor::<T> { length: , descriptor_type: <T as UsbDescriptorTypeProvider>::get_id(), descriptor }
//    }
}

//trait UsbDescriptorTypeProvider {
//    fn get_id() -> UsbDescriptorType;
//}
*/

#[repr(C, packed)]
struct UsbDeviceDescriptor {
    specification_version: UsbVersion,
    device_class: u8,
    device_sub_class: u8,
    device_protocol: u8,
    max_packet_size_ep0: u8,
    vendor_id: u16,
    product_id: u16,
    device_version: UsbVersion,
    /// String table references
    manufacturer: StringIndex,
    product: StringIndex,
    serial_number: StringIndex,
    num_configurations: u8
}

#[repr(C, packed)]
struct UsbQualifierDescriptor {
    specification_version: UsbVersion,
    device_class: u8,
    device_sub_class: u8,
    device_protocol: u8,
    max_packet_size_ep0: u8,
    num_configurations: u8,
    reserved: u8
}

#[repr(C, packed)]
struct UsbConfigDescriptor {
    // total_length: u16 - redundant?
    interface_count: u8,
    configuration_index: u8,
    configuration_description: StringIndex,
    // TODO: should be bitflags!
    attributes: u8,
    max_power: UsbPowerMilliAmps
}

#[repr(C, packed)]
struct UsbPowerMilliAmps {
    value: u8
}

impl UsbPowerMilliAmps {
    const fn new(milli_amps: u8) -> UsbPowerMilliAmps {
        UsbPowerMilliAmps { value: milli_amps >> 1 }
    }

    const fn milli_amps(&self) -> u8 {
        self.value << 1
    }
}

#[repr(C, packed)]
struct UsbVersion {
    value: u16
}

impl UsbVersion {
    const MAJOR_POSITION: u16 = 8;
    const MINOR_POSITION: u16 = 4;

    const fn new(major: u8, minor: u8, revision: u8) -> UsbVersion {
        UsbVersion { value: ((major as u16 & 0xFFu16) << UsbVersion::MAJOR_POSITION) | ((minor as u16 & 0x0Fu16) << UsbVersion::MINOR_POSITION) | revision as u16 & 0x0fu16 }
    }

    const fn major(&self) -> u8 {
        ((self.value & (0xFFu16 << UsbVersion::MAJOR_POSITION)) >> UsbVersion::MAJOR_POSITION) as u8
    }

    const fn minor(&self) -> u8 {
        ((self.value & (0xFFu16 << UsbVersion::MINOR_POSITION)) >> UsbVersion::MINOR_POSITION) as u8
    }

    const fn revision(&self) -> u8 {
        (self.value & 0xFFu16) as u8
    }
}

/// Gets endpoint register by ID - treats all as EP0R so can be treated as same type
#[inline(always)]
fn get_ep(usb: &device::USB, ep: u8) -> &device::usb::EP0R {
    debug_assert!(ep < 8);
    unsafe {
        let ptr: *const device::usb::EP0R = &usb.ep0r;
        &*(ptr.offset(ep as isize))
    }
}

struct UsbDevice<Resources, EventHandler: UsbEventHandler<Resources>> {
    event_handler: EventHandler,
    phantom: PhantomData<*const Resources>
}

fn reset(usb: &device::USB) {
    let pma: &mut PMA = unsafe { &mut*PMA.get() };
    for i in 0..ENDPOINTS {
        get_ep(&usb, i).reset();
        {
            pma.set_rxaddr(i, 0);
        }
        {
            pma.set_txaddr(i, 0);
        }
    }
    usb.daddr.reset();
}

impl <Resources, EventHandler: UsbEventHandler<Resources>> UsbDevice<Resources, EventHandler> {
    fn usb_interrupt(&mut self, _t: &mut rtfm::Threshold, r: Resources) {
        //let mut stdout = hio::hstdout().unwrap();
        let usb: &device::USB = unsafe { &*device::USB.get() };

        //let pma: &mut PMA = unsafe { &mut*PMA.get() };

        let istr_read = usb.istr.read();

        if istr_read.reset().bit_is_set() {
            // need to reset
            reset(usb);
        } else {
            let ep = get_ep(&usb, istr_read.ep_id().bits());

            // CTR CORRECT TRANSFTER
            if istr_read.ctr().bit_is_set() {
                ep.modify(|ep_read, w| {
                    if ep_read.ctr_rx().bit_is_set() {
                        if ep_read.setup().bit_is_set() {
                            // SETUP
                            self.event_handler.get_device_descriptor(r);
                        } else {
                            // RX
                        }
                    } else if ep_read.ctr_rx().bit_is_set() {
                        // TX
                    } else {
                        // Should be RX or TX, something went wrong?
                        cortex_m::asm::bkpt();
                    }
                    w
                });
            }
        }

        usb.istr.reset();
    }
}

trait UsbEventHandler<Resources> {
    fn get_device_descriptor(&self, resources: Resources) -> UsbDeviceDescriptor;
}

// PMA def was from
// https://blog.digital-scurf.org/posts/stm32-usb-in-rust-pma/
// The PMA struct type which the peripheral will return a ref to
// This is the actual representation of the peripheral, we use the C repr
// in order to ensure it ends up packed nicely together
#[repr(C)]
pub struct PMA {
    // The PMA consists of 256 u16 words separated by u16 gaps, so lets
    // represent that as 512 u16 words which we'll only use every other of.
    words: [VolatileCell<u16>; PMA_SIZE as usize],
}

impl PMA {
    #[inline(always)]
    pub fn get_u16(&self, offset: PmaAddress) -> u16 {
        //debug_assert_eq!((offset & 0x01), 0);
        self.words[offset as usize].get()
    }

    // FIXME: We take &mut self to write - but stm32f crate's peripherals allow writing with non-mut reference...
    // why? should do same?
    #[inline(always)]
    pub fn set_u16(&mut self, offset: u16, val: u16) {
        //debug_assert_eq!((offset & 0x01), 0);
        self.words[offset as usize].set(val);
    }

    #[inline(always)]
    const fn offset(ep: EndpointIndex) -> u16 {
        //assert!(ep < ENDPOINTS);
        ep as u16 * 8
    }

    /// get USB_ADDRn_TX
    pub fn get_txaddr(&self, ep: EndpointIndex) -> u16 {
        self.get_u16(PMA::offset(ep))
    }

    /// set USB_ADDRn_TX
    pub fn set_txaddr(&mut self, ep: EndpointIndex, val: u16) {
        self.set_u16(PMA::offset(ep), val)
    }

    /// get USB_COUNTn_TX
    pub fn get_txcount(&self, ep: EndpointIndex) -> u16 {
        self.get_u16(PMA::offset(ep) + 2)
    }

    /// get USB_COUNTn_TX
    pub fn set_txcount(&mut self, ep: EndpointIndex) -> u16 {
        self.get_u16(PMA::offset(ep) + 2)
    }

    /// get USB_ADDRn_RX
    pub fn get_rxaddr(&self, ep: EndpointIndex) -> u16 {
        self.get_u16(PMA::offset(ep) + 4)
    }

    /// set USB_ADDRn_RX
    pub fn set_rxaddr(&mut self, ep: EndpointIndex, val: u16) {
        self.set_u16(PMA::offset(ep) + 4, val)
    }

    /// get USB_COUNTn_RX
    /// Retrieves only COUNTn_RX part, ignores BLSIZE and NUM_BLOCK
    pub fn get_rxcount(&self, ep: u8) -> u16 {
        self.get_u16(PMA::offset(ep) + 6) & 0x3ff
    }

    /// set USB_COUNTn_RX - max size can be received for this endpoint
    /// <=512 bytes, 64 or above increase in powers of two
    /// Sets BLSIZE and NUM_BLOCK
    /// See Table 177. Definition of allocated buffer memory
    /// FIXME: is 512 bytes actually possible? Packet memory area is only 512 bytes total
    /// but 64 bytes are used by the buffer descriptor table?
    pub fn set_rxcount(&mut self, ep: EndpointIndex, val: u16) {
        self.set_u16(PMA::offset(ep) + 6, PMA::calc_rxcount(val))
    }

    fn calc_rxcount(val: u16) -> u16 {
        if val > 62 {
            assert!(val <= 512);
            assert_eq!((val & 0x1f), 0);
            (((val >> 5) - 1) << 10) | 0x8000
        } else {
            assert_eq!((val & 1), 0);
            (val >> 1) << 10
        }
    }

    pub fn write_buffer(&mut self, base: PmaAddress, buf: &[u16]) {
        for (ofs, v) in buf.iter().enumerate() {
            self.set_u16(base + (ofs * 2) as PmaAddress, *v);
        }
    }



    pub fn get_next_buffer(&self, size: PmaAddress) -> PmaAddress {
        let mut result: PmaAddress = PMA_SIZE;
        for i in 0 .. ENDPOINTS {
            result = min_if_set(min_if_set(result, self.get_txaddr(i)), self.get_rxaddr(i));
        }
        if result < size + PMA::offset(ENDPOINTS + 1) {
            panic!("Not enough space in PMA for buffer of size {}", size);
        }
        result
    }
}

fn min_if_set(result: PmaAddress, addr: PmaAddress) -> PmaAddress {
    if unsafe { core::intrinsics::likely(addr == 0 || result < addr) } {
        return result;
    } else {
        return addr;
    }
}