use pnet_packet::{
    icmp::{echo_reply::EchoReplyPacket, echo_request::MutableEchoRequestPacket, IcmpTypes},
    Packet,
};
use socket2::{Domain, Protocol, Socket, Type};
use std::{mem::MaybeUninit, net::SocketAddr};

use async_io::Async;

// Invoke as echo <interface name>
#[tokio::main]
async fn main() {
    // Create a TCP listener bound to two addresses.
    let socket =
        Async::new(Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::ICMPV4)).unwrap())
            .unwrap();
    let mut vec: Vec<u8> = vec![0; 32];

    // Use echo_request so we can set the identifier and sequence number
    let mut echo_packet = MutableEchoRequestPacket::new(&mut vec[..]).unwrap();
    echo_packet.set_sequence_number(201);
    echo_packet.set_icmp_type(IcmpTypes::EchoRequest);

    println!("{:?}", echo_packet);
    let addr: SocketAddr = "10.0.0.1:0".parse().unwrap();
    let addr = addr.into();

    println!(
        "{:?}",
        socket
            .write_with(|socket| { socket.send_to(echo_packet.packet(), &addr) })
            .await
    );
    let mut recv_buf: Vec<MaybeUninit<u8>> = vec![MaybeUninit::uninit(); 1500];
    let (a, b) = socket
        .read_with(|s| s.recv_from(&mut recv_buf))
        .await
        .unwrap();
    println!("{:?}", a);
    println!("{:?}", b.as_socket());
    let recv_buf = recv_buf
        .into_iter()
        .map(|x| unsafe { x.assume_init() })
        .collect::<Vec<u8>>();
    println!("{:?}", EchoReplyPacket::new(&recv_buf[..]).unwrap());
}
