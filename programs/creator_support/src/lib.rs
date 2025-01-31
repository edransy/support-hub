use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer, MintTo};

declare_id!("Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS");

#[error_code]
pub enum ErrorCode {
    #[msg("Overflow occurred.")]
    Overflow,
    #[msg("Invalid argument provided.")]
    InvalidArgument,
    #[msg("Invalid percentage.")]
    InvalidPercentage,
    #[msg("Stake amount is too small.")]
    StakeTooSmall,
    #[msg("No rewards available to claim.")]
    NoRewardsAvailable,
    #[msg("Vault Account already initialized.")]
    VaultAlreadyInitialized,
    #[msg("Unstake amount exceeds maximum allowed.")]
    UnstakeAmountTooLarge,
    #[msg("Claim is too early.")]
    ClaimTooEarly,
}

#[program]
pub mod creator_support {
    use super::*;

    /// Initializes the CreatorSupport account with program parameters.
    pub fn initialize(
        ctx: Context<Initialize>, 
        price_per_impact: u64,
        max_reward_multiplier: u64,
        scaling_factor: u64,
        apr: u64,
        supporter_reward_ratio: u64,
        min_stake_amount: u64,
    ) -> Result<()> {
        let creator_support = &mut ctx.accounts.creator_support;
        creator_support.price_per_impact = price_per_impact;
        creator_support.admin = *ctx.accounts.admin.key;
        creator_support.max_reward_multiplier = max_reward_multiplier;
        creator_support.scaling_factor = scaling_factor;
        creator_support.apr = apr;
        creator_support.supporter_reward_ratio = supporter_reward_ratio;
        creator_support.reward_mint = ctx.accounts.reward_mint.key();
        creator_support.stablecoin_mint = ctx.accounts.stablecoin_mint.key();
        creator_support.min_stake_amount = min_stake_amount;
        Ok(())
    }

    /// Initializes the Creator account.
    pub fn initialize_creator(ctx: Context<InitializeCreator>) -> Result<()> {
        let creator = &mut ctx.accounts.creator;
        creator.registration_time = Clock::get()?.unix_timestamp;
        creator.exists = true;
        creator.total_supporters = 0;
        creator.total_support_amount = 0;
        creator.total_staked = 0;
        creator.last_reward_calculation_time = Clock::get()?.unix_timestamp;
        creator.accumulated_rewards = 0;
        Ok(())
    }

    /// Initializes the Vault Token Account owned by the Vault PDA.
    pub fn initialize_vault(_ctx: Context<InitializeVault>) -> Result<()> {
        // The account initialization is handled by Anchor's constraints
        Ok(())
    }

