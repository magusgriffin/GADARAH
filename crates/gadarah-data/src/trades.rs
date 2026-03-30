use rusqlite::{params, Connection};
use rust_decimal::Decimal;
use std::str::FromStr;

use crate::error::DataError;

#[derive(Debug)]
struct RawTradeRecord {
    id: i64,
    account_id: i64,
    symbol: String,
    direction: String,
    head: String,
    regime: String,
    session: String,
    entry_price: String,
    sl_price: String,
    tp_price: String,
    lots: String,
    risk_pct: String,
    pyramid_level: i32,
    opened_at: i64,
    closed_at: Option<i64>,
    close_price: Option<String>,
    pnl_usd: Option<String>,
    r_multiple: Option<String>,
    close_reason: Option<String>,
    slippage_pips: Option<String>,
}

#[derive(Debug)]
struct RawEquitySnapshot {
    id: i64,
    account_id: i64,
    balance: String,
    equity: String,
    daily_pnl_usd: String,
    daily_dd_pct: String,
    total_dd_pct: String,
    day_state: String,
    snapshotted_at: i64,
}

// ---------------------------------------------------------------------------
// Trade record (flat DB row — not the in-memory OpenPosition)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TradeRecord {
    pub id: Option<i64>,
    pub account_id: i64,
    pub symbol: String,
    pub direction: String,
    pub head: String,
    pub regime: String,
    pub session: String,
    pub entry_price: Decimal,
    pub sl_price: Decimal,
    pub tp_price: Decimal,
    pub lots: Decimal,
    pub risk_pct: Decimal,
    pub pyramid_level: i32,
    pub opened_at: i64,
    pub closed_at: Option<i64>,
    pub close_price: Option<Decimal>,
    pub pnl_usd: Option<Decimal>,
    pub r_multiple: Option<Decimal>,
    pub close_reason: Option<String>,
    pub slippage_pips: Option<Decimal>,
}

#[derive(Debug, Clone)]
pub struct TradeClose {
    pub trade_id: i64,
    pub closed_at: i64,
    pub close_price: Decimal,
    pub pnl_usd: Decimal,
    pub r_multiple: Decimal,
    pub close_reason: String,
    pub slippage_pips: Decimal,
}

/// Insert an open trade. Returns the new row ID.
pub fn insert_trade(conn: &Connection, t: &TradeRecord) -> Result<i64, DataError> {
    conn.execute(
        "INSERT INTO trades (account_id, symbol, direction, head, regime, session,
         entry_price, sl_price, tp_price, lots, risk_pct, pyramid_level, opened_at,
         closed_at, close_price, pnl_usd, r_multiple, close_reason, slippage_pips)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
        params![
            t.account_id,
            t.symbol,
            t.direction,
            t.head,
            t.regime,
            t.session,
            t.entry_price.to_string(),
            t.sl_price.to_string(),
            t.tp_price.to_string(),
            t.lots.to_string(),
            t.risk_pct.to_string(),
            t.pyramid_level,
            t.opened_at,
            t.closed_at,
            t.close_price.map(|d| d.to_string()),
            t.pnl_usd.map(|d| d.to_string()),
            t.r_multiple.map(|d| d.to_string()),
            t.close_reason,
            t.slippage_pips.map(|d| d.to_string()),
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Close a trade: set close fields.
pub fn close_trade(conn: &Connection, close: &TradeClose) -> Result<(), DataError> {
    conn.execute(
        "UPDATE trades SET closed_at=?1, close_price=?2, pnl_usd=?3,
         r_multiple=?4, close_reason=?5, slippage_pips=?6 WHERE id=?7",
        params![
            close.closed_at,
            close.close_price.to_string(),
            close.pnl_usd.to_string(),
            close.r_multiple.to_string(),
            close.close_reason.as_str(),
            close.slippage_pips.to_string(),
            close.trade_id,
        ],
    )?;
    Ok(())
}

/// Load all trades for an account, ordered by opened_at.
pub fn load_trades(conn: &Connection, account_id: i64) -> Result<Vec<TradeRecord>, DataError> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, account_id, symbol, direction, head, regime, session,
         entry_price, sl_price, tp_price, lots, risk_pct, pyramid_level,
         opened_at, closed_at, close_price, pnl_usd, r_multiple, close_reason, slippage_pips
         FROM trades WHERE account_id = ?1 ORDER BY opened_at ASC",
    )?;
    let rows = stmt.query_map(params![account_id], row_to_raw_trade)?;
    let trades: Result<Vec<_>, _> = rows.collect();
    trades?.into_iter().map(raw_to_trade).collect()
}

