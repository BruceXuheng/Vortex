use crate::packet::ipv4_packet::MAX_PACKET_LENGTH;
use crate::packet::ipv4_packet_buffer::Ipv4PacketBuffer;
use crate::relay::packet_source::PacketSource;
use crate::relay::router::Router;
use crate::relay::selector::Selector;
use crate::relay::stream_buffer::StreamBuffer;
use mio::net::TcpStream;
use mio::{Interest, Token};
use std::cell::RefCell;
use std::io::{self, Write};
use std::mem;
use std::net::Shutdown;
use std::rc::Rc;

/// Client 与连接之间的数据通道。
///
/// 当连接需要回传数据给 Android 时，不能直接 borrow Client（因为
/// Router 已经借用了 Client）。ClientChannel 持有 Client 内部
/// buffer 和 interest 的可变引用，让连接可以安全地写入数据。
///
/// 注意：ClientChannel 不持有 stream 引用，不能直接调用 reregister。
/// 它通过修改 `interests` 字段标记需要更新，Client 在 process 结束后
/// 统一执行 reregister。
pub struct ClientChannel<'a> {
    network_to_client: &'a mut StreamBuffer,
    token: Token,
    interests: &'a mut Interest,
}

impl<'a> ClientChannel<'a> {
    fn new(
        network_to_client: &'a mut StreamBuffer,
        token: Token,
        interests: &'a mut Interest,
    ) -> Self {
        Self {
            network_to_client,
            token,
            interests,
        }
    }

    /// 将 IP 包数据写入回传缓冲区并标记 interest 更新。
    ///
    /// 返回 Err(WouldBlock) 表示 Client 缓冲区已满，需要背压处理。
    ///
    /// 注意：`_selector` 参数未使用，保留是为了与 Gnirehtet API 签名对齐。
    pub fn send_to_client(
        &mut self,
        _selector: &mut Selector,
        ipv4_packet: &[u8],
    ) -> io::Result<()> {
        if ipv4_packet.len() <= self.network_to_client.remaining() {
            self.network_to_client.read_from(ipv4_packet);
            self.mark_interests_update();
            Ok(())
        } else {
            log::warn!("Client 缓冲区已满");
            Err(io::Error::new(io::ErrorKind::WouldBlock, "Client buffer full"))
        }
    }

    /// 标记 interest 需要更新（脏标记机制）。
    ///
    /// 将 interests 强制设为 READABLE，制造一个与实际需求不匹配的值。
    /// 下次 update_interests() 计算出正确值（buffer 非空时为 READABLE|WRITABLE）
    /// 时，会发现两者不同，从而触发 reregister。
    ///
    /// 本质上这是一个"脏标记"：告诉 Client "interest 可能过时了，请重新计算"。
    fn mark_interests_update(&mut self) {
        *self.interests = Interest::READABLE;
    }

    /// 获取 token。
    pub fn token(&self) -> Token {
        self.token
    }
}

/// 与 Android 设备的客户端连接。
///
/// 每当 TunnelServer accept 一个新连接，就创建一个 Client。
/// Client 负责：
/// - 从 TCP 流中读取原始 IP 包，交给 Router 路由
/// - 将 Router/Connection 构造的回传 IP 包写入 TCP 流发回 Android
///
/// 数据流：
/// ```text
/// Android → ADB 隧道 → Client.read() → Ipv4PacketBuffer → Router → Connection
/// Connection → Packetizer → Client.send_to_client() → StreamBuffer → Client.write() → ADB 隧道 → Android
/// ```
pub struct Client {
    /// 客户端 ID。
    id: u32,
    /// 与 Android 的 TCP 连接。
    stream: TcpStream,
    /// 当前注册的 interest（跟踪状态以避免不必要的 reregister）。
    interests: Interest,
    /// Selector Token。
    token: Token,
    /// 从设备读入的 IP 包缓冲区。
    client_to_network: Ipv4PacketBuffer,
    /// 待发送到设备的 IP 包缓冲区。
    network_to_client: StreamBuffer,
    /// 五元组路由器。
    router: Router,
    /// 连接是否已关闭。
    closed: bool,
    /// 背压：待处理的包源列表。
    pending_packet_sources: Vec<Rc<RefCell<dyn PacketSource>>>,
    /// 初始阶段：还需要发送 client_id 的剩余字节数。
    pending_id_bytes: usize,
    /// 关闭回调（TunnelServer 用于从列表中移除 Client）。
    close_listener: Box<dyn CloseListener>,
}

