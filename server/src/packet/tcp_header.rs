/// TCP 头的零拷贝读取器。
///
/// TCP 头格式（RFC 793）：
/// ```text
///  0                   1                   2                   3
///  0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |          Source Port          |       Destination Port        |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                        Sequence Number                        |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                    Acknowledgment Number                      |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |  Data |           |U|A|P|R|S|F|                               |
/// | Offset| Reserved  |R|C|S|S|Y|I|            Window             |
/// |       |           |G|K|H|T|N|N|                               |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |           Checksum            |         Urgent Pointer        |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                    Options                    |    Padding    |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// ```
pub struct TcpHeader<'a> {
    data: &'a [u8],
}

impl<'a> TcpHeader<'a> {
    /// 从字节切片创建 TCP 头读取器。
    ///
    /// 切片长度必须至少 20 字节（最小 TCP 头长度）。
    pub fn new(data: &'a [u8]) -> Self {
        assert!(data.len() >= 20, "TCP 头至少 20 字节");
        Self { data }
    }

    /// 源端口。
    pub fn source_port(&self) -> u16 {
        u16::from_be_bytes([self.data[0], self.data[1]])
    }

    /// 目的端口。
    pub fn dest_port(&self) -> u16 {
        u16::from_be_bytes([self.data[2], self.data[3]])
    }

    /// 序列号。
    pub fn seq_number(&self) -> u32 {
        u32::from_be_bytes([self.data[4], self.data[5], self.data[6], self.data[7]])
    }

    /// 确认号。
    pub fn ack_number(&self) -> u32 {
        u32::from_be_bytes([self.data[8], self.data[9], self.data[10], self.data[11]])
    }

    /// 数据偏移（头部长度），单位为 4 字节。
    pub fn data_offset(&self) -> u8 {
        self.data[12] >> 4
    }

    /// 头部长度，单位为字节。
    pub fn header_length_bytes(&self) -> usize {
        self.data_offset() as usize * 4
    }

    /// TCP 标志位（原始值，低 6 位有效）。
    pub fn flags(&self) -> u8 {
        self.data[13] & 0x3F
    }

    /// 窗口大小。
    pub fn window(&self) -> u16 {
        u16::from_be_bytes([self.data[14], self.data[15]])
    }

    /// 校验和。
    pub fn checksum(&self) -> u16 {
        u16::from_be_bytes([self.data[16], self.data[17]])
    }

    /// 紧急指针。
    pub fn urgent_pointer(&self) -> u16 {
        u16::from_be_bytes([self.data[18], self.data[19]])
    }

    // --- 标志位便捷方法 ---

    /// FIN 标志（结束连接）。
    pub fn is_fin(&self) -> bool {
        self.data[13] & 0x01 != 0
    }

    /// SYN 标志（同步/建立连接）。
    pub fn is_syn(&self) -> bool {
        self.data[13] & 0x02 != 0
    }

    /// RST 标志（重置连接）。
    pub fn is_rst(&self) -> bool {
        self.data[13] & 0x04 != 0
    }

    /// PSH 标志（推送数据）。
    pub fn is_psh(&self) -> bool {
        self.data[13] & 0x08 != 0
    }

    /// ACK 标志（确认号有效）。
    pub fn is_ack(&self) -> bool {
        self.data[13] & 0x10 != 0
    }

    /// URG 标志（紧急指针有效）。
    pub fn is_urg(&self) -> bool {
        self.data[13] & 0x20 != 0
    }

    /// 获取整个头部字节切片（含选项）。
    pub fn as_bytes(&self) -> &'a [u8] {
        &self.data[..self.header_length_bytes()]
    }
}

/// TCP 头的可变写入器。
pub struct TcpHeaderMut<'a> {
    data: &'a mut [u8],
}

impl<'a> TcpHeaderMut<'a> {
    /// 从可变字节切片创建 TCP 头写入器。
    pub fn new(data: &'a mut [u8]) -> Self {
        assert!(data.len() >= 20, "TCP 头至少 20 字节");
        Self { data }
    }

    /// 设置源端口。
    pub fn set_source_port(&mut self, port: u16) {
        self.data[0] = (port >> 8) as u8;
        self.data[1] = port as u8;
    }

    /// 设置目的端口。
    pub fn set_dest_port(&mut self, port: u16) {
        self.data[2] = (port >> 8) as u8;
        self.data[3] = port as u8;
    }

    /// 设置序列号。
    pub fn set_seq_number(&mut self, seq: u32) {
        self.data[4..8].copy_from_slice(&seq.to_be_bytes());
    }

    /// 设置确认号。
    pub fn set_ack_number(&mut self, ack: u32) {
        self.data[8..12].copy_from_slice(&ack.to_be_bytes());
    }

    /// 设置数据偏移和标志位。
    pub fn set_data_offset_and_flags(&mut self, data_offset: u8, flags: u8) {
        self.data[12] = data_offset << 4;
        self.data[13] = flags & 0x3F;
    }

    /// 设置窗口大小。
    pub fn set_window(&mut self, window: u16) {
        self.data[14] = (window >> 8) as u8;
        self.data[15] = window as u8;
    }

    /// 计算并设置 TCP 校验和（含伪首部）。
    pub fn compute_and_set_checksum(
        &mut self,
        source_ip: u32,
        dest_ip: u32,
        payload: &[u8],
    ) {
        // 先清零校验和字段
        self.data[16] = 0;
        self.data[17] = 0;

        // 将头部和 payload 拼起来计算
        let header_len = self.header_length_bytes();
        let mut combined = Vec::with_capacity(header_len + payload.len());
        combined.extend_from_slice(&self.data[..header_len]);
        combined.extend_from_slice(payload);

        let checksum = crate::packet::checksum::compute_transport_checksum(
            source_ip,
            dest_ip,
            6, // TCP 协议号
            &combined,
        );
        self.data[16] = (checksum >> 8) as u8;
        self.data[17] = checksum as u8;
    }

    /// 将 TCP 选项缩减为 0（头部长度变为 20 字节）。
    ///
    /// 在构造回传包时使用，因为我们不需要转发 TCP 选项。
    pub fn shrink_options(&mut self) {
        self.data[12] = 0x50; // data_offset=5 (20 字节)
    }

    /// 获取头部长度（字节）。
    fn header_length_bytes(&self) -> usize {
        (self.data[12] >> 4) as usize * 4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tcp_header_read() {
        let mut data = [0u8; 20];
        data[0] = 0x00; data[1] = 0x50; // source_port=80
        data[2] = 0x1F; data[3] = 0x90; // dest_port=8080
        data[4..8].copy_from_slice(&1000u32.to_be_bytes()); // seq=1000
        data[8..12].copy_from_slice(&2000u32.to_be_bytes()); // ack=2000
        data[12] = 0x50; // data_offset=5 (20 字节)
        data[13] = 0x12; // SYN+ACK

        let header = TcpHeader::new(&data);
        assert_eq!(header.source_port(), 80);
        assert_eq!(header.dest_port(), 8080);
        assert_eq!(header.seq_number(), 1000);
        assert_eq!(header.ack_number(), 2000);
        assert!(header.is_syn());
        assert!(header.is_ack());
        assert!(!header.is_fin());
        assert!(!header.is_rst());
    }
}
