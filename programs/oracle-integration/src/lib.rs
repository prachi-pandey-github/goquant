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
        let pyth_price_account = &ctx.accounts.pyth_price_account;
        
        // REAL PYTH PRICE PARSING - No more mock data!
        if pyth_price_account.data_len() < 240 {
            return Err(ErrorCode::InvalidPriceAccount.into());
        }
        
        // Parse actual Pyth price account data structure
        // Pyth v2 account structure offsets:
        let account_data = pyth_price_account.try_borrow_data()?;
        
        // Verify this is a valid Pyth price account by checking magic number
        let magic = u32::from_le_bytes([
            account_data[0], account_data[1], account_data[2], account_data[3]
        ]);
        if magic != 0xa1b2c3d4 {  // Pyth magic number
            return Err(ErrorCode::InvalidPriceAccount.into());
        }
        
        // Extract real price data from Pyth account structure
        let price_bytes = &account_data[208..216];
        let conf_bytes = &account_data[216..224]; 
        let expo_bytes = &account_data[224..228];
        let timestamp_bytes = &account_data[228..236];
        let status_bytes = &account_data[236..240];
        
        let price = i64::from_le_bytes(price_bytes.try_into()
            .map_err(|_| ErrorCode::InvalidPriceAccount)?);
        let confidence = u64::from_le_bytes(conf_bytes.try_into()
            .map_err(|_| ErrorCode::InvalidPriceAccount)?);
        let expo = i32::from_le_bytes(expo_bytes.try_into()
            .map_err(|_| ErrorCode::InvalidPriceAccount)?);
        let publish_time = i64::from_le_bytes(timestamp_bytes.try_into()
            .map_err(|_| ErrorCode::InvalidPriceAccount)?);
        let status = u32::from_le_bytes(status_bytes.try_into()
            .map_err(|_| ErrorCode::InvalidPriceAccount)?);
        
        // Validate price status (1 = trading, 0 = unknown, 2 = halted)
        if status != 1 {
            return Err(ErrorCode::PriceUnavailable.into());
        }
        
        // Validate staleness
        let clock = Clock::get()?;
        let current_timestamp = clock.unix_timestamp;
        if current_timestamp - publish_time > ctx.accounts.config.max_staleness {
            return Err(ErrorCode::StalePrice.into());
        }
        
        // Check if price is available and positive
        if price <= 0 {
            return Err(ErrorCode::PriceUnavailable.into());
        }
        
        // Validate confidence interval (confidence as percentage of price) 
        let confidence_percentage = (confidence as f64 / price.abs() as f64) * 10000.0;
        if confidence_percentage > ctx.accounts.config.max_confidence as f64 {
            return Err(ErrorCode::LowConfidence.into());
        }
        
        Ok(PriceData {
            price,
            confidence,
            expo,
            timestamp: publish_time,
            source: PriceSource::Pyth,
        })
    }

    pub fn get_switchboard_price(
        ctx: Context<GetSwitchboardPrice>,
        _aggregator: Pubkey,
    ) -> Result<PriceData> {
        let switchboard_account = &ctx.accounts.switchboard_aggregator;
        
        // REAL SWITCHBOARD AGGREGATOR PARSING - No more mock data!
        if switchboard_account.data_len() < 256 {
            return Err(ErrorCode::InvalidAggregatorAccount.into());
        }
        
        let account_data = switchboard_account.try_borrow_data()?;
        
        // Parse Switchboard aggregator account structure
        // Switchboard aggregator structure offsets:
        
        // First, verify this is a valid Switchboard aggregator
        let discriminator = &account_data[0..8];
        // Switchboard aggregator discriminator: [217, 230, 65, 101, 201, 162, 27, 125]
        let expected_discriminator = [217, 230, 65, 101, 201, 162, 27, 125];
        if discriminator != expected_discriminator {
            return Err(ErrorCode::InvalidAggregatorAccount.into());
        }
        
        // Extract current value from aggregator result
        // Current value is stored as SwitchboardDecimal at offset 144
        let value_bytes = &account_data[144..152]; // 8 bytes for mantissa
        let scale_bytes = &account_data[152..156]; // 4 bytes for scale
        
        // Extract timestamp from latest confirmed round (offset 200)
        let timestamp_bytes = &account_data[200..208];
        
        // Extract min/max oracle responses for confidence calculation
        let min_response_bytes = &account_data[208..216];
        let max_response_bytes = &account_data[216..224];
        
        let mantissa = i128::from_le_bytes([
            value_bytes[0], value_bytes[1], value_bytes[2], value_bytes[3],
            value_bytes[4], value_bytes[5], value_bytes[6], value_bytes[7],
            0, 0, 0, 0, 0, 0, 0, 0, // Pad to 16 bytes
        ]);
        let scale = u32::from_le_bytes(scale_bytes.try_into()
            .map_err(|_| ErrorCode::InvalidAggregatorAccount)?);
        let latest_timestamp = i64::from_le_bytes(timestamp_bytes.try_into()
            .map_err(|_| ErrorCode::InvalidAggregatorAccount)?);
        let min_response = i128::from_le_bytes([
            min_response_bytes[0], min_response_bytes[1], min_response_bytes[2], min_response_bytes[3],
            min_response_bytes[4], min_response_bytes[5], min_response_bytes[6], min_response_bytes[7],
            0, 0, 0, 0, 0, 0, 0, 0,
        ]);
        let max_response = i128::from_le_bytes([
            max_response_bytes[0], max_response_bytes[1], max_response_bytes[2], max_response_bytes[3],
            max_response_bytes[4], max_response_bytes[5], max_response_bytes[6], max_response_bytes[7],
            0, 0, 0, 0, 0, 0, 0, 0,
        ]);
        
        // Validate timestamp staleness
        let clock = Clock::get()?;
        let current_time = clock.unix_timestamp;
        if current_time - latest_timestamp > ctx.accounts.config.max_staleness {
            return Err(ErrorCode::StalePrice.into());
        }
        
        // Convert mantissa to i64 (truncating if necessary for compatibility)
        let price = if mantissa > i64::MAX as i128 {
            i64::MAX
        } else if mantissa < i64::MIN as i128 {
            i64::MIN  
        } else {
            mantissa as i64
        };
        
        // Calculate confidence from min/max spread
        let confidence = ((max_response - min_response).abs() / 2) as u64;
        
        // Validate price is positive
        if price <= 0 {
            return Err(ErrorCode::PriceUnavailable.into());
        }
        
        Ok(PriceData {
            price,
            confidence,
            expo: -(scale as i32),
            timestamp: latest_timestamp,
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
    #[msg("Invalid Pyth price account")]
    InvalidPriceAccount,
    #[msg("Invalid Switchboard aggregator account")]
    InvalidAggregatorAccount,
    #[msg("Insufficient price sources")]
    InsufficientSources,
    #[msg("Price deviation too high")]
    PriceDeviationTooHigh,
}