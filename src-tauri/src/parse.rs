use regex::Regex;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum MinerEvent {
    Connected,
    Hashrate {
        hps: f64,
    },
    ShareAccepted,
    FoundBlock {
        height: Option<u64>,
        hash: Option<String>,
    },
    Error {
        message: String,
    },
}

pub fn parse_event(line: &str) -> Option<MinerEvent> {
    let l = line.to_lowercase();

    // very forgiving first pass; tighten once you know exact strings
    if l.contains("connected to") || l.contains("syncing") {
        return Some(MinerEvent::Connected);
    }
    // hashrate: "hashrate: 1234 H/s" or "H/s=1234.56"
    if let Some(ev) = parse_hashrate(&l) {
        return Some(ev);
    }
    if l.contains("share accepted") || l.contains("accepted share") {
        return Some(MinerEvent::ShareAccepted);
    }
    if l.contains("found block") || l.contains("contributed block") || l.contains("mined block") {
        let height = capture_u64(&l, r"height[ =:]+(\d+)");
        let hash = capture_str(&l, r"(?:hash|block)[ =:]+([0-9a-fx]+)");
        return Some(MinerEvent::FoundBlock { height, hash });
    }
    if l.contains("error") || l.contains("failed") {
        return Some(MinerEvent::Error {
            message: line.trim().to_string(),
        });
    }
    None
}

fn parse_hashrate(l: &str) -> Option<MinerEvent> {
    // try couple formats
    static RE1: once_cell::sync::Lazy<Regex> =
        once_cell::sync::Lazy::new(|| Regex::new(r"hashrate[:=]\s*([\d\.]+)\s*h/?s").unwrap());
    static RE2: once_cell::sync::Lazy<Regex> =
        once_cell::sync::Lazy::new(|| Regex::new(r"h/?s\s*=?\s*([\d\.]+)").unwrap());
    if let Some(c) = RE1.captures(l).or_else(|| RE2.captures(l)) {
        if let Ok(v) = c[1].parse::<f64>() {
            return Some(MinerEvent::Hashrate { hps: v });
        }
    }
    None
}

fn capture_u64(l: &str, pat: &str) -> Option<u64> {
    let re = Regex::new(pat).ok()?;
    let c = re.captures(l)?;
    c.get(1)?.as_str().parse::<u64>().ok()
}
fn capture_str(l: &str, pat: &str) -> Option<String> {
    let re = Regex::new(pat).ok()?;
    let c = re.captures(l)?;
    Some(c.get(1)?.as_str().to_string())
}
