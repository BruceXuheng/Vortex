/// IP 头校验和计算。
///
/// RFC 1071 规定的反码求和算法：
/// 1. 将头部视为 16 位整数序列
/// 2. 求和（带进位回卷）
/// 3. 取反码即为校验和
///
/// 验证时，将校验和字段包含在内再求和，结果应为 0xFFFF。
pub fn compute_ipv4_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;

    // 按 16 位字求和
    let len = data.len();
    for i in (0..len).step_by(2) {
        if i + 1 < len {
            sum += u32::from(((data[i] as u16) << 8) | data[i + 1] as u16);
        } else {
            // 奇数长度：最后一个字节左移 8 位
            sum += u32::from(data[i] as u16) << 8;
        }
    }

    // 进位回卷：将高 16 位的进位加回低 16 位
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }

    !(sum as u16)
}

/// TCP/UDP 伪首部校验和计算。
///
/// TCP 和 UDP 的校验和不仅覆盖自身头部和数据，还包含一个"伪首部"，
/// 包含源 IP、目的 IP、协议号和 TCP/UDP 长度。
///
/// 伪首部格式（12 字节）：
/// ```text
/// [源 IP (4 bytes)] [目的 IP (4 bytes)] [0x00] [协议] [TCP/UDP 长度 (2 bytes)]
/// ```
pub fn compute_transport_checksum(
    source_ip: u32,
    dest_ip: u32,
    protocol: u8,
    transport_data: &[u8],
) -> u16 {
    // 构造伪首部
    let length = transport_data.len() as u16;
    let pseudo_header = [
        (source_ip >> 24) as u8,
        (source_ip >> 16) as u8,
        (source_ip >> 8) as u8,
        source_ip as u8,
        (dest_ip >> 24) as u8,
        (dest_ip >> 16) as u8,
        (dest_ip >> 8) as u8,
        dest_ip as u8,
        0,
        protocol,
        (length >> 8) as u8,
        length as u8,
    ];

    // 对伪首部求和
    let mut sum: u32 = 0;
    for i in (0..pseudo_header.len()).step_by(2) {
        sum += u32::from(((pseudo_header[i] as u16) << 8) | pseudo_header[i + 1] as u16);
    }

    // 对传输层数据求和
    for i in (0..transport_data.len()).step_by(2) {
        if i + 1 < transport_data.len() {
            sum += u32::from(((transport_data[i] as u16) << 8) | transport_data[i + 1] as u16);
        } else {
            sum += u32::from(transport_data[i] as u16) << 8;
        }
    }

    // 进位回卷
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }

    !(sum as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipv4_checksum_valid() {
        // 构造一个简单的 IPv4 头（20 字节，校验和字段为 0）
        let mut header = [0u8; 20];
        header[0] = 0x45; // version=4, IHL=5
        header[9] = 0x06; // protocol=TCP
        // 设置源 IP 和目的 IP
        header[12] = 192; header[13] = 168; header[14] = 1; header[15] = 1;
        header[16] = 10; header[17] = 0; header[18] = 0; header[19] = 2;

        // 计算校验和
        let checksum = compute_ipv4_checksum(&header);
        // 写入校验和
        header[10] = (checksum >> 8) as u8;
        header[11] = checksum as u8;

        // 验证：包含校验和的头部再算一次，结果应为 0
        let verify = compute_ipv4_checksum(&header);
        assert_eq!(verify, 0, "校验和验证失败，应为 0");
    }

    #[test]
    fn test_transport_checksum() {
        // 简单的伪首部校验和测试
        let source_ip: u32 = 0xC0A80101; // 192.168.1.1
        let dest_ip: u32 = 0x0A000002;   // 10.0.0.2
        let data = [0u8; 20]; // 空的 TCP 头
        let checksum = compute_transport_checksum(source_ip, dest_ip, 6, &data);
        // 校验和不应为 0（除非数据恰好使反码求和为 0xFFFF）
        assert_ne!(checksum, 0xFFFF, "校验和不应为 0xFFFF");
    }
}
