use anyhow::Result;
use statrs::statistics::Statistics;
use tracing::{debug, warn};

use crate::types::{PriceData, PriceSource, Symbol};

/// Advanced price aggregation engine with manipulation resistance
pub struct PriceAggregator {
    // Configuration for different aggregation methods
    _deviation_threshold: f64,
    _confidence_weight: f64,
    min_sources: usize,
}

impl PriceAggregator {
    pub fn new() -> Self {
        Self {
            _deviation_threshold: 0.01, // 1% maximum deviation
            _confidence_weight: 0.7,    // Weight given to confidence in final score
            min_sources: 1,            // Minimum sources required
        }
    }
    
    /// Aggregate prices from multiple sources with advanced consensus
    pub fn aggregate_prices(&self, prices: &[PriceData], symbol: &Symbol) -> Result<PriceData> {
        if prices.len() < self.min_sources {
            anyhow::bail!("Insufficient price sources: {} < {}", prices.len(), self.min_sources);
        }
        
        debug!("Aggregating {} prices for {}", prices.len(), symbol.name);
        
        // Convert prices to common decimal format
        let normalized_prices: Vec<f64> = prices.iter()
            .map(|p| self.normalize_price(p))
            .collect();
        
        // Detect and filter outliers
        let filtered_prices = self.filter_outliers(&normalized_prices, prices)?;
        
        // Calculate consensus price using multiple methods
        let consensus_price = self.calculate_consensus(&filtered_prices)?;
        
        // Calculate aggregated confidence
        let consensus_confidence = self.calculate_confidence(&filtered_prices);
        
        // Get the most recent timestamp
        let latest_timestamp = prices.iter().map(|p| p.timestamp).max().unwrap_or(0);
        
        // Create aggregated price data
        let aggregated = PriceData {
            price: (consensus_price * 10_f64.powi(8)) as i64, // Convert back to integer with 8 decimals
            confidence: consensus_confidence,
            expo: -8, // Standard 8 decimal places
            timestamp: latest_timestamp,
            source: PriceSource::Aggregated,
            symbol: symbol.name.clone(),
        };
        
        debug!("Aggregated price for {}: ${:.2}", symbol.name, consensus_price);
        
        Ok(aggregated)
    }
    
    /// Normalize price to decimal format
    fn normalize_price(&self, price_data: &PriceData) -> f64 {
        price_data.price as f64 / 10_f64.powi(-price_data.expo)
    }
    
    /// Calculate median from a slice of f64 values
    fn calculate_median(&self, mut values: Vec<f64>) -> f64 {
        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let len = values.len();
        if len == 0 {
            0.0
        } else if len % 2 == 0 {
            (values[len / 2 - 1] + values[len / 2]) / 2.0
        } else {
            values[len / 2]
        }
    }

    /// Detect and filter statistical outliers
    fn filter_outliers(&self, prices: &[f64], original_data: &[PriceData]) -> Result<Vec<PriceData>> {
        if prices.len() <= 2 {
            return Ok(original_data.to_vec()); // Can't filter outliers with <= 2 data points
        }
        
        // Calculate median and median absolute deviation (MAD)
        let median = self.calculate_median(prices.to_vec());
        let deviations: Vec<f64> = prices.iter()
            .map(|&p| (p - median).abs())
            .collect();
        let mad = self.calculate_median(deviations);
        
        // Filter outliers using modified z-score method
        let mut filtered = Vec::new();
        for (i, &price) in prices.iter().enumerate() {
            let modified_z_score = if mad > 0.0 {
                0.6745 * (price - median).abs() / mad
            } else {
                0.0
            };
            
            // Keep prices within 2.5 standard deviations (adjustable threshold)
            if modified_z_score <= 2.5 {
                filtered.push(original_data[i].clone());
            } else {
                warn!("Filtered outlier price: ${:.2} (z-score: {:.2})", price, modified_z_score);
            }
        }
        
        if filtered.is_empty() {
            anyhow::bail!("All prices were filtered as outliers");
        }
        
        Ok(filtered)
    }
    
