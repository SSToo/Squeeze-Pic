import { useEffect, useState, type CSSProperties } from "react";
import {
  Columns2,
  ScanLine,
  ZoomIn,
  ZoomOut,
  LoaderCircle,
  Image as ImageIcon,
  X,
} from "lucide-react";
import { command, native } from "./bridge";
import type { Job } from "./types";

export default function Preview({
  job,
  busy,
  en,
  onClose,
}: {
  job: Job | undefined;
  busy: boolean;
  en: boolean;
  onClose: () => void;
}) {
  const t = (zh: string, english: string) => (en ? english : zh);
  const [images, setImages] = useState<{
    before: string;
    after: string | null;
  } | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [split, setSplit] = useState(50);
  const [zoom, setZoom] = useState(1);
  const [sideBySide, setSideBySide] = useState(false);
  const path = job?.path;
  const output = job?.report?.output;
  useEffect(() => {
    let active = true;
    const urls: string[] = [];
    setImages(null);
    setError("");
    setZoom(1);
    if (!native || !path || !job?.info || busy) {
      setLoading(false);
      return;
    }
    setLoading(true);
    async function load() {
      const read = async (file: string) => {
        if (!active) return null;
        const data = await command<ArrayBuffer>(
          "read_preview",
          { path: file },
          () => active,
        );
        if (!active) return null;
        const url = URL.createObjectURL(
          new Blob([data], { type: "image/png" }),
        );
        urls.push(url);
        return url;
      };
      const before = await read(path!);
      const after = output ? await read(output) : null;
      if (active && before) setImages({ before, after });
    }
    load()
      .catch((e) => {
        if (active) setError(String(e));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
      urls.forEach(URL.revokeObjectURL);
    };
  }, [path, output, busy, !!job?.info]);
  const style = { "--split": `${split}%`, "--zoom": zoom } as CSSProperties;
  return (
    <section
      className="preview-panel"
      aria-label={t("图片对比预览", "Image comparison preview")}
    >
      <div className="section-bar">
        <div className="section-title">
          <ScanLine size={17} />
          <h2 id="preview-title">{t("细节对比", "Compare details")}</h2>
        </div>
        <div className="preview-actions">
          <button
            className="icon-button"
            aria-label={t("并排对比", "Side by side")}
            aria-pressed={sideBySide}
            onClick={() => setSideBySide(!sideBySide)}
          >
            <Columns2 size={17} />
          </button>
          <span className="divider" />
          <button
            className="icon-button"
            aria-label={t("缩小预览", "Zoom out")}
            disabled={zoom <= 1}
            onClick={() => setZoom((z) => Math.max(1, z - 0.25))}
          >
            <ZoomOut size={16} />
          </button>
          <span className="zoom-label">{zoom.toFixed(2)}×</span>
          <button
            className="icon-button"
            aria-label={t("放大预览", "Zoom in")}
            disabled={zoom >= 3}
            onClick={() => setZoom((z) => Math.min(3, z + 0.25))}
          >
            <ZoomIn size={16} />
          </button>
          <span className="divider" />
          <button
            className="icon-button"
            aria-label={t("关闭预览", "Close preview")}
            autoFocus
            onClick={onClose}
          >
            <X size={19} />
          </button>
        </div>
      </div>
      <div className="preview-filename">{job?.name}</div>
      <div
        className={`preview-stage checker ${sideBySide && images?.after ? "side-by-side" : ""}`}
        style={style}
      >
        {loading ? (
          <div className="preview-placeholder">
            <LoaderCircle className="spin" size={25} />
            <span>{t("正在生成预览…", "Preparing preview…")}</span>
          </div>
        ) : images ? (
          <>
            <div className="image-plane">
              <img
                src={images.before}
                alt={t("原图预览", "Original preview")}
              />
              <span className="image-badge">{t("原图", "Original")}</span>
            </div>
            {images.after && (
              <div
                className={`image-plane result-plane ${sideBySide ? "" : "split-plane"}`}
              >
                <img
                  src={images.after}
                  alt={t("处理结果预览", "Result preview")}
                />
                <span className="image-badge result-badge">
                  {t("处理后", "Result")}
                </span>
              </div>
            )}
            {images.after && !sideBySide && (
              <>
                <div className="split-line">
                  <span>‹ ›</span>
                </div>
                <input
                  className="comparison-range"
                  type="range"
                  min="0"
                  max="100"
                  value={split}
                  aria-label={t("前后对比分界位置", "Comparison divider")}
                  onChange={(e) => setSplit(Number(e.target.value))}
                />
              </>
            )}
          </>
        ) : (
          <div className="preview-placeholder">
            <ImageIcon size={28} strokeWidth={1.4} />
            <span>
              {error ||
                (busy
                  ? t("处理完成后可预览", "Preview available after processing")
                  : t(
                      "选择图片，查看原图和处理结果",
                      "Select an image to compare the original and result",
                    ))}
            </span>
          </div>
        )}
      </div>
      <div className="preview-caption">
        <span>
          {job?.info
            ? `${job.info.display_width} × ${job.info.display_height} px`
            : t("保留细节，轻装出发", "Less weight. More detail.")}
          {job?.report
            ? ` → ${job.report.after_dimensions.join(" × ")} px`
            : ""}
        </span>
        <span>{t("预览最长边 1600 px", "Preview up to 1600 px")}</span>
      </div>
    </section>
  );
}
