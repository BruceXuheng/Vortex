use crate::packet::checksum;
use crate::packet::ipv4_header::Ipv4Header;

/// IP 包构造器。
///
/// 从真实网络收到响应数据后，需要重新封装成 IP 包发回 Android。
/// 构造过程：
/// 1. 从参考包（Android 发来的原始包）复制 IP 头和传输层头
/// 2. 交换 src/dst IP 和端口
/// 3. 写入新的 payload
/// 4. 重新计算校验和
///
/// 这个结构体缓存了一个 65536 字节的 buffer，避免每次构造都分配内存。
pub struct Packetizer {
    buffer: Box<[u8; 65536]>,
    /// 参考包缓存（用于构造回传 IP 包）。
    reference_packet: Vec<u8>,
}

impl Packetizer {
    /// 创建新的 Packetizer。
    pub fn new() -> Self {
        Self {
            buffer: Box::new([0u8; 65536]),
            reference_packet: Vec::new(),
        }
    }

    /// 设置参考包。
    pub fn set_reference_packet(&mut self, packet: &[u8]) {
        self.reference_packet = packet.to_vec();
    }

    /// 获取参考包。
    pub fn reference_packet(&self) -> &[u8] {
        &self.reference_packet
    }

    /// 从参考 IP 包构造一个回传的 UDP 包。
    ///
    /// `reference_packet` 是 Android 发来的原始 IP 包，用于复制头部模板。
    /// `payload` 是从真实网络收到的 UDP 数据。
    ///
    /// 返回构造好的完整 IP 包字节切片。
    pub fn create_udp_packet(
        &mut self,
        reference_packet: &[u8],
        payload: &[u8],
    ) -> &[u8] {
        let ipv4_header = Ipv4Header::new(reference_packet);
        let ipv4_header_len = ipv4_header.header_length_bytes();

        // UDP 头固定 8 字节
        let udp_header_len = 8;
        let transport_header_len = udp_header_len;
        let transport_len = transport_header_len + payload.len();
        let total_len = ipv4_header_len + transport_len;

        // 1. 复制 IPv4 头到 buffer
        self.buffer[..ipv4_header_len].copy_from_slice(&reference_packet[..ipv4_header_len]);

        // 2. 交换 src/dst IP
        let src_ip = ipv4_header.destination_u32();
        let dst_ip = ipv4_header.source_u32();
        self.buffer[12..16].copy_from_slice(&src_ip.to_be_bytes());
        self.buffer[16..20].copy_from_slice(&dst_ip.to_be_bytes());

        // 3. 设置 total_length
        self.buffer[2] = (total_len >> 8) as u8;
        self.buffer[3] = total_len as u8;

        // 4. 复制 UDP 头模板
        let transport_offset = ipv4_header_len;
        let ref_transport_start = ipv4_header_len;
        if reference_packet.len() >= ref_transport_start + udp_header_len {
            self.buffer[transport_offset..transport_offset + udp_header_len]
                .copy_from_slice(&reference_packet[ref_transport_start..ref_transport_start + udp_header_len]);
        }

        // 5. 交换 src/dst 端口
        let src_port = u16::from_be_bytes([reference_packet[ref_transport_start + 2], reference_packet[ref_transport_start + 3]]);
        let dst_port = u16::from_be_bytes([reference_packet[ref_transport_start], reference_packet[ref_transport_start + 1]]);
        self.buffer[transport_offset] = (src_port >> 8) as u8;
        self.buffer[transport_offset + 1] = src_port as u8;
        self.buffer[transport_offset + 2] = (dst_port >> 8) as u8;
        self.buffer[transport_offset + 3] = dst_port as u8;

        // 6. 设置 UDP length
        self.buffer[transport_offset + 4] = (transport_len >> 8) as u8;
        self.buffer[transport_offset + 5] = transport_len as u8;

        // 7. UDP 校验和设为 0（禁用）
        self.buffer[transport_offset + 6] = 0;
        self.buffer[transport_offset + 7] = 0;

        // 8. 写入 payload
        let payload_offset = transport_offset + transport_header_len;
        self.buffer[payload_offset..payload_offset + payload.len()].copy_from_slice(payload);

        // 9. 计算 IPv4 头校验和
        self.buffer[10] = 0;
        self.buffer[11] = 0;
        let ipv4_checksum = checksum::compute_ipv4_checksum(&self.buffer[..ipv4_header_len]);
        self.buffer[10] = (ipv4_checksum >> 8) as u8;
        self.buffer[11] = ipv4_checksum as u8;

        &self.buffer[..total_len]
    }

