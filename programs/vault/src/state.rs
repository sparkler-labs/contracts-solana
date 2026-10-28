use anchor_lang::prelude::*;

/// 金库条目账户（PDA：seeds = [b"vault", letter_id]）。
/// letter_id 由客户端随机生成 32 字节：密码不可从地址推导，暴力破解只能上链试。
#[account]
#[derive(InitSpace)]
pub struct VaultLetter {
    pub letter_id: [u8; 32],
    pub sender: Pubkey,
    /// 展示信息（公开）
    #[max_len(280)]
    pub message: String,
    /// 密码提示词（公开）
    #[max_len(64)]
    pub secret_words: String,
    /// keccak256(sender || password)，与 EVM 版 Vault 算法一致
    pub password_hash: [u8; 32],
    /// Unix 秒；超过后发送方可 timeout_claim 取回
    pub deadline: i64,
    /// 领取付款类型：0=无，1=SOL，2=SPL Token
    pub pay_kind: u8,
    pub pay_mint: Pubkey,
    pub pay_amount: u64,
    pub annex_count: u8,
    pub bump: u8,
}

/// 附件账户（PDA：seeds = [b"annex", letter_id, index]；本程序命名空间内）
#[account]
#[derive(InitSpace)]
pub struct Annex {
    pub letter_id: [u8; 32],
    pub index: u8,
    /// 1=SOL，2=SPL Token，3=NFT
    pub kind: u8,
    pub mint: Pubkey,
    pub amount: u64,
    pub bump: u8,
}

pub const MAX_ANNEXES: usize = 16;
pub const ASSET_KIND_SOL: u8 = 1;
pub const ASSET_KIND_SPL: u8 = 2;
pub const ASSET_KIND_NFT: u8 = 3;
pub const PAY_KIND_NONE: u8 = 0;
pub const PAY_KIND_SOL: u8 = 1;
pub const PAY_KIND_SPL: u8 = 2;

pub const CUSTODY_SEED: &[u8] = b"custody";
