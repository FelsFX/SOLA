use anchor_lang::prelude::*;

declare_id!("payoUT1111111111111111111111111111111111");

pub const START_THRESHOLD_MULTIPLIER: u64 = 100;
pub const GROWTH_FACTOR_NUMERATOR: u128 = 1_003_726;
pub const GROWTH_FACTOR_DENOMINATOR: u128 = 1_000_000;
pub const TBTC_DECIMALS: u128 = 100_000_000;
pub const USDT_DECIMALS: u64 = 1_000_000;

#[program]
pub mod payout_plan {
    use super::*;

    pub fn initialize_plan(
        ctx: Context<InitializePlan>,
        monthly_usdt_payout: u64,
        metadata_uri: String,
    ) -> Result<()> {
        require!(monthly_usdt_payout > 0, ErrorCode::InvalidMonthlyPayout);
        require!(
            metadata_uri.len() <= PayoutPlan::MAX_METADATA_LEN,
            ErrorCode::InvalidMetadataLength
        );

        let plan = &mut ctx.accounts.plan;
        plan.owner = ctx.accounts.authority.key();
        plan.monthly_usdt_payout = monthly_usdt_payout;
        plan.current_payout_usdt = monthly_usdt_payout as u128;
        plan.total_deposited_usdt = 0;
        plan.total_withdrawn_usdt = 0;
        plan.metadata_uri = metadata_uri.clone();
        plan.nft_mint = None;
        plan.is_active = false;
        plan.emergency_triggered = false;

        emit!(PlanInitialized {
            plan: plan.key(),
            owner: plan.owner,
            monthly_usdt_payout: plan.monthly_usdt_payout as u128,
            metadata_uri,
        });

        Ok(())
    }

    pub fn fund_plan(
        ctx: Context<FundPlan>,
        amount_usdt: u64,
        nft_mint: Option<Pubkey>,
    ) -> Result<()> {
        let plan = &mut ctx.accounts.plan;
        plan.record_funding(amount_usdt, nft_mint)?;

        emit!(PlanFunded {
            plan: plan.key(),
            amount_usdt: amount_usdt as u128,
            total_deposited_usdt: plan.total_deposited_usdt,
            is_active: plan.is_active,
        });

        Ok(())
    }

    pub fn execute_payout(ctx: Context<ExecutePayout>, price_usdt_per_tbtc: u64) -> Result<()> {
        let plan = &mut ctx.accounts.plan;
        require_keys_eq!(
            ctx.accounts.authority.key(),
            plan.owner,
            ErrorCode::Unauthorized
        );

        let (payout_usdt, payout_tbtc) =
            plan.execute_payout_internal(price_usdt_per_tbtc as u128)?;

        emit!(PayoutExecuted {
            plan: plan.key(),
            payout_usdt,
            payout_tbtc,
            price_usdt_per_tbtc: price_usdt_per_tbtc as u128,
            next_payout_usdt: plan.current_payout_usdt,
        });

        Ok(())
    }

    pub fn trigger_emergency(ctx: Context<TriggerEmergency>) -> Result<()> {
        let plan = &mut ctx.accounts.plan;
        require_keys_eq!(
            ctx.accounts.authority.key(),
            plan.owner,
            ErrorCode::Unauthorized
        );
        plan.trigger_emergency()?;

        emit!(EmergencyTriggered { plan: plan.key() });
        Ok(())
    }

    pub fn emergency_withdraw(
        ctx: Context<EmergencyWithdraw>,
        price_usdt_per_tbtc: u64,
    ) -> Result<()> {
        let plan = &mut ctx.accounts.plan;
        require_keys_eq!(
            ctx.accounts.authority.key(),
            plan.owner,
            ErrorCode::Unauthorized
        );
        let (amount_usdt, amount_tbtc) =
            plan.emergency_withdraw_internal(price_usdt_per_tbtc as u128)?;

        emit!(EmergencyWithdrawal {
            plan: plan.key(),
            amount_usdt,
            amount_tbtc,
            price_usdt_per_tbtc: price_usdt_per_tbtc as u128,
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
        space = 8 + PayoutPlan::BASE_SIZE + PayoutPlan::MAX_METADATA_LEN
    )]
    pub plan: Account<'info, PayoutPlan>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct FundPlan<'info> {
    #[account(mut)]
    pub funder: Signer<'info>,
    #[account(mut)]
    pub plan: Account<'info, PayoutPlan>,
}

#[derive(Accounts)]
pub struct ExecutePayout<'info> {
    pub authority: Signer<'info>,
    #[account(mut)]
    pub plan: Account<'info, PayoutPlan>,
}

#[derive(Accounts)]
pub struct TriggerEmergency<'info> {
    pub authority: Signer<'info>,
    #[account(mut)]
    pub plan: Account<'info, PayoutPlan>,
}

#[derive(Accounts)]
pub struct EmergencyWithdraw<'info> {
    pub authority: Signer<'info>,
    #[account(mut)]
    pub plan: Account<'info, PayoutPlan>,
}