    /// Calculate consensus price using multiple statistical methods
    fn calculate_consensus(&self, prices: &[PriceData]) -> Result<f64> {
        let values: Vec<f64> = prices.iter()
            .map(|p| self.normalize_price(p))
            .collect();
        
        if values.is_empty() {
            anyhow::bail!("No valid prices for consensus calculation");
        }
        
        // Method 1: Median (most manipulation-resistant)
        let median_price = self.calculate_median(values.clone());
        
        // Method 2: Confidence-weighted average
        let weighted_avg = self.confidence_weighted_average(prices)?;
        
        // Method 3: Volume-weighted average (if volume data available)
        let volume_weighted = self.volume_weighted_average(prices).unwrap_or(median_price);
        
        // Combine methods with different weights
        let consensus = median_price * 0.5 +           // 50% median (manipulation resistant)
                       weighted_avg * 0.3 +           // 30% confidence weighted
                       volume_weighted * 0.2;         // 20% volume weighted
        
        debug!("Consensus methods - Median: {:.2}, Weighted: {:.2}, Volume: {:.2}, Final: {:.2}",
               median_price, weighted_avg, volume_weighted, consensus);
        
        Ok(consensus)
    }
    
    /// Calculate confidence-weighted average
    fn confidence_weighted_average(&self, prices: &[PriceData]) -> Result<f64> {
        let mut weighted_sum = 0.0;
        let mut total_weight = 0.0;
        
        for price in prices {
            let normalized_price = self.normalize_price(price);
            
            // Weight inversely proportional to confidence interval
            // Lower confidence interval = higher weight
            let confidence_ratio = price.confidence as f64 / price.price as f64;
            let weight = 1.0 / (1.0 + confidence_ratio * 10.0); // Adjust multiplier as needed
            
            weighted_sum += normalized_price * weight;
            total_weight += weight;
        }
        
        if total_weight == 0.0 {
            anyhow::bail!("Zero total weight in confidence calculation");
        }
        
        Ok(weighted_sum / total_weight)
    }
    
    /// Calculate volume-weighted average (placeholder for future enhancement)
    fn volume_weighted_average(&self, prices: &[PriceData]) -> Option<f64> {
        // TODO: Implement volume weighting when volume data becomes available
        // For now, fall back to simple average
        if prices.is_empty() {
            return None;
        }
        
        let sum: f64 = prices.iter().map(|p| self.normalize_price(p)).sum();
        Some(sum / prices.len() as f64)
    }
    
    /// Calculate aggregated confidence interval
    fn calculate_confidence(&self, prices: &[PriceData]) -> u64 {
        if prices.is_empty() {
            return u64::MAX; // Maximum uncertainty if no data
        }
        
        // Calculate combined confidence using root mean square
        let confidence_sum: f64 = prices.iter()
            .map(|p| {
                let conf_ratio = p.confidence as f64 / p.price as f64;
                conf_ratio * conf_ratio
            })
            .sum();
        
        let rms_confidence = (confidence_sum / prices.len() as f64).sqrt();
        let combined_price = prices.iter()
            .map(|p| self.normalize_price(p))
            .sum::<f64>() / prices.len() as f64;
        
        // Convert back to absolute confidence value
        (rms_confidence * combined_price * 10_f64.powi(8)) as u64
    }
    