/// Client 关闭回调 trait。
pub trait CloseListener {
    fn on_closed(&mut self, client_id: u32);
}

impl Client {
    /// 创建新的客户端连接。
    pub fn create(
        id: u32,
        selector: &mut Selector,
        stream: TcpStream,
        close_listener: Box<dyn CloseListener>,
    ) -> io::Result<Rc<RefCell<Self>>> {
        // 初始只关心 WRITABLE（必须先发送 client_id）
        let interests = Interest::WRITABLE;
        let rc = Rc::new(RefCell::new(Self {
            id,
            stream,
            interests,
            token: Token(0), // 临时值，register 后更新
            client_to_network: Ipv4PacketBuffer::new(),
            network_to_client: StreamBuffer::with_capacity(16 * MAX_PACKET_LENGTH),
            router: Router::new(),
            closed: false,
            close_listener,
            pending_packet_sources: Vec::new(),
            pending_id_bytes: 4,
        }));

        {
            let mut self_ref = rc.borrow_mut();
            // 设置 client 到 router（打破循环初始化依赖）
            self_ref.router.set_client(Rc::downgrade(&rc));

            let rc2 = rc.clone();
            let handler = move |selector: &mut Selector, event: &mio::event::Event| {
                rc2.borrow_mut().on_ready(selector, event);
            };
            let token = selector.register(&mut self_ref.stream, interests, handler)?;
            self_ref.token = token;
        }

        log::info!("Client {} 已注册，Token={:?}", id, rc.borrow().token);
        Ok(rc)
    }

    /// 获取客户端 ID。
    pub fn id(&self) -> u32 {
        self.id
    }

    /// 获取 Router 的可变引用。
    pub fn router(&mut self) -> &mut Router {
        &mut self.router
    }

    /// 获取 Token。
    pub fn token(&self) -> Token {
        self.token
    }

