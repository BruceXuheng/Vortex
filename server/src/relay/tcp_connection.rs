use crate::packet::ipv4_packet::Ipv4Packet;
use crate::packet::transport_header::TransportHeader;
use crate::relay::client::{Client, ClientChannel};
use crate::relay::connection::{Connection, ConnectionId};
use crate::relay::packet_source::PacketSource;
use crate::relay::packetizer::Packetizer;
use crate::relay::selector::Selector;
use crate::relay::stream_buffer::StreamBuffer;
use mio::net::TcpStream;
use mio::{Interest, Token};
use rand::random;
use std::cell::RefCell;
use std::cmp;
use std::io::{self, Read};
use std::num::Wrapping;
use std::rc::{Rc, Weak};

/// MTU——和 Android 端 VpnService 的 MTU 保持一致。
const MTU: u16 = 0x4000;
/// 最大 payload 长度 = MTU - 20(IP头) - 20(TCP头)。
const MAX_PAYLOAD_LENGTH: u16 = MTU - 20 - 20;

/// TCP 连接。
///
/// 对齐 Gnirehtet 设计：
/// - 状态机: Init → SynSent → SynReceived → Established → LastAck/FinWait1/FinWait2
/// - 流控: `remaining_client_window()` 决定是否读取网络数据
/// - 延迟 FIN: 收到 FIN 时只设 `fin_received = true`，等 buffer 清空后再处理
/// - 跳过 CloseWait: Established 收到 FIN 后直接到 LastAck
/// - may_read/may_write 动态决定 interest
/// - PacketSource 实现用于背压
///
/// **ClientChannel 传递机制**（对齐 Gnirehtet）：
/// - `send_to_network()` 由 Client 在 `push_one_packet_to_network` 中调用，
///   ClientChannel 在 Client 侧提前创建并传入，避免二次 borrow Client 的 RefCell
/// - `on_ready()` 由 Selector 直接调用，此时 Client 的 RefCell 未被 borrow，
///   Connection 可安全通过 `with_client_channel()` 创建 ClientChannel
pub struct TcpConnection {
    /// 自身弱引用（用于注册为 PacketSource 时避免循环引用）。
    self_weak: Weak<RefCell<TcpConnection>>,
    /// 连接五元组标识。
    id: ConnectionId,
    /// 所属 Client 的弱引用（用于回传数据和更新 interest）。
    client: Weak<RefCell<Client>>,
    /// 与真实服务器的 TCP 连接。
    stream: TcpStream,
    /// 当前注册的 interest（跟踪状态以避免不必要的 reregister）。
    interests: Interest,
    /// Selector Token。
    token: Token,
    /// Android → 真实服务器方向的数据缓冲区。
    client_to_network: StreamBuffer,
    /// 真实服务器 → Android 方向的 IP 包构造器。
    network_to_client: Packetizer,
    /// 背压：待发送但 Client 缓冲区满的包长度。
    packet_for_client_length: Option<u16>,
    /// 背压：缓存待发送的完整 IP 包数据。
    pending_packet_data: Option<Vec<u8>>,
    closed: bool,
    tcb: Tcb,
}

/// 传输控制块 (TCB)——跟踪 TCP 序列号和状态。
struct Tcb {
    state: TcpState,
    /// 客户端 SYN 的序列号（用于检测重复 SYN）。
    syn_sequence_number: u32,
    /// 我方序列号（发回给 Android 的包中使用）。
    sequence_number: Wrapping<u32>,
    /// 我方确认号（期望从 Android 收到的下一个字节序号）。
    acknowledgement_number: Wrapping<u32>,
    /// 流控：Android 确认的序列号（用于计算剩余窗口）。
    their_acknowledgement_number: u32,
    /// 我方发送 FIN 时的序列号（用于匹配对方 ACK of FIN）。
    fin_sequence_number: Option<u32>,
    /// 延迟 FIN 标志——收到 FIN 后等 buffer 清空再处理。
    fin_received: bool,
    /// 流控：Android 声明的接收窗口大小。
    client_window: u16,
}

