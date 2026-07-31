use mio::{Events, Interest, Poll, Token};
use slab::Slab;
use std::rc::Rc;

/// 事件处理器 trait。
///
/// 对齐 Gnirehtet 设计：使用 `&self` 签名，通过闭包捕获 `Rc<RefCell<Self>>`
/// 在闭包内调用 `borrow_mut()` 实现 `&mut self` 效果。
///
/// 为 `Fn(&mut Selector, Event)` 闭包自动实现此 trait，
/// 这样 register 时可以直接传入闭包作为 handler。
pub trait EventHandler {
    /// 当注册的 I/O 事件就绪时被调用。
    fn on_ready(&self, selector: &mut Selector, event: &mio::event::Event);
}

/// 为闭包自动实现 EventHandler。
impl<F> EventHandler for F
where
    F: Fn(&mut Selector, &mio::event::Event),
{
    fn on_ready(&self, selector: &mut Selector, event: &mio::event::Event) {
        self(selector, event)
    }
}

/// I/O 多路复用器，封装 mio Poll + Slab Token 管理。
///
/// 设计要点（对齐 Gnirehtet）：
/// - 使用 `Slab` 管理 Token 到 Handler 的映射，Token 即 Slab 的 index
/// - `deregister` 是延迟的：在事件循环一轮结束后才清理，避免迭代中修改
/// - `Rc<dyn EventHandler>` 存储闭包 handler
/// - 不再需要 defer_reregister 机制——handler 在 on_ready 结束后自行调用 update_interests
pub struct Selector {
    poll: Poll,
    handlers: Slab<Rc<dyn EventHandler>>,
    /// 延迟移除的 Token 列表（在 run_handlers 结束后清理）。
    tokens_to_remove: Vec<Token>,
}

impl Selector {
    /// 创建新的 Selector。
    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            poll: Poll::new()?,
            handlers: Slab::with_capacity(1024),
            tokens_to_remove: Vec::new(),
        })
    }

    /// 注册一个 source 和 handler 到 Poll。
    ///
    /// 返回分配的 Token，后续可用此 Token 来重新注册或注销。
    pub fn register<H>(
        &mut self,
        source: &mut impl mio::event::Source,
        interest: Interest,
        handler: H,
    ) -> std::io::Result<Token>
    where
        H: EventHandler + 'static,
    {
        let token = Token(self.handlers.insert(Rc::new(handler)));
        if let Err(err) = self.poll.registry().register(source, token, interest) {
            // 注册失败时移除已插入的 handler
            self.handlers.remove(token.0);
            Err(err)
        } else {
            Ok(token)
        }
    }

    /// 重新注册，更新 interest。
    pub fn reregister(
        &mut self,
        source: &mut impl mio::event::Source,
        token: Token,
        interest: Interest,
    ) -> std::io::Result<()> {
        self.poll.registry().reregister(source, token, interest)
    }

    /// 延迟注销一个 handler。
    ///
    /// 不会立即移除，而是在当前事件循环轮结束后清理。
    /// 这避免了在迭代 handlers 时修改 Slab 导致的问题。
    pub fn deregister(
        &mut self,
        source: &mut impl mio::event::Source,
        token: Token,
    ) -> std::io::Result<()> {
        self.poll.registry().deregister(source)?;
        self.tokens_to_remove.push(token);
        Ok(())
    }

    /// 等待 I/O 事件。
    ///
    /// `timeout` 为 None 表示阻塞等待，为 Some(0) 表示非阻塞轮询。
    pub fn poll(&mut self, events: &mut Events, timeout: Option<std::time::Duration>) -> std::io::Result<()> {
        self.poll.poll(events, timeout)
    }

    /// 分发事件到对应的 handler，然后清理延迟注销的 token。
    pub fn run_handlers(&mut self, events: &Events) {
        for event in events.iter() {
            let handler = self
                .handlers
                .get(event.token().0)
                .expect("Token 未找到")
                .clone();
            handler.on_ready(self, &event);
        }

        // 清理延迟注销的 handler
        for &token in &self.tokens_to_remove {
            self.handlers.remove(token.0);
        }
        self.tokens_to_remove.clear();
    }
}
