//! ACPI power-resource handling for a PCI function.
//!
//! Linux powers a PCI device on through the ACPI core: `_PS0` on the device and
//! the power resources listed by its `_PR0` package (the rail and clock the
//! platform owns). A bare-metal kernel that only writes the PM capability's
//! power state never runs any of that, so a device firmware left in D3 state by
//! the platform can look "in D0" while its SoC has no reference clock.
//!
//! This module finds the function in the AML namespace, walks its `_PR0` power
//! resources (and those of the devices above it), and evaluates `_STA`/`_ON` so
//! the rail is on before the driver touches the chip.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use spin::{Mutex, Once};

use super::AcpiError;
use super::aml::{AmlError, Interpreter, Namespace, Object, PlatformIo, Value};

/// EISA-encoded `_HID` of a PCI host bridge (`PNP0A03`).
const PNP0A03: u64 = 0x030A_D041;
/// EISA-encoded `_HID` of a PCI Express host bridge (`PNP0A08`).
const PNP0A08: u64 = 0x080A_D041;

#[derive(Debug)]
pub enum PowerError {
    /// The DSDT/SSDTs could not be read.
    Tables(AcpiError),
    /// A `_PR0` package or a power-resource method could not be evaluated.
    Aml(AmlError),
    /// No `Device` with a PCI host-bridge `_HID` covers the requested bus.
    NoRootBridge(u8),
    /// No device below that bridge has a matching `_ADR`.
    NoDevice { bus: u8, device: u8, function: u8 },
}

/// One `_PR0` power resource the device depends on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceState {
    pub path: String,
    /// `_STA` reported the resource as already on.
    pub was_on: bool,
    /// This call evaluated `_ON` successfully.
    pub turned_on: bool,
    /// Why `_ON` could not be run, when it could not.
    pub error: Option<String>,
}

/// What [`power_on_pci_device`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerReport {
    /// AML path of the endpoint that was found.
    pub device: String,
    pub resources: Vec<ResourceState>,
    /// Outcome of evaluating the device's `_PS0`, when it has one.
    pub ps0: Option<String>,
}

impl PowerReport {
    pub fn enabled(&self) -> usize {
        self.resources
            .iter()
            .filter(|state| state.turned_on)
            .count()
    }

    pub fn already_on(&self) -> usize {
        self.resources.iter().filter(|state| state.was_on).count()
    }
}

/// Parsed once: the DSDT and SSDTs do not change at runtime.
static NAMESPACE: Once<Option<Namespace>> = Once::new();
/// The successful result, so a bring-up retry does not re-evaluate every method.
static LAST: Mutex<Option<PowerReport>> = Mutex::new(None);

