use crate::relay::client::{Client, CloseListener};
use crate::relay::selector::Selector;
use mio::Events;
use mio::net::TcpListener;
use mio::{Interest, Token};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Relay 中继服务器的监听端口。
pub const RELAY_PORT: u16 = 31416;

/// 清理过期连接的间隔（秒）。
const CLEANUP_INTERVAL_SECS: u64 = 60;

/// 隧道服务器，负责接受来自 Android 的客户端连接。
///
/// 监听 `127.0.0.1:31416`，每当有新连接到来时：
/// 1. 接受连接，分配一个递增的 `client_id`
/// 2. 创建 `Client` 对象并注册到 Selector
/// 3. 向 Android 发送 4 字节的 `client_id`（big-endian）
pub struct TunnelServer {
    tcp_listener: TcpListener,
    token: Token,
    next_client_id: u32,
    clients: Vec<Rc<RefCell<Client>>>,
}

/// Client 关闭时的回调——从 clients 列表中移除。
struct ClientCloseListener {
    clients: Vec<Rc<RefCell<Client>>>,
}

impl CloseListener for ClientCloseListener {
    fn on_closed(&mut self, client_id: u32) {
        self.clients.retain(|c| c.borrow().id() != client_id);
    }
}

impl TunnelServer {
    /// 创建并启动隧道服务器。
    pub fn new(selector: &mut Selector) -> std::io::Result<Rc<RefCell<Self>>> {
        let addr: std::net::SocketAddr = format!("127.0.0.1:{RELAY_PORT}").parse().unwrap();
        let tcp_listener = TcpListener::bind(addr)?;
        log::info!("TunnelServer 监听于 {addr}");

        let server = Rc::new(RefCell::new(Self {
            tcp_listener,
            token: Token(0),
            next_client_id: 0,
            clients: Vec::new(),
        }));

        // 注册到 Selector
        let token = {
            let mut s = server.borrow_mut();
            let rc2 = server.clone();
            let handler = move |selector: &mut Selector, event: &mio::event::Event| {
                rc2.borrow_mut().on_ready(selector, event);
            };
            let token = selector.register(&mut s.tcp_listener, Interest::READABLE, handler)?;
            s.token = token;
            token
        };

        log::info!("TunnelServer 已注册，Token = {:?}", token);
        Ok(server)
    }

    /// 事件就绪回调。
    fn on_ready(&mut self, selector: &mut Selector, event: &mio::event::Event) {
        if event.is_readable() {
            self.accept(selector);
        }
    }

    /// 接受新连接。
    fn accept(&mut self, selector: &mut Selector) {
        loop {
            match self.tcp_listener.accept() {
                Ok((stream, addr)) => {
                    let client_id = self.next_client_id;
                    self.next_client_id += 1;
                    log::info!("新客户端连接: {addr}, client_id = {client_id}");

                    let close_listener = Box::new(ClientCloseListener {
                        clients: self.clients.clone(),
                    });

                    match Client::create(client_id, selector, stream, close_listener) {
                        Ok(client) => {
                            log::info!("Client {client_id} 创建成功");
                            self.clients.push(client);
                        }
                        Err(e) => {
                            log::error!("Client {client_id} 创建失败: {e}");
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    break;
                }
                Err(e) => {
                    log::error!("接受连接失败: {e}");
                    break;
                }
            }
        }
    }

    /// 清理过期的连接。
    pub fn clean_up(&mut self, selector: &mut Selector) {
        for client in &self.clients {
            client.borrow_mut().clean_expired_connections(selector);
        }
    }
}

/// Relay 主控制器——运行事件循环。
pub struct Relay;

impl Relay {
    /// 启动 Relay 事件循环。
    pub fn run() -> std::io::Result<()> {
        let mut selector = Selector::new()?;
        let mut events = Events::with_capacity(1024);

        let tunnel_server = TunnelServer::new(&mut selector)?;

        log::info!("Relay 事件循环已启动");
        let mut next_cleanup = Instant::now() + Duration::from_secs(CLEANUP_INTERVAL_SECS);

        loop {
            let timeout = next_cleanup.saturating_duration_since(Instant::now());
            selector.poll(&mut events, Some(timeout))?;
            selector.run_handlers(&events);

            if Instant::now() >= next_cleanup {
                tunnel_server.borrow_mut().clean_up(&mut selector);
                next_cleanup = Instant::now() + Duration::from_secs(CLEANUP_INTERVAL_SECS);
            }
        }
    }
}
