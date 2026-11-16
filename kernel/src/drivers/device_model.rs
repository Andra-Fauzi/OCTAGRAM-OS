//! Device/driver binding shared by the PCI and USB buses.

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::any::Any;
use spin::Mutex;

use super::pci::PciDevice;
use super::usb::{EndpointInfo, UsbControllerId};

pub type DriverState = Arc<dyn Any + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceKey {
    Pci {
        bus: u8,
        device: u8,
        function: u8,
    },
    UsbInterface {
        controller: UsbControllerId,
        slot_id: u32,
        interface_number: u8,
    },
}

#[derive(Clone)]
pub struct UsbInterfaceInfo {
    pub controller: UsbControllerId,
    pub slot_id: u32,
    pub root_port: u8,
    pub number: u8,
    pub class: u8,
    pub subclass: u8,
    pub protocol: u8,
    pub endpoints: Vec<EndpointInfo>,
    pub configuration_endpoints: Vec<EndpointInfo>,
}

#[derive(Clone)]
pub enum DeviceInfo {
    Pci(PciDevice),
    UsbInterface(UsbInterfaceInfo),
}

impl DeviceInfo {
    pub fn key(&self) -> DeviceKey {
        match self {
            Self::Pci(device) => DeviceKey::Pci {
                bus: device.bus,
                device: device.device,
                function: device.function,
            },
            Self::UsbInterface(interface) => DeviceKey::UsbInterface {
                controller: interface.controller,
                slot_id: interface.slot_id,
                interface_number: interface.number,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriverError {
    Unsupported,
    ProbeFailed(&'static str),
}

pub trait DeviceDriver: Sync {
    fn name(&self) -> &'static str;
    fn matches(&self, device: &DeviceInfo) -> bool;

    /// Probe runs outside the registry lock and must not run from an ISR.
    ///
    /// # Safety
    /// The bus must have initialized the device and its resources.
    unsafe fn probe(&self, device: &DeviceInfo) -> Result<DriverState, DriverError>;

    /// Stop hardware activity before the bus releases the device.
    ///
    /// # Safety
    /// The device must no longer receive new operations from its bus.
    unsafe fn remove(&self, device: &DeviceInfo, state: DriverState);
}

struct Binding {
    driver: &'static dyn DeviceDriver,
    state: DriverState,
}

struct DeviceRecord {
    info: DeviceInfo,
    binding: Option<Binding>,
    probing: bool,
}

#[derive(Default)]
struct Registry {
    drivers: Vec<&'static dyn DeviceDriver>,
    devices: Vec<DeviceRecord>,
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    drivers: Vec::new(),
    devices: Vec::new(),
});

/// Register a built-in driver and try it against devices that are unbound.
///
/// # Safety
/// Call only after the driver dependencies are initialized and outside an ISR.
pub unsafe fn register_driver(driver: &'static dyn DeviceDriver) -> Result<(), &'static str> {
    let candidates = {
        let mut registry = REGISTRY.lock();
        if registry
            .drivers
            .iter()
            .any(|registered| registered.name() == driver.name())
        {
            return Err("driver name already registered");
        }
        registry.drivers.push(driver);
        registry
            .devices
            .iter()
            .filter(|record| record.binding.is_none() && !record.probing)
            .map(|record| record.info.clone())
            .collect::<Vec<_>>()
    };

    for device in candidates {
        try_bind(&device, driver);
    }
    Ok(())
}

/// Add a bus-enumerated device and try registered drivers against it.
///
/// # Safety
/// The bus must have initialized the device and its resources.
pub unsafe fn register_device(device: DeviceInfo) -> Result<(), &'static str> {
    let drivers = {
        let mut registry = REGISTRY.lock();
        let key = device.key();
        if registry
            .devices
            .iter()
            .any(|record| record.info.key() == key)
        {
            return Err("device already registered");
        }
        registry.devices.push(DeviceRecord {
            info: device.clone(),
            binding: None,
            probing: false,
        });
        registry.drivers.clone()
    };

    for driver in drivers {
        if try_bind(&device, driver) {
            break;
        }
    }
    Ok(())
}

fn try_bind(device: &DeviceInfo, driver: &'static dyn DeviceDriver) -> bool {
    if !driver.matches(device) {
        return false;
    }
    let key = device.key();
    {
        let mut registry = REGISTRY.lock();
        let Some(record) = registry
            .devices
            .iter_mut()
            .find(|record| record.info.key() == key)
        else {
            return false;
        };
        if record.binding.is_some() || record.probing {
            return false;
        }
        record.probing = true;
    }

    let result = unsafe { driver.probe(device) };
    let mut registry = REGISTRY.lock();
    let Some(record) = registry
        .devices
        .iter_mut()
        .find(|record| record.info.key() == key)
    else {
        drop(registry);
        if let Ok(state) = result {
            unsafe { driver.remove(device, state) };
        }
        return false;
    };
    record.probing = false;
    match result {
        Ok(state) => {
            record.binding = Some(Binding { driver, state });
            true
        }
        Err(DriverError::Unsupported) => false,
        Err(DriverError::ProbeFailed(error)) => {
            drop(registry);
            crate::println!(
                "driver {} probe failed for {:?}: {}",
                driver.name(),
                key,
                error
            );
            false
        }
    }
}

/// Unbind and remove a device before its bus releases the resources.
///
/// # Safety
/// The bus must stop submitting new operations for this device first.
pub unsafe fn remove_device(key: DeviceKey) -> Result<bool, &'static str> {
    let record = {
        let mut registry = REGISTRY.lock();
        let Some(index) = registry
            .devices
            .iter()
            .position(|record| record.info.key() == key)
        else {
            return Ok(false);
        };
        if registry.devices[index].probing {
            return Err("device probe is still running");
        }
        registry.devices.remove(index)
    };

    if let Some(binding) = record.binding {
        unsafe { binding.driver.remove(&record.info, binding.state) };
    }
    Ok(true)
}

#[allow(dead_code)]
pub fn driver_state<T: Any + Send + Sync>(key: DeviceKey) -> Option<Arc<T>> {
    let registry = REGISTRY.lock();
    let state = registry
        .devices
        .iter()
        .find(|record| record.info.key() == key)?
        .binding
        .as_ref()?
        .state
        .clone();
    Arc::downcast(state).ok()
}

pub fn is_device_bound(key: DeviceKey) -> bool {
    REGISTRY
        .lock()
        .devices
        .iter()
        .find(|record| record.info.key() == key)
        .and_then(|record| record.binding.as_ref())
        .is_some()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceSnapshot {
    pub key: DeviceKey,
    pub driver_name: Option<&'static str>,
}

pub fn devices() -> Vec<DeviceSnapshot> {
    REGISTRY
        .lock()
        .devices
        .iter()
        .map(|record| DeviceSnapshot {
            key: record.info.key(),
            driver_name: record.binding.as_ref().map(|binding| binding.driver.name()),
        })
        .collect()
}

pub fn drivers() -> Vec<&'static str> {
    REGISTRY
        .lock()
        .drivers
        .iter()
        .map(|driver| driver.name())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        DeviceDriver, DeviceInfo, DriverError, DriverState, devices, driver_state, drivers,
        register_device, register_driver, remove_device,
    };
    use crate::drivers::pci::PciDevice;
    use alloc::sync::Arc;
    use core::sync::atomic::{AtomicBool, Ordering};

    struct TestDriver;
    struct LateTestDriver;

    static TEST_DRIVER: TestDriver = TestDriver;
    static LATE_TEST_DRIVER: LateTestDriver = LateTestDriver;
    static REMOVED: AtomicBool = AtomicBool::new(false);

    impl DeviceDriver for TestDriver {
        fn name(&self) -> &'static str {
            "device-model-test"
        }

        fn matches(&self, device: &DeviceInfo) -> bool {
            matches!(device, DeviceInfo::Pci(device) if device.vendor_id == 0xCAFE)
        }

        unsafe fn probe(&self, _device: &DeviceInfo) -> Result<DriverState, DriverError> {
            Ok(Arc::new(41u32))
        }

        unsafe fn remove(&self, _device: &DeviceInfo, state: DriverState) {
            REMOVED.store(state.downcast_ref::<u32>() == Some(&41), Ordering::SeqCst);
        }
    }

    impl DeviceDriver for LateTestDriver {
        fn name(&self) -> &'static str {
            "device-model-late-test"
        }

        fn matches(&self, device: &DeviceInfo) -> bool {
            matches!(device, DeviceInfo::Pci(device) if device.vendor_id == 0xCAFF)
        }

        unsafe fn probe(&self, _device: &DeviceInfo) -> Result<DriverState, DriverError> {
            Ok(Arc::new(()))
        }

        unsafe fn remove(&self, _device: &DeviceInfo, _state: DriverState) {}
    }

