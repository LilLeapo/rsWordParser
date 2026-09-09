//! AGENT-10：所有载荷先落同目录临时文件，成功后发布；失败恢复已发布的旧输出。
use crate::{Result, error};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(1);
fn temp(parent: &Path) -> PathBuf {
    parent.join(format!(".rsword-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)))
}
#[derive(Debug)]
struct Item {
    dest: PathBuf,
    stage: PathBuf,
    backup: Option<PathBuf>,
    published: bool,
}
impl Drop for Item {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.stage);
        if let Some(b) = &self.backup {
            let _ = fs::remove_file(b);
        }
    }
}
pub fn io(e: impl std::fmt::Display) -> crate::QueryError {
    error("AGENT_IO", e.to_string())
}
#[derive(Debug)]
pub struct Publication {
    items: Vec<Item>,
    finalized: bool,
}
impl Publication {
    pub fn commit(mut self) {
        self.finalized = true;
    }
    pub fn rollback(mut self) -> Result<()> {
        self.undo()
    }
    fn undo(&mut self) -> Result<()> {
        self.finalized = true;
        let mut failures = vec![];
        for item in self.items.iter_mut().rev().filter(|i| i.published) {
            let result = if let Some(b) = &item.backup {
                fs::rename(b, &item.dest)
            } else {
                fs::remove_file(&item.dest)
            };
            if let Err(e) = result {
                let backup = item.backup.take();
                failures.push(format!("{e}; 原字节备份保留于 {backup:?}"));
            }
        }
        if failures.is_empty() { Ok(()) } else { Err(io(failures.join("; "))) }
    }
}
impl Drop for Publication {
    fn drop(&mut self) {
        if !self.finalized {
            let _ = self.undo();
        }
    }
}
pub fn publish(outputs: Vec<(PathBuf, Vec<u8>)>, overwrite: bool) -> Result<Publication> {
    publish_with(outputs, overwrite, |_, _| {})
}
fn publish_with(
    outputs: Vec<(PathBuf, Vec<u8>)>,
    overwrite: bool,
    mut before: impl FnMut(usize, &Path),
) -> Result<Publication> {
    let mut items = Vec::<Item>::new();
    let mut paths = std::collections::BTreeSet::new();
    for (dest, bytes) in outputs {
        let parent = dest
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .canonicalize()
            .map_err(io)?;
        let name = dest.file_name().ok_or_else(|| error("AGENT_BAD_ARGUMENT", "输出必须是文件"))?;
        let dest = parent.join(name);
        if !paths.insert(dest.clone()) {
            return Err(error("AGENT_BAD_ARGUMENT", "输出与报告路径重复"));
        }
        let exists = fs::symlink_metadata(&dest).is_ok();
        if exists && (!overwrite || !dest.is_file()) {
            return Err(error(
                "AGENT_OUTPUT_EXISTS",
                "输出已存在；只有显式 --overwrite 才能替换文件",
            ));
        }
        let stage = temp(&parent);
        // create_new 失败时尚未取得该路径的所有权，不能由 Drop 删除碰巧同名的文件。
        let mut file = OpenOptions::new().create_new(true).write(true).open(&stage).map_err(io)?;
        let mut item = Item { dest: dest.clone(), stage, backup: None, published: false };
        file.write_all(&bytes).map_err(io)?;
        if exists {
            file.set_permissions(fs::metadata(&dest).map_err(io)?.permissions()).map_err(io)?;
            let backup = temp(&parent);
            fs::hard_link(&dest, &backup).map_err(io)?;
            item.backup = Some(backup);
        }
        file.sync_all().map_err(io)?;
        items.push(item);
    }
    for i in 0..items.len() {
        before(i, &items[i].dest);
        let item = &mut items[i];
        let result = if item.backup.is_some() {
            fs::rename(&item.stage, &item.dest)
        } else {
            fs::hard_link(&item.stage, &item.dest)
        };
        if let Err(e) = result {
            Publication { items, finalized: false }
                .rollback()
                .map_err(|restore| io(format!("{e}; 回滚失败: {restore}")))?;
            return Err(io(e));
        }
        item.published = true;
    }
    Ok(Publication { items, finalized: false })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn agent_10_failed_second_publication_restores_first() {
        let dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/m96-output-rollback");
        fs::create_dir_all(&dir).unwrap();
        let first = dir.join("first");
        let second = dir.join("second");
        let _ = fs::remove_dir(&second);
        let _ = fs::remove_file(&second);
        fs::write(&first, b"original").unwrap();
        let result = publish_with(
            vec![(first.clone(), b"changed".to_vec()), (second.clone(), b"second".to_vec())],
            true,
            |index, dest| {
                if index == 1 {
                    fs::create_dir(dest).unwrap();
                }
            },
        );
        assert_eq!(result.unwrap_err().code, "AGENT_IO");
        assert_eq!(fs::read(first).unwrap(), b"original");
        assert!(second.is_dir(), "不能删除发布过程中出现的外部目录");
        fs::remove_dir(second).unwrap();
    }
}
