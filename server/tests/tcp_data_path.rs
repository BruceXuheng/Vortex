//! TCP 数据通路端到端集成测试。

use std::io::{Read, Write};
use std::net::{TcpListener as StdTcpListener, TcpStream as StdTcpStream};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::time::Duration;

use mio::Events;
use mio::net::TcpStream;

use vortex::packet::checksum;
use vortex::packet::ipv4_header::Ipv4Header;
use vortex::relay::client::{Client, CloseListener};
use vortex::relay::selector::Selector;

/// 测试用 Client 关闭监听器（空操作）。
struct TestCloseListener;
impl CloseListener for TestCloseListener {
    fn on_closed(&mut self, _client_id: u32) {}
}

/// 构造 TCP SYN IP 包。
fn make_tcp_syn(src_ip: [u8; 4], src_port: u16, dst_ip: [u8; 4], dst_port: u16, seq: u32) -> Vec<u8> {
    let total_len = 40;
    let mut p = vec![0u8; total_len];
    p[0] = 0x45; p[2] = 0; p[3] = 40; p[8] = 64; p[9] = 6;
    p[12..16].copy_from_slice(&src_ip); p[16..20].copy_from_slice(&dst_ip);
    let ck = checksum::compute_ipv4_checksum(&p[..20]); p[10] = (ck>>8) as u8; p[11] = ck as u8;
    let t = 20;
    p[t] = (src_port>>8) as u8; p[t+1] = src_port as u8;
    p[t+2] = (dst_port>>8) as u8; p[t+3] = dst_port as u8;
    p[t+4..t+8].copy_from_slice(&seq.to_be_bytes());
    p[t+12] = 0x50; p[t+13] = 0x02; // SYN
    p[t+14] = 0xFF; p[t+15] = 0xFF;
    let tc = checksum::compute_transport_checksum(u32::from_be_bytes(src_ip), u32::from_be_bytes(dst_ip), 6, &p[t..]);
    p[t+16] = (tc>>8) as u8; p[t+17] = tc as u8;
    p
}

/// 构造 TCP ACK IP 包。
fn make_tcp_ack(src_ip: [u8; 4], src_port: u16, dst_ip: [u8; 4], dst_port: u16, seq: u32, ack: u32) -> Vec<u8> {
    let total_len = 40;
    let mut p = vec![0u8; total_len];
    p[0] = 0x45; p[2] = 0; p[3] = 40; p[8] = 64; p[9] = 6;
    p[12..16].copy_from_slice(&src_ip); p[16..20].copy_from_slice(&dst_ip);
    let ck = checksum::compute_ipv4_checksum(&p[..20]); p[10] = (ck>>8) as u8; p[11] = ck as u8;
    let t = 20;
    p[t] = (src_port>>8) as u8; p[t+1] = src_port as u8;
    p[t+2] = (dst_port>>8) as u8; p[t+3] = dst_port as u8;
    p[t+4..t+8].copy_from_slice(&seq.to_be_bytes());
    p[t+8..t+12].copy_from_slice(&ack.to_be_bytes());
    p[t+12] = 0x50; p[t+13] = 0x10; // ACK
    p[t+14] = 0xFF; p[t+15] = 0xFF;
    let tc = checksum::compute_transport_checksum(u32::from_be_bytes(src_ip), u32::from_be_bytes(dst_ip), 6, &p[t..]);
    p[t+16] = (tc>>8) as u8; p[t+17] = tc as u8;
    p
}

/// 构造 TCP PSH+ACK 数据 IP 包。
fn make_tcp_data(src_ip: [u8; 4], src_port: u16, dst_ip: [u8; 4], dst_port: u16, seq: u32, ack: u32, payload: &[u8]) -> Vec<u8> {
    let total_len = 40 + payload.len();
    let mut p = vec![0u8; total_len];
    p[0] = 0x45; p[2] = (total_len>>8) as u8; p[3] = total_len as u8; p[8] = 64; p[9] = 6;
    p[12..16].copy_from_slice(&src_ip); p[16..20].copy_from_slice(&dst_ip);
    let ck = checksum::compute_ipv4_checksum(&p[..20]); p[10] = (ck>>8) as u8; p[11] = ck as u8;
    let t = 20;
    p[t] = (src_port>>8) as u8; p[t+1] = src_port as u8;
    p[t+2] = (dst_port>>8) as u8; p[t+3] = dst_port as u8;
    p[t+4..t+8].copy_from_slice(&seq.to_be_bytes());
    p[t+8..t+12].copy_from_slice(&ack.to_be_bytes());
    p[t+12] = 0x50; p[t+13] = 0x18; // PSH+ACK
    p[t+14] = 0xFF; p[t+15] = 0xFF;
    p[40..].copy_from_slice(payload);
    let tc = checksum::compute_transport_checksum(u32::from_be_bytes(src_ip), u32::from_be_bytes(dst_ip), 6, &p[t..]);
    p[t+16] = (tc>>8) as u8; p[t+17] = tc as u8;
    p
}

/// 从 TCP 响应 IP 包中提取 payload。
fn extract_tcp_payload(data: &[u8]) -> Option<&[u8]> {
    if data.len() < 40 { return None; }
    let h = Ipv4Header::new(data);
    if h.version() != 4 || h.protocol() != 6 { return None; }
    let ihl = h.header_length_bytes();
    let data_offset = ((data[ihl + 12] >> 4) as usize) * 4;
    let start = ihl + data_offset;
    if data.len() < start { return None; }
    Some(&data[start..])
}

