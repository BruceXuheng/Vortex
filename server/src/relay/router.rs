use crate::packet::ipv4_packet::Ipv4Packet;
use crate::relay::client::{Client, ClientChannel};
use crate::relay::connection::{Connection, ConnectionId};
use crate::relay::selector::Selector;
use std::cell::RefCell;
use std::io;
use std::rc::{Rc, Weak};

/// 五元组路由器。
///
/// 对齐 Gnirehtet 设计：
/// - 持有 `Weak<RefCell<Client>>`
/// - 已关闭的连接在 `send_to_network`（is_closed 检查）和 `clean_expired_connections` 中移除
/// - `send_to_network()` 传入 ClientChannel——Connection 可直接回传控制包
pub struct Router {
    client: Weak<RefCell<Client>>,
    /// 连接列表（通常每个客户端只有少量连接，Vec 比 HashMap 更高效）。
    connections: Vec<Rc<RefCell<dyn Connection>>>,
}

impl Router {
    /// 创建空路由器。
    pub fn new() -> Self {
        Self {
            client: Weak::new(),
            connections: Vec::new(),
        }
    }

    /// 设置 Client 弱引用（打破循环初始化依赖）。
    pub fn set_client(&mut self, client: Weak<RefCell<Client>>) {
        self.client = client;
    }

    /// 将 IP 包路由到对应的连接。
    ///
    /// 对齐 Gnirehtet：传入 ClientChannel，让 Connection 可直接回传控制包。
    /// 如果没有找到已有连接，则创建新连接。
    pub fn send_to_network(
        &mut self,
        selector: &mut Selector,
        client_channel: &mut ClientChannel,
        ipv4_packet: &[u8],
    ) {
        let packet = Ipv4Packet::new(ipv4_packet);
        if !Self::is_valid(&packet) {
            log::warn!("丢弃无效 IP 包");
            return;
        }

        match self.connection(selector, &packet) {
            Ok(index) => {
                let closed = {
                    let connection_ref = &self.connections[index];
                    let mut connection = connection_ref.borrow_mut();
                    connection.send_to_network(selector, client_channel, ipv4_packet);
                    connection.is_closed()
                };
                if closed {
                    log::debug!("从路由器移除已关闭的连接");
                    self.connections.swap_remove(index);
                }
            }
            Err(err) => log::error!("无法创建路由，丢弃包: {}", err),
        }
    }

    /// 验证 IP 包是否有效。
    ///
    /// 对齐 Gnirehtet：除了检查 IP 版本和长度，还必须能解析传输层头。
    /// 没有传输层头的包（如 ICMP、分片包）无法路由，应直接丢弃。
    fn is_valid(packet: &Ipv4Packet) -> bool {
        let header = packet.ipv4_header();
        header.version() == 4
            && header.total_length() as usize <= packet.raw().len()
            && packet.transport_header().is_some()
    }

    /// 查找或创建连接。
    fn connection(
        &mut self,
        selector: &mut Selector,
        ipv4_packet: &Ipv4Packet,
    ) -> io::Result<usize> {
        let id = Self::connection_id(ipv4_packet);
        match self.find_index(&id) {
            Some(index) => Ok(index),
            None => {
                let connection = Self::create_connection(selector, id, self.client.clone(), ipv4_packet)?;
                let index = self.connections.len();
                self.connections.push(connection);
                Ok(index)
            }
        }
    }

    /// 从 IP 包提取连接 ID。
    fn connection_id(ipv4_packet: &Ipv4Packet) -> ConnectionId {
        let ipv4_header = ipv4_packet.ipv4_header();
        let transport_header = ipv4_packet.transport_header().expect("缺少传输层头");
        ConnectionId::from_headers(&ipv4_header, &transport_header)
    }

    /// 创建新连接（TCP 或 UDP）。
    fn create_connection(
        selector: &mut Selector,
        id: ConnectionId,
        client: Weak<RefCell<Client>>,
        ipv4_packet: &Ipv4Packet,
    ) -> io::Result<Rc<RefCell<dyn Connection>>> {
        match id.protocol() {
            crate::relay::connection::Protocol::Tcp => {
                crate::relay::tcp_connection::TcpConnection::create(
                    selector, id, client, ipv4_packet.raw(),
                )
                    .map(|c| c as Rc<RefCell<dyn Connection>>)
            }
            crate::relay::connection::Protocol::Udp => {
                crate::relay::udp_connection::UdpConnection::create(
                    selector, id, client, ipv4_packet.raw(),
                )
                    .map(|c| c as Rc<RefCell<dyn Connection>>)
            }
        }
    }

    /// 查找连接在 Vec 中的索引。
    fn find_index(&self, id: &ConnectionId) -> Option<usize> {
        self.connections
            .iter()
            .position(|conn| &conn.borrow().id() == id)
    }

    /// 清理所有连接。
    pub fn clear(&mut self, selector: &mut Selector) {
        for connection in &mut self.connections {
            connection.borrow_mut().close(selector);
        }
        self.connections.clear();
    }

    /// 清理过期和已关闭的连接。
    pub fn clean_expired_connections(&mut self, selector: &mut Selector) {
        for i in (0..self.connections.len()).rev() {
            let should_remove = {
                let mut connection = self.connections[i].borrow_mut();
                if connection.is_closed() {
                    // 已关闭的连接直接移除（不需要再调用 close）
                    log::debug!("清理已关闭的连接: {}", connection.id().display());
                    true
                } else if connection.is_expired() {
                    log::debug!("从路由器移除过期连接: {}", connection.id().display());
                    connection.close(selector);
                    true
                } else {
                    false
                }
            };
            if should_remove {
                self.connections.swap_remove(i);
            }
        }
    }
}
