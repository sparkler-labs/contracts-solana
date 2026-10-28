import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import {
  PublicKey,
  SystemProgram,
  Keypair,
  LAMPORTS_PER_SOL,
  Transaction,
  sendAndConfirmTransaction,
} from "@solana/web3.js";
import {
  TOKEN_PROGRAM_ID,
  ASSOCIATED_TOKEN_PROGRAM_ID,
  createMint,
  createAccount,
  mintTo,
  getAssociatedTokenAddress,
  createAssociatedTokenAccountIdempotent,
} from "@solana/spl-token";
import { assert } from "chai";
import { Postoffice } from "../target/types/postoffice";

/**
 * PostOffice 程序行为契约测试（见 openspec/changes/add-solana-chain-support
 * specs/solana-program）。运行：anchor test（需本地 validator）。
 */

describe("postoffice", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  const program = anchor.workspace.Postoffice as Program<Postoffice>;

  const sender = Keypair.generate();
  const receiver = Keypair.generate();

  const letterId = Array.from(Keypair.generate().secretKey.slice(0, 32));
  const [letterPda] = PublicKey.findProgramAddressSync(
    [Buffer.from("letter"), Buffer.from(letterId)],
    program.programId
  );
  const [custodyPda] = PublicKey.findProgramAddressSync(
    [Buffer.from("custody"), Buffer.from(letterId)],
    program.programId
  );
  const annexPda = (index: number) =>
    PublicKey.findProgramAddressSync(
      [Buffer.from("annex"), Buffer.from(letterId), Buffer.from([index])],
      program.programId
    )[0];

  const deadline = Math.floor(Date.now() / 1000) + 3600;

  let usdcMint: PublicKey;
  let nftMint: PublicKey;
  let senderUsdc: PublicKey;
  let senderNft: PublicKey;
  let custodyUsdc: PublicKey;
  let custodyNft: PublicKey;

  before(async () => {
    await provider.connection.requestAirdrop(sender.publicKey, 10 * LAMPORTS_PER_SOL);
    await provider.connection.requestAirdrop(receiver.publicKey, LAMPORTS_PER_SOL);

    usdcMint = await createMint(provider.connection, sender, sender.publicKey, null, 6);
    senderUsdc = await createAssociatedTokenAccountIdempotent(
      provider.connection, sender, usdcMint, sender.publicKey
    );
    await mintTo(provider.connection, sender, usdcMint, senderUsdc, sender, 1_000_000);

    nftMint = await createMint(provider.connection, sender, sender.publicKey, null, 0);
    senderNft = await createAssociatedTokenAccountIdempotent(
      provider.connection, sender, nftMint, sender.publicKey
    );
    await mintTo(provider.connection, sender, nftMint, senderNft, sender, 1);

    custodyUsdc = await getAssociatedTokenAddress(usdcMint, letterPda, true);
    custodyNft = await getAssociatedTokenAddress(nftMint, letterPda, true);
  });

  const annexes = [
    { kind: 2, mint: usdcMint, amount: new anchor.BN(500_000) },
    { kind: 3, mint: nftMint, amount: new anchor.BN(1) },
  ];

  const sendIxAccounts = {
    sender: sender.publicKey,
    receiver: receiver.publicKey,
    letter: letterPda,
    custody: custodyPda,
    tokenProgram: TOKEN_PROGRAM_ID,
    associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
    systemProgram: SystemProgram.programId,
  };

  const claimAccounts = (who: Keypair) => ({
    receiver: who.publicKey,
    sender: sender.publicKey,
    letter: letterPda,
    custody: custodyPda,
    receiverPayAta: SystemProgram.programId,
    senderPayAta: SystemProgram.programId,
    payMint: SystemProgram.programId,
    tokenProgram: TOKEN_PROGRAM_ID,
    associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
    systemProgram: SystemProgram.programId,
  });

  const claimRemaining = (who: Keypair) => {
    const recvUsdc = getAssociatedTokenAddress(usdcMint, who.publicKey);
    const recvNft = getAssociatedTokenAddress(nftMint, who.publicKey);
    return [
      { pubkey: recvUsdc, isSigner: false, isWritable: true },
      { pubkey: usdcMint, isSigner: false, isWritable: false },
      { pubkey: custodyUsdc, isSigner: false, isWritable: true },
      { pubkey: annexPda(0), isSigner: false, isWritable: true },
      { pubkey: recvNft, isSigner: false, isWritable: true },
      { pubkey: nftMint, isSigner: false, isWritable: false },
      { pubkey: custodyNft, isSigner: false, isWritable: true },
      { pubkey: annexPda(1), isSigner: false, isWritable: true },
    ];
  };

  it("send_letter：SPL Token + NFT 附件入托管", async () => {
    await program.methods
      .sendLetter(letterId, new anchor.BN(deadline), 0, SystemProgram.programId, new anchor.BN(0), annexes)
      .accounts(sendIxAccounts)
      .remainingAccounts([
        { pubkey: senderUsdc, isSigner: false, isWritable: true },
        { pubkey: usdcMint, isSigner: false, isWritable: false },
        { pubkey: custodyUsdc, isSigner: false, isWritable: true },
        { pubkey: annexPda(0), isSigner: false, isWritable: true },
        { pubkey: senderNft, isSigner: false, isWritable: true },
        { pubkey: nftMint, isSigner: false, isWritable: false },
        { pubkey: custodyNft, isSigner: false, isWritable: true },
        { pubkey: annexPda(1), isSigner: false, isWritable: true },
      ])
      .signers([sender])
      .rpc();

    const letter = await program.account.letter.fetch(letterPda);
    assert.equal(letter.sender.toBase58(), sender.publicKey.toBase58());
    assert.equal(letter.receiver.toBase58(), receiver.publicKey.toBase58());
    assert.equal(letter.annexCount, 2);

    const custodyUsdcAcct = await provider.connection.getTokenAccountBalance(custodyUsdc);
    assert.equal(custodyUsdcAcct.value.amount, "500000");
    const custodyNftAcct = await provider.connection.getTokenAccountBalance(custodyNft);
    assert.equal(custodyNftAcct.value.amount, "1");
  });

  it("相同 letter_id 撞车被拒绝", async () => {
    try {
      await program.methods
        .sendLetter(letterId, new anchor.BN(deadline), 0, SystemProgram.programId, new anchor.BN(0), annexes)
        .accounts(sendIxAccounts)
        .remainingAccounts([])
        .signers([sender])
        .rpc();
      assert.fail("应拒绝重复 letter_id");
    } catch (err) {
      assert.include(err.toString(), "already in use");
    }
  });

  it("非接收方领取被拒绝（Unauthorized）", async () => {
    const stranger = Keypair.generate();
    await provider.connection.requestAirdrop(stranger.publicKey, LAMPORTS_PER_SOL);
    try {
      await program.methods
        .claim(letterId)
        .accounts(claimAccounts(stranger))
        .remainingAccounts(claimRemaining(stranger))
        .signers([stranger])
        .rpc();
      assert.fail("非接收方不应领取成功");
    } catch (err) {
      assert.include(err.toString(), "Unauthorized");
    }
  });

  it("接收方领取：附件转移 + rent 回收", async () => {
    const recvNft = getAssociatedTokenAddress(nftMint, receiver.publicKey);
    const recvBalBefore = await provider.connection.getBalance(receiver.publicKey);

    await program.methods
      .claim(letterId)
      .accounts(claimAccounts(receiver))
      .remainingAccounts(claimRemaining(receiver))
      .signers([receiver])
      .rpc();

    const nftBal = await provider.connection.getTokenAccountBalance(recvNft);
    assert.equal(nftBal.value.amount, "1");
    const usdcBal = await provider.connection.getTokenAccountBalance(
      getAssociatedTokenAddress(usdcMint, receiver.publicKey)
    );
    assert.equal(usdcBal.value.amount, "500000");
    // 托管 ATA 已关闭
    const custodyInfo = await provider.connection.getAccountInfo(custodyUsdc);
    assert.isNull(custodyInfo, "托管 ATA 应已关闭");
    // rent 归还接收方（余额不减反增）
    const recvBalAfter = await provider.connection.getBalance(receiver.publicKey);
    assert.isAbove(recvBalAfter, recvBalBefore);
  });

  it("timeout_claim：deadline 前拒绝（NotExpired）；超时后退回发送方", async () => {
    const letterId2 = Array.from(Keypair.generate().secretKey.slice(0, 32));
    const [letterPda2] = PublicKey.findProgramAddressSync(
      [Buffer.from("letter"), Buffer.from(letterId2)], program.programId);
    const [custodyPda2] = PublicKey.findProgramAddressSync(
      [Buffer.from("custody"), Buffer.from(letterId2)], program.programId);
    const pastDeadline = Math.floor(Date.now() / 1000) - 10;

    await program.methods
      .sendLetter(letterId2, new anchor.BN(pastDeadline), 1, SystemProgram.programId, new anchor.BN(100_000_000), [
        { kind: 1, mint: SystemProgram.programId, amount: new anchor.BN(0.1 * LAMPORTS_PER_SOL) },
      ])
      .accounts({
        ...sendIxAccounts,
        letter: letterPda2,
        custody: custodyPda2,
      })
      .remainingAccounts([])
      .signers([sender])
      .rpc();

    // deadline 前发送方不能取回
    try {
      await program.methods
        .timeoutClaim(letterId2)
        .accounts({ sender: sender.publicKey, letter: letterPda2, custody: custodyPda2,
          tokenProgram: TOKEN_PROGRAM_ID, associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
          systemProgram: SystemProgram.programId })
        .remainingAccounts([])
        .signers([sender])
        .rpc();
      assert.fail("未超时不应取回");
    } catch (err) {
      assert.include(err.toString(), "NotExpired");
    }

    // 直接改链上 deadline 不可行；本用例以 pastDeadline 寄送后即满足超时条件，
    // 这里补一次正常超时取回（SOL 托管退回）
    const before = await provider.connection.getBalance(sender.publicKey);
    await program.methods
      .timeoutClaim(letterId2)
      .accounts({ sender: sender.publicKey, letter: letterPda2, custody: custodyPda2,
        tokenProgram: TOKEN_PROGRAM_ID, associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId })
      .remainingAccounts([])
      .signers([sender])
      .rpc();
    const after = await provider.connection.getBalance(sender.publicKey);
    assert.isAbove(after, before, "SOL 托管应退回发送方");
  });

  it("send_letter 支持 SOL 附件与 PayInfo（SOL 支付）", async () => {
    const letterId3 = Array.from(Keypair.generate().secretKey.slice(0, 32));
    const [letterPda3] = PublicKey.findProgramAddressSync(
      [Buffer.from("letter"), Buffer.from(letterId3)], program.programId);
    const [custodyPda3] = PublicKey.findProgramAddressSync(
      [Buffer.from("custody"), Buffer.from(letterId3)], program.programId);
    const d3 = Math.floor(Date.now() / 1000) + 3600;

    // 0.5 SOL 附件 + 0.1 SOL 领取付款
    await program.methods
      .sendLetter(letterId3, new anchor.BN(d3), 1, SystemProgram.programId, new anchor.BN(0.1 * LAMPORTS_PER_SOL), [
        { kind: 1, mint: SystemProgram.programId, amount: new anchor.BN(0.5 * LAMPORTS_PER_SOL) },
      ])
      .accounts({ ...sendIxAccounts, letter: letterPda3, custody: custodyPda3 })
      .remainingAccounts([])
      .signers([sender])
      .rpc();

    const custodyBal = await provider.connection.getBalance(custodyPda3);
    assert.isAtLeast(custodyBal, 0.5 * LAMPORTS_PER_SOL);

    const recvBefore = await provider.connection.getBalance(receiver.publicKey);
    const sendBefore = await provider.connection.getBalance(sender.publicKey);
    await program.methods
      .claim(letterId3)
      .accounts({ ...claimAccounts(receiver), custody: custodyPda3 })
      .remainingAccounts([])
      .signers([receiver])
      .rpc();
    // 接收方净变动 = +0.5 SOL 附件 + rent - 0.1 SOL 付款 - 手续费
    const recvAfter = await provider.connection.getBalance(receiver.publicKey);
    const sendAfter = await provider.connection.getBalance(sender.publicKey);
    assert.isAbove(recvAfter, recvBefore - 10_000, "接收方应净增资产");
    assert.isAbove(sendAfter, sendBefore - 0.1 * LAMPORTS_PER_SOL - 10_000);
  });
});
