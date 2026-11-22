use core::net::SocketAddrV4;

use downcast_rs::{impl_downcast, DowncastSync};
use gproxy::proxy;
use pconst::{
    io::PollEvents,
    net::{Domain, ShutdownFlag, SocketAddrIn, SocketType},
};
use shared_heap::{DBox, DVec};

use super::AlienResult;
use crate::{Basic, DeviceBase};

pub type SocketID = usize;

#[proxy(NetDomainProxy, RwLock, String)]
pub trait NetDomain: DeviceBase + Basic + DowncastSync {
    fn init(&self, nic_domain_name: &str) -> AlienResult<()>;
    fn socket(&self, s_domain: Domain, ty: SocketType, protocol: usize) -> AlienResult<SocketID>;
    fn socket_pair(&self, s_domain: Domain, ty: SocketType) -> AlienResult<(SocketID, SocketID)>;
    fn remove_socket(&self, socket_id: SocketID) -> AlienResult<()>;
    fn bind(&self, socket_id: SocketID, addr: &DBox<SocketAddrIn>)
        -> AlienResult<Option<SocketID>>;
    fn listen(&self, socket_id: SocketID, backlog: usize) -> AlienResult<()>;
    fn accept(&self, socket_id: SocketID) -> AlienResult<SocketID>;
    fn connect(&self, socket_id: SocketID, addr: &DBox<SocketAddrV4>) -> AlienResult<()>;

    fn recv_from(
        &self,
        socket_id: SocketID,
        arg_tuple: DBox<SocketArgTuple>,
    ) -> AlienResult<DBox<SocketArgTuple>>;
    fn sendto(
        &self,
        socket_id: SocketID,
        buf: &DVec<u8>,
        remote_addr: Option<&DBox<SocketAddrV4>>,
    ) -> AlienResult<usize>;
    fn shutdown(&self, socket_id: SocketID, how: ShutdownFlag) -> AlienResult<()>;

    fn remote_addr(
        &self,
        socket_id: SocketID,
        addr: DBox<SocketAddrIn>,
    ) -> AlienResult<DBox<SocketAddrIn>>;
    fn local_addr(
        &self,
        socket_id: SocketID,
        addr: DBox<SocketAddrIn>,
    ) -> AlienResult<DBox<SocketAddrIn>>;
    fn read_at(
        &self,
        socket_id: SocketID,
        offset: u64,
        buf: DVec<u8>,
    ) -> AlienResult<(DVec<u8>, usize)>;
    fn write_at(&self, socket_id: SocketID, offset: u64, buf: &DVec<u8>) -> AlienResult<usize>;
    fn poll(&self, socket_id: SocketID, events: PollEvents) -> AlienResult<PollEvents>;
}

pub struct SocketArgTuple {
    pub buf: DVec<u8>,
    pub addr: DBox<SocketAddrIn>,
    pub len: usize,
}

impl_downcast!(sync NetDomain);
