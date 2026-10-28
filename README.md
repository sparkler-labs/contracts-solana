# PostOffice & Vault — Solana Programs

Solana implementation (Rust + [Anchor](https://www.anchor-lang.com/) 0.30+) of
[PostOffice & Vault](https://github.com/sparkler-labs/post-office): encrypted
package delivery and password-protected asset custody.

Feature-parity with the EVM version (`contracts/` in the main repository):
send / claim / timeout-reclaim for packages, and password-gated vault letters.
Asset model mapping: native ETH → SOL, ERC20 → SPL Token, ERC721 / ERC1155 →
Metaplex NFT (supply = 1).

## Programs

| Program   | Address                                       | Seeds                                              |
| --------- | --------------------------------------------- | -------------------------------------------------- |
| postoffice | `21Ky8yTtq1sKkts4NPvrfsFnMFrqnJUbXUbC1djS9AZ9` | `letter`/`annex`/`custody` PDAs per random 32-byte `letter_id` |
| vault      | `CwB99yaBUPmAVx4ae57Dzp1eMtjuJNsss3M6poT9iowe` | `vault`/`annex`/`custody` PDAs per random 32-byte `letter_id` |

Key properties:

- **Random letter IDs** — PDA addresses cannot be derived from passwords or
  participants; collision attempts are rejected deterministically.
- **Sender pays rent** — custody ATAs and letter/annex PDAs are rent-exempt,
  funded by the sender and refunded to the claimer (claim) or sender (timeout).
- **Atomic multi-instruction send** — init letter + annexes + custody ATAs +
  transfers compose into a single transaction.
- **Cross-chain password hashing** — `keccak256(sender_bytes32 ‖ keccak256(raw_password))`,
  same construction as the EVM `Vault.sol`.

## Repository layout

```
programs/postoffice   # package delivery program (Letters + annex custody)
programs/vault        # password-gated custody program
tests/                # anchor test behavioral suites (ts-mocha)
```

## Development

```bash
anchor build            # requires the Solana SBF toolchain (solana CLI + Anchor)
anchor test             # runs tests/ against a local validator
anchor keys list
```

Toolchain (macOS/Linux/WSL; native Windows SBF is not supported):

```bash
avm use 0.30.1
sh -c "$(curl -sSfL https://release.anza.com/stable/install)"
```

## Deployment

```bash
# devnet
anchor deploy --provider.cluster devnet
solana program show <PROGRAM_ID>

# hand upgrade authority to a Squads multisig immediately after deploy
solana program set-upgrade-authority <PROGRAM_ID> --new-upgrade-authority <SQUADS_PDA>

# mainnet-beta
anchor deploy --provider.cluster mainnet --keypair <DEPLOY_KEYPAIR>
```

## License

MIT — see [LICENSE](./LICENSE).
