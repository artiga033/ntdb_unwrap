use crate::{protos, ProtobufSnafu};
use derive_more::{From, Into};
use protobuf::Message as _;
use rusqlite::types::FromSql;
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, From, Into, Serialize, Deserialize)]
pub struct ChatType(i64);
impl fmt::Display for ChatType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self.0 {
                1 => "私聊",
                2 => "群聊",
                4 => "频道",
                103 => "公众号",
                102 => "企业客服",
                100 => "临时会话",
                i => return write!(f, "未知({})", i),
            }
        )
    }
}
impl FromSql for ChatType {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        i64::column_result(value).map(ChatType::from)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, From, Into, Serialize, Deserialize)]
pub struct MessageType(i64);
impl fmt::Display for MessageType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self.0 {
                0 => "无消息",
                1 => "消息空白",
                2 => "文本消息",
                3 => "群文件",
                5 => "系统消息",
                6 => "语音消息",
                7 => "视频文件",
                8 => "合并转发消息",
                9 => "回复类型消息",
                10 => "红包",
                11 => "应用消息",
                i => return write!(f, "未知({})", i),
            }
        )
    }
}
impl FromSql for MessageType {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        i64::column_result(value).map(MessageType::from)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, From, Into, Serialize, Deserialize)]
pub struct SubMessageType(i64);
impl fmt::Display for SubMessageType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self.0 {
                0 => "非常规text消息",
                1 => "普通文本消息",
                2 => "图片消息",
                3 => "群公告",
                4 => "群文件视频消息",
                8 => "群文件音频消息",
                11 => "原创表情包",
                12 => "拍一拍消息",
                16 => "群文件docx消息",
                32 => "平台文本消息",
                33 => "回复类型消息",
                64 => "群文件xlsx消息",
                161 => "存在链接",
                512 => "群文件zip消息",
                2048 => "群文件exe消息",
                4096 => "表情消息",
                i => return write!(f, "未知({})", i),
            }
        )
    }
}
impl FromSql for SubMessageType {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        i64::column_result(value).map(SubMessageType::from)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SendStatus {
    Failed = 0,
    Sending = 1,
    Success = 2,
    /// 消息被和谐
    Erased = 3,
    Unknown,
}
impl From<i64> for SendStatus {
    fn from(i: i64) -> Self {
        match i {
            0 => SendStatus::Failed,
            1 => SendStatus::Sending,
            2 => SendStatus::Success,
            3 => SendStatus::Erased,
            _ => SendStatus::Unknown,
        }
    }
}
impl From<SendStatus> for i64 {
    fn from(val: SendStatus) -> Self {
        match val {
            SendStatus::Failed => 0,
            SendStatus::Sending => 1,
            SendStatus::Success => 2,
            SendStatus::Erased => 3,
            SendStatus::Unknown => -1,
        }
    }
}
impl FromSql for SendStatus {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        i64::column_result(value).map(SendStatus::from)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, From, Into, Serialize, Deserialize)]
pub struct AtFlag(i64);
impl AtFlag {
    pub const SOMEONE_AT_ME: i64 = 6;
    pub const SOMEONE_AT_OTHERS: i64 = 2;
    pub const NO_AT: i64 = 0;
}
impl FromSql for AtFlag {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        i64::column_result(value).map(AtFlag::from)
    }
}

pub use protos::message::{FeedMessage, Message, SingleMessage};
impl FromSql for Message {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let bytes = Vec::<u8>::column_result(value)?;
        Message::parse_from_bytes(&bytes)
            .context(ProtobufSnafu { raw: bytes })
            .map_err(|x| x.into())
    }
}
pub type UnknownProtoBytes = protos::message::Empty;
impl FromSql for UnknownProtoBytes {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let bytes = Vec::<u8>::column_result(value)?;
        UnknownProtoBytes::parse_from_bytes(&bytes)
            .context(ProtobufSnafu { raw: bytes })
            .map_err(|x| x.into())
    }
}
