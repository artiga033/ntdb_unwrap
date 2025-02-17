use super::*;
use crate::Result;
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupMsgTable {
    pub id: i64,
    pub msg_random: i64,
    pub seq_id: i64,
    pub chat_type: ChatType,
    pub msg_type: MessageType,
    pub sub_msg_type: SubMessageType,
    /// > 本机发送的消息为1，其他客户端发送的为2，别人发的消息为0 ，转发消息为5，
    /// > 在已退出或被封禁的消息中为当日整点时间戳
    ///
    /// 不是很好用 Rust 类型表示（若enum则size得翻倍），所以不做处理了
    pub send_type: i64,
    pub sender_uid: String,
    #[serde(rename = "40026")]
    pub _40026: i64,
    pub peer_uid: String,
    pub peer_uin: i64,
    #[serde(rename = "40030")]
    pub _40040: i64,
    pub send_status: SendStatus,
    pub send_time: i64,
    #[serde(rename = "40052")]
    pub _40052: i64,
    /// 发送者群名片
    pub sender_group_name: String,
    pub sender_nickname: String,
    pub message: Option<Message>,
    #[serde(rename = "40900")]
    pub _40900: Option<UnknownProtoBytes>,
    #[serde(rename = "40105")]
    pub _40105: i64,
    #[serde(rename = "40005")]
    pub _40005: i64,
    pub send_date: i64,
    #[serde(rename = "40006")]
    pub _40006: i64,
    pub at_flag: AtFlag,
    #[serde(rename = "40600")]
    pub _40600: Option<UnknownProtoBytes>,
    #[serde(rename = "40060")]
    pub _40060: i64,
    pub reply_msg_seq: i64,
    #[serde(rename = "40851")]
    pub _40851: i64,
    #[serde(rename = "40601")]
    pub _40601: Option<UnknownProtoBytes>,
    #[serde(rename = "40801")]
    pub _40801: Option<UnknownProtoBytes>,
    #[serde(rename = "40605")]
    pub _40605: Option<UnknownProtoBytes>,
    pub group_number: i64,
    pub sender_uin: i64,
    #[serde(rename = "40062")]
    pub _40062: Option<UnknownProtoBytes>,
    #[serde(rename = "40083")]
    pub _40083: i64,
    #[serde(rename = "40084")]
    pub _40084: i64,
}

impl Model for GroupMsgTable {
    fn parse_row(row: &rusqlite::Row) -> Result<Self> {
        Ok(Self {
            id: map_field!(row, "40001")?,
            msg_random: map_field!(row, "40002")?,
            seq_id: map_field!(row, "40003")?,
            chat_type: map_field!(row, "40010")?,
            msg_type: map_field!(row, "40011")?,
            sub_msg_type: map_field!(row, "40012")?,
            send_type: map_field!(row, "40013")?,
            sender_uid: map_field!(row, "40020")?,
            _40026: map_field!(row, "40026")?,
            peer_uid: map_field!(row, "40021")?,
            peer_uin: map_field!(row, "40027")?,
            _40040: map_field!(row, "40040")?,
            send_status: map_field!(row, "40041")?,
            send_time: map_field!(row, "40050")?,
            _40052: map_field!(row, "40052")?,
            sender_group_name: map_field!(row, "40090")?,
            sender_nickname: map_field!(row, "40093")?,
            message: map_field!(row, "40800")?,
            _40900: map_field!(row, "40900")?,
            _40105: map_field!(row, "40105")?,
            _40005: map_field!(row, "40005")?,
            send_date: map_field!(row, "40058")?,
            _40006: map_field!(row, "40006")?,
            at_flag: map_field!(row, "40100")?,
            _40600: map_field!(row, "40600")?,
            _40060: map_field!(row, "40060")?,
            reply_msg_seq: map_field!(row, "40850")?,
            _40851: map_field!(row, "40851")?,
            _40601: map_field!(row, "40601")?,
            _40801: map_field!(row, "40801")?,
            _40605: map_field!(row, "40605")?,
            group_number: map_field!(row, "40030")?,
            sender_uin: map_field!(row, "40033")?,
            _40062: map_field!(row, "40062")?,
            _40083: map_field!(row, "40083")?,
            _40084: map_field!(row, "40084")?,
        })
    }
}
