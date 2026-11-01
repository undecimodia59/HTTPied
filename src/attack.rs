//! Bounded lazy payload combinations. A plan stores sources and counters, never expanded requests.
use crate::payload::{MAX_PAYLOADS, Payloads};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
pub enum Mode {
    #[default]
    Sniper,
    Pitchfork,
    ClusterBomb,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
pub enum Encoding {
    #[default]
    None,
    Url,
    Base64,
}
pub struct Plan {
    sources: Vec<Payloads>,
    pub mode: Mode,
    pub encoding: Encoding,
    pub prefix: String,
    pub suffix: String,
    pub defaults: Vec<String>,
    index: usize,
    total: usize,
}
impl Plan {
    pub fn new(sources: Vec<Payloads>, mode: Mode) -> Result<Self, String> {
        if sources.is_empty() || sources.len() > 4 || sources.iter().any(Payloads::is_empty) {
            return Err("Choose 1–4 nonempty payload sets".into());
        }
        let total = match mode {
            Mode::Sniper => sources.iter().map(Payloads::len).sum(),
            Mode::Pitchfork => sources.iter().map(Payloads::len).min().unwrap(),
            Mode::ClusterBomb => sources
                .iter()
                .try_fold(1usize, |n, s| n.checked_mul(s.len()))
                .ok_or("Payload product overflow")?,
        };
        if total > MAX_PAYLOADS {
            return Err("Attack exceeds 1,000,000 requests".into());
        }
        Ok(Self {
            defaults: vec![String::new(); sources.len()],
            sources,
            mode,
            encoding: Encoding::None,
            prefix: String::new(),
            suffix: String::new(),
            index: 0,
            total,
        })
    }
    pub fn len(&self) -> usize {
        self.total - self.index
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn encode(&self, value: &str) -> String {
        let value = format!("{}{value}{}", self.prefix, self.suffix);
        match self.encoding {
            Encoding::None => value,
            Encoding::Url => {
                percent_encoding::utf8_percent_encode(&value, percent_encoding::NON_ALPHANUMERIC)
                    .to_string()
            }
            Encoding::Base64 => {
                use base64::Engine;
                base64::engine::general_purpose::STANDARD.encode(value)
            }
        }
    }
}
impl Iterator for Plan {
    type Item = Vec<String>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.total {
            return None;
        }
        let mut values = self.defaults.clone();
        let mut index = self.index;
        match self.mode {
            Mode::Sniper => {
                for (slot, source) in self.sources.iter().enumerate() {
                    if index < source.len() {
                        values[slot] = source.at(index)?;
                        break;
                    }
                    index -= source.len();
                }
            }
            Mode::Pitchfork => {
                for (slot, source) in self.sources.iter().enumerate() {
                    values[slot] = source.at(index)?;
                }
            }
            Mode::ClusterBomb => {
                for (slot, source) in self.sources.iter().enumerate().rev() {
                    values[slot] = source.at(index % source.len())?;
                    index /= source.len();
                }
            }
        }
        self.index += 1;
        Some(values.iter().map(|s| self.encode(s)).collect())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    fn source(values: &[&str]) -> Payloads {
        Payloads::lines(Arc::new(values.iter().map(|v| v.to_string()).collect())).unwrap()
    }
    #[test]
    fn combinations_are_lazy_and_attributed() {
        let sources = vec![source(&["a", "b"]), source(&["1", "2", "3"])];
        let cluster = Plan::new(sources.clone(), Mode::ClusterBomb).unwrap();
        assert_eq!(cluster.len(), 6);
        assert_eq!(cluster.collect::<Vec<_>>()[3], vec!["b", "1"]);
        let fork = Plan::new(sources.clone(), Mode::Pitchfork).unwrap();
        assert_eq!(
            fork.collect::<Vec<_>>(),
            vec![vec!["a", "1"], vec!["b", "2"]]
        );
        let sniper = Plan::new(sources, Mode::Sniper).unwrap();
        assert_eq!(sniper.len(), 5);
        assert_eq!(sniper.collect::<Vec<_>>()[2], vec!["", "1"]);
    }
    #[test]
    fn rejects_excessive_product_before_generation() {
        assert!(
            Plan::new(
                vec![Payloads::numbers("0", "9999", "1").unwrap(); 4],
                Mode::ClusterBomb
            )
            .is_err()
        );
    }
}
