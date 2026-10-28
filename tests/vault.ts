import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { PublicKey, SystemProgram, Keypair, LAMPORTS_PER_SOL } from "@solana/web3.js";
import { assert } from "chai";
import { keccak256 } from "js-sha3";
import { Vault } from "../target/types/vault";

/**
 * Vault 程序行为契约测试：正确密码领取 / 错误密码拒绝（InvalidPassword）/
 * 超时取回，密码哈希算法 keccak256(sender || password) 与 EVM 版一致。
 */
describe("vault", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  const program = anchor.workspace.Vault as Program<Vault>;

  const creator = Keypair.generate();
  const claimer = Keypair.generate();

  const letterId = Array.from(Keypair.generate().secretKey.slice(0, 32));
  const [vaultPda] = PublicKey.findProgramAddressSync(
    [Buffer.from("vault"), Buffer.from(letterId)], program.programId);
  const [custodyPda] = PublicKey.findProgramAddressSync(
    [Buffer.from("custody"), Buffer.from(letterId)], program.programId);

  // 与 EVM 客户端一致：password 为 keccak256(原始口令) 的 32 字节
  const password = new Uint8Array(keccak256.arrayBuffer("open-sesame-2026"));
  const passwordHash = new Uint8Array(
    keccak256.arrayBuffer(concatBytes(creator.publicKey.toBytes(), password))
  );

  function concatBytes(a: Uint8Array, b: Uint8Array): Uint8Array {
    const out = new Uint8Array(a.length + b.length);
    out.set(a, 0);
    out.set(b, a.length);
    return out;
  }

  const deadline = Math.floor(Date.now() / 1000) + 3600;

  before(async () => {
    await provider.connection.requestAirdrop(creator.publicKey, 10 * LAMPORTS_PER_SOL);
    await provider.connection.requestAirdrop(claimer.publicKey, LAMPORTS_PER_SOL);
  });

  const sendAccounts = () => ({
    sender: creator.publicKey,
    vaultLetter: vaultPda,
    custody: custodyPda,
    tokenProgram: TOKEN_PROGRAM_ID,
    associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
    systemProgram: SystemProgram.programId,
  });

  it("send_letter：密码哈希上链 + SOL 附件托管", async () => {
    await program.methods
      .sendLetter(
        letterId,
        "my secret message",
        "hint: darkest secret",
        Array.from(password),
        new anchor.BN(deadline),
        0, SystemProgram.programId, new anchor.BN(0),
        [{ kind: 1, mint: SystemProgram.programId, amount: new anchor.BN(0.2 * LAMPORTS_PER_SOL) }]
      )
      .accounts(sendAccounts())
      .remainingAccounts([])
      .signers([creator])
      .rpc();

    const stored = await program.account.vaultLetter.fetch(vaultPda);
    assert.deepEqual(Array.from(stored.passwordHash), Array.from(passwordHash));
    assert.equal(stored.secretWords, "hint: darkest secret");
  });

  it("错误密码被拒绝（InvalidPassword）", async () => {
    const wrong = new Uint8Array(keccak256.arrayBuffer("wrong-password"));
    try {
      await program.methods
        .claim(letterId, Array.from(wrong))
        .accounts({
          claimer: claimer.publicKey,
          sender: creator.publicKey,
          vaultLetter: vaultPda,
          custody: custodyPda,
          receiverPayAta: SystemProgram.programId,
          senderPayAta: SystemProgram.programId,
          payMint: SystemProgram.programId,
          tokenProgram: TOKEN_PROGRAM_ID,
          associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
        })
        .remainingAccounts([])
        .signers([claimer])
        .rpc();
      assert.fail("错误密码不应领取成功");
    } catch (err) {
      assert.include(err.toString(), "InvalidPassword");
    }
  });

  it("正确密码领取：任意持密码者可开（SOL 托管释放）", async () => {
    const before = await provider.connection.getBalance(claimer.publicKey);
    await program.methods
      .claim(letterId, Array.from(password))
      .accounts({
        claimer: claimer.publicKey,
        sender: creator.publicKey,
        vaultLetter: vaultPda,
        custody: custodyPda,
        receiverPayAta: SystemProgram.programId,
        senderPayAta: SystemProgram.programId,
        payMint: SystemProgram.programId,
        tokenProgram: TOKEN_PROGRAM_ID,
        associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      })
      .remainingAccounts([])
      .signers([claimer])
      .rpc();
    const after = await provider.connection.getBalance(claimer.publicKey);
    assert.isAbove(after, before, "开启人应收到托管的 SOL");
  });

  it("timeout_claim：非创建者拒绝；超时后创建者取回", async () => {
    const letterId2 = Array.from(Keypair.generate().secretKey.slice(0, 32));
    const [vaultPda2] = PublicKey.findProgramAddressSync(
      [Buffer.from("vault"), Buffer.from(letterId2)], program.programId);
    const [custodyPda2] = PublicKey.findProgramAddressSync(
      [Buffer.from("custody"), Buffer.from(letterId2)], program.programId);
    const past = Math.floor(Date.now() / 1000) - 10;

    await program.methods
      .sendLetter(
        letterId2, "expired", "hint2",
        Array.from(password), new anchor.BN(past),
        0, SystemProgram.programId, new anchor.BN(0),
        [{ kind: 1, mint: SystemProgram.programId, amount: new anchor.BN(0.1 * LAMPORTS_PER_SOL) }]
      )
      .accounts({ ...sendAccounts(), vaultLetter: vaultPda2, custody: custodyPda2 })
      .remainingAccounts([])
      .signers([creator])
      .rpc();

    const stranger = Keypair.generate();
    await provider.connection.requestAirdrop(stranger.publicKey, LAMPORTS_PER_SOL);
    try {
      await program.methods
        .timeoutClaim(letterId2)
        .accounts({ sender: stranger.publicKey, vaultLetter: vaultPda2, custody: custodyPda2,
          tokenProgram: TOKEN_PROGRAM_ID, associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
          systemProgram: SystemProgram.programId })
        .remainingAccounts([])
        .signers([stranger])
        .rpc();
      assert.fail("非创建者不应能取回");
    } catch (err) {
      // 未超时校验先于 owner 校验不成立：已超时，故应报 UnauthorizedSender
      assert.isTrue(err.toString().includes("UnauthorizedSender") || err.toString().includes("signature"));
    }

    const before = await provider.connection.getBalance(creator.publicKey);
    await program.methods
      .timeoutClaim(letterId2)
      .accounts({ sender: creator.publicKey, vaultLetter: vaultPda2, custody: custodyPda2,
        tokenProgram: TOKEN_PROGRAM_ID, associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId })
      .remainingAccounts([])
      .signers([creator])
      .rpc();
    const after = await provider.connection.getBalance(creator.publicKey);
    assert.isAbove(after, before);
  });
});

import { TOKEN_PROGRAM_ID, ASSOCIATED_TOKEN_PROGRAM_ID } from "@solana/spl-token";