    /// 从参考 IP 包构造一个回传的 TCP 包。
    ///
    /// 与 UDP 版本类似，但需要：
    /// - 缩减 TCP 选项到 20 字节（不转发选项）
    /// - 计算 TCP 伪首部校验和
    pub fn create_tcp_packet(
        &mut self,
        reference_packet: &[u8],
        payload: &[u8],
        seq_number: u32,
        ack_number: u32,
        flags: u8,
        window: u16,
    ) -> &[u8] {
        let ipv4_header = Ipv4Header::new(reference_packet);
        let ipv4_header_len = ipv4_header.header_length_bytes();

        // TCP 头缩减为 20 字节（无选项）
        let tcp_header_len = 20;
        let total_len = ipv4_header_len + tcp_header_len + payload.len();

        // 1. 复制 IPv4 头
        self.buffer[..ipv4_header_len].copy_from_slice(&reference_packet[..ipv4_header_len]);

        // 2. 交换 src/dst IP
        let src_ip = ipv4_header.destination_u32();
        let dst_ip = ipv4_header.source_u32();
        self.buffer[12..16].copy_from_slice(&src_ip.to_be_bytes());
        self.buffer[16..20].copy_from_slice(&dst_ip.to_be_bytes());

        // 3. 设置 total_length
        self.buffer[2] = (total_len >> 8) as u8;
        self.buffer[3] = total_len as u8;

        // 4. 构造 TCP 头（20 字节）
        let transport_offset = ipv4_header_len;
        // 交换 src/dst 端口
        let ref_transport_start = ipv4_header_len;
        let src_port = u16::from_be_bytes([reference_packet[ref_transport_start + 2], reference_packet[ref_transport_start + 3]]);
        let dst_port = u16::from_be_bytes([reference_packet[ref_transport_start], reference_packet[ref_transport_start + 1]]);
        self.buffer[transport_offset] = (src_port >> 8) as u8;
        self.buffer[transport_offset + 1] = src_port as u8;
        self.buffer[transport_offset + 2] = (dst_port >> 8) as u8;
        self.buffer[transport_offset + 3] = dst_port as u8;
        // seq
        self.buffer[transport_offset + 4..transport_offset + 8].copy_from_slice(&seq_number.to_be_bytes());
        // ack
        self.buffer[transport_offset + 8..transport_offset + 12].copy_from_slice(&ack_number.to_be_bytes());
        // data_offset=5 (20字节), flags
        self.buffer[transport_offset + 12] = 0x50; // data_offset=5
        self.buffer[transport_offset + 13] = flags;
        // window
        self.buffer[transport_offset + 14] = (window >> 8) as u8;
        self.buffer[transport_offset + 15] = window as u8;
        // checksum 先清零
        self.buffer[transport_offset + 16] = 0;
        self.buffer[transport_offset + 17] = 0;
        // urgent pointer = 0
        self.buffer[transport_offset + 18] = 0;
        self.buffer[transport_offset + 19] = 0;

        // 5. 写入 payload
        let payload_offset = transport_offset + tcp_header_len;
        self.buffer[payload_offset..payload_offset + payload.len()].copy_from_slice(payload);

        // 6. 计算 TCP 伪首部校验和
        let tcp_checksum = checksum::compute_transport_checksum(
            src_ip,
            dst_ip,
            6, // TCP
            &self.buffer[transport_offset..transport_offset + tcp_header_len + payload.len()],
        );
        self.buffer[transport_offset + 16] = (tcp_checksum >> 8) as u8;
        self.buffer[transport_offset + 17] = tcp_checksum as u8;

        // 7. 计算 IPv4 头校验和
        self.buffer[10] = 0;
        self.buffer[11] = 0;
        let ipv4_checksum = checksum::compute_ipv4_checksum(&self.buffer[..ipv4_header_len]);
        self.buffer[10] = (ipv4_checksum >> 8) as u8;
        self.buffer[11] = ipv4_checksum as u8;

        &self.buffer[..total_len]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_reference_udp_packet() -> Vec<u8> {
        // 20 (IPv4) + 8 (UDP) + 4 (payload) = 32 bytes
        let mut packet = vec![0u8; 32];
        packet[0] = 0x45; // version=4, IHL=5
        packet[2] = 0x00; packet[3] = 0x20; // total_length=32
        packet[9] = 17;   // UDP
        packet[12] = 192; packet[13] = 168; packet[14] = 1; packet[15] = 1; // src
        packet[16] = 10; packet[17] = 0; packet[18] = 0; packet[19] = 2;   // dst
        // UDP 头
        packet[20] = 0x00; packet[21] = 0x35; // src_port=53
        packet[22] = 0x10; packet[23] = 0x00; // dst_port=4096
        packet[24] = 0x00; packet[25] = 0x0C; // length=12
        packet
    }

    #[test]
    fn test_create_udp_packet() {
        let mut packetizer = Packetizer::new();
        let reference = make_reference_udp_packet();
        let payload = b"test";

        let result = packetizer.create_udp_packet(&reference, payload);

        // 验证总长度 = 20 + 8 + 4 = 32
        assert_eq!(result.len(), 32);

        // 验证 IP 头：src/dst 交换
        let src_ip = u32::from_be_bytes([result[12], result[13], result[14], result[15]]);
        let dst_ip = u32::from_be_bytes([result[16], result[17], result[18], result[19]]);
        assert_eq!(src_ip, 0x0A000002); // 原来的 dst
        assert_eq!(dst_ip, 0xC0A80101); // 原来的 src

        // 验证端口交换
        let src_port = u16::from_be_bytes([result[20], result[21]]);
        let dst_port = u16::from_be_bytes([result[22], result[23]]);
        assert_eq!(src_port, 4096); // 原来的 dst_port
        assert_eq!(dst_port, 53);   // 原来的 src_port

        // 验证 payload
        assert_eq!(&result[28..32], b"test");
    }
}
