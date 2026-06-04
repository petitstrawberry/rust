use crate::io::{self, BorrowedCursor, IoSlice, IoSliceMut};
use crate::net::{Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, SocketAddrV4, ToSocketAddrs};
use crate::str::FromStr;
use crate::sync::Mutex;
use crate::sys::pal::abi;
use crate::sys::unsupported;
use crate::time::Duration;
use crate::vec::IntoIter;
use crate::{fmt, vec};

pub struct TcpStream {
    handle: usize,
    peer: Option<SocketAddr>,
}

impl TcpStream {
    pub fn connect<A: ToSocketAddrs>(addr: A) -> io::Result<TcpStream> {
        super::each_addr(addr, |addr| {
            let raw = socket_addr_to_raw_v4(addr)?;
            let handle = abi::socket_create(
                abi::SOCKET_DOMAIN_INET4,
                abi::SOCKET_TYPE_STREAM,
                abi::SOCKET_PROTOCOL_TCP,
            )
            .map_err(|()| io::ErrorKind::Other)?;
            match abi::socket_connect_inet(handle, &raw) {
                Ok(()) => Ok(TcpStream { handle, peer: Some(*addr) }),
                Err(()) => {
                    let _ = abi::handle_close(handle);
                    Err(io::ErrorKind::ConnectionRefused.into())
                }
            }
        })
    }

    pub fn connect_timeout(_: &SocketAddr, _: Duration) -> io::Result<TcpStream> {
        // TODO(scarlet): add nonblocking connect or a timed connect syscall.
        unsupported()
    }

    pub fn set_read_timeout(&self, _: Option<Duration>) -> io::Result<()> {
        // TODO(scarlet): add socket read timeout operations.
        unsupported()
    }

    pub fn set_write_timeout(&self, _: Option<Duration>) -> io::Result<()> {
        // TODO(scarlet): add socket write timeout operations.
        unsupported()
    }

    pub fn read_timeout(&self) -> io::Result<Option<Duration>> {
        // TODO(scarlet): add socket read timeout operations.
        unsupported()
    }

    pub fn write_timeout(&self) -> io::Result<Option<Duration>> {
        // TODO(scarlet): add socket write timeout operations.
        unsupported()
    }

    pub fn peek(&self, _: &mut [u8]) -> io::Result<usize> {
        // TODO(scarlet): expose a socket peek flag or syscall.
        unsupported()
    }

    pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        abi::stream_read(self.handle, buf).map_err(|()| io::ErrorKind::Other.into())
    }

    pub fn read_buf(&self, cursor: BorrowedCursor<'_>) -> io::Result<()> {
        crate::io::default_read_buf(|buf| self.read(buf), cursor)
    }

    pub fn read_vectored(&self, bufs: &mut [IoSliceMut<'_>]) -> io::Result<usize> {
        crate::io::default_read_vectored(|buf| self.read(buf), bufs)
    }

    pub fn is_read_vectored(&self) -> bool {
        false
    }

    pub fn write(&self, buf: &[u8]) -> io::Result<usize> {
        abi::stream_write(self.handle, buf).map_err(|()| io::ErrorKind::Other.into())
    }

    pub fn write_vectored(&self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        crate::io::default_write_vectored(|buf| self.write(buf), bufs)
    }

    pub fn is_write_vectored(&self) -> bool {
        false
    }

    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.peer.ok_or_else(|| io::Error::from(io::ErrorKind::Unsupported))
    }

    pub fn socket_addr(&self) -> io::Result<SocketAddr> {
        // TODO(scarlet): expose getsockname for Native sockets.
        unsupported()
    }

    pub fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        abi::socket_shutdown(self.handle, shutdown_to_raw(how))
            .map_err(|()| io::ErrorKind::Other.into())
    }

    pub fn duplicate(&self) -> io::Result<TcpStream> {
        abi::handle_duplicate(self.handle)
            .map(|handle| TcpStream { handle, peer: self.peer })
            .map_err(|()| io::ErrorKind::Other.into())
    }

    pub fn set_linger(&self, _: Option<Duration>) -> io::Result<()> {
        // TODO(scarlet): add linger support to Native sockets.
        unsupported()
    }

    pub fn linger(&self) -> io::Result<Option<Duration>> {
        // TODO(scarlet): add linger support to Native sockets.
        unsupported()
    }

    pub fn set_nodelay(&self, _: bool) -> io::Result<()> {
        // TODO(scarlet): add TCP_NODELAY support to Native TCP sockets.
        unsupported()
    }

    pub fn nodelay(&self) -> io::Result<bool> {
        // TODO(scarlet): add TCP_NODELAY support to Native TCP sockets.
        unsupported()
    }

    pub fn set_ttl(&self, _: u32) -> io::Result<()> {
        // TODO(scarlet): add TTL socket option support.
        unsupported()
    }

    pub fn ttl(&self) -> io::Result<u32> {
        // TODO(scarlet): add TTL socket option support.
        unsupported()
    }

    pub fn take_error(&self) -> io::Result<Option<io::Error>> {
        // TODO(scarlet): expose pending socket error state.
        Ok(None)
    }

    pub fn set_nonblocking(&self, _: bool) -> io::Result<()> {
        // TODO(scarlet): wire this to Native socket command support.
        unsupported()
    }
}

