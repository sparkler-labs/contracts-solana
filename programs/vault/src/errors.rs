use anchor_lang::prelude::*;

#[error_code]
pub enum VaultError {
    #[msg("金库账户已初始化（letter_id 已存在）")]
    AccountAlreadyInitialized,
    #[msg("密码不正确")]
    InvalidPassword,
    #[msg("只有创建者可以超时取回")]
    UnauthorizedSender,
    #[msg("尚未超时，不能取回")]
    NotExpired,
    #[msg("已超时，不能再凭密码开启")]
    Expired,
    #[msg("deadline 必须晚于当前时间")]
    InvalidDeadline,
    #[msg("付款金额与要求不符")]
    InsufficientPayment,
    #[msg("附件数量超限（≤16）")]
    TooManyAnnexes,
    #[msg("附件参数不合法（kind/mint/amount）")]
    InvalidAnnex,
    #[msg("remaining_accounts 数量或地址与附件不匹配")]
    AccountsMismatch,
    #[msg("托管账户不满足清理条件")]
    CustodyNotEmpty,
}