    /// 创建 ClientChannel（用于连接回传数据）。
    pub fn channel(&mut self) -> ClientChannel<'_> {
        ClientChannel::new(
            &mut self.network_to_client,
            self.token,
            &mut self.interests,
        )
    }

    /// 关闭客户端连接。
    fn close(&mut self, selector: &mut Selector) {
        self.closed = true;
        selector.deregister(&mut self.stream, self.token).unwrap();
        if self.stream.shutdown(Shutdown::Both).is_err() {
            log::warn!("无法关闭 Client socket");
        }
        self.router.clear(selector);
        self.close_listener.on_closed(self.id);
    }

    /// 事件就绪回调。
    fn on_ready(&mut self, selector: &mut Selector, event: &mio::event::Event) {
        match self.process(selector, event) {
            Ok(()) => {}
            Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                log::debug!("Client {} 虚假事件，忽略", self.id);
            }
            Err(_) => panic!("未处理的意外错误"),
        }
    }

    /// 处理事件。
    ///
    /// 处理顺序：先写后读，再更新 interest。
    /// 先写是因为发送可能腾出 network_to_client 缓冲区空间，
    /// 允许后续读取时回传更多数据。
    fn process(&mut self, selector: &mut Selector, event: &mio::event::Event) -> io::Result<()> {
        if !self.closed {
            if event.is_writable() {
                self.process_send(selector)?;
            }
            if !self.closed && event.is_readable() {
                self.process_receive(selector)?;
            }
            if !self.closed {
                self.update_interests(selector);
            }
        }
        Ok(())
    }

    /// 处理发送（client_id 或 buffer 数据）。
    fn process_send(&mut self, selector: &mut Selector) -> io::Result<()> {
        if self.pending_id_bytes > 0 {
            match self.send_id() {
                Ok(()) => {
                    if self.pending_id_bytes == 0 {
                        log::debug!("Client id #{} 已发送", self.id);
                    }
                }
                Err(err) => {
                    if err.kind() == io::ErrorKind::WouldBlock {
                        return Err(err);
                    }
                    log::error!("无法写入 Client id #{}", self.id);
                    self.close(selector);
                }
            }
        } else {
            match self.network_to_client.write_to(&mut self.stream) {
                Ok(0) => {
                    // 写入 0 字节意味着连接已关闭
                    self.close(selector);
                }
                Ok(_) => {
                    self.process_pending(selector);
                }
                Err(err) => {
                    if err.kind() == io::ErrorKind::WouldBlock {
                        return Err(err);
                    }
                    log::error!("写入失败: [{:?}] {}", err.kind(), err);
                    self.close(selector);
                }
            }
        }
        Ok(())
    }

    /// 处理接收（从 Android 读取 IP 包）。
    fn process_receive(&mut self, selector: &mut Selector) -> io::Result<()> {
        match self.read() {
            Ok(true) => self.push_to_network(selector),
            Ok(false) => {
                log::debug!("Client {} 已到达 EOF", self.id);
                self.close(selector);
            }
            Err(err) => {
                if err.kind() == io::ErrorKind::WouldBlock {
                    return Err(err);
                }
                log::error!("读取失败: [{:?}] {}", err.kind(), err);
                self.close(selector);
            }
        }
        Ok(())
    }

    /// 发送 client_id。
    fn send_id(&mut self) -> io::Result<()> {
        let raw_id = self.id.to_be_bytes();
        let w = self.stream.write(&raw_id[4 - self.pending_id_bytes..])?;
        self.pending_id_bytes -= w;
        Ok(())
    }

    /// 从 TCP 流读取数据到 Ipv4PacketBuffer。
    fn read(&mut self) -> io::Result<bool> {
        self.client_to_network.read_from(&mut self.stream)
    }

    /// 根据 buffer 状态更新 interest。
    ///
    /// - 缓冲区为空：只关心 READABLE（等新数据进来）
    /// - 缓冲区非空：READABLE | WRITABLE（需要把数据写出去）
    ///
    /// 只有当 interest 发生变化时才调用 reregister（避免不必要的系统调用）。
    /// 此方法为 pub，供 Connection 在 on_ready 后调用。
    pub fn update_interests(&mut self, selector: &mut Selector) {
        let ready = if self.network_to_client.is_empty() {
            Interest::READABLE
        } else {
            Interest::READABLE | Interest::WRITABLE
        };
        if self.interests != ready {
            self.interests = ready;
            selector
                .reregister(&mut self.stream, self.token, ready)
                .expect("无法重新注册到 poll");
        }
    }

    /// 将缓冲区中的所有 IP 包推送给 Router。
    fn push_to_network(&mut self, selector: &mut Selector) {
        while self.push_one_packet_to_network(selector) {
            self.client_to_network.next();
        }
    }

    /// 推送一个 IP 包给 Router。
    fn push_one_packet_to_network(&mut self, selector: &mut Selector) -> bool {
        match self.client_to_network.as_ipv4_packet() {
            Some(ref packet) => {
                let raw = packet.raw().to_vec();
                self.router.send_to_network(selector, &raw);
                true
            }
            None => false,
        }
    }

    /// 处理待发送的包源（背压恢复后推送）。
    ///
    /// 当 Client 的 network_to_client 缓冲区之前已满时，TcpConnection
    /// 会将自己注册为 pending packet source。此方法在缓冲区有空间后
    /// 被 process_send 调用，逐个尝试将延迟的包发送出去。
    ///
    /// 如果仍然发不出去（缓冲区又满了），保留在 pending 列表中等待下次。
    fn process_pending(&mut self, selector: &mut Selector) {
        let mut vec = Vec::new();
        mem::swap(&mut self.pending_packet_sources, &mut vec);
        for pending in vec.into_iter() {
            let consumed = {
                let mut source = pending.borrow_mut();
                let result = {
                    let ipv4_packet = source
                        .get()
                        .expect("Pending source 不应有空包");
                    self.send_to_client_direct(selector, ipv4_packet)
                };
                match result {
                    Ok(()) => {
                        source.next(selector);
                        true
                    }
                    Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => false,
                    Err(_) => {
                        panic!("无法发送包给 Client（未知原因）");
                    }
                }
            };
            if !consumed {
                // 还发不出去，保留在 pending 列表中
                self.pending_packet_sources.push(pending);
            }
        }
    }

    /// 直接发送 IP 包到 Client（用于 process_pending）。
    fn send_to_client_direct(
        &mut self,
        selector: &mut Selector,
        ipv4_packet: &[u8],
    ) -> io::Result<()> {
        if ipv4_packet.len() <= self.network_to_client.remaining() {
            self.network_to_client.read_from(ipv4_packet);
            self.update_interests(selector);
            Ok(())
        } else {
            log::warn!("Client 缓冲区已满");
            Err(io::Error::new(io::ErrorKind::WouldBlock, "Client buffer full"))
        }
    }

    /// 注册一个待处理的包源（背压）。
    pub fn register_pending_packet_source(&mut self, source: Rc<RefCell<dyn PacketSource>>) {
        self.pending_packet_sources.push(source);
    }

    /// 清理过期连接。
    pub fn clean_expired_connections(&mut self, selector: &mut Selector) {
        self.router.clean_expired_connections(selector);
    }
}