impl Drop for TcpStream {
    fn drop(&mut self) {
        let _ = abi::handle_close(self.handle);
    }
}

impl fmt::Debug for TcpStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TcpStream").field("handle", &self.handle).finish_non_exhaustive()
    }
}

pub struct TcpListener {
    handle: usize,
    local: SocketAddr,
}

impl TcpListener {
    pub fn bind<A: ToSocketAddrs>(addr: A) -> io::Result<TcpListener> {
        super::each_addr(addr, |addr| {
            let raw = socket_addr_to_raw_v4(addr)?;
            let handle = abi::socket_create(
                abi::SOCKET_DOMAIN_INET4,
                abi::SOCKET_TYPE_STREAM,
                abi::SOCKET_PROTOCOL_TCP,
            )
            .map_err(|()| io::ErrorKind::Other)?;
            let result =
                abi::socket_bind_inet(handle, &raw).and_then(|()| abi::socket_listen(handle, 128));
            match result {
                Ok(()) => Ok(TcpListener { handle, local: *addr }),
                Err(()) => {
                    let _ = abi::handle_close(handle);
                    Err(io::ErrorKind::AddrInUse.into())
                }
            }
        })
    }

    pub fn socket_addr(&self) -> io::Result<SocketAddr> {
        Ok(self.local)
    }

    pub fn accept(&self) -> io::Result<(TcpStream, SocketAddr)> {
        let handle = abi::socket_accept(self.handle).map_err(|()| io::ErrorKind::Other)?;
        let peer = SocketAddr::from(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0));
        // TODO(scarlet): make SocketAccept return the peer address.
        Ok((TcpStream { handle, peer: None }, peer))
    }

    pub fn duplicate(&self) -> io::Result<TcpListener> {
        abi::handle_duplicate(self.handle)
            .map(|handle| TcpListener { handle, local: self.local })
            .map_err(|()| io::ErrorKind::Other.into())
    }

    pub fn set_ttl(&self, _: u32) -> io::Result<()> {
        // TODO(scarlet): add TTL socket option support.
        unsupported()
    }

    pub fn ttl(&self) -> io::Result<u32> {
        // TODO(scarlet): add TTL socket option support.
        unsupported()
    }

    pub fn set_only_v6(&self, _: bool) -> io::Result<()> {
        // IPv6 listeners are unsupported in this backend for now.
        unsupported()
    }

    pub fn only_v6(&self) -> io::Result<bool> {
        // IPv6 listeners are unsupported in this backend for now.
        Ok(false)
    }

    pub fn take_error(&self) -> io::Result<Option<io::Error>> {
        // TODO(scarlet): expose pending socket error state.
        Ok(None)
    }

    pub fn set_nonblocking(&self, _: bool) -> io::Result<()> {
        // TODO(scarlet): wire this to Native socket command support.
        unsupported()
    }
}

impl Drop for TcpListener {
    fn drop(&mut self) {
        let _ = abi::handle_close(self.handle);
    }
}

impl fmt::Debug for TcpListener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TcpListener").field("handle", &self.handle).finish_non_exhaustive()
    }
}

pub struct UdpSocket {
    handle: usize,
    local: SocketAddr,
    peer: Mutex<Option<SocketAddr>>,
}

