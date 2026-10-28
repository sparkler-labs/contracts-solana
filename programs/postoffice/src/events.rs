use anchor_lang::prelude::*;

/// 附件信息（事件负载；后端 ChainFamily::AnchorEvents::ANNEX_SCHEMA 与此布局一一对应）
#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct AnnexInfo {
    /// 1=SOL，2=SPL Token，3=NFT
    pub kind: u8,
    /// SOL 为全零占位
    pub mint: Pubkey,
    pub amount: u64,
}

#[event]
pub struct SendLetter {
    pub letter_id: [u8; 32],
    pub sender: Pubkey,
    pub receiver: Pubkey,
    pub deadline: i64,
    /// 0=无，1=SOL，2=SPL Token
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
