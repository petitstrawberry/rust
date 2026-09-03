//! Scarlet Native implementations of the file-descriptor ownership traits for sockets.

use crate::net;
use crate::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, IntoRawFd, OwnedFd, RawFd};
use crate::sys::{self, AsInner, FromInner, IntoInner};

macro_rules! impl_raw_fd {
    ($($ty:ident),+ $(,)?) => {$(
        #[stable(feature = "rust1", since = "1.0.0")]
        impl AsRawFd for net::$ty {
            fn as_raw_fd(&self) -> RawFd {
                self.as_inner().as_raw_handle() as RawFd
            }
        }

        #[stable(feature = "from_raw_os", since = "1.1.0")]
        impl FromRawFd for net::$ty {
            unsafe fn from_raw_fd(raw_fd: RawFd) -> Self {
                // SAFETY: the trait contract requires an exclusively owned valid
                // Scarlet handle for the corresponding socket type.
                let inner = unsafe { sys::net::$ty::from_raw_handle(raw_fd as usize) };
                Self::from_inner(inner)
            }
        }

        #[stable(feature = "into_raw_os", since = "1.4.0")]
        impl IntoRawFd for net::$ty {
            fn into_raw_fd(self) -> RawFd {
                self.into_inner().into_raw_handle() as RawFd
            }
        }
    )+ };
}

macro_rules! impl_owned_fd {
    ($($ty:ident),+ $(,)?) => {$(
        #[stable(feature = "io_safety", since = "1.63.0")]
        impl AsFd for net::$ty {
            fn as_fd(&self) -> BorrowedFd<'_> {
                // SAFETY: the returned borrow cannot outlive this owning socket.
                unsafe { BorrowedFd::borrow_raw(self.as_raw_fd()) }
            }
        }

        #[stable(feature = "io_safety", since = "1.63.0")]
        impl From<net::$ty> for OwnedFd {
            fn from(socket: net::$ty) -> Self {
                // SAFETY: `into_raw_fd` transfers the socket's unique ownership.
                unsafe { Self::from_raw_fd(socket.into_raw_fd()) }
            }
        }

        #[stable(feature = "io_safety", since = "1.63.0")]
        impl From<OwnedFd> for net::$ty {
            fn from(owned_fd: OwnedFd) -> Self {
                // SAFETY: `into_raw_fd` transfers the `OwnedFd`'s unique ownership.
                unsafe { Self::from_raw_fd(owned_fd.into_raw_fd()) }
            }
        }
    )+ };
}

impl_raw_fd!(TcpStream, TcpListener, UdpSocket);
impl_owned_fd!(TcpStream, TcpListener, UdpSocket);
