/** alias 目标：`./helpers/build-docx` → 录制三个构造函数，其余导出原样转发。 */
import * as real from '../tests/helpers/build-docx'
import { record } from './record'

export type { BuildDocxOptions } from '../tests/helpers/build-docx'
export const {
  TINY_PNG_BASE64,
  IMAGE_PARAGRAPH_XML,
  MATH_PARAGRAPH_XML,
  CHART_PARAGRAPH_XML,
  CHART_PART_XML,
  CHART_RELS,
  TABLE_XML,
  REVISION_TABLE_XML,
  NESTED_TABLE_XML,
  kitchenSinkBody,
} = real

export async function buildDocx(options: real.BuildDocxOptions): Promise<Uint8Array> {
  const bytes = await real.buildDocx(options)
  await record(bytes, 'buildDocx')
  return bytes
}

export async function buildKitchenSinkDocx(): Promise<Uint8Array> {
  const bytes = await real.buildKitchenSinkDocx()
  await record(bytes, 'buildKitchenSinkDocx')
  return bytes
}

export async function buildChartDocx(bodyPrefixXml = ''): Promise<Uint8Array> {
  const bytes = await real.buildChartDocx(bodyPrefixXml)
  await record(bytes, 'buildChartDocx')
  return bytes
}