/// TCP 连接状态。
#[derive(Debug, PartialEq, Eq)]
enum TcpState {
    /// 初始状态，等待第一个 SYN。
    Init,
    /// 收到 SYN，正在与真实服务器建立连接。
    SynSent,
    /// 连接已建立，已发送 SYN+ACK，等待 ACK。
    SynReceived,
    /// 连接已建立，正常数据传输。
    Established,
    /// 收到对方 FIN，已发送 FIN+ACK，等待 ACK。
    LastAck,
    /// 主动关闭：已发 FIN，等待 ACK。
    FinWait1,
    /// 收到 ACK of FIN，等待对方 FIN。
    FinWait2,
    /// 同时关闭：发了 FIN 又收到 FIN，等待 ACK。
    Closing,
}

impl Tcb {
    fn new() -> Self {
        Self {
            state: TcpState::Init,
            syn_sequence_number: 0,
            sequence_number: Wrapping(0),
            acknowledgement_number: Wrapping(0),
            their_acknowledgement_number: 0,
            fin_sequence_number: None,
            fin_received: false,
            client_window: 0,
        }
    }

    /// 计算剩余的客户端窗口大小（流控）。
    ///
    /// 窗口 = (对方已确认的序列号 + 对方窗口大小) - 我方当前序列号。
    /// 即 Android 还能接收多少字节的数据。
    /// 如果结果为负（溢出），返回 0（窗口已耗尽，不能继续发送）。
    fn remaining_client_window(&self) -> u16 {
        let wrapped_remaining = Wrapping(self.their_acknowledgement_number)
            + Wrapping(u32::from(self.client_window))
            - self.sequence_number;
        let remaining = wrapped_remaining.0;
        if remaining <= u32::from(self.client_window) {
            remaining as u16
        } else {
            0
        }
    }

    fn numbers(&self) -> String {
        format!(
            "(seq={}, ack={})",
            self.sequence_number, self.acknowledgement_number
        )
    }
}

impl TcpConnection {
    /// 创建新的 TCP 连接。
    pub fn create(
        selector: &mut Selector,
        id: ConnectionId,
        client: Weak<RefCell<Client>>,
        syn_packet: &[u8],
    ) -> io::Result<Rc<RefCell<Self>>> {
        log::info!("{} 打开连接", id.display());

        let stream = Self::create_stream(&id)?;

        let reference_packet = syn_packet.to_vec();

        // 初始 interest 为 WRITABLE（等待连接完成）
        let interests = Interest::WRITABLE;

        let rc = Rc::new(RefCell::new(Self {
            self_weak: Weak::new(),
            id,
            client,
            stream,
            interests,
            token: Token(0),
            client_to_network: StreamBuffer::with_capacity(4 * 65535),
            network_to_client: Packetizer::new(),
            packet_for_client_length: None,
            pending_packet_data: None,
            closed: false,
            tcb: Tcb::new(),
        }));

        {
            let mut self_ref = rc.borrow_mut();
            self_ref.self_weak = Rc::downgrade(&rc);

            let rc2 = rc.clone();
            let handler = move |selector: &mut Selector, event: &mio::event::Event| {
                rc2.borrow_mut().on_ready(selector, event);
            };
            let token = selector.register(&mut self_ref.stream, interests, handler)?;
            self_ref.token = token;

            // 存储参考包，用于 Packetizer
            self_ref.network_to_client.set_reference_packet(&reference_packet);
        }

        // 对齐 Gnirehtet：不在 create 中调用 handle_first_packet。
        // SYN 包的处理在第一次 send_to_network → handle_packet → handle_first_packet 中完成，
        // 此时 ClientChannel 已由 Client 传入，可以安全回传 RST 等控制包。
        // init 状态在 handle_first_packet 中处理时会设置 SynSent 状态。

        Ok(rc)
    }

    fn create_stream(id: &ConnectionId) -> io::Result<TcpStream> {
        let dest_addr: std::net::SocketAddr = id.rewritten_destination().into();
        TcpStream::connect(dest_addr)
    }

