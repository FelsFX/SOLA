use anchor_lang::prelude::*;

pub const START_THRESHOLD_MULTIPLIER: u128 = 100;
pub const GROWTH_FACTOR_NUMERATOR: u128 = 1_003_726;
pub const GROWTH_FACTOR_DENOMINATOR: u128 = 1_000_000;
pub const TBTC_DECIMALS: u128 = 100_000_000;
pub const USDT_DECIMALS: u128 = 1_000_000;

declare_id!("payout111111111111111111111111111111111111111");

#[program]
pub mod payout_plan {
    use super::*;

    pub fn initialize_plan(
        ctx: Context<InitializePlan>,
        monthly_payout_usdt: u64,
        metadata_uri: String,
    ) -> Result<()> {
        require!(monthly_payout_usdt > 0, PayoutError::MonthlyPayoutZero);
        require!(
            metadata_uri.as_bytes().len() <= PlanAccount::MAX_METADATA_LEN,
            PayoutError::MetadataTooLong
        );

        let plan = &mut ctx.accounts.plan;
        plan.authority = ctx.accounts.authority.key();
        plan.bump = *ctx.bumps.get("plan").unwrap();
        plan.monthly_payout_usdt = monthly_payout_usdt;
        plan.current_monthly_payout_usdt = monthly_payout_usdt;
        plan.accumulated_balance_usdt = 0;
        plan.started = false;
        plan.payout_count = 0;
        plan.nft_mint = Pubkey::default();
        plan.nft_created = false;
        plan.metadata_uri = metadata_uri;
        plan.total_paid_out_usdt = 0;
        plan.emergency_unlocked = false;
        plan.status = PlanStatus::Pending;

        emit!(PlanInitialized {
            authority: plan.authority,
            monthly_payout_usdt,
        });

        Ok(())
    }

    pub fn fund_plan(ctx: Context<FundPlan>, deposit_usdt: u64) -> Result<()> {
        require!(deposit_usdt > 0, PayoutError::DepositZero);

        let plan = &mut ctx.accounts.plan;
        require_keys_eq!(
            plan.authority,
            ctx.accounts.authority.key(),
            PayoutError::Unauthorized
        );
        let deposit = deposit_usdt as u128;
        plan.accumulated_balance_usdt = plan
            .accumulated_balance_usdt
            .checked_add(deposit)
            .ok_or(PayoutError::Overflow)?;

        if !plan.nft_created {
            plan.nft_mint = ctx.accounts.nft_mint.key();
            plan.nft_created = true;
        }

        if !plan.started && plan.accumulated_balance_usdt >= plan.start_threshold() {
            plan.started = true;
            plan.status = PlanStatus::Active;
        }

        let balance_usdt: u64 = plan
            .accumulated_balance_usdt
            .try_into()
            .map_err(|_| PayoutError::Overflow)?;

        emit!(PlanFunded {
            authority: plan.authority,
            deposit_usdt,
            balance_usdt,
            started: plan.started,
        });

        Ok(())
    }

    pub fn execute_payout(ctx: Context<ExecutePayout>, price_usdt_per_tbtc: u64) -> Result<()> {
        let plan = &mut ctx.accounts.plan;
        require!(plan.started, PayoutError::PlanNotStarted);
        require!(!plan.emergency_unlocked, PayoutError::PlanInEmergency);
        require!(price_usdt_per_tbtc > 0, PayoutError::PriceMustBePositive);

        let payout_usdt = plan.current_monthly_payout_usdt;
        require!(
            plan.accumulated_balance_usdt >= payout_usdt as u128,
            PayoutError::InsufficientBalance
        );

        let payout_tbtc = convert_usdt_to_tbtc(payout_usdt, price_usdt_per_tbtc)?;

        plan.accumulated_balance_usdt = plan
            .accumulated_balance_usdt
            .checked_sub(payout_usdt as u128)
            .ok_or(PayoutError::Overflow)?;
        plan.total_paid_out_usdt = plan
            .total_paid_out_usdt
            .checked_add(payout_usdt as u128)
            .ok_or(PayoutError::Overflow)?;
        plan.payout_count = plan
            .payout_count
            .checked_add(1)
            .ok_or(PayoutError::Overflow)?;
        plan.current_monthly_payout_usdt = apply_growth(plan.current_monthly_payout_usdt)?;

        let remaining_balance_usdt: u64 = plan
            .accumulated_balance_usdt
            .try_into()
            .map_err(|_| PayoutError::Overflow)?;

        emit!(PayoutExecuted {
            authority: plan.authority,
            payout_index: plan.payout_count,
            payout_usdt,
            payout_tbtc,
            price_usdt_per_tbtc,
            remaining_balance_usdt,
        });

        Ok(())
    }

