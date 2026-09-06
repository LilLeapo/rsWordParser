import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { defineConfig } from 'vitest/config'

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
    // 顺序执行：<文件>__<序号> 编号稳定，去重索引无竞争
    fileParallelism: false,
    reporters: ['dot'],
  },
  resolve: {
    alias: [
      { find: /^\.\/helpers\/build-docx$/, replacement: resolve(here, 'build-docx.wrapper.ts') },
      { find: /^\.\.\/src\/index$/, replacement: resolve(here, 'src-index.wrapper.ts') },
    ],
  },
})
