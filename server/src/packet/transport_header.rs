use crate::packet::tcp_header::TcpHeader;
use crate::packet::udp_header::UdpHeader;

/// IP 协议号常量。
pub const PROTOCOL_TCP: u8 = 6;
pub const PROTOCOL_UDP: u8 = 17;

/// 传输层头部的统一抽象。
///
/// 根据 IP 头的协议字段，解析为 TCP 或 UDP 头。
pub enum TransportHeader<'a> {
    Tcp(TcpHeader<'a>),
    Udp(UdpHeader<'a>),
}

impl<'a> TransportHeader<'a> {
    /// 根据 IPv4 头中的协议号，从 payload 中解析传输层头。
    ///
    /// `ipv4_payload` 是 IP 头之后的数据（即传输层头 + 应用层数据）。
    pub fn from_ipv4_payload(ipv4_payload: &'a [u8], protocol: u8) -> Option<Self> {
        match protocol {
            PROTOCOL_TCP => {
                if ipv4_payload.len() >= 20 {
                    Some(TransportHeader::Tcp(TcpHeader::new(ipv4_payload)))
                } else {
                    None
                }
            }
            PROTOCOL_UDP => {
                if ipv4_payload.len() >= 8 {
                    Some(TransportHeader::Udp(UdpHeader::new(ipv4_payload)))
                } else {
                    None
                }
            }
            _ => None, // ICMP 等其他协议暂不支持
        }
    }

    /// 获取源端口（TCP/UDP 通用）。
    pub fn source_port(&self) -> u16 {
        match self {
            TransportHeader::Tcp(h) => h.source_port(),
            TransportHeader::Udp(h) => h.source_port(),
        }
    }

    /// 获取目的端口（TCP/UDP 通用）。
    pub fn dest_port(&self) -> u16 {
        match self {
            TransportHeader::Tcp(h) => h.dest_port(),
            TransportHeader::Udp(h) => h.dest_port(),
        }
    }

    /// 获取传输层头长度（字节）。
    pub fn header_length_bytes(&self) -> usize {
        match self {
            TransportHeader::Tcp(h) => h.header_length_bytes(),
            TransportHeader::Udp(_) => 8,
        }
    }
}