#[test]
fn test_tcp_data_path_end_to_end() {
    // 1. 启动 TCP echo 服务器（独立线程）
    let echo_listener = StdTcpListener::bind("127.0.0.1:0").unwrap();
    let echo_port = echo_listener.local_addr().unwrap().port();
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();

    let echo_thread = std::thread::spawn(move || {
        while running_clone.load(Ordering::Relaxed) {
            echo_listener.set_nonblocking(true).unwrap();
            if let Ok((mut conn, _)) = echo_listener.accept() {
                conn.set_nonblocking(false).unwrap();
                conn.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
                let mut buf = [0u8; 65535];
                loop {
                    if !running_clone.load(Ordering::Relaxed) {
                        break;
                    }
                    match conn.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => { let _ = conn.write_all(&buf[..n]); }
                        Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                        Err(_) => break,
                    }
                }
            } else {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    });

    // 2. 创建 Client
    let pair = StdTcpListener::bind("127.0.0.1:0").unwrap();
    let pair_addr = pair.local_addr().unwrap();

    let mut selector = Selector::new().expect("create selector");
    let mut events = Events::with_capacity(1024);

    let mut client_side = StdTcpStream::connect(pair_addr).unwrap();
    client_side.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    client_side.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
    client_side.set_nodelay(true).unwrap();

    let (server_raw, _) = pair.accept().unwrap();
    server_raw.set_nodelay(true).unwrap();
    server_raw.set_nonblocking(true).unwrap();
    let server_side = TcpStream::from_std(server_raw);

    let _client = Client::create(1, &mut selector, server_side, Box::new(TestCloseListener))
        .expect("create client");

    // 驱动 client_id 发送
    selector.poll(&mut events, Some(Duration::from_millis(100))).unwrap();
    selector.run_handlers(&mut events);
    let mut id_buf = [0u8; 4];
    client_side.read_exact(&mut id_buf).expect("read client_id");

    // 3. 发送 SYN
    let src_ip = [10, 0, 0, 2];
    let dst_ip = [127, 0, 0, 1];
    let client_seq: u32 = 1000;
    let syn = make_tcp_syn(src_ip, 54321, dst_ip, echo_port, client_seq);
    client_side.write_all(&syn).expect("send SYN");

    // 4. 驱动事件循环直到收到 SYN+ACK
    client_side.set_nonblocking(true).unwrap();

    let mut server_isn: u32 = 0;
    let mut syn_ack_received = false;
    let mut all_read_data = Vec::new();

    for _ in 0..100 {
        selector.poll(&mut events, Some(Duration::from_millis(10))).unwrap();
        selector.run_handlers(&mut events);

        // 每轮尝试读取
        let mut tmp = [0u8; 65535];
        loop {
            match client_side.read(&mut tmp) {
                Ok(0) => break,
                Ok(n) => all_read_data.extend_from_slice(&tmp[..n]),
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }

        // 检查是否收到了 SYN+ACK
        let mut offset = 0;
        while offset + 40 <= all_read_data.len() {
            let hdr = Ipv4Header::new(&all_read_data[offset..]);
            let total_len = hdr.total_length() as usize;
            if total_len < 20 || offset + total_len > all_read_data.len() { break; }

            let flags = all_read_data[offset + 33];
            if flags & 0x12 == 0x12 {
                server_isn = u32::from_be_bytes([
                    all_read_data[offset + 24], all_read_data[offset + 25],
                    all_read_data[offset + 26], all_read_data[offset + 27]
                ]);
                syn_ack_received = true;
                break;
            }
            offset += total_len;
        }

        if syn_ack_received { break; }
    }

    client_side.set_nonblocking(false).unwrap();
    assert!(syn_ack_received, "未收到 SYN+ACK");

    // 5. 发送 ACK 完成握手
    let ack = make_tcp_ack(src_ip, 54321, dst_ip, echo_port, client_seq + 1, server_isn + 1);
    client_side.write_all(&ack).expect("send ACK");

    // 6. 发送数据
    let payload = b"TCP_VORTEX_TEST";
    let data = make_tcp_data(src_ip, 54321, dst_ip, echo_port, client_seq + 1, server_isn + 1, payload);
    client_side.write_all(&data).expect("send data");

    // 7. 驱动事件循环直到收到回传数据
    client_side.set_nonblocking(true).unwrap();

    let mut echo_received = false;

    for _ in 0..100 {
        selector.poll(&mut events, Some(Duration::from_millis(10))).unwrap();
        selector.run_handlers(&mut events);

        // 每轮尝试读取回传数据
        let mut tmp = [0u8; 65535];
        let mut resp_data = Vec::new();
        loop {
            match client_side.read(&mut tmp) {
                Ok(0) => break,
                Ok(n) => resp_data.extend_from_slice(&tmp[..n]),
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }

        // 从回传数据中提取所有 IP 包，找到有 payload 的 TCP 包
        let mut offset = 0;
        while offset + 20 <= resp_data.len() {
            let hdr = Ipv4Header::new(&resp_data[offset..]);
            let total_len = hdr.total_length() as usize;
            if total_len < 20 || offset + total_len > resp_data.len() { break; }

            let packet = &resp_data[offset..offset + total_len];
            if let Some(p) = extract_tcp_payload(packet) {
                if p.len() == payload.len() && p == payload {
                    echo_received = true;
                    break;
                }
            }
            offset += total_len;
        }

        if echo_received { break; }
    }

    client_side.set_nonblocking(false).unwrap();

    running.store(false, Ordering::Relaxed);
    let _ = echo_thread.join();
    assert!(echo_received, "未收到 TCP echo 响应");
}
