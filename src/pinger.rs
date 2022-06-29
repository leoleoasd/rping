use std::{
    io::{self, IoSliceMut},
    mem::MaybeUninit,
    net::Ipv4Addr,
    ops::{Index, Not},
    os::unix::prelude::AsRawFd,
    process::exit,
};

use async_io::Async;

use log::{debug, error, info, trace, warn};
use nix::{
    ifaddrs::getifaddrs,
    libc::{sock_extended_err, SO_EE_ORIGIN_ICMP},
    sys::socket::{
        recvmsg, setsockopt, sockopt::DontRoute, sockopt::Ipv4RecvErr, MsgFlags, SockaddrIn,
        SockaddrStorage,
    },
};
use pnet_packet::{
    icmp::{
        echo_reply::EchoReplyPacket,
        echo_request::{EchoRequestPacket, MutableEchoRequestPacket},
        IcmpPacket, IcmpTypes, MutableIcmpPacket,
    },
    Packet, PacketSize,
};
use quick_error::quick_error;
use socket2::{Domain, Protocol, SockAddr, Socket, Type};
use tokio::sync::{mpsc::Sender, Notify};
use tokio::{
    select,
    sync::{Mutex, RwLock},
    task::JoinHandle,
    time::{interval, sleep, Duration, Instant},
};

// HostUnreachable(Ipv4Addr, u16)
// ProtocolUnreachable(Ipv4Addr, u16)
// PortUnreachable(Ipv4Addr, u16)
// OtherUnreachable(Ipv4Addr, u16, u8)
// TimeExceeded(Ipv4Addr, u16)
// Unknown(Ipv4Addr, u16, u8, u8)
quick_error! {
    #[derive(Debug)]
    enum IcmpError {
        NetworkUnreachable(ip: Ipv4Addr, seq: u16) {
            display("Network unreachable from {}, seq: {}", ip, seq)
        }
        HostUnreachable(ip: Ipv4Addr, seq: u16) {
            display("Host unreachable from {}, seq: {}", ip, seq)
        }
        ProtocolUnreachable(ip: Ipv4Addr, seq: u16) {
            display("Protocol unreachable from {}, seq: {}", ip, seq)
        }
        PortUnreachable(ip: Ipv4Addr, seq: u16) {
            display("Port unreachable from {}, seq: {}", ip, seq)
        }
        OtherUnreachable(ip: Ipv4Addr, seq: u16, code: u8) {
            display("Other unreachable from {}, seq: {}, ee_code: {}", ip, seq, code)
        }
        TimeExceeded(ip: Ipv4Addr, seq: u16) {
            display("Time exceeded from {}, seq: {}", ip, seq)
        }
        Unknown(ip: Ipv4Addr, seq: u16, ee_code: u8, ee_type: u8) {
            display("Unknown from {}, seq: {}, ee_code: {}, ee_type: {}", ip, seq, ee_code, ee_type)
        }
        UnknownOrigin(ip: Ipv4Addr, seq: u16, ee_origin: u8,  ee_code: u8, ee_type: u8) {
            display("Unknown origin from {}, seq: {}, ee_code: {}, ee_type: {}", ip, seq, ee_code, ee_type)
        }
        Io(err: io::Error) {
            display("IO error: {}", err)
            source(err)
            from()
        }
    }
}

impl From<(sock_extended_err, Ipv4Addr, u16)> for IcmpError {
    fn from((err, addr, seq): (sock_extended_err, Ipv4Addr, u16)) -> Self {
        match err.ee_origin {
            SO_EE_ORIGIN_ICMP => match err.ee_type {
                3 => match err.ee_code {
                    0 => IcmpError::NetworkUnreachable(addr, seq),
                    1 => IcmpError::HostUnreachable(addr, seq),
                    2 => IcmpError::ProtocolUnreachable(addr, seq),
                    3 => IcmpError::PortUnreachable(addr, seq),
                    _ => IcmpError::OtherUnreachable(addr, seq, err.ee_code),
                },
                11 => IcmpError::TimeExceeded(addr, seq),
                _ => IcmpError::Unknown(addr, seq, err.ee_type, err.ee_code),
            },
            _ => unreachable!(),
        }
    }
}

#[derive(Debug)]
pub struct Pinger {
    socket: Async<Socket>,
    starts: RwLock<Vec<Instant>>,
    host: SockAddr,
    count: u16,
    size: u16,
    timeout: Duration,
    interval: Duration,
    timeout_handles: Mutex<Vec<JoinHandle<()>>>,
    pub latencies: Mutex<Vec<Option<Duration>>>,
    finished: Notify,
    tx: Sender<Option<Duration>>,
    latencies_sent: Mutex<u16>,
    graph: bool,
}

