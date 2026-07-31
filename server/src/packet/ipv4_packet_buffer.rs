use crate::packet::ipv4_packet::Ipv4Packet;
use std::io::Read;

/// 从 TCP 字节流中解析出完整的 IP 包。
///
/// ## 为什么需要这个？
///
/// ADB 隧道是 TCP 连接，Android 发来的 IP 包会被当作 TCP 数据流传过来。
/// TCP 是字节流协议，不保留消息边界。所以可能出现：
/// - 一次 read 读到两个半 IP 包
/// - 一次 read 读到半个 IP 包
///
/// 我们需要根据 IPv4 头的 `total_length` 字段，从流中正确切出每个完整的包。
///
/// ## 工作原理
///
/// 内部维护一个缓冲区 `buffer` 和一个 `length` 标记已用字节数。
/// 每次读入新数据后，尝试从缓冲区头部解析出完整 IP 包，
/// 直到剩余数据不足一个包为止。
pub struct Ipv4PacketBuffer {
    buffer: Vec<u8>,
    length: usize,
}

/// IP 包的最大长度（含头）。
const MAX_IPV4_PACKET_SIZE: usize = 65535;

/// IPv4 头最小长度。
const MIN_IPV4_HEADER_SIZE: usize = 20;

impl Ipv4PacketBuffer {
    /// 创建一个空的 IP 包缓冲区。
    pub fn new() -> Self {
        Self {
            buffer: vec![0; MAX_IPV4_PACKET_SIZE],
            length: 0,
        }
    }

