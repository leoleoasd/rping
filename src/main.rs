use pnet::packet::icmp::echo_request::{self};
use pnet::packet::icmp::{IcmpTypes, IcmpPacket};
use pnet::packet::{FromPacket, Packet, PacketSize, ip, ipv4};

use pnet::packet::ip::IpNextHeaderProtocols;
use pnet::transport::{TransportProtocol::*, icmp_packet_iter, ipv4_packet_iter};
use pnet::transport::{transport_channel, transport_channel_iterator, TransportChannelType::*};
use pnet::util;

use pnet::packet::icmp::echo_reply::EchoReplyPacket;
use pnet::packet::ipv4::{Ipv4Packet, MutableIpv4Packet};
use pnet_sys;
use std::error::Error;
use std::io;
use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use std::io::ErrorKind;
use std::mem;
use std::net::{self};

use pnet::transport::TransportReceiver;

transport_channel_iterator!(
    EchoReplyPacket,
    EchoReplyTransportChannelIterator,
    icmp_echo_reply_packet_iter
);

// Invoke as echo <interface name>
fn main() -> Result<(), Box<dyn Error>> {
    let protocol = Layer3(IpNextHeaderProtocols::Icmp);
    let (mut tx, mut rx) = transport_channel(4096, protocol).unwrap();
    // Allocate enough space for a new packet
    let mut vec: Vec<u8> = vec![0; 16];

    // Use echo_request so we can set the identifier and sequence number
    let mut echo_packet = echo_request::MutableEchoRequestPacket::new(&mut vec[..]).unwrap();
    echo_packet.set_sequence_number(20);
    echo_packet.set_identifier(2);
    echo_packet.set_icmp_type(IcmpTypes::EchoRequest);

    let csum = util::checksum(echo_packet.packet(), 1);
    echo_packet.set_checksum(csum);
    tx.set_ttl(1).unwrap();
    let mut ip_vec: Vec<u8> = vec![0; Ipv4Packet::minimum_packet_size() + 16];
    let mut ip_packet = MutableIpv4Packet::new(&mut ip_vec[..]).unwrap();

    let total_len = (20 + 16) as u16;

    ip_packet.set_version(4);
    ip_packet.set_header_length(5);
    ip_packet.set_total_length(total_len);
    ip_packet.set_ttl(128);
    ip_packet.set_next_level_protocol(IpNextHeaderProtocols::Icmp);
    ip_packet.set_source(Ipv4Addr::new(172, 31, 135, 147));
    ip_packet.set_destination(Ipv4Addr::new(10, 0,0,2));

    let checksum = ipv4::checksum(&ip_packet.to_immutable());
    ip_packet.set_checksum(checksum);
    ip_packet.set_payload(echo_packet.packet());

    tx.send_to(ip_packet, "162.31.135.1".parse::<IpAddr>().unwrap()).unwrap();

    
    let mut it = ipv4_packet_iter(&mut rx);
    while let Ok((p, a)) = it.next() {
        println!("{:?}", EchoReplyPacket::new(p.payload()));
    }
    Ok(())
}