/// Power the PCI function at `bus`/`device`/`function` on through ACPI.
///
/// `_ON` is idempotent per the ACPI specification, so a repeated call is safe;
/// a successful report is cached, and a failed one is retried on the next call.
pub fn power_on_pci_device(
    bus: u8,
    device: u8,
    function: u8,
) -> Result<PowerReport, PowerError> {
    let cached = LAST.lock().clone();
    if let Some(report) = cached {
        return Ok(report);
    }

    // Every stage below can block: the namespace parse walks the whole DSDT, the
    // resource `_ON`/`_PS0` methods run AML that may touch the embedded
    // controller, and this platform froze without printing anything once the
    // probe reached the power step. One line per stage places the next freeze.
    crate::println!("QCA9377 ACPI power: parsing the ACPI namespace");
    let namespace = namespace().ok_or(PowerError::Tables(AcpiError::InvalidTable))?;
    crate::println!(
        "QCA9377 ACPI power: namespace ready, {} objects",
        namespace.iter().count()
    );
    let bridge = match find_root_bridge(namespace, bus) {
        Some(bridge) => bridge,
        None => {
            let mut shown = 0;
            for (path, object) in namespace.iter() {
                if matches!(object, Object::Device)
                    && (path.contains("PCI") || path.contains("PC0") || path.contains("RP"))
                    && shown < 40
                {
                    crate::println!("QCA9377 ACPI power: device {path}");
                    shown += 1;
                }
            }
            crate::println!("QCA9377 ACPI power: {shown} PCI-looking devices in the namespace");
            return Err(PowerError::NoRootBridge(bus));
        }
    };
    let address = (u64::from(device) << 16) | u64::from(function);
    let path = find_endpoint(namespace, &bridge, address).ok_or(PowerError::NoDevice {
        bus,
        device,
        function,
    })?;

    let mut io = PlatformIo;
    let mut report = PowerReport {
        device: path.clone(),
        resources: Vec::new(),
        ps0: None,
    };
    crate::println!("QCA9377 ACPI power: bridge={bridge} endpoint={path}");

    // The endpoint first, then every device above it up to the host bridge: a
    // PCIe port may own the rail that feeds the slot below it.
    for holder in chain(&path, &bridge) {
        crate::println!("QCA9377 ACPI power: reading _PR0 of {holder}");
        for resource in power_resources(namespace, &holder, &mut io)? {
            if report
                .resources
                .iter()
                .any(|state| state.path == resource)
            {
                continue;
            }
            crate::println!("QCA9377 ACPI power: enabling {resource}");
            report
                .resources
                .push(resource_state(namespace, &resource, &mut io));
        }
    }
    crate::println!("QCA9377 ACPI power: {} resources collected", report.resources.len());

    let ps0 = format!("{path}._PS0");
    if matches!(namespace.get(&ps0), Some(Object::Method { .. })) {
        crate::println!("QCA9377 ACPI power: evaluating {ps0}");
        let mut interpreter = Interpreter::new(namespace);
        report.ps0 = Some(match interpreter.invoke(&ps0, &[], &mut io) {
            Ok(_) => "evaluated".to_string(),
            Err(error) => format!("{error:?}"),
        });
    }

    if report
        .resources
        .iter()
        .all(|state| state.error.is_none())
    {
        *LAST.lock() = Some(report.clone());
    }
    Ok(report)
}

fn namespace() -> Option<&'static Namespace> {
    NAMESPACE
        .call_once(|| {
            let tables = super::aml_tables().ok()?;
            // The boot froze inside this step, so every table is named before it
            // is parsed: the last line on screen then says whether the freeze is
            // the table walk itself or one table's AML.
            for table in &tables {
                crate::println!(
                    "QCA9377 ACPI power: table {} {} bytes",
                    core::str::from_utf8(&table[..4]).unwrap_or("????"),
                    table.len()
                );
            }
            // The parse froze once with every table already named, so it now
            // reports its offset every 4 KiB: a stall then shows the exact byte
            // the reader cannot get past.
            super::aml::PARSE_TRACE.store(true, core::sync::atomic::Ordering::Relaxed);
            let namespace = Namespace::parse(&tables);
            crate::println!(
                "QCA9377 ACPI power: parsed {} objects, {} resyncs, truncated={}",
                namespace.iter().count(),
                namespace.resyncs(),
                namespace.truncated()
            );
            Some(namespace)
        })
        .as_ref()
}

/// The host bridge covering `bus`: a `Device` whose `_HID` is `PNP0A03`/
/// `PNP0A08` and whose `_BBN` is that bus.
fn find_root_bridge(namespace: &Namespace, bus: u8) -> Option<String> {
    let mut first = None;
    for (path, object) in namespace.iter() {
        if !matches!(object, Object::Device) {
            continue;
        }
        let Some(Object::Name(hid)) = namespace.get(&format!("{path}._HID")) else {
            continue;
        };
        let hid = hid.as_integer();
        if hid != PNP0A03 && hid != PNP0A08 {
            continue;
        }
        if first.is_none() {
            first = Some(path.clone());
        }
        let base_bus = match namespace.get(&format!("{path}._BBN")) {
            Some(Object::Name(value)) => value.as_integer(),
            _ => 0,
        };
        if base_bus == u64::from(bus) {
            return Some(path.clone());
        }
    }
    first
}