    /// Allows a supporter to support a creator by transferring stablecoins.
    pub fn support_creator(
        ctx: Context<SupportCreator>,
        stablecoin_amount: u64,
    ) -> Result<()> {
        let creator_support = &ctx.accounts.creator_support;
        let creator = &mut ctx.accounts.creator;
        let supporter = &ctx.accounts.supporter;

        // Split allocation: 70% immediate, 30% staked
        let immediate_allocation = stablecoin_amount
            .checked_mul(70)
            .ok_or(ErrorCode::InvalidPercentage)?
            .checked_div(100)
            .ok_or(ErrorCode::InvalidPercentage)?;

        let staked_allocation = stablecoin_amount
            .checked_sub(immediate_allocation)
            .ok_or(ErrorCode::InvalidPercentage)?;

        // Update creator's total support
        creator.total_support_amount = creator.total_support_amount
            .checked_add(immediate_allocation)
            .ok_or(ErrorCode::Overflow)?;

        // Process staking
        require!(staked_allocation > 0, ErrorCode::StakeTooSmall);
        require!(
            stablecoin_amount >= creator_support.min_stake_amount,
            ErrorCode::StakeTooSmall
        );

        // Initialize or update supporter stake
        let stake = &mut ctx.accounts.supporter_stake;
        stake.supporter = supporter.key();
        stake.creator = creator.key();
        // stake.reward_cooldown = 86400; // 1 day cooldown in seconds

        // Check if this is a new supporter BEFORE updating stake
        let is_new_supporter = stake.staked_amount == 0;

        // Update stake amount
        stake.staked_amount = stake.staked_amount
            .checked_add(staked_allocation)
            .ok_or(ErrorCode::Overflow)?;
        stake.stake_start_time = Clock::get()?.unix_timestamp;
        stake.last_claim_time = Clock::get()?.unix_timestamp;

        // Update creator's total staked
        creator.total_staked = creator.total_staked
            .checked_add(staked_allocation)
            .ok_or(ErrorCode::Overflow)?;

        // Impact calculation remains (could be more complex based on requirements)
        let _impact_coins = creator_support.calculate_reward_multiplier(creator.total_supporters);

        // Transfer 70% to creator's stablecoin account
        let transfer_to_creator_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.supporter_stablecoin_account.to_account_info(),
                to: ctx.accounts.creator_stablecoin_account.to_account_info(),
                authority: ctx.accounts.supporter.to_account_info(),
            },
        );
        
        token::transfer(transfer_to_creator_ctx, immediate_allocation)?;

        // Transfer 30% to vault's stablecoin account
        let transfer_to_vault_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.supporter_stablecoin_account.to_account_info(),
                to: ctx.accounts.vault_account.to_account_info(),
                authority: ctx.accounts.supporter.to_account_info(),
            },
        );
        
        token::transfer(transfer_to_vault_ctx, staked_allocation)?;

        // Increment unique supporters count if new
        if is_new_supporter {
            creator.total_unique_supporters = creator.total_unique_supporters
                .checked_add(1)
                .ok_or(ErrorCode::Overflow)?;
        }

        Ok(())
    }

    /// Allows a supporter to claim their rewards based on staking.
    pub fn claim_rewards(ctx: Context<ClaimRewards>) -> Result<()> {
        let creator_support = &ctx.accounts.creator_support;
        let stake = &mut ctx.accounts.supporter_stake;
        let creator = &mut ctx.accounts.creator;

        let current_time = Clock::get()?.unix_timestamp;
        let time_elapsed = current_time - stake.last_claim_time;

        // TODO: Add cooldown check
        // require!(
        //     time_elapsed >= stake.reward_cooldown,
        //     ErrorCode::ClaimTooEarly
        // );

        // Calculate rewards (simplified; adjust formula as needed)
        let annual_seconds = 31536000u64; // 365 days
        let reward_amount = (stake.staked_amount as u128)
            .checked_mul(creator_support.apr as u128)
            .ok_or(ErrorCode::Overflow)?
            .checked_mul(time_elapsed as u128)
            .ok_or(ErrorCode::Overflow)?
            .checked_mul(1_000_000) // Scale up by decimals before division
            .ok_or(ErrorCode::Overflow)?
            .checked_div(annual_seconds as u128)
            .ok_or(ErrorCode::Overflow)?
            .checked_div(100) // APR percentage
            .ok_or(ErrorCode::Overflow)? as u64;

        msg!("Reward calculation:");
        msg!("Staked amount: {}", stake.staked_amount);
        msg!("APR: {}", creator_support.apr);
        msg!("Time elapsed: {}", time_elapsed);
        msg!("Annual seconds: {}", annual_seconds);
        msg!("Calculated reward: {}", reward_amount);

        // Split rewards: 60% to supporter, 40% to creator
        let supporter_reward = reward_amount
            .checked_mul(60)
            .ok_or(ErrorCode::Overflow)?
            .checked_div(100)
            .ok_or(ErrorCode::Overflow)?;
        
        let creator_reward = reward_amount
            .checked_sub(supporter_reward)
            .ok_or(ErrorCode::Overflow)?;

        // Update stake and creator balances
        stake.last_claim_time = current_time;
        creator.accumulated_rewards = creator.accumulated_rewards
            .checked_add(creator_reward)
            .ok_or(ErrorCode::Overflow)?;

        // Mint supporter rewards
        let auth_seeds = [b"mint_auth" as &[u8], &[ctx.bumps.mint_authority]];
        let auth_signer = &[&auth_seeds[..]];

        let mint_supporter_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            MintTo {
                mint: ctx.accounts.reward_mint.to_account_info(),
                to: ctx.accounts.supporter_reward_account.to_account_info(),
                authority: ctx.accounts.mint_authority.to_account_info(),
            },
            auth_signer
        );
        
        token::mint_to(mint_supporter_ctx, supporter_reward)?;

        // Mint creator rewards using same auth_signer
        let mint_creator_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            MintTo {
                mint: ctx.accounts.reward_mint.to_account_info(),
                to: ctx.accounts.creator_reward_account.to_account_info(),
                authority: ctx.accounts.mint_authority.to_account_info(),
            },
            auth_signer
        );
        
        token::mint_to(mint_creator_ctx, creator_reward)?;

        Ok(())
    }

    /// Allows a supporter to unstake their available portion of staked tokens
    pub fn unstake(ctx: Context<Unstake>, amount: u64) -> Result<()> {
        let stake = &mut ctx.accounts.supporter_stake;
        let creator = &mut ctx.accounts.creator;
        let creator_support = &ctx.accounts.creator_support;

        // Calculate maximum unstakeable amount (60% of staked amount)
        let max_unstakeable = stake.staked_amount
            .checked_mul(creator_support.supporter_reward_ratio)
            .ok_or(ErrorCode::Overflow)?
            .checked_div(100)
            .ok_or(ErrorCode::Overflow)?;

        // Verify unstake amount
        require!(amount <= max_unstakeable, ErrorCode::UnstakeAmountTooLarge);
        require!(amount > 0, ErrorCode::InvalidArgument);

        // Update stake amount
        stake.staked_amount = stake.staked_amount
            .checked_sub(amount)
            .ok_or(ErrorCode::Overflow)?;

        // Update creator's total staked
        creator.total_staked = creator.total_staked
            .checked_sub(amount)
            .ok_or(ErrorCode::Overflow)?;

        // Transfer tokens from vault to supporter
        let vault_auth_seeds = &[
            b"vault",
            ctx.accounts.creator.to_account_info().key.as_ref(),
            &[ctx.bumps.vault_pda],
        ];
        let vault_signer = &[&vault_auth_seeds[..]];

        let transfer_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.vault_account.to_account_info(),
                to: ctx.accounts.supporter_stablecoin_account.to_account_info(),
                authority: ctx.accounts.vault_pda.to_account_info(),
            },
            vault_signer,
        );
        
        token::transfer(transfer_ctx, amount)?;

        Ok(())
    }

    /// Advances the clock (for testing purposes).
    pub fn advance_time(_ctx: Context<AdvanceTime>, _seconds: u64) -> Result<()> {
        // This is a mock function. In reality, on-chain programs can't manipulate the clock.
        Ok(())
    }
}