#[account]
pub struct PayoutPlan {
    pub owner: Pubkey,
    pub monthly_usdt_payout: u64,
    pub current_payout_usdt: u128,
    pub total_deposited_usdt: u128,
    pub total_withdrawn_usdt: u128,
    pub metadata_uri: String,
    pub nft_mint: Option<Pubkey>,
    pub is_active: bool,
    pub emergency_triggered: bool,
}

impl PayoutPlan {
    pub const MAX_METADATA_LEN: usize = 128;
    pub const BASE_SIZE: usize = 32 + 8 + 16 + 16 + 16 + 4 + 33 + 1 + 1;

    pub fn activation_threshold(&self) -> u128 {
        (self.monthly_usdt_payout as u128).saturating_mul(START_THRESHOLD_MULTIPLIER as u128)
    }

    pub fn available_balance(&self) -> u128 {
        self.total_deposited_usdt
            .saturating_sub(self.total_withdrawn_usdt)
    }

    pub fn record_funding(&mut self, amount_usdt: u64, nft_mint: Option<Pubkey>) -> Result<()> {
        require!(amount_usdt > 0, ErrorCode::ZeroFundingAmount);
        require!(!self.emergency_triggered, ErrorCode::EmergencyModeActive);

        match (&self.nft_mint, nft_mint) {
            (None, Some(mint)) => {
                self.nft_mint = Some(mint);
            }
            (None, None) => return err!(ErrorCode::NftMintRequired),
            (Some(existing), Some(mint)) => {
                require!(existing == &mint, ErrorCode::NftMintMismatch);
            }
            (Some(_), None) => {}
        }

        let amount = amount_usdt as u128;
        self.total_deposited_usdt = self
            .total_deposited_usdt
            .checked_add(amount)
            .ok_or(ErrorCode::MathOverflow)?;

        if !self.is_active && self.total_deposited_usdt >= self.activation_threshold() {
            self.is_active = true;
        }

        Ok(())
    }

    pub fn execute_payout_internal(&mut self, price_usdt_per_tbtc: u128) -> Result<(u128, u64)> {
        require!(self.is_active, ErrorCode::PlanInactive);
        require!(!self.emergency_triggered, ErrorCode::EmergencyModeActive);
        require!(self.current_payout_usdt > 0, ErrorCode::NothingToPayout);

        let available = self.available_balance();
        require!(
            available >= self.current_payout_usdt,
            ErrorCode::InsufficientBalance
        );

        let payout_usdt = self.current_payout_usdt;
        let payout_tbtc = convert_usdt_to_tbtc(payout_usdt, price_usdt_per_tbtc)?;

        self.total_withdrawn_usdt = self
            .total_withdrawn_usdt
            .checked_add(payout_usdt)
            .ok_or(ErrorCode::MathOverflow)?;

        self.current_payout_usdt = self
            .current_payout_usdt
            .checked_mul(GROWTH_FACTOR_NUMERATOR)
            .ok_or(ErrorCode::MathOverflow)?
            / GROWTH_FACTOR_DENOMINATOR;

        Ok((payout_usdt, payout_tbtc))
    }

    pub fn trigger_emergency(&mut self) -> Result<()> {
        require!(
            !self.emergency_triggered,
            ErrorCode::EmergencyAlreadyTriggered
        );
        self.emergency_triggered = true;
        self.is_active = false;
        Ok(())
    }

    pub fn emergency_withdraw_internal(
        &mut self,
        price_usdt_per_tbtc: u128,
    ) -> Result<(u128, u64)> {
        require!(self.emergency_triggered, ErrorCode::EmergencyNotTriggered);

        let amount_usdt = self.available_balance();
        if amount_usdt == 0 {
            return Ok((0, 0));
        }

        let amount_tbtc = convert_usdt_to_tbtc(amount_usdt, price_usdt_per_tbtc)?;

        self.total_withdrawn_usdt = self
            .total_withdrawn_usdt
            .checked_add(amount_usdt)
            .ok_or(ErrorCode::MathOverflow)?;
        self.is_active = false;

        Ok((amount_usdt, amount_tbtc))
    }
}

fn convert_usdt_to_tbtc(amount_usdt: u128, price_usdt_per_tbtc: u128) -> Result<u64> {
    require!(price_usdt_per_tbtc > 0, ErrorCode::PriceCannotBeZero);

    let numerator = amount_usdt
        .checked_mul(TBTC_DECIMALS)
        .ok_or(ErrorCode::MathOverflow)?;

    let amount_tbtc = numerator / price_usdt_per_tbtc;
    require!(
        amount_tbtc <= u64::MAX as u128,
        ErrorCode::TbtcConversionOverflow
    );

    Ok(amount_tbtc as u64)
}

#[event]
pub struct PlanInitialized {
    #[index]
    pub plan: Pubkey,
    pub owner: Pubkey,
    pub monthly_usdt_payout: u128,
    pub metadata_uri: String,
}