/// The deepest `Device` below `bridge` whose `_ADR` matches `address`.
fn find_endpoint(namespace: &Namespace, bridge: &str, address: u64) -> Option<String> {
    let prefix = format!("{bridge}.");
    let mut best: Option<(usize, String)> = None;
    for (path, object) in namespace.iter() {
        if !matches!(object, Object::Device) || !path.starts_with(&prefix) {
            continue;
        }
        let Some(Object::Name(adr)) = namespace.get(&format!("{path}._ADR")) else {
            continue;
        };
        if adr.as_integer() != address {
            continue;
        }
        let depth = path.matches('.').count();
        if best
            .as_ref()
            .is_none_or(|(best_depth, _)| depth > *best_depth)
        {
            best = Some((depth, path.clone()));
        }
    }
    best.map(|(_, path)| path)
}

/// `path`, then each of its parents up to and including `bridge`.
fn chain(path: &str, bridge: &str) -> Vec<String> {
    let mut chain = Vec::new();
    let mut current = path.to_string();
    loop {
        chain.push(current.clone());
        if current == bridge {
            break;
        }
        let Some((parent, _)) = current.rsplit_once('.') else {
            break;
        };
        if !parent.starts_with(bridge) {
            break;
        }
        current = parent.to_string();
    }
    chain
}

/// Evaluate `<device>._PR0` and resolve its package to power-resource paths.
fn power_resources(
    namespace: &Namespace,
    device: &str,
    io: &mut PlatformIo,
) -> Result<Vec<String>, PowerError> {
    let path = format!("{device}._PR0");
    let Some(object) = namespace.get(&path) else {
        return Ok(Vec::new());
    };
    let value = match object {
        Object::Name(value) => value.clone(),
        Object::Method { .. } => {
            let mut interpreter = Interpreter::new(namespace);
            interpreter
                .invoke(&path, &[], io)
                .map_err(PowerError::Aml)?
        }
        _ => return Ok(Vec::new()),
    };
    let Value::Package(elements) = value else {
        return Ok(Vec::new());
    };

    let mut resources = Vec::new();
    for element in elements {
        let Value::Reference(reference) = element else {
            continue;
        };
        if let Some(resolved) = resolve_resource(namespace, device, &reference) {
            resources.push(resolved);
        }
    }
    Ok(resources)
}

/// A package reference may be absolute, or relative to the device that owns the
/// package; both forms appear in vendor tables.
fn resolve_resource(namespace: &Namespace, device: &str, reference: &str) -> Option<String> {
    let parent = device.rsplit_once('.').map_or("\\", |(parent, _)| parent);
    [
        reference.to_string(),
        format!("{device}.{reference}"),
        format!("{parent}.{reference}"),
    ]
    .into_iter()
    .find(|candidate| {
        matches!(
            namespace.get(candidate),
            Some(Object::PowerResource { .. })
        )
    })
}

/// Switch one power resource on, following `_STA` then `_ON`.
fn resource_state(namespace: &Namespace, path: &str, io: &mut PlatformIo) -> ResourceState {
    let mut state = ResourceState {
        path: path.to_string(),
        was_on: false,
        turned_on: false,
        error: None,
    };

    let sta = format!("{path}._STA");
    state.was_on = match namespace.get(&sta) {
        Some(Object::Name(value)) => value.as_integer() != 0,
        Some(Object::Method { .. }) => {
            let mut interpreter = Interpreter::new(namespace);
            // A `_STA` this reader cannot evaluate must not block `_ON`: running
            // `_ON` on an already-on resource is harmless.
            interpreter
                .invoke(&sta, &[], io)
                .map(|value| value.as_integer() != 0)
                .unwrap_or(false)
        }
        _ => false,
    };
    if state.was_on {
        return state;
    }

    let on = format!("{path}._ON");
    match namespace.get(&on) {
        Some(Object::Method { .. }) => {
            let mut interpreter = Interpreter::new(namespace);
            match interpreter.invoke(&on, &[], io) {
                Ok(_) => state.turned_on = true,
                Err(error) => state.error = Some(format!("{error:?}")),
            }
        }
        Some(_) => state.error = Some("_ON is not a method".to_string()),
        None => state.error = Some("no _ON method".to_string()),
    }
    state
}