    /// 从 Client 获取 ClientChannel。
    ///
    /// **仅在 on_ready 路径中使用**——此时 Client 的 RefCell 未被 borrow，
    /// 可以安全地 borrow Client 来创建 ClientChannel。
    ///
    /// **绝不能在 send_to_network 路径中使用**——此时 Client 的 RefCell
    /// 已被 borrow（由 Client::push_one_packet_to_network 触发），
    /// 会导致 RefCell already borrowed panic。
    fn with_client_channel<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self, &mut ClientChannel) -> R,
    {
        let client_rc = self.client.upgrade().expect("Client 弱引用不应为空");
        let mut client = client_rc.borrow_mut();
        let mut channel = client.channel();
        f(self, &mut channel)
    }

    /// 事件就绪回调。
    fn on_ready(&mut self, selector: &mut Selector, event: &mio::event::Event) {
        match self.process(selector, event) {
            Ok(()) => {}
            Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                log::trace!("{} 虚假事件，忽略", self.id.display());
            }
            Err(_) => panic!("未处理的意外错误"),
        }
    }

    /// 处理事件。
    fn process(&mut self, selector: &mut Selector, event: &mio::event::Event) -> io::Result<()> {
        if !self.closed {
            if event.is_writable() {
                if self.tcb.state == TcpState::SynSent {
                    // WRITABLE 在 SynSent 状态表示连接已建立
                    self.process_connect(selector);
                } else {
                    self.process_send(selector)?;
                }
            }
            if !self.closed && event.is_readable() {
                self.process_receive(selector)?;
            }
            if !self.closed {
                self.update_interests(selector);
            }
            // 已关闭的连接由 Router::send_to_network（is_closed 检查）
            // 和 clean_expired_connections 负责移除
        }
        Ok(())
    }

    /// 连接建立完成。
    fn process_connect(&mut self, selector: &mut Selector) {
        assert_eq!(self.tcb.state, TcpState::SynSent);
        self.tcb.state = TcpState::SynReceived;
        log::debug!("{} State = {:?}", self.id.display(), self.tcb.state);
        // on_ready 路径，可以安全使用 with_client_channel
        self.with_client_channel(|s, ch| {
            s.reply_empty_packet_to_client(selector, ch, 0x12); // SYN+ACK
        });
        self.tcb.sequence_number += Wrapping(1); // SYN 消耗一个序列号
    }

    /// 处理发送（将 buffer 数据写入真实 TCP 连接）。
    fn process_send(&mut self, selector: &mut Selector) -> io::Result<()> {
        match self.client_to_network.write_to(&mut self.stream) {
            Ok(0) => {
                // 写入 0 字节意味着连接已关闭
                self.close(selector);
            }
            Ok(w) => {
                self.tcb.acknowledgement_number += Wrapping(w as u32);

                if self.tcb.fin_received && self.client_to_network.is_empty() {
                    // 所有数据已发送，处理延迟的 FIN
                    log::debug!("{} 无待发送数据，处理延迟 FIN", self.id.display());
                    // on_ready 路径，可以安全使用 with_client_channel
                    self.with_client_channel(|s, ch| {
                        s.do_handle_fin(selector, ch);
                    });
                } else {
                    log::trace!("{} 发送 ACK {}", self.id.display(), self.tcb.numbers());
                    self.with_client_channel(|s, ch| {
                        s.reply_empty_packet_to_client(selector, ch, 0x10); // ACK
                    });
                }
            }
            Err(err) => {
                if err.kind() == io::ErrorKind::WouldBlock {
                    return Err(err);
                }
                log::error!("{} 写入失败: {:?}", self.id.display(), err.kind());
                self.with_client_channel(|s, ch| {
                    s.reply_empty_packet_to_client(selector, ch, 0x14); // RST+ACK
                });
                self.close(selector);
            }
        }
        Ok(())
    }

    /// 处理接收（从真实 TCP 连接读取数据，构造 IP 包回传）。
    fn process_receive(&mut self, selector: &mut Selector) -> io::Result<()> {
        if self.packet_for_client_length.is_some() {
            log::debug!("{} 有 pending 包，跳过接收", self.id.display());
            return Ok(());
        }
        let remaining_client_window = self.tcb.remaining_client_window();
        if remaining_client_window == 0 {
            log::debug!("{} 客户端窗口为 0，跳过接收", self.id.display());
            return Ok(());
        }
        let max_payload_length =
            cmp::min(remaining_client_window, MAX_PAYLOAD_LENGTH) as usize;

        // 读取数据并构造回传包
        let mut buf = [0u8; 65535];
        let read_len = cmp::min(max_payload_length, buf.len());
        match self.stream.read(&mut buf[..read_len]) {
            Ok(0) => {
                // EOF
                self.eof(selector);
            }
            Ok(n) => {
                let seq = self.tcb.sequence_number.0;
                let ack = self.tcb.acknowledgement_number.0;
                let ref_pkt = self.network_to_client.reference_packet().to_vec();
                let packet = self.network_to_client.create_tcp_packet(
                    &ref_pkt,
                    &buf[..n],
                    seq,
                    ack,
                    0x18, // PSH+ACK
                    self.tcb.client_window,
                );

                // 保存包数据（因为 packet 引用了 Packetizer 内部 buffer，需要拷贝出来）
                let packet_data = packet.to_vec();

                // on_ready 路径，可以安全使用 with_client_channel
                // 尝试发送给 Client；如果缓冲区满则进入背压模式
                match self.send_to_client_via_channel(selector, &packet_data) {
                    Ok(()) => {
                        log::debug!(
                            "{} 包 ({} 字节) 已发送给 Client {}",
                            self.id.display(),
                            n,
                            self.tcb.numbers()
                        );
                        self.tcb.sequence_number += Wrapping(n as u32);
                    }
                    Err(_) => {
                        // Client 缓冲区满——进入背压模式：
                        // 1. 将自己注册为 Client 的 pending packet source
                        // 2. 保存包数据和长度
                        // 3. 等 Client 缓冲区有空间后主动拉取（PacketSource::get/next）
                        let client_rc = self.client.upgrade().expect("Client 弱引用不应为空");
                        let mut client = client_rc.borrow_mut();
                        let self_rc = self.self_weak.upgrade().unwrap();
                        client.register_pending_packet_source(self_rc);
                        self.packet_for_client_length = Some(packet_data.len() as u16);
                        self.pending_packet_data = Some(packet_data);
                    }
                }
            }
            Err(err) => {
                if err.kind() == io::ErrorKind::WouldBlock {
                    return Err(err);
                }
                log::error!("{} 读取失败: {:?}", self.id.display(), err.kind());
                self.with_client_channel(|s, ch| {
                    s.reply_empty_packet_to_client(selector, ch, 0x14); // RST+ACK
                });
                self.close(selector);
            }
        }
        Ok(())
    }

    /// 真实服务器 EOF——发送 FIN+ACK 给 Android，转换状态。
    ///
    /// 如果当前是 Established，转到 FinWait1（主动关闭）；
    /// 如果是 FinWait1（同时关闭），转到 Closing。
    fn eof(&mut self, selector: &mut Selector) {
        // on_ready 路径，可以安全使用 with_client_channel
        self.with_client_channel(|s, ch| {
            s.reply_empty_packet_to_client(selector, ch, 0x11); // FIN+ACK
        });
        self.tcb.fin_sequence_number = Some(self.tcb.sequence_number.0);
        self.tcb.sequence_number += Wrapping(1); // FIN 消耗一个序列号
        self.tcb.state = if self.tcb.state == TcpState::Established {
            TcpState::FinWait1
        } else {
            TcpState::Closing
        };
        log::debug!("{} State = {:?}", self.id.display(), self.tcb.state);
    }

    /// 发送空包给 ClientChannel（使用已传入的 ClientChannel）。
    fn reply_empty_packet_to_client(
        &mut self,
        selector: &mut Selector,
        client_channel: &mut ClientChannel,
        flags: u8,
    ) {
        let seq = self.tcb.sequence_number.0;
        let ack = self.tcb.acknowledgement_number.0;
        let ref_pkt = self.network_to_client.reference_packet().to_vec();
        let packet = self.network_to_client.create_tcp_packet(
            &ref_pkt,
            &[],
            seq,
            ack,
            flags,
            self.tcb.client_window,
        );
        if let Err(err) = client_channel.send_to_client(selector, packet) {
            log::warn!("{} 无法发送包给 Client: {}", self.id.display(), err);
        }
    }

    /// 尝试发送包给 Client（on_ready 路径使用 with_client_channel）。
    fn send_to_client_via_channel(
        &mut self,
        selector: &mut Selector,
        ipv4_packet: &[u8],
    ) -> io::Result<()> {
        self.with_client_channel(|_s, ch| {
            ch.send_to_client(selector, ipv4_packet)
        })
    }

    /// 处理从 Android 收到的 IP 包。
    ///
    /// 对齐 Gnirehtet：接受 `&mut ClientChannel`，可直接回传控制包
    /// （SYN+ACK、FIN+ACK、RST），避免二次 borrow Client 的 RefCell。
    fn handle_packet(
        &mut self,
        selector: &mut Selector,
        client_channel: &mut ClientChannel,
        ipv4_packet: &[u8],
    ) {
        let packet = Ipv4Packet::new(ipv4_packet);
        let tcp_header = match packet.transport_header() {
            Some(TransportHeader::Tcp(t)) => t,
            _ => return,
        };

        if self.tcb.state == TcpState::Init {
            self.handle_first_packet(selector, client_channel, &packet);
            return;
        }

        if tcp_header.is_syn() {
            self.handle_duplicate_syn(selector, client_channel, &packet);
            return;
        }

        // 验证序列号：期望的序号 = 我方确认号 + 缓冲区中待发送的数据量
        // （因为缓冲区中的数据已经从序列号空间"占用"了，下一个包应紧随其后）
        let expected_packet =
            (self.tcb.acknowledgement_number + Wrapping(self.client_to_network.len() as u32)).0;
        if tcp_header.seq_number() != expected_packet {
            log::warn!(
                "{} 忽略包 seq={} ack={}; 期望 {}; flags={}",
                self.id.display(),
                tcp_header.seq_number(),
                tcp_header.ack_number(),
                expected_packet,
                tcp_header.flags()
            );
            return;
        }

        // 更新流控信息
        self.tcb.client_window = tcp_header.window();
        self.tcb.their_acknowledgement_number = tcp_header.ack_number();

        log::trace!(
            "{} 收到预期的包 seq={} flags={}",
            self.id.display(),
            tcp_header.seq_number(),
            tcp_header.flags()
        );

        if tcp_header.is_rst() {
            self.close(selector);
            return;
        }

        if tcp_header.is_ack() {
            self.handle_ack(selector, client_channel, &packet);
        }

        if tcp_header.is_fin() {
            self.handle_fin(selector, client_channel);
        }

        // 检查 FIN+ACK
        if let Some(fin_seq) = self.tcb.fin_sequence_number {
            if tcp_header.ack_number() == fin_seq + 1 {
                log::debug!("{} 收到 FIN 的 ACK", self.id.display());
                self.handle_fin_ack(selector);
            }
        }
    }

    /// 处理第一个包（必须是 SYN）。
    ///
    /// 对齐 Gnirehtet：接受 ClientChannel，非 SYN 首包时可回传 RST。
    fn handle_first_packet(
        &mut self,
        selector: &mut Selector,
        client_channel: &mut ClientChannel,
        ipv4_packet: &Ipv4Packet,
    ) {
        let tcp_header = match ipv4_packet.transport_header() {
            Some(TransportHeader::Tcp(t)) => t,
            _ => return,
        };

        if tcp_header.is_syn() {
            let their_seq = tcp_header.seq_number();
            self.tcb.acknowledgement_number = Wrapping(their_seq) + Wrapping(1);
            self.tcb.syn_sequence_number = their_seq;
            self.tcb.sequence_number = Wrapping(random::<u32>());
            self.tcb.client_window = tcp_header.window();
            self.tcb.state = TcpState::SynSent;
            log::debug!(
                "{} 已初始化 seq={}; ack={}",
                self.id.display(),
                self.tcb.sequence_number,
                self.tcb.acknowledgement_number
            );
            log::debug!("{} State = {:?}", self.id.display(), self.tcb.state);
        } else {
            log::warn!(
                "{} 非预期的首包 seq={} ack={} flags={}",
                self.id.display(),
                tcp_header.seq_number(),
                tcp_header.ack_number(),
                tcp_header.flags()
            );
            // 对齐 Gnirehtet：回传 RST 后关闭
            self.tcb.sequence_number = Wrapping(tcp_header.ack_number());
            self.reply_empty_packet_to_client(selector, client_channel, 0x14); // RST+ACK
            self.close(selector);
        }
    }

    /// 处理重复 SYN。
    ///
    /// 对齐 Gnirehtet：接受 ClientChannel，序列号不匹配时可回传 RST。
    fn handle_duplicate_syn(
        &mut self,
        selector: &mut Selector,
        client_channel: &mut ClientChannel,
        ipv4_packet: &Ipv4Packet,
    ) {
        let tcp_header = match ipv4_packet.transport_header() {
            Some(TransportHeader::Tcp(t)) => t,
            _ => return,
        };
        let their_seq = tcp_header.seq_number();
        if self.tcb.state == TcpState::SynSent {
            self.tcb.syn_sequence_number = their_seq;
            self.tcb.acknowledgement_number = Wrapping(their_seq) + Wrapping(1);
        } else if their_seq != self.tcb.syn_sequence_number {
            // 对齐 Gnirehtet：回传 RST 后关闭
            self.tcb.sequence_number = Wrapping(tcp_header.ack_number());
            self.reply_empty_packet_to_client(selector, client_channel, 0x14); // RST+ACK
            self.close(selector);
        }
    }

    /// 处理 ACK 包。
    fn handle_ack(
        &mut self,
        _selector: &mut Selector,
        _client_channel: &mut ClientChannel,
        ipv4_packet: &Ipv4Packet,
    ) {
        if self.tcb.state == TcpState::SynReceived {
            self.tcb.state = TcpState::Established;
            log::debug!("{} State = {:?}", self.id.display(), self.tcb.state);
            return;
        }

        let payload = ipv4_packet.transport_payload();
        if payload.is_empty() {
            return;
        }

        if self.client_to_network.remaining() < payload.len() {
            log::warn!("{} 缓冲区空间不足，丢弃包", self.id.display());
            return;
        }

        self.client_to_network.read_from(payload);
        // 数据写入 buffer 后，会在 process_send 中发送到真实网络并回复 ACK
    }

    /// 处理 FIN（延迟处理）。
    ///
    /// 对齐 Gnirehtet：接受 ClientChannel，可直接调用 do_handle_fin。
    fn handle_fin(
        &mut self,
        selector: &mut Selector,
        client_channel: &mut ClientChannel,
    ) {
        log::debug!("{} 收到来自 Client 的 FIN {}", self.id.display(), self.tcb.numbers());
        self.tcb.fin_received = true;
        if self.client_to_network.is_empty() {
            log::debug!("{} 无待发送数据，立即处理 FIN", self.id.display());
            self.do_handle_fin(selector, client_channel);
        }
        // 否则等 process_send 中 buffer 清空后再处理
    }

    /// 实际处理 FIN。
    fn do_handle_fin(
        &mut self,
        selector: &mut Selector,
        client_channel: &mut ClientChannel,
    ) {
        self.tcb.acknowledgement_number += Wrapping(1); // FIN 消耗一个序列号

        if self.tcb.state == TcpState::Established {
            self.reply_empty_packet_to_client(selector, client_channel, 0x11); // FIN+ACK
            self.tcb.fin_sequence_number = Some(self.tcb.sequence_number.0);
            self.tcb.sequence_number += Wrapping(1); // FIN 消耗一个序列号
            // 跳过 CloseWait，直接到 LastAck
            self.tcb.state = TcpState::LastAck;
            log::debug!("{} State = {:?}", self.id.display(), self.tcb.state);
        } else if self.tcb.state == TcpState::FinWait1 {
            self.reply_empty_packet_to_client(selector, client_channel, 0x10); // ACK
            self.tcb.state = TcpState::Closing;
            log::debug!("{} State = {:?}", self.id.display(), self.tcb.state);
        } else if self.tcb.state == TcpState::FinWait2 {
            self.reply_empty_packet_to_client(selector, client_channel, 0x10); // ACK
            self.close(selector);
        } else {
            log::warn!(
                "{} 在状态 {:?} 下收到 FIN",
                self.id.display(),
                self.tcb.state
            );
        }
    }

    /// 处理 FIN 的 ACK。
    fn handle_fin_ack(&mut self, selector: &mut Selector) {
        if self.tcb.state == TcpState::LastAck || self.tcb.state == TcpState::Closing {
            self.close(selector);
        } else if self.tcb.state == TcpState::FinWait1 {
            self.tcb.state = TcpState::FinWait2;
            log::debug!("{} State = {:?}", self.id.display(), self.tcb.state);
        } else if self.tcb.state != TcpState::FinWait2 {
            log::warn!(
                "{} 在状态 {:?} 下收到 FIN ACK",
                self.id.display(),
                self.tcb.state
            );
        }
    }

    /// 更新注册的 interest。
    fn update_interests(&mut self, selector: &mut Selector) {
        assert!(!self.closed);
        let ready = if self.tcb.state == TcpState::SynSent {
            Interest::WRITABLE
        } else {
            let mut r = Interest::READABLE;
            if self.may_write() {
                r |= Interest::WRITABLE;
            }
            // 窗口为 0 或有 pending 包时仍保留 READABLE（mio 要求至少一个 interest），
            // process_receive 中会检查窗口状态并跳过读取。
            r
        };
        if self.interests != ready {
            self.interests = ready;
            selector
                .reregister(&mut self.stream, self.token, ready)
                .expect("无法重新注册到 poll");
        }
    }

    /// 是否可以写入网络数据。
    fn may_write(&self) -> bool {
        !self.client_to_network.is_empty()
    }
}

