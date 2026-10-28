use anchor_lang::prelude::*;
use anchor_lang::Discriminator;
use anchor_lang::solana_program::{program::invoke_signed, system_instruction};
use anchor_spl::associated_token::{self, AssociatedToken};
use anchor_spl::token::{self, Token, TokenAccount};

mod errors;
mod events;
mod state;

use errors::*;
use events::*;
use state::*;

declare_id!("21Ky8yTtq1sKkts4NPvrfsFnMFrqnJUbXUbC1djS9AZ9");

/// 客户端提交的附件参数
#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct AnnexInput {
    /// 1=SOL，2=SPL Token，3=NFT
    pub kind: u8,
    /// SPL/NFT 的 mint；SOL 为全零占位
    pub mint: Pubkey,
    pub amount: u64,
}

#[program]
pub mod postoffice {
    use super::*;

    /// 寄送包裹：初始化 Letter 与各 Annex PDA、创建托管 ATA、资产转入托管。
    /// 发送方垫付全部 rent；领取/超时退回时托管账户关闭、rent 归还给领取方/发送方。
    ///
    /// remaining_accounts（按附件顺序；SPL/NFT 各 4 个，SOL 无）：
    ///   [sender_ata, mint, custody_ata, annex_pda] × n
    pub fn send_letter<'info>(
        ctx: Context<'_, '_, '_, 'info, SendLetter<'info>>,
        letter_id: [u8; 32],
        deadline: i64,
        pay_kind: u8,
        pay_mint: Pubkey,
        pay_amount: u64,
        annexes: Vec<AnnexInput>,
    ) -> Result<()> {
        require!(
            !annexes.is_empty() && annexes.len() <= MAX_ANNEXES,
            PostOfficeError::TooManyAnnexes
        );
        require!(
            deadline > Clock::get()?.unix_timestamp,
            PostOfficeError::InvalidDeadline
        );
        require!(pay_kind <= PAY_KIND_SPL, PostOfficeError::InvalidAnnex);
        require!(
            pay_kind != PAY_KIND_SPL || pay_mint != Pubkey::default(),
            PostOfficeError::InvalidAnnex
        );
        require!(
            ctx.accounts.letter.to_account_info().data_is_empty(),
            PostOfficeError::AccountAlreadyInitialized
        );

        // 一次性提取 owned 句柄，此后不再触碰 ctx（避免 remaining_accounts
        // 的不变生命周期与各账户借用交叉）
        let sender = ctx.accounts.sender.to_account_info();
        let letter = ctx.accounts.letter.to_account_info();
        let custody = ctx.accounts.custody.to_account_info();
        let token_program = ctx.accounts.token_program.to_account_info();
        let ata_program = ctx.accounts.associated_token_program.to_account_info();
        let system_program = ctx.accounts.system_program.to_account_info();
        let program_id = *ctx.program_id;
        let letter_bump = ctx.bumps.letter;
        let custody_bump = ctx.bumps.custody;
        let sender_key = *ctx.accounts.sender.key;
        let receiver_key = *ctx.accounts.receiver.key;
        let remaining: Vec<AccountInfo> = ctx.remaining_accounts.to_vec();
        let mut remaining = remaining.iter();

        // 填充 Letter（账户在本指令内由 init 创建，直接写数据）
        let letter_data = Letter {
            letter_id,
            sender: sender_key,
            receiver: receiver_key,
            deadline,
            pay_kind,
            pay_mint: if pay_kind == PAY_KIND_SPL { pay_mint } else { Pubkey::default() },
            pay_amount,
            annex_count: annexes.len() as u8,
            bump: letter_bump,
        };
        write_account(&letter, &letter_data)?;

        // SOL 附件：合并转入程序托管的 system account（首次创建 rent-exempt 账户）
        let sol_total: u64 = annexes
            .iter()
            .filter(|a| a.kind == ASSET_KIND_SOL)
            .map(|a| a.amount)
            .sum();
        if sol_total > 0 {
            if custody.lamports() == 0 {
                let custody_rent = Rent::get()?.minimum_balance(0);
                let seeds: &[&[u8]] = &[CUSTODY_SEED, letter_id.as_ref(), &[custody_bump]];
                invoke_signed(
                    &system_instruction::create_account(
                        &sender_key,
                        &custody.key(),
                        custody_rent.saturating_add(sol_total),
                        0,
                        &program_id,
                    ),
                    &[sender.clone(), custody.clone(), system_program.clone()],
                    &[seeds],
                )?;
            } else {
                invoke_signed(
                    &system_instruction::transfer(&sender_key, &custody.key(), sol_total),
                    &[sender.clone(), custody.clone()],
                    &[],
                )?;
            }
        }

        // SPL/NFT 附件：remaining_accounts 四元组 [sender_ata, mint, custody_ata, annex_pda]
        let letter_seeds: &[&[&[u8]]] = &[&[b"letter", letter_id.as_ref(), &[letter_bump]]];
        for (index, input) in annexes.iter().enumerate() {
            require!(
                (ASSET_KIND_SOL..=ASSET_KIND_NFT).contains(&input.kind),
                PostOfficeError::InvalidAnnex
            );
            if input.kind == ASSET_KIND_SOL {
                require!(input.amount > 0, PostOfficeError::InvalidAnnex);
                continue;
            }
            require!(
                input.amount > 0 && input.mint != Pubkey::default(),
                PostOfficeError::InvalidAnnex
            );

            let sender_ata = next_account(&mut remaining)?;
            let mint = next_account(&mut remaining)?;
            let custody_ata = next_account(&mut remaining)?;
            let annex_account = next_account(&mut remaining)?;

            let sender_token = spl_token_account(&sender_ata)?;
            require!(sender_token.mint == *mint.key, PostOfficeError::InvalidAnnex);
            require!(sender_token.owner == sender_key, PostOfficeError::InvalidAnnex);

            // 托管 ATA：authority 为包裹 Letter PDA，地址必须是两者的关联地址
            let (letter_pda, _) =
                Pubkey::find_program_address(&[b"letter", letter_id.as_ref()], &program_id);
            let expected_custody =
                anchor_spl::associated_token::get_associated_token_address(&letter_pda, mint.key);
            require!(*custody_ata.key == expected_custody, PostOfficeError::AccountsMismatch);

            // 幂等创建托管 ATA（Letter PDA 签名）
            let cpi_accounts = associated_token::Create {
                payer: sender.clone(),
                associated_token: custody_ata.clone(),
                authority: letter.clone(),
                mint: mint.clone(),
                system_program: system_program.clone(),
                token_program: token_program.clone(),
            };
            associated_token::create_idempotent(
                CpiContext::new(ata_program.clone(), cpi_accounts).with_signer(letter_seeds),
            )?;

            // 资产转入托管
            let decimals = mint_decimals(&mint)?;
            let cpi_accounts = token::TransferChecked {
                from: sender_ata.clone(),
                mint: mint.clone(),
                to: custody_ata.clone(),
                authority: sender.clone(),
            };
            token::transfer_checked(
                CpiContext::new(token_program.clone(), cpi_accounts),
                input.amount,
                decimals,
            )?;

            // 初始化 Annex PDA（数量动态，手动 init）
            let (expected_annex, annex_bump) = Pubkey::find_program_address(
                &[b"annex", letter_id.as_ref(), &[index as u8]],
                &program_id,
            );
            require!(*annex_account.key == expected_annex, PostOfficeError::AccountsMismatch);
            let rent = Rent::get()?.minimum_balance(8 + Annex::INIT_SPACE);
            let annex_seeds: &[&[u8]] =
                &[b"annex", letter_id.as_ref(), &[index as u8], &[annex_bump]];
            invoke_signed(
                &system_instruction::create_account(
                    &sender_key,
                    &annex_account.key(),
                    rent,
                    (8 + Annex::INIT_SPACE) as u64,
                    &program_id,
                ),
                &[sender.clone(), annex_account.clone(), system_program.clone()],
                &[annex_seeds],
            )?;
            write_account(&annex_account, &Annex {
                letter_id,
                index: index as u8,
                kind: input.kind,
                mint: input.mint,
                amount: input.amount,
                bump: annex_bump,
            })?;
        }
        require!(remaining.next().is_none(), PostOfficeError::AccountsMismatch);

        emit!(events::SendLetter {
            letter_id,
            sender: sender_key,
            receiver: receiver_key,
            deadline,
            pay_kind,
            pay_mint: letter_data.pay_mint,
            pay_amount,
            annexes: annexes
                .iter()
                .map(|a| AnnexInfo { kind: a.kind, mint: a.mint, amount: a.amount })
                .collect(),
        });
        Ok(())
    }

    /// 接收方领取：支付 pay_info → 附件转给接收方 → 关闭托管 ATA 与 Annex 账户，
    /// rent 全部归还接收方（Letter 账户经 close = receiver 由 Anchor 关闭）。
    ///
    /// remaining_accounts（按附件顺序；SPL/NFT 各 4 个，SOL 无）：
    ///   [receiver_ata, mint, custody_ata, annex_pda] × n
    pub fn claim<'info>(
        ctx: Context<'_, '_, '_, 'info, Claim<'info>>,
        letter_id: [u8; 32],
    ) -> Result<()> {
        let letter_info = ctx.accounts.letter.to_account_info();
        let letter: Letter = {
            let data = letter_info.try_borrow_data()?;
            AnchorDeserialize::try_from_slice(&data[8..])?
        };
        require!(
            Clock::get()?.unix_timestamp <= letter.deadline,
            PostOfficeError::Expired
        );
        let letter_bump = ctx.bumps.letter;
        let letter_seeds: &[&[&[u8]]] = &[&[b"letter", letter_id.as_ref(), &[letter_bump]]];

        let receiver = ctx.accounts.receiver.to_account_info();
        let sender_acct = ctx.accounts.sender.to_account_info();
        let custody = ctx.accounts.custody.to_account_info();
        let receiver_pay_ata = ctx.accounts.receiver_pay_ata.to_account_info();
        let sender_pay_ata = ctx.accounts.sender_pay_ata.to_account_info();
        let pay_mint = ctx.accounts.pay_mint.to_account_info();
        let token_program = ctx.accounts.token_program.to_account_info();
        let ata_program = ctx.accounts.associated_token_program.to_account_info();
        let system_program = ctx.accounts.system_program.to_account_info();
        let receiver_key = *ctx.accounts.receiver.key;
        let sender_key = *ctx.accounts.sender.key;
        let remaining: Vec<AccountInfo> = ctx.remaining_accounts.to_vec();
        let mut remaining = remaining.iter();

        // 1. 支付款项（先付款后放货）
        match letter.pay_kind {
            PAY_KIND_NONE => {}
            PAY_KIND_SOL => {
                invoke_signed(
                    &system_instruction::transfer(
                        &receiver_key,
                        &sender_key,
                        letter.pay_amount,
                    ),
                    &[receiver.clone(), sender_acct.clone(), system_program.clone()],
                    &[],
                )?;
            }
            PAY_KIND_SPL => {
                let receiver_pay = spl_token_account(&receiver_pay_ata)?;
                let sender_pay = spl_token_account(&sender_pay_ata)?;
                require!(receiver_pay.owner == receiver_key, PostOfficeError::InsufficientPayment);
                require!(sender_pay.owner == sender_key, PostOfficeError::InsufficientPayment);
                require!(receiver_pay.mint == *pay_mint.key, PostOfficeError::InsufficientPayment);
                let decimals = mint_decimals(&pay_mint)?;
                let cpi_accounts = token::TransferChecked {
                    from: receiver_pay_ata.clone(),
                    mint: pay_mint.clone(),
                    to: sender_pay_ata.clone(),
                    authority: receiver.clone(),
                };
                token::transfer_checked(
                    CpiContext::new(token_program.clone(), cpi_accounts),
                    letter.pay_amount,
                    decimals,
                )?;
            }
            _ => return err!(PostOfficeError::InvalidAnnex),
        }

        // 2. 附件放行给接收方
        while let Some(receiver_ata) = remaining.next() {
            let mint = next_account(&mut remaining)?;
            let custody_ata = next_account(&mut remaining)?;
            let annex_account = next_account(&mut remaining)?;

            // 接收方 ATA 幂等创建（authority = receiver 本尊）
            let cpi_accounts = associated_token::Create {
                payer: receiver.clone(),
                associated_token: receiver_ata.clone(),
                authority: receiver.clone(),
                mint: mint.clone(),
                system_program: system_program.clone(),
                token_program: token_program.clone(),
            };
            associated_token::create_idempotent(CpiContext::new(ata_program.clone(), cpi_accounts))?;

            let custody_token = spl_token_account(&custody_ata)?;
            let decimals = mint_decimals(&mint)?;
            let cpi_accounts = token::TransferChecked {
                from: custody_ata.clone(),
                mint: mint.clone(),
                to: receiver_ata.clone(),
                authority: letter_info.clone(),
            };
            token::transfer_checked(
                CpiContext::new(token_program.clone(), cpi_accounts).with_signer(letter_seeds),
                custody_token.amount,
                decimals,
            )?;
            // 关闭托管 ATA，rent 归还接收方
            let cpi_accounts = token::CloseAccount {
                account: custody_ata.clone(),
                destination: receiver.clone(),
                authority: letter_info.clone(),
            };
            token::close_account(
                CpiContext::new(token_program.clone(), cpi_accounts).with_signer(letter_seeds),
            )?;
            // 关闭 Annex PDA，rent 归还接收方
            close_program_account(&annex_account, &receiver)?;
        }
        require!(remaining.next().is_none(), PostOfficeError::AccountsMismatch);

        // 3. SOL 托管整体转给接收方（程序所有 system account；lamports 清零后
        //    运行时自动回收该账户）
        move_all_lamports(&custody, &receiver)?;

        emit!(events::Claim {
            letter_id,
            claimer: receiver_key,
        });
        Ok(())
    }

    /// 超时取回（仅发送方，deadline 之后）：附件退回发送方，rent 归还发送方。
    ///
    /// remaining_accounts（按附件顺序；SPL/NFT 各 4 个，SOL 无）：
    ///   [sender_ata, mint, custody_ata, annex_pda] × n
    pub fn timeout_claim<'info>(
        ctx: Context<'_, '_, '_, 'info, TimeoutClaim<'info>>,
        letter_id: [u8; 32],
    ) -> Result<()> {
        let letter_info = ctx.accounts.letter.to_account_info();
        let letter: Letter = {
            let data = letter_info.try_borrow_data()?;
            AnchorDeserialize::try_from_slice(&data[8..])?
        };
        require!(
            Clock::get()?.unix_timestamp > letter.deadline,
            PostOfficeError::NotExpired
        );
        let letter_bump = ctx.bumps.letter;
        let letter_seeds: &[&[&[u8]]] = &[&[b"letter", letter_id.as_ref(), &[letter_bump]]];

        let sender = ctx.accounts.sender.to_account_info();
        let custody = ctx.accounts.custody.to_account_info();
        let token_program = ctx.accounts.token_program.to_account_info();
        let ata_program = ctx.accounts.associated_token_program.to_account_info();
        let system_program = ctx.accounts.system_program.to_account_info();
        let remaining: Vec<AccountInfo> = ctx.remaining_accounts.to_vec();
        let mut remaining = remaining.iter();

        while let Some(sender_ata) = remaining.next() {
            let mint = next_account(&mut remaining)?;
            let custody_ata = next_account(&mut remaining)?;
            let annex_account = next_account(&mut remaining)?;

            let cpi_accounts = associated_token::Create {
                payer: sender.clone(),
                associated_token: sender_ata.clone(),
                authority: sender.clone(),
                mint: mint.clone(),
                system_program: system_program.clone(),
                token_program: token_program.clone(),
            };
            associated_token::create_idempotent(CpiContext::new(ata_program.clone(), cpi_accounts))?;

            let custody_token = spl_token_account(&custody_ata)?;
            let decimals = mint_decimals(&mint)?;
            let cpi_accounts = token::TransferChecked {
                from: custody_ata.clone(),
                mint: mint.clone(),
                to: sender_ata.clone(),
                authority: letter_info.clone(),
            };
            token::transfer_checked(
                CpiContext::new(token_program.clone(), cpi_accounts).with_signer(letter_seeds),
                custody_token.amount,
                decimals,
            )?;
            let cpi_accounts = token::CloseAccount {
                account: custody_ata.clone(),
                destination: sender.clone(),
                authority: letter_info.clone(),
            };
            token::close_account(
                CpiContext::new(token_program.clone(), cpi_accounts).with_signer(letter_seeds),
            )?;
            close_program_account(&annex_account, &sender)?;
        }
        require!(remaining.next().is_none(), PostOfficeError::AccountsMismatch);

        move_all_lamports(&custody, &sender)?;

        emit!(events::TimeoutClaim { letter_id });
        Ok(())
    }

    /// 清理已被领取/退回包裹遗留的空托管 ATA（任何人可调，rent 归还调用者）。
    /// Letter PDA 账户虽已关闭，但 PDA 签名按 seeds+bump 重新推导，不受账户状态影响。
    pub fn close_custody(
        ctx: Context<CloseCustody>,
        letter_id: [u8; 32],
        index: u8,
    ) -> Result<()> {
        let custody = spl_token_account(&ctx.accounts.custody_ata)?;
        require!(custody.amount == 0, PostOfficeError::CustodyNotEmpty);

        let (letter_pda, letter_bump) =
            Pubkey::find_program_address(&[b"letter", letter_id.as_ref()], ctx.program_id);
        require!(
            *ctx.accounts.letter_pda.key == letter_pda,
            PostOfficeError::AccountsMismatch
        );
        require!(custody.owner == letter_pda, PostOfficeError::CustodyNotEmpty);
        require!(
            ctx.accounts.annex.data_is_empty(),
            PostOfficeError::CustodyNotEmpty
        );

        let signer: &[&[&[u8]]] = &[&[b"letter", letter_id.as_ref(), &[letter_bump]]];
        let cpi_accounts = token::CloseAccount {
            account: ctx.accounts.custody_ata.to_account_info(),
            destination: ctx.accounts.relayer.to_account_info(),
            authority: ctx.accounts.letter_pda.to_account_info(),
        };
        token::close_account(
            CpiContext::new(ctx.accounts.token_program.to_account_info(), cpi_accounts)
                .with_signer(signer),
        )?;

        emit!(events::CloseCustody { letter_id, index });
        Ok(())
    }
}