impl UdpSocket {
    pub fn bind<A: ToSocketAddrs>(addr: A) -> io::Result<UdpSocket> {
        super::each_addr(addr, |addr| {
            let raw = socket_addr_to_raw_v4(addr)?;
            let handle = abi::socket_create(
                abi::SOCKET_DOMAIN_INET4,
                abi::SOCKET_TYPE_DATAGRAM,
                abi::SOCKET_PROTOCOL_UDP,
            )
            .map_err(|()| io::ErrorKind::Other)?;
            match abi::socket_bind_inet(handle, &raw) {
                Ok(()) => Ok(UdpSocket { handle, local: *addr, peer: Mutex::new(None) }),
                Err(()) => {
                    let _ = abi::handle_close(handle);
                    Err(io::ErrorKind::AddrInUse.into())
                }
            }
        })
    }

    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.peer.lock().unwrap().ok_or_else(|| io::Error::from(io::ErrorKind::NotConnected))
    }

    pub fn socket_addr(&self) -> io::Result<SocketAddr> {
        Ok(self.local)
    }

    pub fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        let mut raw_addr = [0; 8];
        let len = abi::socket_recvfrom(self.handle, buf, &mut raw_addr)
            .map_err(|()| io::ErrorKind::Other)?;
        Ok((len, raw_v4_sockaddr_to_socket_addr(&raw_addr)?))
    }

    pub fn peek_from(&self, _: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        // TODO(scarlet): expose a socket peek flag or syscall.
        unsupported()
    }

    pub fn send_to(&self, buf: &[u8], addr: &SocketAddr) -> io::Result<usize> {
        let raw_addr = socket_addr_to_raw_v4_sockaddr(addr)?;
        abi::socket_sendto(self.handle, buf, &raw_addr).map_err(|()| io::ErrorKind::Other.into())
    }

    pub fn duplicate(&self) -> io::Result<UdpSocket> {
        let peer = *self.peer.lock().unwrap();
        abi::handle_duplicate(self.handle)
            .map(|handle| UdpSocket { handle, local: self.local, peer: Mutex::new(peer) })
            .map_err(|()| io::ErrorKind::Other.into())
    }

    pub fn set_read_timeout(&self, _: Option<Duration>) -> io::Result<()> {
        // TODO(scarlet): add socket read timeout operations.
        unsupported()
    }

    pub fn set_write_timeout(&self, _: Option<Duration>) -> io::Result<()> {
        // TODO(scarlet): add socket write timeout operations.
        unsupported()
    }

    pub fn read_timeout(&self) -> io::Result<Option<Duration>> {
        // TODO(scarlet): add socket read timeout operations.
        unsupported()
    }

    pub fn write_timeout(&self) -> io::Result<Option<Duration>> {
        // TODO(scarlet): add socket write timeout operations.
        unsupported()
    }

    pub fn set_broadcast(&self, _: bool) -> io::Result<()> {
        // TODO(scarlet): add broadcast socket option support.
        unsupported()
    }

    pub fn broadcast(&self) -> io::Result<bool> {
        // TODO(scarlet): add broadcast socket option support.
        unsupported()
    }

    pub fn set_multicast_loop_v4(&self, _: bool) -> io::Result<()> {
        // TODO(scarlet): add multicast socket option support.
        unsupported()
    }

    pub fn multicast_loop_v4(&self) -> io::Result<bool> {
        // TODO(scarlet): add multicast socket option support.
        unsupported()
    }

    pub fn set_multicast_ttl_v4(&self, _: u32) -> io::Result<()> {
        // TODO(scarlet): add multicast socket option support.
        unsupported()
    }

    pub fn multicast_ttl_v4(&self) -> io::Result<u32> {
        // TODO(scarlet): add multicast socket option support.
        unsupported()
    }

    pub fn set_multicast_loop_v6(&self, _: bool) -> io::Result<()> {
        // IPv6 is unsupported in this backend for now.
        unsupported()
    }

    pub fn multicast_loop_v6(&self) -> io::Result<bool> {
        // IPv6 is unsupported in this backend for now.
        unsupported()
    }

    pub fn join_multicast_v4(&self, _: &Ipv4Addr, _: &Ipv4Addr) -> io::Result<()> {
        // TODO(scarlet): add multicast group operations.
        unsupported()
    }

    pub fn join_multicast_v6(&self, _: &Ipv6Addr, _: u32) -> io::Result<()> {
        // IPv6 is unsupported in this backend for now.
        unsupported()
    }

    pub fn leave_multicast_v4(&self, _: &Ipv4Addr, _: &Ipv4Addr) -> io::Result<()> {
        // TODO(scarlet): add multicast group operations.
        unsupported()
    }

    pub fn leave_multicast_v6(&self, _: &Ipv6Addr, _: u32) -> io::Result<()> {
        // IPv6 is unsupported in this backend for now.
        unsupported()
    }

    pub fn set_ttl(&self, _: u32) -> io::Result<()> {
        // TODO(scarlet): add TTL socket option support.
        unsupported()
    }

    pub fn ttl(&self) -> io::Result<u32> {
        // TODO(scarlet): add TTL socket option support.
        unsupported()
    }

    pub fn take_error(&self) -> io::Result<Option<io::Error>> {
        // TODO(scarlet): expose pending socket error state.
        Ok(None)
    }

    pub fn set_nonblocking(&self, _: bool) -> io::Result<()> {
        // TODO(scarlet): wire this to Native socket command support.
        unsupported()
    }

    pub fn recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        abi::stream_read(self.handle, buf).map_err(|()| io::ErrorKind::Other.into())
    }

    pub fn peek(&self, _: &mut [u8]) -> io::Result<usize> {
        // TODO(scarlet): expose a socket peek flag or syscall.
        unsupported()
    }

    pub fn send(&self, buf: &[u8]) -> io::Result<usize> {
        abi::stream_write(self.handle, buf).map_err(|()| io::ErrorKind::Other.into())
    }

    pub fn connect<A: ToSocketAddrs>(&self, addr: A) -> io::Result<()> {
        super::each_addr(addr, |addr| {
            let raw = socket_addr_to_raw_v4(addr)?;
            abi::socket_connect_inet(self.handle, &raw)
                .map_err(|()| io::Error::from(io::ErrorKind::ConnectionRefused))?;
            *self.peer.lock().unwrap() = Some(*addr);
            Ok(())
        })
    }
}

