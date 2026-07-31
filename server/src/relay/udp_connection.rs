use crate::packet::ipv4_packet::Ipv4Packet;
use crate::relay::client::ClientChannel;
use crate::relay::connection::{Connection, ConnectionId};
use crate::relay::packetizer::Packetizer;
use crate::relay::selector::Selector;
use mio::net::UdpSocket;
use mio::{Interest, Token};
use std::cell::RefCell;
use std::io;
use std::rc::{Rc, Weak};
use std::time::Instant;

/// UDP 连接空闲超时时间（秒）。
const UDP_IDLE_TIMEOUT_SECS: u64 = 120;

/// UDP 连接。
///
/// 对齐 Gnirehtet 设计：
/// - 持有 `Weak<RefCell<Client>>`，通过 ClientChannel 回传数据
/// - 实现 `Connection` trait
/// - 通过 EventHandler 闭包调用 on_ready
pub struct UdpConnection {
    /// 连接五元组标识。
    id: ConnectionId,
    /// 与真实目标通信的 UDP socket。
    socket: UdpSocket,
    /// UDP 只需 READABLE，interest 不会动态变化。
    _interests: Interest,
    /// Selector Token。
    token: Token,
    /// 用于构造回传 IP 包的参考包（Android 发来的原始包，每次收到新包时更新）。
    reference_packet: Vec<u8>,
    /// IP 包构造器（交换 src/dst、计算校验和）。
    packetizer: Packetizer,
    /// 空闲计时器（超时后自动关闭连接）。
    idle_since: Instant,
    /// 所属 Client 的弱引用（用于 on_ready 路径中回传数据）。
    client: Weak<RefCell<crate::relay::client::Client>>,
    /// 连接是否已关闭。
    closed: bool,
}

impl UdpConnection {
    /// 创建新的 UDP 连接。
    pub fn create(
        selector: &mut Selector,
        id: ConnectionId,
        client: Weak<RefCell<crate::relay::client::Client>>,
        ipv4_packet: &[u8],
    ) -> io::Result<Rc<RefCell<Self>>> {
        // 绑定任意端口
        let bind_addr: std::net::SocketAddr = "0.0.0.0:0".parse().unwrap();
        let socket = UdpSocket::bind(bind_addr)?;

        // connect 到真实目标
        let dest_addr: std::net::SocketAddr = id.rewritten_destination().into();
        socket.connect(dest_addr)?;

        let reference_packet = ipv4_packet.to_vec();
        let mut packetizer = Packetizer::new();
        packetizer.set_reference_packet(&reference_packet);

        let interests = Interest::READABLE;

        let rc = Rc::new(RefCell::new(Self {
            id,
            socket,
            _interests: interests,
            token: Token(0),
            reference_packet,
            packetizer,
            idle_since: Instant::now(),
            client,
            closed: false,
        }));

        {
            let mut self_ref = rc.borrow_mut();
            let rc2 = rc.clone();
            let handler = move |selector: &mut Selector, event: &mio::event::Event| {
                rc2.borrow_mut().on_ready(selector, event);
            };
            let token = selector.register(&mut self_ref.socket, interests, handler)?;
            self_ref.token = token;
        }

        log::debug!("UDP 连接创建: {}, Token={:?}", rc.borrow().id.display(), rc.borrow().token);
        Ok(rc)
    }

    /// 事件就绪回调。
    fn on_ready(&mut self, selector: &mut Selector, event: &mio::event::Event) {
        match self.process(selector, event) {
            Ok(()) => {}
            Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                log::trace!("{} UDP 虚假事件", self.id.display());
            }
            Err(_) => panic!("未处理的意外 UDP 错误"),
        }
    }

    /// 处理事件。
    fn process(&mut self, selector: &mut Selector, event: &mio::event::Event) -> io::Result<()> {
        if !self.closed && event.is_readable() {
            self.process_receive(selector)?;
        }
        Ok(())
    }

    /// 处理接收（从真实 UDP socket 读取数据，构造 IP 包回传）。
    ///
    /// on_ready 路径：Client 的 RefCell 未被 borrow，可以安全 borrow。
    fn process_receive(&mut self, selector: &mut Selector) -> io::Result<()> {
        let mut buf = [0u8; 65535];
        match self.socket.recv(&mut buf) {
            Ok(n) => {
                log::trace!("UDP 收到 {} 字节来自 {}", n, self.id.display());

                // 构造回传 UDP 包
                let packet = self.packetizer.create_udp_packet(
                    &self.reference_packet,
                    &buf[..n],
                );

                // on_ready 路径，可以安全 borrow Client
                let client_rc = self.client.upgrade().expect("Client 弱引用不应为空");
                let mut client = client_rc.borrow_mut();
                let mut client_channel = client.channel();
                if let Err(err) = client_channel.send_to_client(selector, packet) {
                    log::warn!("UDP 无法发送包给 Client: {}", err);
                }

                self.idle_since = Instant::now();
            }
            Err(err) => {
                if err.kind() == io::ErrorKind::WouldBlock {
                    return Err(err);
                }
                log::warn!("UDP 接收失败: {err}");
                self.close(selector);
            }
        }
        Ok(())
    }

    /// 处理从 Android 收到的 UDP 数据。
    fn handle_packet(&mut self, ipv4_packet: &[u8]) {
        let packet = Ipv4Packet::new(ipv4_packet);
        let payload = packet.transport_payload();

        if !payload.is_empty() {
            match self.socket.send(payload) {
                Ok(n) => {
                    log::trace!("UDP 发送 {} 字节到 {}", n, self.id.display());
                }
                Err(e) => {
                    log::warn!("UDP 发送失败: {e}");
                }
            }
        }

        // 更新参考包
        self.reference_packet = ipv4_packet.to_vec();
        self.packetizer.set_reference_packet(&self.reference_packet);
        self.idle_since = Instant::now();
    }

    /// 检查是否已超时。
    pub fn is_expired(&self) -> bool {
        self.idle_since.elapsed().as_secs() > UDP_IDLE_TIMEOUT_SECS
    }
}

impl Connection for UdpConnection {
    fn id(&self) -> ConnectionId {
        self.id.clone()
    }

    /// 对齐 Gnirehtet：接受 ClientChannel 参数。
    ///
    /// UDP 的 send_to_network 不需要 ClientChannel（只发送数据到真实 socket，
    /// 不回传控制包），但需要保持与 Connection trait 签名一致。
    fn send_to_network(
        &mut self,
        _selector: &mut Selector,
        _client_channel: &mut ClientChannel,
        ipv4_packet: &[u8],
    ) {
        self.handle_packet(ipv4_packet);
    }

    fn close(&mut self, selector: &mut Selector) {
        log::debug!("UDP 连接关闭: {}", self.id.display());
        self.closed = true;
        let _ = selector.deregister(&mut self.socket, self.token);
    }

    fn is_expired(&self) -> bool {
        self.idle_since.elapsed().as_secs() > UDP_IDLE_TIMEOUT_SECS
    }

    fn is_closed(&self) -> bool {
        self.closed
    }
}
