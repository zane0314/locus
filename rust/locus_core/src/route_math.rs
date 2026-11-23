use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Prefix {
    address: u128,
    length: u8,
    bits: u8,
}

impl Prefix {
    fn contains(&self, other: &Prefix) -> bool {
        if self.bits != other.bits || self.length > other.length {
            return false;
        }
        if self.length == 0 {
            return true;
        }
        let shift = self.bits - self.length;
        (self.address >> shift) == (other.address >> shift)
    }

    fn halves(&self) -> [Prefix; 2] {
        let next_len = self.length + 1;
        let bit_pos = self.bits - next_len;
        [
            Prefix { address: self.address, length: next_len, bits: self.bits },
            Prefix { address: self.address | (1u128 << bit_pos), length: next_len, bits: self.bits },
        ]
    }

    fn format(&self) -> String {
        if self.bits == 32 {
            let ip = Ipv4Addr::from(self.address as u32);
            format!("{}/{}", ip, self.length)
        } else {
            // Match Java InetAddress.getByAddress(16 bytes).getHostAddress(): uncompressed 8 colon-separated hex groups.
            let b = self.address.to_be_bytes();
            let ip = Ipv6Addr::from(b);
            let segs = ip.segments();
            format!(
                "{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}/{}",
                segs[0], segs[1], segs[2], segs[3], segs[4], segs[5], segs[6], segs[7], self.length
            )
        }
    }
}

fn parse_prefix(value: &str) -> Result<Prefix, String> {
    let trimmed = value.trim();
    let parts: Vec<&str> = trimmed.split('/').collect();
    if parts.len() != 2 {
        return Err("路由前缀无效".to_string());
    }
    let ip: IpAddr = parts[0].parse().map_err(|_| "路由前缀无效".to_string())?;
    let (raw, bits): (u128, u8) = match ip {
        IpAddr::V4(v4) => (u32::from(v4) as u128, 32),
        IpAddr::V6(v6) => (u128::from(v6), 128),
    };
    let length: u8 = parts[1].parse().map_err(|_| "路由前缀长度无效".to_string())?;
    if length > bits {
        return Err("路由前缀长度无效".to_string());
    }
    let address = if length == 0 {
        0
    } else if length == bits {
        raw
    } else {
        let shift = bits - length;
        (raw >> shift) << shift
    };
    Ok(Prefix { address, length, bits })
}

fn remove_prefix(prefix: Prefix, excluded: &[Prefix], out: &mut Vec<Prefix>) {
    let mut has_relevant = false;
    for ex in excluded {
        if ex.bits == prefix.bits {
            if ex.contains(&prefix) {
                return;
            }
            if prefix.contains(ex) {
                has_relevant = true;
            }
        }
    }
    if !has_relevant {
        out.push(prefix);
        return;
    }
    for half in prefix.halves() {
        remove_prefix(half, excluded, out);
    }
}

pub fn subtract_routes(include: &[String], exclude: &[String]) -> Result<Vec<String>, String> {
    let mut excluded = Vec::with_capacity(exclude.len());
    for item in exclude {
        excluded.push(parse_prefix(item)?);
    }
    let mut out = Vec::new();
    for item in include {
        let prefix = parse_prefix(item)?;
        remove_prefix(prefix, &excluded, &mut out);
    }
    Ok(out.into_iter().map(|p| p.format()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtracts_ipv4_and_ipv6_cleanly() {
        let res = subtract_routes(
            &["10.0.0.0/8".into()],
            &["10.0.0.0/9".into()],
        ).unwrap();
        assert_eq!(res, vec!["10.128.0.0/9"]);

        let v6 = subtract_routes(
            &["fc00::/7".into()],
            &["fc00::/18".into()],
        ).unwrap();
        assert!(!v6.is_empty());
        assert!(v6.iter().all(|s| !s.starts_with("fc00:0:0:0:0:0:0:0/18")));
    }
}
