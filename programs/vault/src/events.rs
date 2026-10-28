use anchor_lang::prelude::*;

/// 附件信息（事件负载；后端 ChainFamily::AnchorEvents::ANNEX_SCHEMA 与此布局一一对应）
#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct AnnexInfo {
    pub kind: u8,
    pub mint: Pubkey,
    pub amount: u64,
}

/// 与 PostOffice 事件同名但字段按金库语义（无 receiver，带 secret_words/message）；
/// 判别符为 sha256("event:SendLetter")[0,8]，后端按程序（合约类型）区分 schema。
#[event]
pub struct SendLetter {
    pub letter_id: [u8; 32],
    pub sender: Pubkey,
    pub deadline: i64,
    pub secret_words: String,
    pub message: String,
    pub pay_kind: u8,
    pub pay_mint: Pubkey,
    pub pay_amount: u64,
    pub annexes: Vec<AnnexInfo>,
}

#[event]
pub struct Claim {
    pub letter_id: [u8; 32],
    pub claimer: Pubkey,
}

#[event]
pub struct TimeoutClaim {
    pub letter_id: [u8; 32],
}

#[event]
pub struct CloseCustody {
    pub letter_id: [u8; 32],
    pub index: u8,
}