#[account]
pub struct Creator {
    pub registration_time: i64,
    pub exists: bool,
    pub total_supporters: u32,
    pub total_support_amount: u64,
    pub total_staked: u64,
    pub last_reward_calculation_time: i64,
    pub accumulated_rewards: u64,
    pub total_unique_supporters: u32,
}

impl Creator {
    pub const INIT_SPACE: usize = 8 + 8 + 1 + 4 + 8 + 8 + 8 + 8 + 4; // discriminator + fields
}

#[account]
pub struct CreatorSupport {
    pub price_per_impact: u64,
    pub admin: Pubkey,
    pub max_reward_multiplier: u64,
    pub scaling_factor: u64,
    pub apr: u64,                  // Annual Percentage Rate (in basis points)
    pub supporter_reward_ratio: u64, // Percentage (0-100) for supporter rewards
    pub reward_mint: Pubkey,
    pub stablecoin_mint: Pubkey,
    pub min_stake_amount: u64,  // Minimum stake amount
}

impl CreatorSupport {
    pub const INIT_SPACE: usize = 8 + 32 + 8 + 8 + 8 + 8 + 32 + 32 + 8; // 144 bytes

    /// Calculates the reward multiplier based on the number of supporters.
    fn calculate_reward_multiplier(&self, num_supporters: u32) -> u64 {
        let x = num_supporters as f64;
        let max = self.max_reward_multiplier as f64 / 100.0;
        let k = self.scaling_factor as f64 / 1000.0;
        
        let multiplier = max * (1.0 - 1.0 / (1.0 + (-k * x).exp()));
        
        (multiplier * 100.0) as u64
    }
}

#[account]
pub struct SupporterStake {
    pub supporter: Pubkey,
    pub creator: Pubkey,
    pub staked_amount: u64,
    pub stake_start_time: i64,
    pub last_claim_time: i64,
    // pub reward_cooldown: i64,  // Minimum time between claims
}