    #[test]
    fn registered_driver_probes_and_removes_matching_device_state() {
        let device = DeviceInfo::Pci(PciDevice {
            bus: 0xFF,
            device: 31,
            function: 7,
            vendor_id: 0xCAFE,
            device_id: 1,
            class: 0,
            subclass: 0,
            prog_if: 0,
            header_type: 0,
        });
        let key = device.key();
        REMOVED.store(false, Ordering::SeqCst);

        unsafe {
            register_driver(&TEST_DRIVER).unwrap();
            register_device(device).unwrap();
        }

        assert_eq!(*driver_state::<u32>(key).unwrap(), 41);
        assert!(devices().iter().any(|device| device.key == key));
        assert!(drivers().contains(&"device-model-test"));
        assert_eq!(unsafe { remove_device(key) }, Ok(true));
        assert!(REMOVED.load(Ordering::SeqCst));
        assert!(!devices().iter().any(|device| device.key == key));
    }

    #[test]
    fn late_driver_registration_binds_existing_unbound_device() {
        let device = DeviceInfo::Pci(PciDevice {
            bus: 0xFE,
            device: 30,
            function: 6,
            vendor_id: 0xCAFF,
            device_id: 1,
            class: 0,
            subclass: 0,
            prog_if: 0,
            header_type: 0,
        });
        let key = device.key();

        unsafe {
            register_device(device).unwrap();
            register_driver(&LATE_TEST_DRIVER).unwrap();
        }

        assert!(devices().iter().any(
            |device| device.key == key && device.driver_name == Some("device-model-late-test")
        ));
        assert_eq!(unsafe { remove_device(key) }, Ok(true));
    }
}
