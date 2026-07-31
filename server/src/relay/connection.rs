use crate::packet::ipv4_header::Ipv4Header;
use crate::packet::transport_header::{PROTOCOL_TCP, PROTOCOL_UDP, TransportHeader};
use crate::relay::client::ClientChannel;
use crate::relay::selector::Selector;
use std::fmt;
use std::net::Ipv4Addr;

/// 协议类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Protocol {
    Tcp,
    Udp,
}

/// 连接标识——五元组。
///
/// 唯一标识一个网络连接：(协议, 源IP, 源端口, 目的IP, 目的端口)
/// 用于 Router 将收到的 IP 包路由到正确的 Connection。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ConnectionId {
    protocol: Protocol,
    source_ip: u32,
    source_port: u16,
    destination_ip: u32,
    destination_port: u16,
    id_string: String,
}

/// 10.0.2.2 — Android 模拟器中表示宿主机的特殊地址。
const LOCALHOST_FORWARD: u32 = 0x0A_00_02_02;
/// 127.0.0.1。
const LOCALHOST: u32 = 0x7F_00_00_01;

impl ConnectionId {
    /// 从 IP 头和传输层头构造五元组。
    pub fn from_headers(
        ipv4_header: &Ipv4Header,
        transport_header: &TransportHeader,
    ) -> Self {
        let source_ip = ipv4_header.source_u32();
        let source_port = transport_header.source_port();
        let destination_ip = ipv4_header.destination_u32();
        let destination_port = transport_header.dest_port();
        let protocol = match ipv4_header.protocol() {
            PROTOCOL_TCP => Protocol::Tcp,
            PROTOCOL_UDP => Protocol::Udp,
            _ => panic!("未知协议"),
        };
        let src = Ipv4Addr::from(source_ip);
        let dst = Ipv4Addr::from(destination_ip);
        let id_string = format!(
            "{} -> {}",
            Self::format_socket_addr(src, source_port),
            Self::format_socket_addr(dst, destination_port)
        );
        Self {
            protocol,
            source_ip,
            source_port,
            destination_ip,
            destination_port,
            id_string,
        }
    }

    /// 从 IP 协议号解析协议类型。
    pub fn protocol_from_number(num: u8) -> Option<Protocol> {
        match num {
            PROTOCOL_TCP => Some(Protocol::Tcp),
            PROTOCOL_UDP => Some(Protocol::Udp),
            _ => None,
        }
    }

    /// 获取协议类型。
    pub fn protocol(&self) -> Protocol {
        self.protocol
    }

    /// 获取源 IP。
    pub fn source_ip(&self) -> u32 {
        self.source_ip
    }

    /// 获取源端口。
    pub fn source_port(&self) -> u16 {
        self.source_port
    }

    /// 获取目的 IP。
    pub fn destination_ip(&self) -> u32 {
        self.destination_ip
    }

    /// 获取目的端口。
    pub fn destination_port(&self) -> u16 {
        self.destination_port
    }

    /// 获取重写后的目标地址（将 10.0.2.2 转换为 127.0.0.1）。
    pub fn rewritten_destination(&self) -> std::net::SocketAddrV4 {
        let ip = if self.destination_ip == LOCALHOST_FORWARD {
            LOCALHOST
        } else {
            self.destination_ip
        };
        std::net::SocketAddrV4::new(std::net::Ipv4Addr::from(ip), self.destination_port)
    }

    /// 格式化显示。
    pub fn display(&self) -> &str {
        &self.id_string
    }

    fn format_socket_addr(ip: Ipv4Addr, port: u16) -> String {
        format!("{}:{}", ip, port)
    }
}

impl fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.id_string)
    }
}

/// 连接 trait——TCP 和 UDP 连接的统一接口。
///
/// 对齐 Gnirehtet 设计：
/// - `send_to_network()` 接受 `&mut ClientChannel`——Connection 可以直接通过
///   ClientChannel 回传数据给 Android（SYN+ACK、FIN+ACK、RST 等控制包）
/// - ClientChannel 在 Client 的 `push_one_packet_to_network` 中提前创建，
///   避免 Connection 二次 borrow Client 的 RefCell 导致 panic
pub trait Connection {
    /// 获取连接 ID。
    fn id(&self) -> ConnectionId;

    /// 将 IP 包数据发送到真实网络。
    ///
    /// 对于 TCP：解析包内容（SYN/ACK/FIN/数据），更新 TCB 状态，
    /// 将 payload 写入 client_to_network 缓冲区。
    /// 对于 UDP：直接将 payload 发送到真实 UDP socket。
    ///
    /// 通过 ClientChannel 可以立即回传控制包（SYN+ACK、FIN+ACK、RST）给 Android。
    fn send_to_network(
        &mut self,
        selector: &mut Selector,
        client_channel: &mut ClientChannel,
        ipv4_packet: &[u8],
    );

    /// 关闭连接。
    fn close(&mut self, selector: &mut Selector);

    /// 连接是否过期。
    fn is_expired(&self) -> bool;

    /// 连接是否已关闭。
    fn is_closed(&self) -> bool;
}
