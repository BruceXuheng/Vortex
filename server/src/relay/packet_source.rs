use crate::relay::selector::Selector;

/// 包源 trait——用于背压恢复。
///
/// 当 TcpConnection 有数据要发送给 Client，但 Client 缓冲区满时，
/// TcpConnection 会将自己注册为 pending packet source。
/// 等 Client 缓冲区有空间后，Client 调用 `get()` 获取待发送的包，
/// 然后 `next()` 更新序列号和状态。
///
/// 对齐 Gnirehtet 设计。
pub trait PacketSource {
    /// 获取待发送的 IP 包数据。
    fn get(&mut self) -> Option<&[u8]>;

    /// 包已被成功发送，更新状态。
    fn next(&mut self, selector: &mut Selector);
}
