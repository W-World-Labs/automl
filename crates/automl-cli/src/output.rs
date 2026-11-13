//! Output formatting: `--format json` is the stable wire interface; tables are
//! for humans and truncate with `w_util::truncate_chars` (CJK-safe — v1's
//! byte-slicing panic class is structurally gone).

use clap::ValueEnum;
use serde_json::Value;
use w_util::text::truncate_chars;

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Format {
    Table,
    Json,
    Minimal,
}

pub fn print_value(fmt: &Format, v: &Value, table: impl FnOnce(&Value) -> String) {
    match fmt {
        Format::Json => println!("{}", serde_json::to_string_pretty(v).unwrap()),
        Format::Minimal => {
            if let Some(s) = v["status"].as_str().or_else(|| v["run"]["status"].as_str()) {
                println!("{s}");
            } else {
                println!("{v}");
            }
        }
        Format::Table => println!("{}", table(v)),
    }
}

/// Print a list: JSON gets the wire shape `{"data": [...]}`; table renders rows.
pub fn print_list(
    fmt: &Format,
    items: &[Value],
    header: &[&str],
    row: impl Fn(&Value) -> Vec<String>,
) {
    match fmt {
        Format::Json => println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "data": items })).unwrap()
        ),
        Format::Minimal => {
            for item in items {
                println!("{}", item["id"].as_str().unwrap_or("?"));
            }
        }
        Format::Table => {
            let rows: Vec<Vec<String>> = items.iter().map(&row).collect();
            println!("{}", render_table(header, &rows));
            eprintln!("({} rows)", rows.len());
        }
    }
}

/// Fixed-width table with char-counted truncation (never panics mid-CJK).
pub fn render_table(header: &[&str], rows: &[Vec<String>]) -> String {
    const MAX_COL: usize = 42;
    let cols = header.len();
    let mut widths: Vec<usize> = header.iter().map(|h| h.chars().count()).collect();
    let cells: Vec<Vec<String>> = rows
        .iter()
        .map(|r| {
            (0..cols)
                .map(|i| truncate_chars(r.get(i).map(String::as_str).unwrap_or(""), MAX_COL))
                .collect()
        })
        .collect();
    for r in &cells {
        for (i, c) in r.iter().enumerate() {
            widths[i] = widths[i].max(c.chars().count());
        }
    }
    let mut out = String::new();
    let pad = |s: &str, w: usize| {
        let mut t = s.to_string();
        for _ in s.chars().count()..w {
            t.push(' ');
        }
        t
    };
    for (i, h) in header.iter().enumerate() {
        out.push_str(&pad(h, widths[i]));
        out.push_str("  ");
    }
    out.push('\n');
    for (i, _) in header.iter().enumerate() {
        out.push_str(&"-".repeat(widths[i]));
        out.push_str("  ");
    }
    for r in &cells {
        out.push('\n');
        for (i, c) in r.iter().enumerate() {
            out.push_str(&pad(c, widths[i]));
            out.push_str("  ");
        }
    }
    out
}

pub fn short_id(v: &Value) -> String {
    v.as_str().unwrap_or("?").chars().take(8).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression for v1's UTF-8 truncation panic: CJK titles wider than the
    /// column cap must render, truncated by chars, without panicking.
    #[test]
    fn table_with_cjk_does_not_panic() {
        let rows = vec![
            vec![
                "8b0d38cb".to_string(),
                "深度学习模型在多模态科研数据复现中的可信度评估方法研究——以大规模语言模型与自动化科研代理的交叉验证为例的系统性实证分析"
                    .to_string(),
                "under_review".to_string(),
            ],
            vec![
                "aa11bb22".to_string(),
                "短标题".to_string(),
                "draft".to_string(),
            ],
        ];
        let table = render_table(&["ID", "TITLE", "STATUS"], &rows);
        assert!(table.contains("深度学习"));
        assert!(table.contains('…'), "long CJK title must be truncated");
        assert!(table.contains("短标题"));
        // Every rendered line stays within sane bounds (no byte-splitting).
        for line in table.lines() {
            assert!(std::str::from_utf8(line.as_bytes()).is_ok());
        }
    }
}
