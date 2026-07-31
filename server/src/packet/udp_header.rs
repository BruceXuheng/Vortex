/// UDP 头的零拷贝读取器。
///
/// UDP 头格式（RFC 768）只有 8 字节：
/// ```text
///  0      7 8     15 16    23 24    31
/// +--------+--------+--------+--------+
/// |     Source      |   Destination   |
/// |      Port       |      Port       |
/// +--------+--------+--------+--------+
/// |                 |                  |
/// |     Length      |    Checksum      |
/// +--------+--------+--------+--------+
/// ```
pub struct UdpHeader<'a> {
    data: &'a [u8],
}

impl<'a> UdpHeader<'a> {
    /// 从字节切片创建 UDP 头读取器。
    ///
    /// 切片长度必须至少 8 字节。
    pub fn new(data: &'a [u8]) -> Self {
        assert!(data.len() >= 8, "UDP 头至少 8 字节");
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

    /// UDP 长度（头 + 数据）。
    pub fn length(&self) -> u16 {
        u16::from_be_bytes([self.data[4], self.data[5]])
    }

    /// 校验和。
    ///
    /// 在 Vortex 中我们将 UDP 校验和设为 0（合法禁用），
    /// 避免每次转发都要重新计算。
    pub fn checksum(&self) -> u16 {
        u16::from_be_bytes([self.data[6], self.data[7]])
    }

    /// 获取整个头部字节切片。
    pub fn as_bytes(&self) -> &'a [u8] {
        &self.data[..8]
    }
}

/// UDP 头的可变写入器。
pub struct UdpHeaderMut<'a> {
    data: &'a mut [u8],
}

impl<'a> UdpHeaderMut<'a> {
    /// 从可变字节切片创建 UDP 头写入器。
    pub fn new(data: &'a mut [u8]) -> Self {
        assert!(data.len() >= 8, "UDP 头至少 8 字节");
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

    /// 设置 UDP 长度。
    pub fn set_length(&mut self, length: u16) {
        self.data[4] = (length >> 8) as u8;
        self.data[5] = length as u8;
    }

    /// 将校验和设为 0（禁用 UDP 校验和，IPv4 中合法）。
    ///
    /// 这避免了转发时重新计算校验和的开销。
    pub fn set_checksum_zero(&mut self) {
        self.data[6] = 0;
        self.data[7] = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_udp_header_read() {
        let mut data = [0u8; 8];
        data[0] = 0x00; data[1] = 0x35; // source_port=53 (DNS)
        data[2] = 0x10; data[3] = 0x00; // dest_port=4096
        data[4] = 0x00; data[5] = 0x20; // length=32

        let header = UdpHeader::new(&data);
        assert_eq!(header.source_port(), 53);
        assert_eq!(header.dest_port(), 4096);
        assert_eq!(header.length(), 32);
    }
}
