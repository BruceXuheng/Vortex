use std::net::Ipv4Addr;

/// IPv4 头的零拷贝读取器。
///
/// 不拥有数据，只是在原始字节切片上提供便捷的访问方法。
/// 所有字段按照网络字节序（大端）解析。
///
/// IPv4 头格式（RFC 791）：
/// ```text
///  0                   1                   2                   3
///  0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |Version|  IHL  |Type of Service|          Total Length         |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |         Identification        |Flags|      Fragment Offset    |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |  Time to Live |    Protocol   |         Header Checksum       |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                    Source Address                             |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                 Destination Address                           |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                    Options                    |    Padding    |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// ```
pub struct Ipv4Header<'a> {
    data: &'a [u8],
}

impl<'a> Ipv4Header<'a> {
    /// 从字节切片创建 IPv4 头读取器。
    ///
    /// 切片长度必须至少 20 字节（最小 IPv4 头长度）。
    pub fn new(data: &'a [u8]) -> Self {
        assert!(data.len() >= 20, "IPv4 头至少 20 字节");
        Self { data }
    }

    /// 版本号，应为 4。
    pub fn version(&self) -> u8 {
        self.data[0] >> 4
    }

    /// 头部长度，单位为 4 字节。最小值 5（20 字节），最大值 15（60 字节）。
    pub fn header_length(&self) -> u8 {
        self.data[0] & 0x0F
    }

    /// 头部长度，单位为字节。
    pub fn header_length_bytes(&self) -> usize {
        self.header_length() as usize * 4
    }

    /// 服务类型（Type of Service）。
    pub fn tos(&self) -> u8 {
        self.data[1]
    }

    /// 整个 IP 包的总长度（头 + 数据），单位为字节。
    pub fn total_length(&self) -> u16 {
        u16::from_be_bytes([self.data[2], self.data[3]])
    }

    /// 标识字段，用于分片重组。
    pub fn identification(&self) -> u16 {
        u16::from_be_bytes([self.data[4], self.data[5]])
    }

    /// 标志和分片偏移。
    pub fn flags_and_fragment_offset(&self) -> u16 {
        u16::from_be_bytes([self.data[6], self.data[7]])
    }

    /// 生存时间（TTL）。
    pub fn ttl(&self) -> u8 {
        self.data[8]
    }

    /// 协议号。
    ///
    /// 常见值：6 = TCP, 17 = UDP, 1 = ICMP
    pub fn protocol(&self) -> u8 {
        self.data[9]
    }

    /// 头部校验和。
    pub fn checksum(&self) -> u16 {
        u16::from_be_bytes([self.data[10], self.data[11]])
    }

    /// 源 IP 地址。
    pub fn source(&self) -> Ipv4Addr {
        Ipv4Addr::new(self.data[12], self.data[13], self.data[14], self.data[15])
    }

    /// 源 IP 地址（原始 u32）。
    pub fn source_u32(&self) -> u32 {
        u32::from_be_bytes([self.data[12], self.data[13], self.data[14], self.data[15]])
    }

    /// 目的 IP 地址。
    pub fn destination(&self) -> Ipv4Addr {
        Ipv4Addr::new(self.data[16], self.data[17], self.data[18], self.data[19])
    }

    /// 目的 IP 地址（原始 u32）。
    pub fn destination_u32(&self) -> u32 {
        u32::from_be_bytes([self.data[16], self.data[17], self.data[18], self.data[19]])
    }

    /// 获取整个头部字节切片（含选项）。
    pub fn as_bytes(&self) -> &'a [u8] {
        &self.data[..self.header_length_bytes()]
    }
}

/// IPv4 头的可变写入器。
///
/// 用于构造回传给 Android 的 IP 包头部。
pub struct Ipv4HeaderMut<'a> {
    data: &'a mut [u8],
}

impl<'a> Ipv4HeaderMut<'a> {
    /// 从可变字节切片创建 IPv4 头写入器。
    pub fn new(data: &'a mut [u8]) -> Self {
        assert!(data.len() >= 20, "IPv4 头至少 20 字节");
        Self { data }
    }

    /// 设置版本和头部长度。
    pub fn set_version_and_header_length(&mut self, version: u8, header_length: u8) {
        self.data[0] = (version << 4) | (header_length & 0x0F);
    }

    /// 设置总长度。
    pub fn set_total_length(&mut self, length: u16) {
        self.data[2] = (length >> 8) as u8;
        self.data[3] = length as u8;
    }

    /// 设置协议号。
    pub fn set_protocol(&mut self, protocol: u8) {
        self.data[9] = protocol;
    }

    /// 设置源 IP 地址。
    pub fn set_source(&mut self, addr: Ipv4Addr) {
        self.data[12..16].copy_from_slice(&addr.octets());
    }

    /// 设置目的 IP 地址。
    pub fn set_destination(&mut self, addr: Ipv4Addr) {
        self.data[16..20].copy_from_slice(&addr.octets());
    }

    /// 计算并设置校验和。
    ///
    /// 调用前应确保校验和字段为 0。
    pub fn compute_and_set_checksum(&mut self) {
        // 先清零校验和字段
        self.data[10] = 0;
        self.data[11] = 0;
        let checksum = crate::packet::checksum::compute_ipv4_checksum(
            &self.data[..self.header_length_bytes()],
        );
        self.data[10] = (checksum >> 8) as u8;
        self.data[11] = checksum as u8;
    }

    /// 获取头部长度（字节）。
    fn header_length_bytes(&self) -> usize {
        (self.data[0] & 0x0F) as usize * 4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipv4_header_read() {
        // 构造一个最小的 IPv4 头
        let mut data = [0u8; 20];
        data[0] = 0x45; // version=4, IHL=5
        data[2] = 0x00; data[3] = 0x28; // total_length=40
        data[8] = 64;   // TTL=64
        data[9] = 6;    // protocol=TCP
        data[12] = 192; data[13] = 168; data[14] = 1; data[15] = 1;
        data[16] = 10; data[17] = 0; data[18] = 0; data[19] = 2;

        let header = Ipv4Header::new(&data);
        assert_eq!(header.version(), 4);
        assert_eq!(header.header_length(), 5);
        assert_eq!(header.header_length_bytes(), 20);
        assert_eq!(header.total_length(), 40);
        assert_eq!(header.protocol(), 6);
        assert_eq!(header.source(), Ipv4Addr::new(192, 168, 1, 1));
        assert_eq!(header.destination(), Ipv4Addr::new(10, 0, 0, 2));
    }
}
