use anchor_lang::prelude::*;

#[error_code]
pub enum PostOfficeError {
    #[msg("包裹账户已初始化（letter_id 已存在）")]
    AccountAlreadyInitialized,
    #[msg("只有包裹接收方可以领取")]
    Unauthorized,
    #[msg("只有包裹发送方可以超时取回")]
    UnauthorizedSender,
    #[msg("包裹尚未超时，不能取回")]
    NotExpired,
    #[msg("包裹已超时，接收方不能再领取")]
    Expired,
    #[msg("寄送 deadline 必须晚于当前时间")]
    InvalidDeadline,
    #[msg("付款金额与包裹要求不符")]
    InsufficientPayment,
    #[msg("附件数量超限（≤16）")]
    TooManyAnnexes,
    #[msg("附件参数不合法（kind/mint/amount）")]
    InvalidAnnex,
    #[msg("remaining_accounts 数量或地址与附件不匹配")]
    AccountsMismatch,
    #[msg("托管账户不满足清理条件（非空或包裹未终结）")]
    CustodyNotEmpty,
}
