# Pasted chart observations

Visual observations were supplied by the primary task from the live Word UI. Package validation used byte-identical copies only; the three originals retained their SHA256 values.

| 文件 | Word 版本（build）/ 平台 | 制作方式（UI / 脚本名） | 步骤要点 | 看到什么 |
| --- | --- | --- | --- | --- |
| `chart/chart-pasted-embedded.docx` | 16.0.14334.20848 / Windows 11 x64（Office LTSC 2021，非 Microsoft 365） | prepare-excel-paste.ps1 创建真实 Excel 图表；Word UI 粘贴；word-ui-case.ps1 保存 | 从 Excel 复制图表，Word 粘贴选“使用目标主题和嵌入工作簿”；保存一次并导出 PDF | 主任务现场看到簇状柱形图，标题“销售统计”，Category 1/2/3 三类别；蓝色 Series 1 为 10/20/30，橙色 Series 2 为 15/25/35，图例在右，嵌入型，before/after 正常。副本结构检查通过：原生 chart1.xml 与 c:chart 引用、内部 externalData/XLSX 关系；ChartData 工作表 A1:C4 与两条系列缓存一致。 |
| `chart/chart-pasted-linked.docx` | 16.0.14334.20848 / Windows 11 x64（Office LTSC 2021，非 Microsoft 365） | prepare-excel-paste.ps1 创建真实 Excel 图表；Word UI 粘贴；word-ui-case.ps1 保存 | 从 Excel 复制图表，Word 粘贴选“使用目标主题和链接数据”；保存一次并导出 PDF | 主任务现场看到簇状柱形图，标题“销售统计”，Category 1/2/3 三类别；蓝色 Series 1 为 10/20/30，橙色 Series 2 为 15/25/35，图例在右，嵌入型，before/after 正常。副本结构检查通过：原生 chart1.xml，两条系列缓存正确；externalData 关系类型为 oleObject、TargetMode=External，目标为 file:///C:\code\rsWordParser\real-word-corpus-20260906\_assets\chart-paste-source.xlsx，核查时本机源文件存在。外链记录保留了实际绝对路径。 |
| `chart/chart-pasted-picture.docx` | 16.0.14334.20848 / Windows 11 x64（Office LTSC 2021，非 Microsoft 365） | prepare-excel-paste.ps1 创建真实 Excel 图表；Word UI 粘贴；word-ui-case.ps1 保存 | 从 Excel 复制图表，Word 粘贴选“图片”；保存一次并导出 PDF | 主任务现场看到同样的标题“销售统计”、Category 1/2/3、蓝色 10/20/30 与橙色 15/25/35 柱形及右侧图例，图片为嵌入型，before/after 正常。副本结构检查通过：普通图片块，内部 word/media/image1.png（12461 字节），PNG 签名有效；没有原生图表部件或 c:chart 引用。 |