    pub fn trigger_emergency(ctx: Context<TriggerEmergency>) -> Result<()> {
        let plan = &mut ctx.accounts.plan;
        require_keys_eq!(
            plan.authority,
            ctx.accounts.authority.key(),
            PayoutError::Unauthorized
        );
        plan.emergency_unlocked = true;
        plan.status = PlanStatus::Emergency;

        emit!(EmergencyTriggered {
            authority: plan.authority,
        });

        Ok(())
    }

    pub fn emergency_withdraw(ctx: Context<EmergencyWithdraw>) -> Result<()> {
        let plan = &mut ctx.accounts.plan;
        require_keys_eq!(
            plan.authority,
            ctx.accounts.authority.key(),
            PayoutError::Unauthorized
        );
        require!(plan.emergency_unlocked, PayoutError::EmergencyNotEnabled);

        let remaining = plan.accumulated_balance_usdt;
        require!(remaining <= u64::MAX as u128, PayoutError::Overflow);
        plan.accumulated_balance_usdt = 0;
        plan.status = PlanStatus::Closed;

        emit!(EmergencyWithdrawal {
            authority: plan.authority,
            withdrawn_usdt: remaining as u64,
        });

        Ok(())
    }
}

#[derive(Accounts)]
pub struct InitializePlan<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(
        init,
        payer = authority,
        space = PlanAccount::SPACE,
        seeds = [PlanAccount::SEED_PREFIX, authority.key().as_ref()],
        bump
    )]
    pub plan: Account<'info, PlanAccount>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct FundPlan<'info> {
    #[account(mut, has_one = authority, seeds = [PlanAccount::SEED_PREFIX, authority.key().as_ref()], bump = plan.bump)]
    pub plan: Account<'info, PlanAccount>,
    pub authority: Signer<'info>,
    /// CHECK: NFT mint is recorded for metadata purposes and validated off-chain
    pub nft_mint: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct ExecutePayout<'info> {
    #[account(mut, has_one = authority, seeds = [PlanAccount::SEED_PREFIX, authority.key().as_ref()], bump = plan.bump)]
    pub plan: Account<'info, PlanAccount>,
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct TriggerEmergency<'info> {
    #[account(mut, has_one = authority, seeds = [PlanAccount::SEED_PREFIX, authority.key().as_ref()], bump = plan.bump)]
    pub plan: Account<'info, PlanAccount>,
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct EmergencyWithdraw<'info> {
    #[account(mut, has_one = authority, seeds = [PlanAccount::SEED_PREFIX, authority.key().as_ref()], bump = plan.bump)]
    pub plan: Account<'info, PlanAccount>,
    pub authority: Signer<'info>,
}

#[account]
pub struct PlanAccount {
    pub authority: Pubkey,
    pub bump: u8,
    pub monthly_payout_usdt: u64,
    pub current_monthly_payout_usdt: u64,
    pub accumulated_balance_usdt: u128,
    pub total_paid_out_usdt: u128,
    pub started: bool,
    pub payout_count: u64,
    pub nft_mint: Pubkey,
    pub nft_created: bool,
    pub metadata_uri: String,
    pub emergency_unlocked: bool,
    pub status: PlanStatus,
}

impl PlanAccount {
    pub const SEED_PREFIX: &'static [u8] = b"plan";
    pub const MAX_METADATA_LEN: usize = 128;
    pub const SPACE: usize = 8  // discriminator
        + 32 // authority
        + 1 // bump
        + 8 // monthly payout
        + 8 // current monthly payout
        + 16 // accumulated balance
        + 16 // total paid out
        + 1 // started
        + 8 // payout count
        + 32 // nft mint
        + 1 // nft created
        + 4 + Self::MAX_METADATA_LEN // metadata string
        + 1 // emergency unlocked
        + 1; // status

