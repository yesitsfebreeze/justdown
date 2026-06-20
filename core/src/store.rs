// The graph store: a single SQLite file. Replaces the flat graph.tsv. Nodes
// hold the retrieval contract + safety metadata; edges hold resolved @links so
// the graph can actually be traversed (links, paths, connectivity). `meta`
// stamps the schema version so a consumer can detect a format it predates.

use crate::jd::Node;
/// Graph store format version (lib-owned; was the CLI const).
pub const STORE_SCHEMA: i64 = 3;
use rusqlite::{params, Connection};
use std::path::Path;

pub struct Store {
    pub conn: Connection,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Source {
    /// repo-scoped cache: <root>/.bombshell/jd
    Local,
    /// machine-scoped cache: ~/.bombshell/jd (shared across repos)
    Global,
    /// the published index fetched over the network
    Online,
}

impl Source {
    /// On-disk tiers (repo Local + machine Global) are read from the filesystem
    /// and share `get`'s read path; only Online is fetched.
    pub fn is_local(self) -> bool {
        matches!(self, Source::Local | Source::Global)
    }

    /// Stable tier name for output envelopes.
    pub fn label(self) -> &'static str {
        match self {
            Source::Local => "local",
            Source::Global => "global",
            Source::Online => "online",
        }
    }
}

/// A node loaded for querying — the tsv-column view the original awk operated
/// on (array fields kept as their comma-joined strings), plus its source and
/// outbound link targets.
pub struct Row {
    pub source: Source,
    /// For online rows, the raw base URL of the remote this row was loaded from
    /// (so `get` fetches the file from the right belt repo). Empty for on-disk
    /// tiers and single-repo online, where the default raw base applies.
    pub origin: String,
    pub key: String,
    pub name: String,
    pub kind: String,
    pub description: String,
    pub purpose: String,
    pub tags: String,
    pub path: String,
    pub use_when: String,
    pub not_when: String,
    pub danger: String,
    pub side_effects: String,
    pub requires: String,
    pub category: String,
    pub run: String,
    pub has_fm: bool,
    pub links: Vec<String>,
}

const SCHEMA_SQL: &str = "
CREATE TABLE meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
CREATE TABLE node (
  key          TEXT PRIMARY KEY,
  name         TEXT NOT NULL,
  kind         TEXT NOT NULL,
  description  TEXT NOT NULL,
  purpose      TEXT NOT NULL,
  tags         TEXT NOT NULL,
  path         TEXT NOT NULL,
  use_when     TEXT NOT NULL,
  not_when     TEXT NOT NULL,
  danger       TEXT NOT NULL,
  side_effects TEXT NOT NULL,
  requires     TEXT NOT NULL,
  category     TEXT NOT NULL,
  run          TEXT NOT NULL,
  has_fm       INTEGER NOT NULL
);
CREATE TABLE edge (
  src TEXT NOT NULL,   -- node key
  dst TEXT NOT NULL    -- link target in key form (dir/name), may be unresolved
);
CREATE INDEX idx_node_kind     ON node(kind);
CREATE INDEX idx_node_category ON node(category);
CREATE INDEX idx_node_name     ON node(name);
CREATE INDEX idx_edge_src      ON edge(src);
CREATE INDEX idx_edge_dst      ON edge(dst);
";

fn join(v: &[String]) -> String {
    v.join(",")
}

impl Store {
    /// Read just the schema version of an existing store, or None if the file
    /// is absent / unreadable / not a jd store.
    pub fn schema_of(path: &Path) -> Option<i64> {
        if !path.exists() {
            return None;
        }
        let conn = Connection::open(path).ok()?;
        conn.query_row("SELECT value FROM meta WHERE key = 'schema'", [], |r| {
            r.get::<_, String>(0)
        })
        .ok()?
        .parse::<i64>()
        .ok()
    }

    /// Open an existing store read-only-ish (we never mutate during queries).
    pub fn open(path: &Path) -> rusqlite::Result<Store> {
        Ok(Store {
            conn: Connection::open(path)?,
        })
    }

    /// Load every node as a query Row, tagged with `source`, ordered by key.
    /// Each row carries its outbound link targets (from the edge table).
    pub fn load_rows(&self, source: Source) -> rusqlite::Result<Vec<Row>> {
        // outbound links per node, in insertion (first-seen) order
        let mut links: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        let mut estmt = self
            .conn
            .prepare("SELECT src,dst FROM edge ORDER BY rowid")?;
        let mut erows = estmt.query([])?;
        while let Some(r) = erows.next()? {
            let src: String = r.get(0)?;
            let dst: String = r.get(1)?;
            links.entry(src).or_default().push(dst);
        }
        let mut stmt = self.conn.prepare(
            "SELECT key,name,kind,description,purpose,tags,path,use_when,not_when,
                    danger,side_effects,requires,category,run,has_fm
             FROM node ORDER BY key",
        )?;
        let rows = stmt
            .query_map([], |r| {
                let key: String = r.get(0)?;
                let outbound = links.get(&key).cloned().unwrap_or_default();
                Ok(Row {
                    source,
                    origin: String::new(),
                    key,
                    name: r.get(1)?,
                    kind: r.get(2)?,
                    description: r.get(3)?,
                    purpose: r.get(4)?,
                    tags: r.get(5)?,
                    path: r.get(6)?,
                    use_when: r.get(7)?,
                    not_when: r.get(8)?,
                    danger: r.get(9)?,
                    side_effects: r.get(10)?,
                    requires: r.get(11)?,
                    category: r.get(12)?,
                    run: r.get(13)?,
                    has_fm: r.get::<_, i64>(14)? != 0,
                    links: outbound,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Build a fresh store at `path` from the given nodes, replacing any
    /// existing file. Writes nodes, resolved edges, and stamps meta.
    pub fn build(path: &Path, nodes: &[Node], producer: &str) -> rusqlite::Result<()> {
        let _ = std::fs::remove_file(path);
        let mut conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA_SQL)?;

        let tx = conn.transaction()?;
        {
            let mut ins_node = tx.prepare(
                "INSERT OR REPLACE INTO node
                 (key,name,kind,description,purpose,tags,path,use_when,not_when,
                  danger,side_effects,requires,category,run,has_fm)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
            )?;
            let mut ins_edge = tx.prepare("INSERT INTO edge (src,dst) VALUES (?1,?2)")?;
            for n in nodes {
                ins_node.execute(params![
                    n.key,
                    n.name,
                    n.kind,
                    n.description,
                    n.purpose,
                    join(&n.tags),
                    n.path,
                    join(&n.use_when),
                    join(&n.not_when),
                    n.danger,
                    join(&n.side_effects),
                    join(&n.requires),
                    n.category,
                    n.run,
                    n.has_frontmatter as i64,
                ])?;
                for dst in &n.links {
                    ins_edge.execute(params![n.key, dst])?;
                }
            }
            let mut ins_meta =
                tx.prepare("INSERT OR REPLACE INTO meta (key,value) VALUES (?1,?2)")?;
            ins_meta.execute(params!["schema", STORE_SCHEMA.to_string()])?;
            ins_meta.execute(params!["producer", producer])?;
            ins_meta.execute(params!["node_count", nodes.len().to_string()])?;
        }
        tx.commit()?;
        Ok(())
    }
}
