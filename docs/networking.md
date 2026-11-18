# Jaringan

Implementasi jaringan berada di `kernel/src/network.rs` (link layer, ARP, IPv4,
UDP, DHCP, ICMP), `kernel/src/network/tcp.rs` (TCP), `kernel/src/network/dns.rs`
(resolver), dan `kernel/src/network/http.rs` (klien HTTP sederhana). Program user
mengaksesnya lewat syscall socket di [syscall-abi.md](syscall-abi.md).

## Link layer

- Driver mendaftarkan `LinkDevice` dengan `register_link(device)`; hanya satu
  link aktif pada satu waktu (percobaan kedua ditolak).
- `LinkDevice` menyediakan MAC, `transmit(frame)`, dan callback penerimaan frame
  Ethernet untuk driver (RNDIS/NCM USB, dan driver jaringan lain yang
  menambahkan trait yang sama).
- Setelah link terdaftar, state jaringan menyimpan konfigurasi IPv4, satu entri
  neighbor ARP, dan status DHCP.

## IPv4, ARP, dan ICMP

- Frame Ethernet dibangun oleh `build_ipv4_frame()` dengan MTU 1500.
- ARP: bila MAC tujuan belum diketahui, kernel mengirim ARP request dan
  mengembalikan error "ARP request sent; retry after the neighbor responds".
  Cache neighbor hanya satu entri (`neighbor_ip`/`neighbor_mac`).
- Routing sederhana: tujuan di subnet yang sama dikirim langsung, selain itu
  lewat gateway konfigurasi.
- `ping` memakai ICMP echo lewat jalur yang sama.

## DHCP

`net dhcp` menjalankan klien DHCP (`start_dhcp()`): mengirim DHCPDISCOVER dari
port 68 ke port 67, memproses DHCPOFFER/ACK, lalu menyimpan alamat, netmask,
gateway, dan DNS. Status terlihat dengan `net` / `net status`.

Saat lease diterima, `persist_lease()` menulis berkas teks
`/var/lib/octagram/net/lease` (kunci `address`, `netmask`, `gateway`, `dns`),
setelah membuat direktorinya bila perlu. Setelah driver jaringan siap, boot
memanggil `network::restore_saved_lease()`; berkas yang tidak lengkap atau
mengandung oktet tidak valid diabaikan dengan pesan. `register_link()` memakai
ulang lease tersimpan sehingga konfigurasi bertahan tanpa DHCP ulang. Tabel
routing statis belum ikut disimpan karena konfigurasi tetap hanya satu gateway.

## Socket UDP

- `socket(AF_INET, SOCK_DGRAM, 0)`, `bind`, `sendto`, `recvfrom`, `connect`.
- Port ephemeral mulai 49152; `bind(0)` memilih port bebas.
- Setiap socket punya antrean maksimum 8 datagram (`MAX_UDP_QUEUE`); datagram
  berlebih dibuang.
- `recvfrom`/`recvmsg` melaporkan panjang asli datagram dan mengisi `MSG_TRUNC`
  bila buffer lebih kecil.
- Socket UDP yang sudah `connect` hanya menerima datagram dari peer dan bisa
  memakai `send`/`write` tanpa alamat.

## Socket TCP

- API: `socket`, `bind`, `listen` (backlog 8), `accept`, `connect`, `send`,
  `recv`, `shutdown`, `close`.
- MSS awal 1460, buffer terima 8192 byte. `send` dibatasi `peer_mss`,
  `peer_window`, dan congestion window; `EAGAIN` dikembalikan saat kapasitas
  habis atau antrean pipeline penuh.
- Pipeline sampai 4 segmen di luar segmen tertua (`pending` + `pipeline`). ACK
  kumulatif memajukan antrean sekaligus: selama ACK mencakup ujung segmen
  tertua, segmen itu dilepas dan segmen berikutnya menjadi tertua.
- Congestion window byte sederhana (slow start): mulai 1 MSS, naik 1 MSS per
  ACK sampai 4 MSS, dan kembali ke 1 MSS saat RTO. Byte in-flight dihitung dari
  `send_next - send_unacknowledged`.
- Retransmisi bersifat go-back-N: hanya segmen tertua yang dikirim ulang, dengan
  RTO eksponensial 30 → 60 → 120 → 240 → 480 tick, maksimum 5 percobaan; setelah
  itu socket di-RST dengan `TCP connection timed out`.
- Saat jendela peer nol, kernel mengirim probe setiap 60 tick.
- Keepalive opt-in lewat `setsockopt(SO_KEEPALIVE)`: probe ACK setelah 60.000
  tick idle, maksimum 9 probe.
- State TCP mengikuti `Created`, `Bound`, `Listening`, `SynSent`, `SynReceived`,
  `Established`, `FinWait1/2`, `CloseWait`, `LastAck`, `Closed`, `Reset`.

## DNS dan getaddrinfo

- Server DNS dibaca dari baris `nameserver <IPv4>` pertama di `/etc/resolv.conf`;
  bila berkas tidak ada dipakai DNS dari DHCP.
- Query UDP ke port 53 dengan timeout 2 detik lewat `poll`.
- Cache A, PTR, dan AAAA masing-masing 8 entri dengan TTL record dibatasi
  60–86400 detik.
- `getaddrinfo` mendukung `AF_INET`, `AF_INET6`, dan `AF_UNSPEC` (entri IPv4 lalu
  IPv6 lewat `ai_next`), flag `AI_PASSIVE`, `AI_CANONNAME`, `AI_NUMERICHOST`,
  `AI_NUMERICSERV`; `getnameinfo` memakai PTR bila memungkinkan.
- `inet_pton`/`inet_ntop` mendukung `AF_INET6`, termasuk kompresi `::`.

Contoh program: `dns_demo` dan `netdb_demo` di `user/examples/`.

## HTTP

`network/http.rs` menyediakan `parse_url`, `tcp_exchange`, dan `http_get` untuk
permintaan HTTP/1.0 sederhana dengan `Host` header. Shell memakainya lewat
`http`/`curl`. Tidak ada TLS, redirect otomatis, chunked decoding, atau proxy.

## Batasan

- Tanpa stack IPv6 (hanya primitif alamat dan resolve AAAA).
- Tanpa ARP cache lebih dari satu entri, tanpa routing table, tanpa netmask
  panjang variabel selain dari DHCP.
- Congestion window sederhana (slow start 1→4 MSS, reset saat RTO), tanpa fast
  retransmit, congestion avoidance penuh, SACK, atau window scaling.
- Tanpa network namespace atau policy layer; semua proses berbagi konfigurasi.
- Lease dan rute tidak persisten antar reboot.

## Uji

Uji host kernel mencakup pembentukan frame, checksum, parser DHCP, transmisi dan
penerimaan UDP lewat link palsu, penyaringan peer UDP, `retransmit_backoff`,
`zero_window_probe_due`, dan `keepalive_due`. Perilaku jaringan nyata hanya bisa
diuji dengan QEMU dan perangkat jaringan.
