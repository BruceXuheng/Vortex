use crate::packet::ipv4_packet::Ipv4Packet;
use crate::relay::client::Client;
use crate::relay::connection::{Connection, ConnectionId};
use crate::relay::selector::Selector;
use std::cell::RefCell;
use std::io;
use std::rc::{Rc, Weak};

/// 五元组路由器。
///
/// 对齐 Gnirehtet 设计：
/// - 持有 `Weak<RefCell<Client>>`，不持有 CloseListener
/// - Connection 关闭时通过 client 弱引用自行从 router 移除
/// - `send_to_network()` 不传 ClientChannel——Connection 只解析包和更新状态，
///   回传数据通过 on_ready 路径完成
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
    /// 如果没有找到已有连接，则创建新连接。
    /// 注意：不传 ClientChannel——Connection 只解析包和更新状态，
    /// 回传数据通过 on_ready 事件路径完成（Connection 收到 READABLE 事件后
    /// 读取网络数据，构造 IP 包，通过 ClientChannel 回传）。
    pub fn send_to_network(
        &mut self,
        selector: &mut Selector,
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
                    connection.send_to_network(selector, ipv4_packet);
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
    fn is_valid(packet: &Ipv4Packet) -> bool {
        let header = packet.ipv4_header();
        header.version() == 4 && header.total_length() as usize <= packet.raw().len()
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

    /// 移除指定连接（由 Connection 关闭时调用）。
    pub fn remove(&mut self, connection: &dyn Connection) {
        let id = connection.id();
        if let Some(index) = self.connections.iter().position(|c| c.borrow().id() == id) {
            log::debug!("连接自行从路由器移除: {}", id.display());
            self.connections.swap_remove(index);
        }
    }

    /// 清理所有连接。
    pub fn clear(&mut self, selector: &mut Selector) {
        for connection in &mut self.connections {
            connection.borrow_mut().close(selector);
        }
        self.connections.clear();
    }

    /// 清理过期连接。
    pub fn clean_expired_connections(&mut self, selector: &mut Selector) {
        for i in (0..self.connections.len()).rev() {
            let expired = {
                let mut connection = self.connections[i].borrow_mut();
                if connection.is_expired() {
                    log::debug!("从路由器移除过期连接: {}", connection.id().display());
                    connection.close(selector);
                    true
                } else {
                    false
                }
            };
            if expired {
                self.connections.swap_remove(i);
            }
        }
    }
}
