use super::EndpointInfo;

pub fn parse_configuration_endpoints(
    descriptor: &[u8],
) -> Result<alloc::vec::Vec<EndpointInfo>, &'static str> {
    if descriptor.len() < 9 || descriptor[0] < 9 || descriptor[1] != 2 {
        return Err("invalid USB configuration descriptor");
    }
    let total_length = u16::from_le_bytes([descriptor[2], descriptor[3]]) as usize;
    if total_length < 9 || total_length > descriptor.len() {
        return Err("USB configuration descriptor length is invalid");
    }

    let mut endpoints = alloc::vec::Vec::new();
    let mut interface: Option<(u8, u8, u8, u8, bool)> = None;
    let mut offset = 0usize;
    while offset < total_length {
        if offset + 2 > total_length {
            return Err("truncated USB descriptor header");
        }
        let length = usize::from(descriptor[offset]);
        let kind = descriptor[offset + 1];
        if length < 2 || offset + length > total_length {
            return Err("invalid USB descriptor length");
        }
        match kind {
            4 => {
                if length < 9 {
                    return Err("invalid USB interface descriptor");
                }
                interface = Some((
                    descriptor[offset + 2],
                    descriptor[offset + 5],
                    descriptor[offset + 6],
                    descriptor[offset + 7],
                    descriptor[offset + 3] == 0,
                ));
            }
            5 => {
                if length < 7 {
                    return Err("invalid USB endpoint descriptor");
                }
                if let Some((number, class, subclass, protocol, active_alt)) = interface
                    && active_alt
                {
                    endpoints.push(EndpointInfo {
                        address: descriptor[offset + 2],
                        attributes: descriptor[offset + 3],
                        max_packet_size: u16::from_le_bytes([
                            descriptor[offset + 4],
                            descriptor[offset + 5],
                        ]),
                        interval: descriptor[offset + 6],
                        inteface_number: number,
                        interface_subclass: subclass,
                        interface_class: class,
                        interface_protocol: protocol,
                    });
                }
            }
            _ => {}
        }
        offset += length;
    }
    Ok(endpoints)
}

#[cfg(test)]
mod tests {
    use super::parse_configuration_endpoints;

    #[test]
    fn parses_mass_storage_interface_and_bulk_endpoints() {
        let configuration = [
            9, 2, 32, 0, 1, 1, 0, 0x80, 50, // configuration
            9, 4, 0, 0, 2, 8, 6, 0x50, 0, // interface
            7, 5, 0x02, 2, 0, 2, 0, // bulk OUT, 512 bytes
            7, 5, 0x81, 2, 0, 2, 0, // bulk IN, 512 bytes
        ];
        let endpoints = parse_configuration_endpoints(&configuration).unwrap();
        assert_eq!(endpoints.len(), 2);
        assert_eq!(endpoints[0].inteface_number, 0);
        assert_eq!(endpoints[0].interface_class, 8);
        assert_eq!(endpoints[0].interface_subclass, 6);
        assert_eq!(endpoints[0].interface_protocol, 0x50);
        assert_eq!(endpoints[0].max_packet_size, 512);
        assert_eq!(endpoints[1].address, 0x81);
    }

    #[test]
    fn rejects_truncated_configuration_descriptors() {
        assert!(parse_configuration_endpoints(&[9, 2, 10, 0, 1, 1, 0, 0x80, 50]).is_err());
    }
}
