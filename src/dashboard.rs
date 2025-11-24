use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Row, Table, TableState},
    Frame, Terminal,
};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;

use crate::{ArbTransaction, ArbTransactionInstruction, ScannerContext};
use crate::grpc::{ConnectionStatus, GrpcConnectionStatus};

/// Dashboard struct that manages the TUI
pub struct Dashboard {
    scanner_context: Arc<ScannerContext>,
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
    timestamp: Option<i64>,  // Unix timestamp in seconds
}

impl Dashboard {
    /// Create a new Dashboard and run the TUI event loop
    pub fn new(scanner_context: Arc<ScannerContext>) -> Self {
        let dashboard = Self { scanner_context };

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
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // Run the app
        let result = self.run_app(&mut terminal);

        // Restore terminal
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        if let Err(err) = result {
            eprintln!("Error: {:?}", err);
        }

        Ok(())
    }

    /// Main application loop
    fn run_app<B: ratatui::backend::Backend>(&self, terminal: &mut Terminal<B>) -> io::Result<()> {
        let mut table_state = TableState::default();

        loop {
            terminal.draw(|f| self.ui(f, &mut table_state))?;

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
    fn ui(&self, f: &mut Frame, _table_state: &mut TableState) {
        // Create main layout with 5 sections
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(5),   // Connection section
                Constraint::Length(6),   // Transaction counts section
                Constraint::Percentage(30), // Mint metrics table
                Constraint::Percentage(30), // Transaction list
                Constraint::Length(1),   // Footer
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

        // Section 5: Footer
        self.render_footer(f, chunks[4]);
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
            "Profit (SOL)", "Net Volume (SOL)", "Total Volume (SOL)",
            "Fees (Lamports)", "Liquidity (SOL)"
        ])
        .style(Style::default().add_modifier(Modifier::BOLD))
        .bottom_margin(1);

        // Create rows
        let rows: Vec<Row> = mint_metrics.iter().map(|m| {
            Row::new(vec![
                m.rank.to_string(),
                m.mint.to_string(),
                m.arbs_count.to_string(),
                m.fails_count.to_string(),
                format_sol_whole(m.profit),
                format_sol_whole(m.net_volume),
                format_sol_whole(m.total_volume as i64),
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
        let header = Row::new(vec!["Signature", "Hops", "PnL (SOL)", "Fee (Lamports)"])
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
                format_sol_whole(tx.pnl),
                tx.fee.to_string(),
            ])
            .style(style)
        }).collect();

        let table = Table::new(
            rows,
            [
                Constraint::Length(88),  // Signature (full)
                Constraint::Length(6),   // Hops
                Constraint::Length(14),  // PnL
                Constraint::Length(16),  // Fee
            ],
        )
        .header(header)
        .block(Block::default().borders(Borders::ALL).title("Arb Transactions"));

        f.render_widget(table, area);
    }

    /// Section 5: Render footer with keybindings
    fn render_footer(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let footer = Paragraph::new("Press Ctrl+C, Esc, or Q to quit")
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
                };
                (endpoint, status)
            })
            .collect()
    }

    /// Get transaction counts grouped by arb program
    fn get_program_counts(&self) -> Vec<ProgramCounts> {
        let mut program_map: HashMap<Pubkey, (usize, usize)> = HashMap::new();

        for entry in self.scanner_context.transactions.iter() {
            let tx = entry.value();

            // Extract program ID from the first instruction
            if let Some(program_id) = self.extract_program_id(tx) {
                let counts = program_map.entry(program_id).or_insert((0, 0));
                if tx.is_success {
                    counts.0 += 1;
                } else {
                    counts.1 += 1;
                }
            }
        }

        program_map
            .into_iter()
            .map(|(program_id, (success_count, fail_count))| ProgramCounts {
                program_id,
                success_count,
                fail_count,
            })
            .collect()
    }

    /// Extract program ID from transaction instructions
    fn extract_program_id(&self, tx: &ArbTransaction) -> Option<Pubkey> {
        // Look for the first instruction that's not a token program
        // This is a simplified approach - in reality you'd match against config.arb_programs
        for instruction in &tx.instructions {
            match instruction {
                ArbTransactionInstruction::Raw(_inner) => {
                    // You would need access to the accounts array to get the program_id
                    // For now, return None as we don't have that information stored
                    // This would need to be enhanced to store program_id in ArbTransaction
                }
                ArbTransactionInstruction::DexSwap(_) => {
                    // DEX swaps don't directly give us the arb program
                }
            }
        }
        None
    }

    /// Aggregate metrics for intermediate mints
    ///
    /// Calculations:
    /// - Net Volume = Sum(Buy amounts) - Sum(Sell amounts)
    /// - Total Volume = Sum(Buy amounts) + Sum(Sell amounts) + Sum(Fees)
    /// - Profit = Sum of PnL from successful transactions using this mint
    fn aggregate_mint_metrics(&self) -> Vec<MintMetrics> {
        let mut mint_map: HashMap<Pubkey, MintMetrics> = HashMap::new();

        // Collect all intermediate mints from transactions
        for entry in self.scanner_context.transactions.iter() {
            let tx = entry.value();
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
                } else {
                    metrics.fails_count += 1;
                }

                // Calculate volume and fees from the transaction
                let (buy, sell, fees) = self.calculate_volumes_for_mint(tx, &mint);
                metrics.net_volume += buy as i64 - sell as i64;
                metrics.total_volume += buy + sell + fees;
                metrics.fees += fees;
            }
        }

        // Add liquidity information from mints context
        for entry in self.scanner_context.mints.iter() {
            let mint_ctx = entry.value();
            if let Some(metrics) = mint_map.get_mut(&mint_ctx.mint) {
                metrics.liquidity = mint_ctx.liquidity;
            }
        }

        // Sort by (net_volume + total_volume + fees) descending and assign ranks
        let mut metrics_vec: Vec<MintMetrics> = mint_map.into_values().collect();
        metrics_vec.sort_by(|a, b| {
            let a_total = a.net_volume.abs() as u64 + a.total_volume + a.fees;
            let b_total = b.net_volume.abs() as u64 + b.total_volume + b.fees;
            b_total.cmp(&a_total)
        });

        for (idx, metric) in metrics_vec.iter_mut().enumerate() {
            metric.rank = idx + 1;
        }

        metrics_vec
    }

    /// Extract intermediate mints from a transaction
    /// Intermediate mints are tokens that appear in the middle of swap chains
    fn extract_intermediate_mints(&self, tx: &ArbTransaction) -> Vec<Pubkey> {
        let mut mints = Vec::new();
        let dex_swaps: Vec<&crate::dex::DexSwap> = tx.instructions.iter()
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

        for instruction in &tx.instructions {
            if let ArbTransactionInstruction::DexSwap(swap) = instruction {
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
        transactions.sort_by(|a, b| {
            match (b.timestamp, a.timestamp) {
                (Some(b_ts), Some(a_ts)) => b_ts.cmp(&a_ts),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
        });

        transactions
    }
}

/// Calculate PnL for a transaction
/// PnL = last DexSwap token_out amount - first DexSwap token_in amount (in lamports)
fn calculate_pnl(tx: &ArbTransaction) -> i64 {
    let dex_swaps: Vec<&crate::dex::DexSwap> = tx.instructions.iter()
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
    tx.instructions.iter()
        .filter(|inst| matches!(inst, ArbTransactionInstruction::DexSwap(_)))
        .count()
}

/// Calculate total fees for a transaction
fn calculate_total_fees(tx: &ArbTransaction) -> u64 {
    tx.instructions.iter()
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
