use crate::packet::ipv4_header::Ipv4Header;
use crate::packet::transport_header::TransportHeader;

/// IP 包的最大长度（含头）。
pub const MAX_PACKET_LENGTH: usize = 65535;

/// IPv4 包的整体抽象。
///
/// 一个 IP 包由三部分组成：
/// 1. IPv4 头（20~60 字节）
/// 2. 传输层头（TCP 20~60 字节，UDP 8 字节）
/// 3. 应用层数据（payload）
///
/// 这个结构体在原始字节上提供便捷的分层访问，
/// 不做任何数据拷贝。
pub struct Ipv4Packet<'a> {
    data: &'a [u8],
}

impl<'a> Ipv4Packet<'a> {
    /// 从字节切片创建 IP 包读取器。
    ///
    /// 切片应包含完整的 IP 包（头 + 数据）。
    pub fn new(data: &'a [u8]) -> Self {
        Self { data }
    }

    /// 获取原始字节切片。
    pub fn raw(&self) -> &'a [u8] {
        self.data
    }

    /// 获取 IPv4 头。
    pub fn ipv4_header(&self) -> Ipv4Header<'a> {
        Ipv4Header::new(self.data)
    }

    /// 获取 IP 头之后的 payload（即传输层头 + 应用数据）。
    pub fn ipv4_payload(&self) -> &'a [u8] {
        let header_len = self.ipv4_header().header_length_bytes();
        if header_len <= self.data.len() {
            &self.data[header_len..]
        } else {
            &[]
        }
    }

    /// 解析传输层头。
    ///
    /// 根据 IP 头的协议号，自动判断是 TCP 还是 UDP。
    pub fn transport_header(&self) -> Option<TransportHeader<'a>> {
        let protocol = self.ipv4_header().protocol();
        TransportHeader::from_ipv4_payload(self.ipv4_payload(), protocol)
    }

    /// 获取传输层 payload（应用层数据）。
    ///
    /// 即传输层头之后的数据部分。
    pub fn transport_payload(&self) -> &'a [u8] {
        let ipv4_payload = self.ipv4_payload();
        if let Some(transport) = self.transport_header() {
            let transport_header_len = transport.header_length_bytes();
            if transport_header_len <= ipv4_payload.len() {
                return &ipv4_payload[transport_header_len..];
            }
        }
        &[]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipv4_packet_tcp() {
        // 构造一个最小的 TCP/IP 包（20 + 20 = 40 字节，无 payload）
        let mut packet = vec![0u8; 40];
        // IPv4 头
        packet[0] = 0x45; // version=4, IHL=5
        packet[2] = 0x00; packet[3] = 0x28; // total_length=40
        packet[9] = 6;    // protocol=TCP
        packet[12] = 192; packet[13] = 168; packet[14] = 1; packet[15] = 1;
        packet[16] = 10; packet[17] = 0; packet[18] = 0; packet[19] = 2;
        // TCP 头
        packet[20] = 0x00; packet[21] = 0x50; // source_port=80
        packet[22] = 0x1F; packet[23] = 0x90; // dest_port=8080
        packet[32] = 0x50; // data_offset=5

        let ipv4_packet = Ipv4Packet::new(&packet);
        assert_eq!(ipv4_packet.ipv4_header().protocol(), 6);
        assert_eq!(ipv4_packet.ipv4_header().total_length(), 40);

        let transport = ipv4_packet.transport_header().expect("应解析为 TCP");
        match transport {
            TransportHeader::Tcp(tcp) => {
                assert_eq!(tcp.source_port(), 80);
                assert_eq!(tcp.dest_port(), 8080);
            }
            TransportHeader::Udp(_) => panic!("应为 TCP"),
        }
    }

    #[test]
    fn test_ipv4_packet_udp() {
        // 构造一个 UDP/IP 包（20 + 8 + 4 = 32 字节）
        let mut packet = vec![0u8; 32];
        packet[0] = 0x45; // version=4, IHL=5
        packet[2] = 0x00; packet[3] = 0x20; // total_length=32
        packet[9] = 17;   // protocol=UDP
        // UDP 头
        packet[20] = 0x00; packet[21] = 0x35; // source_port=53
        packet[22] = 0x10; packet[23] = 0x00; // dest_port=4096
        packet[24] = 0x00; packet[25] = 0x0C; // length=12

        let ipv4_packet = Ipv4Packet::new(&packet);
        let transport = ipv4_packet.transport_header().expect("应解析为 UDP");
        match transport {
            TransportHeader::Udp(udp) => {
                assert_eq!(udp.source_port(), 53);
                assert_eq!(udp.dest_port(), 4096);
            }
            TransportHeader::Tcp(_) => panic!("应为 UDP"),
        }

        // UDP payload 应为 4 字节
        assert_eq!(ipv4_packet.transport_payload().len(), 4);
    }
}