// ============================ 上下文 ============================

/// 寄送交易的账户上下文。附件与各 ATA 以 remaining_accounts 传入（数量可变）。
#[derive(Accounts)]
#[instruction(letter_id: [u8; 32])]
pub struct SendLetter<'info> {
    /// 发送方（垫付所有 rent 与转账）
    #[account(mut)]
    pub sender: Signer<'info>,
    /// 接收方（仅记录公钥）
    /// CHECK: 不参与本指令签名校验
    pub receiver: UncheckedAccount<'info>,
    /// 包裹 PDA
    #[account(init, payer = sender, space = 8 + Letter::INIT_SPACE, seeds = [b"letter", letter_id.as_ref()], bump)]
    pub letter: Account<'info, Letter>,
    /// SOL 托管 PDA（程序所有 system account）
    #[account(mut, seeds = [CUSTODY_SEED, letter_id.as_ref()], bump)]
    /// CHECK: 经 system_program::create_account 手动初始化
    pub custody: UncheckedAccount<'info>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

/// 领取交易。支付账户按 pay_kind 提供（SPL：双方 ATA + mint；SOL：仅 sender 收款）。
#[derive(Accounts)]
#[instruction(letter_id: [u8; 32])]
pub struct Claim<'info> {
    /// 接收方（领取人 + 付款人）
    #[account(mut)]
    pub receiver: Signer<'info>,
    /// 发送方（收付款项）
    /// CHECK: 仅作为收款地址，不签名
    #[account(mut)]
    pub sender: UncheckedAccount<'info>,
    /// 包裹 PDA：仅接收方地址匹配可领取；指令结束后关闭、rent 归接收方
    #[account(mut, seeds = [b"letter", letter_id.as_ref()], bump, close = receiver,
        constraint = letter.receiver == *receiver.key @ PostOfficeError::Unauthorized)]
    pub letter: Account<'info, Letter>,
    /// SOL 托管 PDA
    #[account(mut, seeds = [CUSTODY_SEED, letter_id.as_ref()], bump)]
    /// CHECK: 程序所有 system account
    pub custody: UncheckedAccount<'info>,
    /// pay_kind=SPL 时：接收方付款 ATA / 发送方收款 ATA / 付款 mint；否则传系统程序占位
    /// CHECK: 按需校验
    #[account(mut)]
    pub receiver_pay_ata: UncheckedAccount<'info>,
    /// CHECK: 按需校验
    #[account(mut)]
    pub sender_pay_ata: UncheckedAccount<'info>,
    /// CHECK: 按需校验
    pub pay_mint: UncheckedAccount<'info>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

