//! 坐标流 diff（`spec/18` 7.2 的 `ReplaceInlines`）。
//!
//! Myers 的 O(ND) 版本，先剥掉公共前后缀。**token 就是一个 run**（`spec/18` 7.2：「坐标流 diff …
//! 聚到 run 边界」）——一个 run 的文本与它的 `w:rPr` 全同才算相等，改一个字或改一处格式，
//! 整个 run 就是删除 + 插入。这正是「接受后 = 不追踪做一遍」要求的：相等段保留原节点时，
//! 它的格式必须真的没变。
//!
//! 新旧两侧的 `w:rPr` 都化成 [`NewElement`] 再比——比较**保守**（属性顺序不同会判成不等），
//! 而保守只会让 diff 变粗，不会把不相等的东西判成相等。

use crate::xml::NewElement;

/// 坐标流里的一个 token。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Tok {
    /// 一个纯文本 run：文本 + `w:rPr`。
    Run(String, Option<Box<NewElement>>),
    /// 非纯文本的内联（图片 / 字段 / 制表符 / 超链接 …）：按整体比较，永远只当一个 token。
    Atom(String),
}

/// diff 脚本的一段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// 两侧各前进 `n` 个 token。
    Equal(usize),
    /// 旧侧删掉 `n` 个。
    Delete(usize),
    /// 新侧插入 `n` 个。
    Insert(usize),
}

/// token 数超过这个数就不做 Myers（退化成整体替换）——一段里的 run 数正常是几十个。
const MAX_TOKENS: usize = 4_000;

/// 旧 → 新的编辑脚本。返回 `None` 表示放弃（太大），调用方整体替换。
pub(crate) fn diff(old: &[Tok], new: &[Tok]) -> Option<Vec<Step>> {
    if old.len() > MAX_TOKENS || new.len() > MAX_TOKENS {
        return None;
    }
    let pre = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let rest_old = &old[pre..];
    let rest_new = &new[pre..];
    let suf = rest_old
        .iter()
        .rev()
        .zip(rest_new.iter().rev())
        .take_while(|(a, b)| a == b)
        .count()
        .min(rest_old.len())
        .min(rest_new.len());
    let a = &rest_old[..rest_old.len() - suf];
    let b = &rest_new[..rest_new.len() - suf];
    let mut steps = Vec::new();
    if pre > 0 {
        steps.push(Step::Equal(pre));
    }
    steps.extend(myers(a, b)?);
    if suf > 0 {
        steps.push(Step::Equal(suf));
    }
    Some(merge(steps))
}

/// 相邻的同类合并。
fn merge(steps: Vec<Step>) -> Vec<Step> {
    let mut out: Vec<Step> = Vec::with_capacity(steps.len());
    for s in steps {
        match (out.last_mut(), s) {
            (Some(Step::Equal(n)), Step::Equal(m)) => *n += m,
            (Some(Step::Delete(n)), Step::Delete(m)) => *n += m,
            (Some(Step::Insert(n)), Step::Insert(m)) => *n += m,
            _ if matches!(s, Step::Equal(0) | Step::Delete(0) | Step::Insert(0)) => {}
            _ => out.push(s),
        }
    }
    out
}

/// Myers O(ND)：记录每一轮的 `v`，走完再回溯出脚本。`d` 超过两侧长度之和就放弃。
fn myers(a: &[Tok], b: &[Tok]) -> Option<Vec<Step>> {
    let (n, m) = (a.len(), b.len());
    if n == 0 {
        return Some(vec![Step::Insert(m)]);
    }
    if m == 0 {
        return Some(vec![Step::Delete(n)]);
    }
    let max = n + m;
    let offset = max as isize;
    let mut v = vec![0usize; 2 * max + 1];
    let mut trace: Vec<Vec<usize>> = Vec::with_capacity(max + 1);
    for d in 0..=max as isize {
        trace.push(v.clone());
        let mut k = -d;
        while k <= d {
            let idx = (k + offset) as usize;
            let mut x = if k == -d || (k != d && v[idx - 1] < v[idx + 1]) {
                v[idx + 1]
            } else {
                v[idx - 1] + 1
            };
            let mut y = (x as isize - k) as usize;
            while x < n && y < m && a[x] == b[y] {
                x += 1;
                y += 1;
            }
            v[idx] = x;
            if x >= n && y >= m {
                return Some(backtrack(&trace, n, m, offset));
            }
            k += 2;
        }
    }
    None
}

/// 从 `trace` 反推脚本（Myers 的标准回溯，倒着生成再反转）。
fn backtrack(trace: &[Vec<usize>], n: usize, m: usize, offset: isize) -> Vec<Step> {
    let mut steps: Vec<Step> = Vec::new();
    let (mut x, mut y) = (n as isize, m as isize);
    for (d, v) in trace.iter().enumerate().rev() {
        let d = d as isize;
        let k = x - y;
        let idx = (k + offset) as usize;
        let prev_k = if k == -d || (k != d && v[idx - 1] < v[idx + 1]) { k + 1 } else { k - 1 };
        let prev_x = v[(prev_k + offset) as usize] as isize;
        let prev_y = prev_x - prev_k;
        while x > prev_x && y > prev_y {
            steps.push(Step::Equal(1));
            x -= 1;
            y -= 1;
        }
        if d > 0 {
            if x == prev_x {
                steps.push(Step::Insert(1));
            } else {
                steps.push(Step::Delete(1));
            }
        }
        x = prev_x;
        y = prev_y;
    }
    steps.reverse();
    steps
}
