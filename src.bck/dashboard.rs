use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
    Frame, Terminal,
};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;

use crate::{ArbTransaction, ArbTransactionInstruction, ScannerContext};
use crate::config::ScannerConfig;
use crate::grpc::{ConnectionStatus, GrpcConnectionStatus};

/// Dashboard struct that manages the TUI
pub struct Dashboard {
    scanner_context: Arc<ScannerContext>,
    config: ScannerConfig,
}

/// Aggregated metrics for intermediate mints
#[derive(Clone)]
struct MintMetrics {
    rank: usize,
    mint: Pubkey,
    arbs_count: usize,
    fails_count: usize,
    profit: i64,  // in lamports
    net_volume: i64,  // in lamports
    total_volume: u64,  // in lamports
    fees: u64,  // in lamports
    liquidity: u64,  // in lamports
}

/// Transaction counts by arb program
struct ProgramCounts {
    program_id: Pubkey,
    success_count: usize,
    fail_count: usize,
}

/// Transaction display data
#[derive(Clone)]
struct TransactionDisplay {
    signature: Signature,
    hops: usize,
    pnl: i64,  // in lamports
    fee: u64,  // in lamports
    is_success: bool,
    timestamp: i64,  // Unix timestamp in seconds
}

/// Mint owner (pool) information
#[derive(Clone)]
struct MintOwnerInfo {
    mint: Pubkey,
    authority: Pubkey,  // PDA/authority from token account data
    dex_program: Option<Pubkey>,  // Known DEX program, or None if still discovering
    liquidity: u64,  // in lamports
    last_updated: i64,  // Unix timestamp in seconds
}

impl Dashboard {
    /// Create a new Dashboard and run the TUI event loop
    pub fn new(scanner_context: Arc<ScannerContext>, config: ScannerConfig) -> Self {
        let dashboard = Self {
            scanner_context,
            config,
        };

        // Run the TUI in a blocking manner
        if let Err(e) = dashboard.run() {
            eprintln!("Dashboard error: {}", e);
        }

        dashboard
    }

    /// Run the main TUI event loop
    fn run(&self) -> io::Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // Run the app
        let result = self.run_app(&mut terminal);