/// 超时取回交易（仅发送方）。
#[derive(Accounts)]
#[instruction(letter_id: [u8; 32])]
pub struct TimeoutClaim<'info> {
    /// 发送方
    #[account(mut, constraint = letter.sender == *sender.key @ PostOfficeError::UnauthorizedSender)]
    pub sender: Signer<'info>,
    /// 包裹 PDA：指令结束后关闭、rent 归发送方
    #[account(mut, seeds = [b"letter", letter_id.as_ref()], bump, close = sender)]
    pub letter: Account<'info, Letter>,
    /// SOL 托管 PDA
    #[account(mut, seeds = [CUSTODY_SEED, letter_id.as_ref()], bump)]
    /// CHECK: 程序所有 system account
    pub custody: UncheckedAccount<'info>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

/// 清理指令：letter_pda / annex 为推导地址的一致性校验位。
#[derive(Accounts)]
#[instruction(letter_id: [u8; 32])]
pub struct CloseCustody<'info> {
    /// 清理发起者（回收 rent）
    #[account(mut)]
    pub relayer: Signer<'info>,
    /// 包裹 PDA 推导地址（账户本身应已关闭）
    /// CHECK: 与 seeds 推导比对
    pub letter_pda: UncheckedAccount<'info>,
    /// 对应 Annex PDA（应已关闭：claim/timeout 中手动关闭）
    /// CHECK: data_is_empty 校验
    pub annex: UncheckedAccount<'info>,
    /// 待清理的空托管 ATA
    /// CHECK: 零余额 + owner 校验
    #[account(mut)]
    pub custody_ata: UncheckedAccount<'info>,
    pub token_program: Program<'info, Token>,
}

