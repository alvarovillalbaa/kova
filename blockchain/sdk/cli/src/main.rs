use std::fs;

use anyhow::Context;
use clap::{Parser, Subcommand};
use ed25519_dalek::SigningKey;
use reqwest::blocking::Client;
use runtime::{CrossDomainMessage, DomainCall};
use sdk_rust::{
    build_cross_domain_relay_signed, build_cross_domain_send_signed, build_domain_execute_signed,
    build_transfer_signed,
};
use serde_json::json;
use uuid::Uuid;

#[derive(Parser, Debug)]
#[command(name = "kova-cli")]
#[command(about = "Kova dev CLI for sending txs and domain calls", long_about = None)]
struct Cli {
    /// Node RPC base URL (e.g., http://localhost:7000)
    #[arg(long, env = "KOVA_RPC", default_value = "http://localhost:7000")]
    rpc: String,

    /// Hex-encoded 32-byte ed25519 private key
    #[arg(long, env = "KOVA_SK")]
    sk: String,

    /// Chain id to tag txs
    #[arg(long, env = "KOVA_CHAIN_ID", default_value = "kova-devnet")]
    chain_id: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Simple transfer in the base chain
    Transfer {
        #[arg(long)]
        to: String,
        #[arg(long)]
        amount: u128,
        #[arg(long, default_value = "0")]
        nonce: u64,
    },
    /// Execute a domain call (EVM/WASM)
    DomainExecute {
        #[arg(long)]
        domain_id: String,
        /// JSON payload file path
        #[arg(long)]
        payload_path: String,
        #[arg(long, default_value = "3000000")]
        gas_limit: u64,
        #[arg(long, default_value = "0")]
        nonce: u64,
    },
    /// Send cross-domain message
    CrossSend {
        #[arg(long)]
        from_domain: String,
        #[arg(long)]
        to_domain: String,
        #[arg(long)]
        payload_path: String,
        #[arg(long)]
        fee: u128,
        #[arg(long, default_value = "0")]
        nonce: u64,
    },
    /// Relay a cross-domain message (expects JSON)
    CrossRelay {
        #[arg(long)]
        message_path: String,
        #[arg(long, default_value = "0")]
        nonce: u64,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let client = Client::new();
    let sk_bytes = hex::decode(cli.sk.trim_start_matches("0x"))
        .context("failed to decode secret key")?;
    let sk = SigningKey::from_bytes(
        sk_bytes
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("secret key must be 32 bytes"))?,
    );

    let tx = match cli.command {
        Commands::Transfer { to, amount, nonce } => {
            let mut dest = [0u8; 32];
            let decoded = hex::decode(to.trim_start_matches("0x"))
                .context("decode recipient address")?;
            for (i, b) in decoded.iter().take(32).enumerate() {
                dest[i] = *b;
            }
            build_transfer_signed(&cli.chain_id, dest, amount, &sk, nonce)?
        }
        Commands::DomainExecute {
            domain_id,
            payload_path,
            gas_limit,
            nonce,
        } => {
            let bytes = fs::read_to_string(&payload_path)
                .with_context(|| format!("reading payload at {payload_path}"))?;
            let payload: serde_json::Value = serde_json::from_str(&bytes)
                .with_context(|| format!("parsing json payload from {payload_path}"))?;
            let call = DomainCall {
                domain_id: Uuid::parse_str(&domain_id)
                    .context("invalid domain_id (uuid expected)")?,
                payload,
                raw: None,
                max_gas: Some(gas_limit),
            };
            build_domain_execute_signed(&cli.chain_id, call, &sk, nonce, gas_limit)?
        }
        Commands::CrossSend {
            from_domain,
            to_domain,
            payload_path,
            fee,
            nonce,
        } => {
            let bytes = fs::read_to_string(&payload_path)
                .with_context(|| format!("reading payload at {payload_path}"))?;
            let payload: serde_json::Value = serde_json::from_str(&bytes)
                .with_context(|| format!("parsing json payload from {payload_path}"))?;
            build_cross_domain_send_signed(
                &cli.chain_id,
                Uuid::parse_str(&from_domain).context("invalid from_domain")?,
                Uuid::parse_str(&to_domain).context("invalid to_domain")?,
                payload,
                fee,
                &sk,
                nonce,
            )?
        }
        Commands::CrossRelay { message_path, nonce } => {
            let bytes = fs::read_to_string(&message_path)
                .with_context(|| format!("reading message at {message_path}"))?;
            let msg: CrossDomainMessage = serde_json::from_str(&bytes)
                .with_context(|| format!("parsing message json from {message_path}"))?;
            build_cross_domain_relay_signed(&cli.chain_id, msg, &sk, nonce)?
        }
    };

    let payload = json!({ "tx": tx });
    let url = format!("{}/send_raw_tx", cli.rpc.trim_end_matches('/'));
    let res = client
        .post(&url)
        .json(&payload)
        .send()
        .context("sending tx to node")?;
    println!("status: {}", res.status());
    let text = res.text().unwrap_or_default();
    if !text.is_empty() {
        println!("{text}");
    }
    Ok(())
}