impl Drop for UdpSocket {
    fn drop(&mut self) {
        let _ = abi::handle_close(self.handle);
    }
}

impl fmt::Debug for UdpSocket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UdpSocket").field("handle", &self.handle).finish_non_exhaustive()
    }
}

pub struct LookupHost {
    addrs: IntoIter<SocketAddr>,
}

impl Iterator for LookupHost {
    type Item = SocketAddr;

    fn next(&mut self) -> Option<SocketAddr> {
        self.addrs.next()
    }
}

pub fn lookup_host(host: &str, port: u16) -> io::Result<LookupHost> {
    if let Ok(addr) = Ipv4Addr::from_str(host) {
        return Ok(LookupHost {
            addrs: vec![SocketAddr::from(SocketAddrV4::new(addr, port))].into_iter(),
        });
    }
    if Ipv6Addr::from_str(host).is_ok() {
        return Err(io::ErrorKind::Unsupported.into());
    }
    // TODO(scarlet): add DNS resolver integration once the kernel/user resolver
    // contract is settled.
    unsupported()
}

fn socket_addr_to_raw_v4(addr: &SocketAddr) -> io::Result<abi::Inet4SocketAddress> {
    match addr {
        SocketAddr::V4(addr) => {
            Ok(abi::Inet4SocketAddress { addr: addr.ip().octets(), port: addr.port() })
        }
        SocketAddr::V6(_) => Err(io::ErrorKind::Unsupported.into()),
    }
}

fn socket_addr_to_raw_v4_sockaddr(addr: &SocketAddr) -> io::Result<[u8; 8]> {
    let raw = socket_addr_to_raw_v4(addr)?;
    let mut sockaddr = [0; 8];
    sockaddr[0] = 2;
    sockaddr[2..6].copy_from_slice(&raw.addr);
    sockaddr[6..8].copy_from_slice(&raw.port.to_be_bytes());
    Ok(sockaddr)
}

fn raw_v4_sockaddr_to_socket_addr(raw: &[u8; 8]) -> io::Result<SocketAddr> {
    if raw[0] != 2 {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let port = u16::from_be_bytes([raw[6], raw[7]]);
    Ok(SocketAddr::from(SocketAddrV4::new(Ipv4Addr::new(raw[2], raw[3], raw[4], raw[5]), port)))
}

fn shutdown_to_raw(how: Shutdown) -> usize {
    match how {
        Shutdown::Read => abi::SOCKET_SHUTDOWN_READ,
        Shutdown::Write => abi::SOCKET_SHUTDOWN_WRITE,
        Shutdown::Both => abi::SOCKET_SHUTDOWN_BOTH,
    }
}