/// Load closed trades for an account in a time range.
pub fn load_closed_trades(
    conn: &Connection,
    account_id: i64,
    from_ts: i64,
    to_ts: i64,
) -> Result<Vec<TradeRecord>, DataError> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, account_id, symbol, direction, head, regime, session,
         entry_price, sl_price, tp_price, lots, risk_pct, pyramid_level,
         opened_at, closed_at, close_price, pnl_usd, r_multiple, close_reason, slippage_pips
         FROM trades WHERE account_id = ?1 AND closed_at IS NOT NULL
           AND closed_at >= ?2 AND closed_at <= ?3
         ORDER BY closed_at ASC",
    )?;
    let rows = stmt.query_map(params![account_id, from_ts, to_ts], row_to_raw_trade)?;
    let trades: Result<Vec<_>, _> = rows.collect();
    trades?.into_iter().map(raw_to_trade).collect()
}

fn parse_dec(field: &'static str, value: &str) -> Result<Decimal, DataError> {
    Decimal::from_str(value).map_err(|_| DataError::InvalidDecimal {
        field,
        value: value.to_string(),
    })
}

fn parse_opt_dec(field: &'static str, value: Option<String>) -> Result<Option<Decimal>, DataError> {
    value.map(|raw| parse_dec(field, &raw)).transpose()
}

fn row_to_raw_trade(row: &rusqlite::Row) -> rusqlite::Result<RawTradeRecord> {
    Ok(RawTradeRecord {
        id: row.get(0)?,
        account_id: row.get(1)?,
        symbol: row.get(2)?,
        direction: row.get(3)?,
        head: row.get(4)?,
        regime: row.get(5)?,
        session: row.get(6)?,
        entry_price: row.get(7)?,
        sl_price: row.get(8)?,
        tp_price: row.get(9)?,
        lots: row.get(10)?,
        risk_pct: row.get(11)?,
        pyramid_level: row.get(12)?,
        opened_at: row.get(13)?,
        closed_at: row.get(14)?,
        close_price: row.get(15)?,
        pnl_usd: row.get(16)?,
        r_multiple: row.get(17)?,
        close_reason: row.get(18)?,
        slippage_pips: row.get(19)?,
    })
}

fn raw_to_trade(raw: RawTradeRecord) -> Result<TradeRecord, DataError> {
    Ok(TradeRecord {
        id: Some(raw.id),
        account_id: raw.account_id,
        symbol: raw.symbol,
        direction: raw.direction,
        head: raw.head,
        regime: raw.regime,
        session: raw.session,
        entry_price: parse_dec("trades.entry_price", &raw.entry_price)?,
        sl_price: parse_dec("trades.sl_price", &raw.sl_price)?,
        tp_price: parse_dec("trades.tp_price", &raw.tp_price)?,
        lots: parse_dec("trades.lots", &raw.lots)?,
        risk_pct: parse_dec("trades.risk_pct", &raw.risk_pct)?,
        pyramid_level: raw.pyramid_level,
        opened_at: raw.opened_at,
        closed_at: raw.closed_at,
        close_price: parse_opt_dec("trades.close_price", raw.close_price)?,
        pnl_usd: parse_opt_dec("trades.pnl_usd", raw.pnl_usd)?,
        r_multiple: parse_opt_dec("trades.r_multiple", raw.r_multiple)?,
        close_reason: raw.close_reason,
        slippage_pips: parse_opt_dec("trades.slippage_pips", raw.slippage_pips)?,
    })
}

// ---------------------------------------------------------------------------
// Equity Snapshots
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct EquitySnapshot {
    pub id: Option<i64>,
    pub account_id: i64,
    pub balance: Decimal,
    pub equity: Decimal,
    pub daily_pnl_usd: Decimal,
    pub daily_dd_pct: Decimal,
    pub total_dd_pct: Decimal,
    pub day_state: String,
    pub snapshotted_at: i64,
}

