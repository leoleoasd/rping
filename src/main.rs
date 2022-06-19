use pnet::datalink::{self, NetworkInterface};
use pnet::datalink::Channel::Ethernet;
use pnet::packet::icmp::echo_request::{IcmpCodes, EchoRequest, self};
use pnet::packet::icmp::{Icmp, IcmpTypes, checksum, IcmpPacket};
use pnet::packet::{Packet, MutablePacket};
use pnet::packet::ethernet::{EthernetPacket, MutableEthernetPacket};
use pnet::transport::{TransportChannelType::*, transport_channel, icmp_packet_iter};
use pnet::transport::TransportProtocol::*;
use pnet::packet::ip::IpNextHeaderProtocols;
use pnet::util;
use std::env;
use std::error::Error;
use std::net::IpAddr;

// Invoke as echo <interface name>
fn main() -> Result<(), Box<dyn Error>> {
    let protocol = Layer4(Ipv4(IpNextHeaderProtocols::Icmp));
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
    tx.set_ttl(128).unwrap();
    tx.send_to(echo_packet, "10.0.0.2".parse::<IpAddr>().unwrap())?;
    let mut it = icmp_packet_iter(&mut rx);
    while let Ok((p, a)) = it.next() {
        println!("{p:?} {a:?}");
        println!("{:?}", p.payload());
    }
    Ok(())
}