// ============================ helpers ============================

/// 解析 SPL Token/ATA 账户为 owned 状态副本（不经 Account<'info>，规避借用生命周期）
    fn spl_token_account(info: &AccountInfo) -> Result<anchor_spl::token::spl_token::state::Account> {
        use anchor_lang::solana_program::program_pack::Pack;
        let data = info.try_borrow_data()?;
        Ok(anchor_spl::token::spl_token::state::Account::unpack(&data)?)
    }

    fn next_account<'i>(
    iter: &mut std::slice::Iter<'_, AccountInfo<'i>>,
) -> Result<AccountInfo<'i>> {
    // AccountInfo 内部为引用计数句柄，clone 廉价；返回 owned 以切断
    // 迭代器借用与元素生命周期的统一约束
    iter.next().cloned().ok_or_else(|| error!(PostOfficeError::AccountsMismatch))
}

/// 读 SPL mint 的 decimals（经 spl_token::Pack 解析，不持有借用跨语句）
fn mint_decimals(mint: &AccountInfo) -> Result<u8> {
    use anchor_lang::solana_program::program_pack::Pack;
    let data = mint.try_borrow_data()?;
    Ok(anchor_spl::token::spl_token::state::Mint::unpack(&data)?.decimals)
}

/// 把 source 的全部 lamports 转给 destination（仅限本程序拥有的账户）
fn move_all_lamports(source: &AccountInfo, destination: &AccountInfo) -> Result<()> {
    let lamports = source.lamports();
    **source.try_borrow_mut_lamports()? = 0;
    let mut dest = destination.try_borrow_mut_lamports()?;
    **dest = (**dest).checked_add(lamports).ok_or(ProgramError::ArithmeticOverflow)?;
    Ok(())
}

/// 关闭本程序所有的 PDA 账户：lamports 转出 + 数据清零（运行时回收账户）
fn close_program_account(account: &AccountInfo, destination: &AccountInfo) -> Result<()> {
    move_all_lamports(account, destination)?;
    let mut data = account.try_borrow_mut_data()?;
    data.fill(0);
    Ok(())
}

/// 写入本程序拥有的账户数据（8 字节判别符 + Borsh 字段）
fn write_account<T: AnchorSerialize + Discriminator>(account: &AccountInfo, data: &T) -> Result<()> {
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(&T::discriminator());
    AnchorSerialize::serialize(data, &mut buf)?;
    let mut acc = account.try_borrow_mut_data()?;
    require!(acc.len() >= buf.len(), PostOfficeError::AccountsMismatch);
    acc[..buf.len()].copy_from_slice(&buf);
    Ok(())
}