    /// 从 Read 源读取数据到缓冲区。
    ///
    /// 返回 `Ok(true)` 表示读到了数据，`Ok(false)` 表示 EOF。
    pub fn read_from(&mut self, reader: &mut impl Read) -> std::io::Result<bool> {
        loop {
            let writable = &mut self.buffer[self.length..];
            if writable.is_empty() {
                log::warn!("Ipv4PacketBuffer 已满，无法继续读取");
                return Ok(true);
            }
            match reader.read(writable) {
                Ok(0) => return Ok(false),
                Ok(n) => {
                    self.length += n;
                    return Ok(true);
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    return Ok(true); // 无数据可读，不算错误
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// 获取当前缓冲区头部的一个完整 IP 包视图（不移除）。
    ///
    /// 如果数据不足以组成完整包，返回 None。
    pub fn as_ipv4_packet(&self) -> Option<Ipv4Packet<'_>> {
        if self.length < MIN_IPV4_HEADER_SIZE {
            return None;
        }
        let total_length = u16::from_be_bytes([self.buffer[2], self.buffer[3]]) as usize;
        if total_length < MIN_IPV4_HEADER_SIZE || total_length > MAX_IPV4_PACKET_SIZE {
            return None;
        }
        if self.length < total_length {
            return None;
        }
        Some(Ipv4Packet::new(&self.buffer[..total_length]))
    }

    /// 移除缓冲区头部的当前包（在 `as_ipv4_packet()` 后调用）。
    pub fn next(&mut self) {
        if self.length < MIN_IPV4_HEADER_SIZE {
            return;
        }
        let total_length = u16::from_be_bytes([self.buffer[2], self.buffer[3]]) as usize;
        if total_length < MIN_IPV4_HEADER_SIZE || total_length > self.length {
            return;
        }
        self.buffer.copy_within(total_length..self.length, 0);
        self.length -= total_length;
    }

    /// 获取可写入的空闲区域。
    ///
    /// 供 `TcpStream::read()` 直接写入，避免额外的拷贝。
    pub fn writable_slice(&mut self) -> &mut [u8] {
        &mut self.buffer[self.length..]
    }

    /// 数据写入完成后，更新已用长度。
    ///
    /// `count` 为本次写入的字节数。
    pub fn advance(&mut self, count: usize) {
        self.length += count;
        debug_assert!(self.length <= self.buffer.len());
    }

    /// 尝试从缓冲区中取出一个完整的 IP 包。
    ///
    /// 返回 `Some(packet_data)` 表示成功取出一个包，
    /// 返回 `None` 表示当前数据不足以组成一个完整的包。
    ///
    /// 取出后，包的数据会从缓冲区中移除（通过内存移动）。
    pub fn extract_packet(&mut self) -> Option<Vec<u8>> {
        // 至少需要 20 字节才能读取 IP 头的 total_length
        if self.length < MIN_IPV4_HEADER_SIZE {
            return None;
        }

        // 读取 total_length
        let total_length = u16::from_be_bytes([self.buffer[2], self.buffer[3]]) as usize;
        if total_length < MIN_IPV4_HEADER_SIZE || total_length > MAX_IPV4_PACKET_SIZE {
            // 无效的 total_length，丢弃所有数据防止无限循环
            log::warn!("无效的 IP 包 total_length: {total_length}，丢弃缓冲区");
            self.length = 0;
            return None;
        }

        // 数据不足以组成完整包
        if self.length < total_length {
            return None;
        }

        // 提取完整包
        let packet = self.buffer[..total_length].to_vec();

        // 移除已提取的数据
        self.buffer.copy_within(total_length..self.length, 0);
        self.length -= total_length;

        Some(packet)
    }

    /// 批量提取所有完整的 IP 包。
    pub fn extract_packets(&mut self) -> Vec<Vec<u8>> {
        let mut packets = Vec::new();
        while let Some(packet) = self.extract_packet() {
            packets.push(packet);
        }
        packets
    }

    /// 缓冲区中剩余的未解析字节数。
    pub fn remaining(&self) -> usize {
        self.length
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一个模拟的 IPv4 包（仅头，无 payload）。
    fn make_test_packet(total_length: u16) -> Vec<u8> {
        let mut packet = vec![0u8; total_length as usize];
        packet[0] = 0x45; // version=4, IHL=5
        packet[2] = (total_length >> 8) as u8;
        packet[3] = (total_length & 0xFF) as u8;
        packet
    }

    #[test]
    fn test_extract_single_packet() {
        let mut buf = Ipv4PacketBuffer::new();
        let packet = make_test_packet(40);

        // 写入一个完整包
        buf.writable_slice()[..packet.len()].copy_from_slice(&packet);
        buf.advance(packet.len());

        let extracted = buf.extract_packet().expect("应提取出一个包");
        assert_eq!(extracted.len(), 40);
        assert_eq!(extracted[0], 0x45);
        assert!(buf.is_empty());
    }

    #[test]
    fn test_extract_partial_packet() {
        let mut buf = Ipv4PacketBuffer::new();
        let packet = make_test_packet(60);

        // 只写入前 30 字节（不完整）
        buf.writable_slice()[..30].copy_from_slice(&packet[..30]);
        buf.advance(30);

        assert!(buf.extract_packet().is_none());
        assert_eq!(buf.remaining(), 30);
    }

    #[test]
    fn test_extract_multiple_packets() {
        let mut buf = Ipv4PacketBuffer::new();
        let p1 = make_test_packet(40);
        let p2 = make_test_packet(32);

        // 写入两个完整包
        let combined: Vec<u8> = p1.iter().chain(p2.iter()).copied().collect();
        buf.writable_slice()[..combined.len()].copy_from_slice(&combined);
        buf.advance(combined.len());

        let packets = buf.extract_packets();
        assert_eq!(packets.len(), 2);
        assert_eq!(packets[0].len(), 40);
        assert_eq!(packets[1].len(), 32);
        assert!(buf.is_empty());
    }

    #[test]
    fn test_extract_one_and_a_half() {
        let mut buf = Ipv4PacketBuffer::new();
        let p1 = make_test_packet(40);
        let p2 = make_test_packet(60);

        // 写入一个完整包 + 下一个包的前 20 字节
        let combined: Vec<u8> = p1.iter().chain(&p2[..20]).copied().collect();
        buf.writable_slice()[..combined.len()].copy_from_slice(&combined);
        buf.advance(combined.len());

        // 第一次应只提取出一个
        let extracted = buf.extract_packet().expect("应提取出第一个包");
        assert_eq!(extracted.len(), 40);

        // 第二次应失败（不完整）
        assert!(buf.extract_packet().is_none());
        assert_eq!(buf.remaining(), 20);
    }

    #[test]
    fn test_as_ipv4_packet_and_next() {
        let mut buf = Ipv4PacketBuffer::new();
        let p1 = make_test_packet(40);
        let p2 = make_test_packet(32);

        let combined: Vec<u8> = p1.iter().chain(p2.iter()).copied().collect();
        buf.writable_slice()[..combined.len()].copy_from_slice(&combined);
        buf.advance(combined.len());

        // as_ipv4_packet 不移除数据
        let pkt = buf.as_ipv4_packet().expect("应返回第一个包");
        assert_eq!(pkt.raw().len(), 40);
        assert_eq!(buf.remaining(), 72); // 数据还在

        // next 移除第一个包
        buf.next();
        assert_eq!(buf.remaining(), 32);

        let pkt2 = buf.as_ipv4_packet().expect("应返回第二个包");
        assert_eq!(pkt2.raw().len(), 32);
    }
}