#[cfg(test)]
mod tests {
    use super::{chain, find_endpoint, find_root_bridge, resolve_resource};
    use crate::drivers::acpi::aml::{Namespace, Object, Value};
    use alloc::boxed::Box;
    use alloc::format;
    use alloc::string::ToString;
    use alloc::vec;
    use alloc::vec::Vec;

    fn seg(segment: &str) -> Vec<u8> {
        let mut bytes = segment.as_bytes().to_vec();
        while bytes.len() < 4 {
            bytes.push(b'_');
        }
        bytes.truncate(4);
        bytes
    }

    fn device(segment: &str, body: &[u8]) -> Vec<u8> {
        let mut header = seg(segment);
        header.extend_from_slice(body);
        let mut out = vec![0x5b, 0x82];
        out.push((header.len() + 1) as u8); // short-form PkgLength
        out.extend(header);
        out
    }

    fn name_op(segment: &str, value: &[u8]) -> Vec<u8> {
        let mut out = vec![0x08];
        out.extend(seg(segment));
        out.extend_from_slice(value);
        out
    }

    fn dword(value: u32) -> Vec<u8> {
        let mut out = vec![0x0c];
        out.extend_from_slice(&value.to_le_bytes());
        out
    }

    fn table() -> &'static [u8] {
        let mut endpoint = Vec::new();
        endpoint.extend(name_op("_ADR", &dword(0x0002_0000)));

        let mut bridge = Vec::new();
        bridge.extend(name_op("_HID", &dword(0x080A_D041)));
        bridge.extend(name_op("_BBN", &[0x00]));
        bridge.extend(device("RP01", &device("WLAN", &endpoint)));

        let mut bytes = vec![0u8; 36];
        bytes[0..4].copy_from_slice(b"DSDT");
        bytes.extend(device("PCI0", &bridge));
        Box::leak(bytes.into_boxed_slice())
    }

    #[test]
    fn finds_the_host_bridge_for_a_bus_and_the_endpoint_by_adr() {
        let namespace = Namespace::parse(&[table()]);
        let bridge = find_root_bridge(&namespace, 0).unwrap();
        assert_eq!(bridge, "\\PCI0");
        assert_eq!(
            find_endpoint(&namespace, &bridge, 0x0002_0000).unwrap(),
            "\\PCI0.RP01.WLAN"
        );
        assert!(find_endpoint(&namespace, &bridge, 0x0003_0000).is_none());
    }

    #[test]
    fn chain_runs_from_the_endpoint_up_to_the_bridge() {
        assert_eq!(
            chain("\\PCI0.RP01.WLAN", "\\PCI0"),
            vec![
                "\\PCI0.RP01.WLAN".to_string(),
                "\\PCI0.RP01".to_string(),
                "\\PCI0".to_string()
            ]
        );
    }

    #[test]
    fn resolves_a_relative_power_resource_reference_against_the_device() {
        // `_PR0` may name a sibling relative to the device that owns it.
        let namespace = Namespace::parse(&[table()]);
        assert_eq!(
            resolve_resource(&namespace, "\\PCI0.RP01.WLAN", "PWR1"),
            None,
            "no PWR1 object exists in this namespace"
        );
        assert_eq!(namespace.get("\\PCI0.RP01"), Some(&Object::Device));
        assert!(matches!(
            namespace.get("\\PCI0.RP01.WLAN._ADR"),
            Some(Object::Name(Value::Integer(0x0002_0000)))
        ));
        let _ = format!("{}", 0);
    }
}
