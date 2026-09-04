//! 字节偏移 → UTF-16 索引（`COMPAT-06`）：每约 4 KiB 记一次累计值，查询二分 + 局部扫描。

/// 一个字符串的 UTF-16 索引表。
pub struct Utf16Index {
    /// `(字节偏移, 该偏移处累计的 UTF-16 单位数)`，字节偏移都是字符边界，升序，首项 `(0, 0)`。
    marks: Vec<(u32, u32)>,
}

const STEP: usize = 4096;

impl Utf16Index {
    pub fn new(s: &str) -> Utf16Index {
        let mut marks = vec![(0u32, 0u32)];
        let mut next = STEP;
        let mut units = 0u32;
        for (i, c) in s.char_indices() {
            if i >= next {
                marks.push((i as u32, units));
                next = i + STEP;
            }
            units += c.len_utf16() as u32;
        }
        Utf16Index { marks }
    }

    /// `byte` 须是 `s` 的字符边界（或 `s.len()`）。
    pub fn at(&self, s: &str, byte: u32) -> u32 {
        let idx = self.marks.partition_point(|&(b, _)| b <= byte) - 1;
        let (base_byte, base_units) = self.marks[idx];
        let slice = &s[base_byte as usize..byte as usize];
        base_units + slice.chars().map(|c| c.len_utf16() as u32).sum::<u32>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compat_06_utf16_index_matches_encode_utf16() {
        let mut s = String::new();
        for i in 0..3000 {
            s.push_str(match i % 4 {
                0 => "a",
                1 => "é",
                2 => "中",
                _ => "😀",
            });
        }
        let idx = Utf16Index::new(&s);
        for (b, _) in s.char_indices().step_by(97) {
            assert_eq!(idx.at(&s, b as u32), s[..b].encode_utf16().count() as u32);
        }
        assert_eq!(idx.at(&s, s.len() as u32), s.encode_utf16().count() as u32);
        assert_eq!(idx.at(&s, 0), 0);
    }
}
