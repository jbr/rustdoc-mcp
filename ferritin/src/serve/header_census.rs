//! Temporary request-header census for sizing header-interning work in trillium-http.
//!
//! Enabled by `FERRITIN_HEADER_CENSUS=<path>`. For every request, records per header name:
//! occurrence count, the number of distinct values (as salted 64-bit hashes, never the
//! values themselves), and a length histogram. Also counts requests per HTTP version and
//! how often a name appears more than once in a request. The whole structure is rewritten
//! to `<path>` as JSON every [`DUMP_EVERY`] requests and on shutdown.

use std::{
    collections::{BTreeMap, HashSet},
    fs,
    hash::{BuildHasher, RandomState},
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use trillium::{Conn, Handler, HeaderName, HeaderValues};

const DUMP_EVERY: u64 = 100;

/// Upper edges of the value-length histogram buckets; the last bucket is open-ended.
/// 24 is `CompactString`'s inline capacity, the threshold below which interning can't win.
const LENGTH_EDGES: [usize; 6] = [24, 64, 128, 256, 512, 1024];

#[derive(Default)]
struct NameStats {
    occurrences: u64,
    /// Requests in which this name appeared with more than one value.
    multi_valued: u64,
    distinct: HashSet<u64>,
    lengths: [u64; LENGTH_EDGES.len() + 1],
    max_len: usize,
}

impl NameStats {
    fn record(&mut self, salt: &RandomState, values: &HeaderValues) {
        self.occurrences += 1;
        if values.len() > 1 {
            self.multi_valued += 1;
        }
        for value in values {
            let bytes: &[u8] = value.as_ref();
            self.distinct.insert(salt.hash_one(bytes));
            let bucket = LENGTH_EDGES
                .iter()
                .position(|&edge| bytes.len() <= edge)
                .unwrap_or(LENGTH_EDGES.len());
            self.lengths[bucket] += 1;
            self.max_len = self.max_len.max(bytes.len());
        }
    }
}

struct Census {
    salt: RandomState,
    requests: u64,
    by_version: BTreeMap<String, u64>,
    names: BTreeMap<String, NameStats>,
}

impl Census {
    fn record(&mut self, conn: &Conn) {
        self.requests += 1;
        *self
            .by_version
            .entry(conn.http_version().to_string())
            .or_default() += 1;
        for (name, values) in conn.request_headers() {
            let name: HeaderName<'_> = name;
            self.names
                .entry(name.as_ref().to_owned())
                .or_default()
                .record(&self.salt, values);
        }
    }

    fn write_json(&self, out: &mut impl Write) -> std::io::Result<()> {
        writeln!(out, "{{")?;
        writeln!(out, "  \"requests\": {},", self.requests)?;
        writeln!(out, "  \"by_version\": {{")?;
        let mut first = true;
        for (version, count) in &self.by_version {
            if !first {
                writeln!(out, ",")?;
            }
            first = false;
            write!(out, "    \"{version}\": {count}")?;
        }
        writeln!(out, "\n  }},")?;
        writeln!(out, "  \"length_edges\": {LENGTH_EDGES:?},")?;
        writeln!(out, "  \"names\": {{")?;
        let mut first = true;
        for (name, stats) in &self.names {
            if !first {
                writeln!(out, ",")?;
            }
            first = false;
            write!(
                out,
                "    \"{name}\": {{\"occurrences\": {}, \"multi_valued\": {}, \"distinct\": {}, \
                 \"max_len\": {}, \"lengths\": {:?}}}",
                stats.occurrences,
                stats.multi_valued,
                stats.distinct.len(),
                stats.max_len,
                stats.lengths,
            )?;
        }
        writeln!(out, "\n  }}")?;
        writeln!(out, "}}")
    }

    fn dump(&self, path: &PathBuf) {
        let tmp = path.with_extension("json.tmp");
        let result = fs::File::create(&tmp)
            .and_then(|mut file| self.write_json(&mut file))
            .and_then(|()| fs::rename(&tmp, path));
        if let Err(e) = result {
            log::warn!("header census: failed to write {}: {e}", path.display());
        }
    }
}

/// Handler that records request-header statistics. See the module docs.
pub(super) struct HeaderCensus {
    path: PathBuf,
    census: Arc<Mutex<Census>>,
}

impl HeaderCensus {
    /// `None` unless `FERRITIN_HEADER_CENSUS` names an output path.
    pub(super) fn from_env() -> Option<Self> {
        let path = PathBuf::from(std::env::var_os("FERRITIN_HEADER_CENSUS")?);
        log::info!("header census enabled, writing to {}", path.display());
        Some(Self {
            path,
            census: Arc::new(Mutex::new(Census {
                salt: RandomState::new(),
                requests: 0,
                by_version: BTreeMap::new(),
                names: BTreeMap::new(),
            })),
        })
    }
}

impl Handler for HeaderCensus {
    async fn run(&self, conn: Conn) -> Conn {
        let mut census = self.census.lock().unwrap_or_else(|e| e.into_inner());
        census.record(&conn);
        if census.requests.is_multiple_of(DUMP_EVERY) {
            census.dump(&self.path);
        }
        conn
    }
}

impl Drop for HeaderCensus {
    fn drop(&mut self) {
        self.census
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .dump(&self.path);
    }
}

impl std::fmt::Debug for HeaderCensus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeaderCensus")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}
