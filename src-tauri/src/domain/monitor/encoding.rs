// 标准 Base64 编码实现（自实现而非依赖第三方库）：每 3 字节输入编码为 4 个输出字符。
pub(crate) fn encode_base64(bytes: &[u8]) -> String {
    // 标准 Base64 字符表（不含 URL-safe 变体）。
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    // 预分配容量：每 3 字节输入产生 4 字节输出，向上取整分组数。
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    // 按 3 字节一组处理输入（最后一组可能不足 3 字节）。
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        // 分组不足 3/2 字节时，缺失的字节按 0 处理（真正的截断由后面 '=' 填充体现）。
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        // 第一个输出字符：取第一字节的高 6 位。
        encoded.push(char::from(ALPHABET[usize::from(first >> 2)]));
        // 第二个输出字符：第一字节低 2 位 + 第二字节高 4 位。
        encoded.push(char::from(
            ALPHABET[usize::from(((first & 0b11) << 4) | (second >> 4))],
        ));
        // 第三个输出字符：分组含第二字节时才有效，否则用 '=' 填充。
        encoded.push(if chunk.len() > 1 {
            char::from(ALPHABET[usize::from(((second & 0b1111) << 2) | (third >> 6))])
        } else {
            '='
        });
        // 第四个输出字符：分组含第三字节时才有效，否则用 '=' 填充。
        encoded.push(if chunk.len() > 2 {
            char::from(ALPHABET[usize::from(third & 0b11_1111)])
        } else {
            '='
        });
    }
    encoded
}