impl SupporterStake {
    pub const INIT_SPACE: usize = 32 + 32 + 8 + 8 + 8 + 8; // 88 bytes
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = admin,
        space = 8 + CreatorSupport::INIT_SPACE, // 144 bytes total
        seeds = [b"creator_support"],
        bump
    )]
    pub creator_support: Account<'info, CreatorSupport>,
    
    #[account(mut)]
    pub admin: Signer<'info>,
    
    #[account(
        seeds = [b"mint_auth"],
        bump,
    )]
    /// CHECK: PDA mint authority
    pub mint_authority: AccountInfo<'info>,
    
    pub reward_mint: Account<'info, Mint>,
    pub stablecoin_mint: Account<'info, Mint>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct InitializeVault<'info> {
    #[account(
        init,
        payer = payer,
        token::mint = stablecoin_mint,
        token::authority = vault_pda,
    )]
    pub vault_account: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub payer: Signer<'info>,
    
    #[account(
        seeds = [b"vault", creator.key().as_ref()],
        bump,
    )]
    /// CHECK: Vault PDA, seed-based address
    pub vault_pda: AccountInfo<'info>,
    
    pub creator: Account<'info, Creator>,
    pub stablecoin_mint: Account<'info, Mint>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct SupportCreator<'info> {
    #[account(mut)]
    pub creator_support: Account<'info, CreatorSupport>,
    
    #[account(mut)]
    pub creator: Account<'info, Creator>,
    
    #[account(mut)]
    pub supporter: Signer<'info>,
    
    #[account(
        init,
        payer = supporter,
        space = 8 + SupporterStake::INIT_SPACE, // 96 bytes
        seeds = [b"stake", supporter.key().as_ref(), creator.key().as_ref()],
        bump
    )]
    pub supporter_stake: Account<'info, SupporterStake>,
    
    pub system_program: Program<'info, System>,
    
    #[account(mut,
        constraint = stablecoin_mint.key() == creator_support.stablecoin_mint
    )]
    pub stablecoin_mint: Account<'info, Mint>,
    
    #[account(mut,
        associated_token::mint = stablecoin_mint,
        associated_token::authority = supporter
    )]
    pub supporter_stablecoin_account: Account<'info, TokenAccount>,
    
    #[account(mut,
        associated_token::mint = stablecoin_mint,
        associated_token::authority = creator
    )]
    pub creator_stablecoin_account: Account<'info, TokenAccount>,
    
    #[account(mut,
        token::mint = stablecoin_mint,
        token::authority = vault_pda,
    )]
    /// CHECK: Vault is a PDA; vault_account is a TokenAccount owned by vault_pda
    pub vault_account: Account<'info, TokenAccount>,
    
    #[account(
        seeds = [b"vault", creator.key().as_ref()],
        bump,
    )]
    /// CHECK: Vault PDA, seed-based address
    pub vault_pda: AccountInfo<'info>,
    
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct ClaimRewards<'info> {
    #[account(mut)]
    pub supporter_stake: Account<'info, SupporterStake>,
    
    #[account(mut)]
    pub creator: Account<'info, Creator>,
    
    #[account(mut)]
    pub supporter: Signer<'info>,
    
    #[account(mut)]
    pub creator_support: Account<'info, CreatorSupport>,
    
    #[account(mut,
        constraint = reward_mint.key() == creator_support.reward_mint
    )]
    pub reward_mint: Account<'info, Mint>,
    
    #[account(
        mut,
        associated_token::mint = reward_mint,
        associated_token::authority = supporter,
    )]
    pub supporter_reward_account: Account<'info, TokenAccount>,
    
    #[account(
        mut,
        associated_token::mint = reward_mint,
        associated_token::authority = creator,
    )]
    pub creator_reward_account: Account<'info, TokenAccount>,
    
    #[account(
        seeds = [b"mint_auth"],
        bump,
    )]
    /// CHECK: This is the program's mint authority PDA
    pub mint_authority: AccountInfo<'info>,
    
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct AdvanceTime<'info> {
    #[account()]
    pub clock: Sysvar<'info, Clock>,
}

/// Initializes the Creator account.
#[derive(Accounts)]
pub struct InitializeCreator<'info> {
    #[account(
        init,
        payer = admin,
        space = 8 + Creator::INIT_SPACE, // 8 bytes discriminator + fields
        seeds = [b"creator", admin.key().as_ref()],
        bump
    )]
    pub creator: Account<'info, Creator>,
    
    #[account(mut)]
    pub admin: Signer<'info>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Unstake<'info> {
    #[account(
        mut,
        seeds = [b"stake", supporter.key().as_ref(), creator.key().as_ref()],
        bump,
        has_one = supporter,
        has_one = creator,
    )]
    pub supporter_stake: Account<'info, SupporterStake>,

    #[account(mut)]
    pub creator: Account<'info, Creator>,

    pub creator_support: Account<'info, CreatorSupport>,

    #[account(mut)]
    pub supporter: Signer<'info>,

    #[account(mut,
        token::mint = stablecoin_mint,
        token::authority = vault_pda,
    )]
    pub vault_account: Account<'info, TokenAccount>,

    #[account(
        seeds = [b"vault", creator.key().as_ref()],
        bump,
    )]
    /// CHECK: Vault PDA, seed-based address
    pub vault_pda: AccountInfo<'info>,

    #[account(mut,
        associated_token::mint = stablecoin_mint,
        associated_token::authority = supporter
    )]
    pub supporter_stablecoin_account: Account<'info, TokenAccount>,

    pub stablecoin_mint: Account<'info, Mint>,
    pub token_program: Program<'info, Token>,
}