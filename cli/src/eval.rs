// `jd eval` — score retrieval quality against eval/queries.tsv: precision@1 and
// MRR. Dev tooling that proves a ranking change helped instead of guessing. A
// port of the `eval` recipe, run in-process (no subprocess per query).

use crate::config::Config;

pub fn run(cfg: &Config) -> i32 {
    let qfile = cfg.root.join("eval/queries.tsv");
    let content = match std::fs::read_to_string(&qfile) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("jd: no eval file: {}", qfile.display());
            return 1;
        }
    };

    println!("{:<5} {:<40} {}", "RANK", "QUERY", "EXPECTED");

    let mut total = 0usize;
    let mut h1 = 0usize;
    let mut mrr = 0.0f64;

    for line in content.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut it = line.splitn(2, '\t');
        let query = it.next().unwrap_or("").trim();
        let expected = it.next().unwrap_or("").trim();
        if query.is_empty() || expected.is_empty() {
            continue;
        }

        let names = crate::query::ranked_names(cfg, query, 20);
        let rank = names.iter().position(|n| n == expected).map(|i| i + 1).unwrap_or(0);

        total += 1;
        if rank == 1 {
            h1 += 1;
        }
        if rank > 0 {
            mrr += 1.0 / rank as f64;
        }
        let rank_str = if rank > 0 { rank.to_string() } else { "MISS".to_string() };
        let qtrunc: String = query.chars().take(40).collect();
        println!("{:<5} {:<40} {}", rank_str, qtrunc, expected);
    }

    if total > 0 {
        println!(
            "\nprecision@1={:.3}   MRR={:.3}   (n={}, top-1 hits={})",
            h1 as f64 / total as f64,
            mrr / total as f64,
            total,
            h1
        );
    }
    0
}