impl Connection for TcpConnection {
    fn id(&self) -> ConnectionId {
        self.id.clone()
    }

    /// 对齐 Gnirehtet：接受 ClientChannel 参数。
    ///
    /// ClientChannel 由 Client 在 push_one_packet_to_network 中提前创建，
    /// 避免 Connection 二次 borrow Client 的 RefCell。
    fn send_to_network(
        &mut self,
        selector: &mut Selector,
        client_channel: &mut ClientChannel,
        ipv4_packet: &[u8],
    ) {
        self.handle_packet(selector, client_channel, ipv4_packet);
        if !self.closed {
            self.update_interests(selector);
        }
    }

    fn close(&mut self, selector: &mut Selector) {
        log::info!("{} 关闭连接", self.id.display());
        self.closed = true;
        if let Err(err) = selector.deregister(&mut self.stream, self.token) {
            log::warn!("{} 注销 TCP 流失败: {:?}", self.id.display(), err);
        }
    }

    fn is_expired(&self) -> bool {
        false
    }

    fn is_closed(&self) -> bool {
        self.closed
    }
}

impl PacketSource for TcpConnection {
    /// 返回待发送的 IP 包数据（Client 缓冲区恢复空间后拉取）。
    fn get(&mut self) -> Option<&[u8]> {
        self.pending_packet_data.as_deref()
    }

    /// 包已成功发送给 Client，更新序列号并清理状态。
    fn next(&mut self, selector: &mut Selector) {
        if self.packet_for_client_length.is_some() {
            let len = self.packet_for_client_length.unwrap();
            log::debug!(
                "{} 延迟包 ({} 字节) 已发送给 Client {}",
                self.id.display(),
                len,
                self.tcb.numbers()
            );
            self.tcb.sequence_number += Wrapping(u32::from(len));
            self.packet_for_client_length = None;
            self.pending_packet_data = None;
            self.update_interests(selector);
        }
    }
}