#[event]
pub struct PlanFunded {
    #[index]
    pub plan: Pubkey,
    pub amount_usdt: u128,
    pub total_deposited_usdt: u128,
    pub is_active: bool,
}

#[event]
pub struct PayoutExecuted {
    #[index]
    pub plan: Pubkey,
    pub payout_usdt: u128,
    pub payout_tbtc: u64,
    pub price_usdt_per_tbtc: u128,
    pub next_payout_usdt: u128,
}

#[event]
pub struct EmergencyTriggered {
    #[index]
    pub plan: Pubkey,
}

#[event]
pub struct EmergencyWithdrawal {
    #[index]
    pub plan: Pubkey,
    pub amount_usdt: u128,
    pub amount_tbtc: u64,
    pub price_usdt_per_tbtc: u128,
}

#[error_code]
pub enum ErrorCode {
    #[msg("monthly payout must be greater than zero")]
    InvalidMonthlyPayout,
    #[msg("metadata exceeds maximum length")]
    InvalidMetadataLength,
    #[msg("funding amount must be greater than zero")]
    ZeroFundingAmount,
    #[msg("emergency mode active")]
    EmergencyModeActive,
    #[msg("NFT mint must be provided on first funding")]
    NftMintRequired,
    #[msg("NFT mint does not match registered mint")]
    NftMintMismatch,
    #[msg("mathematical overflow detected")]
    MathOverflow,
    #[msg("plan is not yet active")]
    PlanInactive,
    #[msg("no funds available for payout")]
    InsufficientBalance,
    #[msg("no payout amount available")]
    NothingToPayout,
    #[msg("price must be greater than zero")]
    PriceCannotBeZero,
    #[msg("tBTC conversion overflow")]
    TbtcConversionOverflow,
    #[msg("caller is not authorized")]
    Unauthorized,
    #[msg("emergency already triggered")]
    EmergencyAlreadyTriggered,
    #[msg("emergency mode not triggered")]
    EmergencyNotTriggered,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_plan() -> PayoutPlan {
        PayoutPlan {
            owner: Pubkey::new_unique(),
            monthly_usdt_payout: 1_000 * USDT_DECIMALS,
            current_payout_usdt: (1_000 * USDT_DECIMALS) as u128,
            total_deposited_usdt: 0,
            total_withdrawn_usdt: 0,
            metadata_uri: "https://example.com/metadata.json".to_string(),
            nft_mint: None,
            is_active: false,
            emergency_triggered: false,
        }
    }

    #[test]
    fn test_activation_threshold() {
        let mut plan = sample_plan();
        let mint = Pubkey::new_unique();

        assert!(plan.record_funding(10 * USDT_DECIMALS, Some(mint)).is_ok());
        assert!(!plan.is_active);

        let additional = plan.activation_threshold() as u64 - 10 * USDT_DECIMALS;
        assert!(plan.record_funding(additional, Some(mint)).is_ok());
        assert!(plan.is_active);
        assert_eq!(plan.nft_mint, Some(mint));
    }

    #[test]
    fn test_payout_growth_and_conversion() {
        let mut plan = sample_plan();
        let mint = Pubkey::new_unique();

        let threshold = plan.activation_threshold() as u64;
        plan.record_funding(threshold, Some(mint)).unwrap();
        assert!(plan.is_active);

        let price = 30_000u64 * USDT_DECIMALS;
        let (payout_usdt, payout_tbtc) =
            plan.execute_payout_internal(price as u128).expect("payout");

        assert_eq!(payout_usdt, 1_000 * USDT_DECIMALS as u128);
        assert_eq!(payout_tbtc, 3_333_333);
        assert_eq!(
            plan.current_payout_usdt,
            (1_000 * USDT_DECIMALS as u128 * GROWTH_FACTOR_NUMERATOR) / GROWTH_FACTOR_DENOMINATOR
        );
    }

    #[test]
    fn test_emergency_flow() {
        let mut plan = sample_plan();
        let mint = Pubkey::new_unique();
        let threshold = plan.activation_threshold() as u64;
        plan.record_funding(threshold, Some(mint)).unwrap();
        assert!(plan.is_active);

        plan.trigger_emergency().unwrap();
        assert!(plan.emergency_triggered);
        assert!(!plan.is_active);

        let price = 25_000u64 * USDT_DECIMALS;
        let (amount_usdt, amount_tbtc) = plan
            .emergency_withdraw_internal(price as u128)
            .expect("withdraw");
        assert!(amount_usdt > 0);
        assert!(amount_tbtc > 0);
        assert_eq!(plan.available_balance(), 0);
    }

    #[test]
    fn test_price_zero_error() {
        let mut plan = sample_plan();
        let mint = Pubkey::new_unique();
        let threshold = plan.activation_threshold() as u64;
        plan.record_funding(threshold, Some(mint)).unwrap();
        assert!(plan.is_active);

        let err = plan
            .execute_payout_internal(0)
            .err()
            .expect("expected error");
        assert_eq!(err, ErrorCode::PriceCannotBeZero.into());
    }
}
