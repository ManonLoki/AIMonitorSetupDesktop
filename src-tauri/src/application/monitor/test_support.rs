// 跨多个测试模块共享的测试夹具：多处子模块（`service_lifecycle`、
// `service_discovery`、`relay::worker`、`relay::forward` 等）的测试都需要
// 构造示例 Profile / 已保存数据，或需要一个模拟设备 HTTP 服务器，因此把这些
// 构造函数放在这里统一复用，避免每个测试文件各自重复实现一份。
use std::{
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    thread,
    time::Duration,
};

use crate::domain::monitor::{AiProfile, AiTool, HookBehavior, device::HookContent};

// 构造一个测试用的 AI Profile：Codex 工具、槽位 1，四种行为各配一张示例图片。
pub(super) fn test_profile() -> AiProfile {
    AiProfile {
        device_id: "screen-1".to_owned(),
        tool: AiTool::Codex,
        slot: 1,
        hooks: [
            (HookBehavior::Idle, "idle.png"),
            (HookBehavior::Running, "running.gif"),
            (HookBehavior::Asking, "asking.png"),
            (HookBehavior::Error, "error.png"),
        ]
        .into_iter()
        .map(|(behavior, image)| HookContent {
            behavior,
            content: String::new(),
            image: image.to_owned(),
        })
        .collect(),
    }
}

/// 从测试用 `TcpStream` 中读出一份完整的 HTTP 请求（读到请求头结束，
/// 再按 Content-Length 读满请求体），供各处伪造的设备服务器共用。
pub(super) fn read_test_http_request(stream: &mut TcpStream) -> Vec<u8> {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut request = Vec::new();
    let mut buffer = [0_u8; 2048];
    loop {
        // 持续读取并追加数据。
        let length = stream.read(&mut buffer).unwrap();
        request.extend_from_slice(&buffer[..length]);
        // 头部结束标志还没出现，继续读。
        let Some(header_end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") else {
            continue;
        };
        let header_end = header_end + 4;
        // 解析出 Content-Length，判断请求体是否已经读完整。
        let headers = std::str::from_utf8(&request[..header_end]).unwrap();
        let content_length = headers
            .split("\r\n")
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap();
        if request.len() >= header_end + content_length {
            break;
        }
    }
    request
}

/// 启动一个只接受一次连接、读完完整请求后先睡眠再回 200 的测试服务器，
/// 用来验证多设备转发是否真的并发执行（而不是排队等前一台超时/变慢）。
pub(super) fn slow_test_server(delay: Duration) -> (SocketAddr, thread::JoinHandle<()>) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        read_test_http_request(&mut stream);
        // 读完整个请求后先睡眠指定延迟，再回复，模拟一台响应缓慢的设备。
        thread::sleep(delay);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
    });
    (address, handle)
}
