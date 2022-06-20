use std::{net::{SocketAddr, TcpListener}, mem::MaybeUninit};
use pnet::packet::{icmp::{echo_request, IcmpTypes, echo_reply::EchoReplyPacket}, Packet, util};
use socket2::{Socket, Domain, Type, Protocol};
use tokio::main;
use async_io::{Async, Timer};

// Invoke as echo <interface name>
#[tokio::main]
async fn main() {
    
    // Create a TCP listener bound to two addresses.
    let socket = Async::new(Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::ICMPV4)).unwrap()).unwrap();
    let mut vec: Vec<u8> = vec![0; 1024];

    // Use echo_request so we can set the identifier and sequence number
    let mut echo_packet = echo_request::MutableEchoRequestPacket::new(&mut vec[..]).unwrap();
    echo_packet.set_sequence_number(201);
    echo_packet.set_icmp_type(IcmpTypes::EchoRequest);

    println!("{:?}", echo_packet);
    let addr: SocketAddr = "10.0.0.1:0".parse().unwrap();
    let addr = addr.into();

    println!("{:?}", socket.write_with(|socket| {
        socket.send_to(echo_packet.packet(), &addr)
    }).await);
    let mut recv_buf: Vec<MaybeUninit<u8>> = vec![MaybeUninit::uninit(); 1500];
    let (a,b) = socket.read_with(|s| {
        s.recv_from(&mut recv_buf)
    }).await.unwrap();
    println!("{:?}",  a);
    println!("{:?}",  b.as_socket());
    let recv_buf = recv_buf.into_iter().map(|x| unsafe {x.assume_init()}).collect::<Vec<u8>>();
    println!("{:?}",  EchoReplyPacket::new(&recv_buf[..]).unwrap());
}
