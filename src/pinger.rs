use std::{io, mem::MaybeUninit, ops::Index, os::unix::prelude::AsRawFd};

use async_io::Async;

use log::{debug, error, info, warn};
use pnet_packet::icmp::{
    echo_reply::EchoReplyPacket, echo_request::MutableEchoRequestPacket, IcmpPacket, IcmpTypes,
};
use socket2::{Domain, Protocol, SockAddr, Socket, Type};
use tokio::{
    sync::{Mutex, RwLock},
    task::JoinHandle,
    time::{sleep, Duration, Instant},
};

macro_rules! syscall {
    ($fn: ident ( $($arg: expr),* $(,)* ) ) => {{
        #[allow(unused_unsafe)]
        let res = unsafe { libc::$fn($($arg, )*) };
        if res == -1 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(res)
        }
    }};
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
}

impl Pinger {
    pub fn new(
        host: SockAddr,
        count: u16,
        broadcast: bool,
        size: u16,
        ttl: u8,
        timeout: Duration,
        interval: Duration,
    ) -> io::Result<Self> {
        let sock = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::ICMPV4)).unwrap();
        sock.set_broadcast(broadcast)?;
        sock.set_ttl(ttl as u32)?;
        let yes: i32 = 1;
        syscall!(setsockopt(
            sock.as_raw_fd(),
            libc::SOL_IP,
            libc::IP_RECVERR,
            &yes as *const i32 as *const libc::c_void,
            std::mem::size_of::<i32>() as u32,
        ))?;
        // syscall!(setsockopt(
        //     sock.as_raw_fd(),
        //     libc::SOL_IP,
        //     libc::IP_RECVTTL,
        //     &yes as *const i32 as *const libc::c_void,
        //     std::mem::size_of::<i32>() as u32,
        // ))?;
        // syscall!(setsockopt(
        //     sock.as_raw_fd(),
        //     libc::SOL_IP,
        //     libc::IP_RETOPTS,
        //     &yes as *const i32 as *const libc::c_void,
        //     std::mem::size_of::<i32>() as u32,
        // ))?;
        Ok(Pinger {
            socket: Async::new(sock)?,
            host,
            count,
            size,
            timeout,
            interval,
            starts: Default::default(),
            timeout_handles: Default::default(),
        })
    }
    pub async fn start(&'static mut self) {
        let b = tokio::spawn(self.listen());
        let a = tokio::spawn(self.ping(b));
        a.await.unwrap();
    }
    async fn ping(&'static self, listen_handle: JoinHandle<()>) {
        let listen_handle = Box::leak(Box::new(listen_handle));
        if self.starts.read().await.len() != 0 {
            panic!("Already started pinging!");
        }
        let mut data: Vec<u8> = vec![0; self.size as usize];
        for i in 0..self.count {
            let mut echo_packet = MutableEchoRequestPacket::new(&mut data[..]).unwrap();
            echo_packet.set_sequence_number(i);
            echo_packet.set_icmp_type(IcmpTypes::EchoRequest);

            self.socket
                .write_with(|socket| socket.send_to(&data, &self.host))
                .await
                .expect("OS Error: Failed to send packet");
            let now = Instant::now();
            self.starts.write().await.push(now);
            debug!("Sent package {i} to {:?}", self.host);
            self.timeout_handles
                .lock()
                .await
                .push(tokio::spawn(self.timeout(i, listen_handle)));
            sleep(self.interval).await;
        }
    }
    async fn listen(&self) {
        for i in 0..self.count {
            let mut recv_buf: Vec<MaybeUninit<u8>> = vec![MaybeUninit::uninit(); 1500];
            let resp_packet = self
                .socket
                .read_with(|s| {
                    s.recv_from(&mut recv_buf)
                    // let mut msg: libc::msghdr = unsafe { mem::zeroed() };

                    // unsafe {
                    //     SockAddr::init(|storage, len| {
                    //         // msg.msg_control
                    //         let msg_namelen = if storage.is_null() {
                    //             0
                    //         } else {
                    //             size_of::<sockaddr_storage>() as libc::socklen_t
                    //         };
                    //         msg.msg_name = storage.cast();
                    //         msg.msg_namelen = msg_namelen;
                    //         let iov = &mut iov[..];
                    //         msg.msg_iov = iov.as_mut_ptr().cast();
                    //         msg.msg_iovlen = min(iov.len(), usize::MAX);
                    //         msg.msg_control = control_buffer.as_mut_ptr() as *mut libc::c_void;
                    //         msg.msg_controllen = control_buffer.len();
                    //         msg.msg_flags = 0;

                    //         syscall!(recvmsg(s.as_raw_fd(), &mut msg, 0))
                    //             .map(|n| (n as usize, msg.msg_namelen, msg.msg_flags))
                    //             .map(|(n, addrlen, recv_flags)| {
                    //                 // Set the correct address length.
                    //                 *len = addrlen;
                    //                 (n, recv_flags)
                    //             })
                    //     })
                    // }
                })
                .await;
            let remote = match resp_packet {
                Ok((_, r)) => r,
                Err(e) => {
                    error!("OS Error: Failed to receive packet id {i}: {}", e);
                    continue;
                }
            };
            let recv_buf = recv_buf
                .into_iter()
                .map(|x| unsafe { x.assume_init() })
                .collect::<Vec<u8>>();
            let icmp = IcmpPacket::new(&recv_buf[..]).unwrap();
            match icmp.get_icmp_type() {
                IcmpTypes::EchoReply => {
                    let echo_reply: EchoReplyPacket = EchoReplyPacket::new(&recv_buf[..]).unwrap();
                    let seq = echo_reply.get_sequence_number();
                    let duration = self.starts.read().await.index(seq as usize).elapsed();
                    info!("Received package {seq} from {:?} in {:?}", remote, duration);
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
    }
    async fn timeout(&self, seq: u16, listen_handle: &JoinHandle<()>) {
        sleep(self.timeout).await;
        error!("Timeout for package {seq}");
        if seq == self.count - 1 {
            listen_handle.abort();
        }
    }
}
