use std::{net::{SocketAddr, TcpListener}, mem::MaybeUninit};
use pnet::packet::{icmp::{echo_request, IcmpTypes, echo_reply::EchoReplyPacket}, Packet, util};
use socket2::{Socket, Domain, Type, Protocol};

// Invoke as echo <interface name>
fn main() {
    
    // Create a TCP listener bound to two addresses.
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::ICMPV4)).unwrap();
    let mut vec: Vec<u8> = vec![0; 1024];

    // Use echo_request so we can set the identifier and sequence number
    let mut echo_packet = echo_request::MutableEchoRequestPacket::new(&mut vec[..]).unwrap();
    echo_packet.set_sequence_number(201);
    echo_packet.set_identifier(0);
    echo_packet.set_icmp_type(IcmpTypes::EchoRequest);
    
    let csum = util::checksum(echo_packet.packet(), 1);
    echo_packet.set_checksum(csum);

    println!("{:?}", echo_packet);
    let addr: SocketAddr = "10.0.0.1:0".parse().unwrap();
    let addr = addr.into();

    println!("{:?}", socket.send_to(echo_packet.packet(), &addr));
    let mut recv_buf: Vec<MaybeUninit<u8>> = vec![MaybeUninit::uninit(); 1500];
    let (a,b) = socket.recv_from(&mut recv_buf).unwrap();
    println!("{:?}",  a);
    println!("{:?}",  b.as_socket());
    let recv_buf = recv_buf.into_iter().map(|x| unsafe {x.assume_init()}).collect::<Vec<u8>>();
    println!("{:?}",  EchoReplyPacket::new(&recv_buf[..]).unwrap());
}
