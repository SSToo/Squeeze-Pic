export type Format =
  | "jpeg"
  | "png"
  | "webp"
  | "avif"
  | "jxl"
  | "heif"
  | "gif"
  | "tiff"
  | "bmp"
  | "ico"
  | "icns"
  | "svg";
export type Preset = "lossless" | "high" | "balanced" | "small" | "custom";
export type Strategy = "fast" | "smart" | "deep";
export type ResizeMode =
  "original" | "percent" | "longest" | "width" | "height" | "fit";
export interface Options {
  format: Format | null;
  preset: Preset;
  strategy: Strategy;
  quality: number;
  resize: {
    mode: ResizeMode;
    value: number;
    width: number;
    height: number;
    allow_upscale: boolean;
  };
  metadata: "preserve" | "strip";
  background: [number, number, number];
}
export interface ImageInfo {
  format: Format;
  bytes: number;
  width: number;
  height: number;
  display_width: number;
  display_height: number;
  has_alpha: boolean;
  orientation: number;
  has_icc: boolean;
  has_exif: boolean;
}
export interface Report {
  outcome: "saved" | "unchanged";
  output: string | null;
  original_bytes: number;
  result_bytes: number;
  reduction_percent: number;
  input_format: Format;
  output_format: Format;
  before_dimensions: [number, number];
  after_dimensions: [number, number];
  elapsed_ms: number;
  encoding: string;
  candidates: number;
  quality: number | null;
  warnings: string[];
}
export interface Imported {
  path: string;
  name: string;
  info: ImageInfo | null;
  thumbnail: string | null;
  error: string | null;
}
export interface Job extends Imported {
  status:
    | "queued"
    | "processing"
    | "saved"
    | "unchanged"
    | "failed"
    | "cancelled"
    | "unsupported";
  report?: Report;
  phase?: string;
}
export const defaults: Options = {
  format: null,
  preset: "balanced",
  strategy: "smart",
  quality: 82,
  resize: {
    mode: "original",
    value: 1920,
    width: 1920,
    height: 1080,
    allow_upscale: false,
  },
  metadata: "preserve",
  background: [255, 255, 255],
};