    /// Detect potential manipulation attempts
    pub fn detect_manipulation(&self, prices: &[PriceData], historical_avg: f64) -> Vec<ManipulationAlert> {
        let mut alerts = Vec::new();
        
        let current_values: Vec<f64> = prices.iter()
            .map(|p| self.normalize_price(p))
            .collect();
        
        // Check for flash crash detection
        for (i, &price) in current_values.iter().enumerate() {
            let deviation = (price - historical_avg).abs() / historical_avg;
            
            if deviation > 0.1 { // 10% deviation threshold
                alerts.push(ManipulationAlert {
                    alert_type: ManipulationType::FlashCrash,
                    source: prices[i].source.clone(),
                    deviation: deviation,
                    price: price,
                    expected: historical_avg,
                });
            }
        }
        
        // Check for suspiciously tight clustering (potential coordination)
        if current_values.len() > 1 {
            let price_variance = current_values.clone().variance();
            let mean_price = current_values.mean();
            
            if price_variance / (mean_price * mean_price) < 0.0001 { // Very low relative variance
                alerts.push(ManipulationAlert {
                    alert_type: ManipulationType::SuspiciousConsensus,
                    source: PriceSource::Aggregated,
                    deviation: price_variance.sqrt() / mean_price,
                    price: mean_price,
                    expected: historical_avg,
                });
            }
        }
        
        alerts
    }
}

/// Types of manipulation that can be detected
#[derive(Debug, Clone)]
pub enum ManipulationType {
    FlashCrash,
    SuspiciousConsensus,
    OutlierAttack,
    TimestampManipulation,
}

/// Manipulation alert structure
#[derive(Debug, Clone)]
pub struct ManipulationAlert {
    pub alert_type: ManipulationType,
    pub source: PriceSource,
    pub deviation: f64,
    pub price: f64,
    pub expected: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PriceSource;
    
    fn create_test_symbol() -> Symbol {
        Symbol {
            name: "BTC/USD".to_string(),
            pyth_feed_id: "test".to_string(),
            switchboard_aggregator: "test".to_string(),
            max_staleness: 300,
            max_confidence: 10000,
            max_deviation: 100,
        }
    }
    
    #[test]
    fn test_price_aggregation() {
        let aggregator = PriceAggregator::new();
        let symbol = create_test_symbol();
        
        let prices = vec![
            PriceData {
                price: 50000_00000000,
                confidence: 500_00000,
                expo: -8,
                timestamp: 1000,
                source: PriceSource::Pyth,
                symbol: "BTC/USD".to_string(),
            },
            PriceData {
                price: 50050_00000000,
                confidence: 1000_00000,
                expo: -8,
                timestamp: 1001,
                source: PriceSource::Switchboard,
                symbol: "BTC/USD".to_string(),
            },
        ];
        
        let result = aggregator.aggregate_prices(&prices, &symbol);
        assert!(result.is_ok());
        
        let aggregated = result.unwrap();
        assert!(aggregated.price > 0);
        assert_eq!(aggregated.source, PriceSource::Aggregated);
    }
    
    #[test]
    fn test_outlier_detection() {
        let aggregator = PriceAggregator::new();
        
        // Create prices where one is clearly an outlier
        let prices = vec![50000.0, 50010.0, 50020.0, 100000.0]; // Last one is outlier
        let original_data = vec![
            PriceData {
                price: 50000_00000000,
                confidence: 500_00000,
                expo: -8,
                timestamp: 1000,
                source: PriceSource::Pyth,
                symbol: "BTC/USD".to_string(),
            },
            PriceData {
                price: 50010_00000000,
                confidence: 500_00000,
                expo: -8,
                timestamp: 1001,
                source: PriceSource::Switchboard,
                symbol: "BTC/USD".to_string(),
            },
            PriceData {
                price: 50020_00000000,
                confidence: 500_00000,
                expo: -8,
                timestamp: 1002,
                source: PriceSource::Pyth,
                symbol: "BTC/USD".to_string(),
            },
            PriceData {
                price: 100000_00000000, // Outlier
                confidence: 500_00000,
                expo: -8,
                timestamp: 1003,
                source: PriceSource::Switchboard,
                symbol: "BTC/USD".to_string(),
            },
        ];
        
        let filtered = aggregator.filter_outliers(&prices, &original_data).unwrap();
        
        // Should filter out the outlier
        assert_eq!(filtered.len(), 3);
        assert!(filtered.iter().all(|p| p.price < 60000_00000000));
    }
}