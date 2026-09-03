/** alias 目标：`../src/index` → 包装 saveDocx 以录制 SaveBlock[] 与输出，其余导出原样转发。 */
import * as real from '../src/index.ts'
import { recordSave } from './record'

export * from '../src/index.ts'

type SaveDocx = typeof real.saveDocx
export const saveDocx: SaveDocx = async (parsed, blocks, options) => {
  const out = await real.saveDocx(parsed, blocks, options)
  await recordSave(parsed as { internal?: { originalBytes?: Uint8Array } }, blocks, options, out)
  return out
}
