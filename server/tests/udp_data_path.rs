//! UDP 数据通路端到端集成测试。

use std::io::{Read, Write};
use std::net::UdpSocket as StdUdpSocket;
use std::time::Duration;

use mio::Events;
use mio::net::TcpStream;

use vortex::packet::ipv4_header::Ipv4Header;
use vortex::relay::client::{Client, CloseListener};
use vortex::relay::selector::Selector;

/// 测试用 Client 关闭监听器（空操作）。
struct TestCloseListener;
impl CloseListener for TestCloseListener {
    fn on_closed(&mut self, _client_id: u32) {}
}

/// 构造一个伪造的 UDP IP 包。
fn make_udp_ip_packet(src_ip: [u8; 4], src_port: u16, dst_ip: [u8; 4], dst_port: u16, payload: &[u8]) -> Vec<u8> {
    let ipv4_header_len = 20;
    let udp_header_len = 8;
    let total_len = ipv4_header_len + udp_header_len + payload.len();

    let mut packet = vec![0u8; total_len];

    // IPv4 头
    packet[0] = 0x45;
    packet[1] = 0x00;
    packet[2] = (total_len >> 8) as u8;
    packet[3] = total_len as u8;
    packet[4] = 0x00; packet[5] = 0x01;
    packet[6] = 0x00; packet[7] = 0x00;
    packet[8] = 64;
    packet[9] = 17; // UDP
    packet[10] = 0; packet[11] = 0;

    packet[12..16].copy_from_slice(&src_ip);
    packet[16..20].copy_from_slice(&dst_ip);

    let ipv4_checksum = vortex::packet::checksum::compute_ipv4_checksum(&packet[..ipv4_header_len]);
    packet[10] = (ipv4_checksum >> 8) as u8;
    packet[11] = ipv4_checksum as u8;

    let udp_offset = ipv4_header_len;
    packet[udp_offset] = (src_port >> 8) as u8;
    packet[udp_offset + 1] = src_port as u8;
    packet[udp_offset + 2] = (dst_port >> 8) as u8;
    packet[udp_offset + 3] = dst_port as u8;
    let udp_len = udp_header_len + payload.len();
    packet[udp_offset + 4] = (udp_len >> 8) as u8;
    packet[udp_offset + 5] = udp_len as u8;
    packet[udp_offset + 6] = 0;
    packet[udp_offset + 7] = 0;

    packet[udp_offset + udp_header_len..].copy_from_slice(payload);

    packet
}

/// 验证回传 IP 包的 src/dst IP 已交换，且 payload 匹配。
fn verify_return_packet(data: &[u8], expected_src_ip: [u8; 4], expected_dst_ip: [u8; 4], expected_payload: &[u8]) -> bool {
    if data.len() < 28 {
        return false;
    }

    let ipv4_header = Ipv4Header::new(data);
    if ipv4_header.version() != 4 || ipv4_header.protocol() != 17 {
        return false;
    }

    if data[12..16] != expected_src_ip || data[16..20] != expected_dst_ip {
        return false;
    }

    let ipv4_header_len = ipv4_header.header_length_bytes();
    let payload_start = ipv4_header_len + 8;
    if data.len() < payload_start + expected_payload.len() {
        return false;
    }
    &data[payload_start..payload_start + expected_payload.len()] == expected_payload
}

#[test]
fn test_udp_data_path_end_to_end() {
    // 1. 启动 UDP echo 服务器
    let echo_socket = StdUdpSocket::bind("127.0.0.1:0").expect("bind echo socket");
    let echo_addr = echo_socket.local_addr().unwrap();
    let echo_port = echo_addr.port();

    // 2. 创建一对 TCP socket 模拟 ADB 隧道
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let listener_addr = listener.local_addr().unwrap();

    let mut client_side = std::net::TcpStream::connect(listener_addr).unwrap();
    client_side.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    client_side.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
    client_side.set_nodelay(true).unwrap();

    let (server_side_raw, _) = listener.accept().unwrap();
    server_side_raw.set_nodelay(true).unwrap();
    server_side_raw.set_nonblocking(true).unwrap();
    let server_side = TcpStream::from_std(server_side_raw);

    // 3. 创建 Selector 和 Client
    let mut selector = Selector::new().expect("create selector");
    let mut events = Events::with_capacity(1024);

    let _client = Client::create(42, &mut selector, server_side, Box::new(TestCloseListener))
        .expect("create client");

    // 驱动 client_id 发送
    selector.poll(&mut events, Some(Duration::from_millis(100))).unwrap();
    selector.run_handlers(&mut events);

    let mut id_buf = [0u8; 4];
    client_side.read_exact(&mut id_buf).expect("read client_id");
    assert_eq!(u32::from_be_bytes(id_buf), 42, "client_id 应该是 42");

    // 4. 构造 UDP IP 包
    let test_payload = b"HELLO_VORTEX";
    let src_ip = [10, 0, 0, 2];
    let dst_ip = [127, 0, 0, 1];
    let ip_packet = make_udp_ip_packet(src_ip, 12345, dst_ip, echo_port, test_payload);

    // 5. 发送 IP 包
    client_side.write_all(&ip_packet).expect("send IP packet");

    // 6. 驱动事件循环
    for _ in 0..15 {
        selector.poll(&mut events, Some(Duration::from_millis(100))).unwrap();
        selector.run_handlers(&mut events);

        let mut echo_buf = [0u8; 65535];
        echo_socket.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
        if let Ok((n, src)) = echo_socket.recv_from(&mut echo_buf) {
            echo_socket.send_to(&echo_buf[..n], src).unwrap();
            break;
        }
    }

    for _ in 0..15 {
        selector.poll(&mut events, Some(Duration::from_millis(100))).unwrap();
        selector.run_handlers(&mut events);
    }

    // 7. 读取回传数据
    let mut response_buf = [0u8; 65535];
    let n = client_side.read(&mut response_buf).expect("读取回传数据");
    assert!(n > 0, "连接已关闭，未收到回传数据");

    // 8. 验证
    assert!(
        verify_return_packet(&response_buf[..n], dst_ip, src_ip, test_payload),
        "回传 IP 包验证失败: {:?}",
        &response_buf[..n.min(64)]
    );

    println!("✅ UDP 端到端测试通过！payload: {} 字节正确回传", test_payload.len());
}