pub fn insert_equity_snapshot(conn: &Connection, s: &EquitySnapshot) -> Result<i64, DataError> {
    conn.execute(
        "INSERT INTO equity_snapshots (account_id, balance, equity, daily_pnl_usd,
         daily_dd_pct, total_dd_pct, day_state, snapshotted_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        params![
            s.account_id,
            s.balance.to_string(),
            s.equity.to_string(),
            s.daily_pnl_usd.to_string(),
            s.daily_dd_pct.to_string(),
            s.total_dd_pct.to_string(),
            s.day_state,
            s.snapshotted_at,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn load_equity_snapshots(
    conn: &Connection,
    account_id: i64,
    from_ts: i64,
    to_ts: i64,
) -> Result<Vec<EquitySnapshot>, DataError> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, account_id, balance, equity, daily_pnl_usd, daily_dd_pct,
         total_dd_pct, day_state, snapshotted_at
         FROM equity_snapshots WHERE account_id = ?1
           AND snapshotted_at >= ?2 AND snapshotted_at <= ?3
         ORDER BY snapshotted_at ASC",
    )?;
    let rows = stmt.query_map(params![account_id, from_ts, to_ts], |row| {
        Ok(RawEquitySnapshot {
            id: row.get(0)?,
            account_id: row.get(1)?,
            balance: row.get(2)?,
            equity: row.get(3)?,
            daily_pnl_usd: row.get(4)?,
            daily_dd_pct: row.get(5)?,
            total_dd_pct: row.get(6)?,
            day_state: row.get(7)?,
            snapshotted_at: row.get(8)?,
        })
    })?;
    let snaps: Result<Vec<_>, _> = rows.collect();
    snaps?
        .into_iter()
        .map(|raw| {
            Ok(EquitySnapshot {
                id: Some(raw.id),
                account_id: raw.account_id,
                balance: parse_dec("equity_snapshots.balance", &raw.balance)?,
                equity: parse_dec("equity_snapshots.equity", &raw.equity)?,
                daily_pnl_usd: parse_dec("equity_snapshots.daily_pnl_usd", &raw.daily_pnl_usd)?,
                daily_dd_pct: parse_dec("equity_snapshots.daily_dd_pct", &raw.daily_dd_pct)?,
                total_dd_pct: parse_dec("equity_snapshots.total_dd_pct", &raw.total_dd_pct)?,
                day_state: raw.day_state,
                snapshotted_at: raw.snapshotted_at,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::init_schema;
    use rust_decimal_macros::dec;

    fn test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        conn
    }

    fn sample_trade() -> TradeRecord {
        TradeRecord {
            id: None,
            account_id: 1,
            symbol: "EURUSD".into(),
            direction: "Buy".into(),
            head: "Momentum".into(),
            regime: "StrongTrendUp".into(),
            session: "London".into(),
            entry_price: dec!(1.10000),
            sl_price: dec!(1.09800),
            tp_price: dec!(1.10400),
            lots: dec!(0.05),
            risk_pct: dec!(0.50),
            pyramid_level: 0,
            opened_at: 1700000000,
            closed_at: None,
            close_price: None,
            pnl_usd: None,
            r_multiple: None,
            close_reason: None,
            slippage_pips: None,
        }
    }

    #[test]
    fn trade_insert_and_close() {
        let conn = test_db();
        let t = sample_trade();
        let id = insert_trade(&conn, &t).unwrap();
        assert!(id > 0);

        close_trade(
            &conn,
            &TradeClose {
                trade_id: id,
                closed_at: 1700003600,
                close_price: dec!(1.10400),
                pnl_usd: dec!(20.00),
                r_multiple: dec!(2.0),
                close_reason: "TP".into(),
                slippage_pips: dec!(0.3),
            },
        )
        .unwrap();

        let trades = load_trades(&conn, 1).unwrap();
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].close_reason.as_deref(), Some("TP"));
        assert_eq!(trades[0].pnl_usd, Some(dec!(20.00)));
    }

    #[test]
    fn equity_snapshot_round_trip() {
        let conn = test_db();
        let snap = EquitySnapshot {
            id: None,
            account_id: 1,
            balance: dec!(10000),
            equity: dec!(10050),
            daily_pnl_usd: dec!(50),
            daily_dd_pct: dec!(0.0),
            total_dd_pct: dec!(0.0),
            day_state: "Active".into(),
            snapshotted_at: 1700000000,
        };
        let id = insert_equity_snapshot(&conn, &snap).unwrap();
        assert!(id > 0);

        let loaded = load_equity_snapshots(&conn, 1, 0, i64::MAX).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].balance, dec!(10000));
    }

    #[test]
    fn load_trades_rejects_invalid_decimal_text() {
        let conn = test_db();
        let t = sample_trade();
        let id = insert_trade(&conn, &t).unwrap();
        conn.execute(
            "UPDATE trades SET entry_price = ?1 WHERE id = ?2",
            params!["bad", id],
        )
        .unwrap();

        let err = load_trades(&conn, 1).unwrap_err();
        assert!(matches!(
            err,
            DataError::InvalidDecimal { field, value }
                if field == "trades.entry_price" && value == "bad"
        ));
    }

    #[test]
    fn load_equity_snapshots_rejects_invalid_decimal_text() {
        let conn = test_db();
        let snap = EquitySnapshot {
            id: None,
            account_id: 1,
            balance: dec!(10000),
            equity: dec!(10050),
            daily_pnl_usd: dec!(50),
            daily_dd_pct: dec!(0.0),
            total_dd_pct: dec!(0.0),
            day_state: "Active".into(),
            snapshotted_at: 1700000000,
        };
        let id = insert_equity_snapshot(&conn, &snap).unwrap();
        conn.execute(
            "UPDATE equity_snapshots SET balance = ?1 WHERE id = ?2",
            params!["bad", id],
        )
        .unwrap();

        let err = load_equity_snapshots(&conn, 1, 0, i64::MAX).unwrap_err();
        assert!(matches!(
            err,
            DataError::InvalidDecimal { field, value }
                if field == "equity_snapshots.balance" && value == "bad"
        ));
    }
}
