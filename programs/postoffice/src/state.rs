use anchor_lang::prelude::*;

/// 包裹账户（PDA：seeds = [b"letter", letter_id]）。
/// letter_id 由客户端随机生成 32 字节，切断 PDA 地址与密码/收件人的推导关系。
#[account]
#[derive(InitSpace)]
pub struct Letter {
    /// 随机 32 字节包裹标识（重复寄送即可重新推导 seeds，便于账户复用校验）
    pub letter_id: [u8; 32],
    pub sender: Pubkey,
    pub receiver: Pubkey,
    /// Unix 秒；超过后发送方可 timeout_claim 取回
    pub deadline: i64,
    /// 领取付款类型：0=无，1=SOL，2=SPL Token
    pub pay_kind: u8,
    /// pay_kind=2 时的 SPL mint；否则为全零占位
    pub pay_mint: Pubkey,
    /// 付款数量（最小单位：lamports / mint 最小分割）
    pub pay_amount: u64,
    /// 附件数（≤ MAX_ANNEXES）
    pub annex_count: u8,
    pub bump: u8,
}

/// 附件账户（PDA：seeds = [b"annex", letter_id, index]）。
/// 每件附件独立 PDA：附件数无上限，且 SPL/NFT 托管 ATA 需独立 authority 推导。
#[account]
#[derive(InitSpace)]
pub struct Annex {
    pub letter_id: [u8; 32],
    pub index: u8,
    /// 资产类型：1=SOL，2=SPL Token，3=NFT（Metaplex，supply=1）
    pub kind: u8,
    /// SPL/NFT 的 mint；SOL 为全零占位
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

/// SOL 托管账户前缀（seeds = [b"custody", letter_id]），
/// 程序拥有的 system account，lamports 即托管资产。
pub const CUSTODY_SEED: &[u8] = b"custody";
