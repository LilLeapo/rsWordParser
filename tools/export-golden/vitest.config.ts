import { basename, dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { defineConfig } from 'vitest/config'
import { BaseSequencer, type TestSpecification } from 'vitest/node'
import { FILE_ORDER } from './file-order'

/**
 * 录制器按字节哈希去重、按"测试文件 + 调用序"编号：谁先跑到同一份文档谁拿到 stem，`.save.<k>` 的 k 也跟文件顺序走。
 * vitest 缺省按上次耗时缓存排序，两次导出顺序会变，stem 与 k 跟着漂（resource-cleanup__001 曾整份"消失"）。
 * 这里按 file-order.ts（首次导出时的实际顺序）排，新文件排最后——既有 stem 因此与 main 上的语料逐字节一致。
 */
class StableSequencer extends BaseSequencer {
  async shard(files: TestSpecification[]): Promise<TestSpecification[]> {
    return files
  }
  async sort(files: TestSpecification[]): Promise<TestSpecification[]> {
    // 与 record.ts 的 currentTest().file 同一种键：文件名去掉 .test.ts
    const key = (f: TestSpecification) => basename(f.moduleId).replace(/\.test\.ts$/, '')
    const rank = (f: TestSpecification) => {
      const i = FILE_ORDER.indexOf(key(f))
      return i === -1 ? FILE_ORDER.length : i
    }
    return [...files].sort((a, b) => rank(a) - rank(b) || a.moduleId.localeCompare(b.moduleId))
  }
}

// 本文件由 run.sh 复制到 <docx-engine>/export-golden.tmp/ 后执行。
const here = dirname(fileURLToPath(import.meta.url))
const engineRoot = resolve(here, '..')

export default defineConfig({
  root: engineRoot,
  test: {
    // export-golden.tmp*/：run.sh 用固定目录，try.sh 每次一个带 pid 后缀的目录；本目录里所有 *.export.test.ts 都算
    include: ['tests/**/*.test.ts', 'export-golden.tmp*/*.export.test.ts'],
    testTimeout: 120_000,
    hookTimeout: 120_000,
    // 顺序执行 + 固定文件顺序：<文件>__<序号> 编号稳定，去重索引无竞争
    fileParallelism: false,
    sequence: { sequencer: StableSequencer },
    reporters: ['dot'],
  },
  resolve: {
    alias: [
      { find: /^\.\/helpers\/build-docx$/, replacement: resolve(here, 'build-docx.wrapper.ts') },
      { find: /^\.\.\/src\/index$/, replacement: resolve(here, 'src-index.wrapper.ts') },
    ],
  },
})
