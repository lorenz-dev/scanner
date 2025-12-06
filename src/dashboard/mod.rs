use std::io;
use std::sync::Arc;
use anyhow::Result;
use dashmap::DashMap;
use std::time::Duration;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers}, execute, terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode}
};
use ratatui::{
    Frame, Terminal, backend::CrosstermBackend, layout::{Constraint, Direction, Layout, Rect}, style::{Color, Style}, text::{Line, Span}, widgets::{Block, Borders, Paragraph, Row, Table}
};

use crate::{ScannerContext, config::ScannerConfig, grpc::{ConnectionContext, ConnectionStatus}, ProcessedTransaction, ParseInnerInstructionEnum, Mint, MintData, utils};

pub struct Dashboard {
}

impl Dashboard {
    pub fn new() -> Self {
        Self {}
    }

    pub fn run(&self, ctx: &ScannerContext, config: &ScannerConfig) -> Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        loop {
            let endpoints_len = config.grpc.endpoints.len() as u16;
            let arb_programs_len = config.arb_programs.len() as u16;

            let _ = terminal.draw(|f| {
                let areas = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(3 + endpoints_len),
                        Constraint::Length(5 + arb_programs_len),
                        Constraint::Min(0),
                        Constraint::Min(0),
                        Constraint::Length(1),
                    ])
                    .split(f.area());

                let connections = ctx.connections.read();
                self.render_connections(f, areas[0], &connections);
                self.render_arb_programs(f, areas[1], &ctx.arb_programs);

                let transactions = ctx.transactions.read();

                // Derive intermediate mint addresses from transaction swaps
                let mut intermediate_mint_addresses: std::collections::HashSet<String> = std::collections::HashSet::new();
                for tx in transactions.iter() {
                    // Extract all swaps
                    let mut all_swaps = Vec::new();
                    for instruction in &tx.instructions {
                        for inner in &instruction.inner_instrucions {
                            if let ParseInnerInstructionEnum::Swap(swap) = inner {
                                all_swaps.push(swap);
                            }
                        }
                    }

                    // Find intermediate mints
                    for i in 0..all_swaps.len().saturating_sub(1) {
                        let current_swap = all_swaps[i];
                        let next_swap = all_swaps[i + 1];

                        if current_swap.token_out == next_swap.token_in {
                            intermediate_mint_addresses.insert(current_swap.token_out.clone());
                        }
                    }
                }

                // Get actual mint data from ctx.mints for intermediate mints
                let mut mints = Vec::new();
                for mint_address in intermediate_mint_addresses {
                    if let Some(mint) = ctx.mints.get(&mint_address) {
                        mints.push(mint.clone());
                    }
                }

                self.render_mint_metrics(f, areas[2], &mints, &transactions, &ctx.token_account_data, &config.base_asset);
                self.render_transactions(f, areas[3], &transactions);
                self.render_footer(f, areas[4]);
            });

            // Poll for events with a timeout
            if event::poll(Duration::from_millis(250))? {
                if let Event::Key(key) = event::read()? {
                    match key.code {
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            break;
                        }
                        _ => {}
                    }
                }
            }
        };
    
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen
        )?;
        terminal.show_cursor()?;

        Ok(())
    }

    pub fn render_connections(&self, f: &mut Frame, r: Rect, connections_ctx: &Vec<ConnectionContext>) -> () {
        let mut lines = Vec::new();

        for connection in connections_ctx {
            let (connection_status, status_color) = match connection.status {
                ConnectionStatus::Connected => ("Connected", Color::Green),
                ConnectionStatus::Connecting => ("Connecting", Color::Yellow),
                ConnectionStatus::Disconnected => ("Disconnected", Color::Red),
            };

            let line = Line::from(vec![
                Span::raw(&connection.endpoint),
                Span::raw(" | ["),
                Span::styled(connection_status, Style::default().fg(status_color)),
                Span::raw("]"),
                Span::raw(format!(" | [{}ms]", connection.ping)),
            ]);

            lines.push(line);
        }

        let connections_widget = Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Connections"));

        f.render_widget(connections_widget, r);
    }

    pub fn render_arb_programs(&self, f: &mut Frame, r: Rect, arb_programs: &DashMap<String, Arc<(u64, u64)>>) -> () {
        let mut lines = Vec::new();

        let mut overall = (0u64, 0u64);

        lines.push(Line::from(vec![
            Span::styled("Overall:", Style::default().fg(Color::Cyan)),
            Span::raw(format!("  Successful Txs: {} | Failed Txs: {}",
                overall.0,
                overall.1,
            )),
        ]));
        lines.push(Line::from(""));

        for entry in arb_programs {
            let program = entry.key();
            let counts = entry.value();
            let (count0, count1) = **counts;
            overall.0 += count0;
            overall.1 += count1;

            lines.push(Line::from(vec![
                Span::raw("["),
                Span::styled(format!("{}", program), Style::default().fg(Color::Cyan)),
                Span::raw("]"),
                Span::raw(" | "),
                Span::styled(
                    format!(" ✓{}", count0),
                    Style::default().fg(Color::Green)
                ),
                Span::raw(" | "),
                Span::styled(
                    format!("✗{}", count1),
                    Style::default().fg(Color::Red)
                ),
            ]));
        }

        let widget = Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Arb Programs Metrics"));

        f.render_widget(widget, r);
    }

    pub fn render_mint_metrics(&self, f: &mut Frame, r: Rect, mints: &[Mint], transactions: &[ProcessedTransaction], token_account_data: &DashMap<String, MintData>, base_asset: &str) -> () {

        // Create a map of mint -> (decimals, arbs, fails, profit, net_volume, total_volume, fees, liquidity, has_liquidity_data)
        let mut mint_stats: std::collections::HashMap<String, (u32, u64, u64, i128, u64, u64, u64, u64, bool)> = std::collections::HashMap::new();

        // Initialize from mints list
        for mint in mints {
            mint_stats.entry(mint.program_id.clone())
                .or_insert((mint.decimals, 0, 0, 0, 0, 0, 0, 0, false));
        }

        // Aggregate transaction data by mint
        for tx in transactions {
            // Extract all swaps
            let mut all_swaps = Vec::new();
            for instruction in &tx.instructions {
                for inner in &instruction.inner_instrucions {
                    if let ParseInnerInstructionEnum::Swap(swap) = inner {
                        all_swaps.push(swap);
                    }
                }
            }

            // Calculate PnL for this transaction (in base_asset)
            let tx_pnl = if let (Some(first), Some(last)) = (all_swaps.first(), all_swaps.last()) {
                if first.token_in == base_asset && last.token_out == base_asset {
                    last.amount_out as i128 - first.amount_in as i128
                } else {
                    0
                }
            } else {
                0
            };

            // Track which mints were used in this transaction
            let mut tx_mints = std::collections::HashSet::new();
            for swap in &all_swaps {
                tx_mints.insert(swap.token_in.clone());
                tx_mints.insert(swap.token_out.clone());
            }

            // Update stats for each mint used in this transaction
            for mint_address in &tx_mints {
                if let Some(stats) = mint_stats.get_mut(mint_address) {
                    // Update arb/fail counts
                    if tx.err.is_none() {
                        stats.1 += 1; // arbs
                    } else {
                        stats.2 += 1; // fails
                    }

                    // Add profit (only if transaction was successful)
                    if tx.err.is_none() {
                        stats.3 += tx_pnl; // profit
                    }

                    // Calculate volume and fees for this mint
                    for swap in &all_swaps {
                        // Volume: track buys and sells
                        if swap.token_out == *mint_address {
                            // Buying this mint (amount_out is how much we got)
                            stats.4 = (stats.4 as i128 - swap.amount_out as i128).max(0) as u64; // net_volume -= buy
                            stats.5 += swap.amount_out; // total_volume += buy
                        }
                        if swap.token_in == *mint_address {
                            // Selling this mint (amount_in is how much we sold)
                            stats.4 += swap.amount_in; // net_volume += sell
                            stats.5 += swap.amount_in; // total_volume += sell
                        }

                        // Aggregate fees for this mint
                        for (fee_mint, fee_amount) in &swap.fees {
                            if fee_mint == mint_address {
                                stats.6 += fee_amount; // fees
                            }
                        }
                    }
                }
            }
        }

        // Aggregate liquidity from token_account_data
        // token_account_data is keyed by token_account, and MintData contains the mint
        for entry in token_account_data.iter() {
            let data = entry.value();
            let mint = &data.mint;
            if let Some(stats) = mint_stats.get_mut(mint) {
                stats.7 += data.liquidity; // liquidity is at index 7
                stats.8 = true; // has_liquidity_data is at index 8
            }
        }

        // Convert to sorted vector and sort by multiple criteria
        let mut mint_vec: Vec<_> = mint_stats.into_iter().collect();
        mint_vec.sort_by(|a, b| {
            // Sort by:
            // 1. Highest profit (descending)
            // 2. Highest net_volume (descending)
            // 3. Highest total_volume (descending)
            // 4. Lowest fees (ascending)
            // 5. Mint address (ascending)

            let profit_cmp = b.1.3.cmp(&a.1.3); // profit (descending)
            if profit_cmp != std::cmp::Ordering::Equal {
                return profit_cmp;
            }

            let net_volume_cmp = b.1.4.cmp(&a.1.4); // net_volume (descending)
            if net_volume_cmp != std::cmp::Ordering::Equal {
                return net_volume_cmp;
            }

            let total_volume_cmp = b.1.5.cmp(&a.1.5); // total_volume (descending)
            if total_volume_cmp != std::cmp::Ordering::Equal {
                return total_volume_cmp;
            }

            let fees_cmp = a.1.6.cmp(&b.1.6); // fees (ascending - lowest first)
            if fees_cmp != std::cmp::Ordering::Equal {
                return fees_cmp;
            }

            a.0.cmp(&b.0) // mint address (ascending)
        });

        // Create table rows
        let rows: Vec<Row> = mint_vec.iter()
            .enumerate()
            .map(|(idx, (mint, (decimals, arbs, fails, profit, net_volume, total_volume, fees, liquidity, has_liquidity_data)))| {
                let profit_str = if *profit >= 0 {
                    format!("+{}", utils::format_lamports(*profit as u64, None))
                } else {
                    format!("-{}", utils::format_lamports((-*profit) as u64, None))
                };

                let liquidity_str = if *has_liquidity_data {
                    utils::format_with_decimals(*liquidity, *decimals)
                } else {
                    "-".to_string()
                };

                Row::new(vec![
                    format!("#{}", idx + 1),
                    mint.clone(),
                    arbs.to_string(),
                    fails.to_string(),
                    profit_str,
                    utils::format_with_decimals(*net_volume, *decimals),
                    utils::format_with_decimals(*total_volume, *decimals),
                    utils::format_lamports(*fees, None),
                    liquidity_str,
                ])
            })
            .collect();

        let widths = [
            Constraint::Length(10),   // Rank
            Constraint::Length(60),  // Mint
            Constraint::Length(10),  // Arbs
            Constraint::Length(10),  // Fails
            Constraint::Length(20),  // Profit
            Constraint::Length(20),  // Net Volume
            Constraint::Length(20),  // Total Volume
            Constraint::Length(20),  // Fees
            Constraint::Min(20),     // Liquidity
        ];

        let table = Table::new(rows, widths)
            .header(
                Row::new(vec!["Rank", "Mint", "Arbs", "Fails", "Profit", "Net Vol", "Tot Vol", "Fees", "Liquidity"])
                    .style(Style::default().fg(Color::Cyan))
            )
            .block(Block::default().borders(Borders::ALL).title("Mint Metrics"));

        f.render_widget(table, r);
    }

    pub fn render_transactions(&self, f: &mut Frame, r: Rect, transactions: &[ProcessedTransaction]) -> () {
        // Create table rows
        let mut rows = Vec::new();

        for tx in transactions.iter().rev() {
            // Count swap instructions (hops)
            let mut swaps = Vec::new();
            for instruction in &tx.instructions {
                for inner in &instruction.inner_instrucions {
                    if let ParseInnerInstructionEnum::Swap(swap) = inner {
                        swaps.push(swap);
                    }
                }
            }

            let hops = swaps.len();

            // Calculate PnL: last amount_out - first amount_in
            let pnl = if let (Some(first), Some(last)) = (swaps.first(), swaps.last()) {
                let pnl_value = last.amount_out as i128 - first.amount_in as i128;
                if pnl_value >= 0 {
                    format!("+{}", utils::format_lamports(pnl_value as u64, None))
                } else {
                    format!("-{}", utils::format_lamports((-pnl_value) as u64, None))
                }
            } else {
                "0".to_string()
            };

            // Aggregate fees by mint
            let mut fee_map: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
            for swap in &swaps {
                for (mint, fee) in &swap.fees {
                    *fee_map.entry(mint.clone()).or_insert(0) += fee;
                }
            }

            let fees = if fee_map.is_empty() {
                "0".to_string()
            } else {
                fee_map.iter()
                    .map(|(mint, amount)| {
                        let short_mint = if mint.len() > 8 {
                            format!("{}..{}", &mint[..4], &mint[mint.len()-4..])
                        } else {
                            mint.clone()
                        };
                        format!("{}: {}", short_mint, utils::format_lamports(*amount, None))
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            };

            rows.push(Row::new(vec![
                tx.signature.clone(),
                hops.to_string(),
                pnl,
                fees,
            ]));
        }

        let widths = [
            Constraint::Length(88),  // Signature
            Constraint::Length(6),   // Hops
            Constraint::Length(20),  // PnL
            Constraint::Min(20),     // Fees
        ];

        let table = Table::new(rows, widths)
            .header(
                Row::new(vec!["Signature", "Hops", "PnL", "Fees"])
                    .style(Style::default().fg(Color::Cyan))
            )
            .block(Block::default().borders(Borders::ALL).title("Transactions"));

        f.render_widget(table, r);
    }

    pub fn render_footer(&self, f: &mut Frame, r: Rect) -> () {
        let footer = Line::from(vec![
            Span::raw(" "),
            Span::styled("Ctrl+C", Style::default().fg(Color::Yellow)),
            Span::raw(" to exit "),
        ]);

        let widget = Paragraph::new(footer)
            .style(Style::default().fg(Color::White));

        f.render_widget(widget, r);
    }
}

impl ScannerContext {
}