        // Restore terminal
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen
        )?;
        terminal.show_cursor()?;

        if let Err(err) = result {
            eprintln!("Error: {:?}", err);
        }

        Ok(())
    }

    /// Main application loop
    fn run_app<B: ratatui::backend::Backend>(&self, terminal: &mut Terminal<B>) -> io::Result<()> {
        loop {
            terminal.draw(|f| self.ui(f))?;

            // Poll for events with a timeout
            if event::poll(Duration::from_millis(250))? {
                if let Event::Key(key) = event::read()? {
                    match key.code {
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            return Ok(());
                        }
                        KeyCode::Char('q') | KeyCode::Esc => {
                            return Ok(());
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    /// Render the UI
    fn ui(&self, f: &mut Frame) {
        // Get data to calculate dynamic sizes
        let connections = self.get_connections();
        let program_counts = self.get_program_counts();

        // Calculate dynamic section sizes
        let connections_height = 3 + connections.len() as u16;
        let programs_height = 3 + program_counts.len() as u16;

        // Create main layout with 6 sections
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(connections_height),  // Connection section (dynamic)
                Constraint::Length(programs_height),     // Transaction counts section (dynamic)
                Constraint::Min(0),                      // Mint metrics table
                Constraint::Min(0),                      // Transaction list
                Constraint::Min(0),                      // Mint owners list
                Constraint::Length(1),                   // Footer
            ])
            .split(f.area());

        // Section 1: Connection Status
        self.render_connections(f, chunks[0]);

        // Section 2: Transaction Counts by Arb Program
        self.render_transaction_counts(f, chunks[1]);

        // Section 3: Intermediate Mint Metrics
        self.render_mint_metrics(f, chunks[2]);

        // Section 4: Arb Transactions List
        self.render_transactions_list(f, chunks[3]);

        // Section 5: Mint Owners
        self.render_mint_owners(f, chunks[4]);

        // Section 6: Footer
        self.render_footer(f, chunks[5]);
    }

    /// Section 1: Render connection status
    fn render_connections(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let connections = self.get_connections();

        let mut lines = vec![Line::from("gRPC Connections:")];

        if connections.is_empty() {
            lines.push(Line::from("  No connections"));
        } else {
            for (endpoint, status) in connections {
                let (indicator, color) = match status.status {
                    ConnectionStatus::Connected => ("●", Color::Green),
                    ConnectionStatus::Connecting => ("◌", Color::Yellow),
                    ConnectionStatus::Disconnected => ("○", Color::Red),
                };

                lines.push(Line::from(vec![
                    Span::styled(format!("  {} ", indicator), Style::default().fg(color)),
                    Span::raw(format!("{} - Ping: {}ms", endpoint, status.ping)),
                ]));
            }
        }

        let paragraph = Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Connections"));
        f.render_widget(paragraph, area);
    }

    /// Section 2: Render transaction counts by arb program
    fn render_transaction_counts(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let program_counts = self.get_program_counts();

        let mut lines = vec![];

        let mut total_success = 0;
        let mut total_fail = 0;

        for pc in &program_counts {
            total_success += pc.success_count;
            total_fail += pc.fail_count;

            // Truncate program ID for display
            let program_str = pc.program_id.to_string();
            let display_str = if program_str.len() > 20 {
                format!("{}...{}", &program_str[..8], &program_str[program_str.len()-8..])
            } else {
                program_str
            };

            lines.push(Line::from(format!(
                "  {} | Success: {} | Failed: {} | Total: {}",
                display_str,
                pc.success_count,
                pc.fail_count,
                pc.success_count + pc.fail_count
            )));
        }

        // Add "ALL" row
        lines.insert(0, Line::from(
            Span::styled(
                format!("  ALL | Success: {} | Failed: {} | Total: {}",
                    total_success, total_fail, total_success + total_fail),
                Style::default().add_modifier(Modifier::BOLD)
            )
        ));

        let paragraph = Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Transaction Counts by Arb Program"));
        f.render_widget(paragraph, area);
    }

    /// Section 3: Render intermediate mint metrics table
    fn render_mint_metrics(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let mint_metrics = self.aggregate_mint_metrics();

        // Create header
        let header = Row::new(vec![
            "Rank", "Intermediate Mint Pubkey", "Arbs", "Fails",
            "Profit (Lamports)", "Net Volume", "Total Volume",
            "Fees (Lamports)", "Liquidity (SOL)"
        ])
        .style(Style::default().add_modifier(Modifier::BOLD))
        .bottom_margin(1);

        // Create rows
        let rows: Vec<Row> = mint_metrics.iter().map(|m| {
            // Get decimals for this mint
            let decimals = self.scanner_context.mints.get(&m.mint)
                .and_then(|ctx| ctx.decimals)
                .unwrap_or(9);  // Default to 9 if not found

            Row::new(vec![
                m.rank.to_string(),
                m.mint.to_string(),
                m.arbs_count.to_string(),
                m.fails_count.to_string(),
                format_signed_lamports_with_underscores(m.profit),
                format_amount_with_decimals(m.net_volume, decimals),
                format_amount_with_decimals_unsigned(m.total_volume, decimals),
                format_lamports_with_underscores(m.fees),
                format_sol_whole(m.liquidity as i64),
            ])
        }).collect();

        let table = Table::new(
            rows,
            [
                Constraint::Length(6),   // Rank
                Constraint::Length(44),  // Mint Pubkey (full)
                Constraint::Length(6),   // Arbs
                Constraint::Length(7),   // Fails
                Constraint::Length(14),  // Profit
                Constraint::Length(17),  // Net Volume
                Constraint::Length(19),  // Total Volume
                Constraint::Length(18),  // Fees
                Constraint::Length(16),  // Liquidity
            ],
        )
        .header(header)
        .block(Block::default().borders(Borders::ALL).title("Intermediate Mint Metrics"));

        f.render_widget(table, area);
    }

    /// Section 4: Render arb transactions list
    fn render_transactions_list(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let transactions = self.get_transactions_display();

        // Create header
        let header = Row::new(vec!["Signature", "Hops", "PnL (Lamports)", "Fee (Lamports)"])
            .style(Style::default().add_modifier(Modifier::BOLD))
            .bottom_margin(1);

        // Create rows with color coding for failed transactions
        let rows: Vec<Row> = transactions.iter().map(|tx| {
            let style = if tx.is_success {
                Style::default()
            } else {
                Style::default().fg(Color::DarkGray)
            };

            Row::new(vec![
                tx.signature.to_string(),
                tx.hops.to_string(),
                format_signed_lamports_with_underscores(tx.pnl),
                format_lamports_with_underscores(tx.fee),
            ])
            .style(style)
        }).collect();

        let table = Table::new(
            rows,
            [
                Constraint::Length(88),  // Signature (full)
                Constraint::Length(6),   // Hops
                Constraint::Length(18),  // PnL (Lamports with underscores)
                Constraint::Length(18),  // Fee (Lamports with underscores)
            ],
        )
        .header(header)
        .block(Block::default().borders(Borders::ALL).title("Arb Transactions"));

        f.render_widget(table, area);
    }

    /// Section 5: Render mint owners list
    fn render_mint_owners(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let mint_owners = self.get_mint_owners();

        // Create header
        let header = Row::new(vec!["Mint", "Authority (PDA)", "DEX Program", "Liquidity", "Updated"])
            .style(Style::default().add_modifier(Modifier::BOLD))
            .bottom_margin(1);

        // Create rows
        let rows: Vec<Row> = mint_owners.iter().map(|info| {
            let dex_status = match &info.dex_program {
                Some(program) => {
                    // Truncate program ID to first 8 chars
                    let program_str = program.to_string();
                    let truncated = format!("{}...", &program_str[..8]);
                    (truncated, Style::default().fg(Color::Green))
                },
                None => {
                    ("Discovering...".to_string(), Style::default().fg(Color::Yellow))
                }
            };

            Row::new(vec![
                Cell::from(truncate_pubkey(&info.mint.to_string())),
                Cell::from(truncate_pubkey(&info.authority.to_string())),
                Cell::from(dex_status.0).style(dex_status.1),
                Cell::from(format_sol_whole(info.liquidity as i64)),
                Cell::from(format_timestamp(info.last_updated)),
            ])
        }).collect();

        let table = Table::new(
            rows,
            [
                Constraint::Length(20),  // Mint (truncated)
                Constraint::Length(20),  // Authority (truncated)
                Constraint::Length(18),  // DEX Program
                Constraint::Length(15),  // Liquidity
                Constraint::Length(20),  // Last Updated
            ],
        )
        .header(header)
        .block(Block::default().borders(Borders::ALL).title("Watched Accounts & Relationships"));

        f.render_widget(table, area);
    }

    /// Section 6: Render footer with keybindings
    fn render_footer(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let footer = Paragraph::new("Ctrl+C / Esc / Q: quit")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(footer, area);
    }

    /// Get connection status from scanner context
    fn get_connections(&self) -> Vec<(String, GrpcConnectionStatus)> {
        self.scanner_context
            .connections
            .iter()
            .map(|entry| {
                let endpoint = entry.key().clone();
                let status = GrpcConnectionStatus {
                    status: match entry.value().status {
                        ConnectionStatus::Connected => ConnectionStatus::Connected,
                        ConnectionStatus::Connecting => ConnectionStatus::Connecting,
                        ConnectionStatus::Disconnected => ConnectionStatus::Disconnected,
                    },
                    ping: entry.value().ping,
                    last_update: entry.value().last_update,
                    ping_sent_at: entry.value().ping_sent_at,
                };
                (endpoint, status)
            })
            .collect()
    }

    /// Get transaction counts grouped by arb program
    fn get_program_counts(&self) -> Vec<ProgramCounts> {
        self.scanner_context.arb_program_counts
            .iter()
            .map(|entry| {
                let program_id = *entry.key();
                let stats = entry.value();
                ProgramCounts {
                    program_id,
                    success_count: stats.success_count,
                    fail_count: stats.fail_count,
                }
            })
            .collect()
    }

    /// Aggregate metrics for intermediate mints
    ///
    /// Calculations:
    /// - Net Volume = Sum(Buy amounts) - Sum(Sell amounts)
    /// - Total Volume = Sum(Buy amounts) + Sum(Sell amounts) + Sum(Fees)
    /// - Profit = Sum of PnL from successful transactions using this mint
    fn aggregate_mint_metrics(&self) -> Vec<MintMetrics> {
        let mut mint_map: HashMap<Pubkey, MintMetrics> = HashMap::new();

        // Parse base_asset once at the start
        let base_asset = match self.config.base_asset.parse::<Pubkey>() {
            Ok(pubkey) => pubkey,
            Err(_) => return vec![],  // If invalid, return empty metrics
        };

        // Collect all intermediate mints from transactions
        for entry in self.scanner_context.transactions.iter() {
            let tx = entry.value();

            // Filter: Only process if first token matches base_asset
            let first_token = self.get_first_token(tx);
            if first_token != Some(base_asset) {
                continue;  // Skip this transaction
            }

            let intermediate_mints = self.extract_intermediate_mints(tx);
            let pnl = calculate_pnl(tx);

            for mint in intermediate_mints {
                let metrics = mint_map.entry(mint).or_insert(MintMetrics {
                    rank: 0,
                    mint,
                    arbs_count: 0,
                    fails_count: 0,
                    profit: 0,
                    net_volume: 0,
                    total_volume: 0,
                    fees: 0,
                    liquidity: 0,
                });

                if tx.is_success {
                    metrics.arbs_count += 1;
                    metrics.profit += pnl;

                    // Calculate volume and fees from the transaction (only for successful txs)
                    let (buy, sell, fees) = self.calculate_volumes_for_mint(tx, &mint);
                    metrics.net_volume += buy as i64 - sell as i64;
                    metrics.total_volume += buy + sell + fees;
                    metrics.fees += fees;
                } else {
                    metrics.fails_count += 1;
                }
            }
        }

        // Add liquidity information from mints context (aggregate across all DEX pools)
        for entry in self.scanner_context.mints.iter() {
            let mint_ctx = entry.value();
            if let Some(metrics) = mint_map.get_mut(&mint_ctx.mint) {
                // Sum liquidity across all pools for this mint
                metrics.liquidity = mint_ctx.pools.iter()
                    .map(|pool_entry| pool_entry.value().liquidity)
                    .sum();
            }
        }

        // Sort by multiple criteria (normalized by decimals):
        // 1. Highest profit (descending)
        // 2. Highest net_volume (descending)
        // 3. Highest total_volume (descending)
        // 4. Lowest fees (ascending)
        // 5. Mint address (ascending)
        let mut metrics_vec: Vec<MintMetrics> = mint_map.into_values().collect();
        metrics_vec.sort_by(|a, b| {
            // Get decimals from scanner context
            let a_decimals = self.scanner_context.mints.get(&a.mint)
                .and_then(|ctx| ctx.decimals)
                .unwrap_or(9) as u32;  // Default to 9 (SOL standard)
            let b_decimals = self.scanner_context.mints.get(&b.mint)
                .and_then(|ctx| ctx.decimals)
                .unwrap_or(9) as u32;

            // Normalize to 9 decimals for comparison (multiply by 10^(9-decimals))
            // This converts everything to SOL-equivalent scale
            let a_profit_normalized = if a_decimals <= 9 {
                a.profit * 10_i64.pow(9 - a_decimals)
            } else {
                a.profit / 10_i64.pow(a_decimals - 9)
            };
            let b_profit_normalized = if b_decimals <= 9 {
                b.profit * 10_i64.pow(9 - b_decimals)
            } else {
                b.profit / 10_i64.pow(b_decimals - 9)
            };

            let a_net_volume_normalized = if a_decimals <= 9 {
                a.net_volume * 10_i64.pow(9 - a_decimals)
            } else {
                a.net_volume / 10_i64.pow(a_decimals - 9)
            };
            let b_net_volume_normalized = if b_decimals <= 9 {
                b.net_volume * 10_i64.pow(9 - b_decimals)
            } else {
                b.net_volume / 10_i64.pow(b_decimals - 9)
            };

            let a_total_volume_normalized = if a_decimals <= 9 {
                a.total_volume * 10_u64.pow(9 - a_decimals)
            } else { 
                a.total_volume / 10_u64.pow(a_decimals - 9)
            };
            let b_total_volume_normalized = if b_decimals <= 9 {
                b.total_volume * 10_u64.pow(9 - b_decimals)
            } else {
                b.total_volume / 10_u64.pow(b_decimals - 9)
            };

            let a_fees_normalized = if a_decimals <= 9 {
                a.fees * 10_u64.pow(9 - a_decimals)
            } else {
                a.fees / 10_u64.pow(a_decimals - 9)
            };
            let b_fees_normalized = if b_decimals <= 9 {
                b.fees * 10_u64.pow(9 - b_decimals)
            } else {
                b.fees / 10_u64.pow(b_decimals - 9)
            };

            b_profit_normalized.cmp(&a_profit_normalized)  // Highest profit first
                .then_with(|| b_net_volume_normalized.cmp(&a_net_volume_normalized))  // Then highest net_volume
                .then_with(|| b_total_volume_normalized.cmp(&a_total_volume_normalized))  // Then highest total_volume
                .then_with(|| a_fees_normalized.cmp(&b_fees_normalized))  // Then lowest fees
                .then_with(|| a.mint.cmp(&b.mint))  // Finally, mint address (ascending) for deterministic ordering
        });

        for (idx, metric) in metrics_vec.iter_mut().enumerate() {
            metric.rank = idx + 1;
        }

        metrics_vec
    }

    /// Get the first token (base asset) from a transaction
    fn get_first_token(&self, tx: &ArbTransaction) -> Option<Pubkey> {
        tx.program_instructions
            .iter()
            .flat_map(|prog_inst| &prog_inst.arb_instructions)
            .filter_map(|inst| match inst {
                ArbTransactionInstruction::DexSwap(swap) => Some(swap.token_in.mint),
                _ => None,
            })
            .next()
    }

    /// Extract intermediate mints from a transaction
    /// Intermediate mints are tokens that appear in the middle of swap chains
    fn extract_intermediate_mints(&self, tx: &ArbTransaction) -> Vec<Pubkey> {
        let mut mints = Vec::new();
        let dex_swaps: Vec<&crate::dex::DexSwap> = tx.program_instructions.iter()
            .flat_map(|prog_inst| &prog_inst.arb_instructions)
            .filter_map(|inst| match inst {
                ArbTransactionInstruction::DexSwap(swap) => Some(swap),
                _ => None,
            })
            .collect();

        // If we have at least 2 swaps, the intermediate mints are the token_out of all
        // swaps except the last one (or equivalently, token_in of all swaps except the first)
        if dex_swaps.len() >= 2 {
            for swap in &dex_swaps[..dex_swaps.len()-1] {
                mints.push(swap.token_out.mint);
            }
        }

        mints
    }

    /// Calculate buy/sell volumes and fees for a specific mint in a transaction
    fn calculate_volumes_for_mint(&self, tx: &ArbTransaction, mint: &Pubkey) -> (u64, u64, u64) {
        let mut buy_volume = 0u64;
        let mut sell_volume = 0u64;
        let mut total_fees = 0u64;

        for program_instruction in &tx.program_instructions {
            for arb_instruction in &program_instruction.arb_instructions {
                if let ArbTransactionInstruction::DexSwap(swap) = arb_instruction {
                    // If token_out is our mint, it's a buy
                    if &swap.token_out.mint == mint {
                        buy_volume += swap.token_out.amount;
                    }
                    // If token_in is our mint, it's a sell
                    if &swap.token_in.mint == mint {
                        sell_volume += swap.token_in.amount;
                    }
                    // Sum fees involving this mint
                    for fee in &swap.fees {
                        if &fee.mint == mint {
                            total_fees += fee.amount;
                        }
                    }
                }
            }
        }

        (buy_volume, sell_volume, total_fees)
    }

    /// Get transactions for display
    fn get_transactions_display(&self) -> Vec<TransactionDisplay> {
        let mut transactions: Vec<TransactionDisplay> = self.scanner_context
            .transactions
            .iter()
            .map(|entry| {
                let tx = entry.value();
                TransactionDisplay {
                    signature: tx.signature,
                    hops: count_hops(tx),
                    pnl: calculate_pnl(tx),
                    fee: calculate_total_fees(tx),
                    is_success: tx.is_success,
                    timestamp: tx.timestamp,
                }
            })
            .collect();

        // Sort by timestamp (newest first)
        transactions.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        transactions
    }

    /// Get mint owners (pool) information with their relationships
    /// Only shows intermediate mints that appear in the metrics (have successful arbs)
    fn get_mint_owners(&self) -> Vec<MintOwnerInfo> {
        let mut mint_owners = Vec::new();

        // First, get the set of intermediate mints from transactions
        // This matches the same logic as aggregate_mint_metrics
        let intermediate_mints_with_arbs: std::collections::HashSet<Pubkey> = {
            let base_asset = match self.config.base_asset.parse::<Pubkey>() {
                Ok(pubkey) => pubkey,
                Err(_) => return vec![],
            };

            let mut mints = std::collections::HashSet::new();
            for entry in self.scanner_context.transactions.iter() {
                let tx = entry.value();

                // Filter: Only process if first token matches base_asset
                let first_token = self.get_first_token(tx);
                if first_token != Some(base_asset) {
                    continue;
                }

                // Add all intermediate mints from this transaction (both successful and failed)
                // This matches the logic in aggregate_mint_metrics
                let intermediate = self.extract_intermediate_mints(tx);
                mints.extend(intermediate);
            }
            mints
        };

        // Now iterate through mints and only include those with successful arbs
        for mint_entry in self.scanner_context.mints.iter() {
            let mint = mint_entry.key();

            // Skip if this mint is not in our intermediate mints with arbs
            if !intermediate_mints_with_arbs.contains(mint) {
                continue;
            }

            let mint_ctx = mint_entry.value();

            // Iterate through all pools for this mint
            for pool_entry in mint_ctx.pools.iter() {
                let pool_liquidity = pool_entry.value();
                let authority = pool_liquidity.owner;

                // Check if this authority is a known DEX program
                let dex_program = if self.scanner_context.known_dex_programs.contains_key(&authority) {
                    Some(authority)
                } else {
                    None
                };

                mint_owners.push(MintOwnerInfo {
                    mint: *mint,
                    authority,
                    dex_program,
                    liquidity: pool_liquidity.liquidity,
                    last_updated: pool_liquidity.last_updated,
                });
            }
        }

        // Sort by:
        // 1. Known DEX programs first (green)
        // 2. Then by mint
        // 3. Then by authority
        mint_owners.sort_by(|a, b| {
            match (a.dex_program.is_some(), b.dex_program.is_some()) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => match a.mint.cmp(&b.mint) {
                    std::cmp::Ordering::Equal => a.authority.cmp(&b.authority),
                    other => other,
                }
            }
        });

        mint_owners
    }
}

/// Calculate PnL for a transaction
/// PnL = last DexSwap token_out amount - first DexSwap token_in amount (in lamports)
fn calculate_pnl(tx: &ArbTransaction) -> i64 {
    let dex_swaps: Vec<&crate::dex::DexSwap> = tx.program_instructions.iter()
        .flat_map(|prog_inst| &prog_inst.arb_instructions)
        .filter_map(|inst| match inst {
            ArbTransactionInstruction::DexSwap(swap) => Some(swap),
            _ => None,
        })
        .collect();

    if let (Some(first), Some(last)) = (dex_swaps.first(), dex_swaps.last()) {
        last.token_out.amount as i64 - first.token_in.amount as i64
    } else {
        0
    }
}

/// Count the number of hops (DexSwap instructions) in a transaction
fn count_hops(tx: &ArbTransaction) -> usize {
    tx.program_instructions.iter()
        .flat_map(|prog_inst| &prog_inst.arb_instructions)
        .filter(|inst| matches!(inst, ArbTransactionInstruction::DexSwap(_)))
        .count()
}

/// Calculate total fees for a transaction
fn calculate_total_fees(tx: &ArbTransaction) -> u64 {
    tx.program_instructions.iter()
        .flat_map(|prog_inst| &prog_inst.arb_instructions)
        .filter_map(|inst| match inst {
            ArbTransactionInstruction::DexSwap(swap) => Some(swap),
            _ => None,
        })
        .flat_map(|swap| &swap.fees)
        .map(|fee| fee.amount)
        .sum()
}

/// Format lamports as SOL (whole numbers only)
fn format_sol_whole(lamports: i64) -> String {
    let sol = lamports / 1_000_000_000;
    sol.to_string()
}

/// Truncate a pubkey to show first 8 and last 4 characters
fn truncate_pubkey(pubkey: &str) -> String {
    if pubkey.len() <= 12 {
        return pubkey.to_string();
    }
    format!("{}...{}", &pubkey[..8], &pubkey[pubkey.len()-4..])
}

/// Format amount with specific decimals (returns whole number part only)
fn format_amount_with_decimals(amount: i64, decimals: u8) -> String {
    if decimals == 0 {
        return amount.to_string();
    }

    let divisor = 10_i64.pow(decimals as u32);
    let whole = amount / divisor;
    whole.to_string()
}

/// Format unsigned amount with specific decimals (returns whole number part only)
fn format_amount_with_decimals_unsigned(amount: u64, decimals: u8) -> String {
    if decimals == 0 {
        return amount.to_string();
    }

    let divisor = 10_u64.pow(decimals as u32);
    let whole = amount / divisor;
    whole.to_string()
}

/// Format lamports with underscore separators
fn format_lamports_with_underscores(lamports: u64) -> String {
    let s = lamports.to_string();
    let mut result = String::new();
    let mut count = 0;

    for c in s.chars().rev() {
        if count > 0 && count % 3 == 0 {
            result.push('_');
        }
        result.push(c);
        count += 1;
    }

    result.chars().rev().collect()
}

/// Format signed lamports with underscore separators (preserves negative sign)
fn format_signed_lamports_with_underscores(lamports: i64) -> String {
    let is_negative = lamports < 0;
    let abs_value = lamports.abs() as u64;
    let formatted = format_lamports_with_underscores(abs_value);

    if is_negative {
        format!("-{}", formatted)
    } else {
        formatted
    }
}

/// Format Unix timestamp as relative time or absolute time
fn format_timestamp(timestamp: i64) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let diff = now - timestamp;

    if diff < 60 {
        format!("{}s ago", diff)
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else {
        format!("{}d ago", diff / 86400)
    }
}
