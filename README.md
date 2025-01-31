# Creator Support Program

A Solana program that enables fans to support creators through staking and rewards. Built with Anchor framework.

## Features

- 🎯 Split support: 70% direct, 30% staked
- 💰 Dynamic APR-based rewards for stakers
- 🤝 Dual rewards: 60% to supporters, 40% to creators
- 📈 Unique supporter tracking
- 🔒 Secure vault system for staked tokens

## Quick Start
```bash
### Install dependencies
npm install
### Build program
anchor build
### Run tests
anchor test
```

## Architecture

The program uses PDAs (Program Derived Addresses) for:
- Creator accounts
- Stake tracking
- Vault management
- Reward minting authority

See `programs/creator_support/src/architecture.mmd` for detailed flow.

## Security

- ✓ Safe arithmetic operations
- ✓ Minimum stake requirements
- ✓ PDA-based authority control
- ✓ Full test coverage

## Technical Details

- **Token Standards**: SPL Token
- **Framework**: Anchor v0.30.1
- **Testing**: Local Validator Environment
- **Reward Calculation**: Time-based APR with decimal precision

## Development

The program implements a staking and reward system where:
1. Supporters can stake tokens to creators
2. Rewards are calculated based on stake duration and APR
3. Both creators and supporters earn from the staking rewards
4. Unstaking has a 60% limit to maintain program stability

## License

MIT