use std::io::Write;
use std::path::Path;

use rust_decimal::Decimal;
use rusqlite::Connection;

// ---------------------------------------------------------------------------
// Trade Journal Export — CSV and JSON formats for external analysis
// ---------------------------------------------------------------------------

/// Lightweight journal row for export.  Derived from the trades table.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct JournalEntry {
    pub id: i64,
    pub symbol: String,
    pub head: String,
    pub direction: String,
    pub regime: String,
    pub session: String,
    pub entry_price: String,
    pub sl_price: String,
    pub tp_price: String,
    pub lots: String,
    pub risk_pct: String,
    pub opened_at: i64,
    pub closed_at: Option<i64>,
    pub close_price: Option<String>,
    pub pnl_usd: Option<String>,
    pub r_multiple: Option<String>,
    pub close_reason: Option<String>,
}

/// Export trade history from the database to CSV.
pub fn export_trades_csv(
    conn: &Connection,
    path: &Path,
) -> Result<usize, Box<dyn std::error::Error>> {
    let trades = load_journal_entries(conn)?;
    let mut file = std::fs::File::create(path)?;

    writeln!(
        file,
        "id,symbol,head,direction,regime,session,entry_price,sl_price,tp_price,lots,risk_pct,opened_at,closed_at,close_price,pnl_usd,r_multiple,close_reason"
    )?;

    for t in &trades {
        writeln!(
            file,
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},\"{}\"",
            t.id,
            t.symbol,
            t.head,
            t.direction,
            t.regime,
            t.session,
            t.entry_price,
            t.sl_price,
            t.tp_price,
            t.lots,
            t.risk_pct,
            format_timestamp(t.opened_at),
            t.closed_at
                .map(format_timestamp)
                .unwrap_or_default(),
            t.close_price.as_deref().unwrap_or(""),
            t.pnl_usd.as_deref().unwrap_or(""),
            t.r_multiple.as_deref().unwrap_or(""),
            t.close_reason.as_deref().unwrap_or(""),
        )?;
    }

    Ok(trades.len())
}

/// Export trade history from the database to JSON.
pub fn export_trades_json(
    conn: &Connection,
    path: &Path,
) -> Result<usize, Box<dyn std::error::Error>> {
    let trades = load_journal_entries(conn)?;
    let json = serde_json::to_string_pretty(&trades)?;
    std::fs::write(path, json)?;
    Ok(trades.len())
}

/// Summary statistics for the journal.
#[derive(Debug)]
pub struct JournalSummary {
    pub total_trades: usize,
    pub closed_trades: usize,
    pub open_trades: usize,
    pub total_pnl: Decimal,
    pub symbols_traded: Vec<String>,
    pub heads_used: Vec<String>,
    pub date_range: Option<(String, String)>,
}

/// Generate a quick summary of the trade journal.
pub fn journal_summary(conn: &Connection) -> Result<JournalSummary, Box<dyn std::error::Error>> {
    let trades = load_journal_entries(conn)?;
    let closed = trades.iter().filter(|t| t.closed_at.is_some()).count();
    let open = trades.len() - closed;

    let total_pnl: Decimal = trades
        .iter()
        .filter_map(|t| t.pnl_usd.as_deref())
        .filter_map(|s| s.parse::<Decimal>().ok())
        .sum();

    let mut symbols: Vec<String> = trades.iter().map(|t| t.symbol.clone()).collect();
    symbols.sort();
    symbols.dedup();

    let mut heads: Vec<String> = trades.iter().map(|t| t.head.clone()).collect();
    heads.sort();
    heads.dedup();

    let date_range = if trades.is_empty() {
        None
    } else {
        let first = trades.iter().map(|t| t.opened_at).min().unwrap();
        let last = trades
            .iter()
            .filter_map(|t| t.closed_at)
            .max()
            .unwrap_or(first);
        Some((format_timestamp(first), format_timestamp(last)))
    };

    Ok(JournalSummary {
        total_trades: trades.len(),
        closed_trades: closed,
        open_trades: open,
        total_pnl,
        symbols_traded: symbols,
        heads_used: heads,
        date_range,
    })
}

fn load_journal_entries(
    conn: &Connection,
) -> Result<Vec<JournalEntry>, Box<dyn std::error::Error>> {
    let mut stmt = conn.prepare(
        "SELECT id, symbol, head, direction, regime, session, \
         entry_price, sl_price, tp_price, lots, risk_pct, \
         opened_at, closed_at, close_price, pnl_usd, r_multiple, close_reason \
         FROM trades ORDER BY opened_at",
    )?;

    let trades = stmt
        .query_map([], |row| {
            Ok(JournalEntry {
                id: row.get(0)?,
                symbol: row.get(1)?,
                head: row.get(2)?,
                direction: row.get(3)?,
                regime: row.get(4)?,
                session: row.get(5)?,
                entry_price: row.get(6)?,
                sl_price: row.get(7)?,
                tp_price: row.get(8)?,
                lots: row.get(9)?,
                risk_pct: row.get(10)?,
                opened_at: row.get(11)?,
                closed_at: row.get(12)?,
                close_price: row.get(13)?,
                pnl_usd: row.get(14)?,
                r_multiple: row.get(15)?,
                close_reason: row.get(16)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(trades)
}

fn format_timestamp(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| ts.to_string())
}
