// The graph store: a single SQLite file. Replaces the flat graph.tsv. Nodes
// hold the retrieval contract + safety metadata; edges hold resolved @links so
// the graph can actually be traversed (links, paths, connectivity). `meta`
// stamps the schema version so a consumer can detect a format it predates.

use crate::jd::Node;
use crate::links::{self, Link, NameIndex, Resolve};
/// Graph store format version (lib-owned; was the CLI const). v4 adds the
/// `edge.kind` column so fuzzy (`@?`) edges are distinguished from direct ones.
pub const STORE_SCHEMA: i64 = 4;
use rusqlite::{params, Connection};
use std::path::Path;

pub struct Store {
    pub conn: Connection,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Source {
    /// repo-scoped home: <project>/.jd
    Local,
    /// machine-scoped home: ~/.jd (shared across repos)
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
    /// Resolved direct (`@name` / `@dir/name`) edge targets in key form — the
    /// traversable graph edges. Unresolved single names are kept verbatim.
    pub links: Vec<String>,
    /// Fuzzy (`@?term`) edge terms — re-ranked live on read, not graph edges.
    pub fuzzy: Vec<String>,
}

impl Row {
    /// A query Row from a parsed [`Node`], tagged `source`. Direct/fuzzy edges
    /// are split by form but single names are left unresolved (no corpus here);
    /// resolution against a full node set happens in [`Store::build`]. Used by
    /// hosts that rank over live-parsed files (the editor's resolve endpoint).
    pub fn from_node(n: &Node, source: Source) -> Row {
        let mut links = Vec::new();
        let mut fuzzy = Vec::new();
        for t in &n.links {
            match links::classify(t) {
                Link::Fuzzy(term) => fuzzy.push(term.to_string()),
                Link::Key(_) | Link::Name(_) => links.push(t.clone()),
            }
        }
        Row {
            source,
            origin: String::new(),
            key: n.key.clone(),
            name: n.name.clone(),
            kind: n.kind.clone(),
            description: n.description.clone(),
            purpose: n.purpose.clone(),
            tags: join(&n.tags),
            path: n.path.clone(),
            use_when: join(&n.use_when),
            not_when: join(&n.not_when),
            danger: n.danger.clone(),
            side_effects: join(&n.side_effects),
            requires: join(&n.requires),
            category: n.category.clone(),
            run: n.run.clone(),
            has_fm: n.has_frontmatter,
            links,
            fuzzy,
        }
    }
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
  src  TEXT NOT NULL,                      -- node key
  dst  TEXT NOT NULL,                      -- direct: target key (may be unresolved); fuzzy: the raw term
  kind TEXT NOT NULL DEFAULT 'direct'      -- 'direct' (@name/@dir/name) | 'fuzzy' (@?term)
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
        // outbound edges per node, in insertion (first-seen) order, split by
        // kind. The `kind` column is v4+; fall back to a kind-less query so a
        // pre-v4 store (e.g. an older online belt) still loads as all-direct.
        let mut links: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        let mut fuzzy: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        let has_kind = self
            .conn
            .prepare("SELECT src,dst,kind FROM edge LIMIT 0")
            .is_ok();
        let sql = if has_kind {
            "SELECT src,dst,kind FROM edge ORDER BY rowid"
        } else {
            "SELECT src,dst FROM edge ORDER BY rowid"
        };
        let mut estmt = self.conn.prepare(sql)?;
        let mut erows = estmt.query([])?;
        while let Some(r) = erows.next()? {
            let src: String = r.get(0)?;
            let dst: String = r.get(1)?;
            let is_fuzzy = has_kind && r.get::<_, String>(2)? == "fuzzy";
            if is_fuzzy {
                fuzzy.entry(src).or_default().push(dst);
            } else {
                links.entry(src).or_default().push(dst);
            }
        }
        drop(erows);
        drop(estmt);
        let mut stmt = self.conn.prepare(
            "SELECT key,name,kind,description,purpose,tags,path,use_when,not_when,
                    danger,side_effects,requires,category,run,has_fm
             FROM node ORDER BY key",
        )?;
        let rows = stmt
            .query_map([], |r| {
                let key: String = r.get(0)?;
                let outbound = links.get(&key).cloned().unwrap_or_default();
                let fuzz = fuzzy.get(&key).cloned().unwrap_or_default();
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
                    fuzzy: fuzz,
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

        // Corpus name index, so a bare `@name` edge can be stored as its
        // canonical `dir/name` key (one clean implementation, shared with lint).
        let idx = NameIndex::build(nodes.iter().map(|n| (n.key.as_str(), n.name.as_str())));

        let tx = conn.transaction()?;
        {
            let mut ins_node = tx.prepare(
                "INSERT OR REPLACE INTO node
                 (key,name,kind,description,purpose,tags,path,use_when,not_when,
                  danger,side_effects,requires,category,run,has_fm)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
            )?;
            let mut ins_edge =
                tx.prepare("INSERT INTO edge (src,dst,kind) VALUES (?1,?2,?3)")?;
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
                for token in &n.links {
                    let (dst, kind) = match links::classify(token) {
                        Link::Key(k) => (k.to_string(), "direct"),
                        Link::Name(name) => {
                            // resolve to the canonical key when unique; otherwise
                            // keep the bare name (lint flags it as unresolved).
                            let dst = match idx.resolve(name) {
                                Resolve::Unique(k) => k,
                                _ => name.to_string(),
                            };
                            (dst, "direct")
                        }
                        Link::Fuzzy(term) => (term.to_string(), "fuzzy"),
                    };
                    ins_edge.execute(params![n.key, dst, kind])?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jd;

    #[test]
    fn build_resolves_direct_names_and_marks_fuzzy_edges() {
        let dir = std::env::temp_dir().join("jd_store_links_test");
        let _ = std::fs::create_dir_all(&dir);
        let store = dir.join("graph.db");

        // `src` links to a bare `@glassmorphism` (resolves to the unique key),
        // a fuzzy `@?soft`, and an unresolvable bare `@ghost`.
        let target = jd::parse(
            "library/ui/glassmorphism.jd",
            "---\nname: glass\nkind: knowledge\ndescription: d\n---\nbody\n",
        );
        let src = jd::parse(
            "library/ui/card.jd",
            "---\nname: card\nkind: knowledge\ndescription: d\n---\nSee @glassmorphism and @?soft and @ghost.\n",
        );
        Store::build(&store, &[src, target], "test").unwrap();

        let rows = Store::open(&store)
            .unwrap()
            .load_rows(Source::Local)
            .unwrap();
        let card = rows.iter().find(|r| r.key == "ui/card").unwrap();
        // bare @glassmorphism resolved to its canonical key; @ghost kept verbatim.
        assert_eq!(card.links, vec!["ui/glassmorphism", "ghost"]);
        // @?soft is a fuzzy edge, kept separate from the graph edges.
        assert_eq!(card.fuzzy, vec!["soft"]);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