impl Pinger {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        host: SockAddr,
        count: u16,
        broadcast: bool,
        size: u16,
        ttl: u8,
        timeout: Duration,
        interval: Duration,
        route: bool,
        tx: Sender<Option<Duration>>,
        graph: bool,
    ) -> io::Result<Self> {
        let addrs = getifaddrs()?;
        for ifaddr in addrs {
            match ifaddr.broadcast {
                Some(addr) => {
                    if addr.as_sockaddr_in().is_some() {
                        let braddr = addr.as_sockaddr_in().unwrap();
                        let host: SockaddrIn = host.as_socket_ipv4().unwrap().into();
                        if *braddr == host && !broadcast {
                            error!("You should specify broadcast option");
                            exit(-1);
                        }
                    }
                }
                None => {}
            }
        }
        let sock = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::ICMPV4)).unwrap();
        sock.set_broadcast(broadcast)?;
        sock.set_ttl(ttl as u32)?;
        setsockopt(sock.as_raw_fd(), Ipv4RecvErr, &true)?;
        if route {
            setsockopt(sock.as_raw_fd(), DontRoute, &true)?;
        }
        Ok(Pinger {
            socket: Async::new(sock)?,
            host,
            count,
            size,
            timeout,
            interval,
            starts: Default::default(),
            timeout_handles: Default::default(),
            latencies: Default::default(),
            finished: Default::default(),
            tx,
            latencies_sent: Default::default(),
            graph,
        })
    }
    pub async fn start(&'static self) {
        let b = tokio::spawn(self.listen());
        let a = tokio::spawn(self.ping(b));
        a.await.unwrap();
        self.finished.notified().await;
    }
    async fn ping(&'static self, listen_handle: JoinHandle<()>) {
        let listen_handle = Box::leak(Box::new(listen_handle));
        if self.starts.read().await.len() != 0 {
            panic!("Already started pinging!");
        }
        let mut data: Vec<u8> = vec![0; self.size as usize];
        let mut timer = interval(self.interval);
        for i in 0..self.count {
            timer.tick().await;
            let mut echo_packet = MutableEchoRequestPacket::new(&mut data[..]).unwrap();
            echo_packet.set_sequence_number(i);
            echo_packet.set_icmp_type(IcmpTypes::EchoRequest);

            let now = Instant::now();
            self.starts.write().await.push(now);
            self.latencies.lock().await.push(None);

            self.timeout_handles
                .lock()
                .await
                .push(tokio::spawn(self.timeout(i, listen_handle)));
            match self
                .socket
                .write_with(|socket| socket.send_to(&data, &self.host))
                .await
            {
                Ok(_) => {}
                Err(e) => {
                    error!("Failed to send packet: {}", e);
                    self.timeout_handles.lock().await.index(i as usize).abort();
                    continue;
                }
            }
            debug!(
                "Sent package {i} to {}",
                self.host.as_socket_ipv4().unwrap().ip()
            );
        }
    }
    pub async fn traceroute(&'static self) -> io::Result<Vec<Option<(Ipv4Addr, Duration)>>> {
        let mut result = vec![];
        for ttl in 1..128 {
            let mut data: Vec<u8> = vec![0; self.size as usize];
            let mut echo_packet = MutableIcmpPacket::new(&mut data[..]).unwrap();
            echo_packet.set_icmp_type(IcmpTypes::EchoRequest);
            self.socket.as_ref().set_ttl(ttl)?;

            let now = Instant::now();

            match self
                .socket
                .write_with(|socket| socket.send_to(&data, &self.host))
                .await
            {
                Ok(_) => {}
                Err(e) => {
                    error!("Failed to send packet: {}", e);
                    panic!("{:?}", e);
                }
            };
            select! {
                package = self.recv() => {
                    trace!("{:?}", package);
                    match package {
                        Ok(icmp) => {
                            result.push(Some((
                                icmp.1,
                                now.elapsed()
                            )));
                            info!("Hop {ttl:>2 }: {:<15 } {:?}", icmp.1, now.elapsed());
                            break;
                        },
                        Err(IcmpError::TimeExceeded(addr, _)) => {
                            result.push(Some((
                                addr,
                                now.elapsed()
                            )));
                            info!("Hop {ttl:>2 }: {addr:<15 } {:?}", now.elapsed());
                        },
                        Err(err) => {
                            error!("{}", err);
                        }
                    }
                }
                _ = sleep(self.timeout) => {
                    info!("Hop {ttl:>2 }: *");
                    result.push(None);
                }
            };

            debug!(
                "Sent package {ttl} to {}",
                self.host.as_socket_ipv4().unwrap().ip()
            );
        }
        Ok(result)
    }
    async fn recv(&'static self) -> Result<(IcmpPacket<'static>, Ipv4Addr), IcmpError> {
        let mut recv_buf: Vec<MaybeUninit<u8>> = vec![MaybeUninit::uninit(); 1500];
        let resp_packet = self.socket.read_with(|s| s.recv_from(&mut recv_buf)).await;
        let (n, remote) = match resp_packet {
            Ok((n, r)) => (n, r),
            Err(_e) => {
                let mut recv_buf: Vec<u8> = vec![0; 1500];
                let result = self
                    .socket
                    .read_with(|s| {
                        let iov = IoSliceMut::new(recv_buf.as_mut_slice());
                        let mut cmsg_buffer = vec![0u8; 1500];
                        recvmsg::<SockaddrStorage>(
                            s.as_raw_fd(),
                            [iov].as_mut_slice(),
                            Some(&mut cmsg_buffer),
                            MsgFlags::MSG_ERRQUEUE,
                        )
                        .map_err(|e| e.into())
                        .map(|r| r.cmsgs().collect::<Vec<_>>())
                    })
                    .await;
                let result = result?;
                let icmp = EchoRequestPacket::new(&recv_buf[..]).unwrap();
                let seq = icmp.get_sequence_number();
                for msg in result {
                    match msg {
                        nix::sys::socket::ControlMessageOwned::Ipv4RecvErr(e, addr) => {
                            let addr = addr
                                .map(|a| Ipv4Addr::from((a.sin_addr.s_addr as u32).to_be()))
                                .unwrap_or_else(|| Ipv4Addr::new(0, 0, 0, 0));
                            if e.ee_origin == SO_EE_ORIGIN_ICMP {
                                return Err(IcmpError::from((e, addr, seq)));
                            } else {
                                return Err(IcmpError::UnknownOrigin(
                                    addr,
                                    seq,
                                    e.ee_origin,
                                    e.ee_code,
                                    e.ee_type,
                                ));
                            }
                        }
                        _ => {
                            panic!("Unexpected control message: {:?}", msg);
                        }
                    }
                }
                panic!("no msg");
            }
        };
        let mut recv_buf = recv_buf
            .into_iter()
            .map(|x| unsafe { x.assume_init() })
            .collect::<Vec<u8>>();
        recv_buf.truncate(n);
        let icmp = IcmpPacket::owned(recv_buf).unwrap();
        return Ok((icmp, *remote.as_socket_ipv4().unwrap().ip()));
    }
    async fn listen(&'static self) {
        for _i in 0..self.count {
            let icmp = self.recv().await;
            match icmp {
                Ok((icmp, remote)) => {
                    match icmp.get_icmp_type() {
                        IcmpTypes::EchoReply => {
                            let echo_reply: EchoReplyPacket =
                                EchoReplyPacket::new(icmp.packet()).unwrap();
                            let seq = echo_reply.get_sequence_number();
                            let duration = self.starts.read().await.index(seq as usize).elapsed();
                            let remote = remote.to_string();
                            info!(
                                "Received package #{seq} {} bytes from {} in {:?}",
                                icmp.packet_size(),
                                remote,
                                duration
                            );
                            self.latencies.lock().await[seq as usize] = Some(duration);

                            if self.graph {
                                let mut sent = self.latencies_sent.lock().await;
                                let mut add = 0;
                                for l in
                                    self.latencies.lock().await[*sent as usize..seq as usize].iter()
                                {
                                    if l.is_some() {
                                        add += 1;
                                        self.tx.send(*l).await.unwrap();
                                    } else {
                                        // wait for timeout thread to send this
                                        break;
                                    }
                                }
                                *sent += add;
                                drop(sent);
                            }

                            self.timeout_handles
                                .lock()
                                .await
                                .index(seq as usize)
                                .abort();
                        }
                        IcmpTypes::TimeExceeded => {
                            // let echo_reply = TimeExceededPacket::new(&recv_buf[..]).unwrap();
                            // let seq = echo_reply.get_sequence_number();
                            // let duration = self.starts.read().await.index(seq as usize).elapsed();
                            info!("Received package from {:?}: Time Exceeded", remote);
                        }
                        _ => {
                            warn!(
                                "Received package from {:?}: {:?}",
                                remote,
                                icmp.get_icmp_type()
                            );
                        }
                    }
                }
                Err(err) => match err {
                    IcmpError::NetworkUnreachable(_, seq)
                    | IcmpError::HostUnreachable(_, seq)
                    | IcmpError::ProtocolUnreachable(_, seq)
                    | IcmpError::PortUnreachable(_, seq)
                    | IcmpError::OtherUnreachable(_, seq, _)
                    | IcmpError::TimeExceeded(_, seq)
                    | IcmpError::Unknown(_, seq, _, _)
                    | IcmpError::UnknownOrigin(_, seq, _, _, _) => {
                        error!("{}", err);
                        self.timeout_handles
                            .lock()
                            .await
                            .index(seq as usize)
                            .abort();
                    }
                    IcmpError::Io(_) => {
                        error!("{}", err);
                    }
                },
            }
        }
        self.finished.notify_one();
    }
    async fn timeout(&self, seq: u16, listen_handle: &JoinHandle<()>) {
        sleep(self.timeout).await;
        error!("Timeout for package {seq}");

        if self.graph {
            // we can assure that this is the first, non_sent timeout package
            // we can safely send a none and add one to latencies_sent
            // if this package is followed by sent packages,
            // they will be send by the listen thread
            self.tx.send(None).await.unwrap();
            *self.latencies_sent.lock().await += 1;
        }

        if seq == self.count - 1 {
            listen_handle.abort();
            self.finished.notify_one();
        }
    }
}
