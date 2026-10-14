const notes: Record<string, string> = {
  "Extended-format input is decoded to sRGB; camera metadata is not retained.":
    "此格式按 sRGB 读取，暂不保留拍摄元数据。",
  "This output codec omits camera metadata; pixels use sRGB.":
    "此输出编码器暂不保留拍摄元数据，图片使用 sRGB 色彩。",
  "GIF is limited to 256 colors and one-bit transparency.":
    "GIF 最多支持 256 色，透明区域只能完全透明或完全不透明。",
  "PNG uses lossless encoding; quality presets do not quantize colors.":
    "PNG 使用无损编码，画质档位不会减少颜色数量。",
  "Transparency is composited onto the selected background.":
    "透明区域已合成到所选背景色。",
  "The embedded RGB ICC profile was converted to sRGB.":
    "内嵌 RGB ICC 色彩配置已转换为 sRGB。",
  "XMP was omitted after rotation/resizing to avoid stale dimensions and orientation.":
    "旋转或缩放后已移除 XMP，避免保留过期的尺寸和方向信息。",
  "Unmapped JPEG application/comment metadata was omitted during pixel re-encoding.":
    "重新编码时已移除无法映射的 JPEG 应用或注释元数据。",
  "PNG text and physical-resolution chunks cannot be carried into this output format.":
    "此输出格式未保留 PNG 文本和物理分辨率数据块。",
};
export function processingNote(value: string, english: boolean) {
  return english ? value : notes[value] || value;
}