    pub fn start_threshold(&self) -> u128 {
        (self.monthly_payout_usdt as u128)
            .checked_mul(START_THRESHOLD_MULTIPLIER)
            .expect("threshold overflow")
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PlanStatus {
    Pending = 0,
    Active = 1,
    Emergency = 2,
    Closed = 3,
}

#[event]
pub struct PlanInitialized {
    pub authority: Pubkey,
    pub monthly_payout_usdt: u64,
}

#[event]
pub struct PlanFunded {
    pub authority: Pubkey,
    pub deposit_usdt: u64,
    pub balance_usdt: u64,
    pub started: bool,
}

#[event]
pub struct PayoutExecuted {
    pub authority: Pubkey,
    pub payout_index: u64,
    pub payout_usdt: u64,
    pub payout_tbtc: u64,
    pub price_usdt_per_tbtc: u64,
    pub remaining_balance_usdt: u64,
}

#[event]
pub struct EmergencyTriggered {
    pub authority: Pubkey,
}

#[event]
pub struct EmergencyWithdrawal {
    pub authority: Pubkey,
    pub withdrawn_usdt: u64,
}

#[error_code]
pub enum PayoutError {
    #[msg("Monthly payout must be greater than zero")]
    MonthlyPayoutZero,
    #[msg("Deposit must be greater than zero")]
    DepositZero,
    #[msg("Caller is not authorized to perform this action")]
    Unauthorized,
    #[msg("Plan has not started yet")]
    PlanNotStarted,
    #[msg("Plan is in emergency mode")]
    PlanInEmergency,
    #[msg("Emergency withdrawals are not enabled")]
    EmergencyNotEnabled,
    #[msg("Insufficient balance for payout")]
    InsufficientBalance,
    #[msg("Price must be a positive value")]
    PriceMustBePositive,
    #[msg("Overflow detected")]
    Overflow,
    #[msg("Metadata URI is too long")]
    MetadataTooLong,
}

pub fn apply_growth(amount: u64) -> Result<u64> {
    let grown = (amount as u128)
        .checked_mul(GROWTH_FACTOR_NUMERATOR)
        .ok_or(PayoutError::Overflow)?
        .checked_div(GROWTH_FACTOR_DENOMINATOR)
        .ok_or(PayoutError::Overflow)?;
    grown.try_into().map_err(|_| PayoutError::Overflow.into())
}

pub fn convert_usdt_to_tbtc(usdt_amount: u64, price_usdt_per_tbtc: u64) -> Result<u64> {
    require!(price_usdt_per_tbtc > 0, PayoutError::PriceMustBePositive);
    let numerator = (usdt_amount as u128)
        .checked_mul(TBTC_DECIMALS)
        .ok_or(PayoutError::Overflow)?;
    let tbtc_amount = numerator
        .checked_div(price_usdt_per_tbtc as u128)
        .ok_or(PayoutError::Overflow)?;
    tbtc_amount
        .try_into()
        .map_err(|_| PayoutError::Overflow.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn growth_factor_applies() {
        let start_amount = 1_000_000u64; // 1 USDT
        let grown = apply_growth(start_amount).unwrap();
        assert_eq!(grown, 1_003_726);
    }

    #[test]
    fn tbtc_conversion_matches_expected() {
        let usdt = 10_000_000u64; // 10 USDT
        let price = 30_000_0000u64; // 3,000 USDT per tBTC (6 decimals)
        let tbtc = convert_usdt_to_tbtc(usdt, price).unwrap();
        // 10 USDT / 3,000 = 0.00333333... tBTC
        assert_eq!(tbtc, 333_333);
    }

    #[test]
    fn start_threshold_is_hundred_x() {
        let mut plan = PlanAccount {
            authority: Pubkey::default(),
            bump: 0,
            monthly_payout_usdt: 5_000_000,
            current_monthly_payout_usdt: 5_000_000,
            accumulated_balance_usdt: 0,
            total_paid_out_usdt: 0,
            started: false,
            payout_count: 0,
            nft_mint: Pubkey::default(),
            nft_created: false,
            metadata_uri: String::new(),
            emergency_unlocked: false,
            status: PlanStatus::Pending,
        };
        assert_eq!(plan.start_threshold(), 500_000_000);
        plan.monthly_payout_usdt = 1_200_000;
        assert_eq!(plan.start_threshold(), 120_000_000);
    }
}
