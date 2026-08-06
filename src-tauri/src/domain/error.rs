use std::collections::BTreeMap;

use serde::Serialize;

/// 面向前端的可序列化业务错误：`code` 对应前端 i18n 词典里的翻译 key，
/// `params` 是该翻译模板需要插值的动态部分（文件名、设备 ID、底层错误文本等）。
/// 翻译文案本身只存在于前端 `shared/i18n` 词典中；Rust 侧不做任何面向用户的文案拼接，
/// 从而保证同一份错误在中英文界面下都能正确显示（对应 CLAUDE.md 里“命令失败必须返回
/// 可序列化、UI 语义化的错误”的约束）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppError {
    pub code: &'static str,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, String>,
}

impl AppError {
    pub fn new(code: &'static str) -> Self {
        Self {
            code,
            params: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn param(mut self, key: &str, value: impl Into<String>) -> Self {
        self.params.insert(key.to_owned(), value.into());
        self
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.code)?;
        if !self.params.is_empty() {
            write!(f, " {:?}", self.params)?;
        }
        Ok(())
    }
}

impl std::error::Error for AppError {}
