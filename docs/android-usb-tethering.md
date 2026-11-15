# Android USB tethering

OCTAGRAM supports the USB RNDIS network function exposed by Android phones
when **USB tethering** is enabled. The current implementation requires an
xHCI USB host controller and the Android RNDIS control-interface class tuple
`E0/01/03`, followed by its CDC data interface. Only one RNDIS interface is
currently active at a time.

Connect the phone, enable USB tethering in Android settings, then use:

```text
drivers
lsdev
net status
net dhcp
net status
ping <phone-gateway-ip>
```

`net dhcp` starts a DHCP exchange; the USB receive path processes replies
asynchronously. `net status` reports the assigned address, netmask, gateway,
and DNS server. `ping` sends an ICMP echo request after resolving the next-hop
MAC address. A UDP datagram can be sent with
`net udp <IPv4-address> <port> <text>`.
If UDP reports that ARP resolution is pending, retry after the neighbor
responds.

The networking layer implements Ethernet, ARP, IPv4, DHCP, ICMP echo,
user-process UDP datagram sockets through `octagram.h`, and an initial IPv4 TCP
transport with POSIX `SOCK_STREAM` calls in `<sys/socket.h>`. Blocking socket
calls yield while waiting for network progress. TCP currently supports basic
connect/listen/accept and stream transfer; it does not yet provide a complete
POSIX socket feature set. `oct_dns_server()` returns the DNS address received
through DHCP; the Rust runtime also provides `dns_lookup()` for IPv4 A records.
TCP/RNDIS interoperability has not been validated on a physical Android phone,
so TCP-based applications should not yet be considered ready for tethered use.

The USB/RNDIS path still needs validation on a physical Android phone; QEMU does
not emulate Android RNDIS by default.
