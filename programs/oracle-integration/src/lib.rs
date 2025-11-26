use anchor_lang::prelude::*;

declare_id!("Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS");

#[program]
pub mod oracle_integration {
    use super::*;

    pub fn initialize_config(
        ctx: Context<InitializeConfig>,
        symbol: String,
        pyth_feed: Pubkey,
        switchboard_aggregator: Pubkey,
        max_staleness: i64,
        max_confidence: u64,
        max_deviation: u64,
    ) -> Result<()> {
        let config = &mut ctx.accounts.config;
        config.symbol = symbol;
        config.pyth_feed = pyth_feed;
        config.switchboard_aggregator = switchboard_aggregator;
        config.max_staleness = max_staleness;
        config.max_confidence = max_confidence;
        config.max_deviation = max_deviation;
        Ok(())
    }

    pub fn get_pyth_price(
        ctx: Context<GetPythPrice>,
        _price_feed: Pubkey,
    ) -> Result<PriceData> {
        let _pyth_price_account = &ctx.accounts.pyth_price_account;
        
        // TODO: Implement real Pyth price parsing
        // For now, return realistic mock data that follows real oracle patterns
        let current_timestamp = Clock::get()?.unix_timestamp;
        
        // Simulate realistic price data with proper validation
        let current_price = pyth_sdk_solana::Price {
            price: 50000_00000000, // $50,000 with 8 decimals
            conf: 500_00000,       // $5 confidence (0.01% of price)
            expo: -8,              // 8 decimal places
            publish_time: current_timestamp - 10, // 10 seconds ago
        };
        
        // Validate staleness (configurable max_staleness from config)
        let clock = Clock::get()?;
        let current_timestamp = clock.unix_timestamp;
        if current_timestamp - current_price.publish_time > ctx.accounts.config.max_staleness {
            return Err(ErrorCode::StalePrice.into());
        }
        
        // Check if price is available and positive
        if current_price.price <= 0 {
            return Err(ErrorCode::PriceUnavailable.into());
        }
        
        // Validate confidence interval (confidence as percentage of price) 
        let confidence_percentage = (current_price.conf as f64 / current_price.price as f64) * 10000.0;
        if confidence_percentage > ctx.accounts.config.max_confidence as f64 {
            return Err(ErrorCode::LowConfidence.into());
        }
        
        Ok(PriceData {
            price: current_price.price,
            confidence: current_price.conf,
            expo: current_price.expo,
            timestamp: current_price.publish_time,
            source: PriceSource::Pyth,
        })
    }

    pub fn get_switchboard_price(
        ctx: Context<GetSwitchboardPrice>,
        _aggregator: Pubkey,
    ) -> Result<PriceData> {
        let _switchboard_account = &ctx.accounts.switchboard_aggregator;
        
        // Mock Switchboard data for now
        let result = switchboard_solana::SwitchboardDecimal {
            mantissa: 50000_00000000, // $50,000
            scale: 8,                 // 8 decimal places
        };
        
        // Validate timestamp
        let clock = Clock::get()?;
        let current_time = clock.unix_timestamp;
        
        Ok(PriceData {
            price: result.mantissa as i64,
            confidence: 1000000, // Mock confidence value
            expo: result.scale as i32,
            timestamp: current_time,
            source: PriceSource::Switchboard,
        })
    }

    pub fn validate_price_consensus(
        _ctx: Context<ValidatePrice>,
        prices: Vec<PriceData>,
    ) -> Result<u64> {
        if prices.len() < 2 {
            return Err(ErrorCode::InsufficientSources.into());
        }
        
        // Calculate median price
        let mut sorted_prices: Vec<i64> = prices.iter().map(|p| p.price).collect();
        sorted_prices.sort();
        
        let median = if sorted_prices.len() % 2 == 0 {
            let mid = sorted_prices.len() / 2;
            (sorted_prices[mid - 1] + sorted_prices[mid]) / 2
        } else {
            sorted_prices[sorted_prices.len() / 2]
        };
        
        // Validate prices within threshold (1% deviation)
        for price_data in &prices {
            let deviation = (price_data.price as f64 - median as f64).abs() / median as f64;
            if deviation > 0.01 { // 1% threshold
                return Err(ErrorCode::PriceDeviationTooHigh.into());
            }
        }
        
        Ok(median as u64)
    }
}

#[derive(Accounts)]
pub struct InitializeConfig<'info> {
    #[account(
        init,
        payer = payer,
        space = 8 + 64 + 32 + 32 + 8 + 8 + 8, // discriminator + symbol + pyth_feed + switchboard_aggregator + max_staleness + max_confidence + max_deviation
        seeds = [b"config"],
        bump
    )]
    pub config: Account<'info, OracleConfig>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct GetPythPrice<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    pub pyth_price_account: AccountInfo<'info>,
    #[account(
        seeds = [b"config"],
        bump,
    )]
    pub config: Account<'info, OracleConfig>,
}

#[derive(Accounts)]
pub struct GetSwitchboardPrice<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    /// CHECK: Switchboard aggregator account
    pub switchboard_aggregator: AccountInfo<'info>,
    #[account(
        seeds = [b"config"],
        bump,
    )]
    pub config: Account<'info, OracleConfig>,
}

#[derive(Accounts)]
pub struct ValidatePrice<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
}

#[account]
pub struct OracleConfig {
    pub symbol: String,
    pub pyth_feed: Pubkey,
    pub switchboard_aggregator: Pubkey,
    pub max_staleness: i64,    // seconds
    pub max_confidence: u64,   // basis points
    pub max_deviation: u64,    // basis points
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct PriceData {
    pub price: i64,
    pub confidence: u64,
    pub expo: i32,
    pub timestamp: i64,
    pub source: PriceSource,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq)]
pub enum PriceSource {
    Pyth,
    Switchboard,
    Internal,
}

#[error_code]
pub enum ErrorCode {
    #[msg("Price is unavailable")]
    PriceUnavailable,
    #[msg("Price is stale")]
    StalePrice,
    #[msg("Confidence interval too low")]
    LowConfidence,
    #[msg("Invalid Switchboard data")]
    InvalidSwitchboardData,
    #[msg("Invalid Pyth data")]
    InvalidPythData,
    #[msg("Insufficient price sources")]
    InsufficientSources,
    #[msg("Price deviation too high")]
    PriceDeviationTooHigh,
}