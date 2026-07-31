/// 环形字节流缓冲区。
///
/// 用于 Client 的 `network_to_client` 方向：多个 IP 包的数据
/// 需要按顺序写入 TCP 流发回 Android，这个缓冲区暂存待写入的数据。
///
/// 内部使用 `Vec<u8>` 而非固定大小环形数组，因为 IP 包最大 65535 字节，
/// 且需要动态增长以容纳突发数据。
pub struct StreamBuffer {
    data: Vec<u8>,
    capacity: usize,
}

/// 默认缓冲区容量（16 * MAX_PACKET_LENGTH）。
const DEFAULT_CAPACITY: usize = 16 * 65535;

impl StreamBuffer {
    /// 创建空缓冲区（使用默认容量）。
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    /// 创建指定容量的缓冲区。
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            data: Vec::new(),
            capacity,
        }
    }

    /// 追加数据到缓冲区尾部。
    pub fn extend_from_slice(&mut self, slice: &[u8]) {
        self.data.extend_from_slice(slice);
    }

    /// 从源数据复制到缓冲区尾部（对齐 Gnirehtet 的 read_from）。
    pub fn read_from(&mut self, data: &[u8]) {
        self.data.extend_from_slice(data);
    }

    /// 获取待写入的数据切片。
    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }

    /// 消费已写入的前 `count` 字节。
    pub fn consume(&mut self, count: usize) {
        if count >= self.data.len() {
            self.data.clear();
        } else {
            self.data.copy_within(count.., 0);
            self.data.truncate(self.data.len() - count);
        }
    }

    /// 缓冲区是否为空。
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// 缓冲区中的字节数。
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// 剩余可用空间。
    pub fn remaining(&self) -> usize {
        self.capacity.saturating_sub(self.data.len())
    }

    /// 将缓冲区数据写入目标 Write。
    pub fn write_to(&mut self, writer: &mut impl std::io::Write) -> std::io::Result<usize> {
        let written = writer.write(self.data.as_slice())?;
        self.consume(written);
        Ok(written)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_buffer() {
        let mut buf = StreamBuffer::new();
        assert!(buf.is_empty());

        buf.extend_from_slice(&[1, 2, 3]);
        buf.extend_from_slice(&[4, 5, 6]);
        assert_eq!(buf.as_slice(), &[1, 2, 3, 4, 5, 6]);

        buf.consume(3);
        assert_eq!(buf.as_slice(), &[4, 5, 6]);

        buf.consume(3);
        assert!(buf.is_empty());
    }

    #[test]
    fn test_remaining() {
        let mut buf = StreamBuffer::with_capacity(100);
        assert_eq!(buf.remaining(), 100);
        buf.extend_from_slice(&[1; 50]);
        assert_eq!(buf.remaining(), 50);
    }
}
