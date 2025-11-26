/**
 * Oracle Integration System Demonstration
 * 
 * This script demonstrates the functionality of our Oracle Integration system
 * without requiring a full Solana program deployment.
 */

import { PublicKey } from "@solana/web3.js";

// Mock Oracle Configuration
interface OracleConfig {
  symbol: string;
  pythFeed: PublicKey;
  switchboardAggregator: PublicKey;
  maxStaleness: number;    // seconds
  maxConfidence: number;   // basis points
  maxDeviation: number;    // basis points
}

// Mock Price Data
interface PriceData {
  price: number;          // price with 8 decimals
  confidence: number;     // confidence interval
  expo: number;           // exponent
  timestamp: number;      // unix timestamp
  source: 'Pyth' | 'Switchboard' | 'Internal';
}

// Oracle Integration System
class OracleIntegrationSystem {
  private config: OracleConfig;

  constructor(config: OracleConfig) {
    this.config = config;
  }

  // Mock Pyth price fetching
  async getPythPrice(): Promise<PriceData> {
    console.log("📊 Fetching price from Pyth oracle...");
    
    // Simulate network delay
    await new Promise(resolve => setTimeout(resolve, 100));
    
    const currentTime = Math.floor(Date.now() / 1000);
    const mockPrice: PriceData = {
      price: 50000_00000000, // $50,000 with 8 decimals
      confidence: 1000000,   // $10 confidence
      expo: -8,              // 8 decimal places
      timestamp: currentTime,
      source: 'Pyth'
    };

    // Validate staleness
    if (currentTime - mockPrice.timestamp > this.config.maxStaleness) {
      throw new Error("Price is stale");
    }

    // Check if price is available
    if (mockPrice.price === 0) {
      throw new Error("Price unavailable");
    }

    // Validate confidence interval
    if (mockPrice.confidence > this.config.maxConfidence) {
      throw new Error("Low confidence");
    }

    console.log(`✅ Pyth Price: $${mockPrice.price / 100000000} (confidence: ±$${mockPrice.confidence / 100000000})`);
    return mockPrice;
  }

  // Mock Switchboard price fetching
  async getSwitchboardPrice(): Promise<PriceData> {
    console.log("📊 Fetching price from Switchboard oracle...");
    
    // Simulate network delay
    await new Promise(resolve => setTimeout(resolve, 150));
    
    const currentTime = Math.floor(Date.now() / 1000);
    const mockPrice: PriceData = {
      price: 50025_00000000, // $50,025 with 8 decimals
      confidence: 1500000,   // $15 confidence
      expo: -8,              // 8 decimal places
      timestamp: currentTime,
      source: 'Switchboard'
    };

    console.log(`✅ Switchboard Price: $${mockPrice.price / 100000000} (confidence: ±$${mockPrice.confidence / 100000000})`);
    return mockPrice;
  }

  // Validate price consensus from multiple sources
  validatePriceConsensus(prices: PriceData[]): number {
    console.log("🔍 Validating price consensus...");
    
    if (prices.length < 2) {
      throw new Error("Insufficient price sources");
    }

    // Calculate median price
    const sortedPrices = prices.map(p => p.price).sort((a, b) => a - b);
    const median = sortedPrices.length % 2 === 0
      ? (sortedPrices[sortedPrices.length / 2 - 1] + sortedPrices[sortedPrices.length / 2]) / 2
      : sortedPrices[Math.floor(sortedPrices.length / 2)];

    console.log(`📈 Median Price: $${median / 100000000}`);

    // Validate prices within threshold
    for (const priceData of prices) {
      const deviation = Math.abs(priceData.price - median) / median;
      console.log(`   ${priceData.source} deviation: ${(deviation * 100).toFixed(4)}%`);
      
      if (deviation > this.config.maxDeviation / 10000) { // Convert basis points to decimal
        throw new Error(`Price deviation too high for ${priceData.source}: ${(deviation * 100).toFixed(4)}%`);
      }
    }

    console.log("✅ Price consensus validated successfully!");
    return median;
  }

  // Main aggregation function
  async aggregatePrices(): Promise<number> {
    console.log(`🚀 Starting price aggregation for ${this.config.symbol}`);
    console.log(`   Max staleness: ${this.config.maxStaleness}s`);
    console.log(`   Max confidence: ${this.config.maxConfidence / 100}%`);
    console.log(`   Max deviation: ${this.config.maxDeviation / 100}%`);
    console.log("");

    try {
      // Fetch prices from multiple sources
      const [pythPrice, switchboardPrice] = await Promise.all([
        this.getPythPrice(),
        this.getSwitchboardPrice()
      ]);

      // Validate consensus
      const consensusPrice = this.validatePriceConsensus([pythPrice, switchboardPrice]);
      
      console.log("");
      console.log(`🎯 Final aggregated price for ${this.config.symbol}: $${consensusPrice / 100000000}`);
      
      return consensusPrice;
    } catch (error: any) {
      console.error("❌ Price aggregation failed:", error.message || error);
      throw error;
    }
  }
}

// Demonstration
async function demonstrateOracleIntegration() {
  console.log("=".repeat(60));
  console.log("🔮 Oracle Integration System Demonstration");
  console.log("=".repeat(60));
  console.log("");

  // Create oracle configuration
  const config: OracleConfig = {
    symbol: "BTC/USD",
    pythFeed: new PublicKey("GVXRSBjFk6e6J3NbVPXohDJetcTjaeeuykUpbQF8UoMU"), // BTC/USD Pyth feed
    switchboardAggregator: new PublicKey("8SXvChNYFhRq4EZuZvnhjrB3jJRQCv4k3P4W6hesH3Ee"), // Mock Switchboard aggregator
    maxStaleness: 300,     // 5 minutes
    maxConfidence: 2000000, // 200 basis points = 2% (more lenient for demo)
    maxDeviation: 500      // 50 basis points = 0.5%
  };

  const oracle = new OracleIntegrationSystem(config);

  try {
    await oracle.aggregatePrices();
    console.log("");
    console.log("✅ Oracle integration demonstration completed successfully!");
  } catch (error: any) {
    console.error("❌ Oracle integration demonstration failed:", error.message || error);
  }

  console.log("");
  console.log("=".repeat(60));
  console.log("📋 System Features Demonstrated:");
  console.log("   • Multi-oracle price aggregation");
  console.log("   • Price staleness validation");
  console.log("   • Confidence interval checking");
  console.log("   • Price deviation consensus validation");
  console.log("   • Error handling and validation");
  console.log("=".repeat(60));
}

// Run the demonstration
demonstrateOracleIntegration().catch(console.error);